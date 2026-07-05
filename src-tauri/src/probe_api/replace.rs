//! Probe entry points: live block-replace (`bulk_replace_live` / `replace-held` / bulk saved-block).

use crate::session::Session;
use crate::audiograph;
use crate::proto;
use crate::saved_blocks::{SavedBlock, find_block_presets_blob, parse_block_presets_map};
use crate::session;
use crate::{BulkReplaceItem, ReplArg, discover_replace_plans, held_replace_one, replace_many_held};

/// Instrumented single-node ReplaceNode diagnostic (`probe --replace-debug SLOT FROM TO`):
/// loads the preset, settles, sends `replaceNode`, and DUMPS the device's reply (looking
/// for `nodeReplaced`/40 vs `connectionError`) so we can see why a replace does/doesn't
/// take. Then saves + re-reads to verify. Read+write (gated on explicit invocation).
pub fn probe_replace_debug(dev_slot: u32, from_id: &str, to_id: &str) -> Result<String, String> {
    let hexn = |b: &[u8], n: usize| {
        b.iter()
            .take(n)
            .map(|x| format!("{x:02x}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let list_index = dev_slot.saturating_sub(1);
    let mut report = String::new();

    // ── Connection 1: find the target node, then LOAD the preset (make it the
    //    device's active/edit preset) and DROP — the active preset persists across
    //    reconnects, so a fresh session then edits it the way Pro Control does
    //    (PC never re-loads; it edits whatever is active). TMP_REPLACE_ONECONN keeps
    //    the old single-connection load+replace for A/B.
    let oneconn = std::env::var("TMP_REPLACE_ONECONN").is_ok();
    let (group, node_id, cur_name) = {
        let mut s1 = Session::connect()?;
        s1.drain_until_quiet(250, 20)?;
        let raw = s1
            .read_slot_preset_json(dev_slot)?
            .ok_or("no field-8 JSON")?;
        let value = session::tolerant_parse_json(&String::from_utf8_lossy(&raw)).ok_or("parse")?;
        let node = audiograph::roster(&value)
            .into_iter()
            .find(|(_, _, fid)| fid == from_id);
        let Some((group, node_id, _)) = node else {
            return Ok(format!("slot {dev_slot}: no node {from_id} found\n"));
        };
        let cur_name = value
            .pointer("/info/displayName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        report.push_str(&format!(
            "target: slot {dev_slot} group={group} nodeId={node_id} name={cur_name:?} → {to_id}\n"
        ));
        s1.load_preset(list_index)?;
        s1.heartbeat()?;
        s1.pump_collect(900)?;
        (group, node_id, cur_name)
    };

    // ── Connection 2: fresh session on the now-active preset (NO reload), mirroring
    //    Pro Control. The seize was released by dropping s1; reconnect.
    let mut s = Session::connect()?;
    // CRITICAL: do NOT drain_until_quiet here — a multi-second silent drain lets the
    // session LAPSE, and the device then answers every structural request with an
    // empty connectionError (the live-controller-status gotcha; same reason
    // device_backup must heartbeat throughout). Instead hold a dense ~200 ms
    // heartbeat to KEEP live-controller status, the way Pro Control does.
    for _ in 0..8 {
        s.heartbeat()?;
        s.pump_collect(200)?;
    }
    if oneconn {
        s.load_preset(list_index)?;
        for _ in 0..4 {
            s.heartbeat()?;
            s.pump_collect(200)?;
        }
        report.push_str("conn2: ONECONN — reloaded preset in the edit connection\n");
    } else {
        report.push_str("conn2: fresh session, dense-heartbeat live-controller (no reload)\n");
    }

    // 3) Pro Control sends nodeJsonRequest(119){groupId,nodeId} immediately BEFORE
    //    replaceNode to enter the node-edit context (device replies nodeJsonResponse/
    //    120). Sent INSIDE the live heartbeat cadence. TMP_REPLACE_NO_NODEJSON skips it.
    let send_nodejson = std::env::var("TMP_REPLACE_NO_NODEJSON").is_err();
    if send_nodejson {
        s.clear_raw();
        s.send_and_collect(&proto::node_json_request(&group, &node_id), 200)?;
        s.heartbeat()?;
        s.pump_collect(200)?;
        let got_120 = s.push_bodies().iter().any(|b| {
            proto::first_bytes(&proto::parse(b), 2)
                .map(|pm| proto::first_bytes(&proto::parse(pm), 120).is_some())
                .unwrap_or(false)
        });
        report.push_str(&format!(
            "nodeJsonRequest(119) sent → nodeJsonResponse(120): {got_120}\n"
        ));
    } else {
        report.push_str("nodeJsonRequest(119): SKIPPED (TMP_REPLACE_NO_NODEJSON)\n");
    }

    // 4) send replaceNode INSIDE the live heartbeat cadence — framing per
    //    TMP_REPLACE_NOBATCH (Pro Control uses NO batchStatus).
    let nobatch = std::env::var("TMP_REPLACE_NOBATCH").is_ok();
    let batch = if nobatch { None } else { Some(11u64) };
    report.push_str(&format!(
        "framing: {}\n",
        if nobatch {
            "NO batchStatus"
        } else {
            "WITH batchStatus=11"
        }
    ));
    s.clear_raw();
    s.send_chunked_collect(&proto::replace_node(&group, &node_id, to_id, batch), 200)?;
    for _ in 0..8 {
        s.heartbeat()?;
        s.pump_collect(200)?;
    }
    // 4) dump every reply body (streams + push bodies) — TMS top + presetMessage
    //    inner fields, flagging nodeReplaced(40) and any NON-empty connectionError.
    let bodies: Vec<Vec<u8>> = s.push_bodies();
    report.push_str(&format!("  {} reply bodies\n", bodies.len()));
    for (bi, body) in bodies.iter().enumerate() {
        let top = proto::parse(body);
        let tf: Vec<u32> = top.iter().map(|(f, _)| *f).collect();
        let mut inner_desc = String::new();
        if let Some(pm) = proto::first_bytes(&top, 2) {
            let inner = proto::parse(pm);
            let ifs: Vec<u32> = inner.iter().map(|(x, _)| *x).collect();
            inner_desc = format!(" presetMsg inner {ifs:?}");
            if proto::first_bytes(&inner, 40).is_some() {
                inner_desc.push_str("  ← nodeReplaced(40)!");
            }
            if proto::first_bytes(&inner, 3).is_some() {
                inner_desc.push_str("  ← currentPresetDataChanged(3)!");
            }
        }
        if let Some(cm) = proto::first_bytes(&top, 4) {
            let inner = proto::parse(cm);
            if let Some(err) = proto::first_bytes(&inner, 3) {
                let tag = if err.is_empty() {
                    "(empty/ack)"
                } else {
                    "NON-EMPTY"
                };
                inner_desc.push_str(&format!("  ← connectionError {tag} {}", hexn(err, 16)));
            }
        }
        report.push_str(&format!(
            "  reply {bi}: [{}B] top {tf:?}{inner_desc}\n",
            body.len()
        ));
    }

    // 5) optional save (TMP_REPLACE_SAVE) + re-read verify via the reliable field-8.
    //    Pro Control persists a structural edit as replaceNode → renameCurrentPreset
    //    (current name) → saveCurrentPreset(slot) — the rename may be load-bearing
    //    for persistence. TMP_REPLACE_NORENAME skips it for A/B.
    if std::env::var("TMP_REPLACE_SAVE").is_ok() {
        if std::env::var("TMP_REPLACE_NORENAME").is_err() && !cur_name.is_empty() {
            s.send_and_collect(&proto::rename_current_preset(&cur_name), 300)?;
            report.push_str(&format!("renameCurrentPreset({cur_name:?}) sent\n"));
        }
        s.save_current_preset(list_index)?;
        s.pump_collect(400)?;
        let vraw = s.read_slot_preset_json(dev_slot)?.unwrap_or_default();
        if let Some(vval) = session::tolerant_parse_json(&String::from_utf8_lossy(&vraw)) {
            report.push_str(&format!(
                "verify after save: {from_id}×{}  {to_id}×{}\n",
                audiograph::count_nodes_with_id(&vval, from_id),
                audiograph::count_nodes_with_id(&vval, to_id)
            ));
        }
    } else {
        report.push_str("(not saved — set TMP_REPLACE_SAVE=1 to persist + verify)\n");
    }
    drop(s);
    Ok(report)
}

/// End-to-end test of Bulk Block Edit's live REPLACE (`probe --bulk-replace FROM TO
/// SLOTS [--commit]`): SLOTS are 1-based device slots. Without `--commit` it is a
/// READ-ONLY dry run — it loads each preset, prints its amp/block roster, and reports
/// which would change (matching `FROM`) vs skip (no match). With `--commit` it applies
/// the swap via the exact held-session production path (`replace_many_held`) + saves,
/// then re-reads to verify. This is the same code path the UI's `bulk_replace_live`
/// command serves.
pub fn probe_bulk_replace(
    from_id: &str,
    to_id: &str,
    device_slots: &[u32],
    commit: bool,
) -> Result<String, String> {
    let mut report = String::new();
    report.push_str(&format!(
        "[probe --bulk-replace] {} → {} on slots {:?} ({})\n",
        from_id,
        to_id,
        device_slots,
        if commit {
            "COMMIT (device write + save)"
        } else {
            "DRY RUN (read-only)"
        }
    ));
    let repl = ReplArg::Model {
        fender_id: to_id.to_string(),
    };

    // Phase 1 — discovery: read every selected slot ONCE and build the target plans.
    let plans = discover_replace_plans(device_slots, from_id)?;
    for (plan, &dev_slot) in plans.iter().zip(device_slots) {
        report.push_str(&format!(
            "\n  slot {dev_slot:03} (list {}) {:?}\n    {} matching {from_id}  → would {}\n",
            plan.list_index,
            plan.name,
            plan.targets.len(),
            if plan.targets.is_empty() {
                "SKIP (no matching block)"
            } else {
                "REPLACE"
            }
        ));
    }
    if !commit {
        report.push_str(
            "\nNOTE: dry run is read-only; --commit writes + saves each matching preset.\n",
        );
        return Ok(report);
    }
    std::thread::sleep(std::time::Duration::from_millis(600));

    // Phase 2 — commit: the EXACT production path (`replace_many_held`: ONE held
    // session, re-armed per preset, with the active-preset + nodeReplaced guards).
    report.push_str("\n  COMMIT (held session):\n");
    let t_commit = std::time::Instant::now();
    let items = replace_many_held(&plans, &repl, true, |_item| {})?;
    for item in &items {
        let dev_slot = item.slot + 1;
        report.push_str(&format!(
            "    slot {dev_slot:03}: {} ({})\n",
            item.outcome, item.detail
        ));
    }
    report.push_str(&format!(
        "    (commit total {:.2}s)\n",
        t_commit.elapsed().as_secs_f64()
    ));
    std::thread::sleep(std::time::Duration::from_millis(600));

    // Phase 3 — verify: one read-only session re-reads each slot and counts ids.
    report.push_str("\n  VERIFY (after save):\n");
    {
        let mut s = Session::connect()?;
        s.drain_until_quiet(250, 20)?;
        for &dev_slot in device_slots {
            let vraw = s.read_slot_preset_json(dev_slot)?.unwrap_or_default();
            if let Some(vval) = session::tolerant_parse_json(&String::from_utf8_lossy(&vraw)) {
                let now_from = audiograph::count_nodes_with_id(&vval, from_id);
                let now_to = audiograph::count_nodes_with_id(&vval, to_id);
                report.push_str(&format!(
                    "    slot {dev_slot:03}: {from_id}×{now_from}  {to_id}×{now_to}\n"
                ));
            } else {
                report.push_str(&format!("    slot {dev_slot:03}: (re-read failed)\n"));
            }
        }
    }
    Ok(report)
}

/// E1 — held-session architecture experiment (`probe --replace-held <FROM> <TO> <slots_csv> [--commit]`).
/// Tests whether ONE session, re-armed after each load, accepts a CONFIRMED
/// `replaceNode` — vs the proven two-connection path (conn1 load → drop → conn2
/// re-attach). If it works, the per-preset cost collapses from ~8 s (two handshakes
/// plus a settle) to ~2 s (zero reopens). Discovery is done up front on a CLEAN session
/// (the proven field-8 path) so this isolates the one variable: does a held re-armed
/// session's edit get `nodeReplaced(40)` and persist? Per-preset timing is printed.
/// The same safety gate as production: a save only when the active preset matches the
/// target AND every replace is confirmed — so even a `--commit` run can't corrupt.
pub fn probe_replace_held(
    from_id: &str,
    to_id: &str,
    device_slots: &[u32],
    commit: bool,
) -> Result<String, String> {
    use std::time::Instant;
    let mut report = String::new();
    report.push_str(&format!(
        "[probe --replace-held] {from_id} → {to_id} on slots {device_slots:?} ({})\n",
        if commit {
            "COMMIT (held-session write + save)"
        } else {
            "DRY RUN (edit attempted, NOT saved)"
        }
    ));
    let repl = ReplArg::Model {
        fender_id: to_id.to_string(),
    };

    // Discovery up front on a CLEAN session (proven field-8 path): isolates the
    // experiment to the held-session EDIT, not field-3 read reliability.
    let plans = discover_replace_plans(device_slots, from_id)?;
    for (plan, &dev_slot) in plans.iter().zip(device_slots) {
        report.push_str(&format!(
            "  slot {dev_slot:03} (list {}) {:?}: {} matching {from_id}\n",
            plan.list_index,
            plan.name,
            plan.targets.len()
        ));
    }
    std::thread::sleep(std::time::Duration::from_millis(600));

    // ── ONE held session via the PRODUCTION held path (`held_replace_one`), with
    //    per-preset timing. `save=commit`: a dry run attempts the edit (confirming
    //    `nodeReplaced`) but never persists — the field-8 verify below is the oracle. ──
    let t_conn = Instant::now();
    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    report.push_str(&format!(
        "\n  HELD-SESSION (one connection, re-armed per preset; connect+warmup {:.2}s):\n",
        t_conn.elapsed().as_secs_f64()
    ));
    for plan in &plans {
        let dev_slot = plan.list_index + 1;
        let t0 = Instant::now();
        let item =
            held_replace_one(&mut s, plan, &repl, commit).unwrap_or_else(|e| BulkReplaceItem {
                slot: plan.list_index,
                name: plan.name.clone(),
                outcome: "error".to_string(),
                detail: e,
            });
        report.push_str(&format!(
            "    slot {dev_slot:03} {:?}: {} ({})  [{:.2}s]\n",
            plan.name,
            item.outcome,
            item.detail,
            t0.elapsed().as_secs_f64()
        ));
    }
    drop(s);

    // ── Verify (fresh CLEAN session, field-8) ──
    std::thread::sleep(std::time::Duration::from_millis(600));
    report.push_str("\n  VERIFY (after, field-8):\n");
    {
        let mut v = Session::connect()?;
        v.drain_until_quiet(250, 20)?;
        for &dev_slot in device_slots {
            let vraw = v.read_slot_preset_json(dev_slot)?.unwrap_or_default();
            if let Some(vval) = session::tolerant_parse_json(&String::from_utf8_lossy(&vraw)) {
                let now_from = audiograph::count_nodes_with_id(&vval, from_id);
                let now_to = audiograph::count_nodes_with_id(&vval, to_id);
                report.push_str(&format!(
                    "    slot {dev_slot:03}: {from_id}×{now_from}  {to_id}×{now_to}\n"
                ));
            } else {
                report.push_str(&format!("    slot {dev_slot:03}: (re-read failed)\n"));
            }
        }
    }
    Ok(report)
}

/// E6 — replace a block with a saved DUAL-CAB block across slots (`probe
/// --bulk-replace-saved <FROM> <slots_csv> [--commit]`), via the proven
/// two-connection `replace_one_live` path with `ReplArg::Saved`
/// (`replaceNodeWithBlock`, field 100 +index). Auto-picks the first
/// `dual_cabs_enabled` saved block on the device. Validates the saved-block replace
/// end-to-end (previously only the stock-model/IR path was HW-validated). FROM should
/// name a cabinet node currently in the target presets (see `probe --roster`).
pub fn probe_bulk_replace_saved(
    from_id: &str,
    device_slots: &[u32],
    commit: bool,
) -> Result<String, String> {
    let mut report = String::new();
    // Resolve the saved dual-cab block (synchronous decode, same path as probe_saved_blocks).
    let saved = {
        let mut s =
            Session::connect_with_burst_request(&proto::request_all_block_presets(Some(2)))?;
        for _ in 0..4 {
            s.pump_collect(250)?;
        }
        let bodies = s.push_bodies();
        drop(s);
        let blob = find_block_presets_blob(&bodies)
            .ok_or_else(|| "device sent no allBlockPresetsResponse".to_string())?;
        parse_block_presets_map(&blob)?
    };
    let duals: Vec<&SavedBlock> = saved.iter().filter(|b| b.dual_cabs_enabled).collect();
    for d in &duals {
        report.push_str(&format!(
            "  dual-cab candidate: {} [idx {}] {:?} cab1={:?} cab2={:?}{}\n",
            d.fender_id,
            d.index,
            d.name,
            d.cab1_id,
            d.cab2_id,
            if d.cab1_id != d.cab2_id {
                " (TRUE dual)"
            } else {
                " (cab1==cab2)"
            }
        ));
    }
    // Prefer a GENUINE user-named dual-cab over an autogen default (cab1≠cab2 is the
    // strongest signal, but a user-named block is the realistic test even if the two
    // cabs share a model).
    let is_autogen =
        |b: &&SavedBlock| b.name.to_lowercase().contains("autogen") || b.name.is_empty();
    let dual = duals
        .iter()
        .find(|b| !is_autogen(b) && b.cab1_id != b.cab2_id)
        .or_else(|| duals.iter().find(|b| !is_autogen(b)))
        .or_else(|| duals.iter().find(|b| b.cab1_id != b.cab2_id))
        .or_else(|| duals.first())
        .copied()
        .ok_or_else(|| "no dual-cab saved block on the device".to_string())?;
    report.push_str(&format!(
        "[probe --bulk-replace-saved] {from_id} → saved dual-cab {} [idx {}] {:?} (cab1={:?} cab2={:?}) on slots {device_slots:?} ({})\n",
        dual.fender_id, dual.index, dual.name, dual.cab1_id, dual.cab2_id,
        if commit { "COMMIT (device write + save)" } else { "DRY RUN (read-only)" }
    ));
    let repl = ReplArg::Saved {
        fender_id: dual.fender_id.clone(),
        index: dual.index as u64,
    };
    let to_id = dual.fender_id.clone();
    std::thread::sleep(std::time::Duration::from_millis(600));

    let plans = discover_replace_plans(device_slots, from_id)?;
    for (plan, &dev_slot) in plans.iter().zip(device_slots) {
        report.push_str(&format!(
            "  slot {dev_slot:03} {:?}: {} matching {from_id}\n",
            plan.name,
            plan.targets.len()
        ));
    }
    if !commit {
        report.push_str("\nNOTE: dry run is read-only; --commit applies the saved dual-cab.\n");
        return Ok(report);
    }
    std::thread::sleep(std::time::Duration::from_millis(600));
    report.push_str("\n  COMMIT (held session):\n");
    let items = replace_many_held(&plans, &repl, true, |_item| {})?;
    for item in &items {
        let dev_slot = item.slot + 1;
        report.push_str(&format!(
            "    slot {dev_slot:03}: {} ({})\n",
            item.outcome, item.detail
        ));
    }
    std::thread::sleep(std::time::Duration::from_millis(600));
    report.push_str("\n  VERIFY (field-8):\n");
    {
        let mut v = Session::connect()?;
        v.drain_until_quiet(250, 20)?;
        for &dev_slot in device_slots {
            let vraw = v.read_slot_preset_json(dev_slot)?.unwrap_or_default();
            if let Some(vval) = session::tolerant_parse_json(&String::from_utf8_lossy(&vraw)) {
                let now_from = audiograph::count_nodes_with_id(&vval, from_id);
                let now_to = audiograph::count_nodes_with_id(&vval, &to_id);
                report.push_str(&format!(
                    "    slot {dev_slot:03}: {from_id}×{now_from}  {to_id}×{now_to}\n"
                ));
            } else {
                report.push_str(&format!("    slot {dev_slot:03}: (re-read failed)\n"));
            }
        }
    }
    Ok(report)
}
