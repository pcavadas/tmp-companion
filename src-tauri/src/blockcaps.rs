//! Firmware-faithful pre-flight guard for the Tone Master Pro's 5 block-count caps
//! (`NodeSelectionRestrictions`, decompiled from `tone-master-stomp-client`).
//!
//! **This is the SOLE enforcement.** The device audio engine (`tm-stomp-server`) does
//! NOT enforce any of these caps and cannot reject an over-cap edit with a
//! `presetError` — two independent decompile passes confirmed the count-cap code path
//! is entirely client-side (UI-only) in `tone-master-stomp-client`. So the Rust apply
//! path (`lib.rs`'s `copy_apply_one` / `held_replace_one` / `replace_one_live`) is the
//! primary, fail-closed guard: it must catch every over-cap edit BEFORE it reaches the
//! device, because nothing downstream will.
//!
//! The 5 rules, in firmware enum order (the order `check_op` reports the first
//! violation in):
//! 0. `ProcessorUtilization` — Σ per-block `cpuByBid` ≤ `budget` (76.5).
//! 1. `FXLoopCoexistence` — `ACD_FxLoop3_4` (stereo) is mutually exclusive with EITHER
//!    `ACD_FxLoop3` or `ACD_FxLoop4` (mono). Bidirectional; not a count.
//! 2. `ConvolutionReverbLimit` — max 1 member of `convolutionSet`.
//! 3. `ComboHalfStackCabinetsLimit` — max 2 member-weight of `cabinetSet`; a dual-cab
//!    node (`dspUnitParameters.cabsim2enabled == true`) counts as 2.
//! 4. `GlooperEffectsLimit` — max 2 of `ACD_Glooper`.
//!
//! Membership is **exact-string** against the sets in `block-classification.json` — the
//! device's ids arrive verbatim with their suffix (e.g.
//! `ACD_DeluxeReverb68CustomCabIRConvRvb`), and the sets already enumerate every
//! runtime variant. NEVER normalize / strip a suffix before a set lookup: that would
//! silently misclassify a real device id (see the module's RE plan for the load-bearing
//! `ConRvb` typo id and the empty-`acdCategory` standalone cab/IR that a derived filter
//! would miss).

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

/// The 3 extracted membership sets + the 2 FX-loop ids, parsed once from
/// `block-classification.json` (checked-in, RE-extracted — see the module docs).
struct Sets {
    convolution: HashSet<String>,
    cabinet: HashSet<String>,
    glooper: String,
    fx_loop_stereo: String,
    fx_loop_mono: HashSet<String>,
}

fn sets() -> &'static Sets {
    static SETS: OnceLock<Sets> = OnceLock::new();
    SETS.get_or_init(|| {
        let v: Value = serde_json::from_str(include_str!(
            "../../src/models/block-classification.json"
        ))
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

/// The CPU budget + per-block cost table, parsed once from `model-cpu.json`.
struct Costs {
    budget: f64,
    cpu_by_bid: HashMap<String, f64>,
}

fn costs() -> &'static Costs {
    static COSTS: OnceLock<Costs> = OnceLock::new();
    COSTS.get_or_init(|| {
        let v: Value = serde_json::from_str(include_str!("../../src/models/model-cpu.json"))
            .expect("model-cpu.json must parse");
        let budget = v.get("budget").and_then(Value::as_f64).unwrap_or(76.5);
        let cpu_by_bid = v
            .get("cpuByBid")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(k, val)| val.as_f64().map(|f| (k.clone(), f)))
            .collect();
        Costs { budget, cpu_by_bid }
    })
}

/// One graph node's identity + the properties the guard counts: its exact FenderId
/// (used byte-verbatim — see module docs) and whether it's a dual-cab CabSimTMS node,
/// which counts as 2 cabinet slots for `ComboHalfStackCabinetsLimit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterEntry {
    pub group: String,
    pub node_id: String,
    pub fender_id: String,
    pub dual_cab: bool,
}

/// Walk decoded preset JSON into the pre-edit roster the guard counts against —
/// sourced from `session::extract_active_graph`, the ONE place that owns the node walk
/// AND the load-bearing dual-cab discriminator (`cabsim2enabled` is a real second cab
/// ONLY on the standalone `ACD_CabSimTMS`; on an amp the same key means a dual MIC on
/// one cab — see `session.rs`'s `extract_active_graph_amp_half_stack…` test). A single
/// source means the cabinet weighting here can never drift from what the strip renders.
pub fn roster_from_preset(preset: &Value) -> Vec<RosterEntry> {
    crate::session::extract_active_graph(preset, None)
        .nodes
        .into_iter()
        .map(|n| RosterEntry {
            group: n.group_id,
            node_id: n.node_id,
            fender_id: n.model,
            dual_cab: n.cab_sim2_enabled == Some(true),
        })
        .collect()
}

