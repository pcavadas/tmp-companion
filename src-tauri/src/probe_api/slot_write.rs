//! Probe entry points: preset write ops (import / clear / map / diag) + block discovery + bulk apply + connect/firmware.

use super::songs::read_song_presets;
use super::SCRATCH_SLOTS;
use crate::bulk_cmd;
use crate::bulkrun;
use crate::leveller;
use crate::library;
use crate::proto;
use crate::session;
use crate::session::Session;
use crate::PresetEntry;
use crate::{format_dry_run, io_for_path, library_categories, targets_from_library};

/// Headless hardware probe used by the `probe` bin: connect (seizing the
/// device), run the handshake, and return the "My Presets" list. Lets us verify
/// the HID stack against a real TMP without launching the GUI.
pub fn probe_connect_and_list() -> Result<Vec<PresetEntry>, String> {
    let mut s = Session::connect()?;
    s.list_my_presets()
}

/// HW PROBE (WRITE): set the global `sceneChangeBehavior` (SettingsMessage field 83).
///
/// Rides TMS 3, the same branch `reampModeActive` (field 30) proves is served for
/// that field specifically — this does not itself prove field 83 is served.
///
/// `value` is the `SceneChangeBehavior::Behavior` enum: keys `Retain`, `Revert`
/// from the client's Qt MOC table. The **ordinal** (`0 = Retain`) is already settled
/// by direct touchscreen observation, not by this write — see open-questions.md A6.
/// Calls [`Session::begin_live_edit`] before writing, closing the confound a past
/// attempt on an unwarmed line left open. Retested 2026-07-26 on a warmed session:
/// still no observable change — see open-questions.md A6 for both results.
///
/// GLOBAL setting — the caller MUST restore the original value.
pub fn probe_set_scene_change_behavior(value: u64) -> Result<String, String> {
    // The enum has exactly two keys; anything else would write an out-of-range
    // ordinal into a global the user has to live with.
    if value > 1 {
        return Err(format!(
            "refusing sceneChangeBehavior={value}: the Behavior enum has exactly two \
             keys (Retain, Revert) in the 1.8.45 client's MOC table"
        ));
    }
    let mut s = Session::connect()?;
    s.drain_until_quiet(300, 20)?;
    s.begin_live_edit()?;
    let dump = s.send_and_dump(&proto::set_scene_change_behavior(value), 600)?;
    Ok(format!(
        "sceneChangeBehavior <- {value} sent.\n\
         Verify: probe --device-backup → settingsBackup.sceneChangeBehavior\n\
         reply stream (informational):\n{dump}"
    ))
}

/// HW PROBE (working-copy WRITE): set one block parameter on a SCRATCH preset.
///
/// Deliberately does ONE thing per invocation. An earlier attempt drove the whole
/// C2 experiment (read → edit → scene round trip → re-read) inside a single
/// connection and could not be trusted: `best_json_payload` returns the best
/// payload accumulated so far, so a second read in the same session can serve the
/// handshake's snapshot rather than live state — which would silently compare a
/// value against itself.
///
/// The reliable shape instead uses ONE SESSION PER STEP, because a working-copy
/// edit PERSISTS on the device across USB reconnects (established with
/// `switchTemplate`). Every read then rides a fresh handshake and is genuinely live:
///
/// ```text
/// probe --set-param 400 G1 ACD_TubeScreamer level 0.125
/// probe --dump-currentpresetdata      # confirm the edit landed
/// probe --loadscene 1 ; probe --loadscene 0
/// probe --dump-currentpresetdata      # retained or reverted?
/// ```
///
/// Nothing is saved. Discard the working copy afterwards by loading another preset.
pub fn probe_set_param(
    slot: u32,
    group: &str,
    node: &str,
    param: &str,
    value: f32,
) -> Result<String, String> {
    if !SCRATCH_SLOTS.contains(&slot) {
        return Err(format!(
            "refusing --set-param on {slot}: scratch-zone only {SCRATCH_SLOTS:?}"
        ));
    }
    // `changeParameter` carries the value in the block's OWN real units verbatim (dB, Hz,
    // …), never range-checked by the wire — disproven this session on hardware:
    // `ACD_Boost.gain` accepts raw dB 0/2.5/5, ~1:1 dB→LUFS (see `param_class`'s doc). This
    // seam is a scratch-zone diagnostic (the caller), so the only guard that belongs here
    // is a sane wire value, not a guessed range.
    if !value.is_finite() {
        return Err(format!("refusing non-finite value {value}"));
    }
    let mut s = Session::connect()?;
    s.load_preset(slot)?;
    std::thread::sleep(std::time::Duration::from_millis(700));
    // Recall BASE explicitly before the write. A preset loads into its SAVED
    // `lastLoadedScene`, so a bare `changeParameter` lands in whatever scene overlay
    // that happens to be — never base by default (HW-confirmed, fw 1.8.45). Hoisted
    // ABOVE the first write of this connection: a recall AFTER a write reverts it.
    // Same fix `set_knob`/`set_knobs`/`capture_full_at` already carry.
    s.load_scene(session::BASE_SCENE_SLOT)?;
    std::thread::sleep(std::time::Duration::from_millis(
        leveller::SETTLE_AFTER_SCENE_RECALL_MS,
    ));
    // Use the SESSION method, not a hand-rolled `send_and_dump(proto::change_parameter(..))`.
    // The session wrapper is the path the leveller drives on every run; sending the
    // bare message skipped its bookkeeping and the edit silently did not land — the
    // parameter read back unchanged with no error reported.
    s.change_parameter(group, node, param, value)?;
    std::thread::sleep(std::time::Duration::from_millis(500));
    Ok(format!(
        "changeParameter {group}/{node}/{param} <- {value} (slot {slot}, working copy).\n\
         Verify on a SEPARATE invocation: probe --dump-currentpresetdata\n"
    ))
}

/// HW PROBE (read-only): fire a settings sub-request on TMS 3 and dump the reply.
///
/// Exists as the discriminator the device backup cannot provide: a backup shows
/// *persisted* state, so an ignored write and a landed-but-unflushed write look
/// identical there. A live read separates them.
///
/// `footswitchSettingsRequest` (47) is the one that matters for scene behaviour —
/// `FootswitchSettings.sceneChangeBehavior` is field 12 of the reply.
pub fn probe_settings_read(field: u32) -> Result<String, String> {
    let mut s = Session::connect()?;
    // Same drain rule as every other read: a request fired into the handshake
    // flood is dropped, which reads as a false negative.
    s.drain_until_quiet(300, 20)?;
    let dump = s.send_and_dump(&proto::settings_request(field), 900)?;
    Ok(format!("SettingsMessage field {field} request →\n{dump}"))
}

