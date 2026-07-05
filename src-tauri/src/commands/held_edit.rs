//! Held-session single/many replace helpers shared by the block-edit writers.
#![allow(clippy::too_many_arguments)]
use crate::*;

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
