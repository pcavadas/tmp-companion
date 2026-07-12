//! "Copy blocks between presets" — ordered replace/insert/remove op apply.
#![allow(clippy::too_many_arguments)]
use crate::*;

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
/// Cooperative cancel for [`copy_apply`] — set by `cancel_copy_apply` (the Copy
/// wizard's Stop), checked between presets so the held-session run stops WRITING the
/// remaining presets. Presets already saved stay changed; the rest are untouched.
static COPY_APPLY_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Stop an in-flight [`copy_apply`] run after the current preset. Lightweight (just
/// sets the flag) so it does NOT take the device-op lock — it must run while the run
/// holds it.
#[tauri::command]
pub(crate) fn cancel_copy_apply() {
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
pub(crate) async fn copy_apply(
    state: State<'_, AppState>,
    jobs: Vec<CopyJob>,
    save: bool,
    on_result: tauri::ipc::Channel<CopyApplyItem>,
) -> Result<Vec<CopyApplyItem>, String> {
    COPY_APPLY_CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
    // Saves change stored presets -- the Doctor's cached BEFORE clip goes stale.
    crate::commands::doctor::clear_doctor_before_cache();
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

#[cfg(test)]
#[path = "copy_e2e_tests.rs"]
mod copy_e2e_tests;