/// The running per-cap totals a roster contributes. A dual-cab node's cabinet
/// contribution is 2 (`ComboHalfStackCabinetsLimit`'s vtable-confirmed weight); every
/// other membership contributes 1. Signed so [`Counts::remove`] (a replace/remove
/// delta) can never underflow-panic.
#[derive(Debug, Clone, Default)]
pub struct Counts {
    pub conv: i64,
    pub cabinet: i64,
    pub glooper: i64,
    pub cpu: f64,
    pub fx_stereo: i64,
    pub fx_mono: i64,
}

impl Counts {
    /// Add one node's contribution (an insert, or the candidate side of a replace).
    pub fn add(&mut self, id: &str, dual_cab: bool) {
        let s = sets();
        if s.convolution.contains(id) {
            self.conv += 1;
        }
        if s.cabinet.contains(id) {
            self.cabinet += if dual_cab { 2 } else { 1 };
        }
        if id == s.glooper {
            self.glooper += 1;
        }
        if id == s.fx_loop_stereo {
            self.fx_stereo += 1;
        }
        if s.fx_loop_mono.contains(id) {
            self.fx_mono += 1;
        }
        self.cpu += costs().cpu_by_bid.get(id).copied().unwrap_or(0.0);
    }

    /// Subtract one node's contribution (a remove, or the replaced-anchor side of a
    /// replace — mode matters: an insert/append never subtracts).
    pub fn remove(&mut self, id: &str, dual_cab: bool) {
        let s = sets();
        if s.convolution.contains(id) {
            self.conv -= 1;
        }
        if s.cabinet.contains(id) {
            self.cabinet -= if dual_cab { 2 } else { 1 };
        }
        if id == s.glooper {
            self.glooper -= 1;
        }
        if id == s.fx_loop_stereo {
            self.fx_stereo -= 1;
        }
        if s.fx_loop_mono.contains(id) {
            self.fx_mono -= 1;
        }
        self.cpu -= costs().cpu_by_bid.get(id).copied().unwrap_or(0.0);
    }
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

/// The authoritative per-op guard: would inserting/replacing `candidate_id` push any of
/// the 5 caps over its max, given the CURRENT running `counts`? Applies the same
/// formula for every count cap: `(cand∈set) − (replaced∈set, only on a replace) +
/// existing < MAX+1`. `FXLoopCoexistence` isn't a count — it's a bidirectional mutual
/// exclusion, checked as its own branch.
///
/// `is_replace` selects whether `replaced_id`'s contribution is subtracted (an
/// insert/append never frees a slot); `cand_dual_cab`/`replaced_dual_cab` are whether
/// the candidate/replaced node is a dual-cab CabSim (counts as 2 cabinet slots).
///
/// Checked in firmware enum order (0..4); returns the FIRST violated reason, matching
/// the TS `checkEdit` sibling so the two guards never disagree on which reason to show.
pub fn check_op(
    counts: &Counts,
    candidate_id: &str,
    replaced_id: Option<&str>,
    is_replace: bool,
    cand_dual_cab: bool,
    replaced_dual_cab: bool,
) -> Result<(), BlockCapError> {
    let mut next = counts.clone();
    next.add(candidate_id, cand_dual_cab);
    if is_replace {
        next.remove(replaced_id.unwrap_or(""), replaced_dual_cab);
    }

    if next.cpu > costs().budget + 1e-6 {
        return Err(BlockCapError::ProcessorUtilization);
    }
    if next.fx_stereo > 0 && next.fx_mono > 0 {
        return Err(BlockCapError::FXLoopCoexistence);
    }
    if next.conv > 1 {
        return Err(BlockCapError::ConvolutionReverbLimit);
    }
    if next.cabinet > 2 {
        return Err(BlockCapError::ComboHalfStackCabinetsLimit);
    }
    if next.glooper > 2 {
        return Err(BlockCapError::GlooperEffectsLimit);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            check_op(&c, DRY_VARIANT, Some(BAKED_REVERB_COMBO), true, false, false),
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
            check_op(&c, "ACD_HypersonicAmp6L6BlueCabIR", None, false, false, false),
            Err(BlockCapError::ProcessorUtilization)
        );
    }

    #[test]
    fn unknown_id_costs_nothing() {
        let c = counts(&roster(&[]));
        assert_eq!(check_op(&c, "ACD_NotARealBlock", None, false, false, false), Ok(()));
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
}
