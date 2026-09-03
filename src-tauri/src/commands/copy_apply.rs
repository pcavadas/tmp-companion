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
/// [`BulkReplaceItem`] (`slot`/`name`/`outcome`/`detail`) plus the signal `graph` the
/// device's WORKING COPY showed in a field-2 re-prompt taken after the last confirmed op
/// and BEFORE the save (the save itself is unacknowledged), so the Copy view can patch its
/// cached library in place (no ~22 s re-scan) after a write. `graph` is `None` when the
/// preset wasn't saved, the read could not be verified (a roster-invariant edit), no
/// reply landed, or the reply did not show the blocks the acked ops produced (see
/// [`read_working_copy`]).
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
        return Ok(error_item(list_index, &name, format!(
                "could not confirm target preset loaded on held session (slot {:?} ≠ {list_index}, active {:?} ≠ target {name:?}) — not edited",
                s.loaded_slot(),
                s.active_preset_name()
            )));
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
        // An IR/saved INSERT addresses the node it adds by FenderId (on the unit a node id
        // IS its FenderId), so a group that ALREADY holds that model — after the ops so far
        // — would make the follow-up swap ambiguous (it could re-point an existing block).
        // (A cap-legal case: two cabinets in one group.)
        if let CopyOp::Insert { group, repl, .. } = op {
            // Refuse, never guess — and when the ops so far cannot be modelled at all
            // (`expected_roster` → `None`, the same `None` that refuses the read), the
            // group's contents are unknown, so the address is unverifiable: refuse too.
            let ambiguous = !matches!(repl, CopyRepl::Model { .. })
                && expected_roster(&roster, &job.ops[..i]).is_none_or(|r| {
                    r.get(group)
                        .is_some_and(|ids| ids.iter().any(|id| id == repl.insert_fender_id()))
                });
            if ambiguous {
                return Ok(error_item(
                    list_index,
                    &name,
                    format!(
                        "op {}/{total} ({}) refused: {group} already holds a {} block (or the \
                         ops so far cannot be modelled), so the inserted node's id would be \
                         ambiguous — NOT saved",
                        i + 1,
                        describe_copy_op(op),
                        repl.insert_fender_id()
                    ),
                ));
            }
        }
        let replaced = target.and_then(|(g, n)| blockcaps_replaced(&roster, g, n));
        if let Err(reason) = blockcaps_check(&counts, candidate_id, is_replace, replaced) {
            return Ok(error_item(
                list_index,
                &name,
                format!(
                    "op {}/{total} ({}) blocked by block-count cap: {reason} — NOT saved",
                    i + 1,
                    describe_copy_op(op)
                ),
            ));
        }

        match apply_copy_op(s, op, first) {
            Ok(true) => {
                blockcaps_advance(&mut counts, candidate_id, replaced);
            }
            Ok(false) => {
                return Ok(error_item(
                    list_index,
                    &name,
                    format!(
                        "device rejected op {}/{total} ({}) — presetError / no confirm — NOT saved",
                        i + 1,
                        describe_copy_op(op)
                    ),
                ));
            }
            Err(e) => {
                return Ok(error_item(
                    list_index,
                    &name,
                    format!(
                        "op {}/{total} ({}) failed: {e} — NOT saved",
                        i + 1,
                        describe_copy_op(op)
                    ),
                ));
            }
        }
    }

    // The working-copy read for the Copy view's in-place cache patch (no ~22 s re-scan):
    // a field-2 re-prompt of the device's LIVE document, taken after the last confirmed op
    // and BEFORE the save (the footswitch flow's HW-proven re-prompt-then-save ordering),
    // never a scrape of whatever the session still had buffered. Only on save (an unsaved
    // edit must not poison the cache), and only when the read can be verified against the
    // acked ops (`read_working_copy`).
    let graph = if save {
        // What the acked ops leave behind — the oracle the read is checked against. An
        // edit that leaves the roster as it was (a same-model re-stamp: on the unit a node
        // id IS its FenderId, so nothing the roster can see changes) is unverifiable —
        // pre-edit and post-edit documents look identical — so no read is spent on it.
        let pre = group_roster(
            roster
                .iter()
                .map(|e| (e.group.as_str(), e.fender_id.as_str())),
        );
        let expected = expected_roster(&roster, &job.ops);
        let graph = read_working_copy(s, &pre, expected.as_ref(), list_index, &name);
        // Identity-preserving persist (Pro Control's rename(current name) → save(slot)):
        // keeps the preset's name and song link.
        if !name.is_empty() {
            s.rename_current_preset(&name)?;
        }
        s.save_current_preset(list_index)?;
        graph
    } else {
        None
    };
    // Keep the live-controller status warm before the next preset.
    s.heartbeat()?;
    s.pump_collect(120)?;
    Ok(CopyApplyItem {
        slot: list_index,
        name,
        outcome: "updated".to_string(),
        detail: format!("{total} op(s)"),
        graph,
    })
}