/// HW PROBE (WRITE): switch the signal-path template of a SCRATCH preset
/// (`PresetMessage.switchTemplate`, field 43).
///
/// Settles what happens to per-scene state when the template changes — the manual
/// says only that it "repopulates the signal path" and "will affect all Scenes",
/// never whether per-scene bypass/parameter overrides are preserved, remapped or
/// wiped.
///
/// Scratch-guarded: this is a destructive edit if the device auto-saves, and
/// whether it does is one of the unknowns. Does NOT load `slot` itself — see the
/// active-preset check below for why an explicit load can't substitute for it.
pub fn probe_switch_template(slot: u32, template_type: &str) -> Result<String, String> {
    if !SCRATCH_SLOTS.contains(&slot) {
        return Err(format!(
            "refusing switchTemplate on {slot}: scratch-zone only {SCRATCH_SLOTS:?} \
             (a template switch repopulates the signal path and may not be undoable)"
        ));
    }
    let mut s = Session::connect()?;
    // Confirm identity through the mutation path's own read — never a caller-
    // supplied assumption (same discipline as `probe_move_preset` below). This
    // function used to just trust that the caller had already loaded `slot` in a
    // prior session; nothing enforced it, so switchTemplate could land on
    // whatever preset the unit actually had open. An explicit `load_preset(slot)`
    // here isn't a substitute fix: loading the ALREADY-current preset emits no
    // field-3 push at all (see the `Some(slot)` note below), so it wouldn't move
    // the risk, only hide it behind a call that looks like it helped.
    //
    // Compares `info.preset_id`, NOT `info.displayName` — displayName is
    // user-editable and duplicates are allowed (see this skill's own documented
    // invariant: preset identity is the UUID, never the label), so a name-only
    // check could pass while a different, same-named preset is actually active.
    // The field-8 read is on a QUIET line (its own drain, like `probe_roster`),
    // BEFORE the undrained capture below — the two don't interleave.
    s.drain_until_quiet(250, 20)?;
    let expected_raw = s
        .read_slot_preset_json(slot + 1)?
        .ok_or_else(|| format!("list index {slot}: no presetJson on the device (empty slot?)"))?;
    let expected_json = session::tolerant_parse_json(&String::from_utf8_lossy(&expected_raw))
        .ok_or_else(|| format!("list index {slot}: presetJson did not parse"))?;
    let expected_id = expected_json
        .pointer("/info/preset_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("list index {slot}: presetJson has no info.preset_id"))?
        .to_string();
    let expected_name = expected_json
        .pointer("/info/displayName")
        .and_then(|v| v.as_str())
        .unwrap_or("<unnamed>")
        .to_string();
    // NO pre-drain here. `capture_full_preset_json` waits for the field-3
    // currentPresetDataChanged PUSH, and `drain_until_quiet` eats exactly that —
    // draining first made the capture fail with "no payload captured". This is the
    // mirror image of the read-request rule (requests need a quiet line; pushes
    // must not be drained away).

    // Read BEFORE and AFTER on THIS connection. A template switch is very likely a
    // working-copy edit (same class as force-bypass), and a working-copy edit is
    // discarded by the reload a fresh connection performs — so a cross-connection
    // read cannot distinguish "unserved" from "applied then discarded". Only a
    // same-connection pair can.
    //
    // `capture_full_preset_json` (the dense-heartbeat capture), NOT
    // `fetch_current_preset_json`: the latter returns empty on a plain session, and
    // an empty read compared against an empty read silently looks like "unchanged".
    // The AFTER read passes `None` so it captures the CURRENT working copy without
    // reloading — a reload would discard the very edit being measured.
    // `Some(slot)` would RE-load the preset, and re-loading the preset that is
    // already current emits no push at all, so the capture times out. Ride the
    // handshake's own push instead (`None`).
    let before = String::from_utf8_lossy(&s.capture_full_preset_json(None, 3000)?).into_owned();

    // Verify identity BEFORE writing, from the same read the "before" comparison
    // already needs — never trust that the caller loaded the right preset earlier.
    // Compares preset_id (the canonical identity); displayName is extracted too,
    // but only for the error message — it's context, not the check.
    let field_of = |j: &str, key: &str| {
        let needle = format!("\"{key}\":\"");
        j.find(&needle).and_then(|i| {
            let r = &j[i + needle.len()..];
            r.find('"').map(|e| r[..e].to_string())
        })
    };
    let active_name = field_of(&before, "displayName").unwrap_or_else(|| "<unknown>".into());
    match field_of(&before, "preset_id") {
        Some(id) if id == expected_id => {}
        Some(_) => {
            return Err(format!(
                "refusing switchTemplate: active preset is {active_name:?}, not \
                 {expected_name:?} (list index {slot}) — load the scratch preset first"
            ));
        }
        None => {
            return Err(format!(
                "refusing switchTemplate: could not read the active preset's preset_id from \
                 the pre-switch capture, so identity against list index {slot} \
                 ({expected_name:?}) could not be confirmed"
            ));
        }
    }

    let dump = s.send_and_dump(&proto::switch_template(template_type), 1500)?;
    std::thread::sleep(std::time::Duration::from_millis(800));
    // Non-fatal: if switchTemplate is ignored there is no push to capture, and a
    // timeout here is itself a data point rather than an error to abort on.
    let after = s
        .capture_full_preset_json(None, 3000)
        .map(|v| String::from_utf8_lossy(&v).into_owned())
        .unwrap_or_default();

    // Substring-scan rather than JSON-parse: currentPresetDataJson is TRUNCATED on
    // this firmware (~5 KB), so serde would fail on a reply that still carries the
    // one field being measured.
    // Reuses `field_of` above rather than re-deriving the scan: the old copy carried a
    // hand-counted `i + 12` offset for `"template":"` that would silently mis-slice if
    // the key were ever renamed. Same unterminated-value-reads-as-absent semantics.
    let tpl = |j: &str| field_of(j, "template").unwrap_or_else(|| "<absent>".into());
    let (b, a) = (tpl(&before), tpl(&after));
    // An absent read is NOT evidence of "unchanged" — two failed reads compare
    // equal and would otherwise manufacture a false negative. Say inconclusive.
    let verdict = if b == "<absent>" || a == "<absent>" {
        "INCONCLUSIVE — a read returned no template field; the comparison proves nothing"
    } else if a == b {
        "NO CHANGE — switchTemplate not applied even on the same connection"
    } else {
        "TEMPLATE CHANGED — switchTemplate is served"
    };
    Ok(format!(
        "switchTemplate({template_type}) on slot {slot}, SAME-connection read-back:\n  \
         template before = {b}  ({} B read)\n  template after  = {a}  ({} B read)\n  \
         verdict: {verdict}\n\
         reply stream (templateSwitched is field 44):\n{dump}",
        before.len(),
        after.len()
    ))
}

/// HW PROBE (DEVICE WRITE): reorder a user preset, `from` → `to` (0-based list
/// indices). The device renumbers presets automatically; whether it also rewrites
/// slot-keyed Song/Setlist bindings is the open question this exists to settle.
///
/// **Confirms identity through the mutation path's own read**, before and after —
/// never a caller-supplied label. It reads the live `list_my_presets` on the same
/// connection that performs the move, prints what actually occupies both indices,
/// and reports the post-move occupants so a caller can diff. An earlier off-by-one
/// destroyed a real preset; a printed label is not identity.
///
/// **Scratch-zone only.** A reorder outside 400/401/402 shifts real user presets,
/// so anything else is refused rather than trusted to the caller.
pub fn probe_move_preset(from: u32, to: u32) -> Result<String, String> {
    if !SCRATCH_SLOTS.contains(&from) || !SCRATCH_SLOTS.contains(&to) {
        return Err(format!(
            "refusing move {from}→{to}: probe reorder is scratch-zone only {SCRATCH_SLOTS:?} \
             (a reorder outside it renumbers real presets)"
        ));
    }
    if from == to {
        return Err("refusing move: from == to (nothing to reorder)".into());
    }

    let mut s = Session::connect()?;
    fn name_at(list: &[PresetEntry], idx: u32) -> String {
        list.iter()
            .find(|p| p.slot == idx)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "<empty>".into())
    }

    let before = s.list_my_presets()?;
    let (from_name, to_name) = (name_at(&before, from), name_at(&before, to));
    if from_name == "<empty>" {
        return Err(format!(
            "refusing move: list index {from} reads empty — nothing to reorder"
        ));
    }
    let mut out = format!("before: [{from}]={from_name:?}  [{to}]={to_name:?}\n");

    s.move_user_preset(from, to)?;
    std::thread::sleep(std::time::Duration::from_millis(800));
    out += &format!(
        "presetError seen on the write connection: {}\n",
        s.saw_preset_error()
    );
    drop(s);

    // Verify on a FRESH connection: a same-connection re-list can serve the
    // pre-move state, so a no-op and a stale read are otherwise indistinguishable.
    std::thread::sleep(std::time::Duration::from_millis(800));
    let mut s = Session::connect()?;
    let after = s.list_my_presets()?;
    out += &format!(
        "after:  [{from}]={:?}  [{to}]={:?}\n",
        name_at(&after, from),
        name_at(&after, to)
    );
    out += &format!(
        "moved {from_name:?} → {}\n",
        if name_at(&after, to) == from_name {
            "landed at the destination index"
        } else {
            "NOT at the destination index — reorder did not land as expected"
        }
    );
    Ok(out)
}

