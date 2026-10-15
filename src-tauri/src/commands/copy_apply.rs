//! "Copy blocks between presets" — ordered replace/insert/remove op apply.
use crate::blockcaps::{group_roster, Roster};
use crate::{
    blockcaps, commands::bulk_replace::blockcaps_advance, commands::bulk_replace::blockcaps_check,
    commands::bulk_replace::blockcaps_pre_edit_roster, commands::bulk_replace::blockcaps_replaced,
    commands::bulk_replace::ItemOutcome, device_gate::with_released_seize, proto, session,
    AppState, ListIndex, Serialize, Session, State,
};

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
    /// Insert `repl` into `group` via field-34 `insert_node`, BEFORE the block `before`
    /// names (`None` appends at the group end). `key` names this insert for a later one
    /// that anchors on it.
    Insert {
        group: String,
        #[serde(default)]
        key: Option<String>,
        before: Option<Anchor>,
        repl: CopyRepl,
    },
    /// Remove the block `node_id` from `group` — `removeNode` (the device re-links).
    Remove {
        group: String,
        #[serde(rename = "nodeId")]
        node_id: String,
    },
}

/// The block an insert lands before: an original block by its pre-edit `nodeId`, or an
/// insert earlier in the same job by its `key`. The device matches the anchor on the
/// group's CURRENT node ids (fw 1.8.58 static RE) and a replace or insert mints a new
/// id, so [`DeviceIds`] resolves it from the confirms at send time.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Anchor {
    Node {
        #[serde(rename = "nodeId")]
        node_id: String,
    },
    Inserted {
        key: String,
    },
}

impl Anchor {
    fn node(node_id: &str) -> Anchor {
        Anchor::Node {
            node_id: node_id.to_string(),
        }
    }
}

/// The device's current node ids, tracked through a job's confirmed ops.
struct DeviceIds {
    /// `(group, id)` of every block in the working copy whose id is known.
    live: Vec<(String, String)>,
    /// Anchor → the block's current id.
    alias: std::collections::HashMap<Anchor, String>,
}

impl DeviceIds {
    fn new(roster: &[blockcaps::RosterEntry]) -> DeviceIds {
        DeviceIds {
            live: roster
                .iter()
                .map(|e| (e.group.clone(), e.node_id.clone()))
                .collect(),
            alias: roster
                .iter()
                .map(|e| (Anchor::node(&e.node_id), e.node_id.clone()))
                .collect(),
        }
    }

    /// The current id `anchor` names, if that block is known to be in `group` now.
    fn resolve(&self, group: &str, anchor: &Anchor) -> Option<&str> {
        let id = self.alias.get(anchor)?;
        self.live
            .iter()
            .any(|(g, x)| g == group && x == id)
            .then_some(id.as_str())
    }

    /// The ids of the blocks `group` is known to hold now.
    fn group_ids(&self, group: &str) -> Vec<String> {
        self.live
            .iter()
            .filter(|(g, _)| g == group)
            .map(|(_, id)| id.clone())
            .collect()
    }

    /// Record a confirmed op. `minted` is the id its confirm carried; without one the
    /// block stays unaddressable, so nothing can anchor on it.
    fn advance(&mut self, op: &CopyOp, minted: Option<String>) {
        match op {
            CopyOp::Remove { group, node_id } | CopyOp::Replace { group, node_id, .. } => {
                let key = Anchor::node(node_id);
                if let Some(old) = self.alias.remove(&key) {
                    self.live.retain(|(g, x)| !(g == group && *x == old));
                }
                if let (CopyOp::Replace { .. }, Some(new)) = (op, minted) {
                    self.live.push((group.clone(), new.clone()));
                    self.alias.insert(key, new);
                }
            }
            CopyOp::Insert { group, key, .. } => {
                if let Some(new) = minted {
                    self.live.push((group.clone(), new.clone()));
                    if let Some(key) = key {
                        self.alias
                            .insert(Anchor::Inserted { key: key.clone() }, new);
                    }
                }
            }
        }
    }
}

/// One target preset for a [`copy_apply`] run: its 0-based `list_index`, display
/// `name` (for the identity-preserving rename-before-save), and the ORDERED list of
/// structural `ops` to apply before saving it.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CopyJob {
    #[serde(rename = "listIndex")]
    pub list_index: ListIndex,
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
    pub slot: ListIndex,
    pub name: String,
    pub outcome: ItemOutcome,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<session::ActiveGraph>,
}