/// One target's `error` row: nothing was saved, and the frontend patches nothing.
fn error_item(list_index: u32, name: &str, detail: String) -> CopyApplyItem {
    CopyApplyItem {
        slot: list_index,
        name: name.to_string(),
        outcome: "error".to_string(),
        detail,
        graph: None,
    }
}

/// Per-group SORTED FenderId lists (a multiset per group) — the shape the working-copy
/// read is compared in. Node ids are deliberately NOT part of it (a device replace
/// re-assigns them, an insert mints one); groups are keyed because the device lists nodes
/// in sorted-group order while the frontend's optimistic graph lists them in signal
/// order; and WITHIN a group the order is dropped because the unit accepts several blocks
/// of one model in a group (ONLINE `copy.spec.ts` 2026-09-03: four `ACD_TubeScreamer` in
/// G1 after chained inserts) and, with a node id being its FenderId, an insert anchored on
/// a duplicated model lands where the device decides, not where `expected_roster`
/// projects. A multiset match still proves the post-edit document: a partial has fewer
/// nodes and a stale one a different multiset — and the adopted read then carries the
/// device's real order.
type Roster = std::collections::BTreeMap<String, Vec<String>>;

fn group_roster<'a>(nodes: impl Iterator<Item = (&'a str, &'a str)>) -> Roster {
    let mut out = Roster::new();
    for (group, fender_id) in nodes {
        out.entry(group.to_string())
            .or_default()
            .push(fender_id.to_string());
    }
    out.values_mut().for_each(|v| v.sort());
    out
}

/// `read` ⊆ `expected` as per-group multisets with fewer nodes in total — a partial cut
/// inside the nodes.
fn is_partial_of(read: &Roster, expected: &Roster) -> bool {
    let count = |r: &Roster| r.values().map(Vec::len).sum::<usize>();
    count(read) < count(expected)
        && read.iter().all(|(g, r)| {
            expected.get(g).is_some_and(|e| {
                let mut pool = e.clone();
                r.iter().all(|id| {
                    pool.iter()
                        .position(|x| x == id)
                        .map(|i| pool.swap_remove(i))
                        .is_some()
                })
            })
        })
}

