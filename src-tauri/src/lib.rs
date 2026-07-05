//! TMP Companion — Tauri backend entry point.
//!
//! The app drives a USB-connected Fender Tone Master Pro in re-amp mode to
//! auto-level presets to a LUFS target via a closed loop:
//!   load preset → play sample → capture processed output → measure LUFS →
//!   adjust `presetLevel` → repeat until on target → save.
//!
//! Module layout (filled in milestone by milestone — see the plan):
//!   hid       — IOKit exclusive-seize HID transport (runloop thread)         [M1]
//!   proto     — hand-rolled FenderMessageTMS encode/decode                   [M1]
//!   session   — handshake + preset list / load / level / save / re-amp       [M1]
//!   audio     — cpal re-amp playback + capture, window alignment             [M2]
//!   lufs      — ebur128 loudness measurement                                 [M2]
//!   leveller  — the closed loop, emits progress events                       [M3]

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::State;

// Several builders/methods are exercised only from M2/M3 onward; silence
// dead-code noise until then without weakening warnings elsewhere.
#[allow(dead_code)]
mod audio;
mod audiograph;
mod audition;
mod backup;
mod backup_read;
mod blockcaps;
mod blocklib;
mod bulk_cmd;
mod bulkrun;
mod device_gate;
#[cfg(target_os = "macos")]
mod dock;
mod footswitch;
#[allow(dead_code)]
mod hid;
mod ir;
#[allow(dead_code)]
mod leveller;
mod library;
mod lint;
#[allow(dead_code)]
mod lufs;
mod migration;
mod monitor;
mod paramedit;
mod preset_io;
mod presetmeta;
mod probe_api;
mod profiles;
#[allow(dead_code)]
mod proto;
mod rename;
mod replace_inplace;
mod saved_blocks;
mod scenes;
mod search;
#[allow(dead_code)]
mod session;
#[cfg(any(test, feature = "e2e"))]
mod sim_device;
mod spectrum;
// `pub` so the `gen_samples` bin (a separate crate) can reach the shared
// catalog as `tmp_companion_lib::topologies`.
pub mod topologies;
mod variants;
mod watcher;

pub use backup_read::*;
pub(crate) use device_gate::*;
// The `probe_*` entry points (reachable as `<libcrate>::probe_xxx` for `bin/probe.rs`).
pub use probe_api::*;
// Interim seam: helpers that stayed-in-lib commands still call after the probe_api
// extraction (Phase 2). Explicit list documents the boundary until a later phase.
pub(crate) use probe_api::level::filter_amp_candidates;
pub(crate) use probe_api::scene_bench::knob_bounds;
pub(crate) use probe_api::scene_jobs::{build_scene_jobs, is_amp_output_level_param, prepass_scene_docs};
pub(crate) use probe_api::setlists::{read_setlist_list, read_setlist_songs};
pub(crate) use probe_api::slot_write::{discover_active_graph, load_then_discover_blocks};
pub(crate) use probe_api::songs::{converge_song_bpm, read_song_list, read_song_presets};
pub(crate) use probe_api::stimulus::{
    read_stimulus_calibrated, read_stimulus_calibrated_with_shortfall,
};
pub use replace_inplace::*;
pub use saved_blocks::*;

pub use session::PresetEntry;
use session::Session;
pub use session::{ActiveGraph, GraphNode, Stage};


/// Read the full preset/scene library via the device backup (one `BackupRequest` →
/// tar.lz4 stream → in-memory decode). Emits `tmp://backup-progress`
/// ([`session::BackupProgress`]) as the transfer advances so the UI can drive a
/// determinate progress bar (the chunk percentage is exact). Read-only on the
/// device; nothing persists (archive in RAM, temp DB deleted). Routed through
/// `with_released_seize` so it serializes via `DEVICE_OP_LOCK` (pausing the monitor)
/// like every device op.
#[tauri::command]
async fn read_library_via_backup<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<BackupReadResult, String> {
    // Offline e2e: decode a built fixture blob (LZ4-frame(tar(normalDb.db3)), the exact
    // device shape) through the SAME `read_backup_archive` path instead of streaming the
    // bulk backup over USB — faking that multi-chunk wire stream buys no fidelity the
    // real decode (lz4 → tar → sqlite → audiograph) doesn't already exercise.
    #[cfg(feature = "e2e")]
    if let Ok(path) = std::env::var("TMP_E2E_BACKUP_FIXTURE") {
        let blob = std::fs::read(&path).map_err(|e| format!("e2e backup fixture {path}: {e}"))?;
        return read_backup_archive(&blob);
    }
    use tauri::Emitter;
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?;
        let (blob, _stats) = s.device_backup(60, move |p| {
            let _ = app.emit("tmp://backup-progress", p);
        })?;
        drop(s); // release the HID seize before host-side decode
        read_backup_archive(&blob)
    })
    .await
}

/// List the user's saved blocks (`RequestAllBlockPresets` → `AllBlockPresetsResponse`).
/// Instant (rides one handshake burst, no 22 s backup). Read-only. Powers the Bulk
/// Block Edit Step-3 "Your saved blocks" palette (incl. saved dual-cabs).
///
/// The in-burst `135` read can transiently MISS on a cold/first connect (the device
/// doesn't answer that round — HW-observed: 1st cold read returned no `136`,
/// the next two succeeded). So retry independent fresh reads until the response lands
/// (mirrors [`read_song_list`]'s fail-closed retry) rather than spuriously surfacing an
/// empty saved-block palette. Each attempt early-exits the moment the `136` arrives.
#[tauri::command]
async fn list_saved_blocks(state: State<'_, AppState>) -> Result<Vec<SavedBlock>, String> {
    with_released_seize(state.session.clone(), move || {
        for _attempt in 0..4 {
            let mut s =
                Session::connect_with_burst_request(&proto::request_all_block_presets(Some(2)))?;
            for _ in 0..8 {
                if let Some(blob) = find_block_presets_blob(&s.push_bodies()) {
                    return parse_block_presets_map(&blob);
                }
                s.pump_collect(250)?;
            }
        }
        Err("device sent no allBlockPresetsResponse after retries".to_string())
    })
    .await
}

/// List the user's impulse responses (`UserIRListRequest` → `UserIRListResponse`).
/// Instant + read-only. Powers the Bulk Block Edit Step-3 "Your impulse responses"
/// palette. Returns an empty list when the device has no user IRs loaded.
#[tauri::command]
async fn list_user_irs(state: State<'_, AppState>) -> Result<Vec<UserIr>, String> {
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?; // handshake already issues userIRListRequest(batch 2)
                                         // A standalone re-send + a few pump windows in case the burst reply was missed.
        s.heartbeat()?;
        s.pump_collect(80)?;
        s.send_and_collect(&proto::userir_field2(2), 500)?;
        for _ in 0..5 {
            s.heartbeat()?;
            s.pump_collect(200)?;
        }
        let bodies = s.push_bodies();
        drop(s);
        Ok(find_user_irs(&bodies))
    })
    .await
}

/// The per-node edit Bulk Block Edit applies on the held session (matches the TS union
/// on the `kind` tag; nested keys arrive camelCase, so `fenderId` is renamed).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReplArg {
    /// Stock model — `replaceNode` fills the model's default params.
    Model {
        #[serde(rename = "fenderId")]
        fender_id: String,
    },
    /// User IR — `replaceNode` to `ACD_UserIRTMS`, then a string `changeParameter` sets
    /// the new node's `file` param to the chosen IR (verified by re-read before save).
    Ir {
        #[serde(rename = "fenderId")]
        fender_id: String,
        file: String,
    },
    /// Saved block (user block / dual cab) — `replaceNodeWithBlock` by the device
    /// library `index`.
    Saved {
        #[serde(rename = "fenderId")]
        fender_id: String,
        index: u64,
    },
    /// Remove the block from the chain — `removeNode` (the device re-links).
    Remove,
}

/// One preset's outcome from a live bulk-replace run (streamed to the UI per preset).
#[derive(Debug, Clone, Serialize)]
pub struct BulkReplaceItem {
    /// 0-based list index of the preset (the UI keys its rows by this).
    pub slot: u32,
    pub name: String,
    /// `"updated"` | `"skipped"` | `"error"`.
    pub outcome: String,
    pub detail: String,
}

/// The block content a copy [`CopyOp`] applies — the SAME three "with a block"
/// variants [`ReplArg`] supports (no `Remove`; that is a [`CopyOp::Remove`] op).
/// Nested keys arrive camelCase, so `fenderId` is renamed.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CopyRepl {
    /// Stock model — `replaceNode` / `insertNode` fills the model's default params.
    Model {
        #[serde(rename = "fenderId")]
        fender_id: String,
    },
    /// User IR — `replaceNode`/`insert` to `ACD_UserIRTMS`, then a string
    /// `changeParameter` points the new node's `file` param at the chosen IR.
    Ir {
        #[serde(rename = "fenderId")]
        fender_id: String,
        file: String,
    },
    /// Saved block (user block / dual cab) — `replaceNodeWithBlock` by the device
    /// library `index`.
    Saved {
        #[serde(rename = "fenderId")]
        fender_id: String,
        index: u64,
    },
}

impl CopyRepl {
    /// The fender id this content resolves to (the model id, the IR placeholder
    /// `ACD_UserIRTMS`, or the saved block's id).
    fn insert_fender_id(&self) -> &str {
        match self {
            CopyRepl::Model { fender_id } => fender_id,
            CopyRepl::Ir { .. } => "ACD_UserIRTMS",
            CopyRepl::Saved { fender_id, .. } => fender_id,
        }
    }
}

/// One ordered structural op the "Copy blocks between presets" feature applies to a
/// target preset. Tagged on `kind`; nested ids arrive camelCase.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CopyOp {
    /// Replace the block `node_id` in `group` with `repl` — `replaceNode` /
    /// `replaceNodeWithBlock` / `replaceNode`→`ACD_UserIRTMS`+file, per the variant.
    Replace {
        group: String,
        #[serde(rename = "nodeId")]
        node_id: String,
        repl: CopyRepl,
    },
    /// Insert `repl` into `group` via field-34 `insert_node`. `before_fender_id` is the
    /// block to insert AHEAD of (the device's field-2 inserts BEFORE the referenced node,
    /// HW-verified fw 1.8.45); `None` appends at the group end. `diffToOps` sets it to the
    /// inserted block's in-array successor's FenderId, or `None` when it's last.
    Insert {
        group: String,
        #[serde(rename = "beforeFenderId")]
        before_fender_id: Option<String>,
        repl: CopyRepl,
    },
    /// Remove the block `node_id` from `group` — `removeNode` (the device re-links).
    Remove {
        group: String,
        #[serde(rename = "nodeId")]
        node_id: String,
    },
}

/// One target preset for a [`copy_apply`] run: its 0-based `list_index`, display
/// `name` (for the identity-preserving rename-before-save), and the ORDERED list of
/// structural `ops` to apply before saving it.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CopyJob {
    #[serde(rename = "listIndex")]
    pub list_index: u32,
    pub name: String,
    pub ops: Vec<CopyOp>,
}

/// One preset's outcome from a [`copy_apply`] run (streamed per preset). Like
/// [`BulkReplaceItem`] (`slot`/`name`/`outcome`/`detail`) plus the post-save signal
/// `graph` read back off the held session, so the Copy view can patch its cached library
/// in place (no ~22 s re-scan) after a write. `graph` is `None` when the preset wasn't
/// saved or its graph couldn't be read back.
#[derive(Debug, Clone, Serialize)]
pub struct CopyApplyItem {
    pub slot: u32,
    pub name: String,
    pub outcome: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<session::ActiveGraph>,
}

/// Cooperative cancel for [`bulk_replace_live`] — set by `cancel_bulk_replace` (the
/// wizard's Stop), checked between presets so the held-session sweep stops WRITING the
/// remaining presets (not just hiding them in the UI). Presets already saved stay
/// changed; the rest are left untouched.
static BULK_REPLACE_CANCEL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Stop an in-flight [`bulk_replace_live`] sweep after the current preset. Lightweight
/// (just sets the flag) so it does NOT take the device-op lock — it must run while the
/// sweep holds it.
#[tauri::command]
fn cancel_bulk_replace() {
    BULK_REPLACE_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Bulk Block Edit — apply one per-node edit (replace with a model/saved block, or
/// remove) across the selected presets, live, via the device's own structural edit
/// (`replaceNode` / `replaceNodeWithBlock` / `removeNode`). This is the faithful path:
/// the device fills the new block's correct params / re-links the chain on removal
/// (there is no host-side default-param catalog — see the feature's design).
/// DEVICE WRITE — gated behind the UI's backup acknowledgment. Streams a
/// `BulkReplaceItem` per preset; one preset's failure degrades to an `error` row and
/// the sweep continues. Stop (`cancel_bulk_replace`) halts before the next preset. Each
/// preset is loaded, edited in place (identity preserved), and saved only when `save`.
#[tauri::command]
async fn bulk_replace_live(
    state: State<'_, AppState>,
    selection: Vec<u32>,
    from_id: String,
    repl: ReplArg,
    save: bool,
    on_result: tauri::ipc::Channel<BulkReplaceItem>,
) -> Result<Vec<BulkReplaceItem>, String> {
    BULK_REPLACE_CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
    with_released_seize(state.session.clone(), move || {
        // Discovery: per-slot field-8 reads for small selections, ONE whole-library
        // device backup once the selection crosses the measured break-even (E5).
        let device_slots: Vec<u32> = selection.iter().map(|i| i + 1).collect();
        let plans = discover_replace_plans_smart(&device_slots, &from_id)?;
        // Edit on ONE HELD session (E1): connect once, warm the live-controller
        // heartbeat once, then edit every preset in place with no reopens and no
        // inter-preset settles (~3.4 s/preset vs ~8 s for the two-connection path).
        // If the held session can't be ESTABLISHED (connect/warmup), fall back to the
        // proven per-preset two-connection loop BEFORE any result is emitted (so no row
        // is double-sent). A per-preset failure inside the held path is an `error` row,
        // not a fallback trigger — the session stays alive for the remaining presets.
        match replace_many_held(&plans, &repl, save, |item| {
            let _ = on_result.send(item.clone());
        }) {
            Ok(out) => Ok(out),
            Err(e) => {
                log::warn!(
                    "[bulk-replace] held-session path failed to establish ({e}); falling back to two-connection"
                );
                std::thread::sleep(std::time::Duration::from_millis(400));
                let mut out = Vec::new();
                let total = plans.len();
                for (i, plan) in plans.iter().enumerate() {
                    if BULK_REPLACE_CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
                        break; // Stop pressed — leave the remaining presets untouched.
                    }
                    let item = replace_one_live(plan, &repl, save).unwrap_or_else(|e| BulkReplaceItem {
                        slot: plan.list_index,
                        name: plan.name.clone(),
                        outcome: "error".to_string(),
                        detail: e,
                    });
                    let _ = on_result.send(item.clone());
                    out.push(item);
                    if i + 1 < total {
                        std::thread::sleep(std::time::Duration::from_millis(400));
                    }
                }
                Ok(out)
            }
        }
    })
    .await
}

/// One preset's matching target nodes, discovered up front (field-8 read) so the
/// edit hot path never does a fragile post-reconnect read.
struct ReplacePlan {
    list_index: u32,
    name: String,
    /// `(group, node_id)` of every node matching the requested `from_id`.
    targets: Vec<(String, String)>,
}

/// Read the selected slots ONCE on a single session and build the per-preset target
/// list. Field-8 reads are reliable on a clean session but flaky right after a
/// reconnect, so all reads are batched here, away from the edit reconnect churn.
pub(crate) fn discover_replace_plans(device_slots: &[u32], from_id: &str) -> Result<Vec<ReplacePlan>, String> {
    let mut s = Session::connect()?;
    s.drain_until_quiet(250, 20)?;
    let mut plans = Vec::new();
    for &dev_slot in device_slots {
        let raw = s
            .read_slot_preset_json(dev_slot)?
            .ok_or_else(|| format!("slot {dev_slot}: field-8 read returned no JSON"))?;
        let value = session::tolerant_parse_json(&String::from_utf8_lossy(&raw))
            .ok_or_else(|| format!("slot {dev_slot}: preset JSON did not parse"))?;
        let name = value
            .pointer("/info/displayName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let targets: Vec<(String, String)> = audiograph::roster(&value)
            .into_iter()
            .filter(|(_, _, fid)| fid == from_id)
            .map(|(group, node_id, _)| (group, node_id))
            .collect();
        plans.push(ReplacePlan {
            list_index: dev_slot.saturating_sub(1),
            name,
            targets,
        });
    }
    Ok(plans)
}

/// Selection size at which whole-library backup discovery beats per-slot field-8 reads
/// (E5, HW-measured: field-8 ≈ 1.2 s/slot, backup ≈ 24 s flat → break-even
/// ~20 slots). At/above this, the backup is also strictly more correct — it reads the
/// COMPLETE preset JSON, so it can't miss a block past the field-8 truncation point.
const BACKUP_DISCOVERY_THRESHOLD: usize = 22;

/// Build replace plans, choosing the discovery source by selection size (E5).
fn discover_replace_plans_smart(
    device_slots: &[u32],
    from_id: &str,
) -> Result<Vec<ReplacePlan>, String> {
    if device_slots.len() < BACKUP_DISCOVERY_THRESHOLD {
        discover_replace_plans(device_slots, from_id)
    } else {
        discover_replace_plans_via_backup(device_slots, from_id)
    }
}

/// Build replace plans from ONE whole-library device backup (E5) — the complete preset
/// JSON, so no field-8 truncation can hide a target block. Slots not present in the
/// backup (empty) get an empty plan (skipped downstream).
fn discover_replace_plans_via_backup(
    device_slots: &[u32],
    from_id: &str,
) -> Result<Vec<ReplacePlan>, String> {
    let mut s = Session::connect()?;
    let (blob, _stats) = s.device_backup(60, |_p| {})?;
    drop(s);
    let result = read_backup_archive(&blob)?;
    let by_slot: std::collections::HashMap<u32, &BackupPresetRow> = result
        .presets
        .iter()
        .filter(|p| p.slot > 0)
        .map(|p| (p.slot as u32, p))
        .collect();
    let plans = device_slots
        .iter()
        .map(|&dev_slot| {
            let (name, targets) = match by_slot.get(&dev_slot) {
                Some(row) => {
                    let targets = row
                        .blocks
                        .iter()
                        .filter(|b| b.fender_id == from_id)
                        .map(|b| (b.group_id.clone(), b.node_id.clone()))
                        .collect();
                    (row.name.clone(), targets)
                }
                None => (String::new(), Vec::new()),
            };
            ReplacePlan {
                list_index: dev_slot.saturating_sub(1),
                name,
                targets,
            }
        })
        .collect();
    Ok(plans)
}

// ─── blockcaps guard plumbing ──────────────────────────────────────────────────
//
// Shared by every LIVE structural-edit writer (`replace_one_live`, `held_replace_one`,
// `copy_apply_one`): the device does NOT enforce the 5 firmware block-count caps (see
// `blockcaps.rs`'s module docs — two RE passes confirmed the cap logic is entirely
// client-side in `tone-master-stomp-client` and can never emit a `presetError` for an
// over-cap edit), so this Rust guard is the SOLE enforcement before a save. Every
// writer: (1) reads the PRE-edit roster right after load+attach-confirm (before its
// first structural edit), fail-closed if it can't be read; (2) checks each op's delta
// against the running counts BEFORE emitting it; (3) on a violation, aborts the WHOLE
// target with an `error` outcome carrying the reason — never a partial, unvalidated
// save.

/// Read the just-loaded preset's pre-edit node roster (+ its cap counts) off the held
/// session, retrying (mirrors [`ordered_group`]'s pattern — the post-load field-3 push
/// can lag a heartbeat window). Fail-closed: `Err` propagates as the caller's `error`
/// outcome rather than ever letting an edit through unvalidated.
fn blockcaps_pre_edit_roster(
    s: &mut Session,
) -> Result<(Vec<blockcaps::RosterEntry>, blockcaps::Counts), String> {
    for i in 0..10 {
        if i > 0 {
            let _ = s.heartbeat();
            let _ = s.pump_collect(250);
        }
        if let Ok(v) = s.current_preset_value() {
            let roster = blockcaps::roster_from_preset(&v);
            if !roster.is_empty() {
                let counts = blockcaps::counts(&roster);
                return Ok((roster, counts));
            }
        }
    }
    Err(
        "blockcaps: could not read the pre-edit node roster after load — refusing to \
         emit an unvalidated edit (the device does not enforce the block-count caps, \
         so an unreadable roster must not save)"
            .to_string(),
    )
}

/// What a Replace/Remove op's `(group, node_id)` target is replacing/removing, off the
/// pre-edit roster (an Insert has no target; it only adds). Linear scan — rosters are
/// tens of nodes; a node inserted by an EARLIER op in the same job resolves to `None`
/// (its contribution is simply never freed — the strict, safe direction).
fn blockcaps_replaced<'a>(
    roster: &'a [blockcaps::RosterEntry],
    group: &str,
    node_id: &str,
) -> Option<(&'a str, bool)> {
    roster
        .iter()
        .find(|e| e.group == group && e.node_id == node_id)
        .map(|e| (e.fender_id.as_str(), e.dual_cab))
}

/// Cap-check ONE candidate insert/replace against the running pre-edit `counts`.
/// `candidate_id = None` is a bare Remove — no candidate to check; removing a node can
/// only shrink counts, never push a cap over its max. `replaced` is the (fender_id,
/// dual_cab) of the node a Replace targets, from [`blockcaps_replaced`] (`None` for
/// an Insert/Remove). `cand_dual_cab` is always `false`: the wire format
/// (`CopyRepl`/`ReplArg`) never carries a candidate's dual-cab flag — a fresh Model/Ir
/// insert is never dual by construction; a `Saved` (device-library) block COULD be
/// dual, but that's only knowable by reading the library entry, which the copy/replace
/// wire protocol doesn't surface. Known residual gap (documented, not silently assumed).
fn blockcaps_check(
    counts: &blockcaps::Counts,
    candidate_id: Option<&str>,
    is_replace: bool,
    replaced: Option<(&str, bool)>,
) -> Result<(), String> {
    let Some(candidate_id) = candidate_id else {
        return Ok(());
    };
    let (replaced_id, replaced_dual) = replaced.map_or((None, false), |(id, d)| (Some(id), d));
    blockcaps::check_op(
        counts,
        candidate_id,
        replaced_id,
        is_replace,
        false,
        replaced_dual,
    )
    .map_err(|reason| reason.to_string())
}

/// Roll the running `counts` forward after a CONFIRMED op, so the NEXT op in the same
/// job/plan is checked against the up-to-date state (a job can carry multiple ops that
/// each affect the same caps — e.g. two conv inserts in one job must be checked
/// cumulatively, not each against the stale pre-edit snapshot).
fn blockcaps_advance(
    counts: &mut blockcaps::Counts,
    candidate_id: Option<&str>,
    replaced: Option<(&str, bool)>,
) {
    if let Some(id) = candidate_id {
        counts.add(id, false); // see `blockcaps_check`'s doc on `cand_dual_cab`.
    }
    if let Some((id, dual)) = replaced {
        counts.remove(id, dual);
    }
}

/// The FenderId a [`ReplArg`] would insert, or `None` for `Remove` (no candidate to
/// cap-check — see [`blockcaps_check`]).
fn repl_arg_fender_id(repl: &ReplArg) -> Option<&str> {
    match repl {
        ReplArg::Model { fender_id }
        | ReplArg::Ir { fender_id, .. }
        | ReplArg::Saved { fender_id, .. } => Some(fender_id.as_str()),
        ReplArg::Remove => None,
    }
}

/// Replace every pre-discovered target node in ONE preset and (if `save`) persist it.
/// Owns TWO connections — the proven fw-1.8.45 path: conn1 loads the preset (making it
/// the device's active/edit preset), then conn2's fresh handshake RE-ATTACHES to that
/// active preset before editing it. (Load + edit in one session leaves the session
/// attached to the pre-load preset, so the device rejects the edit.) No field-8 read
/// here — `plan.targets` were read up front by [`discover_replace_plans`].
///
/// SAFETY (a wrong-content save corrupted a slot): the save is gated on
/// (1) conn2's active preset matching the target name (the load took) AND (2) EVERY
/// `replaceNode` being confirmed by `nodeReplaced(40)` — a `presetError(53)`/no-ack
/// aborts WITHOUT saving, so a misattached or rejected edit can never persist the
/// wrong audioGraph. (3) blockcaps guard — see the module docs above: EVERY target's
/// delta against the 5 firmware block-count caps is checked against the PRE-edit
/// roster before it's emitted, since the device enforces none of them.
fn replace_one_live(
    plan: &ReplacePlan,
    repl: &ReplArg,
    save: bool,
) -> Result<BulkReplaceItem, String> {
    let list_index = plan.list_index;
    let name = plan.name.clone();
    if plan.targets.is_empty() {
        return Ok(BulkReplaceItem {
            slot: list_index,
            name,
            outcome: "skipped".to_string(),
            detail: "no matching block".to_string(),
        });
    }

    // ── conn1: LOAD the preset (make it active). Fire-and-forget; no read. ──
    {
        let mut s1 = Session::connect()?;
        s1.load_preset(list_index)?;
        s1.heartbeat()?;
        s1.pump_collect(500)?;
    }
    // Quiet settle before reconnecting — avoids the HID open-lockout/congestion a rapid
    // drop→reopen triggers.
    std::thread::sleep(std::time::Duration::from_millis(400));

    // ── conn2: fresh handshake re-attaches to the now-active preset; edit it ──
    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    // SAFETY 1 — confirm conn2 is on the TARGET preset (the load took). Prefer the
    // `PresetLoaded` slot echo (identity); fall back to the active-preset NAME. If
    // NEITHER confirms (empty/duplicate name + no slot echo), SKIP — editing+saving an
    // unverified preset would corrupt this slot.
    if !s.active_matches(list_index, Some(&name)) {
        return Ok(BulkReplaceItem {
            slot: list_index,
            name: name.clone(),
            outcome: "error".to_string(),
            detail: format!(
                "could not confirm target preset (slot {:?} ≠ {list_index}, active {:?} ≠ target {name:?}) — not edited",
                s.loaded_slot(),
                s.active_preset_name()
            ),
        });
    }
    // ── blockcaps guard — read the PRE-edit roster now, before the first structural
    //    edit (fail-closed: an unreadable roster refuses the WHOLE target). ──
    let (roster, mut counts) = blockcaps_pre_edit_roster(&mut s)?;
    let candidate_id = repl_arg_fender_id(repl);

    // SAFETY 2 — only persist if EVERY replace is confirmed (nodeReplaced/40).
    let mut applied = 0usize;
    for (group, node_id) in &plan.targets {
        let replaced = blockcaps_replaced(&roster, group, node_id);
        if let Err(reason) = blockcaps_check(&counts, candidate_id, true, replaced) {
            return Ok(BulkReplaceItem {
                slot: list_index,
                name: name.clone(),
                outcome: "error".to_string(),
                detail: format!(
                    "blocked by block-count cap ({reason}) replacing {group}/{node_id} — NOT saved"
                ),
            });
        }
        let confirmed = match repl {
            ReplArg::Saved { fender_id, index } => {
                s.replace_node_with_block(group, node_id, fender_id, *index)?
            }
            ReplArg::Model { fender_id } => s.replace_node(group, node_id, fender_id)?,
            ReplArg::Ir { file, .. } => s.replace_node_with_ir(group, node_id, file)?,
            ReplArg::Remove => s.remove_node(group, node_id)?,
        };
        if !confirmed {
            return Ok(BulkReplaceItem {
                slot: list_index,
                name: name.clone(),
                outcome: "error".to_string(),
                detail: format!("device rejected replace of {group}/{node_id} (presetError / no nodeReplaced) — NOT saved"),
            });
        }
        blockcaps_advance(&mut counts, candidate_id, replaced);
        applied += 1;
    }
    if save {
        // Pro Control persists a structural edit as renameCurrentPreset(current name)
        // → saveCurrentPreset(slot); the rename preserves the preset's name/identity.
        if !name.is_empty() {
            s.rename_current_preset(&name)?;
        }
        s.save_current_preset(list_index)?;
    }
    Ok(BulkReplaceItem {
        slot: list_index,
        name,
        outcome: "updated".to_string(),
        detail: format!("{applied} block(s)"),
    })
}

/// Edit ONE preset on a HELD session (the validated fast path, E1): load it, re-arm so
/// the session re-attaches to it, confirm attachment, replace every pre-discovered
/// target node, and (if `save`) persist — all WITHOUT reopening the connection. The
/// session's live-controller heartbeat (established once by `begin_live_edit`) must
/// already be warm; this keeps it warm for the next preset. The re-arm
/// (`connection_request` + `preset_list_request` + `current_preset_info_request`) is
/// what re-attaches the edit context to the just-loaded preset — my earlier
/// "load+edit in one session is rejected" finding was missing it (HW-proven:
/// 3/3 presets attached + confirmed + persisted on one connection, ~3.4 s/preset vs
/// ~8 s for the two-connection path). Echo-gated waits (E3) exit each settle window on
/// the device's echo. SAME safety gate as [`replace_one_live`]: a save only when the
/// active preset matches the target AND every replace returns `nodeReplaced` (40).
pub(crate) fn held_replace_one(
    s: &mut Session,
    plan: &ReplacePlan,
    repl: &ReplArg,
    save: bool,
) -> Result<BulkReplaceItem, String> {
    let list_index = plan.list_index;
    let name = plan.name.clone();
    if plan.targets.is_empty() {
        return Ok(BulkReplaceItem {
            slot: list_index,
            name,
            outcome: "skipped".to_string(),
            detail: "no matching block".to_string(),
        });
    }

    // ── LOAD on the held session (the bench-scene-leveling precedent: a held session
    //    accepts a load + decodes its field-3 push). ──
    s.clear_raw();
    s.send_and_collect(&proto::load_preset((list_index + 1) as u64, 1), 200)?;
    // ── RE-ARM: re-arm the device's reply state on the open connection + force a fresh
    //    currentPresetInfoChanged (field 22). Echo-gated: await_active_preset exits as
    //    soon as the field-22 for `name` lands. ──
    s.send_and_collect(&proto::connection_request(), 80)?;
    s.send_and_collect(&proto::preset_list_request(1, 1), 20)?;
    s.send_and_collect(&proto::current_preset_info_request(2), 120)?;
    let _ = s.await_active_preset(&name, 8); // pump for the fresh currentPresetInfoChanged
                                             // SAFETY 1 — confirm the held session re-attached to the TARGET preset before
                                             // editing+saving (a load that didn't take leaves a DIFFERENT preset active, and
                                             // saving it corrupts this slot — HW). active_matches prefers the `PresetLoaded` slot
                                             // echo (identity, immune to duplicate display names), falling back to the active
                                             // preset NAME only when no slot echo arrived; if NEITHER confirms, SKIP.
    if !s.active_matches(list_index, Some(&name)) {
        return Ok(BulkReplaceItem {
            slot: list_index,
            name: name.clone(),
            outcome: "error".to_string(),
            detail: format!(
                "could not confirm target preset loaded on held session (slot {:?} ≠ {list_index}, active {:?} ≠ target {name:?}) — not edited",
                s.loaded_slot(),
                s.active_preset_name()
            ),
        });
    }
    // ── blockcaps guard — read the PRE-edit roster now, before the first structural
    //    edit (fail-closed: an unreadable roster refuses the WHOLE target). ──
    let (roster, mut counts) = blockcaps_pre_edit_roster(s)?;
    let candidate_id = repl_arg_fender_id(repl);

    // SAFETY 2 — only persist if EVERY replace is confirmed (nodeReplaced/40).
    let mut applied = 0usize;
    for (group, node_id) in &plan.targets {
        let replaced = blockcaps_replaced(&roster, group, node_id);
        if let Err(reason) = blockcaps_check(&counts, candidate_id, true, replaced) {
            return Ok(BulkReplaceItem {
                slot: list_index,
                name: name.clone(),
                outcome: "error".to_string(),
                detail: format!(
                    "blocked by block-count cap ({reason}) replacing {group}/{node_id} — NOT saved"
                ),
            });
        }
        let confirmed = match repl {
            ReplArg::Saved { fender_id, index } => {
                s.replace_node_with_block(group, node_id, fender_id, *index)?
            }
            ReplArg::Model { fender_id } => s.replace_node(group, node_id, fender_id)?,
            ReplArg::Ir { file, .. } => s.replace_node_with_ir(group, node_id, file)?,
            ReplArg::Remove => s.remove_node(group, node_id)?,
        };
        if !confirmed {
            return Ok(BulkReplaceItem {
                slot: list_index,
                name: name.clone(),
                outcome: "error".to_string(),
                detail: format!("device rejected replace of {group}/{node_id} (presetError / no nodeReplaced) — NOT saved"),
            });
        }
        blockcaps_advance(&mut counts, candidate_id, replaced);
        applied += 1;
    }
    if save {
        if !name.is_empty() {
            s.rename_current_preset(&name)?;
        }
        s.save_current_preset(list_index)?;
    }
    // Keep live-controller status before the next preset (no long quiet gap).
    s.heartbeat()?;
    s.pump_collect(120)?;
    Ok(BulkReplaceItem {
        slot: list_index,
        name,
        outcome: "updated".to_string(),
        detail: format!("{applied} block(s)"),
    })
}

/// Run a bulk replace across `plans` on ONE held session (the E1 architecture):
/// connect once, warm the live-controller heartbeat once (`begin_live_edit`), then
/// `held_replace_one` each preset — no reopens, no inter-preset settles. A per-preset
/// failure degrades to an `error`/`skipped` row (the session stays alive); only a
/// failure to ESTABLISH the session (connect/warmup) propagates as `Err`, so the
/// caller can fall back to the two-connection path before any result is emitted.
/// `on_each` is called as each preset completes (streams to the UI channel).
pub(crate) fn replace_many_held(
    plans: &[ReplacePlan],
    repl: &ReplArg,
    save: bool,
    mut on_each: impl FnMut(&BulkReplaceItem),
) -> Result<Vec<BulkReplaceItem>, String> {
    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    let mut out = Vec::with_capacity(plans.len());
    for plan in plans {
        if BULK_REPLACE_CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
            break; // Stop pressed — leave the remaining presets untouched.
        }
        let item = held_replace_one(&mut s, plan, repl, save).unwrap_or_else(|e| BulkReplaceItem {
            slot: plan.list_index,
            name: plan.name.clone(),
            outcome: "error".to_string(),
            detail: e,
        });
        on_each(&item);
        out.push(item);
    }
    Ok(out)
}

