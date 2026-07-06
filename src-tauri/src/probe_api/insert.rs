//! Probe entry points: live block-insert into a preset group + roster-order helpers.

use crate::audiograph;
use crate::proto;
use crate::session;
use crate::session::Session;
use crate::BulkReplaceItem;

/// ADD a block to the device's CURRENT ACTIVE preset over USB — the live `insertNode`
/// (field 34) path, RE'd byte-exact from a Pro Control add-block capture
/// but never before confirmed on hardware. Mirrors the proven held-session replace
/// architecture (`held_replace_one`): identify the active preset, then on ONE held
/// session load+re-arm that slot, `insertNode` the new block, and (if `commit`) persist
/// in-place (`renameCurrentPreset` → `saveCurrentPreset`, song-link-safe). A DRY run
/// (no `--commit`) inserts, reports what the device replied + whether the block shows up
/// in a read-back, then RELOADS the preset to DISCARD the edit (nothing saved). The
/// active preset is resolved by: explicit `slot_override` (1-based device slot) →
/// `loaded_slot()` echo → unique active-name match in the list; ambiguous/unknown errors
/// out asking for `--slot`. Append by default (`after = None`), or insert after a given
/// FenderId. Group defaults to the primary guitar group "G1" (the capture's group).
pub fn probe_insert_active(
    fender_id: &str,
    group: Option<&str>,
    after: Option<&str>,
    slot_override: Option<u32>,
    commit: bool,
) -> Result<String, String> {
    let mut report = String::new();
    report.push_str(&format!(
        "[probe --insert-active] add {fender_id} ({})\n",
        if commit {
            "COMMIT (insert + save in-place)"
        } else {
            "DRY RUN (insert + verify, NOT saved — reverted)"
        }
    ));

    // ── Identify the ACTIVE preset on a clean session ──
    let mut s = Session::connect()?;
    s.begin_live_edit()?; // warmup harvests the connect-time field-22/field-3 pushes
    let active_name = s.active_preset_name();
    let loaded = s.loaded_slot(); // 0-based list index, or None (no load this session)
    let presets = s.list_my_presets().unwrap_or_default();

    // Resolve the 0-based list index (loaded_slot + PresetEntry.slot are both 0-based).
    let list_index = if let Some(dev) = slot_override {
        dev.saturating_sub(1)
    } else if let Some(idx) = loaded {
        idx
    } else if let Some(ref nm) = active_name {
        let matches: Vec<u32> = presets
            .iter()
            .filter(|p| &p.name == nm)
            .map(|p| p.slot)
            .collect();
        match matches.as_slice() {
            [one] => *one,
            [] => return Err(format!("active preset {nm:?} not found in the preset list — pass --slot <deviceSlot>")),
            _ => return Err(format!("active preset name {nm:?} is ambiguous ({} matching slots) — pass --slot <deviceSlot>", matches.len())),
        }
    } else {
        return Err("could not determine the active preset (no loaded-slot echo, no active name) — pass --slot <deviceSlot>".to_string());
    };
    let name = active_name
        .clone()
        .or_else(|| {
            presets
                .iter()
                .find(|p| p.slot == list_index)
                .map(|p| p.name.clone())
        })
        .unwrap_or_default();

    // Pick the target group: explicit, else the capture's "G1" if present, else the
    // first guitar group in the live roster (a non-existent group → device presetError,
    // which the safety gate rejects without saving).
    let target_group = match group {
        Some(g) => g.to_string(),
        None => {
            let mut groups: Vec<String> = s
                .current_preset_value()
                .ok()
                .map(|v| {
                    audiograph::roster(&v)
                        .into_iter()
                        .map(|(g, _, _)| g)
                        .collect()
                })
                .unwrap_or_default();
            groups.sort();
            groups.dedup();
            // sorted groups → the first "G*" is G1 when present, else the first guitar
            // group; default to "G1" (the capture's group) when none/none-guitar.
            groups
                .into_iter()
                .find(|g| g.starts_with('G'))
                .unwrap_or_else(|| "G1".to_string())
        }
    };
    report.push_str(&format!(
        "  active preset {name:?}  list_index={list_index} (device slot {})  group={target_group}  insert_after={after:?}\n",
        list_index + 1
    ));
    drop(s);
    std::thread::sleep(std::time::Duration::from_millis(600));

    // ── Held session: load+re-arm the active preset, insert, verify, save|revert ──
    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    let item = held_insert_one(
        &mut s,
        list_index,
        &name,
        &target_group,
        after,
        fender_id,
        commit,
    )
    .unwrap_or_else(|e| BulkReplaceItem {
        slot: list_index,
        name: name.clone(),
        outcome: "error".to_string(),
        detail: e,
    });
    report.push_str(&format!("  result: {} — {}\n", item.outcome, item.detail));
    drop(s);

    // ── Verify the PERSISTED state on a fresh clean session (field-8 slot read) ──
    std::thread::sleep(std::time::Duration::from_millis(600));
    let dev_slot = list_index + 1;
    let mut v = Session::connect()?;
    v.drain_until_quiet(250, 20)?;
    match v.read_slot_preset_json(dev_slot)? {
        Some(raw) => {
            if let Some(vval) = session::tolerant_parse_json(&String::from_utf8_lossy(&raw)) {
                let n = audiograph::count_nodes_with_id(&vval, fender_id);
                report.push_str(&format!(
                    "  VERIFY (field-8, persisted) slot {dev_slot:03}: {fender_id}×{n}  {}\n",
                    if commit {
                        "(expect ×1 after commit)"
                    } else {
                        "(expect ×0 after dry run)"
                    }
                ));
            } else {
                report.push_str(&format!(
                    "  VERIFY slot {dev_slot:03}: (re-read did not parse)\n"
                ));
            }
        }
        None => report.push_str(&format!(
            "  VERIFY slot {dev_slot:03}: (field-8 read returned no JSON)\n"
        )),
    }
    Ok(report)
}

