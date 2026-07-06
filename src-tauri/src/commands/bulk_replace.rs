//! Bulk Block Edit — live per-node replace across a selection + discovery.
#![allow(clippy::too_many_arguments)]
use crate::*;

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
/// Cooperative cancel for [`bulk_replace_live`] — set by `cancel_bulk_replace` (the
/// wizard's Stop), checked between presets so the held-session sweep stops WRITING the
/// remaining presets (not just hiding them in the UI). Presets already saved stay
/// changed; the rest are left untouched.
pub(crate) static BULK_REPLACE_CANCEL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Stop an in-flight [`bulk_replace_live`] sweep after the current preset. Lightweight
/// (just sets the flag) so it does NOT take the device-op lock — it must run while the
/// sweep holds it.
#[tauri::command]
pub(crate) fn cancel_bulk_replace() {
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
pub(crate) async fn bulk_replace_live(
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
pub(crate) struct ReplacePlan {
    pub(crate) list_index: u32,
    pub(crate) name: String,
    /// `(group, node_id)` of every node matching the requested `from_id`.
    pub(crate) targets: Vec<(String, String)>,
}

/// Read the selected slots ONCE on a single session and build the per-preset target
/// list. Field-8 reads are reliable on a clean session but flaky right after a
/// reconnect, so all reads are batched here, away from the edit reconnect churn.
pub(crate) fn discover_replace_plans(
    device_slots: &[u32],
    from_id: &str,
) -> Result<Vec<ReplacePlan>, String> {
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
pub(crate) fn blockcaps_pre_edit_roster(
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
pub(crate) fn blockcaps_replaced<'a>(
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
pub(crate) fn blockcaps_check(
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
pub(crate) fn blockcaps_advance(
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
pub(crate) fn repl_arg_fender_id(repl: &ReplArg) -> Option<&str> {
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