/// The roster the acked `ops` leave behind, applied in order to the PRE-edit roster the
/// blockcaps guard read off the load-time document. Mirrors `diffToOps`' contract: a
/// replace keeps its position, a remove drops the block, an insert lands BEFORE the first
/// same-group block carrying the anchor FenderId (or appends to its group). `None` when an
/// op's target or anchor isn't in the roster — the model then can't say what the device
/// holds, and the read-back is refused rather than trusted.
fn expected_roster(pre: &[blockcaps::RosterEntry], ops: &[CopyOp]) -> Option<Roster> {
    struct Work {
        group: String,
        /// `None` once the device re-assigned it (a replace) or minted it (an insert).
        node_id: Option<String>,
        fender_id: String,
    }
    let mut work: Vec<Work> = pre
        .iter()
        .map(|e| Work {
            group: e.group.clone(),
            node_id: Some(e.node_id.clone()),
            fender_id: e.fender_id.clone(),
        })
        .collect();
    let find = |work: &[Work], group: &str, node_id: &str| {
        work.iter()
            .position(|w| w.group == group && w.node_id.as_deref() == Some(node_id))
    };
    for op in ops {
        match op {
            CopyOp::Replace {
                group,
                node_id,
                repl,
            } => {
                let i = find(&work, group, node_id)?;
                work[i].node_id = None;
                work[i].fender_id = repl.insert_fender_id().to_string();
            }
            CopyOp::Remove { group, node_id } => {
                let i = find(&work, group, node_id)?;
                work.remove(i);
            }
            CopyOp::Insert {
                group,
                before_fender_id,
                repl,
            } => {
                let at = match before_fender_id {
                    Some(anchor) => work
                        .iter()
                        .position(|w| &w.group == group && &w.fender_id == anchor)?,
                    None => work
                        .iter()
                        .rposition(|w| &w.group == group)
                        .map_or(work.len(), |i| i + 1),
                };
                work.insert(
                    at,
                    Work {
                        group: group.clone(),
                        node_id: None,
                        fender_id: repl.insert_fender_id().to_string(),
                    },
                );
            }
        }
    }
    Some(group_roster(
        work.iter()
            .map(|w| (w.group.as_str(), w.fender_id.as_str())),
    ))
}

/// The working-copy graph for the Copy view's cache patch — a REAL read: `clear_raw` +
/// `currentPresetDataRequest` (field 2) on the held session, the `Session::live_ftsw`
/// wire shape, taken after the last confirmed op and BEFORE the save. HW 2026-09-03
/// (fw 1.8.45, `probe --reprompt-map`, Copy's exact session shape): 6/6 re-prompts
/// answered in ~520 ms with the POST-edit roster (insert and remove), the stream complete
/// in its first slice, and both saves after a re-prompt persisted (field-8 read-back).
/// The previous "read-back" was a buffer scrape that once returned the LOAD-TIME graph
/// (HW 2026-09-02) and patched the cache back to the pre-edit blocks.
///
/// The roster oracle (`expected_roster`) is both the completion predicate — a mid-flight
/// partial keeps pumping until the roster matches, then the payload must hold still for
/// two slices before the parse is trusted (`Session::live_audio_graph`) — and the
/// adoption guard. Verdicts, one line per saved slot: `adopted`; `truncated` (a per-group
/// prefix of the oracle — checked first, since a tail insert's partial reads exactly as
/// the pre-edit roster); `read == pre-edit` (the working copy does NOT show the acked
/// edit — logged, never blocks the save: the ops were confirmed, and no HW sample of this
/// contradiction exists yet); other mismatch; `no reply`. `None` = the frontend patches
/// from the edit it staged; an edit never triggers a refetch either way.
fn read_working_copy(
    s: &mut Session,
    pre: &Roster,
    expected: Option<&Roster>,
    list_index: u32,
    name: &str,
) -> Option<session::ActiveGraph> {
    let Some(expected) = expected else {
        log::warn!(
            "[copy_apply] slot {list_index}: working-copy read has no oracle (an acked op \
             named a block the pre-edit roster lacks) — not read, the cache is patched from \
             the acked edit"
        );
        return None;
    };
    if expected == pre {
        log::info!(
            "[copy_apply] slot {list_index}: working-copy read is unverifiable (the acked \
             ops leave the block roster unchanged) — not read, the cache is patched from \
             the acked edit"
        );
        return None;
    }
    // ONE node walk for every compare: the pre-edit roster came through
    // `extract_active_graph` (model = FenderId, else nodeId), so the read must too.
    let roster_of = |v: &serde_json::Value| {
        let g = session::extract_active_graph(v, None);
        group_roster(
            g.nodes
                .iter()
                .map(|n| (n.group_id.as_str(), n.model.as_str())),
        )
    };
    let graph = s.live_audio_graph(|v| roster_of(v) == *expected);
    let carriers = s.json_payload_carriers();
    let mut graph = match graph {
        Ok(graph) => graph,
        Err(e) => {
            // Classify whatever DID land so the next online run can read the failure shape.
            let (warn, verdict) = match s.current_preset_value() {
                Ok(v) => {
                    let read = roster_of(&v);
                    // A partial cut inside the nodes — tested FIRST, because a tail
                    // insert's partial reads exactly as the pre-edit roster and must not be
                    // reported as a missing edit.
                    let is_prefix = is_partial_of(&read, expected);
                    let what = if is_prefix {
                        "is truncated (a partial of the acked edit)"
                    } else if read == *pre {
                        "== the PRE-edit roster — the device's working copy does NOT show the \
                         acked edit"
                    } else {
                        "does NOT match the acked edit"
                    };
                    (
                        !is_prefix,
                        format!(
                            "working-copy read {what} (carriers {carriers:?}; read {read:?}; \
                             expected {expected:?})"
                        ),
                    )
                }
                Err(_) => (false, format!("no working-copy reply ({e})")),
            };
            let line = format!(
                "[copy_apply] slot {list_index}: {verdict} — the cache is patched from the \
                 acked edit"
            );
            if warn {
                log::warn!("{line}");
            } else {
                log::info!("{line}");
            }
            return None;
        }
    };
    // After `clear_raw` the reply carries no `currentPresetInfoChanged`/`PresetLoaded`
    // stream, so the identity fields come from the job the read belongs to.
    if graph.name.is_none() && !name.is_empty() {
        graph.name = Some(name.to_string());
    }
    graph.slot.get_or_insert(list_index);
    log::info!(
        "[copy_apply] slot {list_index}: working-copy read shows the acked edit (carriers \
         {carriers:?}) — adopted"
    );
    Some(graph)
}