/// Ordered FenderIds of the blocks in `group`, in signal order, from a parsed preset.
fn group_roster_fender_ids(v: &serde_json::Value, group: &str) -> Vec<String> {
    audiograph::roster(v)
        .into_iter()
        .filter(|(g, _, _)| g == group)
        .map(|(_, _, fid)| fid)
        .collect()
}

/// Live ordered FenderIds in `group` off the held session, retry-pumping because the
/// post-edit field-3 push can lag a single heartbeat window.
fn ordered_group(s: &mut Session, group: &str) -> Vec<String> {
    for _ in 0..10 {
        let _ = s.heartbeat();
        let _ = s.pump_collect(250);
        if let Ok(v) = s.current_preset_value() {
            let roster = group_roster_fender_ids(&v, group);
            if !roster.is_empty() {
                return roster;
            }
        }
    }
    Vec::new()
}

/// Ordered FenderIds in `group` of the SAVED preset at `device_slot` (field-8 read, the
/// reliable post-save order source — the live working copy doesn't refresh after an edit
/// on a lean session).
fn field8_group_order(device_slot: u32, group: &str) -> Vec<String> {
    let read = || -> Result<Vec<String>, String> {
        let mut v = Session::connect()?;
        v.drain_until_quiet(250, 20)?;
        let raw = v
            .read_slot_preset_json(device_slot)?
            .ok_or_else(|| "no field-8 JSON".to_string())?;
        let val = session::tolerant_parse_json(&String::from_utf8_lossy(&raw))
            .ok_or_else(|| "field-8 did not parse".to_string())?;
        Ok(group_roster_fender_ids(&val, group))
    };
    read().unwrap_or_default()
}

