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

/// Ordered FenderIds in `group` off whatever field-3 document the held session has
/// BUFFERED (retry-pumping for a lagging load push). A buffer read, not a re-prompt: after
/// a structural edit it shows nothing new — use `Session::live_preset_value` for the
/// working copy.
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

/// Ordered FenderIds in `group` of the SAVED preset at `device_slot` (field-8 read — the
/// stored document, i.e. what a save actually persisted).
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

/// `current_audio_graph` as one report line: does the buffered document parse to a graph
/// with its routing template (the product's truncation guard)?
fn describe_graph(s: &Session) -> String {
    match s.current_audio_graph() {
        Ok(g) => format!(
            "Ok(template={:?}, nodes={}, split_mix={})",
            g.template,
            g.nodes.len(),
            g.split_mix.is_some()
        ),
        Err(e) => format!("Err({e})"),
    }
}

/// EXPERIMENT (`probe --reprompt-map <slot> <name> <group> (--remove <nodeId> |
/// --insert <fenderId> [--before <id>]) [--commit]`): does a field-2 working-copy re-prompt sent on Copy's
/// EXACT held-session shape — load + `connection_request`/list/info re-arm, one confirmed
/// structural edit, then `Session::live_preset_value` (the `live_ftsw` wire shape: NO
/// re-arm, batch 3, heartbeat-pumped) — answer with the POST-edit roster? The recorded
/// pre-edit result (`notes/write-safety.md`) came from the `--insert-map` DRY arm, whose
/// re-prompt re-sent `connection_request` on the live session (the field-2 then goes
/// unanswered) and read its buffer without clearing it. Prints the roster before, the re-prompt's
/// roster twice (repeatability) with payload growth per pump then reverts by reload (DRY),
/// or runs the PRODUCT seam — `Session::live_audio_graph` then the immediate rename/save —
/// and reads the slot back over field-8 to prove the save persists the edit (COMMIT).
pub fn probe_reprompt_map(
    device_slot: u32,
    name: &str,
    group: &str,
    remove: Option<&str>,
    insert: Option<&str>,
    before: Option<&str>,
    commit: bool,
) -> Result<String, String> {
    let list_index = device_slot.saturating_sub(1);
    let mut report = String::new();
    report.push_str(&format!(
        "[probe --reprompt-map] slot {device_slot:03} group={group} remove={remove:?} insert={insert:?} before={before:?} ({})\n",
        if commit { "COMMIT (saves, field-8 readback)" } else { "DRY (reverted by reload)" }
    ));
    // The target's name comes from the caller, as it does for Copy (`CopyJob.name`). A
    // `list_my_presets` on the live session BEFORE the load left the whole load + re-arm
    // unanswered (fields=[] for 10 s, HW 2026-09-03; cause unresolved) — a separate
    // observation from the old arm's pre-edit reading, which `TMP_PROBE_LEGACY_REPROMPT`
    // below isolates.
    let name = name.to_string();
    let mut s = Session::connect()?;
    s.begin_live_edit()?;

    // Copy's per-preset preamble (`copy_apply_one`), verbatim.
    s.clear_raw();
    s.send_and_collect(&proto::load_preset(device_slot as u64, 1), 200)?;
    s.send_and_collect(&proto::connection_request(), 80)?;
    s.send_and_collect(&proto::preset_list_request(1, 1), 20)?;
    s.send_and_collect(&proto::current_preset_info_request(2), 120)?;
    // Timed confirm: Copy waits 8 × 150 ms; here keep pumping up to ~10 s and report
    // WHEN the echo landed, so a slow unit is measured rather than mis-read as a drop.
    let t_load = std::time::Instant::now();
    let mut matched_at = None;
    for _ in 0..64 {
        if s.active_matches(list_index, Some(&name)) {
            matched_at = Some(t_load.elapsed().as_millis());
            break;
        }
        let _ = s.heartbeat();
        let _ = s.pump_collect(150);
    }
    report.push_str(&format!(
        "  LOAD confirm: matched after {matched_at:?} ms (loaded={:?}, active={:?}, fields={:?})\n",
        s.loaded_slot(),
        s.active_preset_name(),
        s.seen_preset_fields()
    ));
    if !s.active_matches(list_index, Some(&name)) {
        return Err(format!(
            "could not confirm slot {device_slot} loaded (loaded={:?}, active={:?}, list name={name:?}, reply fields={:?})",
            s.loaded_slot(),
            s.active_preset_name(),
            s.seen_preset_fields()
        ));
    }
    let pre = ordered_group(&mut s, group);
    report.push_str(&format!("  BEFORE {group}: {pre:?}\n"));
    report.push_str(&format!(
        "  LOAD push: payload={}B current_audio_graph={}\n",
        s.json_payload_len(),
        describe_graph(&s)
    ));

    let do_op = |s: &mut Session| -> Result<bool, String> {
        match (remove, insert) {
            (Some(node), _) => s.remove_node(group, node),
            (None, Some(fid)) => s.insert_node(group, before, fid),
            (None, None) => Err("nothing to do".into()),
        }
    };
    let mut confirmed = do_op(&mut s)?;
    if !confirmed && !s.saw_preset_error() {
        confirmed = do_op(&mut s)?;
    }
    let seen = s.seen_preset_fields();
    if s.saw_preset_error() || !confirmed {
        report.push_str(&format!(
            "  REJECTED/UNCONFIRMED confirmed={confirmed} presetError={} reply_fields={seen:?} — reverting\n",
            s.saw_preset_error()
        ));
        s.clear_raw();
        let _ = s.send_and_collect(&proto::load_preset(device_slot as u64, 1), 200);
        return Ok(report);
    }
    report.push_str(&format!("  CONFIRMED reply_fields={seen:?}\n"));

    // `TMP_PROBE_LEGACY_REPROMPT=1`: the `--insert-map` DRY arm's re-prompt shape instead
    // (`connection_request` re-sent on the live session, batch 2, buffer read without a
    // clear) — to isolate which part of that shape produced the recorded pre-edit reading.
    if std::env::var("TMP_PROBE_LEGACY_REPROMPT").is_ok() {
        let _ = s.send_and_collect(&proto::connection_request(), 80);
        let _ = s.send_and_collect(&proto::current_preset_data_request(2), 200);
        let order = ordered_group(&mut s, group);
        report.push_str(&format!(
            "  LEGACY re-prompt (connection_request + batch 2, no clear): {group}={order:?} payload={}B carriers={:?} fields={:?}\n",
            s.json_payload_len(),
            s.json_payload_carriers(),
            s.seen_preset_fields()
        ));
    }
    if commit {
        // COMMIT = the PRODUCT seam with the product's timing: `live_audio_graph` (accept
        // → two stable slices) then the rename/save IMMEDIATELY, as `copy_apply_one` does —
        // the persistence sample must not be taken after extra quiet the product never has.
        let t0 = std::time::Instant::now();
        let want: Vec<String> = match (remove, insert) {
            (Some(node), _) => pre.iter().filter(|f| *f != node).cloned().collect(),
            (None, Some(fid)) => {
                let mut w = pre.clone();
                let at = before
                    .and_then(|b| w.iter().position(|f| f == b))
                    .unwrap_or(w.len());
                w.insert(at, fid.to_string());
                w
            }
            (None, None) => Vec::new(),
        };
        let graph = s.live_audio_graph(|v| group_roster_fender_ids(v, group) == want);
        report.push_str(&format!(
            "  PRODUCT read after {} ms: {}\n",
            t0.elapsed().as_millis(),
            match &graph {
                Ok(g) => format!(
                    "Ok(template={:?}, nodes={}) payload={}B",
                    g.template,
                    g.nodes.len(),
                    s.json_payload_len()
                ),
                Err(e) => format!("Err({e})"),
            }
        ));
        if !name.is_empty() {
            s.rename_current_preset(&name)?;
        }
        s.save_current_preset(list_index)?;
        s.heartbeat()?;
        s.pump_collect(120)?;
        drop(s);
        std::thread::sleep(std::time::Duration::from_millis(600));
        let after = field8_group_order(device_slot, group);
        report.push_str(&format!(
            "  AFTER (field-8, saved) {group}: {after:?} — {}\n",
            if after == want {
                "PERSISTED the edit"
            } else {
                "does NOT match the acked edit"
            }
        ));
        return Ok(report);
    }

    // DRY: the re-prompt twice — first parse accepted, then 4 more pumps to watch the
    // payload grow/stabilise (a partial mid-flight is the documented hazard) — then revert.
    for round in 1..=2 {
        let t0 = std::time::Instant::now();
        let first = s.live_preset_value(|v| !group_roster_fender_ids(v, group).is_empty());
        match first {
            Ok(v) => report.push_str(&format!(
                "  REPROMPT#{round} first accepted parse after {} ms: {group}={:?} payload={}B carriers={:?}\n",
                t0.elapsed().as_millis(),
                group_roster_fender_ids(&v, group),
                s.json_payload_len(),
                s.json_payload_carriers()
            )),
            Err(e) => report.push_str(&format!("  REPROMPT#{round} NO reply: {e}\n")),
        }
        report.push_str(&format!(
            "    current_audio_graph on the reply: {}\n",
            describe_graph(&s)
        ));
        for k in 1..=4 {
            let _ = s.heartbeat();
            let _ = s.pump_collect(200);
            let roster = s
                .current_preset_value()
                .map(|v| group_roster_fender_ids(&v, group))
                .unwrap_or_default();
            report.push_str(&format!(
                "    +pump{k}: {group}={roster:?} payload={}B\n",
                s.json_payload_len()
            ));
        }
    }

    s.clear_raw();
    s.send_and_collect(&proto::load_preset(device_slot as u64, 1), 200)?;
    s.heartbeat()?;
    s.pump_collect(120)?;
    let reverted = ordered_group(&mut s, group);
    report.push_str(&format!("  REVERTED (reload) {group}: {reverted:?}\n"));
    Ok(report)
}