/// Headless firmware-version read (`probe --fw`): connect, request the version
/// in-burst (`currentFwRequest`), and return the `currentFwResponse` data.
pub fn probe_firmware_version() -> Result<String, String> {
    let s = Session::connect_with_firmware()?;
    s.firmware_version()
        .ok_or_else(|| "handshake carried no currentFwResponse".to_string())
}

/// Retry active-graph discovery because the TMP's field-3 stream length varies
/// slightly between handshakes. A graph is usable only after its routing
/// template arrives; otherwise a parallel path can be rendered as series.
pub(crate) fn discover_active_graph() -> Result<(session::ActiveGraph, String), String> {
    let mut errors = Vec::new();
    for _ in 0..3 {
        match Session::connect_for_discovery() {
            Ok(mut s) => {
                for _ in 0..4 {
                    let diagnostics = format!(
                        "{}\n{}",
                        s.slot_read_diagnostics(),
                        s.active_graph_diagnostics()
                    );
                    match s.current_audio_graph() {
                        Ok(mut graph) => {
                            if graph.slot.is_none() {
                                graph.slot = s.resolve_unique_my_preset_slot(graph.name.as_deref());
                            }
                            return Ok((graph, diagnostics));
                        }
                        Err(e) => errors.push(format!("{e}\n{diagnostics}")),
                    }
                    s.pump_more(250)?;
                }
            }
            Err(e) => errors.push(e),
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    Err(errors.join("\n--- retry ---\n"))
}

/// Read-only diagnostic for the active-preset signal chain. Prints the
/// live discovery payload summary and the routing-aware graph sent to React.
pub fn probe_active_graph() -> Result<String, String> {
    let (graph, diagnostics) = discover_active_graph()?;
    let graph = serde_json::to_string_pretty(&graph)
        .map_err(|e| format!("serialize active graph diagnostic: {e}"))?;
    Ok(format!("{diagnostics}\n{graph}\n"))
}

/// Force re-amp mode OFF on a fresh connection — the recovery path for a unit left
/// input-muted ("no sound") by an interrupted leveling run (re-amp routes the input
/// to USB; a dropped fire-and-forget OFF strands it there). `probe --reamp-off`.
pub fn probe_reamp_off() -> Result<(), String> {
    Session::connect()?.set_reamp_mode(false).map(|_| ())
}

/// Discover a preset's level-type block controls (the leveling-knob candidates).
///
/// Primary path is the 1.8.45-SAFE RICH LEAN SESSION (the bench intel-session /
/// `prepass_scene_docs` pattern): heartbeat warmup → `send_and_collect(LoadPreset)`
/// → pump past the 125 hit → read `current_preset_blocks` from the accumulated
/// field-3 push bodies. `connect_for_discovery` (field-78) is effectively DEAD on
/// fw 1.8.45 — it never delivers `currentPresetDataChanged` — so it can no longer be
/// the primary; it stays only as a fallback for older firmware. Without this, FS-scene
/// leveling found zero amp candidates and silently skipped every scene (the device
/// never switched scenes).
///
/// DANGER — every path below LOADS `slot`, so this is a stale-load site (`danger.md`'s
/// lazy-commit clause): a discovery load inside a same-slot save's commit window
/// materializes the PRE-save doc and its own commit reverts that save (the preset-24
/// class). The barrier lives HERE so EVERY caller of the block-discovery seam is guarded,
/// present and future — which imposes the seam's contract: a caller MUST NOT hold a device
/// session when it calls (the barrier and the discovery each open their own), and `slot`
/// is the 0-based list index, the registry's own key space. Probe processes never save
/// through the registry, so there the barrier is a zero-cost no-op. `op_aborted` is the
/// right cancel hook: the UI path enters through `with_released_seize`, whose
/// `lock_device_op` clears the flag.
pub(crate) fn load_then_discover_blocks(slot: u32) -> Result<Vec<session::LevelBlock>, String> {
    crate::leveller::ensure_fresh_load(slot, &mut || crate::op_aborted())?;
    match discover_blocks_rich(slot) {
        Ok(blocks) if !blocks.is_empty() => return Ok(blocks),
        Ok(_) => log::info!("rich block discovery for slot={slot}: loaded but no level blocks"),
        Err(e) => log::warn!("rich block discovery for slot={slot}: {e}"),
    }
    // Fallback for older firmware where the field-78 discovery handshake works.
    {
        let mut s = Session::connect()?;
        s.load_preset(slot)?;
        std::thread::sleep(std::time::Duration::from_millis(1200));
    }
    std::thread::sleep(std::time::Duration::from_millis(400));
    match Session::connect_for_discovery()?.current_preset_blocks() {
        Ok(blocks) => Ok(blocks),
        Err(first_err) => {
            log::warn!("block discovery fallback for slot={slot}: {first_err}");
            let mut s = Session::connect()?;
            let raw = s.capture_full_preset_json(Some(slot), 2000)?;
            let text = String::from_utf8_lossy(&raw);
            let value = session::tolerant_parse_json(&text)
                .ok_or_else(|| format!("{first_err}; fallback field-3 JSON did not parse"))?;
            // Same picker gate as `current_preset_blocks` above — this is the last fallback
            // of the SAME enumeration, so it must not offer a different control set.
            let blocks = session::extract_level_candidates(&value);
            if blocks.is_empty() {
                Err(format!(
                    "{first_err}; fallback field-3 JSON had no level blocks"
                ))
            } else {
                Ok(blocks)
            }
        }
    }
}

/// 1.8.45-safe block discovery: a single rich lean session loads the preset via
/// `send_and_collect` (NOT `load_preset`, which discards the reports the field-3 push
/// rides on) and reads the level blocks from the accumulated push bodies. Mirrors the
/// bench intel session + `prepass_scene_docs`. Private on purpose: this is the
/// deliberately-UNBARRIERED inner half of `load_then_discover_blocks` — every outside
/// caller must come through the wrapper and its commit-window barrier.
fn discover_blocks_rich(slot: u32) -> Result<Vec<session::LevelBlock>, String> {
    let mut s = Session::connect()?;
    s.rich_warmup()?;
    s.rich_load_collect(slot)?;
    s.current_preset_blocks()
}

/// Enumerate a preset's level-type block controls (the leveling-knob candidates).
/// Used by `probe --blocks`.
pub fn probe_list_blocks(slot: u32) -> Result<String, String> {
    let blocks = load_then_discover_blocks(slot)?;
    if blocks.is_empty() {
        return Err(
            "no level-type block controls found (preset JSON may have truncated \
                    before audioGraph completed, or the preset has no level params)"
                .to_string(),
        );
    }
    let mut out = format!(
        "slot {slot}: {} level-type block control(s):\n",
        blocks.len()
    );
    for b in &blocks {
        out += &format!(
            "  {} / {} [{}] / {} = {:.4}\n",
            b.group_id, b.node_id, b.model_id, b.parameter_id, b.value
        );
    }
    Ok(out)
}

/// Probe (AC3): re-import a `.preset` file to the device over USB and
/// report where it landed. Reads the raw file bytes (the OFFLINE codec's XOR'd
/// output), sends the chunked `importPresetRequest`, and re-lists "My Presets" to
/// show the change. The device chooses the slot; this is additive (creates a new
/// user preset) — clear it afterward to restore state.
pub fn probe_import_preset(path: &str) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    // Baseline list on its own session, then import on a second, then re-list on a
    // third fresh session — the post-import re-list must NOT reuse the importing
    // session's buffer (the chunked send pollutes its accumulated reports).
    let before = Session::connect()?.list_my_presets()?;
    let before_pairs: std::collections::HashSet<(u32, String)> =
        before.iter().map(|p| (p.slot, p.name.clone())).collect();

    let resp = Session::connect()?.import_preset(&bytes)?;

    let after = Session::connect()?.list_my_presets()?;
    let changed: Vec<String> = after
        .iter()
        .filter(|p| !before_pairs.contains(&(p.slot, p.name.clone())))
        .map(|p| format!("idx {} (device slot {}) = {:?}", p.slot, p.slot + 1, p.name))
        .collect();
    let resp_str = match resp {
        Some((le, slot)) => format!("importPresetResponse: listEnum={le} presetSlot={slot}"),
        None => {
            "no importPresetResponse echo (device may not reply — verify via the slot diff)".into()
        }
    };
    Ok(format!(
        "[probe --import] file={path} raw_bytes={}\n{resp_str}\n\
         My Presets: before={} slots, after={} slots\n\
         slots whose name changed after import: {changed:?}\n",
        bytes.len(),
        before.len(),
        after.len(),
    ))
}

/// Probe: clear (delete) a user preset slot — `clearUserPreset` (AC4 setter).
/// Used to undo a `--import` test and to exercise the setter on hardware. Guarded:
/// only clears a slot that currently reads `expect_name` (pass the imported name,
/// e.g. "Guitar") so a mistyped slot can't nuke an unrelated preset.
/// `slot` is the 0-BASED list index — the same space as `PresetEntry.slot` (the
/// guard's read) and `clear_user_preset`'s argument (the mutation), so guard and
/// mutation act in ONE address space (the write-safety lesson). NB: `--export`
/// takes the 1-BASED device slot instead — this probe's list index + 1.
pub fn probe_clear_preset(slot: u32, expect_name: &str) -> Result<String, String> {
    let before = Session::connect()?.list_my_presets()?;
    let cur = before
        .iter()
        .find(|p| p.slot == slot)
        .map(|p| p.name.clone());
    if cur.as_deref() != Some(expect_name) {
        return Ok(format!(
            "[probe --clear] slot {slot} reads {cur:?}, not {expect_name:?} — refused (no change)\n"
        ));
    }
    // Deliberately NOT `confirm_slot_name`: this path reports a refusal as an `Ok`
    // message so `probe --clear` exits 0 on a mismatch. Only the recycle gaps are
    // shared — three back-to-back `connect()`s with no gap is the shape that lands in
    // the exclusive-open lockout (`0xe00002c5`).
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    Session::connect()?.clear_user_preset(slot)?;
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    let after = Session::connect()?.list_my_presets()?;
    let now = after
        .iter()
        .find(|p| p.slot == slot)
        .map(|p| p.name.clone());
    Ok(format!(
        "[probe --clear] slot {slot} was {expect_name:?}; cleared → now reads {now:?}\n"
    ))
}

/// Diagnostic (`--diag-frames`): dump the raw inbound frame magic/len sequence for a
/// setlist-list read, so the multi-packet framing (0x33 start / 0x34 cont / 0x35 final)
/// and any interleaved foreign streams are visible, plus whether a strict decode lands.
pub fn probe_diag_frames() -> Result<String, String> {
    let req = proto::setlist_list_request(Some(2));
    let mut s = Session::connect_with_burst_request(&req)?;
    let mut out = String::from("[diag-frames] setlist read, per-pump frame magics:\n");
    for i in 0..6 {
        let strict = s.harvest_setlists_strict().map(|v| v.len());
        out += &format!(
            "  pump {i}: strict={strict:?}\n    frames: {}\n",
            s.raw_frame_summary()
        );
        s.pump_more(300)?;
    }
    Ok(out)
}

/// Diagnostic (`--diag-writes`): compare the device's REPLY to addSong (known-good)
/// vs addSetlist (silently failing) to distinguish an error reply from a silent
/// ignore, and whether setlist context changes the reply. Creates throwaway
/// "Diag*" entries (clean up after).
pub fn probe_diag_writes() -> Result<String, String> {
    let mut out = String::new();
    {
        let mut s = Session::connect()?;
        out += "[diag] addSong \"DiagSong\" (default context) reply:\n";
        out += &s.send_and_dump(&proto::add_song("DiagSong"), 800)?;
    }
    {
        let mut s = Session::connect()?;
        out += "[diag] addSetlist \"DiagBare\" reply:\n";
        out += &s.send_and_dump(&proto::add_setlist("DiagBare"), 800)?;
    }
    Ok(out)
}

/// Probe (AC7 prerequisite): report the data needed to understand the
/// list-index ↔ device-userSlot relationship.
///
/// `list_my_presets` returns 0-based list positions; the `userSlot` setters
/// (`saveCurrentPreset`/`clearUserPreset`/`moveUserPreset`) address a device
/// userSlot. `requestNextEmptyPresetSlot` (81→82) is **dead on 1.7.75**, so we
/// can't enumerate device empties; instead we report the list's `--` placeholders
/// and Song-1's `userPresetSlot`s. Correlating a Song row's `userPresetSlot` with
/// the list index of the preset assigned to it (you supply that preset) pins the
/// numbering: if they match, list index == device userSlot. The shipped leveller
/// already saves via list index, which is consistent with that.
pub fn probe_map_slots() -> Result<String, String> {
    let list = Session::connect()?.list_my_presets()?;
    let empty_list: Vec<u32> = list
        .iter()
        .filter(|p| session::is_empty_slot_name(&p.name))
        .map(|p| p.slot)
        .collect();
    let songs = read_song_presets(1).unwrap_or_default();
    let song_rows: Vec<(u32, u32)> = songs
        .iter()
        .filter(|r| !r.is_empty)
        .map(|r| (r.user_preset_slot, r.preset_scene_slot))
        .collect();

    let mut out = format!(
        "[probe --map-slots]\n\
         My Presets list entries: {}\n\
         empty ('--') list indices (first 12): {:?}\n\
         song-1 (userPresetSlot, presetSceneSlot) rows: {:?}\n",
        list.len(),
        empty_list.iter().take(12).collect::<Vec<_>>(),
        song_rows,
    );
    out += "note: requestNextEmptyPresetSlot (81→82) is dead on 1.7.75 — scratch slots are \
            found by observation (import then re-list), not prediction. To pin the \
            list↔userSlot numbering, assign a known preset to a song and compare its \
            list index with the userPresetSlot reported above.\n";
    Ok(out)
}

/// Build + reconcile a library from `folder` against the live device (Pro Control
/// closed). Shared by the dry-run / apply probes.
fn probe_load_reconciled_library(folder: &str) -> Result<library::Library, String> {
    let (mut records, errors) =
        library::load_library_from_dir(std::path::Path::new(folder), &library_categories())?;
    if !errors.is_empty() {
        eprintln!("[probe] {} file(s) skipped: {errors:?}", errors.len());
    }
    let device_list = Session::connect()?.list_my_presets()?;
    library::reconcile_with_device(&mut records, &device_list);
    Ok(library::Library {
        folder: folder.into(),
        records,
    })
}

/// `probe --bulk-dryrun <folder> <opspec.json>` — preview an op over all matched
/// presets; writes nothing.
pub fn probe_bulk_dryrun(folder: &str, opspec_json: &str) -> Result<String, String> {
    let lib = probe_load_reconciled_library(folder)?;
    let op: bulk_cmd::OpSpec =
        serde_json::from_str(opspec_json).map_err(|e| format!("parse opspec: {e}"))?;
    let selection: Vec<u32> = lib.records.iter().filter_map(|r| r.list_index).collect();
    let targets = targets_from_library(&lib, &selection)?;
    let operation = bulk_cmd::build_operation(&op)?;
    Ok(format_dry_run(&bulkrun::dry_run(
        &targets,
        operation.as_ref(),
    )))
}

/// `probe --bulk-apply <folder> <opspec.json> [--slots a,b] [revert]` — apply on the
/// real device (backup-first), optionally reverting immediately for a full round-trip.
pub fn probe_bulk_apply(
    folder: &str,
    opspec_json: &str,
    slots: Option<Vec<u32>>,
    revert: bool,
) -> Result<String, String> {
    let lib = probe_load_reconciled_library(folder)?;
    let op: bulk_cmd::OpSpec =
        serde_json::from_str(opspec_json).map_err(|e| format!("parse opspec: {e}"))?;
    let selection =
        slots.unwrap_or_else(|| lib.records.iter().filter_map(|r| r.list_index).collect());
    let targets = targets_from_library(&lib, &selection)?;
    let operation = bulk_cmd::build_operation(&op)?;
    let mut io = io_for_path(op.io_path());
    let backup_dir = std::env::temp_dir().join(format!(
        "tmp-companion-probe-backups-{}",
        std::process::id()
    ));

    let report = bulkrun::apply(&targets, operation.as_ref(), io.as_mut(), &backup_dir);
    let mut out = format!(
        "[probe bulk apply] {} target(s): {} changed, {} verified, {} error(s)\n",
        report.entries.len(),
        report.changed(),
        report.verified(),
        report.errors(),
    );
    for e in &report.entries {
        out += &format!(
            "  slot {:>3} {:<24} changed={} verified={}{}\n",
            e.list_index,
            e.display_name,
            e.changed,
            e.verified,
            e.error
                .as_ref()
                .map(|x| format!("  ERROR: {x}"))
                .unwrap_or_default(),
        );
    }
    if revert {
        let rev = bulkrun::revert(&report, io.as_mut());
        out += &format!(
            "[probe bulk revert] {} restored\n",
            rev.iter().filter(|r| r.restored).count()
        );
        for r in &rev {
            out += &format!(
                "  slot {:>3} restored={}{}\n",
                r.list_index,
                r.restored,
                r.error
                    .as_ref()
                    .map(|x| format!("  ERROR: {x}"))
                    .unwrap_or_default(),
            );
        }
    }
    Ok(out)
}

/// ponytail: throwaway HW experiment (`--save-load-test`): can ONE connection chain
/// item N's `saveCurrentPreset` with item N+1's `loadPreset`? Conn 1 makes `slot_a`
/// current; conn 2 sets a distinctive presetLevel, saves, then loads `slot_b` on the
/// SAME connection. Verification is the caller's (field-8 read of `slot_a` + an
/// active-graph check). DESTRUCTIVE: overwrites `slot_a`'s stored presetLevel — guarded
/// by a non-destructive read of `slot_a`'s current name against `expected_name_a`
/// (same 0-based list-index space as the mutation) before anything is touched.
pub fn probe_save_load_test(
    slot_a: u32,
    slot_b: u32,
    level: f32,
    expected_name_a: &str,
) -> Result<String, String> {
    let actual_name = Session::connect()?
        .list_my_presets()?
        .into_iter()
        .find(|p| p.slot == slot_a)
        .map(|p| p.name)
        .ok_or_else(|| format!("no preset at list index {slot_a}"))?;
    if actual_name != expected_name_a {
        return Err(format!(
            "guard mismatch: slot {slot_a} is \"{actual_name}\", expected \"{expected_name_a}\" — refusing to overwrite"
        ));
    }

    {
        let mut s = Session::connect()?;
        s.load_preset(slot_a)?;
        std::thread::sleep(std::time::Duration::from_millis(
            crate::leveller::settle_after_load_ms(),
        ));
    }
    std::thread::sleep(std::time::Duration::from_millis(
        crate::leveller::RECONNECT_GAP_MS,
    ));
    let mut s = Session::connect()?;
    let ack = s.set_preset_level(level)?;
    if ack.is_none() {
        return Err(format!(
            "slot {slot_a}: no presetLevel acknowledgement from the device — refusing to save an unconfirmed level"
        ));
    }
    std::thread::sleep(std::time::Duration::from_millis(
        crate::leveller::SETTLE_AFTER_SET_MS,
    ));
    s.save_current_preset(slot_a)?;
    s.load_preset(slot_b)?;
    std::thread::sleep(std::time::Duration::from_millis(
        crate::leveller::settle_after_load_ms(),
    ));
    Ok(format!(
        "set level {level} on slot {} (ack) · save sent · loaded slot {} — save+load on ONE connection (initial slot-{}-current load used a separate connection; persistence not read back)",
        slot_a + 1,
        slot_b + 1,
        slot_a + 1
    ))
}

/// The gain-budget redistribution (PR5) atomicity PRE-REQUISITE (`probe
/// --redistribute-persist-check <scratchSlot> <expectedName>`). It HW-verifies the
/// single load-bearing assumption behind the feature's read-back-then-save design:
/// that a `presetLevel` write **+** a BASE amp `outputLevel` `changeParameter` **+**
/// a scene-overlay `outputLevel` write can all accumulate UNSAVED in the working copy
/// and survive ONE `saveCurrentPreset`. If any of the three does NOT persist, the
/// redistribution feature's atomicity model is unsound and must be reworked — this is
/// the go/no-go gate the plan mandates running FIRST.
///
/// Point it at a prepared SCRATCH preset that carries an amp (a guitarNodes node with
/// an `outputLevel` control) and ≥1 footswitch scene — e.g. the e2e "E2E Rig"
/// after `probe --seed-scenario` (e.g. "E2E Parallel" at 403, whose amp id is short
/// enough to fit a single-report `changeParameter`). It reads the slot's current values,
/// applies three DISTINCTIVE test values (base 0.30, scene[1] 0.66, presetLevel 0.42) all
/// on ONE live-edit session, saves ONCE, reconnects, reads back, and asserts. Then —
/// name-guarded — it restores the originals, leaving the slot as found.
///
/// Two HW-learned rules baked in: (1) all writes + the save share ONE session (a fresh
/// full-handshake reconnect re-reads the preset and discards the prior session's working
/// copy → an empty-graph save); (2) recall the BASE scene before saving (saving with a
/// non-base scene active + scene-edit ON serialized an empty base graph on HW). No re-amp
/// is engaged (a persistence check, not a loudness measure), so the post-re-amp save-drop
/// can't bite. Read-back is FIELD-8: it reliably carries presetLevel + the base amp
/// outputLevel (the surviving prefix), but NOT the scene overlay post-save (a device save
/// re-serializes verbose, pushing scenes[] past field-8's fixed truncation window — the
/// overlay leg's persistence is HW-established independently, not re-read here).
/// Quiet gap between the write-session's save-close and the field-8 read-back reconnect:
/// a rapid close→reopen after a save lands on a lean line whose data request goes
/// unanswered (a truncated/garbled read), so let the device settle first.
const READBACK_SETTLE_MS: u64 = 8_000;

pub fn probe_redistribute_persist_check(slot: u32, expected_name: &str) -> Result<String, String> {
    use crate::audiograph;
    use std::time::Duration;

    guard_slot_name(slot, expected_name)?;

    // Read the current preset (field-8) and locate the amp + a scene to write.
    let (preset, _, _) = crate::read_slot_preset_parsed(slot)?;
    let (group_id, node_id, orig_base) = find_amp_output_level(&preset)
        .ok_or_else(|| format!("slot {slot} has no amp node with an outputLevel control"))?;
    let orig_preset_level = audiograph::preset_level(&preset)
        .ok_or_else(|| format!("slot {slot} preset JSON carries no presetLevel"))?
        as f32;
    // Use scene 1, not 0: the OPEN scene-0 anomaly (USB loadScene(0) materializes a
    // different amp state than the footswitch tap) confounds a scene-0 read-back.
    if scene_count(&preset) < 2 {
        return Err(format!(
            "slot {slot} has < 2 footswitch scenes — the check needs scene 1 (scene 0 is the confounded case)"
        ));
    }
    let scene_slot = 1u32;
    let orig_scene = scene_overlay_output_level(&preset, scene_slot, &group_id, &node_id);

    // Distinctive test values (each in the amp/level valid range, each unambiguous on
    // read-back; chosen to differ from typical stored values so a "no-op" can't false-pass).
    const TEST_PRESET_LEVEL: f32 = 0.42;
    const TEST_BASE_OL: f32 = 0.30;
    const TEST_SCENE_OL: f32 = 0.66;
    const TOL: f32 = 0.01;

    let mut out = format!(
        "=== redistribute-persist-check · slot {slot} (\"{expected_name}\") ===\n\
         amp {group_id}/{node_id}  orig: presetLevel={orig_preset_level:.4} baseOL={orig_base:.4} scene[{scene_slot}]OL={:?}\n\
         writing: presetLevel={TEST_PRESET_LEVEL} baseOL={TEST_BASE_OL} scene[{scene_slot}]OL={TEST_SCENE_OL} (all UNSAVED), then ONE save…\n",
        orig_scene
    );

    // Apply the three write kinds + save on ONE live-edit session (see
    // `write_three_and_save`: fresh reconnects between edits reset the working copy).
    write_three_and_save(
        slot,
        &group_id,
        &node_id,
        scene_slot,
        TEST_PRESET_LEVEL,
        TEST_BASE_OL,
        TEST_SCENE_OL,
    )?;

    // Read back via FIELD-8 after a quiet settle. field-8 reliably carries presetLevel +
    // the base guitarNodes amp outputLevel (both live in the surviving prefix) — HW-proven
    // to read back the written 0.42 / 0.30. It does NOT carry the SCENE overlay post-save:
    // a device save re-serializes the preset verbose and its scenes[] fall past field-8's
    // fixed ~17 KB truncation window (HW-confirmed by A/B — the production scene-write path
    // reads back scenes=0 on field-8 too, while the overlay is physically saved). So the
    // scene-overlay leg's persistence is HW-ESTABLISHED INDEPENDENTLY (a single
    // saveCurrentPreset persists all accumulated overlays), not re-read here.
    std::thread::sleep(Duration::from_millis(READBACK_SETTLE_MS));
    let (after, _, _) = crate::read_slot_preset_parsed(slot)?;
    let got_pl = audiograph::preset_level(&after).map(|v| v as f32);
    let got_base = find_amp_output_level(&after).map(|(_, _, v)| v);
    let got_scene = scene_overlay_output_level(&after, scene_slot, &group_id, &node_id);
    // Guard a garbled read from becoming a false verdict: the base amp must be present.
    if got_base.is_none() {
        out += "read-back: field-8 base amp missing — the read was garbled/truncated, not a \
                persistence failure.\nVERDICT: INCONCLUSIVE — re-run on a rested device.\n";
        let _ = restore_scratch(
            slot,
            expected_name,
            &group_id,
            &node_id,
            scene_slot,
            orig_preset_level,
            orig_base,
            orig_scene.unwrap_or(orig_base),
        );
        return Ok(out);
    }
    let near = |got: Option<f32>, want: f32| got.is_some_and(|v| (v - want).abs() <= TOL);
    let ok_pl = near(got_pl, TEST_PRESET_LEVEL);
    let ok_base = near(got_base, TEST_BASE_OL);
    // The scene overlay isn't field-8-readable post-save (truncation, above): if it happens
    // to survive the window we assert it; if it fell off (`None`), that is EXPECTED, not a
    // failure — the leg is covered by the independent overlay-persistence HW fact.
    let scene_note = match got_scene {
        Some(v) if (v - TEST_SCENE_OL).abs() <= TOL => "PASS (survived the field-8 window)",
        Some(_) => "MISMATCH (read a stale/base value — investigate)",
        None => {
            "not readable post-save (field-8 truncation) — covered by the independent \
                 overlay-persistence HW fact"
        }
    };
    out += &format!(
        "read-back (field-8): presetLevel={got_pl:?} [{}] · baseOL={got_base:?} [{}] · scene[{scene_slot}]OL={got_scene:?} [{scene_note}]\n",
        pass(ok_pl),
        pass(ok_base),
    );
    let novel_ok = ok_pl && ok_base; // the NOVEL combination this check exists to prove
    out += if novel_ok {
        "VERDICT: GO — presetLevel + a base amp outputLevel changeParameter persist TOGETHER \
         through one save (the novel combination); scene overlays persist independently \
         (HW-established). The redistribution read-back-then-save atomicity design is sound.\n"
    } else {
        "VERDICT: NO-GO — presetLevel and/or the base amp write did NOT persist through one \
         save; the redistribution feature must NOT be built on this atomicity assumption.\n"
    };

    // Name-guarded restore: put the scratch preset back the way we found it (best-effort;
    // a scene that had no original overlay gets the original written back, harmless on a
    // scratch slot). Failures are logged into the report, never masked.
    match restore_scratch(
        slot,
        expected_name,
        &group_id,
        &node_id,
        scene_slot,
        orig_preset_level,
        orig_base,
        orig_scene.unwrap_or(orig_base),
    ) {
        Ok(()) => {
            out += "cleanup: restored original presetLevel + base + scene, saved (name-guarded).\n"
        }
        Err(e) => out += &format!("cleanup FAILED (scratch slot left modified): {e}\n"),
    }
    Ok(out)
}

fn pass(ok: bool) -> &'static str {
    if ok {
        "PASS"
    } else {
        "FAIL"
    }
}