/// EMPIRICAL insert-placement mapping (`probe --insert-map <slot> <group> <fenderId>
/// [--before <id>] [--at-index <n>]`). Loads the slot on a held re-armed session, prints
/// the ORDERED group roster, sends ONE insert (field-34 before-anchor when `--before`,
/// else field-99 `insertNodeAtBlockIndex` when `--at-index`, else a bare append), prints
/// the ordered roster again, then either COMMITs (saves + field-8 readback) or REVERTs
/// (reload, live readback). Used to nail down what each wire op does to the in-group ORDER.
pub fn probe_insert_map(
    device_slot: u32,
    group: &str,
    fender_id: &str,
    before: Option<&str>,
    at_index: Option<u32>,
    commit: bool,
) -> Result<String, String> {
    let list_index = device_slot.saturating_sub(1);
    let mut report = String::new();
    report.push_str(&format!(
        "[probe --insert-map] slot {device_slot:03} group={group} insert={fender_id} before={before:?} at_index={at_index:?} ({})\n",
        if commit { "COMMIT (saves, field-8 readback)" } else { "DRY (reverted, live readback)" }
    ));

    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    let name = s
        .list_my_presets()
        .ok()
        .and_then(|ps| {
            ps.into_iter()
                .find(|p| p.slot == list_index)
                .map(|p| p.name)
        })
        .unwrap_or_default();

    // Load + re-arm the target preset (the held_insert_one preamble).
    s.clear_raw();
    s.send_and_collect(&proto::load_preset(device_slot as u64, 1), 200)?;
    s.send_and_collect(&proto::connection_request(), 80)?;
    s.send_and_collect(&proto::preset_list_request(1, 1), 20)?;
    s.send_and_collect(&proto::current_preset_info_request(2), 120)?;
    let _ = s.await_active_preset(&name, 8); // pump for the fresh currentPresetInfoChanged
    if !s.active_matches(list_index, Some(&name)) {
        return Err(format!(
            "could not confirm slot {device_slot} loaded (loaded={:?}, active={:?})",
            s.loaded_slot(),
            s.active_preset_name()
        ));
    }

    report.push_str(&format!(
        "  BEFORE {group}: {:?}\n",
        ordered_group(&mut s, group)
    ));

    // ONE insert (retry once past the cold-first-edit silent drop, never past a reject).
    let do_insert = |s: &mut Session| match at_index {
        Some(idx) => s.insert_node_at_index(group, idx, fender_id),
        None => s.insert_node(group, before, fender_id),
    };
    let mut confirmed = do_insert(&mut s)?;
    if !confirmed && !s.saw_preset_error() {
        confirmed = do_insert(&mut s)?;
    }
    let seen = s.seen_preset_fields();
    let rejected = s.saw_preset_error();

    if rejected || !confirmed {
        report.push_str(&format!(
            "  REJECTED/UNCONFIRMED confirmed={confirmed} presetError={rejected} reply_fields={seen:?} — reverting\n"
        ));
        s.clear_raw();
        let _ = s.send_and_collect(&proto::load_preset(device_slot as u64, 1), 200);
        return Ok(report);
    }

    // COMMIT → identity-preserving save + field-8 readback (reliable); DRY → re-prompt a
    // best-effort live read, then revert by reloading.
    let after_order = if commit {
        if !name.is_empty() {
            s.rename_current_preset(&name)?;
        }
        s.save_current_preset(list_index)?;
        s.heartbeat()?;
        s.pump_collect(120)?;
        drop(s);
        std::thread::sleep(std::time::Duration::from_millis(600));
        field8_group_order(device_slot, group)
    } else {
        let _ = s.send_and_collect(&proto::connection_request(), 80);
        let _ = s.send_and_collect(&proto::current_preset_data_request(2), 200);
        let order = ordered_group(&mut s, group);
        s.clear_raw();
        s.send_and_collect(&proto::load_preset(device_slot as u64, 1), 200)?;
        s.heartbeat()?;
        s.pump_collect(120)?;
        order
    };
    report.push_str(&format!(
        "  AFTER ({}) {group}: {after_order:?}\n  confirmed={confirmed} reply_fields={seen:?}\n",
        if commit { "field-8, saved" } else { "live" }
    ));
    Ok(report)
}