/// Cooperative cancel for [`copy_apply`] — set by `cancel_copy_apply` (the Copy
/// wizard's Stop), checked between presets so the held-session run stops WRITING the
/// remaining presets. Presets already saved stay changed; the rest are untouched.
static COPY_APPLY_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Stop an in-flight [`copy_apply`] run after the current preset. Lightweight (just
/// sets the flag) so it does NOT take the device-op lock — it must run while the run
/// holds it.
#[tauri::command]
fn cancel_copy_apply() {
    COPY_APPLY_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// "Copy blocks between presets" — apply an ORDERED list of structural ops
/// (replace / insert / remove) to EACH target preset, live, then save that preset in
/// place (only when every op confirmed AND `save`). Mirrors [`bulk_replace_live`]'s
/// architecture exactly: ONE held re-armed session per preset (`copy_apply_one`), the
/// same DEVICE_OP_LOCK / monitor-pause bookend (`with_released_seize`), streamed
/// `CopyApplyItem`s, and a cooperative cancel (`cancel_copy_apply`). DEVICE WRITE —
/// gated behind the UI's backup acknowledgment. A per-preset failure degrades to an
/// `error`/`skipped`/`rejected` row and the run CONTINUES; an empty `ops` list →
/// `skipped`.
#[tauri::command]
async fn copy_apply(
    state: State<'_, AppState>,
    jobs: Vec<CopyJob>,
    save: bool,
    on_result: tauri::ipc::Channel<CopyApplyItem>,
) -> Result<Vec<CopyApplyItem>, String> {
    COPY_APPLY_CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
    with_released_seize(state.session.clone(), move || {
        // ONE held session for the whole run (the E1 architecture): connect once, warm
        // the live-controller heartbeat once (`begin_live_edit`), then `copy_apply_one`
        // each preset with no reopens. A per-preset failure stays an `error` row (the
        // session stays alive); only a failure to ESTABLISH the session propagates.
        let mut s = Session::connect()?;
        s.begin_live_edit()?;
        let mut out = Vec::with_capacity(jobs.len());
        for job in &jobs {
            if COPY_APPLY_CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
                break; // Stop pressed — leave the remaining presets untouched.
            }
            let item = copy_apply_one(&mut s, job, save).unwrap_or_else(|e| CopyApplyItem {
                slot: job.list_index,
                name: job.name.clone(),
                outcome: "error".to_string(),
                detail: e,
                graph: None,
            });
            let _ = on_result.send(item.clone());
            out.push(item);
        }
        Ok(out)
    })
    .await
}

/// Apply one [`CopyJob`]'s ordered ops to its target preset on a HELD re-armed session
/// and (if `save`) persist it — the [`held_replace_one`] shape generalised from one
/// replace to a list of replace/insert/remove ops. Loads the preset, re-arms the edit
/// context, confirms attachment (the SAME safety gate: never edit/save an unverified
/// preset), applies each op (RETRY-HARDENING the cold first op's silent DROP), and
/// saves ONLY when every op confirmed AND no `presetError`. An empty op list → skipped.
fn copy_apply_one(s: &mut Session, job: &CopyJob, save: bool) -> Result<CopyApplyItem, String> {
    let list_index = job.list_index;
    let name = job.name.clone();
    if job.ops.is_empty() {
        return Ok(CopyApplyItem {
            slot: list_index,
            name,
            outcome: "skipped".to_string(),
            detail: "no ops".to_string(),
            graph: None,
        });
    }

    // ── LOAD on the held session + RE-ARM the edit context to the just-loaded preset
    //    (mirrors `held_replace_one`). ──
    s.clear_raw();
    s.send_and_collect(&proto::load_preset((list_index + 1) as u64, 1), 200)?;
    s.send_and_collect(&proto::connection_request(), 80)?;
    s.send_and_collect(&proto::preset_list_request(1, 1), 20)?;
    s.send_and_collect(&proto::current_preset_info_request(2), 120)?;
    let _ = s.await_active_preset(&name, 8); // pump for the fresh currentPresetInfoChanged
                                             // SAFETY — confirm the held session re-attached to the TARGET preset before
                                             // editing/saving (active_matches prefers the PresetLoaded slot echo, falling back to
                                             // the active name only when no slot echo arrived).
    if !s.active_matches(list_index, Some(&name)) {
        return Ok(CopyApplyItem {
            slot: list_index,
            name: name.clone(),
            outcome: "error".to_string(),
            detail: format!(
                "could not confirm target preset loaded on held session (slot {:?} ≠ {list_index}, active {:?} ≠ target {name:?}) — not edited",
                s.loaded_slot(),
                s.active_preset_name()
            ),
            graph: None,
        });
    }

    // ── blockcaps guard — read the PRE-edit roster now, before the first structural
    //    edit (fail-closed: an unreadable roster refuses the WHOLE target). ──
    let (roster, mut counts) = blockcaps_pre_edit_roster(s)?;

    // Apply each op in order. The FIRST structural edit after a fresh load can be
    // silently DROPPED — retry it once (but NEVER on a presetError, a real rejection).
    let total = job.ops.len();
    for (i, op) in job.ops.iter().enumerate() {
        let first = i == 0;

        // Candidate/mode/target per op kind: Remove has no candidate (only shrinks —
        // never a cap check); Replace subtracts its target's contribution, Insert
        // doesn't (mirrors the TS `checkOp` mode-aware formula).
        let (candidate_id, is_replace, target): (Option<&str>, bool, Option<(&str, &str)>) =
            match op {
                CopyOp::Replace {
                    group,
                    node_id,
                    repl,
                } => (
                    Some(repl.insert_fender_id()),
                    true,
                    Some((group.as_str(), node_id.as_str())),
                ),
                CopyOp::Insert { repl, .. } => (Some(repl.insert_fender_id()), false, None),
                CopyOp::Remove { group, node_id } => {
                    (None, false, Some((group.as_str(), node_id.as_str())))
                }
            };
        let replaced = target.and_then(|(g, n)| blockcaps_replaced(&roster, g, n));
        if let Err(reason) = blockcaps_check(&counts, candidate_id, is_replace, replaced) {
            return Ok(CopyApplyItem {
                slot: list_index,
                name: name.clone(),
                outcome: "error".to_string(),
                detail: format!(
                    "op {}/{total} ({}) blocked by block-count cap: {reason} — NOT saved",
                    i + 1,
                    describe_copy_op(op)
                ),
                graph: None,
            });
        }

        match apply_copy_op(s, op, first) {
            Ok(true) => {
                blockcaps_advance(&mut counts, candidate_id, replaced);
            }
            Ok(false) => {
                return Ok(CopyApplyItem {
                    slot: list_index,
                    name: name.clone(),
                    outcome: "error".to_string(),
                    detail: format!(
                        "device rejected op {}/{total} ({}) — presetError / no confirm — NOT saved",
                        i + 1,
                        describe_copy_op(op)
                    ),
                    graph: None,
                });
            }
            Err(e) => {
                return Ok(CopyApplyItem {
                    slot: list_index,
                    name: name.clone(),
                    outcome: "error".to_string(),
                    detail: format!(
                        "op {}/{total} ({}) failed: {e} — NOT saved",
                        i + 1,
                        describe_copy_op(op)
                    ),
                    graph: None,
                });
            }
        }
    }

    if save {
        // Identity-preserving persist (Pro Control's rename(current name) → save(slot)):
        // keeps the preset's name and song link.
        if !name.is_empty() {
            s.rename_current_preset(&name)?;
        }
        s.save_current_preset(list_index)?;
    }
    // Keep the live-controller status warm before the next preset.
    s.heartbeat()?;
    s.pump_collect(120)?;
    // Read back the post-save graph so the Copy view can patch its cached library in
    // place (no ~22 s re-scan). The held session's dense heartbeat carries the full
    // `guitarNodes`; `None` when the field-3 isn't readable, and the frontend then falls
    // back to a re-scan. Only on save — an unsaved edit must not poison the cache.
    let graph = if save {
        s.current_preset_value()
            .ok()
            .map(|v| session::extract_active_graph(&v, None))
    } else {
        None
    };
    Ok(CopyApplyItem {
        slot: list_index,
        name,
        outcome: "updated".to_string(),
        detail: format!("{total} op(s)"),
        graph,
    })
}

/// Apply ONE [`CopyOp`] on the held session, returning whether the device CONFIRMED it
/// (`nodeReplaced`(40) / `nodeRemoved`(36) / `nodeInserted`(33)). `retry_drop` re-tries
/// a single SILENT drop (the cold first edit after a fresh load) but never a
/// `presetError`. IR/saved INSERT re-resolves the newly-added node id and applies the
/// IR-file / saved-block follow-up; if the new id can't be resolved it FALLS BACK to a
/// bare Model insert and `log::warn!`s the degradation.
fn apply_copy_op(s: &mut Session, op: &CopyOp, retry_drop: bool) -> Result<bool, String> {
    match op {
        CopyOp::Replace {
            group,
            node_id,
            repl,
        } => {
            let confirmed = apply_copy_replace(s, group, node_id, repl)?;
            if !confirmed && retry_drop && !s.saw_preset_error() {
                return apply_copy_replace(s, group, node_id, repl);
            }
            Ok(confirmed)
        }
        CopyOp::Remove { group, node_id } => {
            let confirmed = s.remove_node(group, node_id)?;
            if !confirmed && retry_drop && !s.saw_preset_error() {
                return s.remove_node(group, node_id);
            }
            Ok(confirmed)
        }
        CopyOp::Insert {
            group,
            before_fender_id,
            repl,
        } => {
            let confirmed =
                apply_copy_insert(s, group, before_fender_id.as_deref(), repl, retry_drop)?;
            Ok(confirmed)
        }
    }
}

/// `CopyRepl` REPLACE dispatch — the `ReplArg` dispatch from `held_replace_one`, minus
/// the (absent) Remove variant.
fn apply_copy_replace(
    s: &mut Session,
    group: &str,
    node_id: &str,
    repl: &CopyRepl,
) -> Result<bool, String> {
    match repl {
        CopyRepl::Model { fender_id } => s.replace_node(group, node_id, fender_id),
        CopyRepl::Saved { fender_id, index } => {
            s.replace_node_with_block(group, node_id, fender_id, *index)
        }
        CopyRepl::Ir { file, .. } => s.replace_node_with_ir(group, node_id, file),
    }
}

/// INSERT a block. The Model insert is the faithful one-shot (`insert_node` 34). For an
/// IR/saved insert we insert the bare model/placeholder, then RE-RESOLVE the
/// newly-added node id (the node present after the insert that was absent before, read
/// off the held session's roster) and apply the IR-file link / saved-block swap to it.
/// If the new id can't be resolved, FALL BACK to the bare Model insert and warn.
fn apply_copy_insert(
    s: &mut Session,
    group: &str,
    before_fender_id: Option<&str>,
    repl: &CopyRepl,
    retry_drop: bool,
) -> Result<bool, String> {
    let insert_id = repl.insert_fender_id();
    // Roster of this group BEFORE the insert — to diff the new node id afterwards.
    let before: std::collections::HashSet<String> = roster_node_ids_in_group(s, group);

    // field-34 insert: `before_fender_id` is the anchor to insert AHEAD of (the device's
    // field-2 inserts BEFORE the referenced node); `None` appends at the group end.
    let do_insert = |s: &mut Session| s.insert_node(group, before_fender_id, insert_id);
    let mut confirmed = do_insert(s)?;
    if !confirmed && retry_drop && !s.saw_preset_error() {
        confirmed = do_insert(s)?;
    }
    if !confirmed {
        return Ok(false);
    }

    // A plain Model insert is complete.
    let CopyRepl::Model { .. } = repl else {
        // IR / Saved → re-resolve the new node id and apply the content follow-up.
        let after: std::collections::HashSet<String> = roster_node_ids_in_group(s, group);
        let new_id = after.difference(&before).next().cloned();
        match new_id {
            Some(id) => match repl {
                CopyRepl::Ir { file, .. } => {
                    // Replace the just-inserted node WITH the IR (two-step: → ACD_UserIRTMS
                    // + the string `file` param), full-fidelity.
                    return s.replace_node_with_ir(group, &id, file);
                }
                CopyRepl::Saved { fender_id, index } => {
                    return s.replace_node_with_block(group, &id, fender_id, *index);
                }
                CopyRepl::Model { .. } => unreachable!("handled above"),
            },
            None => {
                log::warn!(
                    "[copy_apply] IR/saved INSERT into {group} degraded to a bare insert: could not \
                     re-resolve the newly-added node id on the held session (inserted {insert_id})"
                );
                // The bare insert DID land (confirmed) — report success, but the block is
                // a bare placeholder/model, not the IR/saved content.
                return Ok(true);
            }
        }
    };
    Ok(true)
}

/// The node ids currently in `group` on the held session (from a fresh field-3
/// roster read). Empty if no preset JSON is available yet.
fn roster_node_ids_in_group(s: &mut Session, group: &str) -> std::collections::HashSet<String> {
    let _ = s.heartbeat();
    let _ = s.pump_collect(200);
    s.current_preset_value()
        .ok()
        .map(|v| {
            audiograph::roster(&v)
                .into_iter()
                .filter(|(g, _, _)| g == group)
                .map(|(_, node_id, _)| node_id)
                .collect()
        })
        .unwrap_or_default()
}

/// Short human description of a `CopyOp` for the per-preset `error` detail.
fn describe_copy_op(op: &CopyOp) -> String {
    match op {
        CopyOp::Replace {
            group,
            node_id,
            repl,
        } => {
            format!("replace {group}/{node_id} → {}", repl.insert_fender_id())
        }
        CopyOp::Insert {
            group,
            before_fender_id,
            repl,
        } => format!(
            "insert {} into {group}{}",
            repl.insert_fender_id(),
            match before_fender_id.as_deref() {
                Some(b) => format!(" before {b}"),
                None => " (append)".to_string(),
            }
        ),
        CopyOp::Remove { group, node_id } => format!("remove {group}/{node_id}"),
    }
}




/// Lock a state mutex, recovering the guard if a previous holder panicked and poisoned it
/// (`into_inner`). These mutexes guard single-writer state (the session slot, the library,
/// the run registry, the monitor caches); recovery is always the right move — a poisoned
/// `unwrap()` would otherwise brick the always-running monitor or every future device op.
/// Used at every lock site across lib.rs / monitor.rs / watcher.rs.
pub(crate) fn lock_ok<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod lock_ok_tests {
    use super::lock_ok;
    use std::sync::{Arc, Mutex};

    #[test]
    fn recovers_a_poisoned_mutex_instead_of_panicking() {
        let m = Arc::new(Mutex::new(5));
        let m2 = Arc::clone(&m);
        // Poison the mutex: a thread panics while holding the lock.
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("poison the mutex");
        })
        .join();
        assert!(m.lock().is_err(), "the mutex must be poisoned");
        // A plain .lock().unwrap() would panic here; lock_ok recovers the guard.
        assert_eq!(*lock_ok(&m), 5);
        *lock_ok(&m) = 9;
        assert_eq!(*lock_ok(&m), 9);
    }
}

/// Shared device session. `None` until the user connects. Behind an `Arc<Mutex>`
/// so blocking HID work can run off the UI thread via `spawn_blocking`.
#[derive(Default)]
struct AppState {
    session: Arc<Mutex<Option<Session>>>,
    /// The imported OFFLINE `.preset` library (None until `import_library`). The
    /// canonical full-preset source every bulk feature edits.
    library: Arc<Mutex<Option<library::Library>>>,
    /// Completed bulk runs, keyed by run_id, so `bulk_revert` can restore one.
    runs: Arc<Mutex<bulk_cmd::RunRegistry>>,
    /// Rendered audition clips, keyed by slot+topology, so re-auditioning
    /// skips the re-amp pass. Session-scoped (see `audition` module caveat).
    clip_cache: Arc<Mutex<audition::ClipCache>>,
}

#[derive(Serialize)]
struct AppInfo {
    name: String,
    version: String,
}

/// Frontend handshake on mount — confirms the backend is reachable.
#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        name: "TMP Companion".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// Result of a combined connect+discover handshake. The frontend receives both
/// the firmware version AND the active signal graph in one shot, eliminating the
/// separate `read_active_preset` round-trip that previously doubled the connect time.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectResult {
    firmware: Option<String>,
    graph: Option<session::ActiveGraph>,
}