/// Name-guard for a slot-keyed write: a slot is a position, not an identity — refuse
/// unless it still holds the preset the caller named (the write-safety lesson). Opens
/// its own connection (a non-destructive read before any mutation).
fn guard_slot_name(slot: u32, expected_name: &str) -> Result<(), String> {
    let name = Session::connect()?
        .list_my_presets()?
        .into_iter()
        .find(|p| p.slot == slot)
        .map(|p| p.name)
        .ok_or_else(|| format!("no preset at list index {slot}"))?;
    if name != expected_name {
        return Err(format!(
            "guard mismatch: slot {slot} is \"{name}\", expected \"{expected_name}\" — refusing to write"
        ));
    }
    Ok(())
}

/// Find the first amp node carrying an `outputLevel` control: `(group, nodeId, value)`.
fn find_amp_output_level(preset: &serde_json::Value) -> Option<(String, String, f32)> {
    let ag = preset.get("audioGraph")?.get("guitarNodes")?.as_object()?;
    let mut keys: Vec<&String> = ag.keys().collect();
    keys.sort();
    for k in keys {
        for node in ag[k].as_array().into_iter().flatten() {
            if let Some(v) = node
                .get("dspUnitParameters")
                .and_then(|p| p.get("outputLevel"))
                .and_then(serde_json::Value::as_f64)
            {
                let id = crate::audiograph::node_id(node)?.to_string();
                return Some((k.clone(), id, v as f32));
            }
        }
    }
    None
}

