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
    /// library `index`. `dual_cab` is the library entry's `dualCabsEnabled`, which the
    /// block-count cap weighs it by. The listing reports it true for every `ACD_CabSimTMS`
    /// entry, second cab on or off (static RE, fw 1.8.58), so a saved cabinet block weighs
    /// two slots: the cautious side of the cap.
    Saved {
        #[serde(rename = "fenderId")]
        fender_id: String,
        index: u64,
        #[serde(rename = "dualCab")]
        dual_cab: bool,
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
// Shared by every LIVE structural-edit writer (`held_replace_one`, `replace_many_held`,
// `copy_apply_one`): the device enforces none of the 4 block-count caps, and its CPU-budget
// check answers a replace with `presetError` 14 and aborts the server on an insert (static
// RE, fw 1.8.58; see `blockcaps.rs`'s module docs), so this Rust guard is the SOLE
// enforcement before a save. Every
// writer: (1) reads the PRE-edit roster right after load+attach-confirm (before its
// first structural edit), fail-closed if it can't be read; (2) checks each op's delta
// against the running counts BEFORE emitting it; (3) on a violation, aborts the WHOLE
// target with an `error` outcome carrying the reason — never a partial, unvalidated
// save.

/// Read the just-loaded preset's pre-edit node roster (+ its cap counts) off the held
/// session, retrying (mirrors `probe_api::insert`'s `ordered_group` — the post-load field-3 push
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
/// `candidate = None` is a bare Remove — no candidate to check; removing a node can
/// only shrink counts, never push a cap over its max. `candidate` and `replaced` are
/// `(fender_id, dual_cab)` pairs: the candidate's from [`repl_arg_candidate`] and
/// [`replace_candidate`] (a fresh Model/Ir block is single except a stock `ACD_CabSimTMS`
/// over a carried-over cab; a `Saved` block carries its library entry's flag), the
/// replaced node's from [`blockcaps_replaced`] (`None` for an Insert/Remove).
pub(crate) fn blockcaps_check(
    counts: &blockcaps::Counts,
    candidate: Option<(&str, bool)>,
    is_replace: bool,
    replaced: Option<(&str, bool)>,
) -> Result<(), String> {
    let Some((candidate_id, cand_dual)) = candidate else {
        return Ok(());
    };
    let (replaced_id, replaced_dual) = replaced.map_or((None, false), |(id, d)| (Some(id), d));
    blockcaps::check_op(
        counts,
        candidate_id,
        replaced_id,
        is_replace,
        cand_dual,
        replaced_dual,
    )
    .map_err(|reason| reason.to_string())
}

/// Roll the running `counts` forward after a CONFIRMED op, so the NEXT op in the same
/// job/plan is checked against the up-to-date state (a job can carry multiple ops that
/// each affect the same caps — e.g. two conv inserts in one job must be checked
/// cumulatively, not each against the stale pre-edit snapshot). `landed_dual` is the new
/// node's own second-cab flag from the confirm (`Session::confirmed_node_dual`), which
/// replaces the candidate's predicted one when present.
pub(crate) fn blockcaps_advance(
    counts: &mut blockcaps::Counts,
    candidate: Option<(&str, bool)>,
    replaced: Option<(&str, bool)>,
    landed_dual: Option<bool>,
) {
    if let Some((id, dual)) = candidate {
        counts.add(id, landed_dual.unwrap_or(dual));
    }
    if let Some((id, dual)) = replaced {
        counts.remove(id, dual);
    }
}

/// A REPLACE candidate weighed against the node it replaces: a stock `ACD_CabSimTMS` over a
/// cabinet model the firmware carries over lands dual (`blockcaps::stock_cabsim_replace_is_dual`;
/// the on-load default is not read, so the factory single cab stands for it otherwise).
pub(crate) fn replace_candidate<'a>(
    candidate: Option<(&'a str, bool)>,
    stock: bool,
    old_node_id: &str,
) -> Option<(&'a str, bool)> {
    candidate.map(|(id, dual)| {
        let carries_over = stock
            && id == "ACD_CabSimTMS"
            && blockcaps::stock_cabsim_replace_is_dual(old_node_id, None);
        (id, dual || carries_over)
    })
}

/// The `(fender_id, dual_cab)` a [`ReplArg`] would insert, or `None` for `Remove` (no
/// candidate to cap-check — see [`blockcaps_check`]).
pub(crate) fn repl_arg_candidate(repl: &ReplArg) -> Option<(&str, bool)> {
    match repl {
        ReplArg::Model { fender_id } | ReplArg::Ir { fender_id, .. } => {
            Some((fender_id.as_str(), false))
        }
        ReplArg::Saved {
            fender_id,
            dual_cab,
            ..
        } => Some((fender_id.as_str(), *dual_cab)),
        ReplArg::Remove => None,
    }
}

#[cfg(test)]
mod advance_tests {
    use super::*;

    /// A landed node's own second-cab flag, from the confirm, decides its weight: a stock
    /// `ACD_CabSimTMS` that came out dual takes two cabinet slots [HW].
    #[test]
    fn a_landed_dual_cab_weighs_two_whatever_was_predicted() {
        let mut counts = blockcaps::Counts::default();
        blockcaps_advance(
            &mut counts,
            Some(("ACD_CabSimTMS", false)),
            None,
            Some(true),
        );
        assert_eq!(counts.cabinet, 2);
        let mut counts = blockcaps::Counts::default();
        blockcaps_advance(
            &mut counts,
            Some(("ACD_CabSimTMS", true)),
            None,
            Some(false),
        );
        assert_eq!(counts.cabinet, 1);
        let mut counts = blockcaps::Counts::default();
        blockcaps_advance(&mut counts, Some(("ACD_CabSimTMS", true)), None, None);
        assert_eq!(counts.cabinet, 2, "no confirm JSON keeps the prediction");
    }
}