/// Start the monitor-owned startup session and wait for its first snapshot. The
/// monitor's single handshake supplies firmware + preset list + (usually) the active
/// graph, so startup doesn't serialize separate HID sessions for connect/list/live.
///
/// IDEMPOTENT against an already-running monitor (the webview-reload case): when
/// `MONITOR_ENABLED` is already set, the live pump never re-runs its one-shot
/// handshake, so clearing the snapshot here would wait 8 s for a snapshot that can
/// never be re-produced. Instead the already-enabled path serves the cached
/// snapshot (its graph is kept current by `monitor::refresh_snapshot_graph` on
/// every field-3 push) — or, when the monitor is mid-connect/device-absent, polls
/// WITHOUT clearing so the monitor's own connect error / next snapshot surfaces.
#[tauri::command]
async fn connect_device(state: State<'_, AppState>) -> Result<ConnectResult, String> {
    let arc = state.session.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<ConnectResult, String> {
        if !MONITOR_ENABLED.load(SeqCst) {
            // Genuinely-disabled path (first connect / post-stop): fresh start.
            *lock_ok(&arc) = None;
            monitor::reset_startup_state();
            MONITOR_ENABLED.store(true, SeqCst);
        }
        // Shared poll: snapshot | monitor connect error | 8 s deadline. On the
        // already-enabled-with-snapshot path the first iteration returns at once.
        let t0 = std::time::Instant::now();
        let deadline = std::time::Duration::from_secs(8);
        loop {
            if let Some(snapshot) = monitor::startup_snapshot() {
                log::info!(
                    "connect_device: monitor snapshot in {} ms, firmware={:?}, graph={}",
                    t0.elapsed().as_millis(),
                    snapshot.firmware,
                    if snapshot.graph.is_some() {
                        "ok"
                    } else {
                        "none"
                    }
                );
                return Ok(ConnectResult {
                    firmware: snapshot.firmware,
                    graph: snapshot.graph,
                });
            }
            if let Some(e) = monitor::last_connect_error() {
                if !e.contains("no TMP found") && !e.contains("IOHIDDeviceSetReport failed") {
                    log::warn!("connect_device failed via monitor: {e}");
                }
                return Err(e);
            }
            if t0.elapsed() >= deadline {
                return Err("monitor startup timed out waiting for TMP handshake".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    })
    .await
    .map_err(|e| format!("connect task failed: {e}"))?
}

// (There is intentionally no `disconnect_device` command — the app has no manual
// Connect/Disconnect buttons, nothing invoked it, and a sync command that drops the
// seize can't be serialized by the device-op gate without risking a main-thread
// stall. The session is released by `with_released_seize` / `connect_device` only.)

/// Enumerate the device's "My Presets" list. Under the app-level monitor this is a
/// snapshot read (no HID, no device-op lock), so the list paints with connect. A
/// fresh-session fallback remains for monitor-disabled diagnostic contexts.
#[tauri::command]
async fn list_presets(state: State<'_, AppState>) -> Result<Vec<PresetEntry>, String> {
    let arc = state.session.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<PresetEntry>, String> {
        if MONITOR_ENABLED.load(SeqCst) {
            if let Some(snapshot) = monitor::startup_snapshot() {
                return Ok(snapshot.presets);
            }
            if let Some(e) = monitor::last_connect_error() {
                return Err(e);
            }
            return Err("not connected — monitor startup snapshot is not ready".to_string());
        }
        let _op = lock_device_op();
        let maybe_session = {
            let mut guard = lock_ok(&arc);
            guard.take()
        };
        if let Some(mut session) = maybe_session {
            let result = session.list_my_presets_strict();
            *lock_ok(&arc) = Some(session);
            return result;
        }
        Session::connect()?.list_my_presets_strict()
    })
    .await
    .map_err(|e| format!("list task failed: {e}"))?
}


/// One resolved amp knob: `(group_id, node_id, current_outputLevel)`.
pub(crate) type AmpKnobSpec = (String, String, f32);


/// A bundled stimulus sample the user can pick per preset.
#[derive(Serialize)]
struct SampleInfo {
    /// Display label (file stem, e.g. "humbucker").
    name: String,
    /// Absolute path passed back as `stimulus_path`.
    path: String,
}

/// List the synthetic stimulus samples bundled in the app's resource dir.
#[tauri::command]
fn list_samples(app: tauri::AppHandle) -> Result<Vec<SampleInfo>, String> {
    use tauri::Manager;
    let dir = app
        .path()
        .resolve("resources/samples", tauri::path::BaseDirectory::Resource)
        .map_err(|e| format!("resolve samples dir: {e}"))?;
    let mut out = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("read {dir:?}: {e}"))?;
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) == Some("wav") {
            let name = p
                .file_stem()
                .and_then(|x| x.to_str())
                .unwrap_or("?")
                .to_string();
            if let Some(path) = p.to_str() {
                out.push(SampleInfo {
                    name,
                    path: path.to_string(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// A shipped pickup topology surfaced to the UI (Settings "Pickups" dropdown).
/// Mirrors the display-relevant fields of `topologies::Topology`; synth params
/// stay backend-only.
#[derive(Serialize)]
struct TopologyInfo {
    id: String,
    label: String,
    instrument: String,
}

/// List the shipped pickup topologies (the catalog backing instrument profiles).
/// Supersedes `list_samples` for the UI — profiles reference a topology by `id`.
#[tauri::command]
fn list_pickup_topologies() -> Vec<TopologyInfo> {
    topologies::TOPOLOGIES
        .iter()
        .map(|t| TopologyInfo {
            id: t.id.to_string(),
            label: t.label.to_string(),
            instrument: t.instrument.to_string(),
        })
        .collect()
}

/// Load the persisted profile store (instrument profiles + per-slot assignments).
#[tauri::command]
fn get_store<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<profiles::Store, String> {
    profiles::load(&app)
}

/// Replace the profile list (keeps the per-slot assignment map intact).
#[tauri::command]
fn save_profiles(app: tauri::AppHandle, profiles: Vec<profiles::Profile>) -> Result<(), String> {
    let mut store = self::profiles::load(&app)?;
    store.profiles = profiles;
    // Drop assignments that now point at a deleted profile.
    let live: std::collections::HashSet<&str> =
        store.profiles.iter().map(|p| p.id.as_str()).collect();
    store
        .profile_by_slot
        .retain(|_, id| live.contains(id.as_str()));
    self::profiles::save(&app, &store)
}

/// Replace the user's loudness targets (the named live levels edited in Settings).
#[tauri::command]
fn save_targets(app: tauri::AppHandle, targets: Vec<profiles::Target>) -> Result<(), String> {
    let mut store = self::profiles::load(&app)?;
    store.targets = targets;
    self::profiles::save(&app, &store)
}

/// Set the playback loudness leveling compensates for (Settings "Playback level").
#[tauri::command]
fn set_playback_level(app: tauri::AppHandle, level: profiles::PlaybackLevel) -> Result<(), String> {
    let mut store = self::profiles::load(&app)?;
    store.playback_level = level;
    self::profiles::save(&app, &store)
}

/// Resolve a topology id to its bundled stimulus WAV path in the resource dir.
/// Returns an error for an unknown id or unbundled WAV.
fn topology_wav_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    topology_id: &str,
) -> Result<String, String> {
    use tauri::Manager;
    topologies::by_id(topology_id)
        .ok_or_else(|| format!("unknown pickup topology '{topology_id}'"))?;
    let res = app
        .path()
        .resolve(
            format!("resources/samples/{topology_id}.wav"),
            tauri::path::BaseDirectory::Resource,
        )
        .map_err(|e| format!("resolve topology wav: {e}"))?;
    res.to_str()
        .map(str::to_string)
        .ok_or_else(|| "topology wav path not UTF-8".to_string())
}

/// One leveling job from the UI: a preset slot + the LUFS target to hit.
#[derive(serde::Deserialize)]
struct LevelJob {
    slot: u32,
    target_lufs: f64,
    /// Persist the computed `presetLevel` to the preset (SaveCurrentPreset).
    save: bool,
    /// Selected instrument's pickup topology id → its bundled stimulus WAV.
    topology_id: Option<String>,
    /// Tier-2 calibration: the profile's measured real output (K-weighted LUFS).
    /// When set, the stimulus is scaled to this loudness before injection.
    calibration_lufs: Option<f32>,
    /// Optional explicit stimulus override (takes precedence over `topology_id`).
    stimulus_path: Option<String>,
    /// Block-knob leveling: when all three are set, level by driving this block
    /// control (ChangeParameter, closed loop) instead of the master `presetLevel`.
    /// Coordinates come from `list_level_blocks`.
    block_group_id: Option<String>,
    block_node_id: Option<String>,
    block_parameter_id: Option<String>,
    /// The block param's current value (from `list_level_blocks`) — used to pick
    /// closed-loop search bounds (amplitude 0..1 vs dB-unit) without re-enumerating.
    block_value: Option<f32>,
}

/// Enumerate a preset's level-type block controls so the UI can offer them as
/// leveling knobs. Loads `slot` then reconnects (discovery handshake) to read its
/// `audioGraph` — runs with the app's seize released, like the leveling commands.
#[tauri::command]
async fn list_level_blocks(
    state: State<'_, AppState>,
    slot: u32,
) -> Result<Vec<session::LevelBlock>, String> {
    let blocks = with_released_seize(state.session.clone(), move || {
        load_then_discover_blocks(slot)
    })
    .await?;
    log::info!(
        "list_level_blocks slot={slot}: {} block(s): {}",
        blocks.len(),
        blocks
            .iter()
            .map(|b| format!("[{}]{}={:.3}", b.model_id, b.parameter_id, b.value))
            .collect::<Vec<_>>()
            .join(" ")
    );
    Ok(blocks)
}

// ── Active-preset signal chain: live reads + deliberate writes ──────────────────
// The connected device is the single source of truth.
// Reads parse the field-3 partial (block strip) and the songListResponse.
// Writes are DELIBERATE — every one fires only on an explicit human click in the
// ritual UI (confirm → write → read-back verify); none ever runs unattended.

/// The active preset's signal-chain graph for the "now playing" strip
/// (blocks + routing, read live via the field-78 discovery handshake). No load —
/// reads whatever preset is currently active on the device.
#[tauri::command]
async fn read_active_preset(state: State<'_, AppState>) -> Result<session::ActiveGraph, String> {
    with_released_seize(state.session.clone(), move || {
        discover_active_graph().map(|(graph, _)| graph)
    })
    .await
}

/// The monitor's CURRENT cached graph — the startup snapshot's graph, which
/// `monitor::refresh_snapshot_graph` keeps current on every field-3 push. A cheap
/// no-device-I/O, no-lock read (mirrors `list_presets`'s snapshot path) that lets a
/// freshly-mounted view re-seed its hero after a graphless connect, without the
/// heavy `read_active_preset` discovery. `None` when the cache has no graph yet.
#[tauri::command]
async fn current_graph() -> Result<Option<session::ActiveGraph>, String> {
    Ok(monitor::startup_graph())
}

/// Scene metadata for one preset, returned by the pure-lazy field-8 read.
#[derive(Clone, Serialize)]
pub(crate) struct PresetScenes {
    pub(crate) scenes: Vec<String>,
    pub(crate) fs: Vec<Option<u32>>,
    /// Block-acting footswitches (on/off + parameter change), with leveling-candidate
    /// params — empty when the preset has none.
    footswitches: Vec<footswitch::FootswitchInfo>,
}

pub(crate) fn decode_preset_scenes(json: &[u8]) -> Result<PresetScenes, String> {
    let live = session::decode_plain_preset_live(json)
        .ok_or_else(|| "could not parse preset scene JSON".to_string())?;
    let scenes = live
        .scene_names
        .ok_or_else(|| "preset scene JSON truncated before scenes".to_string())?;
    let map = live.ftsw.as_ref().map(footswitch::scene_fs_map);
    let fs = (0..scenes.len())
        .map(|i| {
            map.as_ref()
                .and_then(|m| m.get(&(i as u32)).copied())
                .map(|sw| sw + 1)
        })
        .collect();
    // Block-acting footswitches need the FULL preset (dspUnitParameters), which the
    // ActiveGraph drops — re-parse the raw field-8 JSON (tolerant: it survives the
    // scene-tail truncation; ftsw + audioGraph are well before it).
    let footswitches = match (
        session::tolerant_parse_json(&String::from_utf8_lossy(json)),
        live.ftsw.as_ref(),
    ) {
        (Some(preset), Some(ftsw)) => footswitch::enumerate_block_footswitches(ftsw, &preset),
        _ => Vec::new(),
    };
    Ok(PresetScenes {
        scenes,
        fs,
        footswitches,
    })
}

pub(crate) fn read_preset_scenes_fresh(list_index: u32) -> Result<PresetScenes, String> {
    let mut s = Session::connect()?;
    s.drain_until_quiet(250, 20)?;
    let json = s
        .read_slot_preset_json(list_index + 1)?
        .ok_or_else(|| format!("no preset scene data returned for slot {}", list_index + 1))?;
    decode_preset_scenes(&json)
}

/// Pure-lazy scene read for one preset. It never loads the preset: the command reads
/// slot-addressed field-8 plaintext JSON (`presetDataRequest` → `presetDataChanged`)
/// and decodes scene names + real footswitch tags from `ftsw`. It first tries the
/// monitor's metadata lane; when the monitor is not live it falls back to the proven
/// pause + fresh-session path.
#[tauri::command]
async fn read_preset_scenes(
    state: State<'_, AppState>,
    list_index: u32,
) -> Result<PresetScenes, String> {
    if let Some(result) = monitor::try_metadata_read(list_index) {
        match result {
            Ok(Some(json)) => return decode_preset_scenes(&json),
            Ok(None) => {
                log::warn!("read_preset_scenes: monitor lane returned no data; falling back")
            }
            Err(e) => return Err(e),
        }
    }
    with_released_seize(state.session.clone(), move || {
        read_preset_scenes_fresh(list_index)
    })
    .await
}

/// One streamed row of the Level dialog's selected-preset scene scan. `result`
/// is `None` when the slot read went unanswered or undecodable — the dialog
/// renders that preset as scanned-with-no-scenes (block roles still level it).
#[derive(Clone, Serialize)]
struct SceneScanItem {
    list_index: u32,
    result: Option<PresetScenes>,
}

/// Cooperative cancel for [`scan_preset_scenes`] — set by `cancel_scene_scan`
/// ("Skip — load during the run" / closing the dialog), checked between reads.
static SCENE_SCAN_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
fn cancel_scene_scan() {
    SCENE_SCAN_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Batch scene scan for the Level dialog: ONE dedicated lean session reading
/// every selected preset's field-8 plaintext JSON back-to-back (the HW-proven
/// `scan_all_scenes` / `probe --scenes-passive` recipe, ~0.5 s per preset),
/// streaming each preset's scenes over `on_result` as it lands so rows render
/// progressively. NON-DESTRUCTIVE — zero LoadPreset; the device's active preset
/// only ever changes later, in the post-disclaimer leveling RUN. Per-preset
/// monitor-lane reads (`read_preset_scenes`) pay ~3× per read in heartbeat
/// contention + IPC; batches must use this instead.
#[tauri::command]
async fn scan_preset_scenes(
    state: State<'_, AppState>,
    list_indices: Vec<u32>,
    on_result: tauri::ipc::Channel<SceneScanItem>,
) -> Result<(), String> {
    SCENE_SCAN_CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?;
        // Drain the handshake flood before the first re-armed read (a read
        // fired mid-flood is dropped device-side — the classic 0/25).
        s.drain_until_quiet(250, 20)?;
        for &idx in &list_indices {
            if SCENE_SCAN_CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            // Per-read failures degrade to a `None` row instead of aborting the
            // sweep — one unanswered slot must not strand the remaining rows.
            let result = match s.read_slot_preset_json(idx + 1) {
                Ok(Some(json)) => decode_preset_scenes(&json).ok(),
                Ok(None) | Err(_) => None,
            };
            let _ = on_result.send(SceneScanItem {
                list_index: idx,
                result,
            });
        }
        Ok(())
    })
    .await
}

/// One row of the active preset's live scene list (`sceneListResponse`). `fs` is the
/// best-effort footswitch tag — `None` for now (FS-tag RE is out of scope; the UI
/// renders an em-dash for null). Mirrors the monitor's `tmp://scene-list` rows.
#[derive(Serialize)]
struct SceneListRow {
    name: String,
    fs: Option<u32>,
}

/// Fetch the ACTIVE preset's scene list on demand — `sceneListRequest` (field 126).
/// The canonical scene-row source is the monitor's field-3 decode (the preset JSON's
/// `scenes[]`, pushed on every device change AND in the connect handshake); the unit
/// pushes `sceneListResponse(125)` itself only on an actual preset SWITCH. This
/// command is a manual diagnostic top-up. Routed through `with_released_seize`
/// so it serializes via `DEVICE_OP_LOCK` (pausing the monitor) like every device op.
#[tauri::command]
async fn request_scene_list(state: State<'_, AppState>) -> Result<Vec<SceneListRow>, String> {
    with_released_seize(state.session.clone(), move || {
        let names = Session::connect()?.request_scene_list()?;
        Ok(names
            .into_iter()
            .map(|name| SceneListRow { name, fs: None })
            .collect())
    })
    .await
}

/// Stop live-sync: clear `MONITOR_ENABLED` (the monitor drops its seize on its next
/// poll), then re-establish the persistent UI session so `list_presets` / commands
/// work as before. Idempotent. Returns the firmware version of the reclaimed session
/// (like `connect_device`), or `None` if the reconnect didn't carry it / no device.
#[tauri::command]
async fn stop_live_sync(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let arc = state.session.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Disable FIRST so the monitor releases its seize, THEN take the device-op gate
        // (which pauses + waits for the monitor to drop) before reconnecting the UI
        // session. Without the gate the reconnect could race the monitor's last seize.
        MONITOR_ENABLED.store(false, SeqCst);
        let _op = lock_device_op();
        *lock_ok(&arc) = None;
        let fw = match Session::connect_with_firmware() {
            Ok(s) => {
                let fw = s.firmware_version();
                *lock_ok(&arc) = Some(s);
                fw
            }
            Err(_) => None, // no device / not ready — UI session stays None (as before)
        };
        log::info!("live-sync stopped — UI session reclaimed (fw={fw:?})");
        fw
    })
    .await
    .map_err(|e| format!("stop_live_sync task failed: {e}"))
}

/// Read every Song's metadata (name / notes / BPM) for the Songs overview — the
/// net-new live `songListResponse` read (rides the handshake burst).
#[tauri::command]
async fn list_songs(state: State<'_, AppState>) -> Result<Vec<session::SongRecord>, String> {
    // Strict, fail-closed read (retry-until-complete): a read immediately after a
    // write is the worst case for the multi-packet truncation, and the Songs page
    // re-reads after every mutation, so accept only a strictly-complete response.
    with_released_seize(state.session.clone(), read_song_list).await
}

/// Make a preset the active one on the amp (`loadPreset`). A DELIBERATE action —
/// it switches the live tone — so it's a kebab item, never a row-tap. `list_index`
/// is 0-based; `session.load_preset` adds the device +1.
#[tauri::command]
async fn load_preset_on_amp(state: State<'_, AppState>, list_index: u32) -> Result<(), String> {
    let arc = state.session.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Fast path: while live-sync is on, fire the loadPreset on the monitor's
        // persistent session (~0.2 s) instead of the release→handshake→reconnect
        // bookend (~2 s). Falls back to the classic path when the lane isn't live.
        if let Some(r) = monitor::try_live_op(monitor::LiveOp::LoadPreset(list_index)) {
            return r;
        }
        with_released_seize_blocking(arc, move || {
            let mut s = Session::connect()?;
            s.load_preset(list_index)
        })
    })
    .await
    .map_err(|e| format!("device task failed: {e}"))?
}

/// Permanently clear a user slot (`clearUserPreset`) — DESTRUCTIVE, no undo. Goes
/// through [`guarded_clear`]: a fresh non-destructive read in the SAME 1-based
/// device-slot space must confirm the slot still holds `expect_name` before the
/// clear fires (the lesson from the off-by-one that erased real presets). The §4
/// confirm + read-back verify happen in the UI; this is the safe primitive.
#[tauri::command]
async fn delete_preset(
    state: State<'_, AppState>,
    list_index: u32,
    expect_name: String,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        guarded_clear(list_index, &expect_name)
    })
    .await
}

/// Reorder a user preset (`moveUserPreset`). DESTRUCTIVE to slot positions (no
/// undo). 0-based list indices; `session.move_user_preset` adds the device +1.
#[tauri::command]
async fn move_preset(state: State<'_, AppState>, from: u32, to: u32) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?;
        s.move_user_preset(from, to)
    })
    .await
}

/// Rename a preset in place: load it, `renameCurrentPreset`, then
/// `saveCurrentPreset` over its own slot (Pro Control's rename = rename + save).
/// DESTRUCTIVE (permanent) and it LOADS the slot (switches the live tone), so it's
/// a deliberate confirmed action. `list_index` is 0-based.
#[tauri::command]
async fn rename_save_preset(
    state: State<'_, AppState>,
    list_index: u32,
    name: String,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?;
        // Capture the target's CURRENT name so conn2 can confirm the right preset is
        // active before renaming+saving it — a dropped load would otherwise rename+save
        // a DIFFERENT preset over this slot.
        let name_before = s
            .list_my_presets()?
            .into_iter()
            .find(|p| p.slot == list_index)
            .map(|p| p.name)
            .ok_or_else(|| format!("rename target list index {list_index} out of range"))?;
        s.load_preset(list_index)?;
        drop(s);
        std::thread::sleep(std::time::Duration::from_millis(RECONNECT_AFTER_MS));
        let mut s = Session::connect()?;
        s.confirm_active(list_index, Some(&name_before))?;
        s.rename_current_preset(&name)?;
        s.save_current_preset(list_index)
    })
    .await
}

/// Recall a scene on the device — `loadScene` (PresetMessage field 101). `scene_slot`
/// is the **0-based** `scenes[]` index within the active preset;
/// `session::BASE_SCENE_SLOT` (8) recalls the base scene (the wire constant — HW-proven
/// by the `--loadscene 1` → scenes[1] "Reverb" activegraph diff + base echoing slot 8
/// even on a 0-scene preset). The proto's `LoadScene` addresses
/// a scene of the CURRENT preset, with no preset addressing of its own. So when
/// `list_index` is `Some`, the preset is loaded first (its own connection — a load
/// and a scene-recall in the SAME connection would have the load override the
/// scene), then a fresh connection recalls the scene; when `None`, the scene is
/// recalled on whatever preset is already active. A DELIBERATE action (it switches
/// the live tone), mirroring `load_preset_on_amp`. `list_index` is 0-based;
/// `session.load_preset` adds the device +1.
#[tauri::command]
async fn load_scene_on_amp(
    state: State<'_, AppState>,
    list_index: Option<u32>,
    scene_slot: u32,
) -> Result<(), String> {
    let arc = state.session.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Fast path — ACTIVE-preset scene recall only (`list_index == None`, the
        // shipped UI's normal case): fire the loadScene on the monitor's live
        // session. The `Some` case keeps the classic two-connection path — a load
        // and a scene-recall in the SAME connection have the load override the
        // scene (see the doc above), and that hazard is untested on the monitor's
        // long-lived session.
        if list_index.is_none() {
            if let Some(r) = monitor::try_live_op(monitor::LiveOp::LoadScene(scene_slot)) {
                return r;
            }
        }
        with_released_seize_blocking(arc, move || {
            if let Some(idx) = list_index {
                let mut s = Session::connect()?;
                s.load_preset(idx)?;
                drop(s);
                std::thread::sleep(std::time::Duration::from_millis(RECONNECT_AFTER_MS));
            }
            Session::connect()?.load_scene(scene_slot)
        })
    })
    .await
    .map_err(|e| format!("device task failed: {e}"))?
}

// ─── Song & Setlist CRUD for the device-backed Songs page ─────────────────────
// Read-back-after-write: each write fires through its own fresh connection, then
// re-reads via the strict fail-closed helpers and returns the fresh authoritative
// list, so the UI never predicts the device's positional slots (which shift on
// every add/remove). All run with the app's seize released (`with_released_seize`).
// These are the proven `probe_*` flows (probe_api::{songs,setlists})
// reshaped as slot-addressed, frontend-callable commands. HW writes are gated by
// the read-only-on-hardware policy (lifted per-session by explicit authorization).

/// Read every Setlist's name for the Songs page — strict fail-closed live read.
#[tauri::command]
async fn read_setlists(state: State<'_, AppState>) -> Result<Vec<session::SetlistRecord>, String> {
    with_released_seize(state.session.clone(), read_setlist_list).await
}

/// Read a setlist's songs in device order as GLOBAL song slots, trailing empty
/// (`songSlot==0`) entries dropped so the list is dense — position `i` here is the
/// `setlistSongSlot` the membership ops address (the index base is HW-pinned).
#[tauri::command]
async fn list_setlist_songs(
    state: State<'_, AppState>,
    setlist_slot: u32,
) -> Result<Vec<u32>, String> {
    with_released_seize(state.session.clone(), move || {
        read_setlist_songs(setlist_slot)
    })
    .await
}

/// Create a song; returns the fresh song list (the device assigns the slot).
#[tauri::command]
async fn add_song(
    state: State<'_, AppState>,
    name: String,
) -> Result<Vec<session::SongRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.add_song(&name)?;
        }
        read_song_list()
    })
    .await
}

/// Rename a song by slot; returns the fresh song list.
#[tauri::command]
async fn rename_song(
    state: State<'_, AppState>,
    slot: u32,
    name: String,
) -> Result<Vec<session::SongRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.rename_song(slot, &name)?;
        }
        read_song_list()
    })
    .await
}

/// Delete a song by slot — DESTRUCTIVE. Guarded: a fresh read in the SAME slot space
/// must still show `expect_name` at `slot` (tolerant of duplicate names elsewhere),
/// else refuse. Returns the fresh song list.
#[tauri::command]
async fn remove_song(
    state: State<'_, AppState>,
    slot: u32,
    expect_name: String,
) -> Result<Vec<session::SongRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        let before = read_song_list()?;
        let rec = before
            .iter()
            .find(|r| r.slot == slot)
            .ok_or_else(|| format!("song slot {slot} no longer exists — refusing to delete"))?;
        if rec.name != expect_name {
            return Err(format!(
                "guarded remove refused: song slot {slot} reads {:?}, expected {:?} (list changed)",
                rec.name, expect_name
            ));
        }
        {
            let mut s = Session::connect()?;
            s.remove_song(slot)?;
        }
        read_song_list()
    })
    .await
}