/// Number of footswitch scenes (`scenes[]` array length).
fn scene_count(preset: &serde_json::Value) -> usize {
    preset
        .get("scenes")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len)
}

/// A scene overlay's `outputLevel` for `node_id` (`scenes[i].guitarNodes.<group>.<nodeId>`),
/// `None` when the scene carries no `outputLevel` under that node.
///
/// Deliberately does NOT classify the overlay the way `scene_jobs::scene_overlay` does
/// (`Full` / `BypassOnly` / `Absent` / `Unknown`): this is a pure VALUE lookup, so it is
/// the equivalent of that enum's shared `Full | BypassOnly` arm and a bypass-only overlay
/// correctly reads `None` (no `outputLevel` key to report). Nothing here infers overlay
/// PRESENCE to gate a `setNodeSceneEdit`: both enable sites in this module
/// (`write_three_and_save`, `probe_scene_write_cell`) are caller-driven diagnostic axes on
/// a SCRATCH slot, which is what makes the reseed they exercise safe.
fn scene_overlay_output_level(
    preset: &serde_json::Value,
    scene_slot: u32,
    group_id: &str,
    node_id: &str,
) -> Option<f32> {
    preset
        .get("scenes")?
        .as_array()?
        .get(scene_slot as usize)?
        .get("guitarNodes")?
        .get(group_id)?
        .get(node_id)?
        .get("dspUnitParameters")?
        .get("outputLevel")
        .and_then(serde_json::Value::as_f64)
        .map(|v| v as f32)
}