/// "Copy blocks between presets" — apply an ORDERED list of structural ops
/// (replace / insert / remove) to EACH target preset, live, then save that preset in
/// place (only when every op confirmed AND `save`). Mirrors [`replace_many_held`]'s
/// architecture: ONE held re-armed session per preset (`copy_apply_one`), the
/// DEVICE_OP_LOCK / monitor-pause bookend (`with_released_seize`) and streamed
/// `CopyApplyItem`s. There is no cancel (the Copy tab has no Stop button). DEVICE WRITE —
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
    with_released_seize(state.session.clone(), move || {
        // ONE held session for the whole run (the E1 architecture): connect once, warm
        // the live-controller heartbeat once (`begin_live_edit`), then `copy_apply_one`
        // each preset with no reopens. A per-preset failure stays an `error` row (the
        // session stays alive); only a failure to ESTABLISH the session propagates.
        let mut s = Session::connect()?;
        s.begin_live_edit()?;
        let mut out = Vec::with_capacity(jobs.len());
        for job in &jobs {
            let item = copy_apply_one(&mut s, job, save).unwrap_or_else(|e| CopyApplyItem {
                slot: job.list_index,
                name: job.name.clone(),
                outcome: ItemOutcome::Error,
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
/// preset), applies each op (an unconfirmed first op is READ BACK before any re-send — see
/// [`apply_copy_op`]), and saves ONLY when every op confirmed AND no `presetError`. An
/// empty op list → skipped.
fn copy_apply_one(s: &mut Session, job: &CopyJob, save: bool) -> Result<CopyApplyItem, String> {
    let list_index = job.list_index;
    let name = job.name.clone();
    if job.ops.is_empty() {
        return Ok(CopyApplyItem {
            slot: list_index,
            name,
            outcome: ItemOutcome::Skipped,
            detail: "no ops".to_string(),
            graph: None,
        });
    }

    // ── LOAD on the held session + RE-ARM the edit context to the just-loaded preset
    //    (mirrors `held_replace_one`). ──
    s.clear_raw();
    s.send_and_collect(&proto::load_my_preset(list_index.into()), 200)?;
    s.rearm_active_info()?;
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
    let mut ids = DeviceIds::new(&roster);

    // Apply each op in order. An unconfirmed FIRST edit with no presetError is read back
    // before anything is re-sent (`apply_copy_op`). The old "cold first edit is silently
    // dropped" reading is a fw 1.8.45 belief with no cited HW evidence: fw 1.8.58 has no
    // silent-drop path for a correctly framed edit (INFERRED). A missing confirm is a
    // confirm queued behind the FIFO, a lapse (which draws ConnectionError), a backup
    // freeze, or an insert abort. Recover by read-back, never by blind re-send.
    let total = job.ops.len();
    for (i, op) in job.ops.iter().enumerate() {
        let first = i == 0;

        let (candidate_id, is_replace, target) = cap_inputs(op);
        // The group's FenderIds before this op, for an insert's anchor check, its IR/saved
        // address check, and an insert's or replace's read-back. `None` when the ops so far
        // cannot be modelled: the group's contents are then unknown.
        let before = match op {
            CopyOp::Insert { group, .. } | CopyOp::Replace { group, .. } => {
                expected_roster(&roster, &job.ops[..i])
                    .map(|r| r.get(group).cloned().unwrap_or_default())
            }
            CopyOp::Remove { .. } => None,
        };
        let anchor_id = match insert_precheck(op, &ids, before.as_deref()) {
            Ok(anchor_id) => anchor_id,
            Err(why) => {
                return Ok(error_item(
                    list_index,
                    &name,
                    format!(
                        "op {}/{total} ({}) refused: {why}",
                        i + 1,
                        describe_copy_op(op)
                    ),
                ));
            }
        };
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

        let applied = AppliedOp {
            anchor_id: anchor_id.as_deref(),
            before: before.as_deref(),
            old_fender: replaced.map(|(fender, _)| fender),
        };
        match apply_copy_op(s, op, first, &applied) {
            Ok(true) => {
                blockcaps_advance(&mut counts, candidate_id, replaced);
                // Every new block's id — the confirm's, else a read — so later ops in the
                // group can address it and tell it apart from the next new block.
                let minted = match op {
                    CopyOp::Remove { .. } => None,
                    CopyOp::Replace { group, .. } | CopyOp::Insert { group, .. } => {
                        s.inserted_node_id(group, &ids.group_ids(group))
                    }
                };
                ids.advance(op, minted);
            }
            Ok(false) => {
                return Ok(error_item(
                    list_index,
                    &name,
                    format!(
                        "device rejected op {}/{total} ({}) — {} — NOT saved",
                        i + 1,
                        describe_copy_op(op),
                        s.rejection()
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

    let graph = if save {
        save_copy_target(s, &roster, job)?
    } else {
        None
    };
    // Keep the live-controller status warm before the next preset.
    s.heartbeat()?;
    s.pump_silent(120)?;
    Ok(CopyApplyItem {
        slot: list_index,
        name,
        outcome: ItemOutcome::Updated,
        detail: format!("{total} op(s)"),
        graph,
    })
}

/// One op's block-cap inputs: `(candidate, is_replace, target)`. Remove has no candidate
/// (only shrinks — never a cap check); Replace subtracts its target's contribution, Insert
/// doesn't (mirrors the TS `checkOp` mode-aware formula).
fn cap_inputs(op: &CopyOp) -> (Option<&str>, bool, Option<(&str, &str)>) {
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
    }
}

/// Persist a target whose every op confirmed: the working-copy read for the Copy view's
/// in-place cache patch (no ~22 s re-scan), then rename-to-itself + save.
///
/// The read is a field-2 re-prompt of the device's LIVE document, taken after the last
/// confirmed op and BEFORE the save (the footswitch flow's HW-proven re-prompt-then-save
/// ordering), never a scrape of whatever the session still had buffered. Only on save (an
/// unsaved edit must not poison the cache), and only when the read can be verified against
/// the acked ops (`read_working_copy`).
fn save_copy_target(
    s: &mut Session,
    roster: &[blockcaps::RosterEntry],
    job: &CopyJob,
) -> Result<Option<session::ActiveGraph>, String> {
    // What the acked ops leave behind — the oracle the read is checked against. An
    // edit that leaves the roster as it was (a same-model re-stamp: the roster compares
    // FenderIds, so nothing it can see changes) is unverifiable —
    // pre-edit and post-edit documents look identical — so no read is spent on it.
    let pre = group_roster(
        roster
            .iter()
            .map(|e| (e.group.as_str(), e.fender_id.as_str())),
    );
    let expected = expected_roster(roster, &job.ops);
    let graph = read_working_copy(s, &pre, expected.as_ref(), job.list_index, &job.name);
    // Identity-preserving persist (Pro Control's rename(current name) → save(slot)):
    // keeps the preset's name and song link.
    if !job.name.is_empty() {
        s.rename_current_preset(&job.name)?;
    }
    s.save_current_preset(job.list_index)?;
    Ok(graph)
}

/// An insert's pre-send checks, given the group's FenderIds before it (`before`): its
/// resolved anchor id (`None` = append, or not an insert), or the refusal's reason.
fn insert_precheck(
    op: &CopyOp,
    ids: &DeviceIds,
    before: Option<&[String]>,
) -> Result<Option<String>, String> {
    let CopyOp::Insert {
        group,
        repl,
        before: anchor,
        ..
    } = op
    else {
        return Ok(None);
    };
    // fw 1.8.58 answers an insert whose anchor is not in the group with no reply and
    // aborts its server (static RE), so the anchor must resolve to a block known to be
    // there now.
    let anchor_id = match anchor {
        Some(anchor) => match ids.resolve(group, anchor) {
            Some(id) => Some(id.to_string()),
            None => {
                return Err(format!(
                    "its anchor is not a block {group} is known to hold now — NOT sent"
                ));
            }
        },
        None => None,
    };
    // An IR/saved INSERT addresses the node it adds by FenderId (a node id is its
    // FenderId only for the first instance of a model in a group: fw 1.8.58 mints
    // `X`, `X_1`, `X_2`… — device-confirmed for the first, static RE for the rest;
    // `notes/write-safety.md`), so a group that ALREADY holds that model would make the
    // follow-up swap ambiguous (it could re-point an existing block). (A cap-legal
    // case: two cabinets in one group.) Refuse, never guess.
    let ambiguous = !matches!(repl, CopyRepl::Model { .. })
        && before.is_none_or(|ids| ids.iter().any(|id| id == repl.insert_fender_id()));
    if ambiguous {
        return Err(format!(
            "{group} already holds a {} block (or the ops so far cannot be modelled), so the \
             inserted node's id would be ambiguous — NOT saved",
            repl.insert_fender_id()
        ));
    }
    Ok(anchor_id)
}

/// One target's `error` row: nothing was saved, and the frontend patches nothing.
fn error_item(list_index: ListIndex, name: &str, detail: String) -> CopyApplyItem {
    CopyApplyItem {
        slot: list_index,
        name: name.to_string(),
        outcome: ItemOutcome::Error,
        detail,
        graph: None,
    }
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
/// replace keeps its position, a remove drops the block, an insert lands BEFORE its
/// anchor (or appends to its group). `None` when an
/// op's target or anchor isn't in the roster — the model then can't say what the device
/// holds, and the read-back is refused rather than trusted.
fn expected_roster(pre: &[blockcaps::RosterEntry], ops: &[CopyOp]) -> Option<Roster> {
    struct Work {
        group: String,
        /// The job's name for the block (its pre-edit node id, or an insert's key).
        anchor: Option<Anchor>,
        fender_id: String,
    }
    let mut work: Vec<Work> = pre
        .iter()
        .map(|e| Work {
            group: e.group.clone(),
            anchor: Some(Anchor::node(&e.node_id)),
            fender_id: e.fender_id.clone(),
        })
        .collect();
    let find = |work: &[Work], group: &str, anchor: &Anchor| {
        work.iter()
            .position(|w| w.group == group && w.anchor.as_ref() == Some(anchor))
    };
    for op in ops {
        match op {
            CopyOp::Replace {
                group,
                node_id,
                repl,
            } => {
                let i = find(&work, group, &Anchor::node(node_id))?;
                work[i].fender_id = repl.insert_fender_id().to_string();
            }
            CopyOp::Remove { group, node_id } => {
                let i = find(&work, group, &Anchor::node(node_id))?;
                work.remove(i);
            }
            CopyOp::Insert {
                group,
                key,
                before,
                repl,
            } => {
                let at = match before {
                    Some(anchor) => find(&work, group, anchor)?,
                    None => work
                        .iter()
                        .rposition(|w| &w.group == group)
                        .map_or(work.len(), |i| i + 1),
                };
                work.insert(
                    at,
                    Work {
                        group: group.clone(),
                        anchor: key.clone().map(|key| Anchor::Inserted { key }),
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
    list_index: ListIndex,
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

/// What [`apply_copy_op`] knows about the op's target before it is sent.
struct AppliedOp<'a> {
    /// An insert's anchor: the device node id to insert AHEAD of (`None` appends).
    anchor_id: Option<&'a str>,
    /// The op's group FenderIds before it (insert and replace), `None` when unknown.
    before: Option<&'a [String]>,
    /// A replace's target FenderId in the pre-edit roster, `None` when unknown.
    old_fender: Option<&'a str>,
}

/// Apply ONE [`CopyOp`] on the held session, returning whether the device CONFIRMED it
/// (`nodeReplaced`(40) / `nodeRemoved`(36) / `nodeInserted`(33)).
///
/// `recover_unconfirmed`: an edit with no confirm and no `presetError` is READ BACK from
/// the working copy before anything is re-sent, and a landed edit is never re-sent (a
/// missing confirm on fw 1.8.58 is a landed edit whose confirm was lost, INFERRED). A
/// replace mints a new node id, so a blind re-send would name a node that is already gone,
/// which may abort the device server (static RE that `replaceNode` on an unknown id throws;
/// whether it is caught is OPEN). See [`replace_read_back`]; a remove re-sends only when the
/// read shows the node still there; an insert reads against `before` (its group's FenderIds
/// before the op), see [`Session::insert_landed`]. Never on a `presetError`.
///
/// An IR/saved INSERT then applies its IR-file / saved-block follow-up to the node it
/// added, addressed by FenderId (its id when it is the group's first instance of that
/// model); `copy_apply_one` refuses the op up front when that address would be ambiguous.
fn apply_copy_op(
    s: &mut Session,
    op: &CopyOp,
    recover_unconfirmed: bool,
    target: &AppliedOp,
) -> Result<bool, String> {
    match op {
        CopyOp::Replace {
            group,
            node_id,
            repl,
        } => {
            if apply_copy_replace(s, group, node_id, repl)? {
                return Ok(true);
            }
            if !recover_unconfirmed || s.saw_preset_error() {
                return Ok(false);
            }
            match replace_read_back(s, group, node_id, target, repl.insert_fender_id()) {
                // An IR replace is two steps: a landed first step whose confirm was lost
                // never ran its `file` follow-up, so it stays unconfirmed (no save).
                Some(true) => Ok(!matches!(repl, CopyRepl::Ir { .. })),
                Some(false) => apply_copy_replace(s, group, node_id, repl),
                None => Ok(false),
            }
        }
        CopyOp::Remove { group, node_id } => {
            if s.remove_node(group, node_id)? {
                return Ok(true);
            }
            if !recover_unconfirmed || s.saw_preset_error() {
                return Ok(false);
            }
            match node_present(s, group, node_id) {
                Some(false) => Ok(true),
                Some(true) => s.remove_node(group, node_id),
                None => Ok(false),
            }
        }
        CopyOp::Insert { group, repl, .. } => {
            let retry = if recover_unconfirmed {
                target.before
            } else {
                None
            };
            apply_copy_insert(s, group, target.anchor_id, repl, retry)
        }
    }
}

/// Read the working copy after an unconfirmed replace of `group`/`node_id` by `new` (its
/// target FenderId): `Some(true)` = landed (`node_id` is gone and the group holds one more
/// `new` than before), `Some(false)` = not landed (`node_id` is still there with its old
/// FenderId), `None` = anything else, or no read, never guessed. A same-model re-stamp
/// (old FenderId equals `new`) is unverifiable — `None` without a read — because a landed
/// and a missing re-stamp leave the same roster.
fn replace_read_back(
    s: &mut Session,
    group: &str,
    node_id: &str,
    target: &AppliedOp,
    new: &str,
) -> Option<bool> {
    let old = target.old_fender?;
    if old == new {
        log::warn!(
            "[copy_apply] unconfirmed same-model re-stamp of {group}/{node_id} is \
             unverifiable — not re-sent, not saved"
        );
        return None;
    }
    let was = target.before?.iter().filter(|f| *f == new).count();
    let doc = s
        .live_complete_value()
        .map_err(|e| log::warn!("[copy_apply] replace read-back failed: {e}"))
        .ok()?;
    let nodes: Vec<_> = blockcaps::roster_from_preset(&doc)
        .into_iter()
        .filter(|e| e.group == group)
        .collect();
    let now = nodes.iter().filter(|e| e.fender_id == new).count();
    let verdict = match nodes.iter().find(|e| e.node_id == node_id) {
        None if now == was + 1 => Some(true),
        Some(e) if e.fender_id == old => Some(false),
        _ => None,
    };
    log::info!(
        "[copy_apply] unconfirmed replace {group}/{node_id} → {new}: read-back {}",
        match verdict {
            Some(true) => "shows it landed — not re-sent",
            Some(false) => "shows it did not land — re-sending once",
            None => "is inconclusive — not re-sent, not saved",
        }
    );
    verdict
}

/// Whether the working copy's `group` still holds `node_id` (`None` = no complete read).
fn node_present(s: &mut Session, group: &str, node_id: &str) -> Option<bool> {
    let doc = s
        .live_complete_value()
        .map_err(|e| log::warn!("[copy_apply] remove read-back failed: {e}"))
        .ok()?;
    Some(
        blockcaps::roster_from_preset(&doc)
            .iter()
            .any(|e| e.group == group && e.node_id == node_id),
    )
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
/// node it just added and apply the IR-file link / saved-block swap to it. A node's id is
/// its FenderId only for the first instance of a model in a group (fw 1.8.58 mints `X`,
/// `X_1`, `X_2`… — device-confirmed for the first, static RE for the rest;
/// `notes/write-safety.md`), so the new node is addressed as
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
    anchor_id: Option<&str>,
    repl: &CopyRepl,
    retry: Option<&[String]>,
) -> Result<bool, String> {
    let insert_id = repl.insert_fender_id();

    // field-34 insert: `anchor_id` is the node id to insert AHEAD of; `None` appends at
    // the group end.
    let was = retry.map(|before| before.iter().filter(|id| *id == insert_id).count());
    if !s.insert_node_once(group, anchor_id, insert_id, was)? {
        return Ok(false);
    }

    // IR / Saved → the content follow-up on the node just added (its id = its FenderId,
    // as the group's first instance of that model — see the doc above).
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
            before,
            repl,
            ..
        } => format!(
            "insert {} into {group}{}",
            repl.insert_fender_id(),
            match before {
                Some(Anchor::Node { node_id }) => format!(" before {node_id}"),
                Some(Anchor::Inserted { key }) => format!(" before inserted {key}"),
                None => " (append)".to_string(),
            }
        ),
        CopyOp::Remove { group, node_id } => format!("remove {group}/{node_id}"),
    }
}

#[cfg(test)]
#[path = "copy_e2e_tests.rs"]
mod copy_e2e_tests;