/// Set a song's notes by slot; returns the fresh song list.
#[tauri::command]
async fn set_song_notes(
    state: State<'_, AppState>,
    slot: u32,
    notes: String,
) -> Result<Vec<session::SongRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.set_song_notes(slot, &notes)?;
        }
        read_song_list()
    })
    .await
}

/// Set a song's numeric BPM by slot via the RE'd mechanism (there is NO dedicated
/// BPM setter): ensure the song has a footswitch (`assignSongPreset`), activate it
/// (`loadPreset tabEnum=5`), send the global `tapTempoBpm` (which the device stores
/// as the ACTIVE song's BPM — so this mutates active-song state as a side effect),
/// enable BPM display, verify by re-read. Retries the activate+tempo (the first load
/// after a fresh assign often doesn't settle). Non-convergence → Err. Returns the
/// fresh song list on success.
#[tauri::command]
async fn set_song_bpm(
    state: State<'_, AppState>,
    slot: u32,
    bpm: f32,
) -> Result<Vec<session::SongRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        let (after, converged) = converge_song_bpm(slot, bpm)?;
        if converged {
            return Ok(after);
        }
        let got = after
            .iter()
            .find(|x| x.slot == slot)
            .map(|x| x.bpm)
            .unwrap_or(0);
        Err(format!(
            "BPM for song slot {slot} did not converge to {bpm} after retries (read-back={got}); \
             tempo applies to the active song and this slot may not have activated"
        ))
    })
    .await
}

/// Create a setlist; returns the fresh setlist list.
#[tauri::command]
async fn add_setlist(
    state: State<'_, AppState>,
    name: String,
) -> Result<Vec<session::SetlistRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.add_setlist(&name)?;
        }
        read_setlist_list()
    })
    .await
}

/// Rename a setlist by slot; returns the fresh setlist list.
#[tauri::command]
async fn rename_setlist(
    state: State<'_, AppState>,
    slot: u32,
    name: String,
) -> Result<Vec<session::SetlistRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.rename_setlist(slot, &name)?;
        }
        read_setlist_list()
    })
    .await
}

/// Delete a setlist by slot — DESTRUCTIVE (the songs themselves are kept). Guarded
/// by `expect_name` in the same read space. Returns the fresh setlist list.
#[tauri::command]
async fn remove_setlist(
    state: State<'_, AppState>,
    slot: u32,
    expect_name: String,
) -> Result<Vec<session::SetlistRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        let before = read_setlist_list()?;
        let rec = before
            .iter()
            .find(|r| r.slot == slot)
            .ok_or_else(|| format!("setlist slot {slot} no longer exists — refusing to delete"))?;
        if rec.name != expect_name {
            return Err(format!(
                "guarded remove refused: setlist slot {slot} reads {:?}, expected {:?} (list changed)",
                rec.name, expect_name
            ));
        }
        {
            let mut s = Session::connect()?;
            s.remove_setlist(slot)?;
        }
        read_setlist_list()
    })
    .await
}

/// Add a song (by GLOBAL song slot) to a setlist; returns the setlist's fresh
/// ordered member song slots (dense — trailing 0s dropped).
#[tauri::command]
async fn add_setlist_song(
    state: State<'_, AppState>,
    setlist_slot: u32,
    song_slot: u32,
) -> Result<Vec<u32>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.add_setlist_song(setlist_slot, song_slot)?;
        }
        read_setlist_songs(setlist_slot)
    })
    .await
}

/// Remove a song from a setlist by its POSITION within the setlist
/// (`setlist_song_slot`, NOT the global song slot). Returns fresh member slots.
#[tauri::command]
async fn remove_setlist_song(
    state: State<'_, AppState>,
    setlist_slot: u32,
    setlist_song_slot: u32,
) -> Result<Vec<u32>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.remove_setlist_song(setlist_slot, setlist_song_slot)?;
        }
        read_setlist_songs(setlist_slot)
    })
    .await
}

/// Reorder a song within a setlist by POSITION (both indices are positions within
/// the setlist, NOT global song slots). Returns fresh member slots.
#[tauri::command]
async fn move_setlist_song(
    state: State<'_, AppState>,
    setlist_slot: u32,
    old_pos: u32,
    new_pos: u32,
) -> Result<Vec<u32>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.move_setlist_song(setlist_slot, old_pos, new_pos)?;
        }
        read_setlist_songs(setlist_slot)
    })
    .await
}

// ─── Batched song/setlist transactions ────────────────────────────────────────
// The granular commands above pay one full `with_released_seize` bookend + one
// strict fail-closed read PER FIELD (a song create with notes + BPM was 3 bookends
// + 3 reads ≈ 10 s+). These transactions run the same proven per-write fresh
// connections (the wire behavior is untouched) but under ONE bookend, skipping the
// intermediate read-backs: only the final authoritative read(s) remain. Slot
// stability inside a transaction: notes/BPM/membership writes don't shift slots —
// only song add/remove do, and a transaction does at most one add (first).

/// Result of a batched song save: the fresh authoritative song list, the fresh
/// membership of `add_to_setlist` (when requested), and the best-effort BPM
/// warning (BPM is the active-song tap tempo on the unit and can fail to settle —
/// the song itself is kept, mirroring the UI's previous per-call behavior).
#[derive(serde::Serialize)]
struct SongSaveOutcome {
    songs: Vec<session::SongRecord>,
    /// `Some` only when `add_to_setlist` was requested: that setlist's fresh
    /// ordered member song slots.
    members: Option<Vec<u32>>,
    bpm_warning: Option<String>,
}

/// Best-effort BPM step shared by the batched song transactions: returns the
/// fresh song list when the converge ran (success OR non-convergence), plus the
/// warning when it didn't stick. Never fails the transaction — mirrors the UI's
/// previous "Saved, but BPM didn't stick" toast semantics.
fn apply_song_bpm_best_effort(
    slot: u32,
    bpm: f32,
) -> (Option<Vec<session::SongRecord>>, Option<String>) {
    match converge_song_bpm(slot, bpm) {
        Ok((after, true)) => (Some(after), None),
        Ok((after, false)) => {
            let got = after
                .iter()
                .find(|x| x.slot == slot)
                .map(|x| x.bpm)
                .unwrap_or(0);
            (
                Some(after),
                Some(format!("BPM didn't converge to {bpm} (read-back={got})")),
            )
        }
        Err(e) => (None, Some(e)),
    }
}

/// Create a song with optional notes / BPM / setlist membership as ONE device
/// transaction (one bookend, one final read) — replaces the UI's add → read →
/// notes → read → bpm → read → addToSetlist → read chain. The created song is
/// resolved BY NAME from the post-add read (a new song inserts at protocol slot 1
/// and shifts every other song +1 — the device assigns the slot).
#[tauri::command]
async fn create_song_full(
    state: State<'_, AppState>,
    name: String,
    notes: Option<String>,
    bpm: Option<f32>,
    add_to_setlist: Option<u32>,
) -> Result<SongSaveOutcome, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.add_song(&name)?;
        }
        let mut songs = read_song_list()?;
        let Some(slot) = songs.iter().find(|s| s.name == name).map(|s| s.slot) else {
            // Created but not resolvable by name (duplicate-name edge) — return the
            // fresh list; the optional fields are skipped, surfaced as a warning.
            return Ok(SongSaveOutcome {
                songs,
                members: None,
                bpm_warning: Some(format!(
                    "song {name:?} created, but not resolvable by name — notes/BPM skipped"
                )),
            });
        };
        let notes = notes.filter(|n| !n.trim().is_empty());
        if let Some(n) = &notes {
            let mut s = Session::connect()?;
            s.set_song_notes(slot, n.trim())?;
        }
        let mut bpm_warning = None;
        match bpm {
            Some(b) => {
                let (fresh, warn) = apply_song_bpm_best_effort(slot, b);
                if let Some(fresh) = fresh {
                    songs = fresh; // the converge already re-read — reuse it
                }
                bpm_warning = warn;
            }
            None if notes.is_some() => songs = read_song_list()?,
            None => {}
        }
        let members = match add_to_setlist {
            Some(setlist_slot) => {
                {
                    let mut s = Session::connect()?;
                    s.add_setlist_song(setlist_slot, slot)?;
                }
                Some(read_setlist_songs(setlist_slot)?)
            }
            None => None,
        };
        Ok(SongSaveOutcome {
            songs,
            members,
            bpm_warning,
        })
    })
    .await
}

/// Update a song's changed fields (rename / notes / BPM) as ONE device
/// transaction (one bookend, one final read) — replaces the UI's per-field
/// command chain. `None` = field unchanged. The caller skips the call entirely
/// when nothing changed.
#[tauri::command]
async fn update_song_full(
    state: State<'_, AppState>,
    slot: u32,
    name: Option<String>,
    notes: Option<String>,
    bpm: Option<f32>,
) -> Result<SongSaveOutcome, String> {
    with_released_seize(state.session.clone(), move || {
        if let Some(n) = &name {
            let mut s = Session::connect()?;
            s.rename_song(slot, n)?;
        }
        if let Some(n) = &notes {
            let mut s = Session::connect()?;
            s.set_song_notes(slot, n)?;
        }
        let mut songs = None;
        let mut bpm_warning = None;
        if let Some(b) = bpm {
            let (fresh, warn) = apply_song_bpm_best_effort(slot, b);
            songs = fresh;
            bpm_warning = warn;
        }
        let songs = match songs {
            Some(s) => s,
            None => read_song_list()?, // one final authoritative read
        };
        Ok(SongSaveOutcome {
            songs,
            members: None,
            bpm_warning,
        })
    })
    .await
}

/// Add several songs (by GLOBAL song slot) to a setlist under ONE bookend with ONE
/// final membership read — replaces the UI's per-song `add_setlist_song` loop
/// (which paid a bookend + membership read per song). Same proven per-write fresh
/// connection per `addSetlistSong`.
#[tauri::command]
async fn add_setlist_songs(
    state: State<'_, AppState>,
    setlist_slot: u32,
    song_slots: Vec<u32>,
) -> Result<Vec<u32>, String> {
    with_released_seize(state.session.clone(), move || {
        for ss in &song_slots {
            let mut s = Session::connect()?;
            s.add_setlist_song(setlist_slot, *ss)?;
        }
        read_setlist_songs(setlist_slot)
    })
    .await
}

/// Resolve the stimulus WAV: explicit path → selected topology → `TMP_LEVELLER_STIMULUS`
/// env → the default bundled synthetic sample.
fn resolve_stimulus<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    explicit: Option<String>,
    topology_id: Option<String>,
) -> Result<String, String> {
    // Offline e2e: a fixed repo stimulus WAV (MockRuntime can't resolve bundle resources).
    #[cfg(feature = "e2e")]
    if let Ok(p) = std::env::var("TMP_E2E_STIMULUS") {
        if !p.is_empty() {
            return Ok(p);
        }
    }
    if let Some(p) = explicit.filter(|p| !p.is_empty()) {
        return Ok(p);
    }
    if let Some(tid) = topology_id.filter(|t| !t.is_empty()) {
        return topology_wav_path(app, &tid);
    }
    if let Ok(p) = std::env::var("TMP_LEVELLER_STIMULUS") {
        if !p.is_empty() {
            return Ok(p);
        }
    }
    topology_wav_path(app, topologies::DEFAULT_TOPOLOGY_ID)
}

use std::sync::atomic::{AtomicBool, Ordering::SeqCst};

/// Monitor intent: when set, the persistent device monitor (`monitor.rs`) owns the
/// idle HID seize, streams unsolicited unit pushes, and publishes the startup
/// snapshot. `connect_device` sets this after releasing any old UI session; commands
/// borrow the device through `DEVICE_OP_LOCK` + pause/ack. `stop_live_sync` is kept
/// for diagnostics/settings paths that explicitly need to reclaim a UI session.
pub(crate) static MONITOR_ENABLED: AtomicBool = AtomicBool::new(false);

/// A command (holding [`DEVICE_OP_LOCK`]) asks the persistent device monitor to
/// yield its exclusive HID seize so the command can open its own connection
/// without a `0xe00002c5` collision. Set true while a command's [`MonitorPauseGuard`]
/// is alive; cleared on its Drop. The monitor polls this every pump iteration.
pub(crate) static MONITOR_PAUSE_REQ: AtomicBool = AtomicBool::new(false);
/// The monitor has dropped its `Session` (its seize is free) in response to a pause
/// request. The command waits (bounded) for this ack before proceeding. Cleared by
/// the monitor when it resumes after the request clears.
pub(crate) static MONITOR_PAUSED_ACK: AtomicBool = AtomicBool::new(false);

/// Fletcher–Munson playback compensation for a leveling job: the LU offset added
/// to the target, from the store's playback level × the stimulus topology's
/// instrument family. Equal-LUFS is equal-loudness only at the SPL the K-weighting
/// curve approximates (~stage volume); at quieter playback the equal-loudness
/// contours steepen and a bass preset matched at equal LUFS sits perceptibly
/// quieter, so its target is raised (see `profiles::playback_offset_lu`). `None` /
/// unknown topology falls back to the guitar default (offset 0); `Stage` (the
/// store default) is always 0, so legacy stores level exactly as before.
fn playback_offset_for<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    topology_id: Option<&str>,
) -> f64 {
    let level = profiles::load(app)
        .map(|s| s.playback_level)
        .unwrap_or_default();
    profiles::playback_offset_lu(level, stimulus_instrument(topology_id))
}

/// The instrument family a leveling job's stimulus belongs to (`None` / unknown
/// topology = the guitar default).
fn stimulus_instrument(topology_id: Option<&str>) -> &'static str {
    topologies::by_id(topology_id.unwrap_or(topologies::DEFAULT_TOPOLOGY_ID))
        .map(|t| t.instrument)
        .unwrap_or("guitar")
}

/// Level one preset to its target (the real, one-shot open-loop path). The
/// leveller opens its own fresh connections (load → measure → set), so the work
/// runs with the app's seize released (see `with_released_seize`).
#[tauri::command]
async fn level_preset<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    job: LevelJob,
) -> Result<leveller::LevelResult, String> {
    let LevelJob {
        slot,
        target_lufs,
        save,
        topology_id,
        calibration_lufs,
        stimulus_path,
        block_group_id,
        block_node_id,
        block_parameter_id,
        block_value,
    } = job;
    let offset_lu = playback_offset_for(&app, topology_id.as_deref());
    if offset_lu != 0.0 {
        log::info!("level_preset slot={slot}: playback compensation {offset_lu:+.1} LU on target {target_lufs:.1}");
    }
    let target_lufs = target_lufs + offset_lu;
    let stim_path = resolve_stimulus(&app, stimulus_path, topology_id)?;
    // A block knob is selected only when all three coordinates are present;
    // otherwise level the master `presetLevel` (the validated one-shot path).
    let block = match (block_group_id, block_node_id, block_parameter_id) {
        (Some(g), Some(n), Some(p)) if !g.is_empty() && !n.is_empty() && !p.is_empty() => {
            Some((g, n, p))
        }
        _ => None,
    };
    // Reset the cooperative cancel flag for this run; `cancel_preset_leveling` sets it
    // (it only flips the atomic — no device lock — so it runs while this op holds it).
    PRESET_LEVEL_CANCEL.store(false, SeqCst);
    let app_evt = app.clone();
    with_released_seize(state.session.clone(), move || {
        // Stream advisory live LUFS while each capture runs (dropped at closure end).
        let _lufs = LiveLufsGuard::install(app_evt);
        let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;
        let opts = leveller::LevelOptions { save, verify: true, ..Default::default() };
        let cancelled = || PRESET_LEVEL_CANCEL.load(SeqCst);
        let result = match block {
            Some((group_id, node_id, parameter_id)) => {
                let (lo, hi) = knob_bounds(block_value.unwrap_or(0.5));
                let knob = leveller::LevelKnob::Block { group_id, node_id, parameter_id, scene_slot: None };
                leveller::level_preset_block(slot, &stim, &knob, lo, hi, target_lufs, opts, cancelled)
            }
            None => leveller::level_preset(slot, &stim, target_lufs, opts, cancelled),
        };
        match &result {
            Ok(r) => log::info!(
                "level_preset slot={} save={} measured={:.2} LUFS target={:.2} LUFS final_level={:.4} verify={:?}",
                r.slot,
                r.saved,
                r.measured_lufs,
                r.target_lufs,
                r.final_level,
                r.verify_lufs,
            ),
            Err(e) => log::warn!("level_preset slot={slot} save={save} failed: {e}"),
        }
        result
    })
    .await
}

/// A candidate leveling knob for `level_scenes_apply` — the frontend passes EVERY
/// amp-level candidate (it owns amp-ness via the models catalog); the backend picks
/// PER SCENE the one whose block is actually ON in that scene.
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LevelBlockArg {
    pub(crate) group_id: String,
    pub(crate) node_id: String,
    pub(crate) parameter_id: String,
    pub(crate) value: f32,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SceneLevelProgressItem {
    scene_slot: u32,
    status: String,
    result: Option<leveller::LevelResult>,
    message: Option<String>,
}

/// Wire payload for `tmp://leveling-lufs` — the advisory live measured loudness streamed
/// while a leveling capture runs, so the UI can show a "measuring…" readout. ADVISORY: this
/// is the loudness at the reference level, NOT the final preset level (the result row is the
/// confirm). Mirrored in `src/lib/types.ts`.
#[derive(Clone, serde::Serialize)]
struct LiveLufsEvent {
    lufs: f64,
}

/// RAII guard: installs an advisory live-LUFS sink that emits `tmp://leveling-lufs` for the
/// lifetime of a leveling run, clearing it on drop (incl. unwind). Every leveling command
/// runs serialized under the device-op lock, so only one guard is ever live at a time.
struct LiveLufsGuard;

impl LiveLufsGuard {
    fn install<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Self {
        use tauri::Emitter;
        audio::set_live_lufs_sink(Box::new(move |lufs| {
            let _ = app.emit("tmp://leveling-lufs", LiveLufsEvent { lufs });
        }));
        LiveLufsGuard
    }
}

impl Drop for LiveLufsGuard {
    fn drop(&mut self) {
        audio::clear_live_lufs_sink();
    }
}

pub(crate) static SCENE_LEVEL_CANCEL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
fn cancel_scene_leveling() {
    SCENE_LEVEL_CANCEL.store(true, SeqCst);
}

/// Cooperative cancel for [`level_preset`] (base-preset leveling) — set by
/// `cancel_preset_leveling`, reset at the command's start, read via a closure passed into
/// `leveller::level_preset`/`level_preset_block`, which bail before the apply+save.
static PRESET_LEVEL_CANCEL: AtomicBool = AtomicBool::new(false);

#[tauri::command]
fn cancel_preset_leveling() {
    PRESET_LEVEL_CANCEL.store(true, SeqCst);
}

// ───────────────────────── Footswitch (engaged-state) leveling ─────────────────────────

/// One footswitch-leveling request: level switch `switch`'s engaged state by solving the
/// `(lev_group_id, lev_node_id, lev_parameter_id)` param to hit `target_lufs`.
#[derive(serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FootswitchLevelJob {
    pub(crate) switch: u32,
    pub(crate) lev_group_id: String,
    pub(crate) lev_node_id: String,
    pub(crate) lev_parameter_id: String,
    pub(crate) target_lufs: f64,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FootswitchLevelProgressItem {
    switch: u32,
    status: String, // active | done | error | cancelled
    result: Option<leveller::FootswitchLevelResult>,
    message: Option<String>,
}

static FOOTSWITCH_LEVEL_CANCEL: AtomicBool = AtomicBool::new(false);

#[tauri::command]
fn cancel_footswitch_leveling() {
    FOOTSWITCH_LEVEL_CANCEL.store(true, SeqCst);
}

/// Read a slot's field-8 preset JSON on a fresh quiet session and return the parsed preset, the
/// scene gate (`Some(empty)` = definitely no FS scenes; truncated/unknown or non-empty →
/// conservative `true`), and the raw byte length. Shared by the footswitch leveling command +
/// probes (the connect→drain→read→parse→scene-check boilerplate).
pub(crate) fn read_slot_preset_parsed(slot: u32) -> Result<(serde_json::Value, bool, usize), String> {
    let mut s = Session::connect()?;
    s.drain_until_quiet(250, 20)?;
    let json = s
        .read_slot_preset_json(slot + 1)?
        .ok_or_else(|| format!("no preset data for slot {}", slot + 1))?;
    let preset = session::tolerant_parse_json(&String::from_utf8_lossy(&json))
        .ok_or_else(|| "preset JSON did not parse".to_string())?;
    let has_fs_scenes = session::scene_names_from_slot_json(&json).is_none_or(|n| !n.is_empty());
    Ok((preset, has_fs_scenes, json.len()))
}

/// A numeric `dspUnitParameter` of `node_id` (e.g. the lev param's current value = `valueB`).
pub(crate) fn node_param_f64(preset: &serde_json::Value, node_id: &str, param: &str) -> Option<f64> {
    let mut found = None;
    audiograph::for_each_node(preset, |obj| {
        if obj.get("nodeId").and_then(|v| v.as_str()) == Some(node_id) {
            found = obj
                .get("dspUnitParameters")
                .and_then(|p| p.get(param))
                .and_then(|v| v.as_f64());
        }
    });
    found
}

/// Resolved inputs to `leveller::level_footswitch`: the switch-OFF value (`valueB` = the
/// param's current value) and the write spec.
type FootswitchJobResolution = (f32, leveller::FootswitchWriteSpec);

/// Resolve a footswitch-leveling job against the preset: the lev param's current value
/// (`valueB`) and the write spec (edit an existing matching `param` function, else add at
/// the next free index; enforce the firmware's 5-function cap). The leveler only ever
/// creates/edits a parameter-change assignment — it does not touch on/off.
pub(crate) fn resolve_footswitch_job(
    ftsw: &serde_json::Value,
    preset: &serde_json::Value,
    job: &FootswitchLevelJob,
) -> Result<FootswitchJobResolution, String> {
    let switches = ftsw.as_array().ok_or("preset has no ftsw")?;
    let sw = switches
        .get(job.switch as usize)
        .and_then(|s| s.as_array())
        .ok_or_else(|| format!("footswitch {} not found", job.switch))?;

    let value_b =
        node_param_f64(preset, &job.lev_node_id, &job.lev_parameter_id).ok_or_else(|| {
            format!(
                "parameter {} not found on {}",
                job.lev_parameter_id, job.lev_node_id
            )
        })? as f32;

    // Edit an existing param fn on (lev_node, lev_param), else add (≤5 cap).
    let existing = footswitch::existing_param_fn_index(
        ftsw,
        job.switch,
        &job.lev_node_id,
        &job.lev_parameter_id,
    )
    .and_then(|i| sw.get(i as usize).map(|a| (i, a)));
    let spec = match existing {
        Some((i, a)) => leveller::FootswitchWriteSpec {
            function_index: i,
            color_a: a.get("colorA").and_then(|v| v.as_u64()).unwrap_or(3) as u32,
            color_b: a.get("colorB").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            custom_label: a
                .get("customLabel")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            link_group: a.get("linkGroup").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            is_active: a.get("isActive").and_then(|v| v.as_bool()).unwrap_or(true),
        },
        None => {
            if sw.len() >= 5 {
                return Err(format!(
                    "footswitch {} is full (5 functions) — no room to add a leveling param",
                    job.switch
                ));
            }
            leveller::FootswitchWriteSpec {
                function_index: sw.len() as u32,
                color_a: 3,
                color_b: 0,
                custom_label: String::new(),
                link_group: 0,
                is_active: true,
            }
        }
    };
    Ok((value_b, spec))
}

/// Level one or more block-acting footswitches of preset `slot`, streaming a progress item
/// per switch. Each switch's engaged state is measured/solved independently against the
/// base preset; jobs run sequentially. Mirrors `level_scenes_apply_batched`.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn level_footswitches_apply(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    slot: u32,
    jobs: Vec<FootswitchLevelJob>,
    save: bool,
    topology_id: Option<String>,
    calibration_lufs: Option<f32>,
    on_result: tauri::ipc::Channel<FootswitchLevelProgressItem>,
) -> Result<Vec<leveller::FootswitchLevelResult>, String> {
    let stim_path = resolve_stimulus(&app, None, topology_id.clone())?;
    let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;
    let offset = playback_offset_for(&app, topology_id.as_deref());
    FOOTSWITCH_LEVEL_CANCEL.store(false, SeqCst);
    let app_evt = app.clone();

    with_released_seize(state.session.clone(), move || {
        // Stream advisory live LUFS while each capture runs (dropped at closure end).
        let _lufs = LiveLufsGuard::install(app_evt);
        // Read the preset once (resolve every job) + whether it has FS scenes (the bake gate).
        let (preset, has_fs_scenes, _) = read_slot_preset_parsed(slot)?;
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
        let ftsw = preset
            .get("ftsw")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        // Plan bake-vs-assign for the whole batch (pure) — block-off-in-base + sole-owner +
        // no-scenes ⇒ bake straight onto the block; otherwise the (engaged-measured) param fn.
        let keys: Vec<footswitch::FsJobKey> = jobs
            .iter()
            .map(|j| footswitch::FsJobKey {
                switch: j.switch,
                lev_node: &j.lev_node_id,
                lev_param: &j.lev_parameter_id,
                target_bits: j.target_lufs.to_bits(),
            })
            .collect();
        let plans = footswitch::plan_footswitch_jobs(&ftsw, &preset, &keys, has_fs_scenes);

        let mut results: Vec<Option<leveller::FootswitchLevelResult>> = vec![None; jobs.len()];
        for (idx, job) in jobs.iter().enumerate() {
            if FOOTSWITCH_LEVEL_CANCEL.load(SeqCst) {
                let _ = on_result.send(FootswitchLevelProgressItem {
                    switch: job.switch,
                    status: "cancelled".into(),
                    result: None,
                    message: None,
                });
                break;
            }
            let _ = on_result.send(FootswitchLevelProgressItem {
                switch: job.switch,
                status: "active".into(),
                result: None,
                message: None,
            });
            let lev = (
                job.lev_group_id.as_str(),
                job.lev_node_id.as_str(),
                job.lev_parameter_id.as_str(),
            );
            let outcome: Result<leveller::FootswitchLevelResult, String> = match &plans[idx] {
                footswitch::FsLevelPlan::Clamp(msg) => Err(msg.clone()),
                // A sibling switch already baked this (node, param, target) — reuse its result.
                footswitch::FsLevelPlan::BakeShared { rep } => results[*rep]
                    .clone()
                    .map(|mut r| {
                        r.switch = job.switch;
                        r
                    })
                    .ok_or_else(|| "shared bake produced no result".to_string()),
                footswitch::FsLevelPlan::Bake {
                    engaged,
                    clear_stale,
                } => leveller::level_footswitch(
                    slot,
                    job.switch,
                    lev,
                    engaged,
                    &leveller::FsWrite::Bake {
                        clear_stale: *clear_stale,
                    },
                    &stim,
                    job.target_lufs + offset,
                    save,
                    true,
                ),
                footswitch::FsLevelPlan::Assign { engaged } => {
                    match resolve_footswitch_job(&ftsw, &preset, job) {
                        Err(e) => Err(e),
                        Ok((value_b, spec)) => leveller::level_footswitch(
                            slot,
                            job.switch,
                            lev,
                            engaged,
                            &leveller::FsWrite::Assign { value_b, spec },
                            &stim,
                            job.target_lufs + offset,
                            save,
                            true,
                        ),
                    }
                }
            };
            let item = match outcome {
                Ok(r) => {
                    results[idx] = Some(r.clone());
                    FootswitchLevelProgressItem {
                        switch: job.switch,
                        status: "done".into(),
                        result: Some(r),
                        message: None,
                    }
                }
                Err(e) => FootswitchLevelProgressItem {
                    switch: job.switch,
                    status: "error".into(),
                    result: None,
                    message: Some(e),
                },
            };
            let _ = on_result.send(item);
        }
        // Guarantee re-amp OFF on a fresh connection.
        if let Ok(mut s) = Session::connect() {
            let _ = s.set_reamp_mode(false);
        }
        Ok(results.into_iter().flatten().collect())
    })
    .await
}