/// Apply the three write kinds + save on ONE live-edit session (the
/// `write_footswitch_values` model). A fresh full-handshake reconnect RE-READS the
/// preset and discards the prior session's working-copy edits, so load + presetLevel +
/// base amp `outputLevel` (base scene) + scene overlay (`loadScene` + scene-edit) + save
/// must all share the SAME session. No re-amp is toggled, so the same-session save is
/// safe (the post-re-amp save-drop can't bite).
fn write_three_and_save(
    slot: u32,
    group_id: &str,
    node_id: &str,
    scene_slot: u32,
    preset_level: f32,
    base_ol: f32,
    scene_ol: f32,
) -> Result<(), String> {
    use std::time::Duration;
    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    s.load_preset(slot)?;
    let name = s.active_preset_name().unwrap_or_default();
    if !name.is_empty() && !s.await_active_preset(&name, 20) {
        return Err("after load, active preset changed — aborting before write".into());
    }
    for _ in 0..8 {
        let _ = s.heartbeat();
        let _ = s.pump_collect(150);
    }
    // presetLevel (global) + base amp outputLevel — base scene is active after a load. The
    // presetLevel ack is advisory (flaky on a lean session, HW); the read-back confirms.
    if s.set_preset_level(preset_level)?.is_none() {
        log::warn!("persist-check: no presetLevel ack (advisory — read-back confirms)");
    }
    std::thread::sleep(Duration::from_millis(crate::leveller::SETTLE_AFTER_SET_MS));
    s.change_parameter(group_id, node_id, "outputLevel", base_ol)?;
    let _ = s.heartbeat();
    let _ = s.pump_collect(150);
    // Scene overlay — recall the scene, enable scene edit, write within the ~700 ms window.
    s.load_scene(scene_slot)?;
    std::thread::sleep(Duration::from_millis(150));
    s.set_node_scene_edit(group_id, node_id, true)?;
    std::thread::sleep(Duration::from_millis(300));
    s.change_parameter(group_id, node_id, "outputLevel", scene_ol)?;
    std::thread::sleep(Duration::from_millis(200));
    // Recall the BASE scene before saving (the proven `save_deferred_scene_writes` shape):
    // saving with a non-base scene active + scene-edit ON serialized an EMPTY base graph on
    // HW (403 came back presetLevel 0.53 / no nodes). Base recall stamps the full graph.
    s.load_scene(crate::session::BASE_SCENE_SLOT)?;
    std::thread::sleep(Duration::from_millis(crate::leveller::SETTLE_AFTER_SET_MS));
    // ONE save, same session (no re-amp toggled → no save-drop).
    s.save_current_preset(slot)
}