/// Apply ONE [`CopyOp`] on the held session, returning whether the device CONFIRMED it
/// (`nodeReplaced`(40) / `nodeRemoved`(36) / `nodeInserted`(33)). `retry_drop` re-tries
/// a single SILENT drop (the cold first edit after a fresh load) but never a
/// `presetError`. An IR/saved INSERT then applies its IR-file / saved-block follow-up to
/// the node it added, addressed by FenderId (= its id on the unit); `copy_apply_one`
/// refuses the op up front when that address would be ambiguous.
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
/// node it just added and apply the IR-file link / saved-block swap to it. On the unit a
/// node's id IS its FenderId (`notes/write-safety.md`), so the new node is addressed as
/// `repl.insert_fender_id()` directly — no before/after roster diff (the old diff read the
/// session buffer, which `insert_node`'s `clear_raw` had just emptied, so it always
/// degraded to a bare insert on hardware; and a partial "before" read could have diffed
/// out an EXISTING node and pointed the IR/block swap at it). `copy_apply_one` refuses the
/// op up front when the group already holds that model. The IR/saved follow-ups are
/// software-green (SimDevice) with their HW validation still pending — `diffToOps` emits
/// only Model inserts today.
fn apply_copy_insert(
    s: &mut Session,
    group: &str,
    before_fender_id: Option<&str>,
    repl: &CopyRepl,
    retry_drop: bool,
) -> Result<bool, String> {
    let insert_id = repl.insert_fender_id();

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

    // IR / Saved → the content follow-up on the node just added (its id = its FenderId).
    match repl {
        CopyRepl::Model { .. } => Ok(true),
        CopyRepl::Ir { file, .. } => {
            // Replace the just-inserted node WITH the IR (two-step: → ACD_UserIRTMS + the
            // string `file` param), full-fidelity.
            s.replace_node_with_ir(group, insert_id, file)
        }
        CopyRepl::Saved { fender_id, index } => {
            s.replace_node_with_block(group, insert_id, fender_id, *index)
        }
    }
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