fn pick_scene_level_knob(
    slot: u32,
    scene: u32,
    candidates: &[LevelBlockArg],
) -> Result<(leveller::LevelKnob, f32, f32, f32), String> {
    let scene_slot = if scene >= session::BASE_SCENE_SLOT {
        None
    } else {
        Some(scene)
    };
    // ONE rich session (HW-rearchitected): heartbeat warmup → loads
    // via send_and_collect → live doc from the accumulated field-3 pushes. The
    // old connect → load → drop → connect_for_discovery chain is broken on fw
    // 1.8.45 twice over: a close chased by a re-open wedges the device's next
    // exclusive open (0xe00002c5 lockout), and field-78 kills field-3 delivery
    // for its whole session anyway. After each load the raw accumulator is
    // cleared so the doc reflects the POST-scene live state (the pick must read
    // the sounding graph, never stale pre-scene pushes).
    let live_doc = {
        let mut s = Session::connect()?;
        for _ in 0..16 {
            s.heartbeat()?;
            s.pump_collect(120)?;
        }
        s.raw.clear();
        s.send_and_collect(&proto::load_preset((slot + 1) as u64, 1), 300)?;
        for _ in 0..8 {
            s.heartbeat()?;
            s.pump_collect(200)?;
        }
        if let Some(sl) = scene_slot {
            s.raw.clear();
            s.send_and_collect(&proto::load_scene(sl as u64), 300)?;
            for _ in 0..8 {
                s.heartbeat()?;
                s.pump_collect(200)?;
            }
        }
        s.current_preset_value()?
    };
    for c in candidates {
        log::info!(
            "pick_scene_level_knob scene={scene} candidate {}/{}/{} live_bypass={:?}",
            c.group_id,
            c.node_id,
            c.parameter_id,
            scenes::block_bypass_in_live_graph(&live_doc, &c.group_id, &c.node_id),
        );
    }
    let picked = candidates
        .iter()
        .filter(|c| is_amp_output_level_param(&c.parameter_id))
        .find(|c| {
            scenes::block_bypass_in_live_graph(&live_doc, &c.group_id, &c.node_id) == Some(false)
        })
        .ok_or_else(|| format!("no active amp outputLevel control found for scene slot {scene}"))?;
    let (lo, hi) = knob_bounds(picked.value);
    Ok((
        leveller::LevelKnob::Block {
            group_id: picked.group_id.clone(),
            node_id: picked.node_id.clone(),
            parameter_id: picked.parameter_id.clone(),
            scene_slot,
        },
        lo,
        hi,
        picked.value,
    ))
}

/// Level ONE scene the capture-per-connection way (`level_preset_block`): pick
/// the scene's knob from its live graph, then closed-loop with fresh re-amp
/// captures. The legacy `level_scenes_apply` path; the shipped batched flow is
/// `level_scenes_apply_batched` → `leveller::level_scenes_live_batched`.
fn level_one_scene_legacy(
    slot: u32,
    scene: u32,
    candidates: &[LevelBlockArg],
    stimulus: &[f32],
    target_lufs: f64,
    save: bool,
) -> Result<leveller::LevelResult, String> {
    let (knob, lo, hi, _current) = pick_scene_level_knob(slot, scene, candidates)?;
    // 800 ms before the leveller's first fresh connect — the empirical safe gap
    // after a rich-session close (shorter chases trip the device's open lockout).
    std::thread::sleep(std::time::Duration::from_millis(800));
    let opts = leveller::LevelOptions {
        save,
        verify: true,
        ..Default::default()
    };
    leveller::level_preset_block(slot, stimulus, &knob, lo, hi, target_lufs, opts, || false)
}

/// Per-scene leveling APPLY (chosen mechanism: enable scene mode on the amp
/// block, level only the amp `outputLevel` control). For each selected scene, drive
/// the scene's ACTIVE amp's `outputLevel` knob closed-loop to `target_lufs` with
/// per-block Scene Edit enabled —
/// so the level lands on that scene's overlay, not the base. The knob is resolved
/// PER SCENE from `candidates` by the scene overlay's `bypass` (HW-found:
/// a preset can carry several amps with scenes swapping which is live — leveling a
/// bypassed amp's knob measures flat and clamps).
/// `scene_slots` are the WIRE slots: 0-based `scenes[]` indices for FS scenes;
/// `session::BASE_SCENE_SLOT` (8) = the base/preset value (levelled WITHOUT scene-edit
/// — a preset load activates base, so no scene recall is needed).
/// DEVICE WRITE when `save` — opt-in, gated by the read-only HW policy + the leveling
/// overlay confirm. Reuses `level_preset_block` (the scene context rides the knob and
/// is re-asserted on every connection). Each scene is a self-contained leveling pass.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn level_scenes_apply(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    slot: u32,
    scene_slots: Vec<u32>,
    candidates: Vec<LevelBlockArg>,
    target_lufs: f64,
    save: bool,
    topology_id: Option<String>,
    calibration_lufs: Option<f32>,
) -> Result<Vec<leveller::LevelResult>, String> {
    if !candidates
        .iter()
        .any(|c| is_amp_output_level_param(&c.parameter_id))
    {
        return Err("per-scene leveling needs at least one amp outputLevel candidate".to_string());
    }
    if scene_slots.is_empty() {
        return Err("no scenes selected".to_string());
    }
    let target_lufs = target_lufs + playback_offset_for(&app, topology_id.as_deref());
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;
        let run = || -> Result<Vec<leveller::LevelResult>, String> {
            let mut results = Vec::with_capacity(scene_slots.len());
            for scene in &scene_slots {
                let r = level_one_scene_legacy(
                    slot,
                    *scene,
                    &candidates,
                    &stim,
                    target_lufs,
                    save,
                )?;
                log::info!(
                    "level_scenes_apply slot={slot} scene={scene} save={save} final_level={:.4} measured={:.2} clamped={}",
                    r.final_level, r.measured_lufs, r.clamped,
                );
                results.push(r);
            }
            Ok(results)
        };
        let result = run();
        // GUARANTEED re-amp OFF on a fresh connection, success or failure. The
        // leveller's in-connection `set_reamp_mode(false)` is fire-and-forget and
        // demonstrably gets dropped under the run's connection churn — HW-observed (TWICE): the unit came out of a scene-leveling run stuck in
        // re-amp (guitar input muted, "no sound") until a power-cycle.
        match Session::connect().and_then(|mut s| s.set_reamp_mode(false)) {
            Ok(_) => log::info!("level_scenes_apply: final re-amp OFF sent"),
            Err(e) => log::warn!("level_scenes_apply: final re-amp OFF failed ({e})"),
        }
        result
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn level_scenes_apply_batched(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    slot: u32,
    scene_slots: Vec<u32>,
    candidates: Vec<LevelBlockArg>,
    target_lufs: f64,
    save: bool,
    rebalance: bool,
    topology_id: Option<String>,
    calibration_lufs: Option<f32>,
    on_result: tauri::ipc::Channel<SceneLevelProgressItem>,
) -> Result<Vec<leveller::LevelResult>, String> {
    if !candidates
        .iter()
        .any(|c| is_amp_output_level_param(&c.parameter_id))
    {
        return Err("per-scene leveling needs at least one amp outputLevel candidate".to_string());
    }
    if scene_slots.is_empty() {
        return Err("no scenes selected".to_string());
    }
    SCENE_LEVEL_CANCEL.store(false, SeqCst);
    let target_lufs = target_lufs + playback_offset_for(&app, topology_id.as_deref());
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    let app_evt = app.clone();
    with_released_seize(state.session.clone(), move || {
        // Stream advisory live LUFS while each capture runs (dropped at closure end).
        let _lufs = LiveLufsGuard::install(app_evt);
        let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;
        let run_batched = |save_run: bool| -> Result<Vec<leveller::BatchedSceneOutcome>, String> {
            // Un-engaged pre-pass (scene docs → jobs), then the ONE-SHOT runner:
            // amp `outputLevel` is linear in dB, so each scene is measured once at a
            // reference level (ISOLATED fresh re-amp capture) and solved exactly — the
            // BatchedLive shared-stream loop mis-measured scenes (HW).
            let docs = prepass_scene_docs(slot, &scene_slots)?;
            // Inter-session HID gap: the prepass session has just closed; the one-shot
            // runner opens a fresh one. Reuse the leveller's HW-proven open-after-close
            // gap (was a hard-coded 800, copied from the bench). build_scene_jobs below
            // is pure CPU, so this is the only wait here.
            std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
            let jobs = build_scene_jobs(&scene_slots, &candidates, &docs)?;
            let on_scene = |scene, done: Option<&leveller::BatchedSceneOutcome>| match done {
                None => {
                    let _ = on_result.send(SceneLevelProgressItem {
                        scene_slot: scene,
                        status: "active".to_string(),
                        result: None,
                        message: None,
                    });
                }
                Some(o) => {
                    let item = match &o.failure {
                        None => SceneLevelProgressItem {
                            scene_slot: scene,
                            status: "done".to_string(),
                            result: Some(outcome_to_level_result(slot, target_lufs, save_run, o)),
                            message: None,
                        },
                        Some(e) => SceneLevelProgressItem {
                            scene_slot: scene,
                            status: "error".to_string(),
                            result: None,
                            message: Some(e.clone()),
                        },
                    };
                    let _ = on_result.send(item);
                }
            };
            let cancelled = || SCENE_LEVEL_CANCEL.load(SeqCst);
            // `rebalance` (opt-in) equalizes a path-MERGE scene's two lanes before joint-k;
            // non-mergeable scenes fall through to the same joint-k either way.
            if rebalance {
                leveller::level_scenes_rebalance(
                    slot,
                    &jobs,
                    &stim,
                    target_lufs,
                    save_run,
                    on_scene,
                    cancelled,
                )
            } else {
                leveller::level_scenes_oneshot(
                    slot,
                    &jobs,
                    &stim,
                    target_lufs,
                    save_run,
                    on_scene,
                    cancelled,
                )
            }
        };
        // Per-scene leveling drives ONLY the active amp's `outputLevel`. When a scene
        // can't reach target even at the knob's limit it CLAMPS and reports the achieved
        // loudness — we do NOT raise the global `presetLevel` to compensate. Raising it
        // lifts EVERY other scene off-target (presetLevel is the Base's job, settled once
        // before the scene pass), and HW the old boost-and-rerun drove
        // presetLevel to 1.0 and blew preset 001's loud scenes 5–7 LU over target.
        let outcome = run_batched(save);
        let result = match outcome {
            Ok(outcomes) => Ok(outcomes
                .iter()
                .filter(|o| o.failure.is_none())
                .map(|o| outcome_to_level_result(slot, target_lufs, save, o))
                .collect()),
            Err(e) if e == leveller::CANCELLED => {
                let _ = on_result.send(SceneLevelProgressItem {
                    scene_slot: session::BASE_SCENE_SLOT,
                    status: "cancelled".to_string(),
                    result: None,
                    message: Some(e),
                });
                Ok(Vec::new())
            }
            Err(e) => Err(e),
        };
        match Session::connect().and_then(|mut s| s.set_reamp_mode(false)) {
            Ok(_) => log::info!("level_scenes_apply_batched: final re-amp OFF sent"),
            Err(e) => log::warn!("level_scenes_apply_batched: final re-amp OFF failed ({e})"),
        }
        result
    })
    .await
}

/// Map a [`leveller::BatchedSceneOutcome`] onto the frontend's `LevelResult`
/// contract (the batched runner's outcome is per-scene; `verify_lufs` carries
/// the final measured window).
fn outcome_to_level_result(
    slot: u32,
    target_lufs: f64,
    save: bool,
    o: &leveller::BatchedSceneOutcome,
) -> leveller::LevelResult {
    let lufs = o.final_lufs.unwrap_or(f64::NAN);
    leveller::LevelResult {
        slot,
        ref_level: o.final_level.unwrap_or(0.0),
        measured_lufs: lufs,
        constant_c: f64::NAN,
        final_level: o.final_level.unwrap_or(0.0),
        target_lufs,
        predicted_lufs: lufs,
        clamped: o.clamped,
        saved: save,
        verify_lufs: o.final_lufs,
        iterations: o.windows.max(o.writes),
        dynamic_spread_lu: o.dynamic_spread_lu,
        clamp_reason: o.clamp_reason.clone(),
        verify_by_ear: o.verify_by_ear,
    }
}

/// Headroom (LU) below the quietest-capable preset's ceiling when auto-picking
/// the setlist common target. Small margin so the floor preset isn't clamped.
const SETLIST_HEADROOM_LU: f64 = 1.0;

/// One preset in a setlist leveling job: its slot + the instrument profile's
/// topology (resolved to that instrument's stimulus).
#[derive(serde::Deserialize)]
struct SetlistJobEntry {
    slot: u32,
    topology_id: Option<String>,
    calibration_lufs: Option<f32>,
}

/// Level a whole setlist to one common loudness target so switching presets (and
/// instruments) on stage causes no jump. Measures every preset's ceiling, picks a
/// target just below the quietest, and applies it to all. Like `level_preset`, it
/// releases the app's seize, runs, then re-establishes the UI session.
#[tauri::command]
async fn level_setlist(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    entries: Vec<SetlistJobEntry>,
    save: bool,
) -> Result<leveller::SetlistResult, String> {
    if entries.is_empty() {
        return Err("no presets selected to level".to_string());
    }
    // Resolve each entry's stimulus path + playback compensation on the UI
    // thread (needs AppHandle; the store is read ONCE for the whole setlist).
    // The common target stays one loudness; a bass entry's offset rides its own
    // effective target inside the leveller.
    let playback = profiles::load(&app)
        .map(|s| s.playback_level)
        .unwrap_or_default();
    let resolved: Vec<(u32, String, Option<f32>, f64)> = entries
        .into_iter()
        .map(|e| {
            let offset_lu = profiles::playback_offset_lu(
                playback,
                stimulus_instrument(e.topology_id.as_deref()),
            );
            resolve_stimulus(&app, None, e.topology_id)
                .map(|p| (e.slot, p, e.calibration_lufs, offset_lu))
        })
        .collect::<Result<_, _>>()?;
    with_released_seize(state.session.clone(), move || {
        // Own each stimulus (calibrated if the profile has a real-output level),
        // then borrow into entries for the leveller.
        let stims: Vec<(u32, Vec<f32>, f64)> = resolved
            .into_iter()
            .map(|(slot, path, cal, off)| {
                read_stimulus_calibrated(&path, cal).map(|s| (slot, s, off))
            })
            .collect::<Result<_, _>>()?;
        let lvl_entries: Vec<leveller::SetlistEntry> = stims
            .iter()
            .map(|(slot, s, off)| leveller::SetlistEntry {
                slot: *slot,
                stimulus: s,
                offset_lu: *off,
            })
            .collect();
        leveller::level_setlist(&lvl_entries, SETLIST_HEADROOM_LU, 0.5, save)
    })
    .await
}

/// What one Tier-2 calibration measured, plus its two quality caveats.
/// Mirrored in `src/lib/types.ts` (`CalibrateResult`).
#[derive(Debug, Clone, Copy, Serialize)]
struct CalibrateResult {
    /// Measured K-weighted loudness of the dry capture (stored on the profile).
    lufs: f32,
    /// The dry tap (USB-Out 3, no limiter) hit 0 dBFS — the measurement is biased
    /// LOW (clipped transients flatten the brightness K-weighting credits).
    clipped: bool,
    /// The topology stimulus cannot be scaled up to `lufs` without clipping (the
    /// 0.99 peak cap in `read_stimulus_calibrated_with_shortfall`): leveling will
    /// drive the amp this many LU softer than the real instrument. `None` = reachable.
    stimulus_shortfall_lu: Option<f32>,
}

/// Tier-2 calibration: capture the dry instrument (USB-Out 3) for `secs` while
/// the user plays their real guitar, measure its K-weighted loudness (LUFS), store
/// it on the profile's `calibration_lufs`, and return the measured value plus the
/// clip/stimulus-ceiling caveats. The device must be in normal mode with the
/// guitar in the front INSTRUMENT input.
#[tauri::command]
async fn calibrate_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    secs: f32,
) -> Result<CalibrateResult, String> {
    let app2 = app.clone();
    with_released_seize(state.session.clone(), move || {
        // Force normal mode so the dry instrument flows on USB-Out 3.
        if let Ok(mut s) = Session::connect() {
            let _ = s.set_reamp_mode(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        let cap = audio::capture_input(secs.clamp(2.0, 30.0), 48_000)?;

        let peak = cap.channel_peak(audio::DRY_INSTRUMENT_IN_CH);
        if peak < 1e-4 {
            return Err("no instrument signal captured — play continuously during \
                        calibration (guitar in the front INSTRUMENT input, volume up)"
                .to_string());
        }
        // K-weighted loudness (perceptual), not flat RMS — see read_stimulus_calibrated.
        let lufs =
            lufs::measure_mono(&cap.channel(audio::DRY_INSTRUMENT_IN_CH), 48_000)?.integrated_lufs;
        if !lufs.is_finite() {
            return Err("captured signal too quiet to measure — play louder/longer".to_string());
        }
        let lufs = lufs as f32;

        let mut store = profiles::load(&app2)?;
        let p = store
            .profiles
            .iter_mut()
            .find(|p| p.id == profile_id)
            .ok_or_else(|| format!("unknown profile '{profile_id}'"))?;
        p.calibration_lufs = Some(lufs);
        let topology_id = p.topology_id.clone();
        profiles::save(&app2, &store)?;

        // Best-effort caveats (calibration is already persisted; a WAV-resolution
        // failure must not fail the command).
        let stimulus_shortfall_lu = resolve_stimulus(&app2, None, Some(topology_id))
            .and_then(|path| read_stimulus_calibrated_with_shortfall(&path, Some(lufs)))
            .map(|(_, shortfall)| shortfall)
            .unwrap_or(None);
        Ok(CalibrateResult {
            lufs,
            clipped: peak >= 0.99,
            stimulus_shortfall_lu,
        })
    })
    .await
}

// ─── OFFLINE library + bulk-run commands ─────────────────────────────────────────

/// Block-category map for facet indexing. TODO: derive amp/cab categories
/// from the firmware `product_profile.json`; until then facets index blocks/IRs/SICs
/// /name/level/template (everything except the amp/cab classification).
pub(crate) fn library_categories() -> search::CategoryMap {
    search::CategoryMap::new()
}

/// Zero-padded epoch-millis id for a bulk run (sortable, collision-free per ms).
fn now_stamp_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("run-{ms:020}")
}

/// Build `PresetTarget`s for the selected list indices from the imported library.
/// Errors if any index is not a matched, writable record (the destructive-op guard:
/// unmatched/ambiguous records have `list_index = None` and never reach here).
pub(crate) fn targets_from_library(
    lib: &library::Library,
    selection: &[u32],
) -> Result<Vec<bulkrun::PresetTarget>, String> {
    selection
        .iter()
        .map(|&idx| {
            lib.records
                .iter()
                .find(|r| r.list_index == Some(idx))
                .ok_or_else(|| {
                    format!("list index {idx} is not a matched, writable library record")
                })
                .and_then(|r| r.to_target())
        })
        .collect()
}

/// Construct the right `PresetIo` for a run's path (built fresh per run; both are
/// stateless and open their own device connections).
pub(crate) fn io_for_path(path: bulk_cmd::IoPath) -> Box<dyn bulkrun::PresetIo> {
    match path {
        bulk_cmd::IoPath::Live => Box::new(preset_io::LiveIo),
        bulk_cmd::IoPath::Offline => Box::new(preset_io::OfflineIo),
    }
}

/// Import a folder of Pro-Control-exported `.preset` files as the canonical library
/// and reconcile it against the live device slot list. Returns the reconcile report
/// (matched / unmatched / ambiguous) for the user to confirm before any write.
#[tauri::command]
async fn import_library(
    folder: String,
    state: State<'_, AppState>,
) -> Result<library::ReconcileReport, String> {
    let folder_path = std::path::PathBuf::from(&folder);
    let (mut records, errors) =
        library::load_library_from_dir(&folder_path, &library_categories())?;
    if !errors.is_empty() {
        log::warn!(
            "[import_library] {} file(s) skipped: {errors:?}",
            errors.len()
        );
    }
    let device_list: Vec<PresetEntry> = with_released_seize(state.session.clone(), || {
        Session::connect()?.list_my_presets()
    })
    .await
    .unwrap_or_default();
    let report = library::reconcile_with_device(&mut records, &device_list);
    *lock_ok(&state.library) = Some(library::Library {
        folder: folder_path,
        records,
    });
    Ok(report)
}

/// The imported library's records (decoded JSON omitted from the wire — it's large
/// and the UI doesn't need it; bulk ops resolve it backend-side).
#[tauri::command]
fn library_records(state: State<'_, AppState>) -> Result<Vec<library::LibraryRecord>, String> {
    let guard = lock_ok(&state.library);
    let lib = guard
        .as_ref()
        .ok_or("no library imported — import a .preset folder first")?;
    Ok(lib.records.clone())
}

/// Filter args mirroring `search::Filter` (which has no serde derive).
#[derive(serde::Deserialize, Default)]
struct FilterArgs {
    name_substr: Option<String>,
    amp: Option<String>,
    block: Option<String>,
    ir: Option<String>,
    sic: Option<String>,
    level_lt: Option<f64>,
    level_gt: Option<f64>,
}

/// List indices of library records matching `filter` (selection feeder).
/// Only **writable** (matched) records are returned — unmatched/ambiguous ones
/// (sentinel `u32::MAX`) are dropped so they can't be selected for a write.
#[tauri::command]
fn library_filter(filter: FilterArgs, state: State<'_, AppState>) -> Result<Vec<u32>, String> {
    let guard = lock_ok(&state.library);
    let lib = guard.as_ref().ok_or("no library imported")?;
    let f = search::Filter {
        name_substr: filter.name_substr,
        amp: filter.amp,
        block: filter.block,
        ir: filter.ir,
        sic: filter.sic,
        level_lt: filter.level_lt,
        level_gt: filter.level_gt,
    };
    let records: Vec<search::PresetRecord> =
        lib.records.iter().map(|r| r.search_record()).collect();
    Ok(search::filter_records(&records, &f)
        .into_iter()
        .map(|r| r.list_index)
        .filter(|&i| i != u32::MAX)
        .collect())
}

/// Preview a bulk operation over a selection — per-preset change list, writes nothing.
#[tauri::command]
fn bulk_dry_run(
    selection: Vec<u32>,
    op: bulk_cmd::OpSpec,
    state: State<'_, AppState>,
) -> Result<Vec<bulkrun::DryRunEntry>, String> {
    let guard = lock_ok(&state.library);
    let lib = guard.as_ref().ok_or("no library imported")?;
    let targets = targets_from_library(lib, &selection)?;
    let operation = bulk_cmd::build_operation(&op)?;
    Ok(bulkrun::dry_run(&targets, operation.as_ref()))
}

// ─── Block templates (persisted in the app config dir, applied via OpSpec::ApplyBlock) ─

/// Path to the persisted block-template library (app config dir).
fn block_lib_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|e| format!("app config dir: {e}"))?
        .join("block_library.json"))
}

/// The saved block templates.
#[tauri::command]
fn list_block_templates(app: tauri::AppHandle) -> Result<Vec<blocklib::BlockTemplate>, String> {
    Ok(blocklib::load_library_from_path(&block_lib_path(&app)?).unwrap_or_default())
}

