//! Firmware-faithful pre-flight guard for the Tone Master Pro's 5 block restrictions — the
//! CPU budget and 4 count caps (`NodeSelectionRestrictions`, decompiled from
//! `tone-master-stomp-client`).
//!
//! **This is the SOLE enforcement of the 4 count caps.** The device audio engine
//! (`tm-stomp-server`) does NOT enforce them and cannot reject an over-cap edit with a
//! `presetError` — the count-cap code path is entirely client-side (UI-only) in
//! `tone-master-stomp-client` (static RE, fw 1.8.58). The CPU budget is different: the
//! server does check it (static RE, fw 1.8.58), but an over-budget INSERT gets no reply
//! and its exception aborts the server process (a replace gets `presetError` 14). So the
//! Rust apply path (`commands::copy_apply::copy_apply_one`,
//! `commands::held_edit::held_replace_one` / `replace_many_held`) is the primary,
//! fail-closed guard for all 5 rules: it must catch every over-cap edit BEFORE it reaches
//! the device.
//!
//! The 5 rules, in firmware enum order (the order `check_op` reports the first
//! violation in):
//! 0. `ProcessorUtilization` — Σ per-block `cpuByBid`, rounded half away from zero,
//!    ≤ `budget` (76.5), so a total of exactly 76.5 fails. An id missing from the
//!    table costs `unknownCost` (20), as on the device.
//! 1. `FXLoopCoexistence` — an FX-loop candidate (`fxLoops`) is refused when the preset
//!    already holds the same loop, or when it is `ACD_FxLoop3_4` (stereo) and either mono
//!    `ACD_FxLoop3` / `ACD_FxLoop4` is present, or the reverse. A non-loop candidate always
//!    passes. Not a count.
//! 2. `ConvolutionReverbLimit` — max 1 member of `convolutionSet`: `(cand∈set) −
//!    (replaced∈set) + (any member present ? 1 : 0) < 2`, so a non-member candidate and a
//!    member→member replace always pass.
//! 3. `ComboHalfStackCabinetsLimit` — max 2 member-weight of `cabinetSet`: each member
//!    node weighs 1, plus 1 when its `dspUnitParameters.cabsim2enabled` is true (an amp's
//!    second mic or a CabSim's second cab). A `cabinetExempt` candidate
//!    (`ACD_ExternalCab`) always passes.
//! 4. `GlooperEffectsLimit` — max 2 of `ACD_Glooper`.
//!
//! Membership is **exact-string** against the sets in `block-classification.json` — the
//! device's ids arrive verbatim with their suffix (e.g.
//! `ACD_DeluxeReverb68CustomCabIRConvRvb`), and the sets already enumerate every
//! runtime variant. NEVER normalize / strip a suffix before a set lookup: that would
//! silently misclassify a real device id (`block-classification.json` carries the
//! load-bearing `ConRvb` typo id and the empty-`acdCategory` standalone cab/IR that a
//! derived filter would miss).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde_json::Value;

/// The 5 firmware reason names, exactly as `NodeSelectionRestrictions` reports them —
/// the string the frontend shows as the block-cap violation copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum BlockCapError {
    ProcessorUtilization,
    FXLoopCoexistence,
    ConvolutionReverbLimit,
    ComboHalfStackCabinetsLimit,
    GlooperEffectsLimit,
}

impl std::fmt::Display for BlockCapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            BlockCapError::ProcessorUtilization => "ProcessorUtilization",
            BlockCapError::FXLoopCoexistence => "FXLoopCoexistence",
            BlockCapError::ConvolutionReverbLimit => "ConvolutionReverbLimit",
            BlockCapError::ComboHalfStackCabinetsLimit => "ComboHalfStackCabinetsLimit",
            BlockCapError::GlooperEffectsLimit => "GlooperEffectsLimit",
        };
        f.write_str(s)
    }
}

/// The membership sets (convolution, cabinet, cabinet exemptions, FX loops) and the
/// Glooper and stereo/mono FX-loop ids, parsed once from
/// `block-classification.json` (checked-in, RE-extracted — see the module docs).
struct Sets {
    convolution: HashSet<String>,
    cabinet: HashSet<String>,
    cabinet_exempt: HashSet<String>,
    /// `cabsimCarryOver`: the node ids a stock replace into `ACD_CabSimTMS` carries over
    /// ([`stock_cabsim_replace_is_dual`]).
    cabsim_carry_over: HashSet<String>,
    glooper: String,
    fx_loops: HashSet<String>,
    fx_loop_stereo: String,
    fx_loop_mono: HashSet<String>,
}