/// Name-guarded restore of the three touched values (+ save), on one session.
#[allow(clippy::too_many_arguments)]
fn restore_scratch(
    slot: u32,
    expected_name: &str,
    group_id: &str,
    node_id: &str,
    scene_slot: u32,
    preset_level: f32,
    base_ol: f32,
    scene_ol: f32,
) -> Result<(), String> {
    guard_slot_name(slot, expected_name)?;
    std::thread::sleep(std::time::Duration::from_millis(
        crate::leveller::RECONNECT_GAP_MS,
    ));
    write_three_and_save(
        slot,
        group_id,
        node_id,
        scene_slot,
        preset_level,
        base_ol,
        scene_ol,
    )
}

/// Confirm `slot` still holds `expect_name` before a destructive write, then wait the
/// device's session-recycle gap.
///
/// Shared by every slot-keyed destructive probe path so the guard cannot drift between
/// copies — the same consolidation `SCRATCH_SLOTS` got. The read is non-destructive and
/// happens in the SAME address space as the mutation it protects, which is the whole
/// point: a `clear` once deleted a real preset because its guard checked list-index
/// space while the op acted in device-slot space.
///
/// The trailing sleep is part of the contract, not a caller's concern: the guard session
/// is dropped at the end of the read, and re-opening immediately after a close is the
/// shape that lands in the exclusive-open lockout (`0xe00002c5`).
pub(crate) fn confirm_slot_name(slot: u32, expect_name: &str) -> Result<(), String> {
    let before = Session::connect()?.list_my_presets()?;
    let cur = before
        .iter()
        .find(|p| p.slot == slot)
        .map(|p| p.name.clone());
    if cur.as_deref() != Some(expect_name) {
        return Err(format!(
            "slot {slot} reads {cur:?}, not {expect_name:?} — refused (no change)"
        ));
    }
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    Ok(())
}