/// Insert one block into the preset at 0-based `list_index` on a HELD session — the
/// `held_replace_one` shape, with `insertNode` instead of `replaceNode`. Load + re-arm +
/// the same SAFETY gate (only proceed when the held session re-attached to the TARGET
/// preset). The insert gets a single RETRY on a silent DROP (the held path's cold first
/// structural edit after a fresh load can be dropped; an immediate retry lands it), but
/// NEVER on a `presetError` (a rejection — never saved). Saves only when the edit is
/// confirmed (nodeInserted) OR read back as present, and never on a presetError.
#[allow(clippy::too_many_arguments)]
fn held_insert_one(
    s: &mut Session,
    list_index: u32,
    name: &str,
    group: &str,
    after: Option<&str>,
    fender_id: &str,
    save: bool,
) -> Result<BulkReplaceItem, String> {
    // LOAD on the held session + RE-ARM the edit context to the just-loaded preset.
    s.clear_raw();
    s.send_and_collect(&proto::load_preset((list_index + 1) as u64, 1), 200)?;
    s.send_and_collect(&proto::connection_request(), 80)?;
    s.send_and_collect(&proto::preset_list_request(1, 1), 20)?;
    s.send_and_collect(&proto::current_preset_info_request(2), 120)?;
    let _ = s.await_active_preset(name, 8); // pump for the fresh currentPresetInfoChanged
                                            // SAFETY — confirm the held session re-attached to the TARGET preset (active_matches
                                            // prefers the PresetLoaded slot echo, falling back to the active name) before editing.
    if !s.active_matches(list_index, Some(name)) {
        return Ok(BulkReplaceItem {
            slot: list_index,
            name: name.to_string(),
            outcome: "error".to_string(),
            detail: format!(
                "could not confirm target preset loaded (slot {:?} ≠ {list_index}, active {:?} ≠ {name:?}) — not edited",
                s.loaded_slot(),
                s.active_preset_name()
            ),
        });
    }

    // INSERT — bare insertNode, with a single retry for the cold-first-edit DROP.
    let mut confirmed = s.insert_node(group, after, fender_id)?;
    let mut seen = s.seen_preset_fields();
    let mut rejected = s.saw_preset_error();
    if !confirmed && !rejected {
        confirmed = s.insert_node(group, after, fender_id)?;
        seen = s.seen_preset_fields();
        rejected = s.saw_preset_error();
    }

    // Content read-back: coax a fresh field-3 push, then check the block is present.
    s.heartbeat()?;
    s.pump_collect(250)?;
    let present = s
        .current_preset_value()
        .ok()
        .map(|v| {
            audiograph::roster(&v)
                .iter()
                .any(|(g, _, fid)| g == group && fid == fender_id)
        })
        .unwrap_or(false);
    let detail = format!(
        "nodeInserted(33)={confirmed} presetError={rejected} readback_present={present} reply_fields={seen:?}"
    );

    if rejected {
        return Ok(BulkReplaceItem {
            slot: list_index,
            name: name.to_string(),
            outcome: "rejected".to_string(),
            detail: format!("device sent presetError — NOT saved. {detail}"),
        });
    }
    if !confirmed && !present {
        return Ok(BulkReplaceItem {
            slot: list_index,
            name: name.to_string(),
            outcome: "unconfirmed".to_string(),
            detail: format!("no nodeInserted + block absent from read-back — NOT saved. {detail}"),
        });
    }

    if save {
        if !name.is_empty() {
            s.rename_current_preset(name)?;
        }
        s.save_current_preset(list_index)?;
        s.heartbeat()?;
        s.pump_collect(120)?;
        Ok(BulkReplaceItem {
            slot: list_index,
            name: name.to_string(),
            outcome: "inserted+saved".to_string(),
            detail,
        })
    } else {
        // DRY: discard the live edit by reloading the same preset.
        s.clear_raw();
        s.send_and_collect(&proto::load_preset((list_index + 1) as u64, 1), 200)?;
        s.heartbeat()?;
        s.pump_collect(120)?;
        Ok(BulkReplaceItem {
            slot: list_index,
            name: name.to_string(),
            outcome: "inserted (dry, reverted)".to_string(),
            detail,
        })
    }
}