fn sets() -> &'static Sets {
    static SETS: OnceLock<Sets> = OnceLock::new();
    SETS.get_or_init(|| {
        let v: Value =
            serde_json::from_str(include_str!("../../src/models/block-classification.json"))
                .expect("block-classification.json must parse");
        let string_set = |key: &str| -> HashSet<String> {
            v.get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        };
        Sets {
            convolution: string_set("convolutionSet"),
            cabinet: string_set("cabinetSet"),
            cabinet_exempt: string_set("cabinetExempt"),
            cabsim_carry_over: string_set("cabsimCarryOver"),
            fx_loops: string_set("fxLoops"),
            glooper: v
                .get("glooper")
                .and_then(Value::as_str)
                .unwrap_or("ACD_Glooper")
                .to_string(),
            fx_loop_stereo: v
                .get("fxLoopStereo")
                .and_then(Value::as_str)
                .unwrap_or("ACD_FxLoop3_4")
                .to_string(),
            fx_loop_mono: string_set("fxLoopMono"),
        }
    })
}

/// The firmware's budget test: `frinta(total) > budget` fails. The epsilon keeps a
/// sum that drifted just under a `.5` boundary on the failing side.
fn over_budget(total: f64) -> bool {
    (total + 1e-6).round() > costs().budget
}

fn cost(id: &str) -> f64 {
    let c = costs();
    c.cpu_by_bid.get(id).copied().unwrap_or(c.unknown_cost)
}

/// The CPU budget + per-block cost table, parsed once from `model-cpu.json`.
struct Costs {
    budget: f64,
    /// What the firmware charges a FenderId missing from its table (static RE).
    unknown_cost: f64,
    cpu_by_bid: HashMap<String, f64>,
}

fn costs() -> &'static Costs {
    static COSTS: OnceLock<Costs> = OnceLock::new();
    COSTS.get_or_init(|| {
        let v: Value = serde_json::from_str(include_str!("../../src/models/model-cpu.json"))
            .expect("model-cpu.json must parse");
        let budget = v.get("budget").and_then(Value::as_f64).unwrap_or(76.5);
        let unknown_cost = v.get("unknownCost").and_then(Value::as_f64).unwrap_or(20.0);
        let cpu_by_bid = v
            .get("cpuByBid")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(k, val)| val.as_f64().map(|f| (k.clone(), f)))
            .collect();
        Costs {
            budget,
            unknown_cost,
            cpu_by_bid,
        }
    })
}

/// One graph node's identity + the properties the guard counts: its exact FenderId
/// (used byte-verbatim — see module docs) and whether it takes a second cabinet slot
/// (`cabsim2enabled` on any node: an amp's second mic or a CabSim's second cab), which
/// weighs a cabinet member 2 for `ComboHalfStackCabinetsLimit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterEntry {
    pub group: String,
    pub node_id: String,
    pub fender_id: String,
    pub dual_cab: bool,
}

/// Per-group SORTED FenderId lists (a multiset per group) — the shape the working-copy
/// read is compared in. Node ids are deliberately NOT part of it (a device replace
/// re-assigns them, an insert mints one); groups are keyed because the device lists nodes
/// in sorted-group order while the frontend's optimistic graph lists them in signal
/// order; and WITHIN a group the order is dropped because the unit accepts several blocks
/// of one model in a group (HW), and an insert anchored on a duplicated model lands where
/// the device decides, not where `expected_roster` projects. A multiset match still proves
/// the post-edit document: a partial has fewer
/// nodes and a stale one a different multiset — and the adopted read then carries the
/// device's real order.
pub(crate) type Roster = std::collections::BTreeMap<String, Vec<String>>;

pub(crate) fn group_roster<'a>(nodes: impl Iterator<Item = (&'a str, &'a str)>) -> Roster {
    let mut out = Roster::new();
    for (group, fender_id) in nodes {
        out.entry(group.to_string())
            .or_default()
            .push(fender_id.to_string());
    }
    out.values_mut().for_each(|v| v.sort());
    out
}

/// [`group_roster`] of a decoded preset's whole graph.
pub(crate) fn group_roster_of(nodes: &[crate::session::GraphNode]) -> Roster {
    group_roster(
        nodes
            .iter()
            .map(|n| (n.group_id.as_str(), n.model.as_str())),
    )
}