/// Capture the first block of `model` from a source preset into the library under
/// `name` (replaces a same-named entry). PURE capture + persist to disk; no device.
#[tauri::command]
fn save_block_template(
    app: tauri::AppHandle,
    source_list_index: u32,
    model: String,
    name: String,
    state: State<'_, AppState>,
) -> Result<Vec<blocklib::BlockTemplate>, String> {
    let template = {
        let guard = lock_ok(&state.library);
        let lib = guard.as_ref().ok_or("no library imported")?;
        let rec = lib
            .records
            .iter()
            .find(|r| r.list_index == Some(source_list_index))
            .ok_or_else(|| format!("source preset {source_list_index} not found / not matched"))?;
        let v: serde_json::Value =
            serde_json::from_str(&rec.decoded_json).map_err(|e| e.to_string())?;
        blocklib::capture_block(&v, &model, &name)
            .ok_or_else(|| format!("source preset has no block of model {model:?}"))?
    };
    let path = block_lib_path(&app)?;
    let mut lib = blocklib::load_library_from_path(&path).unwrap_or_default();
    lib.retain(|t| t.name != name); // replace a same-named template
    lib.push(template);
    blocklib::save_library_to_path(&path, &lib)?;
    Ok(lib)
}

// ─── Create variants (create = append-import) ────────────────────────────────────

/// Serde-friendly mirror of `variants::VariantEdit`.
#[derive(serde::Deserialize, Clone)]
#[serde(tag = "type")]
enum VariantEditArg {
    SetParam {
        model: String,
        param: String,
        value: f64,
    },
    ReplaceBlock {
        from: String,
        to: String,
    },
    SetBpm {
        bpm: f64,
    },
}

#[derive(serde::Deserialize, Clone)]
struct RecipeArg {
    name_suffix: String,
    edits: Vec<VariantEditArg>,
}
impl RecipeArg {
    fn to_recipe(&self) -> variants::Recipe {
        variants::Recipe {
            name_suffix: self.name_suffix.clone(),
            edits: self
                .edits
                .iter()
                .map(|e| match e.clone() {
                    VariantEditArg::SetParam {
                        model,
                        param,
                        value,
                    } => variants::VariantEdit::SetParam {
                        model,
                        param,
                        value,
                    },
                    VariantEditArg::ReplaceBlock { from, to } => {
                        variants::VariantEdit::ReplaceBlock { from, to }
                    }
                    VariantEditArg::SetBpm { bpm } => variants::VariantEdit::SetBpm(bpm),
                })
                .collect(),
        }
    }
}

/// Create a variant on the device (LIVE, append-only): clone + recipe -> .preset bytes
/// -> `import_preset` (the device files it at the next empty slot — variants carry no
/// inherited Song membership, so an append is correct, NOT an in-place overwrite).
/// HW-validation pending.
#[tauri::command]
async fn create_variant(
    source_list_index: u32,
    recipe: RecipeArg,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let variant_bytes = {
        let guard = lock_ok(&state.library);
        let lib = guard.as_ref().ok_or("no library imported")?;
        let rec = lib
            .records
            .iter()
            .find(|r| r.list_index == Some(source_list_index))
            .ok_or_else(|| format!("source preset {source_list_index} not found / not matched"))?;
        let source: serde_json::Value =
            serde_json::from_str(&rec.decoded_json).map_err(|e| e.to_string())?;
        let variant = variants::apply_recipe(&source, &recipe.to_recipe())?;
        backup::xor_jld(
            serde_json::to_string(&variant)
                .map_err(|e| e.to_string())?
                .as_bytes(),
        )
    };
    with_released_seize(state.session.clone(), move || {
        let reported = Session::connect()?.import_preset(&variant_bytes)?;
        Ok(match reported {
            Some((le, slot)) => {
                format!("variant imported (device reported listEnum {le}, slot {slot})")
            }
            None => "variant imported (device gave no slot echo — re-list to confirm)".to_string(),
        })
    })
    .await
}

// ─── Bulk rename (apply = LIVE) ──────────────────────────────────────────────────

/// Max preset-name length for the rename validator. Generous pending an exact HW
/// limit (better not to falsely flag than to block a valid name).
const RENAME_MAX: usize = 60;

/// Serde-friendly mirror of `rename::RenameSpec` (which has no serde derive).
#[derive(serde::Deserialize, Clone)]
#[serde(tag = "type")]
enum RenameSpecArg {
    FindReplace { from: String, to: String },
    Template { pattern: String },
    Number { width: usize, start: u32 },
}
impl RenameSpecArg {
    fn to_spec(&self) -> rename::RenameSpec {
        match self.clone() {
            RenameSpecArg::FindReplace { from, to } => rename::RenameSpec::FindReplace { from, to },
            RenameSpecArg::Template { pattern } => rename::RenameSpec::Template { pattern },
            RenameSpecArg::Number { width, start } => rename::RenameSpec::Number { width, start },
        }
    }
}

#[derive(serde::Serialize, Clone)]
struct RenameRow {
    list_index: Option<u32>,
    name: String,
    new_name: String,
    /// Set on a no-op rename (new == old) or a validation failure; the row is then skipped on apply.
    note: Option<String>,
}

/// Compute new names for the selection (PURE — no device). The `{n}` token / Number
/// offset use each preset's position within the selection.
fn rename_rows(
    lib: &library::Library,
    selection: &[u32],
    spec: &rename::RenameSpec,
) -> Vec<RenameRow> {
    selection
        .iter()
        .enumerate()
        .filter_map(|(i, idx)| {
            let r = lib.records.iter().find(|r| r.list_index == Some(*idx))?;
            let new_name = rename::apply_rename(&r.display_name, i, spec);
            let note = if new_name == r.display_name {
                Some("unchanged".to_string())
            } else {
                rename::validate_name(&new_name, RENAME_MAX).err()
            };
            Some(RenameRow {
                list_index: r.list_index,
                name: r.display_name.clone(),
                new_name,
                note,
            })
        })
        .collect()
}

#[derive(serde::Serialize)]
struct RenameApplyRow {
    list_index: u32,
    new_name: String,
    applied: bool,
    error: Option<String>,
}

/// Apply the rename to each selected preset on the device (LIVE): per preset
/// `load_preset → rename_current_preset → save_current_preset` (the PC "save under a
/// new name" pair). Rows with a validation note / no-op are skipped.
#[tauri::command]
async fn bulk_rename(
    selection: Vec<u32>,
    spec: RenameSpecArg,
    state: State<'_, AppState>,
) -> Result<Vec<RenameApplyRow>, String> {
    let rows: Vec<RenameRow> = {
        let guard = lock_ok(&state.library);
        let lib = guard.as_ref().ok_or("no library imported")?;
        rename_rows(lib, &selection, &spec.to_spec())
    };
    // Only valid, changed, matched rows are applied.
    let jobs: Vec<(u32, String, String)> = rows
        .into_iter()
        .filter(|r| r.note.is_none())
        .filter_map(|r| r.list_index.map(|i| (i, r.name, r.new_name)))
        .collect();
    if jobs.is_empty() {
        return Err("nothing to rename (all rows unchanged or invalid)".into());
    }
    with_released_seize(state.session.clone(), move || {
        let mut out = Vec::new();
        for (idx, name_before, new_name) in jobs {
            let mut row = RenameApplyRow {
                list_index: idx,
                new_name: new_name.clone(),
                applied: false,
                error: None,
            };
            let res = (|| -> Result<(), String> {
                let mut s = Session::connect()?;
                // Load (accumulating the PresetLoaded echo) then CONFIRM the target is
                // active before renaming+saving — a dropped load would otherwise
                // rename+save the WRONG preset over this slot (same fix as the
                // single-preset rename_save_preset path).
                s.clear_raw();
                s.send_and_collect(&proto::load_preset((idx + 1) as u64, 1), 200)?;
                s.confirm_active(idx, Some(&name_before))?;
                s.rename_current_preset(&new_name)?;
                s.save_current_preset(idx)?; // persist (rename = save-under-new-name)
                Ok(())
            })();
            match res {
                Ok(()) => row.applied = true,
                Err(e) => row.error = Some(e),
            }
            out.push(row);
        }
        Ok(out)
    })
    .await
}

// ─── Audition (MEASURE — re-amp render → WAV data URL for playback) ──────────────

/// Standard-alphabet base64 (no padding omitted) — small + dependency-free, for
/// embedding a rendered WAV as a `data:` URL the webview can play.
fn base64_encode(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(A[(n >> 18 & 63) as usize] as char);
        out.push(A[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            A[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            A[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Encode mono f32 samples as a 32-bit-float WAV (in memory).
fn wav_bytes(samples: &[f32], rate: u32) -> Result<Vec<u8>, String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut cursor = std::io::Cursor::new(Vec::<u8>::new());
    {
        let mut w =
            hound::WavWriter::new(&mut cursor, spec).map_err(|e| format!("wav writer: {e}"))?;
        for &s in samples {
            w.write_sample(s).map_err(|e| format!("wav write: {e}"))?;
        }
        w.finalize().map_err(|e| format!("wav finalize: {e}"))?;
    }
    Ok(cursor.into_inner())
}

/// Re-amp a preset and return its processed audio as a `data:audio/wav;base64,…` URL
/// the frontend `<audio>` element can play (MEASURE — drives the device,
/// HW-pending). A/B and before/after are a later refinement (render two + compare).
#[tauri::command]
async fn audition_render(
    app: tauri::AppHandle,
    slot: u32,
    topology_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    // Cache hit → return the already-rendered clip, skipping the re-amp pass.
    let cache_key = audition::clip_key(slot, topology_id.as_deref().unwrap_or("default"));
    if let Some(url) = lock_ok(&state.clip_cache).get(&cache_key) {
        return Ok(url);
    }
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    let url = with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        let (samples, rate) = leveller::capture_samples(slot, &stim, 0.5)?;
        let wav = wav_bytes(&samples, rate)?;
        Ok(format!("data:audio/wav;base64,{}", base64_encode(&wav)))
    })
    .await?;
    state
        .clip_cache
        .lock()
        .unwrap()
        .insert(&cache_key, url.clone());
    Ok(url)
}

// ─── Spectrum report (MEASURE — re-amp capture + band analysis) ──────────────────

/// Per-band energies + tonal flags for one preset.
#[derive(serde::Serialize)]
struct SpectrumResult {
    bands: Vec<f64>,
    flags: Vec<String>,
}

/// Re-amp a preset and analyze its captured spectrum (MEASURE — drives the device;
/// HW-validation pending). Reuses the leveller's validated capture sequence.
#[tauri::command]
async fn spectrum_scan(
    app: tauri::AppHandle,
    slot: u32,
    topology_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<SpectrumResult, String> {
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        let (samples, rate) = leveller::capture_samples(slot, &stim, 0.5)?;
        let bands = spectrum::band_energies(&samples, rate as f32, &spectrum::default_bands());
        let flags = spectrum::tonal_flags(&bands);
        Ok(SpectrumResult { bands, flags })
    })
    .await
}

/// EQ-match: source vs reference spectra + the per-band gain deltas that move
/// source toward reference, with a preview of the matched spectrum.
#[derive(serde::Serialize)]
struct EqMatchResult {
    source_bands: Vec<f64>,
    reference_bands: Vec<f64>,
    distance: f64,
    deltas: Vec<f64>,
    matched_bands: Vec<f64>,
}

/// Re-amp two presets and compute the EQ-match from `source` toward `reference`
/// (MEASURE — drives the device; HW-validation pending).
#[tauri::command]
async fn eq_match(
    app: tauri::AppHandle,
    source_slot: u32,
    reference_slot: u32,
    topology_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<EqMatchResult, String> {
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        let cfg = spectrum::default_bands();
        let (s, sr) = leveller::capture_samples(source_slot, &stim, 0.5)?;
        let source_bands = spectrum::band_energies(&s, sr as f32, &cfg);
        let (r, rr) = leveller::capture_samples(reference_slot, &stim, 0.5)?;
        let reference_bands = spectrum::band_energies(&r, rr as f32, &cfg);
        let distance = spectrum::spectral_distance(&source_bands, &reference_bands);
        let deltas = spectrum::eq_match_deltas(&source_bands, &reference_bands);
        let matched_bands = spectrum::apply_deltas(&source_bands, &deltas);
        Ok(EqMatchResult {
            source_bands,
            reference_bands,
            distance,
            deltas,
            matched_bands,
        })
    })
    .await
}

/// Re-amp a target + candidate presets and rank the candidates by spectral distance
/// to the target, nearest first ("best match" — MEASURE; HW-pending).
#[tauri::command]
async fn rank_candidates(
    app: tauri::AppHandle,
    target_slot: u32,
    candidate_slots: Vec<u32>,
    topology_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<spectrum::SicRank>, String> {
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        let cfg = spectrum::default_bands();
        let (t, tr) = leveller::capture_samples(target_slot, &stim, 0.5)?;
        let target = spectrum::band_energies(&t, tr as f32, &cfg);
        let mut cands = Vec::with_capacity(candidate_slots.len());
        for slot in candidate_slots {
            let (c, cr) = leveller::capture_samples(slot, &stim, 0.5)?;
            cands.push((
                format!("slot {slot}"),
                spectrum::band_energies(&c, cr as f32, &cfg),
            ));
        }
        Ok(spectrum::rank_sics(&target, &cands))
    })
    .await
}

// ─── Firmware-migration analysis over the imported library (no device) ────────────

/// One preset affected by a firmware migration: the in-use block models it
/// references that are absent from the target catalog.
#[derive(serde::Serialize)]
struct MigrationRow {
    list_index: Option<u32>,
    name: String,
    affected_blocks: Vec<String>,
}

/// Scan the imported library against a target firmware's `target_catalog` (valid block
/// model ids) and list presets that reference models the target no longer has
/// (OFFLINE read-only). The user resolves them via Block Replace.
#[tauri::command]
fn migration_scan(
    target_catalog: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<MigrationRow>, String> {
    let guard = lock_ok(&state.library);
    let lib = guard.as_ref().ok_or("no library imported")?;
    // The "old" catalog is every block model actually used across the library.
    let mut in_use: std::collections::BTreeSet<String> = Default::default();
    for r in &lib.records {
        for b in &r.facets.blocks {
            in_use.insert(b.clone());
        }
    }
    let old: Vec<String> = in_use.into_iter().collect();
    let diff = migration::diff_catalogs(&old, &target_catalog);
    let removed: std::collections::HashSet<String> = diff.removed.into_iter().collect();
    // Index by array position (stable identity), then map results back to records.
    let presets: Vec<(u32, serde_json::Value)> = lib
        .records
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            serde_json::from_str(&r.decoded_json)
                .ok()
                .map(|v| (i as u32, v))
        })
        .collect();
    Ok(migration::scan_affected(&presets, &removed)
        .into_iter()
        .map(|(pos, blocks)| {
            let r = &lib.records[pos as usize];
            MigrationRow {
                list_index: r.list_index,
                name: r.display_name.clone(),
                affected_blocks: blocks,
            }
        })
        .collect())
}

/// The migration plan: how the catalog changed + the concrete block swaps to run.
#[derive(serde::Serialize)]
struct MigrationPlan {
    classified: migration::ClassifiedDiff,
    plan: Vec<migration::Replacement>,
}

/// Build the classified catalog diff + per-preset replacement plan from a target
/// catalog + a user rename map (matched presets only, keyed by real list_index).
fn compute_migration_plan(
    lib: &library::Library,
    target_catalog: &[String],
    rename_map: &std::collections::BTreeMap<String, String>,
) -> MigrationPlan {
    let mut in_use: std::collections::BTreeSet<String> = Default::default();
    for r in &lib.records {
        for b in &r.facets.blocks {
            in_use.insert(b.clone());
        }
    }
    let old: Vec<String> = in_use.into_iter().collect();
    let diff = migration::diff_catalogs(&old, target_catalog);
    let classified = migration::classify(&diff, rename_map);
    let removed: std::collections::HashSet<String> = diff.removed.iter().cloned().collect();
    let presets: Vec<(u32, serde_json::Value)> = lib
        .records
        .iter()
        .filter_map(|r| r.list_index.zip(serde_json::from_str(&r.decoded_json).ok()))
        .collect();
    let affected = migration::scan_affected(&presets, &removed);
    let plan = migration::plan_replacements(&affected, rename_map);
    MigrationPlan { classified, plan }
}

/// Preview the renamed/removed/added block ids + the planned swaps for a
/// target firmware catalog and a rename map (pure, no device).
#[tauri::command]
fn migration_plan(
    target_catalog: Vec<String>,
    rename_map: std::collections::BTreeMap<String, String>,
    state: State<'_, AppState>,
) -> Result<MigrationPlan, String> {
    let guard = lock_ok(&state.library);
    let lib = guard.as_ref().ok_or("no library imported")?;
    Ok(compute_migration_plan(lib, &target_catalog, &rename_map))
}

/// One preset's migration-apply outcome.
#[derive(serde::Serialize)]
struct MigrationApplyRow {
    list_index: u32,
    swaps: usize,
    applied: bool,
    error: Option<String>,
}

/// Snapshot a preset's pre-edit JSON before an in-place migration write, so a mid-run
/// failure still leaves it revertible (bulkrun's AC2 discipline, which `migration_apply`
/// bypasses by calling `replace_inplace_core` directly). `Err` ⇒ DO NOT WRITE. `source`
/// is `"offline-file"`: migration edits a complete `.preset` from the offline library.
fn snapshot_before_migrate(
    backup_dir: &std::path::Path,
    list_index: u32,
    display_name: &str,
    before_json: &str,
) -> Result<std::path::PathBuf, String> {
    // list_enum 1 = My Presets; source "offline-file" = migration edits a complete
    // `.preset` from the offline library.
    bulkrun::save_pre_write_snapshot(
        backup_dir,
        1,
        list_index,
        display_name,
        "offline-file",
        before_json,
    )
}

/// Apply the migration plan: per affected preset, swap each renamed block
/// (Block Replace defaults) and re-import OFFLINE in place. `dry_run` previews the swap
/// counts without writing (HW-pending — device write gated by the read-only policy).
#[tauri::command]
async fn migration_apply(
    target_catalog: Vec<String>,
    rename_map: std::collections::BTreeMap<String, String>,
    dry_run: bool,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<MigrationApplyRow>, String> {
    // Build the plan + each affected preset's (original json, edited bytes) under the lock.
    let mut jobs: Vec<(u32, String, usize, String, Vec<u8>)> = Vec::new();
    {
        let guard = lock_ok(&state.library);
        let lib = guard.as_ref().ok_or("no library imported")?;
        let plan = compute_migration_plan(lib, &target_catalog, &rename_map).plan;
        // Group the plan by preset, then apply all its swaps to one decoded copy.
        let mut by_preset: std::collections::BTreeMap<u32, Vec<migration::Replacement>> =
            Default::default();
        for r in plan {
            by_preset.entry(r.list_index).or_default().push(r);
        }
        for (li, reps) in by_preset {
            let Some(rec) = lib.records.iter().find(|r| r.list_index == Some(li)) else {
                continue;
            };
            let before_json = rec.decoded_json.clone();
            let mut v: serde_json::Value =
                serde_json::from_str(&before_json).map_err(|e| e.to_string())?;
            let swaps: usize = reps
                .iter()
                .map(|r| migration::apply_replacement(&mut v, r))
                .sum();
            let bytes = backup::xor_jld(
                serde_json::to_string(&v)
                    .map_err(|e| e.to_string())?
                    .as_bytes(),
            );
            jobs.push((li, rec.display_name.clone(), swaps, before_json, bytes));
        }
    }
    // Dry run — preview the swap counts without touching the device or a backup.
    if dry_run {
        return Ok(jobs
            .into_iter()
            .map(|(li, _name, swaps, _before, _bytes)| MigrationApplyRow {
                list_index: li,
                swaps,
                applied: false,
                error: None,
            })
            .collect());
    }
    // A write must be revertible: snapshot each preset's pre-edit state before its
    // in-place re-import (bulkrun's AC2, which migration bypasses by calling
    // replace_inplace_core directly). Resolve the backups dir once up front so a whole
    // run refuses rather than writing the first preset unbacked.
    let backup_dir = backup::backups_dir(&app)?;
    let mut rows = Vec::with_capacity(jobs.len());
    for (li, display_name, swaps, before_json, bytes) in jobs {
        // AC2 — snapshot before writing; refuse the write if it fails (no backup = not
        // revertible). Uses the ORIGINAL decoded_json, not the edited bytes.
        if let Err(e) = snapshot_before_migrate(&backup_dir, li, &display_name, &before_json) {
            rows.push(MigrationApplyRow {
                list_index: li,
                swaps,
                applied: false,
                error: Some(format!("snapshot: {e} — NOT written (kept revertible)")),
            });
            continue;
        }
        let outcome = with_released_seize(state.session.clone(), move || {
            replace_inplace_core(li, &bytes)
        })
        .await;
        match outcome {
            Ok(o) if o.edit_landed => rows.push(MigrationApplyRow {
                list_index: li,
                swaps,
                applied: true,
                error: None,
            }),
            Ok(_) => rows.push(MigrationApplyRow {
                list_index: li,
                swaps,
                applied: false,
                error: Some("edit did not land".into()),
            }),
            Err(e) => rows.push(MigrationApplyRow {
                list_index: li,
                swaps,
                applied: false,
                error: Some(e),
            }),
        }
    }
    Ok(rows)
}

#[derive(serde::Serialize)]
struct BulkApplyResult {
    run_id: String,
    report: bulkrun::RunReport,
}

/// Apply a bulk operation to a selection, snapshotting each preset first, and record
/// the run so it can be reverted. Runs with the app's HID seize released (the io
/// adapters open their own connections).
#[tauri::command]
async fn bulk_apply(
    app: tauri::AppHandle,
    selection: Vec<u32>,
    op: bulk_cmd::OpSpec,
    backup: bool,
    state: State<'_, AppState>,
) -> Result<BulkApplyResult, String> {
    let targets = {
        let guard = lock_ok(&state.library);
        let lib = guard.as_ref().ok_or("no library imported")?;
        targets_from_library(lib, &selection)?
    };
    if targets.is_empty() {
        return Err("selection is empty (no matched, writable presets)".into());
    }
    let io_path = op.io_path();
    let op_label = bulk_cmd::build_operation(&op)?.label();
    let backup_dir = if backup {
        backup::backups_dir(&app)?
    } else {
        std::env::temp_dir().join("tmp-companion-nobackup")
    };
    let run_id = now_stamp_id();

    // Build the operation INSIDE the blocking closure (OpSpec is Send; a boxed
    // Operation is not), so the closure stays Send for spawn_blocking.
    let report = with_released_seize(state.session.clone(), move || {
        let operation = bulk_cmd::build_operation(&op)?;
        let mut io = io_for_path(io_path);
        Ok(bulkrun::apply(
            &targets,
            operation.as_ref(),
            io.as_mut(),
            &backup_dir,
        ))
    })
    .await?;

    lock_ok(&state.runs).insert(
        run_id.clone(),
        bulk_cmd::StoredRun {
            report: report.clone(),
            op_label,
        },
    );
    Ok(BulkApplyResult { run_id, report })
}

/// Revert a recorded run — restore every touched preset to its pre-run snapshot.
#[tauri::command]
async fn bulk_revert(
    run_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<bulkrun::RevertEntry>, String> {
    let stored = {
        let guard = lock_ok(&state.runs);
        guard
            .get(&run_id)
            .cloned()
            .ok_or_else(|| format!("no run {run_id} to revert"))?
    };
    let report = stored.report;
    with_released_seize(state.session.clone(), move || {
        // Revert ALWAYS restores via OFFLINE re-import of the captured original
        // `.preset` (the snapshot is a complete offline-file). This is correct for
        // both forward paths: a LIVE (ParamEdit) run can't be reverted by `LiveIo`
        // — its `write` diffs before-vs-after, and on revert both are the snapshot,
        // so the diff is empty and the device keeps the applied change while falsely
        // reporting `restored`. OfflineIo re-imports the original bytes in place
        // (identity-preserving), which undoes either forward path.
        let mut io = io_for_path(bulk_cmd::IoPath::Offline);
        Ok(bulkrun::revert(&report, io.as_mut()))
    })
    .await
}

pub(crate) fn format_dry_run(entries: &[bulkrun::DryRunEntry]) -> String {
    let mut s = format!("[probe bulk dry-run] {} preset(s)\n", entries.len());
    for e in entries {
        s += &format!(
            "  slot {:>3} {:<24} {:?}{}\n",
            e.list_index,
            e.display_name,
            e.status,
            match &e.error {
                Some(err) => format!("  ERROR: {err}"),
                None if !e.changes.is_empty() => format!("  ({} field(s) change)", e.changes.len()),
                None => String::new(),
            }
        );
    }
    s
}


// ─── Loudness audit ──────────────────────────────────────────────────────────────

/// Re-amp each selected preset and flag clipping + loudness outliers (vs the
/// median), the gain-stage audit (MEASURE — drives the device; HW-pending).
#[tauri::command]
async fn audit_loudness(
    app: tauri::AppHandle,
    slots: Vec<u32>,
    topology_id: Option<String>,
    outlier_lu: f64,
    state: State<'_, AppState>,
) -> Result<Vec<lint::Finding>, String> {
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        let mut measures = Vec::with_capacity(slots.len());
        for slot in slots {
            let (samples, rate) = leveller::capture_samples(slot, &stim, 0.5)?;
            let peak = samples.iter().fold(0f32, |m, &s| m.max(s.abs()));
            let peak_dbfs = if peak > 0.0 {
                20.0 * (peak as f64).log10()
            } else {
                -120.0
            };
            let loud = lufs::measure_mono(&samples, rate)?;
            measures.push(lint::AuditMeasure {
                list_index: slot,
                peak_dbfs,
                loudness_lufs: loud.integrated_lufs,
            });
        }
        Ok(lint::audit_measures(&measures, outlier_lu))
    })
    .await
}

// ─── Song-assignment device WRITE (SongMessage 14–17) ────────────────────────────

/// Bind a user preset (+ scene) to a Song row on the device — `assignSongPreset`.
/// `user_list_index` is 0-based (session applies the device +1). DEVICE WRITE.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn song_assign(
    song_slot: u32,
    song_preset_slot: u32,
    user_list_index: u32,
    footswitch_label: String,
    footswitch_color: u32,
    preset_scene_slot: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        Session::connect()?.assign_song_preset(
            song_slot,
            song_preset_slot,
            user_list_index,
            &footswitch_label,
            footswitch_color,
            preset_scene_slot,
        )
    })
    .await
}

