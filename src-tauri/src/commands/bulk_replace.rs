//! Live per-node block replace across a selection + its discovery: the shared types,
//! guards and held-session plans behind `probe`'s replace arms and the Copy tab.
use crate::{audiograph, blockcaps, session, DeviceSlot, ListIndex, Serialize, Session};

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

/// What happened to one preset in a live bulk-replace or Copy run. Serializes to the same
/// lowercase words the field always carried (`"updated" | "skipped" | "error"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemOutcome {
    Updated,
    Skipped,
    Error,
}

impl std::fmt::Display for ItemOutcome {
    /// The wire word, for the probe's text reports.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ItemOutcome::Updated => "updated",
            ItemOutcome::Skipped => "skipped",
            ItemOutcome::Error => "error",
        })
    }
}

/// One preset's outcome from a live bulk-replace run (streamed to the UI per preset).
#[derive(Debug, Clone, Serialize)]
pub struct BulkReplaceItem {
    /// 0-based list index of the preset (the UI keys its rows by this).
    pub slot: ListIndex,
    pub name: String,
    pub outcome: ItemOutcome,
    pub detail: String,
}
/// One preset's matching target nodes, discovered up front (field-8 read) so the
/// edit hot path never does a fragile post-reconnect read.
pub(crate) struct ReplacePlan {
    pub(crate) list_index: ListIndex,
    pub(crate) name: String,
    /// The discovered body's `info.preset_id` (`library::identity_preset_id_of`). The
    /// writers require it to match the loaded working copy before the first edit
    /// ([`confirm_plan_identity`]); `None` refuses the preset.
    pub(crate) preset_id: Option<String>,
    /// `(group, node_id)` of every node matching the requested `from_id`.
    pub(crate) targets: Vec<(String, String)>,
}

/// Read the selected slots ONCE on a single session and build the per-preset target
/// list. Field-8 reads are reliable on a clean session but flaky right after a
/// reconnect, so all reads are batched here, away from the edit reconnect churn.
pub(crate) fn discover_replace_plans(
    device_slots: &[DeviceSlot],
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
            list_index: dev_slot.list_index(),
            name,
            preset_id: crate::library::identity_preset_id_of(&value).map(str::to_string),
            targets,
        });
    }
    Ok(plans)
}

/// An `error` row for `list_index` (the preset was not edited).
pub(crate) fn error_item(list_index: ListIndex, name: String, detail: String) -> BulkReplaceItem {
    BulkReplaceItem {
        slot: list_index,
        name,
        outcome: ItemOutcome::Error,
        detail,
    }
}

/// Confirm the loaded working copy is still the body `plan` was discovered from. The
/// slot echo only proves the load took, and node ids repeat across presets. Every save
/// re-mints `preset_id` and imports keep it (fw 1.8.58), so a match means "this slot's
/// body is unchanged since discovery". Re-prompts once when nothing is buffered.
pub(crate) fn confirm_plan_identity(s: &mut Session, plan: &ReplacePlan) -> Result<(), String> {
    let id_of = |v: serde_json::Value| crate::library::preset_id_of(&v).map(str::to_string);
    let live_id = s.current_preset_value().ok().and_then(id_of).or_else(|| {
        s.live_preset_value(|v| crate::library::preset_id_of(v).is_some())
            .ok()
            .and_then(id_of)
    });
    plan_identity_verdict(plan, live_id.as_deref())
}

/// [`confirm_plan_identity`]'s decision, pure. Both ids must be present, not the
/// template's, and equal. No slot + name fallback: a body rejected at load becomes the
/// template under the slot's STORED name, so slot and name both confirm an empty body.
pub(crate) fn plan_identity_verdict(
    plan: &ReplacePlan,
    live_id: Option<&str>,
) -> Result<(), String> {
    let slot = plan.list_index;
    let name = &plan.name;
    let Some(want) = plan.preset_id.as_deref() else {
        return Err(format!(
            "list index {slot} ({name:?}): discovery read no usable info.preset_id \
             — refusing to edit an unidentified preset"
        ));
    };
    match live_id {
        Some(got) if got == want => Ok(()),
        Some(crate::library::TEMPLATE_PRESET_ID) => Err(format!(
            "list index {slot} loaded as the firmware's empty template, not {name:?} \
             (an empty slot or a body rejected at load) — not edited"
        )),
        Some(got) => Err(format!(
            "list index {slot} now holds preset_id {got:?}, not the discovered {want:?} \
             ({name:?}) — the slot's body changed since discovery; not edited"
        )),
        None => Err(format!(
            "could not read the loaded preset's info.preset_id at list index {slot} \
             (expected {want:?}, {name:?}) — refusing to edit an unidentified preset"
        )),
    }
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
            let _ = s.pump_silent(250);
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