/// The loaded working copy must hold the imported file's blocks, group by group. An empty
/// roster where the file has blocks is the firmware's silent load-time substitute; any
/// other difference (a `DUBS_Unknown` rename, a read cut short) also refuses — this gates
/// an irreversible overwrite, so it fails closed.
pub(crate) fn check_loaded_roster(file: &Roster, live: &Roster) -> Result<(), String> {
    if file == live {
        return Ok(());
    }
    if live.is_empty() {
        return Err(
            "the loaded import has NO blocks where the file has some — the device replaced \
             it with an empty body at load"
                .to_string(),
        );
    }
    Err(format!(
        "the loaded import's blocks {live:?} differ from the file's {file:?}"
    ))
}

/// Walk decoded preset JSON into the pre-edit roster the guard counts against —
/// sourced from `session::extract_active_graph`, the ONE place that owns the node walk
/// AND the cabinet-cap weight (`GraphNode::second_cab_slot`: `cabsim2enabled` on any
/// node, an amp's second mic as much as a CabSim's second cab).
pub fn roster_from_preset(preset: &Value) -> Vec<RosterEntry> {
    crate::session::extract_active_graph(preset, None)
        .nodes
        .into_iter()
        .map(|n| RosterEntry {
            group: n.group_id,
            node_id: n.node_id,
            fender_id: n.model,
            dual_cab: n.second_cab_slot,
        })
        .collect()
}

/// The running per-cap totals a roster contributes. A cabinet member with a second cab
/// slot weighs 2 (`ComboHalfStackCabinetsLimit`); every other membership contributes 1.
/// Signed so [`Counts::remove`] (a replace/remove delta) can never underflow-panic.
#[derive(Debug, Clone, Default)]
pub struct Counts {
    pub conv: i64,
    pub cabinet: i64,
    pub glooper: i64,
    pub cpu: f64,
    /// How many of each FX-loop id (`fxLoops`) the graph holds.
    pub fx_loops: std::collections::BTreeMap<String, i64>,
}

impl Counts {
    /// Add one node's contribution (an insert, or the candidate side of a replace).
    pub fn add(&mut self, id: &str, dual_cab: bool) {
        self.step(id, dual_cab, 1);
    }

    /// Subtract one node's contribution (a remove, or the replaced-anchor side of a
    /// replace — mode matters: an insert/append never subtracts).
    pub fn remove(&mut self, id: &str, dual_cab: bool) {
        self.step(id, dual_cab, -1);
    }

    fn step(&mut self, id: &str, dual_cab: bool, sign: i64) {
        let s = sets();
        if s.convolution.contains(id) {
            self.conv += sign;
        }
        if s.cabinet.contains(id) {
            self.cabinet += sign * if dual_cab { 2 } else { 1 };
        }
        if id == s.glooper {
            self.glooper += sign;
        }
        if s.fx_loops.contains(id) {
            *self.fx_loops.entry(id.to_string()).or_default() += sign;
        }
        self.cpu += sign as f64 * cost(id);
    }

    fn holds_loop(&self, id: &str) -> bool {
        self.fx_loops.get(id).is_some_and(|&n| n > 0)
    }
}

/// The guard for one `insertNode` against the pre-edit `roster`, advancing `counts` on
/// success. fw 1.8.58 answers an anchor its group lacks, or an insert over the CPU
/// budget, with no reply and aborts its server (static RE), so neither may be sent.
pub fn check_insert(
    roster: &[RosterEntry],
    counts: &mut Counts,
    group: &str,
    before: Option<&str>,
    fender_id: &str,
) -> Result<(), String> {
    if let Some(b) = before.filter(|b| !roster.iter().any(|e| e.group == group && e.node_id == *b))
    {
        return Err(format!(
            "{group} holds no node {b} to insert {fender_id} before — not sent"
        ));
    }
    check_op(counts, fender_id, None, false, false, false)
        .map_err(|r| format!("adding {fender_id} would break the device's {r} limit — not sent"))?;
    counts.add(fender_id, false);
    Ok(())
}

/// The pre-edit counts a roster (every node BEFORE any op in the job/plan is applied)
/// contributes to each cap.
pub fn counts(roster: &[RosterEntry]) -> Counts {
    let mut c = Counts::default();
    for e in roster {
        c.add(&e.fender_id, e.dual_cab);
    }
    c
}