/// Empty a Song row on the device — `clearSongPreset`. DEVICE WRITE.
#[tauri::command]
async fn song_clear(
    song_slot: u32,
    song_preset_slot: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        Session::connect()?.clear_song_preset(song_slot, song_preset_slot)
    })
    .await
}

/// Reorder a Song row on the device — `moveSongPreset`. DEVICE WRITE.
#[tauri::command]
async fn song_move(
    song_slot: u32,
    old: u32,
    new: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        Session::connect()?.move_song_preset(song_slot, old, new)
    })
    .await
}

/// Swap two Song rows on the device — `swapSongPreset`. DEVICE WRITE.
#[tauri::command]
async fn song_swap(
    song_slot: u32,
    a: u32,
    b: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        Session::connect()?.swap_song_preset(song_slot, a, b)
    })
    .await
}

/// Measure each scene's ceiling loudness (re-amp + `loadScene` per scene)
/// and return the per-scene gain offsets to a common target (MEASURE — drives the
/// device; HW-pending). Supersedes hand-entered C values when hardware is present.
#[tauri::command]
async fn level_scenes(
    app: tauri::AppHandle,
    slot: u32,
    scene_count: u32,
    topology_id: Option<String>,
    headroom_lu: f64,
    state: State<'_, AppState>,
) -> Result<Vec<f64>, String> {
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        let cs = leveller::capture_scene_ceilings(slot, scene_count, &stim)?;
        scenes::normalize_scene_targets(&cs, headroom_lu)
            .ok_or_else(|| "no finite scene loudness measured".to_string())
    })
    .await
}

/// List the bulk-run backup snapshots on disk (newest first), as file paths — the
/// audit trail of what `bulk_apply` snapshotted. A missing dir = no backups.
#[tauri::command]
fn list_snapshots(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    Ok(backup::list_snapshots_in_dir(&backup::backups_dir(&app)?)?
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        // Frontend (console/error) + backend `log::*` records → OS log dir
        // (~/Library/Logs/dev.tmpcompanion.app/) and stdout. Gives render
        // crashes and device errors an on-disk trace.
        .plugin(
            tauri_plugin_log::Builder::new()
                // `Builder::new()` already ships DEFAULT_LOG_TARGETS = [Stdout, LogDir]
                // and `.target()` APPENDS — so re-adding the same two duplicated every
                // record (each line written 2× to BOTH the log file and stdout). Clear
                // the defaults first, then set exactly the two sinks we want.
                .clear_targets()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::LogDir { file_name: None },
                ))
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                ))
                .level(log::LevelFilter::Info)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            app_info,
            connect_device,
            list_presets,
            list_samples,
            list_pickup_topologies,
            get_store,
            save_profiles,
            save_targets,
            set_playback_level,
            calibrate_profile,
            level_preset,
            level_setlist,
            list_level_blocks,
            import_library,
            library_records,
            library_filter,
            bulk_dry_run,
            bulk_apply,
            bulk_revert,
            migration_scan,
            bulk_rename,
            create_variant,
            list_block_templates,
            save_block_template,
            spectrum_scan,
            audition_render,
            eq_match,
            rank_candidates,
            migration_plan,
            migration_apply,
            audit_loudness,
            list_snapshots,
            song_assign,
            song_clear,
            song_move,
            song_swap,
            level_scenes,
            level_scenes_apply,
            level_scenes_apply_batched,
            cancel_scene_leveling,
            cancel_preset_leveling,
            level_footswitches_apply,
            cancel_footswitch_leveling,
            read_active_preset,
            current_graph,
            request_scene_list,
            stop_live_sync,
            list_songs,
            load_preset_on_amp,
            delete_preset,
            move_preset,
            rename_save_preset,
            load_scene_on_amp,
            read_setlists,
            list_setlist_songs,
            add_song,
            rename_song,
            remove_song,
            set_song_notes,
            set_song_bpm,
            add_setlist,
            rename_setlist,
            remove_setlist,
            add_setlist_song,
            remove_setlist_song,
            move_setlist_song,
            create_song_full,
            update_song_full,
            add_setlist_songs,
            read_preset_scenes,
            scan_preset_scenes,
            cancel_scene_scan,
            read_library_via_backup,
            list_saved_blocks,
            list_user_irs,
            bulk_replace_live,
            cancel_bulk_replace,
            copy_apply,
            cancel_copy_apply
        ])
        // Native macOS menu. Setting a menu replaces the default, so the standard
        // App / Edit / Window submenus are rebuilt explicitly (Edit is load-bearing
        // — copy/paste in the rename fields ride its predefined items). The
        // non-affiliation notice lives in the standard "About TMP Companion" panel
        // via AboutMetadata; the leveling explainer is in-app (Level tab), so there
        // is no custom Help submenu.
        .menu(|handle| {
            use tauri::menu::{AboutMetadataBuilder, MenuBuilder, SubmenuBuilder};
            let about = AboutMetadataBuilder::new()
                .name(Some("TMP Companion"))
                // ponytail: omit `version` (NSAboutPanelOptionApplicationVersion, the
                // parenthetical) — macOS already shows the bundle's short version, so
                // setting it too renders the redundant `Version 0.1.0 (0.1.0)`.
                .short_version(Some(env!("CARGO_PKG_VERSION")))
                // The dev binary has no bundle icon, so the panel would show a
                // generic folder — set it explicitly (same art as the Dock icon).
                .icon(tauri::image::Image::from_bytes(include_bytes!("../icons/dock.png")).ok())
                // macOS draws `copyright` as the small line and `credits` as the
                // body. Copyright = the real © line; the affiliation + trademark
                // notice is the body.
                .copyright(Some("© 2026 Pedro Cavadas"))
                .credits(Some(
                    "Fender, Tone Master Pro, and other amp, cabinet, and effect \
                     names are trademarks of their respective owners, used \
                     nominatively to describe compatibility and lineage. \
                     Independent project — not affiliated with Fender Musical \
                     Instruments Corporation.",
                ))
                .build();
            let app_menu = SubmenuBuilder::new(handle, "TMP Companion")
                .about_with_text("About TMP Companion", Some(about))
                .separator()
                .hide()
                .hide_others()
                .show_all()
                .separator()
                .quit()
                .build()?;
            let edit = SubmenuBuilder::new(handle, "Edit")
                .undo()
                .redo()
                .separator()
                .cut()
                .copy()
                .paste()
                .select_all()
                .build()?;
            let window = SubmenuBuilder::new(handle, "Window")
                .minimize()
                .maximize()
                .separator()
                .fullscreen()
                .close_window()
                .build()?;
            MenuBuilder::new(handle)
                .items(&[&app_menu, &edit, &window])
                .build()
        })
        .setup(|app| {
            // Confirms the logger is live (and gives the log file a deterministic
            // first line). Subsequent warn/error from the device + frontend paths
            // append here too.
            log::info!("TMP Companion {} started", env!("CARGO_PKG_VERSION"));
            // Dock icon for `tauri dev` (the raw binary has no bundle .icns).
            #[cfg(target_os = "macos")]
            dock::set_dock_icon();
            // Hotplug watcher: attach/detach events + dead-seize cleanup.
            use tauri::Manager;
            let session = app.state::<AppState>().session.clone();
            watcher::spawn(app.handle().clone(), session.clone());
            // Device monitor: app-level `connect_device` enables it, then the monitor
            // owns the idle seize with a dense ~250 ms heartbeat, publishes the startup
            // snapshot, and mirrors unsolicited unit pushes as tmp://live-preset /
            // live-scene / scene-list / signal-chain / sync. It coexists with commands
            // via the pause-then-ack protocol inside `lock_device_op`, and only opens
            // HID while `AppState.session` is None.
            monitor::spawn(app.handle().clone(), session);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Offline-UI-e2e backend (`--features e2e`): a windowless MockRuntime app whose REAL
/// commands are invoked over HTTP by the Playwright `bridge-client`. The transport
/// factory routes every device open to a shared `SimDevice`, a fixture startup snapshot
/// makes the app appear connected (no monitor thread), and the bulk backup is served
/// from the built fixture blob — so the real React UI in Chromium drives the real Rust
/// backend down to the (faked) unit. No window, no HTTP-framework dependency: a localhost
/// `std::net` server wrapping `tauri::test::get_ipc_response`. Request/response only —
/// the V1 Copy/Level journeys complete on the command's return value, not on Channels.
/// The one source of truth for the e2e mode: `TMP_E2E_ONLINE` set ⇒ drive the REAL device
/// (no SimDevice factory, real re-amp, real device backup); unset ⇒ the offline fake. Read
/// by `run_e2e_server`, the `/sim/reset` guard, and `audio::reamp_capture`.
#[cfg(feature = "e2e")]
pub(crate) fn e2e_online() -> bool {
    std::env::var("TMP_E2E_ONLINE").is_ok()
}

#[cfg(feature = "e2e")]
pub fn run_e2e_server() {
    use std::net::TcpListener;

    let online = e2e_online();
    // OFFLINE only: default the backup fixture so `read_library_via_backup` decodes it
    // through the real backup path. ONLINE must stream the REAL device backup, so the var
    // must be UNSET — affirmatively CLEAR it (don't just skip the default), or a stale
    // `TMP_E2E_BACKUP_FIXTURE` inherited from a prior offline shell would silently divert
    // the online tier to the fixture instead of the plugged-in unit's real library.
    if online {
        std::env::remove_var("TMP_E2E_BACKUP_FIXTURE");
    } else if std::env::var("TMP_E2E_BACKUP_FIXTURE").is_err() {
        std::env::set_var(
            "TMP_E2E_BACKUP_FIXTURE",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../e2e/fixtures/backup-fixture.bin"
            ),
        );
    }
    // The leveling stimulus (MockRuntime can't resolve bundle resources) — a committed WAV.
    if std::env::var("TMP_E2E_STIMULUS").is_err() {
        std::env::set_var(
            "TMP_E2E_STIMULUS",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/resources/samples/guitar-humbucker.wav"
            ),
        );
    }
    // ONLINE (`TMP_E2E_ONLINE=1`): drive the REAL device — no transport factory, so every
    // Session opens real `Hid`. One real handshake seeds the startup snapshot so
    // connect/list serve it (no Wry-typed monitor on the MockRuntime). The default OFFLINE
    // path installs the `SimDevice` factory + fixture snapshot instead. The server keeps
    // serving either way (a device-absent online run surfaces the error to the spec).
    if online {
        match e2e_seed_online_snapshot() {
            Ok(()) => eprintln!("e2e_server: ONLINE — seeded snapshot from the real device"),
            Err(e) => eprintln!("e2e_server: ONLINE — device handshake failed: {e}"),
        }
    } else {
        e2e_install_offline_fake();
    }

    let app = tauri::test::mock_builder()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            app_info,
            connect_device,
            list_presets,
            read_library_via_backup,
            copy_apply,
            cancel_copy_apply,
            get_store,
            level_preset,
            list_songs,
            read_setlists,
            add_song,
            rename_song,
            remove_song,
            create_song_full,
            update_song_full,
            list_setlist_songs,
            add_setlist,
            rename_setlist,
            remove_setlist,
            add_setlist_songs,
            remove_setlist_song,
            move_setlist_song,
            e2e_seed_scenario,
            e2e_clear_preset,
            e2e_load_preset,
            e2e_reamp_off
        ])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build e2e mock app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("build e2e webview");

    let port: u16 = std::env::var("TMP_E2E_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(7600);
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind e2e port");
    let mode = if online {
        "ONLINE / real device"
    } else {
        "offline / SimDevice"
    };
    eprintln!("e2e_server: listening on http://127.0.0.1:{port} ({mode})");
    // Single-threaded serial accept: Playwright runs `workers:1` (the device is
    // exclusive-seize), and the webview handle stays on this one thread.
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        e2e_handle_conn(&webview, &mut stream);
    }
}

/// Install the offline fake: the shared `SimDevice` transport factory + a fixture startup
/// snapshot (keep its presets in sync with `e2e/fixtures/backup-fixture.bin` — the
/// build script lists them). Re-callable to reset device state between specs (`/sim/reset`).
#[cfg(feature = "e2e")]
fn e2e_install_offline_fake() {
    // SHOWCASE (`TMP_E2E_SHOWCASE=1`, the marketing-screenshot tour): drive the whole app
    // from the curated, non-personal `e2e/fixtures/showcase/` library instead of the
    // 3-preset test scenario. The committed `.bin` (built from `showcase.json` by the
    // `build_showcase_fixture` generator) is the SAME device-backup shape, so `read_*`
    // decode it unchanged; we just point the env there, derive the preset list + hero graph
    // from it, and seed the curated song/setlist names. No test-gate path touches this.
    if std::env::var("TMP_E2E_SHOWCASE").is_ok() {
        e2e_install_showcase();
        return;
    }
    let sim = crate::sim_device::SimDevice::new();
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sim.clone())));
    // The 3 scenario presets at slots 400/401/402 — same slots the online tier seeds by
    // cloning, and the same presets baked into the backup fixture, so one set of specs
    // runs in both modes. `ensureScenario` finds them present offline and skips seeding.
    let presets = vec![
        session::PresetEntry {
            slot: 400,
            name: "E2E Reference".into(),
        },
        session::PresetEntry {
            slot: 401,
            name: "E2E Target 1".into(),
        },
        session::PresetEntry {
            slot: 402,
            name: "E2E Target 2".into(),
        },
    ];
    MONITOR_ENABLED.store(true, SeqCst);
    monitor::e2e_install_snapshot(Some("1.8.45".into()), presets, None);
}

/// Install the SHOWCASE offline fake (marketing screenshots). Points the backup-fixture
/// env at the curated `.bin`, decodes it to derive the preset list + the active preset's
/// hero graph (so the Level chain paints), and seeds the SimDevice with the curated
/// song/setlist names read from `showcase.json` (those names aren't in the decoded archive
/// result; the `.bin` carries presets + graph + song↔preset bindings). Best-effort: any
/// read failure falls back to an empty library rather than panicking the server.
#[cfg(feature = "e2e")]
fn e2e_install_showcase() {
    let bin = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../e2e/fixtures/showcase/showcase-fixture.bin"
    );
    std::env::set_var("TMP_E2E_BACKUP_FIXTURE", bin);
    let json = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../e2e/fixtures/showcase/showcase.json"
    );

    // The single curated source — parsed once (`firmware`, `activeSlot`, song/setlist names
    // come from here; presets + graph come from the `.bin`). Null on any read/parse error,
    // so the indexing below all yields empties and the server still boots.
    let spec = std::fs::read_to_string(json)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .unwrap_or(serde_json::Value::Null);
    let names = |key: &str| {
        spec[key]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        x.as_str()
                            .or_else(|| x["name"].as_str())
                            .map(str::to_string)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    // Curated song / setlist names for the live read-back (Songs tab main list).
    let sim = crate::sim_device::SimDevice::new().with_songs(names("songs"), names("setlists"));
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sim.clone())));

    // Preset list + hero graph, decoded from the same curated `.bin`.
    let (presets, graph) = match std::fs::read(bin)
        .ok()
        .and_then(|b| read_backup_archive(&b).ok())
    {
        Some(res) => {
            // PresetEntry.slot is the 0-based LIST INDEX; the DB `slot` (i64) is index + 1.
            let presets = res
                .presets
                .iter()
                .map(|r| session::PresetEntry {
                    slot: (r.slot - 1).max(0) as u32,
                    name: r.name.clone(),
                })
                .collect();
            // Hero = the `activeSlot` preset's routed graph.
            let active = spec["activeSlot"].as_u64().unwrap_or(0);
            let graph = res
                .presets
                .iter()
                .find(|r| r.slot as u64 == active)
                .map(|r| r.graph.clone());
            (presets, graph)
        }
        None => (Vec::new(), None),
    };

    let firmware = spec["firmware"].as_str().unwrap_or("1.8.45").to_string();
    MONITOR_ENABLED.store(true, SeqCst);
    monitor::e2e_install_snapshot(Some(firmware), presets, graph);
}

/// e2e ONLINE seam: one real-device handshake → install the startup snapshot (firmware +
/// My Presets) so `connect_device` / `list_presets` serve it WITHOUT a monitor thread; no
/// transport factory is installed, so every command opens the real seized `Hid`. The
/// graph stays `None` (the hero just won't paint a live chain); the journeys don't need
/// it. Requires the device plugged in + Pro Control closed.
#[cfg(feature = "e2e")]
fn e2e_seed_online_snapshot() -> Result<(), String> {
    let mut s = session::Session::connect_with_firmware()?;
    let fw = s.firmware_version();
    let presets = s.list_my_presets()?;
    drop(s); // release the seize; commands reopen via with_released_seize
    MONITOR_ENABLED.store(true, SeqCst);
    monitor::e2e_install_snapshot(fw, presets, None);
    Ok(())
}

/// Patch ONE slot's name in the startup snapshot's preset list so the UI's snapshot-backed
/// list (the Level tab) reflects a scratch-slot clone/clear immediately. Done locally from
/// the KNOWN write rather than a device re-read — `list_my_presets` lags its own writes
/// (read-after-write propagation), so an immediate re-read installs a stale list.
#[cfg(feature = "e2e")]
fn e2e_patch_snapshot_slot(slot: u32, name: &str) {
    let Some(snap) = monitor::startup_snapshot() else {
        return;
    };
    let mut presets = snap.presets;
    if let Some(e) = presets.iter_mut().find(|p| p.slot == slot) {
        e.name = name.to_string();
    }
    monitor::e2e_install_snapshot(snap.firmware, presets, snap.graph);
}

/// ONLINE-e2e DETERMINISTIC scratch setup: import the THREE committed scenario presets
/// (`e2e/fixtures/scenario-presets.json` — the SAME presetJsons baked into the offline
/// backup fixture) into their list indices (400/401/402). So both modes run the identical
/// fixed presets, validated against known blocks, rather than a clone of whatever is on the
/// unit. Each is placed in-place via [`replace_inplace_core`] (import → land → save to slot
/// → clear the scratch landing, guarded). The target slots start EMPTY (the 400+ scratch
/// zone); [`e2e_clear_preset`] returns them to empty. Idempotent at the spec layer
/// (`ensureScenario` skips when they already exist — i.e. offline).
#[cfg(feature = "e2e")]
#[tauri::command]
async fn e2e_seed_scenario(state: State<'_, AppState>) -> Result<(), String> {
    #[derive(serde::Deserialize)]
    struct ScenarioPreset {
        #[serde(rename = "listIndex")]
        list_index: u32,
        name: String,
        #[serde(rename = "presetJson")]
        preset_json: String,
    }
    let path = std::env::var("TMP_E2E_SCENARIO_PRESETS").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../e2e/fixtures/scenario-presets.json"
        )
        .into()
    });
    let raw = std::fs::read(&path).map_err(|e| format!("scenario presets {path}: {e}"))?;
    let presets: Vec<ScenarioPreset> =
        serde_json::from_slice(&raw).map_err(|e| format!("parse scenario presets: {e}"))?;
    with_released_seize(state.session.clone(), move || {
        for (i, p) in presets.iter().enumerate() {
            // ponytail: real-TMP HID open-lockout recovery, plus a deliberate fresh-connect
            // import. replace_inplace_core does its post-import "where did it land?" list read
            // on a FRESH Session::connect() — a full handshake forces the device to
            // re-enumerate the just-imported slot. A held single session is faster but its
            // re-arm list read does NOT reflect a fresh import (read-after-write lag; the
            // device only re-enumerates inside the recognized full handshake), so the scratch
            // slot is invisible and seeding fails. So we keep the fresh-connect path and pay
            // the open-lockout tax with an 8 s quiet gap between presets, which lets the
            // device recover (offline SimDevice has no lockout, and the offline specs use the
            // baked fixture, so this only bites online). ~92 s for three presets — fine for
            // one-time e2e setup; correctness over the held-session speedup.
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_secs(8));
            }
            // A `.preset` file is `xor_jld(compact JSON)`; `import_preset` adds the outer LZ4.
            let bytes = backup::xor_jld(p.preset_json.as_bytes());
            replace_inplace_core(p.list_index, &bytes)?;
            e2e_patch_snapshot_slot(p.list_index, &p.name);
        }
        Ok(())
    })
    .await
}

/// ONLINE-e2e scratch teardown: clear scratch slot `slot` (0-based list index), restoring
/// the empty state. SAFETY: refuses unless the slot currently holds `expect_name` (read in
/// the same session) — so a wrong index can never clear a real preset.
#[cfg(feature = "e2e")]
#[tauri::command]
async fn e2e_clear_preset(
    state: State<'_, AppState>,
    slot: u32,
    expect_name: String,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?;
        let list = s.list_my_presets_strict()?;
        let entry = list
            .get(slot as usize)
            .ok_or_else(|| format!("slot {slot} out of range"))?;
        if entry.name != expect_name {
            return Err(format!(
                "refusing to clear slot {slot}: expected '{expect_name}', found '{}'",
                entry.name
            ));
        }
        s.clear_user_preset(slot)?;
        e2e_patch_snapshot_slot(slot, "Empty");
        Ok(())
    })
    .await
}

/// ONLINE-e2e end-of-scenario state: recall a preset (0-based list index) on the unit so
/// the test leaves it on a known preset (001 = index 0). Non-destructive (a load, no save).
#[cfg(feature = "e2e")]
#[tauri::command]
async fn e2e_load_preset(state: State<'_, AppState>, slot: u32) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        Session::connect()?.load_preset(slot)
    })
    .await
}

/// ONLINE-e2e safety teardown: disengage re-amp on a fresh connection. The re-amp latch is
/// device-side and survives the HID release, so a Level run KILLED mid-capture (a Playwright
/// timeout tearing down the server) would otherwise leave the unit input-muted. The Level
/// flow's own in-session `set_reamp_mode(false)` doesn't run on an abrupt kill — this is the
/// belt-and-braces OFF the scenario teardown calls. No-op offline (the fake never engages
/// re-amp), so it's harmless on the offline path.
#[cfg(feature = "e2e")]
#[tauri::command]
async fn e2e_reamp_off(state: State<'_, AppState>) -> Result<(), String> {
    if !e2e_online() {
        return Ok(());
    }
    with_released_seize(state.session.clone(), move || {
        Session::connect()?.set_reamp_mode(false).map(|_| ())
    })
    .await
}

/// Parse one HTTP/1.1 request and reply. Routes: `POST /invoke` (the command bridge),
/// `POST /sim/reset` (fresh device state), `GET /health`, `OPTIONS` (CORS preflight).
#[cfg(feature = "e2e")]
fn e2e_handle_conn(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    stream: &mut std::net::TcpStream,
) {
    use std::io::{BufRead, BufReader, Read, Write};

    let Ok(clone) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(clone);
    let mut req_line = String::new();
    if reader.read_line(&mut req_line).is_err() {
        return;
    }
    let mut it = req_line.split_whitespace();
    let method = it.next().unwrap_or("").to_string();
    let path = it.next().unwrap_or("").to_string();
    let mut content_len = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            return;
        }
        let t = line.trim_end();
        if t.is_empty() {
            break;
        }
        if let Some(v) = t
            .strip_prefix("Content-Length:")
            .or_else(|| t.strip_prefix("content-length:"))
        {
            content_len = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; content_len];
    if content_len > 0 && reader.read_exact(&mut body).is_err() {
        return;
    }

    let (status, payload) = e2e_route(webview, &method, &path, &body);
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: content-type\r\nAccess-Control-Allow-Methods: POST,GET,OPTIONS\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        payload.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&payload);
    let _ = stream.flush();
}

/// Map a request to `(status, json body)`. `/invoke` wraps the command result in an
/// `{ok,data}` / `{ok,error}` envelope the bridge-client unwraps into resolve/reject.
#[cfg(feature = "e2e")]
fn e2e_route(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    method: &str,
    path: &str,
    body: &[u8],
) -> (&'static str, Vec<u8>) {
    use serde_json::json;
    if method == "OPTIONS" {
        return ("200 OK", Vec::new());
    }
    match (method, path) {
        ("GET", "/health") => ("200 OK", b"{\"ok\":true}".to_vec()),
        ("POST", "/sim/reset") => {
            // ONLINE: the real device IS the state — re-installing the offline fake (a
            // SimDevice factory) would clobber it, so the reset is a no-op online.
            if !e2e_online() {
                e2e_install_offline_fake();
            }
            ("200 OK", b"{\"ok\":true}".to_vec())
        }
        ("POST", "/invoke") => {
            let req: serde_json::Value = serde_json::from_slice(body).unwrap_or(json!({}));
            let cmd = req
                .get("cmd")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let args = req.get("args").cloned().unwrap_or(json!({}));
            let request = tauri::webview::InvokeRequest {
                cmd,
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            };
            let env = match tauri::test::get_ipc_response(webview, request) {
                Ok(b) => {
                    let data = b
                        .deserialize::<serde_json::Value>()
                        .unwrap_or(serde_json::Value::Null);
                    json!({ "ok": true, "data": data })
                }
                Err(e) => json!({ "ok": false, "error": e }),
            };
            ("200 OK", serde_json::to_vec(&env).unwrap_or_default())
        }
        _ => (
            "404 Not Found",
            b"{\"ok\":false,\"error\":\"not found\"}".to_vec(),
        ),
    }
}

#[cfg(all(test, feature = "e2e"))]
mod e2e_server_spike {
    use super::*;
    use tauri::test::MockRuntime;
    use tauri::WebviewWindow;