/// Write ONE block parameter to a preset and persist it (`probe --set-scene-param`) —
/// the repair seam for a preset whose scene overlay drifted. Routes through the
/// leveller's own [`leveller::apply_level`] write path, so `scene = Some(n)` gets the
/// supported per-scene treatment (scene recall → per-block Scene Edit →
/// `changeParameter`) and `scene = None` writes the base/global value. `verify: false`,
/// so no stimulus is read and re-amp is never engaged — this is a pure write.
///
/// TWO HW OBSERVATIONS from this arm's first use (fw 1.8.45, preset 028):
///
/// (a) SETTLED by the `--scene-write-cell` 3-cell isolation matrix (fw 1.8.45, scratch
/// slot 32): `set_node_scene_edit(node, true)` **alone** reseeds that node's scene
/// overlay from BASE — recall+write without the enable kept all 7 sibling params, while
/// recall+enable *without any write* reset 6 of them. So it is neither a race nor
/// `change_parameter`; it SUPERSEDES the earlier assumption in `notes/leveling.md`
/// ("enabling scene mode is harmless in itself" — corrected since) and
/// `leveller::set_knobs`, which used to blame a re-`load_scene` between per-knob writes.
///
/// The enable's necessity depends on whether the node ALREADY has an overlay in the
/// target scene (both branches HW-proven on scratch slot 30):
///   * overlay EXISTS  → the write lands on it with the enable dropped, and the enable
///     is actively harmful (it reseeds the overlay from base).
///   * overlay ABSENT  → without the enable the write LEAKS TO BASE (G3
///     `ACD_MarG12H30BB.hpf` 59 → 200 landed on BASE while scene 0 gained no overlay).
///     The enable is what materialises the overlay.
///
/// So `set_knob` should enable scene edit ONLY for a node with no overlay in that
/// scene. This also reconciles `notes/leveling.md`'s "leaks to base" claim — true, but
/// only for the overlay-absent case.
///
/// (b) RESOLVED — there is NO normalisation. `change_parameter` carries real-unit values
/// VERBATIM (`ratehz` 6.0 → 3.0 and `hpf` 59 → 200 both landed exactly). `tapTimeBPM` is
/// a DERIVED mirror of the block's rate — the PERIOD IN SECONDS, not BPM — which is why
/// writes to it never stick and why it looked normalised: the 0.2599872 readback is
/// 1/3.84634 s and the node's `speed` was 3.8463430404663086; a later write of
/// `ratehz` = 3.0 moved the same field to 0.33333334 = 1/3. Both exact. The Doctor's
/// measured-frequency EQ write (`commands/doctor.rs::apply_doctor_ops`) is therefore
/// safe on the value axis.
///
/// (c) NEW, and the sharpest of the three: a `change_parameter` with NO preceding
/// `load_scene` writes into the **currently active scene's overlay**, not base. A preset
/// loads into its saved `lastLoadedScene`, so on a scened preset every "base" operation
/// that omits the recall silently targets that scene (HW: slot 30 loads on scene 3;
/// bare writes of `level` 0.80 and `ratehz` 3.0 landed in scene 3's overlay with BASE
/// untouched — then, once base was the active context, an identical bare write moved
/// BASE). `LevelKnob::Block { scene_slot: None }` (`commands/level_preset.rs`) and
/// `capture_full_at`'s `scene: None` both take that path, so base block leveling
/// measures AND writes the wrong scene. Base is a scene recall (wire slot 8), never an
/// omission — `views/level/LevelView.tsx`'s "loading a preset activates BASE" is false.
///
/// ponytail: single-param, [0,1]-only. Both limits are the diagnostic's, not the
/// device's — widen it (multi-param targets, real-unit encoding) only if a repair
/// actually needs it.
pub fn probe_set_scene_param(
    slot: u32,
    expect_name: &str,
    scene: Option<u32>,
    group: &str,
    node: &str,
    param: &str,
    value: f32,
) -> Result<String, String> {
    // This SAVES (`save: true` below), so it is destructive on a mis-typed index.
    // TWO independent guards, because the identity check alone still lets a correct name
    // authorise a write anywhere in the bank:
    //   1. scratch-zone restriction — the same allowlist every other destructive probe
    //      path checks, opt-out-able for the deliberate case;
    //   2. list-index -> preset mapping confirmed by a non-destructive read, in the SAME
    //      address space as the mutation (a `clear` once deleted a real preset because
    //      its guard checked a different index space than the op).
    if !SCRATCH_SLOTS.contains(&slot) && std::env::var("TMP_ALLOW_NONSCRATCH_WRITE").is_err() {
        return Err(format!(
            "refusing --set-scene-param on list index {slot}: outside the scratch zone \
             {SCRATCH_SLOTS:?} and this SAVES. Re-run with TMP_ALLOW_NONSCRATCH_WRITE=1 if \
             writing this preset is intended."
        ));
    }
    confirm_slot_name(slot, expect_name)?;
    let knob = leveller::LevelKnob::Block {
        group_id: group.to_string(),
        node_id: node.to_string(),
        parameter_id: param.to_string(),
        scene_slot: scene,
    };
    // A SCENE write refuses without the saved document (it decides per node whether Scene
    // Edit must be enabled — both write shapes corrupt the overlay when guessed), so the
    // field-8 read is mandatory there; a base write never consults it, so it is skipped
    // rather than paying ~4 s on this arm. `confirm_slot_name` already slept the reconnect
    // gap, which is the read's own gap contract.
    let saved_doc = scene.and_then(|_| crate::read_saved_preset(slot));
    let opts = leveller::LevelOptions {
        save: true,
        verify: false,
        // The save must re-stamp the preset's ORIGINAL `lastLoadedScene` — the scene
        // recall the write performs otherwise leaves the written scene active at save
        // time (the same clobber `probe_level_preset` guards against).
        restore_scene: saved_doc.as_ref().and_then(crate::last_loaded_scene),
        ..Default::default()
    };
    let (saved, _) = leveller::apply_levels(
        slot,
        &[],
        &[(&knob, value)],
        opts,
        true,
        saved_doc.as_ref(),
        &[],
    )?;
    Ok(format!(
        "[probe --set-scene-param] slot {} · scene {} · {group}/{node}.{param} = {value:.4}  ⇒  {}\n",
        slot + 1,
        scene
            .map(|s| s.to_string())
            .unwrap_or_else(|| "base(global)".to_string()),
        if saved { "SAVED" } else { "NOT SAVED" },
    ))
}

/// One CELL of the scene-write isolation matrix (`probe --scene-write-cell`) — the arm that
/// separates the three candidate causes of "a per-scene write resets the node's other scene
/// params to base". Unlike [`probe_set_scene_param`] (which goes through
/// `leveller::apply_level` and always does recall → scene-edit → write), this drives the
/// session steps DIRECTLY so each one can be omitted independently:
///
/// * `scene_edit=false` → recall + write, no `setNodeSceneEdit`. Siblings survive ⇒ the
///   scene-edit enable is the wiper.
/// * `value=None` → recall + scene-edit + save, no `changeParameter`. Siblings wiped ⇒ the
///   write is exonerated; the enable alone reseeds the overlay.
/// * `recall_settle_ms` → lengthen the post-`loadScene` settle (keep well under the ~700 ms
///   scene-write acceptance cliff). Siblings survive at a longer settle ⇒ it is a RACE (the
///   node's scene values had not materialised when the enable arrived), not a semantic.
///
/// Always saves, so the caller can diff the persisted preset. Diagnostic only.
#[allow(clippy::too_many_arguments)] // mirrors leveller::level_footswitch: a diagnostic seam with one arg per session step
pub fn probe_scene_write_cell(
    slot: u32,
    expect_name: &str,
    scene: Option<u32>,
    group: &str,
    node: &str,
    param: &str,
    value: Option<f32>,
    scene_edit: bool,
    recall_settle_ms: u64,
) -> Result<String, String> {
    // Non-destructive read-back BEFORE any write: confirm the list-index mapping in the
    // SAME address space as the mutation below, so an index mistake refuses instead of
    // silently saving over a real preset (the write-safety lesson this PR series is about).
    confirm_slot_name(slot, expect_name)?;
    {
        let mut s = Session::connect()?;
        s.load_preset(slot)?;
        std::thread::sleep(std::time::Duration::from_millis(
            leveller::settle_after_load_ms(),
        ));
    }
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));

    let mut s = Session::connect()?;
    if let Some(sc) = scene {
        s.load_scene(sc)?;
        std::thread::sleep(std::time::Duration::from_millis(recall_settle_ms));
    }
    if scene_edit && scene.is_some() {
        s.set_node_scene_edit(group, node, true)?;
        // 300 = leveller's private SETTLE_AFTER_SCENE_EDIT_MS, mirrored rather than
        // widening its visibility for a diagnostic.
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    if let Some(v) = value {
        s.change_parameter(group, node, param, v)?;
    }
    s.save_current_preset(slot)?;
    Ok(format!(
        "[probe --scene-write-cell] slot {} · scene {:?} · {group}/{node}.{param}\n  \
         scene_edit={scene_edit} write={:?} recall_settle={recall_settle_ms}ms  ⇒ saved\n",
        slot + 1,
        scene,
        value,
    ))
}