/// Whether a stock `replaceNode` into `ACD_CabSimTMS` of the node `old_node_id` lands with
/// its second cab on. When the old node's id is `ACD_` + a cab on the firmware's list, the new
/// node takes that cab as both cabs, with the old mic, position, distance, axis and filters,
/// and `cabsim2enabled` on [HW + static RE, fw 1.8.58]. Any other old node (an amp with a
/// built-in cab, an IR, a `_1` or non-minted id) gives the block's on-load default:
/// `on_load_dual`, the factory single cab when unknown.
pub fn stock_cabsim_replace_is_dual(old_node_id: &str, on_load_dual: Option<bool>) -> bool {
    sets().cabsim_carry_over.contains(old_node_id) || on_load_dual.unwrap_or(false)
}

/// The authoritative per-op guard: would inserting/replacing `candidate_id` break any
/// of the 5 caps, given the CURRENT running `counts`? Each rule is the fw 1.8.58
/// client's own predicate (module docs), so a candidate a count cap does not cover
/// passes it, except that an already over-cap cabinet count refuses every candidate but
/// an exempt one, as on the unit.
///
/// `is_replace` selects whether `replaced_id`'s contribution is subtracted (an
/// insert/append never frees a slot); `cand_dual_cab`/`replaced_dual_cab` are whether
/// the candidate/replaced node takes a second cabinet slot (2 on the cabinet cap, for a
/// cabinet member only, matching what [`Counts`] holds).
///
/// Checked in firmware enum order (0..4); returns the FIRST violated reason. The TS
/// `checkOp` sibling applies the same rules, and weighs a stock candidate the same way: one
/// cabinet slot, or two for a stock `ACD_CabSimTMS` over a carried-over cab
/// ([`stock_cabsim_replace_is_dual`]).
pub fn check_op(
    counts: &Counts,
    candidate_id: &str,
    replaced_id: Option<&str>,
    is_replace: bool,
    cand_dual_cab: bool,
    replaced_dual_cab: bool,
) -> Result<(), BlockCapError> {
    let s = sets();
    // A replace with no resolved target frees nothing.
    let is_replace = is_replace && replaced_id.is_some();
    let replaced = replaced_id.filter(|_| is_replace).unwrap_or("");
    let mut next = counts.clone();
    next.add(candidate_id, cand_dual_cab);
    if is_replace {
        next.remove(replaced, replaced_dual_cab);
    }
    if over_budget(next.cpu) {
        return Err(BlockCapError::ProcessorUtilization);
    }

    // FX loops: the candidate against every loop left after the replace.
    if s.fx_loops.contains(candidate_id) {
        let mut others = counts.clone();
        if is_replace {
            others.remove(replaced, replaced_dual_cab);
        }
        let stereo = &s.fx_loop_stereo;
        let clash = others.holds_loop(candidate_id)
            || (candidate_id == stereo && s.fx_loop_mono.iter().any(|m| others.holds_loop(m)))
            || (s.fx_loop_mono.contains(candidate_id) && others.holds_loop(stereo));
        if clash {
            return Err(BlockCapError::FXLoopCoexistence);
        }
    }

    let member = |set: &HashSet<String>, id: &str| i64::from(set.contains(id));
    let conv = member(&s.convolution, candidate_id) - member(&s.convolution, replaced)
        + i64::from(counts.conv > 0);
    if conv > 1 {
        return Err(BlockCapError::ConvolutionReverbLimit);
    }

    if !s.cabinet_exempt.contains(candidate_id) {
        // Members only, as `Counts` holds them: a dual flag on a non-member weighs nothing.
        let weight = |id: &str, dual: bool| member(&s.cabinet, id) * if dual { 2 } else { 1 };
        let cand = weight(candidate_id, cand_dual_cab);
        let gone = if is_replace {
            weight(replaced, replaced_dual_cab)
        } else {
            0
        };
        if counts.cabinet + cand - gone > 2 {
            return Err(BlockCapError::ComboHalfStackCabinetsLimit);
        }
    }

    if next.glooper > 2 && candidate_id == s.glooper {
        return Err(BlockCapError::GlooperEffectsLimit);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stock replace into `ACD_CabSimTMS` lands dual only over a node whose id is a cab
    /// model on the firmware's list (static RE + HW, fw 1.8.58).
    #[test]
    fn a_stock_cabsim_replace_lands_dual_only_over_a_listed_cab_id() {
        // A cab model's own id (the HW case), and a listed cab with no catalog tile of its own.
        assert!(stock_cabsim_replace_is_dual("ACD_MarG12H30BB", None));
        assert!(stock_cabsim_replace_is_dual(
            "ACD_TwoRock2x12Vertical",
            None
        ));
        // A `_1` id, a non-minted id, an amp with a built-in cab, a cab block itself and
        // a catalog cab the firmware lacks: the on-load default, factory single.
        for id in [
            "ACD_MarG12H30BB_1",
            "cab1",
            "ACD_TwinReverb65NoFxCabIR",
            "ACD_CabSimTMS",
            "ACD_British212AlnicoBlue",
        ] {
            assert!(!stock_cabsim_replace_is_dual(id, None), "{id}");
            assert!(
                stock_cabsim_replace_is_dual(id, Some(true)),
                "{id}: a dual on-load default"
            );
        }
        assert_eq!(sets().cabsim_carry_over.len(), 68);
    }

    fn groups(groups: &[(&str, &[&str])]) -> Roster {
        group_roster(
            groups
                .iter()
                .flat_map(|(g, ids)| ids.iter().map(move |id| (*g, *id))),
        )
    }

    #[test]
    fn the_empty_load_substitute_fails_the_loaded_body_check() {
        let file = groups(&[("G1", &["ACD_TubeScreamer"]), ("G2", &["ACD_TwinReverb65"])]);
        let err = check_loaded_roster(&file, &Roster::new()).unwrap_err();
        assert!(err.contains("empty body"), "{err}");
    }

    #[test]
    fn the_loaded_body_check_ignores_order_within_a_group_only() {
        let file = groups(&[("G1", &["ACD_TubeScreamer", "ACD_Rat", "ACD_TubeScreamer"])]);
        let live = groups(&[("G1", &["ACD_Rat", "ACD_TubeScreamer", "ACD_TubeScreamer"])]);
        assert!(check_loaded_roster(&file, &live).is_ok());
        assert!(check_loaded_roster(&Roster::new(), &Roster::new()).is_ok());
        let moved = groups(&[
            ("G1", &["ACD_Rat", "ACD_TubeScreamer"]),
            ("G2", &["ACD_TubeScreamer"]),
        ]);
        assert!(check_loaded_roster(&file, &moved).is_err());
    }

    #[test]
    fn a_renamed_or_cut_body_fails_the_loaded_body_check() {
        let file = groups(&[("G1", &["ACD_TubeScreamer", "ACD_TwinReverb65"])]);
        let renamed = groups(&[("G1", &["DUBS_Unknown", "ACD_TwinReverb65"])]);
        let cut = groups(&[("G1", &["ACD_TubeScreamer"])]);
        assert!(check_loaded_roster(&file, &renamed).is_err());
        assert!(check_loaded_roster(&file, &cut).is_err());
    }

    // Real ids from `block-classification.json` / `model-cpu.json`.
    const CAB_A: &str = "ACD_AC30BrilliantCabIR";
    const CAB_B: &str = "ACD_AC30NormalCabIR";
    const CAB_C: &str = "ACD_Ampeg66B15CabIR";
    const STANDALONE_CAB: &str = "ACD_CabSimTMS";
    const CONV_ONLY_A: &str = "ACD_TMSpring63Conv";
    const CONV_ONLY_B: &str = "ACD_TMSpring65Conv";
    // Baked reverb-combo: a member of BOTH convolutionSet AND cabinetSet.
    const BAKED_REVERB_COMBO: &str = "ACD_DeluxeReverb68CustomCabIRConvRvb";
    // Its dry sibling: cabinet-only (NOT in convolutionSet) — replacing the wet combo
    // with this frees the conv slot but keeps the cabinet slot occupied.
    const DRY_VARIANT: &str = "ACD_DeluxeReverb68CustomNoFxCabIR";
    const GLOOPER: &str = "ACD_Glooper";
    const NON_CAPPED: &str = "ACD_ChorusCE2"; // in neither set — always a no-op insert.

    fn roster(entries: &[(&str, bool)]) -> Vec<RosterEntry> {
        entries
            .iter()
            .map(|(id, d)| RosterEntry {
                group: "G1".into(),
                node_id: (*id).into(),
                fender_id: (*id).into(),
                dual_cab: *d,
            })
            .collect()
    }

    #[test]
    fn two_cabinets_then_a_third_insert_errs() {
        let c = counts(&roster(&[(CAB_A, false), (CAB_B, false)]));
        assert_eq!(
            check_op(&c, CAB_C, None, false, false, false),
            Err(BlockCapError::ComboHalfStackCabinetsLimit)
        );
        // A non-cabinet insert is unaffected by the existing 2 cabs.
        assert_eq!(check_op(&c, NON_CAPPED, None, false, false, false), Ok(()));
    }

    #[test]
    fn one_conv_plus_baked_reverb_combo_insert_errs() {
        // Existing conv-only block + inserting a combo that is ALSO a conv member.
        let c = counts(&roster(&[(CONV_ONLY_A, false)]));
        assert_eq!(
            check_op(&c, BAKED_REVERB_COMBO, None, false, false, false),
            Err(BlockCapError::ConvolutionReverbLimit)
        );
        // And the reverse order: existing baked combo + inserting a plain conv.
        let c2 = counts(&roster(&[(BAKED_REVERB_COMBO, false)]));
        assert_eq!(
            check_op(&c2, CONV_ONLY_A, None, false, false, false),
            Err(BlockCapError::ConvolutionReverbLimit)
        );
    }

    #[test]
    fn conv_to_conv_replace_is_ok() {
        let c = counts(&roster(&[(CONV_ONLY_A, false)]));
        assert_eq!(
            check_op(&c, CONV_ONLY_B, Some(CONV_ONLY_A), true, false, false),
            Ok(())
        );
    }

    #[test]
    fn replacing_a_reverb_combo_with_its_dry_variant_frees_the_conv_slot() {
        let mut c = counts(&roster(&[(BAKED_REVERB_COMBO, false)]));
        // Step 1: replace the wet combo with its dry (cabinet-only) sibling — frees conv.
        assert_eq!(
            check_op(
                &c,
                DRY_VARIANT,
                Some(BAKED_REVERB_COMBO),
                true,
                false,
                false
            ),
            Ok(())
        );
        c.add(DRY_VARIANT, false);
        c.remove(BAKED_REVERB_COMBO, false);
        assert_eq!(c.conv, 0, "the conv slot must be freed");
        assert_eq!(c.cabinet, 1, "the cabinet slot stays occupied");
        // Step 2: a SECOND conv insert is now allowed (conv count is back to 0).
        assert_eq!(check_op(&c, CONV_ONLY_A, None, false, false, false), Ok(()));
    }

    #[test]
    fn dual_cab_alone_is_ok_but_a_second_cab_errs() {
        let c = counts(&roster(&[(STANDALONE_CAB, true)])); // dual → cabinet=2
        assert_eq!(check_op(&c, NON_CAPPED, None, false, false, false), Ok(()));
        assert_eq!(
            check_op(&c, CAB_A, None, false, false, false),
            Err(BlockCapError::ComboHalfStackCabinetsLimit)
        );
    }

    #[test]
    fn two_gloopers_ok_a_third_errs() {
        let c = counts(&roster(&[(GLOOPER, false), (GLOOPER, false)]));
        assert_eq!(check_op(&c, NON_CAPPED, None, false, false, false), Ok(()));
        assert_eq!(
            check_op(&c, GLOOPER, None, false, false, false),
            Err(BlockCapError::GlooperEffectsLimit)
        );
    }

    #[test]
    fn fx_loop_coexistence_is_bidirectional() {
        let stereo_present = counts(&roster(&[("ACD_FxLoop3_4", false)]));
        assert_eq!(
            check_op(&stereo_present, "ACD_FxLoop3", None, false, false, false),
            Err(BlockCapError::FXLoopCoexistence)
        );
        let mono_present = counts(&roster(&[("ACD_FxLoop3", false)]));
        assert_eq!(
            check_op(&mono_present, "ACD_FxLoop3_4", None, false, false, false),
            Err(BlockCapError::FXLoopCoexistence)
        );
        // The two mono loops coexist fine (not mutually exclusive with each other).
        let one_mono = counts(&roster(&[("ACD_FxLoop3", false)]));
        assert_eq!(
            check_op(&one_mono, "ACD_FxLoop4", None, false, false, false),
            Ok(())
        );
    }

    #[test]
    fn cpu_sum_over_budget_errs() {
        // ACD_GuitarSynth 36.0 + ACD_DualRectifierCabIR 22.8 + ACD_HypersonicAmp6L6BlueCabIR
        // 28.8 = 87.6 > 76.5.
        let c = counts(&roster(&[
            ("ACD_GuitarSynth", false),
            ("ACD_DualRectifierCabIR", false),
        ]));
        assert_eq!(
            check_op(
                &c,
                "ACD_HypersonicAmp6L6BlueCabIR",
                None,
                false,
                false,
                false
            ),
            Err(BlockCapError::ProcessorUtilization)
        );
    }

    #[test]
    fn unknown_id_costs_twenty_like_the_firmware() {
        // 3 × 20 = 60 fits; a fourth unknown reaches 80.
        let ids = [
            ("ACD_NotA", false),
            ("ACD_NotB", false),
            ("ACD_NotC", false),
        ];
        let c = counts(&roster(&ids));
        assert!((c.cpu - 60.0).abs() < 1e-9);
        assert_eq!(
            check_op(&c, "ACD_NotD", None, false, false, false),
            Err(BlockCapError::ProcessorUtilization)
        );
    }

    #[test]
    fn a_total_of_exactly_the_budget_fails() {
        // ACD_GuitarSynth 36 + 36 = 72; + ACD_LoFi 9.5 (1.8.58) = 81.5 fails, while
        // 72 + ACD_Blackbox 1.7 = 73.7 fits. 76.4 fits, 76.5 rounds to 77 and fails.
        let c = counts(&roster(&[
            ("ACD_GuitarSynth", false),
            ("ACD_GuitarSynth", false),
        ]));
        assert_eq!(
            check_op(&c, "ACD_LoFi", None, false, false, false),
            Err(BlockCapError::ProcessorUtilization)
        );
        assert_eq!(
            check_op(&c, "ACD_Blackbox", None, false, false, false),
            Ok(())
        );
        assert!(!over_budget(76.4));
        assert!(over_budget(76.5));
        assert!(over_budget(76.4 + 0.1));
    }

    #[test]
    fn roster_from_preset_reads_dual_cab_from_dspunitparameters() {
        let preset = serde_json::json!({
            "audioGraph": {
                "guitarNodes": {
                    "G1": [
                        {
                            "FenderId": "ACD_CabSimTMS",
                            "nodeId": "n1",
                            "dspUnitParameters": { "cabsim2enabled": true }
                        },
                        { "FenderId": "ACD_AC30BrilliantCabIR", "nodeId": "n2" }
                    ]
                }
            }
        });
        let r = roster_from_preset(&preset);
        assert_eq!(
            r,
            vec![
                RosterEntry {
                    group: "G1".into(),
                    node_id: "n1".into(),
                    fender_id: "ACD_CabSimTMS".into(),
                    dual_cab: true,
                },
                RosterEntry {
                    group: "G1".into(),
                    node_id: "n2".into(),
                    fender_id: "ACD_AC30BrilliantCabIR".into(),
                    dual_cab: false,
                },
            ]
        );
        let c = counts(&r);
        assert_eq!(c.cabinet, 3, "dual-cab (2) + one more cabinet (1)");
    }

    // fw 1.8.58 client rules, the cabinet weights confirmed on the unit's own screen.
    const AMP_COMBO: &str = "ACD_TwinReverb65NoFxCabIR";
    const STANDALONE_CAB_MODEL: &str = "ACD_Fen57Champ";
    const EXTERNAL_CAB: &str = "ACD_ExternalCab";

    #[test]
    fn an_amp_with_its_second_mic_takes_two_cabinet_slots() {
        let preset = serde_json::json!({"audioGraph": {"guitarNodes": {"G1": [{
            "FenderId": AMP_COMBO, "nodeId": "amp",
            "dspUnitParameters": {"cabsim2enabled": true}
        }]}}});
        let c = counts(&roster_from_preset(&preset));
        assert_eq!(c.cabinet, 2);
        assert_eq!(
            check_op(&c, STANDALONE_CAB, None, false, false, false),
            Err(BlockCapError::ComboHalfStackCabinetsLimit),
            "the unit greys out every cabinet and IR"
        );
        let one_mic = counts(&roster(&[(AMP_COMBO, false)]));
        assert_eq!(
            check_op(&one_mic, STANDALONE_CAB, None, false, false, false),
            Ok(())
        );
    }

    #[test]
    fn external_cab_neither_counts_nor_is_ever_refused() {
        let c = counts(&roster(&[(EXTERNAL_CAB, false), (AMP_COMBO, false)]));
        assert_eq!(c.cabinet, 1, "ExternalCab is exempt");
        assert_eq!(check_op(&c, CAB_A, None, false, false, false), Ok(()));
        let full = counts(&roster(&[(CAB_A, false), (CAB_B, false), (CAB_C, false)]));
        assert_eq!(
            check_op(&full, EXTERNAL_CAB, None, false, false, false),
            Ok(()),
            "an exempt candidate passes even over the cap"
        );
        assert_eq!(
            check_op(&full, NON_CAPPED, None, false, false, false),
            Err(BlockCapError::ComboHalfStackCabinetsLimit),
            "over the cap, every other candidate is refused"
        );
    }

    #[test]
    fn standalone_cabinet_models_count_toward_the_cap() {
        let c = counts(&roster(&[(STANDALONE_CAB_MODEL, false), (CAB_A, false)]));
        assert_eq!(c.cabinet, 2);
        assert_eq!(
            check_op(&c, CAB_B, None, false, false, false),
            Err(BlockCapError::ComboHalfStackCabinetsLimit)
        );
    }

    #[test]
    fn a_dual_cab_replace_frees_two_slots_and_a_dual_candidate_takes_two() {
        let c = counts(&roster(&[(STANDALONE_CAB, true), (CAB_A, false)]));
        assert_eq!(c.cabinet, 3);
        assert_eq!(
            check_op(&c, CAB_B, Some(STANDALONE_CAB), true, false, true),
            Ok(())
        );
        let one = counts(&roster(&[(CAB_A, false)]));
        assert_eq!(
            check_op(&one, STANDALONE_CAB, None, false, true, false),
            Err(BlockCapError::ComboHalfStackCabinetsLimit)
        );
    }

    #[test]
    fn the_convolution_cap_counts_presence_not_members() {
        let two = counts(&roster(&[(CONV_ONLY_A, false), (CONV_ONLY_B, false)]));
        assert_eq!(
            check_op(&two, CONV_ONLY_A, Some(CONV_ONLY_B), true, false, false),
            Ok(()),
            "a member-for-member replace passes even over the cap"
        );
        assert_eq!(
            check_op(&two, NON_CAPPED, None, false, false, false),
            Ok(())
        );
        let c = counts(&roster(&[("ACD_PrincetonReverb68CabIRConvRvb", false)]));
        assert_eq!(
            check_op(&c, CONV_ONLY_A, None, false, false, false),
            Err(BlockCapError::ConvolutionReverbLimit),
            "the '68 Princeton reverb combo is a convolution member on fw 1.8.58"
        );
    }

    #[test]
    fn no_fx_loop_may_appear_twice() {
        for id in [
            "ACD_FxLoop1",
            "ACD_FxLoop2",
            "ACD_FxLoop3",
            "ACD_FxLoop4",
            "ACD_FxLoop3_4",
        ] {
            let c = counts(&roster(&[(id, false)]));
            assert_eq!(
                check_op(&c, id, None, false, false, false),
                Err(BlockCapError::FXLoopCoexistence),
                "{id}"
            );
            assert_eq!(
                check_op(&c, id, Some(id), true, false, false),
                Ok(()),
                "{id}"
            );
        }
        let c = counts(&roster(&[("ACD_FxLoop1", false), ("ACD_FxLoop3_4", false)]));
        assert_eq!(
            check_op(&c, "ACD_FxLoop2", None, false, false, false),
            Ok(())
        );
    }

    #[test]
    fn a_replace_with_no_resolved_target_frees_nothing() {
        let c = counts(&roster(&[(CAB_A, false), (CAB_B, false)]));
        assert_eq!(
            check_op(&c, CAB_C, None, true, false, false),
            Err(BlockCapError::ComboHalfStackCabinetsLimit)
        );
        // 2 × 36 = 72 fits; a third reaches 108 with nothing freed.
        let full = counts(&roster(&[
            ("ACD_GuitarSynth", false),
            ("ACD_GuitarSynth", false),
        ]));
        assert_eq!(
            check_op(&full, "ACD_GuitarSynth", None, true, false, false),
            Err(BlockCapError::ProcessorUtilization)
        );
    }

    #[test]
    fn a_dual_flag_on_a_non_member_frees_no_cabinet_slot() {
        let c = counts(&roster(&[
            (CAB_A, false),
            (CAB_B, false),
            (NON_CAPPED, true),
        ]));
        assert_eq!(c.cabinet, 2);
        assert_eq!(
            check_op(&c, CAB_C, Some(NON_CAPPED), true, false, true),
            Err(BlockCapError::ComboHalfStackCabinetsLimit)
        );
    }
}