    /// The transport factory + startup snapshot are process-GLOBAL; cargo runs tests in
    /// parallel, so the factory-installing tests must hold this for their whole body or
    /// they stomp each other's fake (a hard-to-spot cross-contamination).
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        lock_ok(&SERIAL)
    }

    /// Invoke a command through the SAME IPC path the HTTP bridge uses: a JSON body in,
    /// the command's JSON response out (or its error value).
    fn invoke(
        webview: &WebviewWindow<MockRuntime>,
        cmd: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, serde_json::Value> {
        tauri::test::get_ipc_response(
            webview,
            tauri::webview::InvokeRequest {
                cmd: cmd.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            },
        )
        .map(|b| b.deserialize::<serde_json::Value>().expect("json body"))
    }

    /// The full OFFLINE Copy journey driven through the real backend exactly as the UI
    /// drives it — connect → list presets → read the library → copy_apply — with the
    /// device replaced by a `SimDevice` (via the transport factory) and the bulk backup
    /// replaced by the built fixture blob. This is "UI to unit" minus the browser: every
    /// command runs for real over the mock IPC; only the USB transport + the snapshot are
    /// faked. The HTTP bridge + Playwright layer reuses this exact wiring.
    #[test]
    fn offline_copy_journey_through_real_backend() {
        use std::sync::atomic::Ordering::SeqCst;
        let _serial = serial();

        // One shared fake: every Session::connect* (command lane) clones it.
        let sim = crate::sim_device::SimDevice::new();
        let sim_for_factory = sim.clone();
        crate::session::e2e_transport::set_factory(Box::new(move || {
            Box::new(sim_for_factory.clone())
        }));
        // The library read decodes the fixture blob through the real backup path.
        std::env::set_var(
            "TMP_E2E_BACKUP_FIXTURE",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../e2e/fixtures/backup-fixture.bin"
            ),
        );
        // Pre-fill the startup snapshot so connect/list serve it with no monitor thread —
        // the 3 scenario presets at slots 400/401/402 (matching the backup fixture).
        let presets = vec![
            crate::session::PresetEntry {
                slot: 400,
                name: "E2E Reference".into(),
            },
            crate::session::PresetEntry {
                slot: 401,
                name: "E2E Target 1".into(),
            },
            crate::session::PresetEntry {
                slot: 402,
                name: "E2E Target 2".into(),
            },
        ];
        MONITOR_ENABLED.store(true, SeqCst);
        monitor::e2e_install_snapshot(Some("1.8.45".into()), presets, None);

        let app = tauri::test::mock_builder()
            .manage(AppState::default())
            .invoke_handler(tauri::generate_handler![
                connect_device,
                list_presets,
                read_library_via_backup,
                copy_apply
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .expect("build webview");

        // 1. connect → the pre-filled snapshot (firmware).
        let conn = invoke(&webview, "connect_device", serde_json::json!({})).expect("connect");
        assert_eq!(
            conn.get("firmware").and_then(|v| v.as_str()),
            Some("1.8.45")
        );

        // 2. list presets → the snapshot's 3 fixture entries.
        let list = invoke(&webview, "list_presets", serde_json::json!({})).expect("list");
        assert_eq!(list.as_array().map(|a| a.len()), Some(3), "presets: {list}");

        // 3. read the library via the fixture backup → 3 rows, decoded graphs.
        let lib =
            invoke(&webview, "read_library_via_backup", serde_json::json!({})).expect("library");
        let rows = lib
            .get("presets")
            .and_then(|p| p.as_array())
            .expect("library presets array");
        assert_eq!(rows.len(), 3, "library rows: {lib}");
        assert!(
            rows.iter()
                .any(|r| r.get("graph").is_some_and(|g| !g.is_null())),
            "at least one library row carries a decoded signal graph: {lib}"
        );

        // 4. copy_apply a dry-run replace on the target → outcome "updated", NOTHING saved.
        // The job is the exact camelCase wire shape `CopyJob`/`CopyOp`/`CopyRepl` accept
        // (the input-only structs the frontend's `diffToOps` produces). The fake confirms
        // any structural edit, so the nodeId need not match a fixture node.
        let jobs = serde_json::json!([{
            "listIndex": 401,
            "name": "E2E Target 1",
            "ops": [{
                "kind": "replace",
                "group": "G1",
                "nodeId": "ACD_PhaserP90",
                "repl": { "kind": "model", "fenderId": "ACD_KingOfTone" }
            }]
        }]);
        let items = invoke(
            &webview,
            "copy_apply",
            serde_json::json!({ "jobs": jobs, "save": false, "onResult": "__CHANNEL__:0" }),
        )
        .expect("copy_apply");
        let items = items.as_array().expect("copy items");
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].get("outcome").and_then(|v| v.as_str()),
            Some("updated"),
            "copy outcome: {items:?}"
        );
        let ev = sim.events();
        assert!(
            ev.iter()
                .any(|e| matches!(e, crate::sim_device::SimEvent::Replace { .. })),
            "the replace reached the fake: {ev:?}"
        );
        assert!(
            !ev.iter()
                .any(|e| matches!(e, crate::sim_device::SimEvent::Saved(_))),
            "dry run must not save: {ev:?}"
        );
    }

    /// The Level journey's measure→solve→apply path runs end-to-end OFFLINE: the device
    /// goes through the `SimDevice` factory and the re-amp capture through the
    /// `--features e2e` audio fake (`audio::reamp_capture` returns the stimulus), so the
    /// leveler produces a finite `C` / final level with no hardware. Proves the audio
    /// seam the UI Level run depends on; loudness fidelity stays the online tier's job.
    #[test]
    fn offline_level_preset_runs_against_the_fake_audio() {
        let _serial = serial();
        let sim = crate::sim_device::SimDevice::new();
        let sim_for_factory = sim.clone();
        crate::session::e2e_transport::set_factory(Box::new(move || {
            Box::new(sim_for_factory.clone())
        }));

        // 0.5 s of a 440 Hz tone at 48 kHz — non-silent so the loudness meter is finite.
        let rate = 48_000usize;
        let stim: Vec<f32> = (0..rate / 2)
            .map(|i| 0.2 * (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin())
            .collect();

        let opts = crate::leveller::LevelOptions {
            save: false,
            verify: true,
            ..Default::default()
        };
        let r =
            crate::leveller::level_preset(0, &stim, -30.0, opts, || false).expect("level_preset");
        assert!(
            r.final_level.is_finite() && r.final_level > 0.0,
            "solved a finite level: {r:?}"
        );
        assert!(
            r.measured_lufs.is_finite(),
            "measured a finite loudness: {r:?}"
        );
        // Dry run — the fake recorded a level set but no save.
        let ev = sim.events();
        assert!(
            ev.iter()
                .any(|e| matches!(e, crate::sim_device::SimEvent::PresetLevel(_))),
            "the level setter reached the fake: {ev:?}"
        );
        assert!(
            !ev.iter()
                .any(|e| matches!(e, crate::sim_device::SimEvent::Saved(_))),
            "save:false must not save: {ev:?}"
        );
    }

    /// Songs CRUD through the real backend over the mock IPC: the SimDevice models the
    /// song wire protocol (list / add / rename / remove), so `list_songs` reads the seed,
    /// a write mutates it, and the read-back reflects the change — the Songs tab's
    /// read-after-write contract, with no hardware.
    #[test]
    fn offline_songs_crud_through_real_backend() {
        let _serial = serial();
        let sim = crate::sim_device::SimDevice::new();
        let sim_for_factory = sim.clone();
        crate::session::e2e_transport::set_factory(Box::new(move || {
            Box::new(sim_for_factory.clone())
        }));

        let app = tauri::test::mock_builder()
            .manage(AppState::default())
            .invoke_handler(tauri::generate_handler![
                list_songs,
                read_setlists,
                add_song,
                rename_song,
                remove_song
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .expect("build webview");

        // Seed: 2 songs, 1 setlist.
        let songs = invoke(&webview, "list_songs", serde_json::json!({})).expect("list_songs");
        assert_eq!(
            songs.as_array().map(|a| a.len()),
            Some(2),
            "seed songs: {songs}"
        );
        let setlists =
            invoke(&webview, "read_setlists", serde_json::json!({})).expect("read_setlists");
        assert_eq!(
            setlists.as_array().map(|a| a.len()),
            Some(1),
            "seed setlists: {setlists}"
        );

        // Add → read-back reflects it.
        let after_add = invoke(
            &webview,
            "add_song",
            serde_json::json!({ "name": "Soundcheck" }),
        )
        .expect("add_song");
        assert_eq!(
            after_add.as_array().map(|a| a.len()),
            Some(3),
            "after add: {after_add}"
        );
        assert!(sim.song_names().iter().any(|n| n == "Soundcheck"));

        // Remove the first → back to 2.
        let after_rm = invoke(
            &webview,
            "remove_song",
            serde_json::json!({ "slot": 1, "expectName": "Opening Set" }),
        )
        .expect("remove_song");
        assert_eq!(
            after_rm.as_array().map(|a| a.len()),
            Some(2),
            "after remove: {after_rm}"
        );
        assert!(!sim.song_names().iter().any(|n| n == "Opening Set"));
    }
}

#[cfg(test)]
mod audition_tests {
    use super::*;

    /// The exact JSON the "Copy blocks between presets" frontend sends for a
    /// `copy_apply` job deserializes into the [`CopyJob`]/[`CopyOp`]/[`CopyRepl`]
    /// shapes — replace (all three repl variants), insert (append + before-anchor), remove.
    #[test]
    fn copy_job_round_trips_from_frontend_json() {
        let json = r#"{
            "listIndex": 7,
            "name": "Lead Tone",
            "ops": [
                { "kind": "replace", "group": "G1", "nodeId": "ACD_TwinReverb",
                  "repl": { "kind": "model", "fenderId": "ACD_HiwattDR103CanMod" } },
                { "kind": "replace", "group": "M1", "nodeId": "ACD_CabSimTMS",
                  "repl": { "kind": "saved", "fenderId": "ACD_CabSimTMS", "index": 3 } },
                { "kind": "insert", "group": "G1", "beforeFenderId": "ACD_Comp",
                  "repl": { "kind": "ir", "fenderId": "ACD_UserIRTMS", "file": "Oversize.wav" } },
                { "kind": "insert", "group": "G2", "beforeFenderId": null,
                  "repl": { "kind": "model", "fenderId": "ACD_Klon" } },
                { "kind": "remove", "group": "G1", "nodeId": "ACD_Comp" }
            ]
        }"#;
        let job: CopyJob = serde_json::from_str(json).expect("CopyJob deserializes");
        assert_eq!(job.list_index, 7);
        assert_eq!(job.name, "Lead Tone");
        assert_eq!(job.ops.len(), 5);

        // op0: replace → model
        let CopyOp::Replace {
            group,
            node_id,
            repl,
        } = &job.ops[0]
        else {
            panic!("replace")
        };
        assert_eq!(group, "G1");
        assert_eq!(node_id, "ACD_TwinReverb");
        let CopyRepl::Model { fender_id } = repl else {
            panic!("model repl")
        };
        assert_eq!(fender_id, "ACD_HiwattDR103CanMod");

        // op1: replace → saved (index)
        let CopyOp::Replace { repl, .. } = &job.ops[1] else {
            panic!("replace")
        };
        let CopyRepl::Saved { fender_id, index } = repl else {
            panic!("saved repl")
        };
        assert_eq!(fender_id, "ACD_CabSimTMS");
        assert_eq!(*index, 3);

        // op2: insert BEFORE a FenderId → ir (file)
        let CopyOp::Insert {
            group,
            before_fender_id,
            repl,
        } = &job.ops[2]
        else {
            panic!("insert")
        };
        assert_eq!(group, "G1");
        assert_eq!(before_fender_id.as_deref(), Some("ACD_Comp"));
        let CopyRepl::Ir { fender_id, file } = repl else {
            panic!("ir repl")
        };
        assert_eq!(fender_id, "ACD_UserIRTMS");
        assert_eq!(file, "Oversize.wav");
        // The IR's insert id resolves to the placeholder, not the catalog id.
        assert_eq!(repl.insert_fender_id(), "ACD_UserIRTMS");

        // op3: insert APPEND (before = None) → model
        let CopyOp::Insert {
            before_fender_id,
            repl,
            ..
        } = &job.ops[3]
        else {
            panic!("insert")
        };
        assert!(before_fender_id.is_none(), "null beforeFenderId → append");
        assert_eq!(repl.insert_fender_id(), "ACD_Klon");

        // op4: remove
        let CopyOp::Remove { group, node_id } = &job.ops[4] else {
            panic!("remove")
        };
        assert_eq!(group, "G1");
        assert_eq!(node_id, "ACD_Comp");
    }

    /// A `copy_apply` Channel item carries the `BulkReplaceItem` fields plus an OPTIONAL
    /// `graph` that is OMITTED from the wire when `None` (`skip_serializing_if`), so the
    /// no-graph case mirrors `BulkReplaceItem` exactly and the TS `graph?` field stays
    /// optional.
    #[test]
    fn copy_apply_item_serializes_like_bulk_replace_item() {
        let item = CopyApplyItem {
            slot: 7,
            name: "Lead Tone".to_string(),
            outcome: "updated".to_string(),
            detail: "5 op(s)".to_string(),
            graph: None,
        };
        let v = serde_json::to_value(&item).unwrap();
        assert_eq!(v["slot"], 7);
        assert_eq!(v["name"], "Lead Tone");
        assert_eq!(v["outcome"], "updated");
        assert_eq!(v["detail"], "5 op(s)");
        assert!(
            v.get("graph").is_none(),
            "None graph is omitted from the wire"
        );
    }

    #[test]
    fn base64_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn wav_bytes_has_riff_header_and_round_trips() {
        let samples = [0.0f32, 0.5, -0.5, 1.0];
        let bytes = wav_bytes(&samples, 48000).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        // Decodes back to the same samples via hound.
        let mut rdr = hound::WavReader::new(std::io::Cursor::new(bytes)).unwrap();
        let got: Vec<f32> = rdr.samples::<f32>().map(|s| s.unwrap()).collect();
        assert_eq!(got, samples);
    }

}

/// End-to-end tests of the held-session Copy/Level orchestration over the in-memory
/// `sim_device::SimDevice` fake — NO hardware. These exercise the real command-layer
/// state machine (`copy_apply_one`'s load → re-arm → confirm-gated edits → identity-
/// preserving save) and the two safety invariants that previously only ever ran on the
/// device: a `presetError` is NEVER followed by a save, and the cold first edit's silent
/// drop is retried. The pure edit→op diff is unit-tested elsewhere (`copyModel` in
/// Vitest, `bulk_cmd` in Rust); this is the wire-driving the diff feeds.
#[cfg(test)]
mod copy_level_e2e_tests {
    use super::*;
    use crate::session::Session;
    use crate::sim_device::{SimDevice, SimEvent};

    fn model_replace(group: &str, node: &str, fender: &str) -> CopyOp {
        CopyOp::Replace {
            group: group.into(),
            node_id: node.into(),
            repl: CopyRepl::Model {
                fender_id: fender.into(),
            },
        }
    }

    fn one_replace_job(slot: u32, name: &str) -> CopyJob {
        CopyJob {
            list_index: slot,
            name: name.into(),
            ops: vec![model_replace("G1", "n2", "ACD_DeluxeReverb65")],
        }
    }

    /// Drive `copy_apply_one` over a configured fake device, returning the outcome item
    /// plus the ordered device actions the fake recorded.
    fn run_copy(sim: SimDevice, job: &CopyJob, save: bool) -> (CopyApplyItem, Vec<SimEvent>) {
        let mut s = Session::from_transport(Box::new(sim.clone()));
        let item = copy_apply_one(&mut s, job, save).unwrap();
        (item, sim.events())
    }

    #[test]
    fn copy_replace_happy_path_loads_confirms_renames_and_saves_in_order() {
        let (item, ev) = run_copy(SimDevice::new(), &one_replace_job(5, "Stadium Lead"), true);
        assert_eq!(item.outcome, "updated");
        assert_eq!(
            ev,
            vec![
                SimEvent::Loaded(5),
                SimEvent::Replace {
                    group: "G1".into(),
                    node_id: "n2".into(),
                    fender_id: "ACD_DeluxeReverb65".into(),
                },
                SimEvent::Renamed("Stadium Lead".into()),
                SimEvent::Saved(5),
            ]
        );
    }

    #[test]
    fn copy_multi_op_applies_remove_replace_insert_in_order_then_saves() {
        let job = CopyJob {
            list_index: 3,
            name: "Clean Verse".into(),
            ops: vec![
                CopyOp::Remove {
                    group: "G1".into(),
                    node_id: "n1".into(),
                },
                model_replace("G1", "n2", "ACD_Klon"),
                CopyOp::Insert {
                    group: "G1".into(),
                    before_fender_id: Some("ACD_Klon".into()),
                    repl: CopyRepl::Model {
                        fender_id: "ACD_TapeEcho".into(),
                    },
                },
            ],
        };
        let (item, ev) = run_copy(SimDevice::new(), &job, true);
        assert_eq!(item.outcome, "updated");
        assert_eq!(
            ev,
            vec![
                SimEvent::Loaded(3),
                SimEvent::Remove {
                    group: "G1".into(),
                    node_id: "n1".into(),
                },
                SimEvent::Replace {
                    group: "G1".into(),
                    node_id: "n2".into(),
                    fender_id: "ACD_Klon".into(),
                },
                SimEvent::Insert {
                    group: "G1".into(),
                    before: Some("ACD_Klon".into()),
                    fender_id: "ACD_TapeEcho".into(),
                },
                SimEvent::Renamed("Clean Verse".into()),
                SimEvent::Saved(3),
            ]
        );
    }

    #[test]
    fn copy_preset_error_is_never_followed_by_a_save() {
        // The device REJECTS the edit (presetError 53) — copy must report an error and
        // MUST NOT rename or save (an unconfirmed save corrupted a real preset).
        let (item, ev) = run_copy(
            SimDevice::new().with_reject_at(1),
            &one_replace_job(5, "Stadium Lead"),
            true,
        );
        assert_eq!(item.outcome, "error");
        assert!(item.detail.contains("NOT saved"), "detail: {}", item.detail);
        assert!(
            !ev.iter()
                .any(|e| matches!(e, SimEvent::Saved(_) | SimEvent::Renamed(_))),
            "a rejected edit must not save/rename: {ev:?}"
        );
    }

    #[test]
    fn copy_cold_first_edit_silent_drop_is_retried_then_saved() {
        // The first structural edit after a fresh load is silently DROPPED; the held
        // path retries it once and then confirms + saves.
        let (item, ev) = run_copy(
            SimDevice::new().with_drop_first(),
            &one_replace_job(5, "Stadium Lead"),
            true,
        );
        assert_eq!(item.outcome, "updated");
        let replaces = ev
            .iter()
            .filter(|e| matches!(e, SimEvent::Replace { .. }))
            .count();
        assert_eq!(replaces, 2, "the dropped edit + its retry: {ev:?}");
        assert!(ev.iter().any(|e| matches!(e, SimEvent::Saved(5))));
    }

    #[test]
    fn copy_dry_run_applies_edits_but_does_not_save() {
        let (item, ev) = run_copy(SimDevice::new(), &one_replace_job(5, "Stadium Lead"), false);
        assert_eq!(item.outcome, "updated");
        assert!(ev.iter().any(|e| matches!(e, SimEvent::Replace { .. })));
        assert!(
            !ev.iter()
                .any(|e| matches!(e, SimEvent::Saved(_) | SimEvent::Renamed(_))),
            "dry run must not persist: {ev:?}"
        );
    }

    #[test]
    fn copy_empty_op_list_is_skipped_without_touching_the_device() {
        let job = CopyJob {
            list_index: 2,
            name: "Untouched".into(),
            ops: vec![],
        };
        let (item, ev) = run_copy(SimDevice::new(), &job, true);
        assert_eq!(item.outcome, "skipped");
        assert!(ev.is_empty(), "no ops → no device traffic");
    }

    #[test]
    fn level_setter_roundtrip_echoes_and_records() {
        // The Level seam over the fake: set_preset_level's wire encoding round-trips and
        // the device's PresetLevelChanged(77) echo parses back to the value sent.
        let sim = SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        let echo = s.set_preset_level(0.5).unwrap();
        assert!((echo.expect("level echo") - 0.5).abs() < 1e-6);
        s.save_current_preset(7).unwrap();
        assert_eq!(
            sim.events(),
            vec![SimEvent::PresetLevel(0.5), SimEvent::Saved(7)]
        );
    }

    #[test]
    fn copy_partial_failure_saves_only_the_confirmed_targets() {
        // The multi-target copy loop runs every job on ONE held session. With the SECOND
        // target's edit rejected (presetError), the FIRST must still confirm + save and the
        // rejected one must NOT save — a batch is partial-success, never all-or-nothing, and
        // a rejected target never gets a wrong-content save. (The frontend then patches the
        // cache for the "updated" target only — CopyView.tsx's `outcome === "updated"` gate.)
        let sim = SimDevice::new().with_reject_at(2);
        let mut s = Session::from_transport(Box::new(sim.clone()));
        let a = copy_apply_one(&mut s, &one_replace_job(5, "Target A"), true).unwrap();
        let b = copy_apply_one(&mut s, &one_replace_job(6, "Target B"), true).unwrap();
        assert_eq!(a.outcome, "updated");
        assert_eq!(b.outcome, "error");
        let ev = sim.events();
        assert!(
            ev.contains(&SimEvent::Saved(5)),
            "confirmed target A must save: {ev:?}"
        );
        assert!(
            !ev.contains(&SimEvent::Saved(6)),
            "rejected target B must NOT save: {ev:?}"
        );
    }

    #[test]
    fn copy_apply_refuses_an_over_cap_insert_end_to_end() {
        // The device does NOT enforce the 5 firmware block-count caps (C-1, see
        // `blockcaps.rs`'s module docs) — this is the guard's REAL test: an over-cap
        // edit must be refused HERE, before it ever reaches the device, not by any
        // device response (the fake would happily confirm it, like the real firmware).
        let sim = SimDevice::new().with_preset_json(
            r#"{"audioGraph":{"guitarNodes":{"G1":[
                {"FenderId":"ACD_AC30BrilliantCabIR","nodeId":"n1"},
                {"FenderId":"ACD_AC30NormalCabIR","nodeId":"n2"}
            ]}}}"#,
        );
        let mut s = Session::from_transport(Box::new(sim.clone()));
        let job = CopyJob {
            list_index: 5,
            name: "Stadium Lead".into(),
            ops: vec![CopyOp::Insert {
                group: "G1".into(),
                before_fender_id: None,
                repl: CopyRepl::Model {
                    fender_id: "ACD_Ampeg66B15CabIR".into(), // a 3rd cabinet member
                },
            }],
        };
        let item = copy_apply_one(&mut s, &job, true).unwrap();
        assert_eq!(item.outcome, "error");
        assert!(
            item.detail.contains("ComboHalfStackCabinetsLimit"),
            "detail: {}",
            item.detail
        );
        let ev = sim.events();
        assert!(
            !ev.iter().any(|e| matches!(
                e,
                SimEvent::Insert { .. } | SimEvent::Saved(_) | SimEvent::Renamed(_)
            )),
            "an over-cap insert must never reach the device, let alone save: {ev:?}"
        );
    }
    // ── PR2: confirm-before-save write safety (Session::confirm_active) ──

    #[test]
    fn confirm_active_ok_when_slot_echo_matches() {
        // A load the device echoed (PresetLoaded) confirms via the SLOT identity.
        let sim = SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.send_and_collect(&crate::proto::load_preset(6, 1), 50)
            .unwrap(); // dev slot 6 = 0-based list index 5
        assert!(
            s.confirm_active(5, None).is_ok(),
            "slot echo should confirm: loaded={:?}",
            s.loaded_slot()
        );
        s.save_current_preset(5).unwrap();
        assert!(sim.events().iter().any(|e| matches!(e, SimEvent::Saved(5))));
    }

    #[test]
    fn confirm_active_errs_and_blocks_save_when_load_dropped() {
        // No PresetLoaded echo and no matching active name (a dropped load) ⇒ confirm
        // errs, and a caller using `?` never reaches the save (no wrong-content write).
        let sim = SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        let attempt = |s: &mut Session| -> Result<(), String> {
            s.confirm_active(7, Some("Target"))?;
            s.save_current_preset(7)?; // MUST NOT run
            Ok(())
        };
        assert!(attempt(&mut s).is_err(), "unconfirmed load must not save");
        assert!(
            !sim.events().iter().any(|e| matches!(e, SimEvent::Saved(_))),
            "no save on an unconfirmed load: {:?}",
            sim.events()
        );
    }

    #[test]
    fn confirm_active_errs_when_a_different_preset_is_active() {
        // The device says a DIFFERENT slot is active — a possibly-duplicate name must not
        // override the contradicting slot echo, so confirm errs (never edit the wrong one).
        let sim = SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.send_and_collect(&crate::proto::load_preset(4, 1), 50)
            .unwrap(); // dev slot 4 = 0-based list index 3
        assert!(
            s.confirm_active(5, Some("Target")).is_err(),
            "slot 3 active but target 5 — must not confirm"
        );
    }

    // ── PR2: migration snapshot-before-write (AC2) ──

    #[test]
    fn migration_refuses_to_snapshot_into_an_unwritable_dir() {
        // A path whose parent is not a directory ⇒ the snapshot fails; migration_apply
        // then skips the write and keeps the preset revertible.
        let bad = std::path::Path::new("/dev/null/tmp-companion-cannot-mkdir");
        let r = super::snapshot_before_migrate(bad, 3, "Cliff", r#"{"info":{"preset_id":"x"}}"#);
        assert!(r.is_err(), "unwritable backup dir must refuse: {r:?}");
    }

    #[test]
    fn migration_snapshot_captures_the_pre_edit_json() {
        let dir = std::env::temp_dir().join(format!(
            "tmp-companion-migsnap-{}",
            crate::bulkrun::now_stamp()
        ));
        let before = r#"{"info":{"preset_id":"abc","displayName":"Cliff"}}"#;
        let p = super::snapshot_before_migrate(&dir, 3, "Cliff", before).unwrap();
        let content = std::fs::read_to_string(&p).unwrap();
        assert!(
            content.contains("abc"),
            "snapshot must carry the pre-edit json: {content}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
