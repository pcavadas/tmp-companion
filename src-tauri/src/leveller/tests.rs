//! Unit tests across the leveling lanes (the pure solves, the floor guard, the correction
//! loop, and the device seams against a scripted transport).

use super::*;
use crate::proto;

/// The `loadPreset` list indices `t` was sent, in order.
fn loads_sent(t: &crate::test_support::ScriptedTransport) -> Vec<u32> {
    let sent = t.sent.lock().unwrap().clone();
    sent.iter()
        .filter_map(|b| (0..512u32).find(|&slot| *b == proto::load_preset(u64::from(slot) + 1, 1)))
        .collect()
}

/// BUG→GATE: fw 1.8.58 skips a same-slot `loadPreset` while the dirty flag is clear,
/// so the restore — the one "discard unsaved edits" path — must never
/// rely on it: it loads a neighbour first, and the target LAST.
#[test]
fn restore_loads_a_neighbour_before_the_target() {
    for (slot, want) in [
        (408, vec![407, 408]),
        (400, vec![401, 400]),
        (0, vec![1, 0]),
    ] {
        let t = crate::test_support::ScriptedTransport::default();
        let mut s = crate::test_support::session_over(&t, Vec::new());
        reload_stored_on(&mut s, ListIndex(slot)).unwrap();
        assert_eq!(loads_sent(&t), want);
    }
}

#[test]
fn doctor_stim_slice_truncates_then_pads_with_leading_silence() {
    let src = vec![0.7f32; doctor_stim_samples() * 2];
    let prepared = doctor_stim_slice(src);
    assert_eq!(prepared.len(), doctor_pad_samples() + doctor_stim_samples());
    assert!(prepared[..doctor_pad_samples()].iter().all(|&s| s == 0.0));
    assert!(prepared[doctor_pad_samples()..].iter().all(|&s| s == 0.7));
}

// A cancel flag already set at entry must bail at the PRE-MEASURE checkpoint —
// before any `Session::connect`/device touch — so this runs with no hardware and
// returns `LevelError::Cancelled`. Guards both the master-level and block-knob paths.
#[test]
fn cancel_before_measure_short_circuits_without_device() {
    let stim = [0.0f32; 16];
    let opts = LevelOptions::default();
    assert_eq!(
        level_preset(ListIndex(0), &stim, -30.0, opts, &[], None, || true).unwrap_err(),
        LevelError::Cancelled
    );
    let knob = LevelKnob::PresetLevel;
    assert_eq!(
        level_preset_block(ListIndex(0), &stim, &knob, 0.05, 1.0, -30.0, opts, || true)
            .unwrap_err(),
        LevelError::Cancelled
    );
}

// #269: a cancel whose stored-preset restore ALSO fails used to come back as
// "cancelled; also failed to restore …", which no longer equalled the sentinel, so every
// `== CANCELLED` caller treated the Stop as an ordinary failure. Typed, it stays a cancel.
#[test]
fn a_cancelled_run_whose_restore_fails_is_still_a_cancel() {
    let mut restored = Vec::new();
    let failing = |slot: ListIndex| -> Result<(), String> {
        restored.push(slot);
        Err("HID thread gone".to_string())
    };
    // `save: true` still restores on a cancel (it bailed before `apply_level`).
    let err = restore_after_unsaved_error_with(
        ListIndex(7),
        true,
        Err::<(), _>(LevelError::Cancelled),
        failing,
    )
    .unwrap_err();
    assert_eq!(
        restored,
        [ListIndex(7)],
        "a cancel must attempt the restore"
    );
    assert!(
        err.is_cancelled(),
        "a cancel with a failed restore is still a cancel: {err:?}"
    );
    // The text the UI shows is unchanged from the String era.
    assert_eq!(
        err.to_string(),
        "cancelled; also failed to restore stored preset: HID thread gone"
    );
    // And the run-end stamp leaves it a cancel: no re-amp warning appended to it.
    let unverified = ReampOffOutcome { verified: false };
    assert!(unverified.annotate_err(err).is_cancelled());

    // A non-cancel failure on a `save: true` run skips the restore entirely …
    let err = restore_after_unsaved_error_with(
        ListIndex(7),
        true,
        Err::<(), _>(LevelError::Device("write failed".into())),
        |_| panic!("a save run's own failure must not reload"),
    )
    .unwrap_err();
    assert_eq!(err, LevelError::Device("write failed".into()));
    // … and a silent capture on an unsaved run keeps its kind through a failed restore.
    let err = restore_after_unsaved_error_with(
        ListIndex(7),
        false,
        Err::<(), _>(LevelError::NoSignal),
        |_| Err("gone".to_string()),
    )
    .unwrap_err();
    assert!(err.is_no_signal() && !err.is_cancelled(), "{err:?}");
}

// Footswitch generic param-space secant: hits a linear response, gives up on a flat one.
#[test]
fn fs_secant_converges_and_detects_flat() {
    // loudness(v) = 10·v − 30 → target −23 ⇒ v = 0.7.
    let f = |v: f64| 10.0 * v - 30.0;
    let next = secant_step(
        (0.25f64, f(0.25)),
        (0.75, f(0.75)),
        -23.0,
        Flatness::SolveCoord,
        None,
    )
    .unwrap();
    assert!((next - 0.70).abs() < 1e-9, "got {next}");
    // Flat response → None (no authority).
    assert!(secant_step(
        (0.25, -9.0),
        (0.75, -9.0),
        -23.0,
        Flatness::SolveCoord,
        None
    )
    .is_none());
}

// See `fs_bracket_expansion`'s doc for the bug this covers.
#[test]
fn fs_bracket_expansion_targets_the_extreme_that_can_bracket() {
    // Seed pair both near a saturated ceiling (flat) — target is well below both, so
    // the amp needs to go QUIETER: probe toward 0.0.
    assert_eq!(
        fs_bracket_expansion(0.25, -18.0, 0.75, -17.9, -25.0, UNIT_BOUNDS),
        Some(0.0)
    );
    // Symmetric case: target well ABOVE both seeds (needs MORE loudness) → probe 1.0.
    assert_eq!(
        fs_bracket_expansion(0.25, -30.0, 0.75, -29.9, -20.0, UNIT_BOUNDS),
        Some(1.0)
    );
    // Target already bracketed by the seed pair → no expansion needed, the plain
    // secant can converge as-is.
    assert_eq!(
        fs_bracket_expansion(0.25, -30.0, 0.75, -18.0, -23.0, UNIT_BOUNDS),
        None
    );
    // The relevant extreme is ALREADY one of the seeds — nothing left to try.
    assert_eq!(
        fs_bracket_expansion(0.0, -30.0, 0.75, -29.9, -35.0, UNIT_BOUNDS),
        None
    );
    assert_eq!(
        fs_bracket_expansion(0.25, -18.0, 1.0, -17.9, -10.0, UNIT_BOUNDS),
        None
    );
    // The extremes follow the PARAM's range, not a hard-coded [0, 1]: on a raw-dB
    // `[0, 12]` param the quiet/loud probes are 0.0 and 12.0, and a seed already AT
    // 12.0 leaves nothing to try.
    assert_eq!(
        fs_bracket_expansion(3.0, -30.0, 9.0, -29.9, -20.0, (0.0, 12.0)),
        Some(12.0)
    );
    assert_eq!(
        fs_bracket_expansion(3.0, -30.0, 12.0, -29.9, -20.0, (0.0, 12.0)),
        None
    );
}

// The full bracket-then-secant shape, mirroring `correct_iter_secant_converges_on_compressor`'s
// convention (replicate the runner's loop against a synthetic response curve, no device).
#[test]
fn fs_bracket_expansion_lets_the_secant_reach_a_target_below_a_saturated_seed_pair() {
    let ceiling = -18.0_f64;
    let model = |v: f64| {
        // Saturates by v=0.2 — BOTH seeds (0.25 and 0.75) land in the flat ceiling,
        // reproducing the reported bug (a knob whose useful range is a small slice
        // of [0, 1]).
        if v > 0.2 {
            ceiling
        } else {
            // Rises linearly from -40 (silent) to the ceiling over 0..0.2.
            -40.0 + (ceiling - -40.0) * (v / 0.2)
        }
    };
    let target = -25.0_f64;
    let (v_lo, v_hi) = (0.25_f64, 0.75_f64);
    let (l_lo, l_hi) = (model(v_lo), model(v_hi));
    let err = |l: f64| (l - target).abs();

    // Old behavior check: the seed pair alone is flat (both on the saturated ceiling) —
    // confirms this fixture actually reproduces the reported bug, not a fixture error.
    assert!(
        (l_hi - l_lo).abs() < KNOB_TOL_LU,
        "fixture must reproduce a flat seed pair: l_lo={l_lo} l_hi={l_hi}"
    );
    assert!(
        err(l_lo.min(l_hi)) > KNOB_TOL_LU,
        "target must be unreached by either seed"
    );

    let mut best = if err(l_lo) <= err(l_hi) {
        (v_lo, l_lo)
    } else {
        (v_hi, l_hi)
    };
    let (mut p0, mut p1) = ((v_lo, l_lo), (v_hi, l_hi));
    if let Some(v_extreme) =
        fs_bracket_expansion(v_lo as f32, l_lo, v_hi as f32, l_hi, target, UNIT_BOUNDS)
    {
        let l_extreme = model(v_extreme as f64);
        if err(l_extreme) < err(best.1) {
            best = (v_extreme as f64, l_extreme);
        }
        if err(p0.1) <= err(p1.1) {
            p1 = (v_extreme as f64, l_extreme);
        } else {
            p0 = (v_extreme as f64, l_extreme);
        }
    }
    if (p1.1 - p0.1).abs() >= KNOB_TOL_LU {
        for _ in 0..MEASURE_CORRECT_MAX {
            if err(best.1) <= KNOB_TOL_LU {
                break;
            }
            let Some(raw) = secant_step(p0, p1, target, Flatness::SolveCoord, None) else {
                break;
            };
            let v2 = raw.clamp(0.0, 1.0);
            let l2 = model(v2);
            if err(l2) < err(best.1) {
                best = (v2, l2);
            }
            p0 = p1;
            p1 = (v2, l2);
        }
    }
    assert!(
        err(best.1) <= KNOB_TOL_LU,
        "expected convergence near {target}, got {} (v={})",
        best.1,
        best.0
    );
}

#[test]
fn param_fn_present_matches_switch_index_and_param() {
    let ftsw = serde_json::json!([
        [],
        [{ "func": "on-off" }, { "func": "param", "parameterId": "gain" }],
    ]);
    assert!(param_fn_present(&ftsw, 1, 1, "gain"));
    assert!(!param_fn_present(&ftsw, 1, 1, "level")); // wrong param
    assert!(!param_fn_present(&ftsw, 1, 0, "gain")); // index 0 is on-off
    assert!(!param_fn_present(&ftsw, 0, 0, "gain")); // empty switch
}

/// Solve for `C` from a real on-device data point: -26.70 LUFS at ref 0.5.
/// C is the captured loudness at level=1.0 (20·log10(1)=0), i.e. this
/// preset's MAX achievable captured loudness ≈ -20.68 LUFS.
fn c_from_real_data() -> f64 {
    let (measured, ref_level) = (-26.70f64, 0.5f64);
    measured - 20.0 * ref_level.log10()
}

#[test]
fn reachable_target_hits_exactly() {
    let c = c_from_real_data();
    assert!((c - (-20.68)).abs() < 0.1, "C={c}");
    // -30 is quieter than C → reachable. The model at the computed level
    // returns the target exactly; the level lands near the on-device point
    // (0.3225 measured -30.51, so -30 needs slightly more: ~0.342).
    let target = -30.0f64;
    let level = (10f64.powf((target - c) / 20.0)).clamp(0.0, 1.0);
    assert!((level - 0.342).abs() < 0.005, "level={level}");
    let back = 20.0 * level.log10() + c;
    assert!((back - target).abs() < 1e-6, "got {back}");
}

#[test]
fn target_louder_than_max_clamps() {
    let c = c_from_real_data(); // ≈ -20.68 = max achievable
    let target = -16.0f64; // louder than the preset can reach
    let ideal = 10f64.powf((target - c) / 20.0);
    assert!(ideal > 1.0, "ideal={ideal}");
    assert_eq!((ideal as f32).clamp(0.0, 1.0), 1.0);
}

#[test]
fn solve_level_reachable_and_clamped() {
    let c = c_from_real_data();
    // Reachable target → exact, not clamped.
    let (lvl, clamped, predicted) = super::solve_level(c, -30.0);
    assert!(!clamped);
    assert!((lvl - 0.342).abs() < 0.005, "lvl={lvl}");
    assert!((predicted - (-30.0)).abs() < 1e-4, "predicted={predicted}");
    // Target louder than C → clamps at 1.0 and predicts C (the ceiling).
    let (lvl2, clamped2, predicted2) = super::solve_level(c, -16.0);
    assert!(clamped2);
    assert_eq!(lvl2, 1.0);
    assert!((predicted2 - c).abs() < 1e-9, "predicted2={predicted2}");
}

#[test]
fn redistribute_delta_is_min_of_deficit_headroom_and_downroom() {
    use super::redistribute_delta_db;
    // pl=0.5 → 6.02 dB headroom; min knob 0.5 → 20 dB down-room; deficit 3 + 1 margin = 4
    // (deficit-bound, well under headroom/down-room).
    assert!((redistribute_delta_db(0.5, 3.0, 0.5) - 4.0).abs() < 1e-9);
    // pl=0.9 → 0.915 dB headroom binds (deficit+margin=4, but no room to raise past ceiling).
    assert!((redistribute_delta_db(0.9, 3.0, 0.5) - 0.9151).abs() < 1e-3);
    // min knob 0.06 → 20·log10(0.06/0.05)=1.584 dB down-room binds.
    assert!((redistribute_delta_db(0.5, 3.0, 0.06) - 1.5836).abs() < 1e-3);
    // A knob already at/below the floor → no room → 0 (don't offer).
    assert_eq!(redistribute_delta_db(0.5, 3.0, 0.05), 0.0);
    // No clamp deficit → 0 (the margin never fires without a real deficit).
    assert_eq!(redistribute_delta_db(0.5, 0.0, 0.5), 0.0);
    // pl at ceiling (1.0) → no headroom → 0 (the quiet class PR6 owns).
    assert_eq!(redistribute_delta_db(1.0, 3.0, 0.5), 0.0);
}

// ── joint-k (parallel-merged) solve ──────────────────────────────────────
fn amp_knob(current: f32) -> super::KnobTarget {
    super::KnobTarget {
        knob: super::LevelKnob::Block {
            group_id: "G1".into(),
            node_id: "ACD_X".into(),
            parameter_id: "outputLevel".into(),
            scene_slot: Some(0),
        },
        lo: 0.0,
        hi: 1.0,
        current,
    }
}

// Single amp: joint-k degenerates to the validated one-amp solve.
#[test]
fn joint_k_single_amp_hits_target() {
    let j = super::solve_joint_k_at(&[amp_knob(0.5)], -30.0, -26.0, super::LEVEL_MIN).unwrap();
    assert!(!j.clamped);
    assert!(
        (j.achieved - (-30.0)).abs() < 1e-6,
        "achieved={}",
        j.achieved
    );
    assert!(
        (j.levels[0] - 0.3155).abs() < 0.002,
        "level={}",
        j.levels[0]
    );
}

// Two equal lanes summing to `measured`: each scaled by the same k → target hit,
// balance (equal) preserved.
#[test]
fn joint_k_two_equal_amps_scale_together() {
    let j = super::solve_joint_k_at(
        &[amp_knob(0.5), amp_knob(0.5)],
        -26.0,
        -20.0,
        super::LEVEL_MIN,
    )
    .unwrap();
    assert!(!j.clamped);
    assert!(
        (j.achieved - (-26.0)).abs() < 1e-6,
        "achieved={}",
        j.achieved
    );
    assert_eq!(j.levels.len(), 2);
    assert!(
        (j.levels[0] - j.levels[1]).abs() < 1e-6,
        "balance preserved"
    );
    assert!(
        (j.levels[0] - 0.2505).abs() < 0.002,
        "level={}",
        j.levels[0]
    );
}

// Unequal lanes, boost beyond the hottest amp's headroom → ratio-preserving clamp:
// hottest hits 1.0, the other scales by the SAME k_eff (mix intact), `clamped` set,
// `achieved` reports the shortfall (NOT the target).
#[test]
fn joint_k_unequal_clamp_preserves_ratio() {
    let j = super::solve_joint_k_at(
        &[amp_knob(0.9), amp_knob(0.3)],
        -18.0,
        -30.0,
        super::LEVEL_MIN,
    )
    .unwrap();
    assert!(j.clamped);
    assert!(
        (j.levels[0] - 1.0).abs() < 1e-4,
        "hottest pinned at 1.0: {}",
        j.levels[0]
    );
    let ratio = j.levels[0] / j.levels[1];
    assert!(
        (ratio - 3.0).abs() < 1e-3,
        "0.9:0.3 ratio preserved, got {ratio}"
    );
    assert!(
        j.achieved < -28.0,
        "achieved reports the shortfall: {}",
        j.achieved
    );
    assert!(j.achieved > -30.0);
}

// At target already → k_eff ≈ 1, not clamped. `jointk_one_scene`'s caller-side
// skip no longer keys off k_eff though — it compares `measured` vs `target_lufs`
// directly via `scene_at_target` (the KNOB_TOL_LU band), which a unity k_eff implies.
#[test]
fn joint_k_at_target_is_unity_unclamped() {
    let j = super::solve_joint_k_at(
        &[amp_knob(0.5), amp_knob(0.2)],
        -30.0,
        -30.0,
        super::LEVEL_MIN,
    )
    .unwrap();
    assert!(!j.clamped);
    assert!((j.k_eff - 1.0).abs() < 1e-6, "k_eff={}", j.k_eff);
}

// ⟦4a⟧ THE HOLD'S FLOOR (danger.md: outputLevel = 0 is deep digital silence) — the trade's
// base hold may not solve there. The floor bounds **k**, ratio-preserving, and the pin is
// REPORTED.
#[test]
fn joint_k_honours_the_base_fader_floor_on_a_scale_down() {
    use crate::headroom_trade::BASE_FADER_FLOOR;
    // A 30 dB cut asked of a 0.02 lane: only ~6 dB of it fits above the 0.01 floor.
    let j = super::solve_joint_k_at(
        &[amp_knob(0.8), amp_knob(0.02)],
        -56.0,
        -26.0,
        BASE_FADER_FLOOR,
    )
    .unwrap();
    assert!(j.clamped, "a floor pin is a clamp, not a silent success");
    assert!(
        (j.levels[1] - BASE_FADER_FLOOR).abs() < 1e-6,
        "the QUIETEST lane pins at the floor, got {}",
        j.levels[1]
    );
    let ratio = j.levels[0] / j.levels[1];
    assert!(
        (ratio - 40.0).abs() < 1e-2,
        "0.8:0.02 ratio preserved: {ratio}"
    );
    assert!(
        j.achieved > -33.0 && j.achieved < -31.0,
        "achieved reports the ~6 dB it could actually pay: {}",
        j.achieved
    );

    // LEVEL_MIN (every other lane) is byte-identical to the pre-floor behaviour.
    let free = super::solve_joint_k_at(
        &[amp_knob(0.8), amp_knob(0.02)],
        -56.0,
        -26.0,
        super::LEVEL_MIN,
    )
    .unwrap();
    assert!(!free.clamped);
    assert!((free.achieved - (-56.0)).abs() < 1e-6, "{}", free.achieved);
}

// A lane the AUTHOR muted stays muted: the floor bounds `k`, never the individual levels,
// so `0 · k` is never clamped UP to the floor (which would un-mute it — a tone change).
#[test]
fn the_fader_floor_never_unmutes_an_author_muted_lane() {
    use crate::headroom_trade::BASE_FADER_FLOOR;
    let j = super::solve_joint_k_at(
        &[amp_knob(0.8), amp_knob(0.0)],
        -32.0,
        -26.0,
        BASE_FADER_FLOOR,
    )
    .unwrap();
    assert_eq!(j.levels[1], 0.0, "the muted lane stays muted");
    assert!(!j.clamped, "and it does not veto the audible lane's cut");
    assert!((j.achieved - (-32.0)).abs() < 1e-6);
}

// ⟦8⟧ Only a DIRECTION-BLOCKING pin names a cause. A row that merely ran out of secant
// captures mid-range is clamped with no false "already maxed out" claim.
#[test]
fn only_a_direction_blocking_pin_counts_as_a_ceiling() {
    use super::PinnedBound;
    // Maxed out and the target is still louder → a real ceiling.
    assert_eq!(
        super::joint_levels_pinned(&[1.0], &[0.5], super::LEVEL_MIN, -20.0, -15.0),
        Some(PinnedBound::Max)
    );
    // Maxed out but the target is BELOW what we achieved → the search stopped early.
    assert_eq!(
        super::joint_levels_pinned(&[1.0], &[0.5], super::LEVEL_MIN, -15.0, -20.0),
        None
    );
    // Mid-range and off target in either direction → not pinned at all.
    assert_eq!(
        super::joint_levels_pinned(&[0.4], &[0.5], super::LEVEL_MIN, -20.0, -15.0),
        None
    );
    // At the trade's fader floor with the target still quieter → out of fader.
    assert_eq!(
        super::joint_levels_pinned(
            &[crate::headroom_trade::BASE_FADER_FLOOR],
            &[0.5],
            crate::headroom_trade::BASE_FADER_FLOOR,
            -15.0,
            -20.0
        ),
        Some(PinnedBound::Floor)
    );
    // An author-muted lane the solve never moved is not the solve pinning there.
    assert_eq!(
        super::joint_levels_pinned(&[0.0], &[0.0], super::LEVEL_MIN, -15.0, -20.0),
        None
    );
}

// A dB-unit knob can't be scaled multiplicatively → error, never a garbage write.
#[test]
fn joint_k_rejects_db_knob() {
    let mut kt = amp_knob(0.5);
    kt.lo = -18.0;
    kt.hi = 6.0;
    assert!(super::solve_joint_k_at(&[kt], -30.0, -26.0, super::LEVEL_MIN).is_err());
}

// Rebalance: equal-ceiling lanes both sit at 1.0; a louder lane is attenuated to match
// the quieter (which pins at 1.0), and both stay ≤ 1.0 — equal SOLO loudness.
#[test]
fn balanced_solo_levels_equalizes_lanes() {
    let (la, lb) = super::balanced_solo_levels(-20.0, -20.0);
    assert!(
        (la - 1.0).abs() < 1e-6 && (lb - 1.0).abs() < 1e-6,
        "equal → both 1.0"
    );

    // A louder (C=-15) than B (C=-21): B pins at 1.0, A attenuates to 10^(-6/20)≈0.501.
    let (la, lb) = super::balanced_solo_levels(-15.0, -21.0);
    assert!((lb - 1.0).abs() < 1e-6, "quieter lane B at 1.0, got {lb}");
    assert!(
        (la - 0.501).abs() < 0.005,
        "louder lane A attenuated, got {la}"
    );
    // Equal solo loudness check: 20·log10(la)+C_a ≈ 20·log10(lb)+C_b.
    let solo_a = 20.0 * (la as f64).log10() + (-15.0);
    let solo_b = 20.0 * (lb as f64).log10() + (-21.0);
    assert!(
        (solo_a - solo_b).abs() < 0.05,
        "solo loudness equal: {solo_a} vs {solo_b}"
    );
}

#[test]
fn secant_next_solves_linear_response() {
    // A perfectly linear knob: lufs = 10*x - 25. Target -20 ⇒ x = 0.5.
    let f = |x: f32| 10.0 * x as f64 - 25.0;
    let x = secant_step(
        (0.2f32, f(0.2)),
        (0.8f32, f(0.8)),
        -20.0,
        Flatness::Knob,
        None,
    )
    .unwrap();
    assert!((x - 0.5).abs() < 1e-4, "x={x}");
}

#[test]
fn secant_next_none_on_flat_response() {
    // A knob that doesn't move loudness → no solution.
    assert!(secant_step(
        (0.2f32, -20.0),
        (0.8f32, -20.0),
        -18.0,
        Flatness::Knob,
        None
    )
    .is_none());
}

// Item 3 — no-authority verdict: a LARGE applied gain that barely moves loudness means
// the amp is off-branch; a small gain is inconclusive (headroom clamp), and a real
// response means the amp has authority.
#[test]
fn no_authority_flags_dead_knob_only() {
    assert!(
        super::no_authority(12.0, 0.10),
        "big boost, no response → off-branch"
    );
    assert!(
        super::no_authority(-9.0, -0.05),
        "big cut, no response → off-branch"
    );
    assert!(
        !super::no_authority(12.0, 6.0),
        "big gain, real response → has authority"
    );
    assert!(
        !super::no_authority(2.0, 0.05),
        "small gain → inconclusive (headroom clamp)"
    );
    assert!(
        !super::no_authority(0.0, 0.0),
        "no gain applied → not no-authority"
    );
}

// Item 1 — the secant step is trust-region-clamped: a shallow (but > 0.05) slope with a
// big residual must NOT explode the Newton jump; it caps at ±BATCH_TRUST_DB.
#[test]
fn secant_next_db_trust_region_caps_jump() {
    let prev = (0.0, -30.0);
    let last = (1.0, -29.7); // slope 0.3 over 1 dB; raw jump ≈ +32 dB
    let next = secant_step(
        prev,
        last,
        -20.0,
        Flatness::AppliedDb,
        Some(BATCH_TRUST_DB as f64),
    )
    .unwrap();
    assert!(
        (next - (last.0 + super::BATCH_TRUST_DB as f64)).abs() < 1e-9,
        "step clamped to +{} dB, got {next}",
        super::BATCH_TRUST_DB
    );
}

#[test]
fn secant_next_db_none_on_flat_slope() {
    // slope ≈ 0.0017 ≤ 0.05 → no usable response (→ the loop stops / no-authority path).
    assert!(secant_step(
        (0.0, -30.0),
        (6.0, -29.99),
        -20.0,
        Flatness::AppliedDb,
        Some(BATCH_TRUST_DB as f64)
    )
    .is_none());
}

// `correct_iter_core` harness: `model` answers each VERIFIED write (None = capture
// dropped); every write is recorded. Asserts the invariant every case must hold: the
// device's final state (last write, else `levels0`) is exactly `Correction.levels` —
// the deferred save persists the device, not what was reported.
type Writes = Vec<(Vec<f32>, bool)>;
fn run_core(
    base: &[f32],
    levels0: &[f32],
    (measured0, v0, target): (f64, f64, f64),
    mut model: impl FnMut(&[f32]) -> Option<f64>,
) -> (super::Correction, Writes) {
    let mut writes: Writes = Vec::new();
    let c = super::correct_iter_core(
        base,
        levels0.to_vec(),
        measured0,
        v0,
        target,
        super::LEVEL_MIN,
        |lv, verify| {
            writes.push((lv.to_vec(), verify));
            Ok(if verify { model(lv) } else { None })
        },
    )
    .unwrap();
    let device = writes.last().map_or(levels0, |w| w.0.as_slice());
    assert_eq!(
        device,
        c.levels.as_slice(),
        "device state must equal the returned levels"
    );
    (c, writes)
}

fn gain(levels: &[f32], base: &[f32]) -> f64 {
    let lv = levels.iter().cloned().fold(0.0_f32, f32::max) as f64;
    let b = base.iter().cloned().fold(0.0_f32, f32::max) as f64;
    20.0 * (lv / b).log10()
}

// #186: a dropped probe capture leaves the probe levels on the device; a later flat-slope
// stop must not report levels0 while the device holds the probe.
#[test]
fn correct_iter_dropped_probe_capture_keeps_device_honest() {
    let base = [0.2_f32];
    let levels0 = [0.2 * 10f32.powf(4.0 / 20.0)];
    let (c, writes) = run_core(&base, &levels0, (-30.0, -30.1, -26.0), |_| None);
    assert!(writes.iter().any(|w| w.1), "the probe must have fired");
    assert!(c.clamp_reason.is_none());
}

// #189: an adverse first response must not beat the original base point.
#[test]
fn correct_iter_adverse_first_response_falls_back_to_base() {
    let base = [0.1_f32];
    let levels0 = [0.1 * 10f32.powf(4.0 / 20.0)];
    let (c, writes) = run_core(&base, &levels0, (-30.0, -35.0, -26.0), |_| None);
    assert_eq!(c.levels, base.to_vec(), "base@-30 beats levels0@-35");
    assert!((c.lufs + 30.0).abs() < 1e-9, "lufs {}", c.lufs);
    assert_eq!(writes.last().unwrap().0, base.to_vec());
}

// At a low output level, levels a few dB apart differ by < 1e-3 in absolute terms:
// the restore to base must still be written, not skipped as "already held".
#[test]
fn correct_iter_restores_base_at_low_output_level() {
    let base = [0.0011_f32];
    let levels0 = [0.0011 * 10f32.powf(4.0 / 20.0)];
    let (c, writes) = run_core(&base, &levels0, (-30.0, -35.0, -26.0), |_| None);
    assert_eq!(c.levels, base.to_vec());
    assert_eq!(writes.last().unwrap().0, base.to_vec());
}

// #189b: a measured probe point closer to target than levels0/base can win.
#[test]
fn correct_iter_probe_point_can_be_best() {
    let base = [0.1_f32];
    let levels0 = [0.1 * 10f32.powf(4.0 / 20.0)];
    let probe_lv = 0.1 * 10f32.powf(-2.0 / 20.0);
    let (c, _) = run_core(&base, &levels0, (-30.0, -30.1, -29.0), |lv| {
        Some(if (lv[0] - probe_lv).abs() < 1e-6 {
            -29.0
        } else {
            -30.1
        })
    });
    assert!((c.lufs + 29.0).abs() < 1e-9, "lufs {}", c.lufs);
    assert!((c.levels[0] - probe_lv).abs() < 1e-6);
}

// #188: a muted lane 0 must not zero the shared gain (and fake a no-authority verdict).
#[test]
fn correct_iter_muted_lane_zero_still_corrects() {
    for (base, levels0) in [
        ([0.0_f32, 0.5], [0.0_f32, 0.7]),
        ([0.5_f32, 0.0], [0.7_f32, 0.0]),
    ] {
        let v0 = -30.0 + gain(&levels0, &base);
        let (c, writes) = run_core(&base, &levels0, (-30.0, v0, -26.0), |lv| {
            Some(-30.0 + gain(lv, &base))
        });
        assert!(
            !writes.is_empty(),
            "a correction write must happen: {base:?}"
        );
        assert!(c.clamp_reason.is_none(), "no false verdict: {base:?}");
        assert!(
            (c.lufs + 26.0).abs() <= super::KNOB_TOL_LU,
            "lufs {}",
            c.lufs
        );
    }
}

// #188b: every lane muted → nothing to scale: keep levels0, no writes, no verdict.
#[test]
fn correct_iter_all_lanes_muted_is_a_noop() {
    let (c, writes) = run_core(&[0.0, 0.0], &[0.0, 0.0], (-30.0, -30.0, -26.0), |_| None);
    assert!(writes.is_empty());
    assert!(c.clamp_reason.is_none());
    assert_eq!(c.levels, vec![0.0, 0.0]);
}

// The secant converges on a saturating (compressor-like) response through the real
// loop (`correct_iter_core`), not a mirror of it.
#[test]
fn correct_iter_core_converges_on_compressor() {
    let (l0, g, tau, target) = (-30.0_f64, 15.0_f64, 8.0_f64, -22.0_f64);
    let base = [0.1_f32];
    let model = |lv: &[f32]| l0 + g * (1.0 - (-gain(lv, &base) / tau).exp());
    let levels0 = [0.1 * 10f32.powf(((target - l0) / 20.0) as f32)];
    let v0 = model(&levels0);
    let (c, writes) = run_core(&base, &levels0, (model(&base), v0, target), |lv| {
        Some(model(lv))
    });
    assert!(
        (c.lufs - target).abs() <= super::KNOB_TOL_LU,
        "lufs {}",
        c.lufs
    );
    let steps: Vec<f64> = writes.iter().map(|w| gain(&w.0, &base)).collect();
    assert!(
        steps.len() as u32 <= super::MEASURE_CORRECT_MAX,
        "steps={steps:?}"
    );
    let mut prev_db = gain(&levels0, &base);
    for db in steps {
        assert!(
            (db - prev_db).abs() <= super::BATCH_TRUST_DB as f64 + 1e-6,
            "trust region: {prev_db} → {db}"
        );
        prev_db = db;
    }
}

// Drive the secant loop against a synthetic dB-of-amplitude knob, searching
// in log-of-knob coordinate (c = 20·log10(x)) exactly as `level_preset_block`
// does. In that space the knob is linear, so it converges in one secant step.
#[test]
fn secant_loop_converges_on_log_knob() {
    // captured_LUFS = 20*log10(x) + C, x in (0,1], C = -10 (amp outputLevel).
    let model = |x: f32| 20.0 * (x.max(1e-4) as f64).log10() - 10.0;
    let target = -24.0f64;
    let (lo, hi) = (0.0f32, 1.0f32);
    let eps = 1e-3f32;
    let to_c = |x: f32| 20.0 * x.max(eps).log10();
    let from_c = |c: f32| 10f32.powf(c / 20.0).clamp(lo, hi);
    let (c_lo, c_hi) = (to_c(lo.max(eps)), to_c(hi));
    let span = c_hi - c_lo;
    let (mut ca, mut cb) = (c_lo + 0.4 * span, c_lo + 0.75 * span);
    let (mut ya, mut yb) = (model(from_c(ca)), model(from_c(cb)));
    let mut best = if (ya - target).abs() <= (yb - target).abs() {
        (ca, ya)
    } else {
        (cb, yb)
    };
    let mut iters = 2;
    while iters < super::KNOB_MAX_ITERS && (best.1 - target).abs() > super::KNOB_TOL_LU {
        let Some(nc) = secant_step((ca, ya), (cb, yb), target, Flatness::Knob, None) else {
            break;
        };
        let nc = nc.clamp(c_lo, c_hi);
        if (nc - cb).abs() < 1e-4 {
            break;
        }
        let ny = model(from_c(nc));
        iters += 1;
        if (ny - target).abs() < (best.1 - target).abs() {
            best = (nc, ny);
        }
        (ca, ya, cb, yb) = (cb, yb, nc, ny);
    }
    assert!(
        (best.1 - target).abs() <= super::KNOB_TOL_LU,
        "converged to lufs {} for target {target}",
        best.1
    );
    let final_x = from_c(best.0);
    assert!((final_x - 0.1995).abs() < 0.02, "final knob {final_x}"); // 10^((-24+10)/20)
    assert!(
        iters <= 3,
        "should converge fast in log space, iters={iters}"
    );
}

/// The setlist common target is min(C) − headroom; presets whose C equals the
/// floor land below the target (reachable), so none clamp.
#[test]
fn setlist_common_target_is_below_min_c() {
    // Three presets with different ceilings.
    let cs = [-20.68f64, -24.0, -22.5];
    let headroom = 1.0;
    let min_c = cs.iter().cloned().fold(f64::INFINITY, f64::min);
    let target = min_c - headroom;
    assert!((target - (-25.0)).abs() < 1e-9, "target={target}");
    // Every preset can reach the common target (level ≤ 1.0, not clamped).
    for &c in &cs {
        let (lvl, clamped, _) = super::solve_level(c, target);
        assert!(!clamped, "C={c} unexpectedly clamped at target {target}");
        assert!(lvl <= 1.0 && lvl > 0.0, "lvl={lvl}");
    }
}

// scene_at_target mirrors the correction loop's ACCEPT_TOL_LU acceptance band —
// a re-run must not rewrite an already-in-tolerance scene.
#[test]
fn scene_at_target_accepts_within_the_band() {
    assert!(
        super::scene_at_target(-22.0, -22.24, false),
        "0.24 LU off, unclamped"
    );
}

#[test]
fn scene_at_target_rejects_just_outside_the_band() {
    assert!(!super::scene_at_target(-22.0, -22.26, false), "0.26 LU off");
}

#[test]
fn scene_at_target_rejects_when_clamped_even_at_zero_delta() {
    assert!(
        !super::scene_at_target(-22.0, -22.0, true),
        "clamped must still report"
    );
}

// switch_at_target is the footswitch mirror of scene_at_target — the re-run
// idempotency band (the PR #74 follow-up gap). The same ACCEPT_TOL_LU band as scenes:
// "on target" means one thing in every lane (#236).
#[test]
fn switch_at_target_accepts_within_the_band() {
    assert!(
        super::switch_at_target(-24.0, -24.24, false),
        "0.24 LU off, unclamped → skip the re-solve"
    );
}

#[test]
fn switch_at_target_rejects_just_outside_the_band() {
    assert!(
        !super::switch_at_target(-24.0, -24.26, false),
        "0.26 LU off → must re-level"
    );
}

// level_unchanged: LU-space ratio tolerance matching KNOB_TOL_LU, guards the
// Base-leveling idempotency skip against re-writing an in-tolerance presetLevel.
#[test]
fn level_unchanged_true_on_identical_levels() {
    assert!(super::level_unchanged(0.5160, 0.5160));
}

#[test]
fn level_unchanged_true_within_knob_tol() {
    // 20*log10(0.5160/0.5300) ≈ -0.23 LU
    assert!(super::level_unchanged(0.5160, 0.5300));
}

#[test]
fn level_unchanged_false_beyond_knob_tol() {
    // 20*log10(0.5160/0.55) ≈ -0.55 LU
    assert!(!super::level_unchanged(0.5160, 0.55));
}

#[test]
fn level_unchanged_false_on_zero_previous() {
    assert!(!super::level_unchanged(0.5, 0.0));
}

#[test]
fn level_unchanged_false_on_negative_previous() {
    assert!(!super::level_unchanged(0.5, -1.0));
}

// BUG→GATE (the boosted-preset re-run coin flip). After a BOOST run `presetLevel` sits at
// LEVEL_MAX with C solved onto target, so run 2's C round-trips through f32 storage + f64
// log10 a few float units PAST exact (observed offline: -23.000000488 against -23.0) and
// `solve_level` reports `clamped` for a target already reached. `already_on_target` is the
// band that must absorb it — a skip requiring `!clamped` would re-persist the identical
// presetLevel and surface a spurious `clamped` row.
#[test]
fn a_hairline_float_overshoot_still_counts_as_on_target() {
    let c = -23.000_000_488_f64;
    // The flip this gate exists for: the solve DOES clamp on this input.
    let (lvl, clamped, _) = super::solve_level(c, -23.0);
    assert!(clamped, "the hairline overshoot must still clamp the solve");
    assert_eq!(lvl, super::LEVEL_MAX);
    // ...and the band must call it on target anyway.
    assert!(super::already_on_target(c, 1.0, -23.0));
}

// The honest clamp this band must NOT weaken: 3 LU short at presetLevel max is genuinely
// unreachable, so the skip must not fire and the run must go on to report today's clamp.
#[test]
fn a_genuinely_unreachable_target_is_not_on_target() {
    assert!(!super::already_on_target(-26.0, 1.0, -23.0));
}

// Re-running the SAME preset at a DIFFERENT target must never skip — the saved level is on
// the old target, not this one.
#[test]
fn a_changed_target_is_not_on_target() {
    let c = -23.0;
    assert!(super::already_on_target(c, 1.0, -23.0));
    assert!(!super::already_on_target(c, 1.0, -25.0));
}

// The band's edges, in the same LU space `ACCEPT_TOL_LU` is stated in.
#[test]
fn already_on_target_accepts_inside_and_rejects_outside_the_band() {
    let c = -23.0;
    let inside = super::ACCEPT_TOL_LU - 0.01;
    let outside = super::ACCEPT_TOL_LU + 0.01;
    assert!(super::already_on_target(c, 1.0, -23.0 + inside));
    assert!(super::already_on_target(c, 1.0, -23.0 - inside));
    assert!(!super::already_on_target(c, 1.0, -23.0 + outside));
    assert!(!super::already_on_target(c, 1.0, -23.0 - outside));
}

// The band is PINNED at ACCEPT_TOL_LU, not at the flatness threshold KNOB_TOL_LU (0.3).
// This is the only offline check that can fail on a silent re-widening: the SimDevice's
// residual for the boosted fixture is float-round-trip small (~1e-7 LU), so E8 passes at
// either value and cannot tell them apart. A 0.28 LU shortfall at `presetLevel`'s ceiling
// must fall through, write and report `clamped`, never be absorbed as `done` with no
// re-level affordance (`SummaryPage`'s clamped bucket).
#[test]
fn a_shortfall_above_the_band_is_not_absorbed_even_though_knob_tol_would_have() {
    let (_lvl, clamped, _predicted) = super::solve_level(-23.28, -23.0);
    assert!(
        clamped,
        "0.28 LU short at the ceiling must still clamp the solve"
    );
    assert!(!super::already_on_target(-23.28, 1.0, -23.0));
    // The residual straddles the two constants — that IS the pin. If the band were ever
    // widened back to `KNOB_TOL_LU` this shortfall would be absorbed as `done` again.
    let residual = (-23.0f64 - super::lufs_at_level(-23.28, 1.0)).abs();
    assert!(
        residual > super::ACCEPT_TOL_LU,
        "{residual} must exceed the band"
    );
    assert!(
        residual <= super::KNOB_TOL_LU,
        "{residual} must sit INSIDE the scene band, or this test pins nothing"
    );
}

// The extracted identity IS what `solve_level` predicts — one definition, four callers
// (the solver, the boost planner, `apply_base_boost`, and the band above).
#[test]
fn lufs_at_level_is_solve_levels_own_predicted() {
    let c = c_from_real_data();
    let (lvl, clamped, predicted) = super::solve_level(c, -30.0);
    assert!(!clamped);
    assert_eq!(super::lufs_at_level(c, lvl), predicted);
}

// (A3) A base block knob write must recall base explicitly — a preset loads
// into its saved lastLoadedScene, not necessarily base (HW), so a bare write
// with no recall would silently land wherever that saved scene left it. This
// is the reported bug's exact shape: preset 28 (the e2e `E2E Hiwatt 3S` fixture)
// has `lastLoadedScene = 3`, and scene 3 is literally named "Base Scene" —
// the naming collision that made the symptom read as "leveling wrote into
// the base preset" when it actually wrote into scene 3's overlay. The
// engine only ever addresses scenes by numeric slot (no name field exists
// at this layer), so the fix is immune to the name regardless.
#[test]
fn set_knob_base_block_recalls_base_explicitly() {
    let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(30)).expect("load_preset"); // activates saved scene 3, not base
    let knob = LevelKnob::Block {
        group_id: "G1".into(),
        node_id: "amp".into(),
        parameter_id: "outputLevel".into(),
        scene_slot: None,
    };
    set_knob(&mut s, &knob, 0.6, None).expect("set_knob");
    let ev = sim.events();
    assert!(
        ev.iter().any(|e| matches!(
            e,
            crate::sim_device::SimEvent::ChangeParameter {
                scene: crate::sim_device::SCENE_BASE,
                param,
                ..
            } if param == "outputLevel"
        )),
        "a base block knob must write scene_base, not the leftover saved scene 3: {ev:?}"
    );
}

// Same fix, batched path: a base-only `set_knobs` target set must ALSO recall
// base explicitly, not rely on the connection's default scene.
#[test]
fn set_knobs_refuses_a_batch_that_mixes_scenes() {
    // Only the FIRST scene found is recalled, so a mixed batch would land every
    // write in that scene's overlay — silently, each write confirming normally.
    let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(30)).expect("load_preset");
    let mk = |scene: u32| LevelKnob::Block {
        group_id: "G1".into(),
        node_id: "amp".into(),
        parameter_id: "outputLevel".into(),
        scene_slot: Some(scene),
    };
    let (a, b) = (mk(1), mk(2));
    let err = set_knobs(&mut s, &[(&a, 0.6), (&b, 0.4)], None)
        .expect_err("a batch mixing scenes 1 and 2 must be refused, not silently merged");
    assert!(
        err.to_string().contains("mixes scenes"),
        "error should name the mixed-scene cause: {err}"
    );
    assert!(
        !sim.events()
            .iter()
            .any(|e| matches!(e, crate::sim_device::SimEvent::ChangeParameter { .. })),
        "nothing may be written when the batch is refused: {:?}",
        sim.events()
    );
}

#[test]
fn set_knobs_refuses_a_batch_mixing_base_and_scene() {
    // The scene branch recalls the scene whenever ANY scene target is present, so a
    // base-scoped knob riding along would be written under that overlay, not base.
    let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(30)).expect("load_preset");
    let mk = |scene: Option<u32>| LevelKnob::Block {
        group_id: "G1".into(),
        node_id: "amp".into(),
        parameter_id: "outputLevel".into(),
        scene_slot: scene,
    };
    let (base, scened) = (mk(None), mk(Some(2)));
    let err = set_knobs(&mut s, &[(&base, 0.6), (&scened, 0.4)], None)
        .expect_err("a batch mixing a base target with a scene target must be refused");
    assert!(
        err.to_string().contains("mixes base and scene"),
        "error should name the base/scene mix: {err}"
    );
    assert!(
        !sim.events()
            .iter()
            .any(|e| matches!(e, crate::sim_device::SimEvent::ChangeParameter { .. })),
        "nothing may be written when the batch is refused: {:?}",
        sim.events()
    );
}

#[test]
fn set_knobs_base_only_batch_recalls_base_explicitly() {
    let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(30)).expect("load_preset");
    let knob = LevelKnob::Block {
        group_id: "G1".into(),
        node_id: "amp".into(),
        parameter_id: "outputLevel".into(),
        scene_slot: None,
    };
    set_knobs(&mut s, &[(&knob, 0.6)], None).expect("set_knobs");
    let ev = sim.events();
    assert!(
        ev.iter().any(|e| matches!(
            e,
            crate::sim_device::SimEvent::ChangeParameter {
                scene: crate::sim_device::SCENE_BASE,
                param,
                ..
            } if param == "outputLevel"
        )),
        "a base-only set_knobs batch must write scene_base: {ev:?}"
    );
}

// A PresetLevel-only batch gets NO recall — setPresetLevel is a global
// multiplier, not a scene-scoped changeParameter, so a recall would only risk
// reverting an unsaved presetLevel write for no benefit.
#[test]
fn set_knobs_preset_level_only_gets_no_recall() {
    let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(30)).expect("load_preset");
    set_knobs(&mut s, &[(&LevelKnob::PresetLevel, 0.5)], None).expect("set_knobs");
    let ev = sim.events();
    assert!(
        !ev.iter()
            .any(|e| matches!(e, crate::sim_device::SimEvent::LoadScene(_))),
        "a PresetLevel-only batch must not recall any scene: {ev:?}"
    );
}

// The pre-save recall reverts an UNSAVED `presetLevel` to the saved value (the
// load-level-apply gotcha), so every recalling save must replay it between the
// recall and the save — the exact op order is the invariant (HW: a solved
// 0.3096 "[SAVED]" persisted the prior 0.32 until this re-assert existed;
// caught live by the online level.online.spec.ts base idempotency test).
#[test]
fn recall_reassert_save_replays_the_unsaved_level_after_the_recall() {
    let sim = crate::sim_device::SimDevice::new()
        .with_preset_json(crate::sim_device::SCENED_PRESET_JSON)
        .with_saved_scene(30, Some(3));
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(30)).expect("load_preset");
    recall_reassert_save(
        &mut s,
        ListIndex(30),
        Some(3),
        &[Reassert::PresetLevel(0.42)],
    )
    .expect("save");
    let tail: Vec<String> = sim
        .events()
        .iter()
        .filter_map(|e| match e {
            crate::sim_device::SimEvent::LoadScene(n) => Some(format!("scene{n}")),
            crate::sim_device::SimEvent::PresetLevel(v) => Some(format!("level{v}")),
            crate::sim_device::SimEvent::Saved(n) => Some(format!("save{n}")),
            _ => None,
        })
        .collect();
    assert_eq!(
        tail,
        vec!["scene3", "level0.42", "save30"],
        "order must be recall → re-assert → save"
    );
}

// Without a recall there is nothing to revert — the re-assert must NOT fire.
#[test]
fn recall_reassert_save_skips_the_reassert_without_a_recall() {
    let sim = crate::sim_device::SimDevice::new();
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(30)).expect("load_preset");
    recall_reassert_save(&mut s, ListIndex(30), None, &[Reassert::PresetLevel(0.42)])
        .expect("save");
    let ev = sim.events();
    assert!(
        !ev.iter()
            .any(|e| matches!(e, crate::sim_device::SimEvent::PresetLevel(_))),
        "no recall → no re-assert: {ev:?}"
    );
    assert!(
        ev.iter()
            .any(|e| matches!(e, crate::sim_device::SimEvent::Saved(30))),
        "the save itself must still land: {ev:?}"
    );
}

/// Saved (field-8) preset for the Scene Edit tests: one amp node in G1, base
/// `outputLevel`/`gain`, and — when `overlay` — a scene-0 overlay for that node.
fn saved_with_overlay(overlay: bool) -> serde_json::Value {
    let mut p = serde_json::json!({
        "lastLoadedScene": 3,
        "audioGraph": { "template": "gtrSeries", "guitarNodes": { "G1": [
            { "nodeId": "amp", "FenderId": "ACD_Twin57",
              "dspUnitParameters": { "bypass": false, "outputLevel": 0.5, "gain": 0.7 } }
        ] } },
        "scenes": [{ "sceneName": "Lead" }]
    });
    if overlay {
        p["scenes"][0]["guitarNodes"] = serde_json::json!({
            "G1": { "amp": { "dspUnitParameters": { "outputLevel": 0.4, "gain": 0.3 } } }
        });
    }
    p
}

fn scene_knob(param: &str) -> LevelKnob {
    LevelKnob::Block {
        group_id: "G1".into(),
        node_id: "amp".into(),
        parameter_id: param.into(),
        scene_slot: Some(0),
    }
}

// B1: `SetNodeSceneEdit(node, true)` RESEEDS the node's scene overlay from base (HW
// 3-cell matrix, `probe_api/slot_write.rs`), so a scene that ALREADY has an overlay for
// the node must be written with the enable DROPPED — the write lands on the overlay
// regardless, and the enable would wipe the scene's other stored params (the reported
// corruption: scene tone params reseeded to base while the leveled value survived).
#[test]
fn set_knobs_omits_the_scene_edit_enable_when_the_overlay_exists() {
    let sim = crate::sim_device::SimDevice::new();
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(0)).expect("load_preset");
    // Base gain 0.7; scene 0 already carries its own gain 0.3 (what the reseed wipes).
    s.load_scene(crate::session::BASE_SCENE_SLOT).expect("base");
    s.change_parameter("G1", "amp", "gain", 0.7)
        .expect("base gain");
    s.load_scene(0).expect("scene 0");
    s.set_node_scene_edit("G1", "amp", true)
        .expect("seed overlay");
    s.change_parameter("G1", "amp", "gain", 0.3)
        .expect("scene gain");

    let knob = scene_knob("outputLevel");
    let saved = saved_with_overlay(true);
    set_knobs(&mut s, &[(&knob, 0.9)], Some(&saved)).expect("set_knobs");

    let enables = sim
        .events()
        .iter()
        .skip_while(|e| !matches!(e, crate::sim_device::SimEvent::ChangeParameter { param, value, .. } if param == "gain" && *value == 0.3))
        .filter(|e| matches!(e, crate::sim_device::SimEvent::SceneEdit { enable: true, .. }))
        .count();
    assert_eq!(
        enables,
        0,
        "an existing overlay must be written WITHOUT the reseeding enable: {:?}",
        sim.events()
    );
    assert_eq!(
        sim.param_write(0, "G1", "amp", "gain"),
        Some(0.3),
        "the scene's own gain must survive the leveling write, not be reseeded to base's 0.7"
    );
}

// The other HW branch: no overlay for the node in that scene → the enable is what
// MATERIALISES the overlay, so without it the write LEAKS TO BASE. It must be sent.
#[test]
fn set_knobs_enables_scene_edit_when_the_overlay_is_absent() {
    let sim = crate::sim_device::SimDevice::new();
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(0)).expect("load_preset");
    let knob = scene_knob("outputLevel");
    let saved = saved_with_overlay(false);
    set_knobs(&mut s, &[(&knob, 0.9)], Some(&saved)).expect("set_knobs");
    assert!(
        sim.events().iter().any(|e| matches!(
            e,
            crate::sim_device::SimEvent::SceneEdit { group, node, enable: true }
                if group == "G1" && node == "amp"
        )),
        "an absent overlay needs the enable to materialise it: {:?}",
        sim.events()
    );
}

// Neither write shape is safe when overlay presence is UNKNOWN (truncated field-8 read)
// or no saved doc was read at all — both mistakes corrupt the preset, so refuse before
// touching the device rather than guess.
#[test]
fn set_knobs_refuses_a_scene_write_without_known_overlay_presence() {
    for (label, saved) in [
        ("no saved doc", None),
        // `scenes` truncated away → SceneOverlay::Unknown.
        (
            "truncated read",
            Some(serde_json::json!({ "audioGraph": { "guitarNodes": { "G1": [
            { "nodeId": "amp", "FenderId": "ACD_Twin57", "dspUnitParameters": {} }] } } })),
        ),
    ] {
        let sim = crate::sim_device::SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(ListIndex(0)).expect("load_preset");
        let knob = scene_knob("outputLevel");
        let err = set_knobs(&mut s, &[(&knob, 0.9)], saved.as_ref())
            .expect_err("a scene write with unknown overlay presence must be refused");
        assert!(
            err.to_string().contains("overlay"),
            "{label}: the error must name the overlay-presence cause: {err}"
        );
        assert!(
            !sim.events().iter().any(|e| matches!(
                e,
                crate::sim_device::SimEvent::ChangeParameter { .. }
                    | crate::sim_device::SimEvent::SceneEdit { .. }
            )),
            "{label}: nothing may be written when the batch is refused: {:?}",
            sim.events()
        );
    }
}

// The BYPASS-ONLY branch (HW-verified fw 1.8.45): the node's Scene Edit flag is OFF, so
// the scene carries only the bypass family and SHARES the node's knobs with base. Both
// write shapes are wrong — the enable reseeds, the enable-dropped write lands on BASE and
// changes every sharing scene (measured: base gain 2.5 → 7.0, the bypass-only overlay
// unchanged). Refuse, with the SHARING named so the user can act on it, and touch nothing.
#[test]
fn set_knobs_refuses_a_scene_write_on_a_bypass_only_overlay() {
    for (label, overlay) in [
        ("bypass only", serde_json::json!({ "bypass": false })),
        (
            "bypass + bypassType",
            serde_json::json!({ "bypass": false, "bypassType": "Post" }),
        ),
    ] {
        let sim = crate::sim_device::SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(ListIndex(0)).expect("load_preset");
        let mut saved = saved_with_overlay(true);
        saved["scenes"][0]["guitarNodes"]["G1"]["amp"] =
            serde_json::json!({ "dspUnitParameters": overlay });
        let knob = scene_knob("outputLevel");
        let err = set_knobs(&mut s, &[(&knob, 0.9)], Some(&saved))
            .expect_err("a bypass-only overlay must refuse the scene-scoped write");
        assert!(
            err.to_string().contains("shares") && err.to_string().contains("Base"),
            "{label}: the error must name the sharing and point at Base: {err}"
        );
        assert!(
            !sim.events().iter().any(|e| matches!(
                e,
                crate::sim_device::SimEvent::ChangeParameter { .. }
                    | crate::sim_device::SimEvent::SceneEdit { .. }
            )),
            "{label}: nothing may be written when the batch is refused: {:?}",
            sim.events()
        );
    }
}

/// The bounds of an ordinary `[0,1]` control — what the FS solve assumed unconditionally
/// before the param-class split, so every pre-existing solve test keeps its exact
/// arithmetic by passing it.
const UNIT_BOUNDS: (f32, f32) = (0.0, 1.0);

/// A plain `level_linear` `[0,1]` solve target — the shape every legacy FS solve test
/// exercised implicitly. `authored` is irrelevant outside the WetMix floor, so 0.5.
fn fs_unit_param() -> FsParamTarget {
    let p = FsParamTarget::new("ACD_SomeBlock", "level", 0.5);
    assert_eq!(p.info.class, crate::param_class::ParamClass::LevelLinear);
    assert_eq!(p.bounds(), UNIT_BOUNDS);
    p
}

/// A synthetic loudness reading for the injected-capture footswitch tests.
fn fs_loud(integrated: f64) -> lufs::Loudness {
    lufs::Loudness {
        integrated_lufs: integrated,
        short_term_max_lufs: integrated + 2.0,
        true_peak_dbtp: -1.0,
        momentary_spread_lu: 6.0,
        clipped: false,
    }
}

/// FS isolation must be re-sent on EVERY capture, not just the first: each capture
/// recalls a scene (base included — `arm_measurement`) and a recall RE-ASSERTS that
/// scene's own bypass state, so a once-only forced write leaves captures 2..N measuring
/// a NON-isolated sound (the sibling block-acting switch audible again). Two switches:
/// the sibling's forced bypass must appear in every call's list.
///
/// The injected capture keeps this off the device paths that open a real session
/// (`reamp_off`): a plain solve with `current_value: None` and live readings never
/// reaches them.
#[test]
fn solve_footswitch_resends_the_isolation_list_on_every_capture() {
    let iso = vec![("G1".to_string(), "sibling".to_string(), true)];
    let seen: std::cell::RefCell<Vec<Vec<(String, String, bool)>>> =
        std::cell::RefCell::new(Vec::new());
    // Response with real slope so the secant keeps iterating (target never reached).
    let r = solve_footswitch(
        2,
        &iso,
        &[],
        -20.0,
        "baked",
        None,
        &fs_unit_param(),
        |byp, v| {
            seen.borrow_mut().push(byp.to_vec());
            Ok(fs_loud(-40.0 + 10.0 * f64::from(v)))
        },
    )
    .expect("solve");
    let calls = seen.borrow();
    assert!(
        calls.len() >= 3,
        "expected a multi-capture solve: {calls:?}"
    );
    assert!(
        calls.iter().all(|got| got == &iso),
        "every capture must carry the full isolation list, not just the first: {calls:?}"
    );
    assert_eq!(r.switch, 2);
}

/// Monotone piecewise-linear interpolant over measured `(knob, LUFS)` anchors — the
/// shared body of every HW-captured response-curve fixture below.
fn piecewise_lufs(anchors: &[(f64, f64)], v: f32) -> f64 {
    let v = f64::from(v).clamp(0.0, 1.0);
    for w in anchors.windows(2) {
        let ((v0, l0), (v1, l1)) = (w[0], w[1]);
        if v <= v1 {
            return l0 + (v - v0) / (v1 - v0) * (l1 - l0);
        }
    }
    anchors[anchors.len() - 1].1
}

/// The HW curve that beat the plain secant (Hiwatt fs12, UniVibe `volume` —
/// measured points from the strict-harness post-mortem, 3/3 reproducible):
/// a steep audio-taper cliff (~90 LU/knob-unit) under a shallow top (~10),
/// modeled as the monotone piecewise-linear interpolant of the captures.
fn univibe_volume_curve(v: f32) -> f64 {
    const ANCHORS: [(f64, f64); 8] = [
        (0.0, -80.0),
        (0.25, -45.6),
        (0.3207, -38.0),
        (0.45, -32.0),
        (0.6146, -16.94),
        (0.6758, -16.2),
        (0.75, -15.5),
        (1.0, -14.5),
    ];
    piecewise_lufs(&ANCHORS, v)
}

/// On the UniVibe cliff the plain sliding secant provably stops ~3 LU hot
/// (it did on HW, and the strict e2e re-measure caught the miss); the
/// bracket-aware loop must converge inside `KNOB_TOL_LU` — the seed pair
/// already straddles the target, so Illinois-damped bracket retention owns
/// the endgame instead of the slide.
#[test]
fn solve_footswitch_bracket_mode_converges_on_the_univibe_cliff() {
    let mut captures = 0u32;
    let r = solve_footswitch(
        12,
        &[],
        &[],
        -20.0,
        "baked",
        None,
        &fs_unit_param(),
        |_, v| {
            captures += 1;
            Ok(fs_loud(univibe_volume_curve(v)))
        },
    )
    .expect("solve");
    assert!(
        (r.predicted_lufs - -20.0).abs() <= KNOB_TOL_LU,
        "bracket mode must land within KNOB_TOL_LU: best {} LUFS at v={} ({} captures)",
        r.predicted_lufs,
        r.final_value,
        captures
    );
    assert!(!r.clamped && !r.unconverged, "converged solve: {r:?}");
    // Budget sanity: seeds + expansion + bounded correction, never unbounded.
    assert!(captures <= 3 + FS_CORRECT_MAX, "captures={captures}");
}

// A knob with NO authority over loudness (every capture measures the same
// off-target LUFS) is a physical dead end: a re-run repeats the identical miss,
// so the verdict must be the reason-less clamp — never "unconverged", whose UI
// copy advertises a re-run that cannot help.
#[test]
fn solve_footswitch_flat_response_reports_clamped_not_unconverged() {
    let mut captures = 0u32;
    let r = solve_footswitch(
        12,
        &[],
        &[],
        -20.0,
        "baked",
        None,
        &fs_unit_param(),
        |_, _| {
            captures += 1;
            Ok(fs_loud(-27.0))
        },
    )
    .expect("solve");
    assert!(
        r.clamped && !r.unconverged,
        "flat response must clamp, not advertise a re-run: {r:?}"
    );
    assert!(captures <= 3 + FS_CORRECT_MAX, "captures={captures}");
}

// --- Regression coverage for the "silence past the first seed aborts the whole solve"
// bug (HW-reproduced, fw 1.8.45, preset "TR+BD2+BMP" — 4 drive pedals into a '65 Twin).
// A pedal `level`/`volume` knob at 0 reads genuinely SILENT on the real unit — that
// silence is the quiet EXTREME of the response curve (DATA), not a routing error, for
// every measurement AFTER the first seed. See `FS_SILENT_GEOMETRY_LUFS`'s doc for why
// the synthesized pseudo point is pinned there rather than at the true digital-silence
// floor `MUTE_FLOOR_SILENT_LUFS`. The first-seed routing-clamp arm has its own test just
// below, now that its disengage is injected rather than a real connection.

/// #239 follow-up: a CLIPPED probe past the first seed is the knob's loud end, not a
/// fatal error. Before, the hub's clip refusal aborted the whole row even though a lower
/// value reaches the target. Here everything above 0.40 clips and the target sits at
/// about 0.36: the solve must step back down and land in band on a real capture.
#[test]
fn solve_footswitch_steps_back_down_from_a_clipped_probe() {
    let seen: std::cell::RefCell<Vec<(f32, bool)>> = std::cell::RefCell::new(Vec::new());
    let r = solve_footswitch(
        12,
        &[],
        &[],
        -24.0,
        "baked",
        None,
        &fs_unit_param(),
        |_, v| {
            let clipped = v > 0.40;
            seen.borrow_mut().push((v, clipped));
            if clipped {
                Err(LevelError::Clipped)
            } else {
                Ok(fs_loud(-40.0 + 45.0 * f64::from(v)))
            }
        },
    )
    .expect("a clipped probe must not abort the solve");
    let seen = seen.into_inner();
    assert!(
        seen.iter().any(|&(_, c)| c),
        "the fixture must reach the clipped arm: {seen:?}"
    );
    assert!(
        !r.clamped && !r.unconverged,
        "must converge below the clip: {r:?} {seen:?}"
    );
    assert!(r.final_value <= 0.40, "{r:?}");
    assert!((r.predicted_lufs - -24.0).abs() <= ACCEPT_TOL_LU, "{r:?}");
}

/// #261: the solve owns no connection. A silent FIRST seed is the routing clamp; it
/// disengages through the caller's hook exactly once, and opens nothing itself.
#[test]
fn solve_footswitch_routing_clamp_disengages_through_the_callers_hook() {
    let mut offs = 0u32;
    let r = solve_footswitch_with(
        12,
        &[],
        &[],
        -20.0,
        "baked",
        None,
        &fs_unit_param(),
        || offs += 1,
        |_, _| Err(LevelError::NoSignal),
    )
    .expect("a routing clamp is a result, not an error");
    assert!(r.clamped, "{r:?}");
    assert_eq!(r.clamp_reason.as_deref(), Some("no signal on USB 1/2"));
    assert_eq!(offs, 1, "one mid-solve OFF, through the hook");
}

/// HW-measured Plumes-pedal `level` knob curve (`probe --fs-sweep`, preset
/// "TR+BD2+BMP", fw 1.8.45): 0.11→−66.9, 0.20→−24.2, 0.30→−20.1, 0.50→−18.1,
/// 0.75→−17.2, 1.00→−16.9 LUFS — the cliff shifted 0.01 up from the HW-measured
/// 0.10 so the fixed seed pair (0.25/0.75) reads a genuinely SLOPED, non-flat pair
/// (`fs_bracket_expansion` must NOT fire) while the correction loop's FIRST
/// extrapolated point still lands on the wrong side of the cliff — the exact
/// pathology reported (HW, preset "TR+BD2+BMP"): a knob whose useful range is a
/// narrow slice of its own range extrapolates a knob value PAST the silent
/// extreme mid-correction, not at either seed. `v ≤ 0.11` collapses to a
/// genuinely SILENT capture (not merely quiet).
fn plumes_level_curve(v: f32) -> Result<lufs::Loudness, LevelError> {
    const ANCHORS: [(f64, f64); 6] = [
        (0.11, -66.9),
        (0.20, -24.2),
        (0.30, -20.1),
        (0.50, -18.1),
        (0.75, -17.2),
        (1.00, -16.9),
    ];
    if f64::from(v) <= 0.11 {
        return Err(LevelError::NoSignal);
    }
    Ok(fs_loud(piecewise_lufs(&ANCHORS, v)))
}

/// Re-pinned to the MID-LOOP silence arm (a `LevelError::NoSignal` on the bracket-aware
/// secant's OWN extrapolated point, not on either seed) — the law-predicted seed 2
/// reroutes this fixture off the arm it was originally built to cover, and the
/// `FS_MIN_SEED_GAP_LU` fix (the law-predicted seed 2's acceptance gate is now the
/// EXPECTED LUFS separation, not the raw v-space span) reroutes it AGAIN: at the old
/// -26.0 target the prediction is now ACCEPTED (a real, ≈0.16 knob value — the gap-gate
/// fix's whole point), so the mid-loop-silence arm needs a target far enough from seed
/// 1's -22.15 LUFS reading to fail the CENTRAL-5%-95%-of-range frac gate instead
/// (rather than the gap gate, which a target this far away trivially clears) — -40.0
/// pushes the raw prediction below v=0.05, rejecting it and falling seed 2 back to the
/// fixed 0.75 exactly as before. Seed 1 (0.25, real) and seed 2 (0.75, real) are both
/// proven-real captures with real slope (`fs_bracket_expansion` never fires — the pair
/// isn't flat), and the correction loop's own extrapolation from that (comparatively
/// shallow, real-region-only) pair wildly undershoots into the silent zone before
/// climbing back — several consecutive captures land silent before the secant recovers
/// (mirroring the ORIGINAL reported pathology more faithfully than a single-dip case
/// would: a shallow seed pair genuinely can take more than one correction to walk back
/// off a cliff it undershot). Provably reaches the arm: `seen[2]` (the first
/// correction-loop capture, neither seed) is asserted silent.
#[test]
fn solve_footswitch_treats_post_seed_silence_as_the_quiet_extreme_not_a_routing_error() {
    let seen: std::cell::RefCell<Vec<f32>> = std::cell::RefCell::new(Vec::new());
    let r = solve_footswitch(
        21,
        &[],
        &[],
        -40.0,
        "baked",
        None,
        &fs_unit_param(),
        |_, v| {
            seen.borrow_mut().push(v);
            plumes_level_curve(v)
        },
    )
    .expect("a silent capture mid-correction must be DATA, never a fatal abort");
    let seen = seen.borrow();
    assert_eq!(seen[0], 0.25, "seed 1 is the fixed quarter-range point");
    assert_eq!(
        seen[1], 0.75,
        "seed 2's law prediction is gated off (predicted value outside the central \
         5%-95% of the range), so it falls back to the fixed complement"
    );
    assert!(
        seen.len() >= 3 && seen[2] <= 0.11,
        "the MID-LOOP arm must actually fire — the third capture (the correction \
         loop's own extrapolation, not either seed) must probe below the cliff: \
         {seen:?}"
    );
    assert!(
        (r.predicted_lufs - -40.0).abs() <= KNOB_TOL_LU,
        "expected convergence near -40.0: got {} at v={} ({} captures)",
        r.predicted_lufs,
        r.final_value,
        seen.len()
    );
    assert!(
        !r.clamped,
        "the -40 target sits on the measured cliff, reachable: {r:?}"
    );
    assert!(
        !r.unconverged,
        "must actually converge, not run out of budget: {r:?}"
    );
    assert!(
        r.final_value > 0.11 && r.final_value < 0.25,
        "solved value should land on the measured cliff (0.11, 0.25): {}",
        r.final_value
    );
    assert!(
        seen.len() as u32 <= 3 + FS_CORRECT_MAX,
        "captures={}",
        seen.len()
    );
}

/// A knob whose useful range is a narrow slice near the TOP of `[0, 1]`: the fixed
/// 0.25/0.75 seeds land on an identical flat −17.0 plateau (the ORIGINAL false
/// "no authority" trigger — `fs_bracket_expansion` fires), but the expansion probe
/// toward 0.0 is genuinely SILENT below 0.12 rather than merely quiet. Before the fix
/// this silence was swallowed whole by the bracket-expansion probe's `if let Ok`,
/// leaving the flat pair undisturbed → the false reason-less clamp. After the fix the
/// probe's silence becomes a pseudo point, giving the pair real slope so the secant
/// reaches the target sitting in the 0.12–0.25 ramp.
fn flat_plateau_curve(v: f32) -> Result<lufs::Loudness, LevelError> {
    let v = f64::from(v);
    if v < 0.12 {
        return Err(LevelError::NoSignal);
    }
    if v >= 0.25 {
        return Ok(fs_loud(-17.0));
    }
    let frac = (v - 0.12) / (0.25 - 0.12);
    Ok(fs_loud(-50.0 + frac * 33.0))
}

/// Re-pinned to the EXPANSION-PROBE arm specifically. The seed-1 reading here sits
/// EXACTLY on the flat plateau (`v = 0.25` is the plateau's own boundary, `l_a =
/// −17.0`), so under `FS_MIN_SEED_GAP_LU` the ONLY way to reject the law-predicted
/// seed 2 is a target within 1 LU of `l_a` — a target far enough away (the original
/// −26.0, or −22.0) instead gets ACCEPTED and lands the prediction for real inside the
/// 0.12–0.25 ramp, never reaching the expansion probe at all. −17.6 (0.6 LU of −17.0,
/// under the 1 LU gap floor) rejects the prediction on the gap gate — the SAME
/// rejection that keeps seed 2 at the fixed 0.75 complement, also real, also flat vs
/// seed 1 (`v = 0.75` is still on the plateau) — so the seed PAIR itself stays flat and
/// it is `fs_bracket_expansion` alone that reaches for the 0.0 extreme and finds it
/// silent. Provably reaches the arm: `seen[2] == 0.0` — the low bound the expansion
/// probe (never a seed) targets.
#[test]
fn solve_footswitch_flat_seed_pair_with_silent_expansion_still_converges() {
    let seen: std::cell::RefCell<Vec<f32>> = std::cell::RefCell::new(Vec::new());
    let r = solve_footswitch(
        22,
        &[],
        &[],
        -17.6,
        "baked",
        None,
        &fs_unit_param(),
        |_, v| {
            seen.borrow_mut().push(v);
            flat_plateau_curve(v)
        },
    )
    .expect("a silent bracket-expansion probe must not abort the solve");
    let seen = seen.borrow();
    assert_eq!(seen[0], 0.25, "seed 1 is the fixed quarter-range point");
    assert_eq!(
        seen[1], 0.75,
        "seed 2's law prediction is gated off (the expected LUFS gap from seed 1 is \
         under FS_MIN_SEED_GAP_LU), keeping the pair flat so the expansion probe — not \
         seed 2 — owns the silence"
    );
    assert_eq!(
        seen.get(2),
        Some(&0.0),
        "the EXPANSION-PROBE arm must fire: the third capture is `fs_bracket_expansion`'s \
         own low-bound probe, not a seed: {seen:?}"
    );
    assert!(
        (r.predicted_lufs - -17.6).abs() <= KNOB_TOL_LU,
        "expected convergence near -17.6: got {} at v={} ({} captures)",
        r.predicted_lufs,
        r.final_value,
        seen.len()
    );
    assert!(
        !r.clamped && !r.unconverged,
        "the false 'no authority' clamp must not fire once the expansion probe's \
         silence is treated as data: {r:?}"
    );
    assert!(
        seen.len() as u32 <= 3 + FS_CORRECT_MAX,
        "captures={}",
        seen.len()
    );
}

/// A "gate"-style knob response — DEcreasing with `v` (a physically real shape: e.g. a
/// noise-gate threshold, where turning it UP silences MORE) so seed 2 is the one that
/// lands silent, not seed 1. Steepened from the original -26 LU/knob-unit slope to -44:
/// with the shallower slope the law-predicted seed 2 landed INSIDE the real region
/// (0.436, computed from seed 1's -32.33 LUFS reading), rerouting this fixture off the
/// `seed2_synthetic` arm entirely. At -44 LU/knob-unit the same prediction (`0.25 +
/// 20·log10⁻¹(...)` ≈ 1.03) overshoots past the range's 0.95 fraction ceiling (in fact
/// past `1.0` outright), so the frac gate — UNCHANGED by the `FS_MIN_SEED_GAP_LU` fix,
/// which only replaced the separate v-space-span component — rejects it and seed 2
/// falls back to the fixed complement (0.75) — which this curve keeps silent
/// (`v ≥ 0.60`). Exercises the `seed2_synthetic` arm directly: the initial best-seed pick
/// must still choose the real (0.25) reading, never the synthesized one, and the pseudo
/// point must still let the secant find the target. Provably reaches the arm: `seen[1]`
/// (seed 2 itself) is asserted silent.
fn second_seed_silent_curve(v: f32) -> Result<lufs::Loudness, LevelError> {
    let v = f64::from(v);
    if v >= 0.60 {
        return Err(LevelError::NoSignal);
    }
    Ok(fs_loud(-14.0 - (v / 0.60) * 44.0))
}

#[test]
fn solve_footswitch_second_seed_silence_becomes_data_not_a_hard_error() {
    let seen: std::cell::RefCell<Vec<f32>> = std::cell::RefCell::new(Vec::new());
    let r = solve_footswitch(
        23,
        &[],
        &[],
        -20.0,
        "baked",
        None,
        &fs_unit_param(),
        |_, v| {
            seen.borrow_mut().push(v);
            second_seed_silent_curve(v)
        },
    )
    .expect("a silent SECOND seed must not abort the solve either");
    let seen = seen.borrow();
    assert_eq!(seen[0], 0.25, "seed 1 is the fixed quarter-range point");
    assert!(
        seen.len() >= 2 && seen[1] >= 0.60,
        "the seed2_synthetic arm must fire: seed 2 itself must be the silent capture: {seen:?}"
    );
    assert!(
        (r.predicted_lufs - -20.0).abs() <= KNOB_TOL_LU,
        "expected convergence near -20.0: got {} at v={} ({} captures)",
        r.predicted_lufs,
        r.final_value,
        seen.len()
    );
    assert!(
        !r.clamped && !r.unconverged,
        "reachable target must converge: {r:?}"
    );
    assert!(
        seen.len() as u32 <= 3 + FS_CORRECT_MAX,
        "captures={}",
        seen.len()
    );
}

/// The Plumes shape shifted 40 LU down — a very quiet chain whose REAL captures sit
/// BELOW the −50 sentinel. A fixed-LUFS pseudo point would be the LOUDEST point in any
/// pair it enters (slope sign inverts — the solver would walk the wrong way), so the
/// sentinel must ride a fixed margin below the quietest real capture instead.
fn deep_quiet_pedal_curve(v: f32) -> Result<lufs::Loudness, LevelError> {
    const ANCHORS: [(f64, f64); 7] = [
        (0.10, -106.9),
        (0.15, -69.8),
        (0.20, -64.2),
        (0.30, -60.1),
        (0.50, -58.1),
        (0.75, -57.2),
        (1.00, -56.9),
    ];
    if f64::from(v) <= 0.10 {
        return Err(LevelError::NoSignal);
    }
    Ok(fs_loud(piecewise_lufs(&ANCHORS, v)))
}

/// Retargeted from the original −68.0 to −72.0 LUFS: the law-predicted seed 2 is what
/// now drives this fixture into the silent zone in the first place — at −68.0 the
/// prediction (0.127, computed from seed 1's −62.15 LUFS reading) stayed just above the
/// 0.10 cliff, so NO capture ever went silent and the fixture stopped exercising
/// `fs_silent_geometry` at all. At −72.0 the same prediction formula lands at ≈0.080
/// (silent), and the correction loop that follows keeps re-probing the silent zone at
/// slightly different `v` for several iterates BEFORE crossing back into real territory
/// — a direct exercise of `min_real`'s running-floor property (every real capture on
/// this curve sits below −50, so a pseudo point anchored at the FIXED
/// `FS_SILENT_GEOMETRY_LUFS` sentinel rather than `min_real - FS_SILENT_MARGIN_LU` would
/// sit ABOVE the real points and invert the secant's slope). UNCHANGED by the
/// `FS_MIN_SEED_GAP_LU` fix: both gates agree here (the 9.85 LU expected gap clears
/// `FS_MIN_SEED_GAP_LU`, and 0.080 sits inside the central 5%-95% frac window), so this
/// fixture's target and behavior needed no retune. Provably reaches the arm: at least
/// two DISTINCT probed values sit in the silent zone.
#[test]
fn solve_footswitch_silence_sentinel_stays_below_real_captures_on_quiet_chains() {
    let seen: std::cell::RefCell<Vec<f32>> = std::cell::RefCell::new(Vec::new());
    let r = solve_footswitch(
        26,
        &[],
        &[],
        -72.0,
        "baked",
        None,
        &fs_unit_param(),
        |_, v| {
            seen.borrow_mut().push(v);
            deep_quiet_pedal_curve(v)
        },
    )
    .expect("a silent probe on a quiet chain must be data, never an abort");
    let seen = seen.borrow();
    let silent_probes: Vec<f32> = seen.iter().copied().filter(|&v| v <= 0.10).collect();
    assert!(
        silent_probes.len() >= 2,
        "fs_silent_geometry's min_real floor needs REPEATED silent probes (not a \
         one-off) to prove it: {seen:?}"
    );
    assert!(
        silent_probes
            .iter()
            .any(|&a| silent_probes.iter().any(|&b| (a - b).abs() > 1e-6)),
        "the repeated silent probes must land at DISTINCT knob values, not the same \
         point re-measured: {seen:?}"
    );
    assert!(
        (r.predicted_lufs - -72.0).abs() <= KNOB_TOL_LU,
        "expected convergence near -72.0: got {} at v={} ({} captures) — a \
         sentinel ABOVE the real captures inverts the secant's slope",
        r.predicted_lufs,
        r.final_value,
        seen.len()
    );
    assert!(
        !r.clamped && !r.unconverged,
        "the -72 target sits on the curve's cliff, reachable: {r:?}"
    );
    assert!(
        seen.len() as u32 <= 3 + FS_CORRECT_MAX,
        "captures={}",
        seen.len()
    );
}

// Regression: an HONESTLY flat response (never silent anywhere) must keep the
// reason-less headroom/no-authority clamp — the fix must not turn a genuine flat
// response into a spurious silence-driven convergence.
#[test]
fn solve_footswitch_flat_response_without_silence_keeps_honest_clamp() {
    let mut captures = 0u32;
    let r = solve_footswitch(
        25,
        &[],
        &[],
        -26.0,
        "baked",
        None,
        &fs_unit_param(),
        |_, _v| {
            captures += 1;
            Ok(fs_loud(-17.0))
        },
    )
    .expect("solve");
    assert!(
        r.clamped && !r.unconverged,
        "an honestly flat (never silent) response must clamp, not advertise a re-run: {r:?}"
    );
    assert_eq!(
        r.clamp_reason, None,
        "a headroom/no-authority clamp carries NO reason (that belongs to the seed's \
         routing probe alone): {r:?}"
    );
    assert!(captures <= 3 + FS_CORRECT_MAX, "captures={captures}");
}

// ── log-space coordinate map + law-predicted seed 2 ─────────────────────────────────

/// The linear-amplitude law `L = 20·log10(v) + C` — proven exact for `presetLevel`,
/// amp `outputLevel` (scene lane), and the VolumePedal DSP (`out = in × g`) — is
/// solvable from ONE live point: seed 1 fixes `C`, seed 2 lands on target. The old
/// knob-space secant instead crept ~1 LU per capture (HW, MythicDrive FS:
/// −30 → −22 → −24 → −25 → −26 over 5 captures). Each capture is a real ~15–20 s
/// re-amp measurement, so the budget IS the feature: seed 1 + the exact law-predicted
/// seed 2 = 2 captures, budget 3 gives noise headroom.
#[test]
fn solve_footswitch_log_law_curve_solves_in_three_captures() {
    let mut captures = 0u32;
    let r = solve_footswitch(
        12,
        &[],
        &[],
        -20.0,
        "baked",
        None,
        &fs_unit_param(),
        |_, v| {
            captures += 1;
            Ok(fs_loud(20.0 * f64::from(v.max(1e-4)).log10() - 14.0))
        },
    )
    .expect("solve");
    assert!(
        (r.predicted_lufs - -20.0).abs() <= ACCEPT_TOL_LU,
        "must land within ACCEPT_TOL_LU: best {} LUFS at v={} ({captures} captures)",
        r.predicted_lufs,
        r.final_value,
    );
    assert!(!r.clamped && !r.unconverged, "converged solve: {r:?}");
    assert!(captures <= 3, "captures={captures}");
}

/// The idempotency probe's capture is a PAID measurement: when it misses target it
/// must join the solve as seed 1 (it proved signal, so it owns no routing verdict —
/// that stays with the fixed seed on the probe-less path), not be discarded and
/// re-bought. Probe + law-placed seed 2 = 2 captures on an exact-law curve — the SAME
/// count as the no-probe path (`solve_footswitch_log_law_curve_solves_in_three_captures`);
/// the probe replaces seed 1 rather than adding to it, so it is never MORE captures,
/// only ever fewer than main's PRE-this-change discard-and-rebuy behavior (which paid
/// 3: the wasted probe, seed 1, seed 2).
#[test]
fn solve_footswitch_reuses_the_idempotency_probe_as_a_seed() {
    let mut captures = 0u32;
    let r = solve_footswitch(
        12,
        &[],
        &[],
        -20.0,
        "baked",
        Some(0.3),
        &fs_unit_param(),
        |_, v| {
            captures += 1;
            Ok(fs_loud(20.0 * f64::from(v.max(1e-4)).log10() - 14.0))
        },
    )
    .expect("solve");
    assert!(
        (r.predicted_lufs - -20.0).abs() <= ACCEPT_TOL_LU,
        "must land within ACCEPT_TOL_LU: best {} LUFS at v={} ({captures} captures)",
        r.predicted_lufs,
        r.final_value,
    );
    assert!(!r.clamped && !r.unconverged, "converged solve: {r:?}");
    assert_eq!(captures, 2, "probe replaces seed 1: exactly 2 captures");
    assert_eq!(
        r.iterations, 2,
        "the probe counts as capture 1, so `iterations` is 2 after seed 2 — the same \
         accounting as the no-probe path"
    );
}

/// A param whose loudness DECREASES with knob value — names imply neither direction
/// nor monotonicity, so the log-space transform must stay a pure monotone
/// reparameterization (a misprediction falls back to the fixed seed pair, or a
/// straddling bracket recovers) and never a direction assumption. Must converge inside
/// the standard fallback-seed budget.
#[test]
fn solve_footswitch_inverted_response_still_converges() {
    let mut captures = 0u32;
    let r = solve_footswitch(
        12,
        &[],
        &[],
        -20.0,
        "baked",
        None,
        &fs_unit_param(),
        |_, v| {
            captures += 1;
            Ok(fs_loud(-26.0 + 12.0 * (1.0 - f64::from(v))))
        },
    )
    .expect("solve");
    assert!(
        (r.predicted_lufs - -20.0).abs() <= ACCEPT_TOL_LU,
        "must land within ACCEPT_TOL_LU: best {} LUFS at v={} ({captures} captures)",
        r.predicted_lufs,
        r.final_value,
    );
    assert!(!r.clamped && !r.unconverged, "converged solve: {r:?}");
    assert!(captures <= 2 + 1 + FS_CORRECT_MAX, "captures={captures}");
}

/// Flat response with a `current_value` probe: seeding from the probe + a law-placed
/// (or fixed-fallback) second seed must not shrink the pair below the span the
/// no-authority proof needs — the verdict stays the reason-less clamp (never
/// "unconverged"), exactly as on the probe-less path.
#[test]
fn solve_footswitch_flat_response_with_probe_still_reports_clamped() {
    let mut captures = 0u32;
    let r = solve_footswitch(
        12,
        &[],
        &[],
        -20.0,
        "baked",
        Some(0.5),
        &fs_unit_param(),
        |_, _| {
            captures += 1;
            Ok(fs_loud(-27.0))
        },
    )
    .expect("solve");
    assert!(
        r.clamped && !r.unconverged,
        "flat response must clamp, not advertise a re-run: {r:?}"
    );
    assert!(captures <= 2 + 1 + FS_CORRECT_MAX, "captures={captures}");
}

/// `FS_MIN_SEED_GAP_LU`'s whole reason to exist: the OLD `FS_MIN_SEED_SPAN_FRAC` gate
/// (raw v-space span ≥ 12% of the range) wrongly rejected a correct law prediction at a
/// LOW knob value, because the log-knob map compresses hardest exactly there — seed 1
/// at `v ≈ 0.05` needing a genuine ~6 LU correction moves `v` by only ~0.05, well under
/// a 12%-of-`[0,1]` span, so the feature silently no-op'd in the exact regime it was
/// built for. The new LU-gap gate accepts it instead. Uses a probe seed (`current_value:
/// Some(0.05)`) so seed 1 sits at 0.05 directly rather than the fixed 0.25 fraction, on
/// an EXACT log-amplitude law (`L = 20·log10(v)`) so the law-predicted seed 2 lands
/// exactly on target — solved in 2 captures (the probe + the accepted, non-fallback
/// seed 2), never reaching the correction loop at all.
#[test]
fn solve_footswitch_accepts_a_quiet_knob_law_prediction_the_old_span_gate_rejected() {
    let seen: std::cell::RefCell<Vec<f32>> = std::cell::RefCell::new(Vec::new());
    let r = solve_footswitch(
        27,
        &[],
        &[],
        -20.0,
        "baked",
        Some(0.05),
        &fs_unit_param(),
        |_, v| {
            seen.borrow_mut().push(v);
            Ok(fs_loud(20.0 * f64::from(v).log10()))
        },
    )
    .expect("solve");
    let seen = seen.borrow();
    assert_eq!(
        seen[0], 0.05,
        "the probe supplies seed 1 directly, at the quiet value"
    );
    assert!(
        seen.contains(&0.1),
        "the law-predicted (non-fallback) seed 2 must actually be probed — the fixed \
         fallback for a seed 1 this low in the range would be 0.75, never 0.1: {seen:?}"
    );
    assert!(
        (r.predicted_lufs - -20.0).abs() <= ACCEPT_TOL_LU,
        "must land within ACCEPT_TOL_LU: best {} LUFS at v={} ({} captures)",
        r.predicted_lufs,
        r.final_value,
        seen.len()
    );
    assert!(!r.clamped && !r.unconverged, "converged solve: {r:?}");
    assert!(seen.len() as u32 <= 3, "captures={}", seen.len());
}

/// Round-trip identity that makes [`FsParamTarget::coord_to_value`]'s floor safe: for
/// every coordinate at or above `to_coord`'s own `-60` floor, mapping back through
/// `coord_to_value` and forward again through `to_coord` must reproduce it exactly —
/// otherwise a correction-loop coordinate near the floor could drift to a DIFFERENT
/// real, paid knob value on every iterate instead of collapsing cleanly onto the floor.
#[test]
fn coord_round_trips_exactly_at_and_above_the_log_floor() {
    let param = fs_unit_param();
    for u in [-60.0, -59.999, -40.0, -20.0, -12.041, -0.5, 0.0] {
        let v = param.coord_to_value(u);
        assert!(
            (param.to_coord(v) - u).abs() < 1e-9,
            "to_coord(coord_to_value({u})) should reproduce {u} exactly, got {} via v={v}",
            param.to_coord(v)
        );
    }
    // Below the floor, `coord_to_value` clamps to `KNOB_LOG_FLOOR` rather than emitting
    // a real, distinct value per coordinate — two different sub-floor coordinates must
    // collapse to the identical `v`.
    assert_eq!(param.coord_to_value(-70.0), param.coord_to_value(-90.0));
    assert_eq!(param.coord_to_value(-70.0), KNOB_LOG_FLOOR);
}

/// The RESCUE half of `FS_MIN_SEED_GAP_LU`'s safety guarantor, which every other
/// accepted-prediction fixture leaves unexercised (they all end in a clamp): seed 1
/// (0.25, −24.0) and the law-predicted, ACCEPTED seed 2 (≈0.284 — gap 1.1 LU ≥
/// `FS_MIN_SEED_GAP_LU`, frac ≈0.28 inside 5%–95% — land in the −23.9 flat plateau
/// [0.28, 0.6], so the pair measures ~0.1 LU flat (< `KNOB_TOL_LU`) even though this
/// prediction was accepted, not rejected. `fs_bracket_expansion`'s entry condition
/// fires; the target (−22.9) sits ABOVE the flat plateau's loudness, so it probes the
/// HI extreme (1.0) and finds REAL slope there (−18.0), rescuing the pair into a
/// genuine converged solve rather than a false no-authority clamp. Provably reaches
/// the arm: `seen[1]` is the accepted prediction (not the 0.75 fixed fallback),
/// `seen[2] == 1.0` (the expansion probe, not a seed).
#[test]
fn solve_footswitch_accepted_flat_pair_is_rescued_by_bracket_expansion() {
    fn accepted_flat_pair_curve(v: f32) -> Result<lufs::Loudness, LevelError> {
        let v = f64::from(v);
        let l = if v <= 0.25 {
            -24.0
        } else if v < 0.28 {
            let frac = (v - 0.25) / (0.28 - 0.25);
            -24.0 + frac * (-23.9 - -24.0)
        } else if v <= 0.6 {
            -23.9
        } else {
            let frac = (v - 0.6) / (1.0 - 0.6);
            -23.9 + frac * (-18.0 - -23.9)
        };
        Ok(fs_loud(l))
    }
    let seen: std::cell::RefCell<Vec<f32>> = std::cell::RefCell::new(Vec::new());
    let r = solve_footswitch(
        40,
        &[],
        &[],
        -22.9,
        "baked",
        None,
        &fs_unit_param(),
        |_, v| {
            seen.borrow_mut().push(v);
            accepted_flat_pair_curve(v)
        },
    )
    .expect("solve");
    let seen = seen.borrow();
    assert_eq!(seen[0], 0.25, "seed 1 is the fixed quarter-range point");
    assert!(
        (0.26..0.6).contains(&seen[1]),
        "seed 2 must be the ACCEPTED law prediction landing in the flat plateau, not \
         the 0.75 fixed fallback: {seen:?}"
    );
    assert_eq!(
        seen.get(2),
        Some(&1.0),
        "the accepted-but-flat-measured pair must trigger `fs_bracket_expansion`'s hi \
         extreme probe, not a mid-loop secant step: {seen:?}"
    );
    assert!(
        (r.predicted_lufs - -22.9).abs() <= KNOB_TOL_LU,
        "the extreme probe's real slope must let the secant converge: best {} LUFS at \
         v={} ({} captures)",
        r.predicted_lufs,
        r.final_value,
        seen.len()
    );
    assert!(
        !r.clamped && !r.unconverged,
        "a REAL slope at the extreme must rescue the solve, never a false no-authority \
         clamp: {r:?}"
    );
    assert!(
        seen.len() as u32 <= 3 + FS_CORRECT_MAX,
        "captures={}",
        seen.len()
    );
}

/// The original Plumes incident (HW-reproduced, fw 1.8.45, preset "TR+BD2+BMP") AT ITS
/// REAL in-UI-range target: the re-pinned mid-loop-silence test above had to move to
/// −40.0 (outside any real target range) to keep exercising that specific arm once the
/// law-predicted seed 2 started landing for real — but that leaves the ORIGINAL −26.0
/// shape uncovered. There, the accepted prediction (gap 3.85 LU, frac ≈0.16 — both
/// gates pass) lands at v≈0.16, where `plumes_level_curve`'s REAL response reads
/// ~16 LU off the idealized log-amplitude law it was predicted from (a piecewise HW
/// curve, not the exact law `solve_footswitch_log_law_curve_solves_in_three_captures`
/// uses) — this is the only convergence coverage for an ACCEPTED-but-badly-wrong
/// prediction on a real, non-ideal curve; every other accepted-prediction test uses an
/// exact law curve where the prediction is exact by construction. Provably reaches the
/// arm: `seen[1]` is the accepted prediction (~0.16), not the 0.75 fixed fallback.
#[test]
fn solve_footswitch_plumes_accepted_prediction_still_converges_from_a_bad_first_guess() {
    let seen: std::cell::RefCell<Vec<f32>> = std::cell::RefCell::new(Vec::new());
    let r = solve_footswitch(
        41,
        &[],
        &[],
        -26.0,
        "baked",
        None,
        &fs_unit_param(),
        |_, v| {
            seen.borrow_mut().push(v);
            plumes_level_curve(v)
        },
    )
    .expect("solve");
    let seen = seen.borrow();
    assert_eq!(seen[0], 0.25, "seed 1 is the fixed quarter-range point");
    assert!(
        (0.10..0.25).contains(&seen[1]),
        "seed 2 must be the ACCEPTED law prediction (~0.16), not the 0.75 fixed \
         fallback: {seen:?}"
    );
    assert!(
        (r.predicted_lufs - -26.0).abs() <= KNOB_TOL_LU,
        "the correction loop must recover from a badly-wrong-but-accepted first guess: \
         best {} LUFS at v={} ({} captures)",
        r.predicted_lufs,
        r.final_value,
        seen.len()
    );
    assert!(
        !r.clamped && !r.unconverged,
        "a reachable target must converge, not clamp or run out of budget: {r:?}"
    );
    assert!(
        seen.len() as u32 <= 3 + FS_CORRECT_MAX,
        "captures={}",
        seen.len()
    );
}

// ── param-class-driven solve: refusal, bounds, wet floor ────────────────────────────

// ENTRY GUARD: a param the classifier answers `Other` for is not a level control.
// Sweeping it would change the sound the player wrote, so the solve must refuse BEFORE
// any device work — no capture may be requested at all.
#[test]
fn solve_footswitch_refuses_a_param_that_is_not_a_level_control() {
    let mut captures = 0u32;
    // `intensity` is in neither the defaults nor any block override ⇒ Other.
    let param = FsParamTarget::new("ACD_TremoloBias", "intensity", 0.5);
    assert_eq!(param.info.class, crate::param_class::ParamClass::Other);
    let err = solve_footswitch(3, &[], &[], -20.0, "baked", None, &param, |_, _| {
        captures += 1;
        Ok(fs_loud(-20.0))
    })
    .expect_err("an Other-classified param must refuse")
    .to_string();
    assert_eq!(captures, 0, "the refusal must precede every device capture");
    assert!(
        err.contains("intensity")
            && err.contains("ACD_TremoloBias")
            && err.contains("not a level control"),
        "the refusal must name the param, the block and the cause: {err}"
    );
    // The block-scoped override trap rides the same guard: `level` is a level_linear
    // DEFAULT everywhere, but on the TM Rumble it is an amp knob that must never be swept.
    let trapped = FsParamTarget::new("ACD_TMRumbleV3", "level", 0.5);
    assert!(
        solve_footswitch(3, &[], &[], -20.0, "baked", None, &trapped, |_, _| Ok(
            fs_loud(-20.0)
        ))
        .is_err()
    );
}

// BOUNDS: params are no longer all `[0,1]`. On a raw-dB `[0, 12]` control the seeds land
// at a quarter/three quarters of THAT range (3.0 / 9.0), the secant clamps to it, and a
// solved value above 1.0 survives instead of being silently pinned.
#[test]
fn solve_footswitch_solves_in_the_params_own_db_range() {
    let param = FsParamTarget::new("ACD_Boost", "gain", 2.5);
    assert_eq!(param.info.class, crate::param_class::ParamClass::LevelDb);
    assert_eq!(param.bounds(), (0.0, 12.0));
    let seen: std::cell::RefCell<Vec<f32>> = std::cell::RefCell::new(Vec::new());
    // ~1:1 dB→LUFS, HW-verified for this block: -30 LUFS at 0 dB.
    let r = solve_footswitch(0, &[], &[], -22.0, "baked", None, &param, |_, v| {
        seen.borrow_mut().push(v);
        Ok(fs_loud(-30.0 + f64::from(v)))
    })
    .expect("solve");
    // Seed 1 is still the fixed quarter across the range (3.0). Seed 2 is now the
    // LAW-PREDICTED point, not the fixed three-quarters mark: `LevelDb` is ~1:1
    // dB→LUFS (identity `to_coord`/`coord_to_value`), so predicting from seed 1's -27.0 dB
    // reading toward the -22.0 target lands EXACTLY on 3.0 + (-22.0 - (-27.0)) =
    // 8.0 — a deliberate behavior change (the prediction is exact here), not a
    // weakened gate.
    assert_eq!(
        seen.borrow()[..2],
        [3.0, 8.0],
        "seed 1 stays the fixed quarter-range point; seed 2 is now law-predicted"
    );
    assert!(
        (r.final_value - 8.0).abs() < 0.05,
        "target -22 needs +8 dB, a value the old [0,1] clamp could never return: {r:?}"
    );
    assert!(!r.clamped && !r.unconverged, "8 dB is well inside [0,12]");
}

// ...and the bound-hit verdict follows the range too: pinned at 12.0 with the target
// still louder is UNREACHABLE, not an exhausted search.
#[test]
fn solve_footswitch_clamps_at_the_db_range_ceiling() {
    let param = FsParamTarget::new("ACD_Boost", "gain", 2.5);
    let r = solve_footswitch(0, &[], &[], -5.0, "baked", None, &param, |_, v| {
        Ok(fs_loud(-30.0 + f64::from(v)))
    })
    .expect("solve");
    assert!(
        (r.final_value - 12.0).abs() < 1e-3,
        "the solve must pin at the param's own ceiling: {r:?}"
    );
    assert!(
        r.clamped && !r.unconverged,
        "maxed with the target still louder is unreachable, not re-runnable: {r:?}"
    );
}

// WET FLOOR: a wet/mix control driven toward 0 REMOVES the effect rather than making it
// quieter, so `bounds()` raises the low bound to `WET_FLOOR_FRACTION` × authored — the
// solve can't even probe below it. The floored row is reported clamped with the
// `wet_floor` flag — and deliberately WITHOUT `clamp_reason`, whose contract is "not
// reaching USB 1/2" (the UI renders any reason as `offbranch`).
#[test]
fn solve_footswitch_floors_a_wet_mix_at_a_quarter_of_the_authored_value() {
    // Authored mix 0.80 ⇒ floor 0.20. The response wants far less to hit target.
    let param = FsParamTarget::new("ACD_Chorus", "mix", 0.8);
    assert_eq!(param.info.class, crate::param_class::ParamClass::WetMix);
    let r = solve_footswitch(0, &[], &[], -40.0, "baked", None, &param, |_, v| {
        Ok(fs_loud(-20.0 + 10.0 * f64::from(v)))
    })
    .expect("solve");
    assert!(
        (r.final_value - 0.2).abs() < 1e-6,
        "the solve must be floored at 25% of the authored 0.8: {r:?}"
    );
    assert!(
        r.clamped && !r.unconverged,
        "a floored row is an honest clamp, never a re-runnable miss: {r:?}"
    );
    assert_eq!(
        r.clamp_reason, None,
        "clamp_reason means 'not on USB 1/2' ONLY — a wet floor must not be rendered as \
         a routing failure: {r:?}"
    );
    assert!(
        r.wet_floor,
        "the floor's cause rides the wet_floor flag: {r:?}"
    );
    // With the floor folded into `bounds()`, the reported loudness is a REAL reading
    // of the written value — never an estimate at an unwritable point below the floor.
    assert!(
        (r.predicted_lufs - (-20.0 + 10.0 * f64::from(r.final_value))).abs() < 1e-9,
        "predicted_lufs must be the capture AT final_value: {r:?}"
    );
}

// The floor is RELATIVE, never an absolute 0.25: a solve that lands ABOVE the floor is
// untouched and unflagged, and an authored 0.0 (already fully dry) constrains nothing.
#[test]
fn solve_footswitch_wet_floor_is_relative_and_only_binds_when_crossed() {
    let unfloored = solve_footswitch(
        0,
        &[],
        &[],
        -14.0,
        "baked",
        None,
        &FsParamTarget::new("ACD_Chorus", "mix", 0.8),
        |_, v| Ok(fs_loud(-20.0 + 10.0 * f64::from(v))),
    )
    .expect("solve");
    assert!(
        (unfloored.final_value - 0.6).abs() < 0.01 && !unfloored.clamped,
        "0.6 is above the 0.2 floor, so nothing is clamped or flagged: {unfloored:?}"
    );
    assert!(!unfloored.wet_floor);

    // Authored 0.0 ⇒ floor 0.0 ⇒ no constraint at all (never a hard 0.25).
    let dry = solve_footswitch(
        0,
        &[],
        &[],
        -40.0,
        "baked",
        None,
        &FsParamTarget::new("ACD_Chorus", "mix", 0.0),
        |_, v| Ok(fs_loud(-20.0 + 10.0 * f64::from(v))),
    )
    .expect("solve");
    assert!(
        dry.final_value <= 1e-6,
        "an authored-0.0 mix floors at 0.0, not at an absolute 0.25: {dry:?}"
    );
    assert!(
        !dry.wet_floor,
        "a floor that never RAISED the low bound is an ordinary range edge, not the \
         verify-by-ear advisory: {dry:?}"
    );
}

// The wet floor must anchor on the ENGAGED value, not the switch-OFF base the target was
// constructed from — an existing assign's stored valueA is what the player actually dialed
// in while engaged, and it reaches the solve as `current_value` (the re-run anchor), which
// `solve_footswitch` folds into the target itself (`anchored`) so no call site can forget.
// Base 0.05 (near-dry, switch-OFF `valueB`) with an existing engaged valueA of 0.9 must
// floor at 0.9 × 25% = 0.225, not 0.05 × 25% = 0.0125 — the exact incident (chorus mix→0)
// the anchor exists to prevent.
#[test]
fn wet_floor_anchors_on_the_existing_assigns_engaged_value_not_the_base() {
    let param = FsParamTarget::new("ACD_Chorus", "mix", 0.05);
    let r = solve_footswitch(0, &[], &[], -40.0, "assigned", Some(0.9), &param, |_, v| {
        Ok(fs_loud(-20.0 + 10.0 * f64::from(v)))
    })
    .expect("solve");
    assert!(
        (r.final_value - 0.225).abs() < 1e-6,
        "must floor at 25% of the ENGAGED 0.9, not the base 0.05: {r:?}"
    );
    assert!(r.clamped && !r.unconverged && r.wet_floor);
}

// Base-anchoring stays intact when there is no existing assign (`engaged: None`) — a fresh
// assign floors on the base value exactly as before the anchor existed.
#[test]
fn wet_floor_anchor_is_a_noop_with_no_existing_assign() {
    let param = FsParamTarget::new("ACD_Chorus", "mix", 0.05);
    assert_eq!(param.anchored(None).authored, 0.05);
}

// The WRITE path must carry a raw-dB solved value VERBATIM: with params no longer all
// `[0,1]`, a stray clamp anywhere between the solve and the wire would silently pin a
// `+8 dB` boost at `1.0`. Pinned on the BAKE write, which goes out as `changeParameter`
// (`proto::change_parameter`'s `field_f32`, no clamp — the only `clamp(0.0, 1.0)` in
// `proto.rs` belongs to `setPresetLevel`, a different message). The ASSIGN write's
// `valueA` is a plain `serde_json` float in the same function and is likewise unclamped.
#[test]
fn write_fs_values_carries_a_raw_db_value_past_one_unclamped() {
    let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
    let mut s = Session::from_transport(Box::new(sim.clone()));
    let pending = vec![FsPendingWrite {
        switch: 1,
        lev: ("G1".to_string(), "boost".to_string(), "gain".to_string()),
        write: FsWrite::Bake {
            clear_stale: None,
            mirror_scenes: vec![],
        },
        value: 8.0,
    }];
    write_fs_values_on_session(&mut s, ListIndex(30), &pending, None).expect("write");
    assert_eq!(
        sim.param_write(crate::sim_device::SCENE_BASE, "G1", "boost", "gain"),
        Some(8.0),
        "a raw-dB +8 must reach the device as 8.0, never pinned to 1.0: {:?}",
        sim.events()
    );
}

// The three-state split (was ONE `clamped` flag with `clamp_reason: None` for both the
// unreachable and the merely-unconverged case — the UI could only say "clamped", telling
// a user to accept a number a re-run would have improved, and telling another to retry
// something physically unreachable). Each row must produce a DISTINCT tuple.
#[test]
fn classify_fs_outcome_splits_a_bound_hit_from_an_exhausted_search() {
    let target = -20.0;
    for (label, best_v, best_lufs, want) in [
        // Knob maxed and target still louder → unreachable.
        ("maxed, target louder", 1.0, -26.0, (true, false)),
        // Knob at zero and target still quieter → unreachable.
        ("zeroed, target quieter", 0.0, -14.0, (true, false)),
        // Mid-range miss: captures ran out, the knob still has room → re-runnable.
        ("mid-range miss", 0.55, -26.0, (false, true)),
        ("mid-range overshoot", 0.55, -14.0, (false, true)),
        // On target (within ACCEPT_TOL_LU) → neither, even sitting on a bound. 0.05, not
        // 0.1 exactly — the exact boundary is float-rounding-sensitive (0.1's f64
        // representation makes `abs(-20.1 - -20.0)` land a hair ABOVE 0.1).
        ("on target", 1.0, -20.05, (false, false)),
        // At a bound but the miss is in the direction the knob CAN still move → the
        // search simply stopped early, so it is unconverged, not unreachable.
        ("maxed, target quieter", 1.0, -14.0, (false, true)),
    ] {
        assert_eq!(
            classify_fs_outcome(best_v, best_lufs, target, UNIT_BOUNDS),
            want,
            "{label}: v={best_v} lufs={best_lufs}"
        );
    }
}

// A `FsWrite::Bake` persists with `changeParameter`, which lands in the connection's
// CURRENT scene — and the write session's own `load_preset` activates the preset's saved
// `lastLoadedScene`. Without a base recall the solved value went into that scene's
// overlay while base (what the leveler measured, and what the switch's off position
// renders) kept its old value.
#[test]
fn write_fs_values_bakes_into_base_not_the_saved_scene() {
    let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
    let mut s = Session::from_transport(Box::new(sim.clone()));
    let pending = vec![FsPendingWrite {
        switch: 1,
        lev: (
            "G1".to_string(),
            "amp".to_string(),
            "outputLevel".to_string(),
        ),
        write: FsWrite::Bake {
            clear_stale: None,
            mirror_scenes: vec![],
        },
        value: 0.42,
    }];
    write_fs_values_on_session(&mut s, ListIndex(30), &pending, None).expect("write");
    let ev = sim.events();
    let baked: Vec<i64> = ev
        .iter()
        .filter_map(|e| match e {
            crate::sim_device::SimEvent::ChangeParameter { scene, param, .. }
                if param == "outputLevel" =>
            {
                Some(*scene)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        baked,
        vec![crate::sim_device::SCENE_BASE],
        "the bake must land in BASE, not the saved scene 3: {ev:?}"
    );
}

// THE WRITER side of the param-func-without-valueType HW finding (the device rejects a
// WHOLE imported preset at load when any `func: "param"` ftsw entry lacks `valueType` —
// see `notes/gotchas.md`'s entry of the same name). Pins the
// ASSIGN branch's composed functionJson (the literal wire string a test can parse, per
// `SimEvent::SetFootswitchAssignment`'s own doc) directly, so a refactor that threads
// `FootswitchWriteSpec` "faithfully" but drops the field fails HERE instead of only
// re-arming the discard on a later export/import cycle.
#[test]
fn write_fs_values_assign_composes_a_numeric_value_type() {
    let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
    let mut s = Session::from_transport(Box::new(sim.clone()));
    let pending = vec![FsPendingWrite {
        switch: 1,
        lev: ("G1".to_string(), "n1".to_string(), "level".to_string()),
        write: FsWrite::Assign {
            value_b: 0.2,
            spec: FootswitchWriteSpec {
                function_index: 0,
                color_a: 5,
                color_b: 0,
                custom_label: "TEST".to_string(),
                link_group: 0,
                is_active: false,
                switch_type: 0,
            },
        },
        value: 0.7,
    }];
    write_fs_values_on_session(&mut s, ListIndex(30), &pending, None).expect("write");
    let ev = sim.events();
    let sent = ev
        .iter()
        .find_map(|e| match e {
            crate::sim_device::SimEvent::SetFootswitchAssignment { function_json, .. } => {
                Some(function_json.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("the assign must send a setFootswitchAssignment: {ev:?}"));
    let json: serde_json::Value = serde_json::from_str(&sent).expect("valid functionJson");
    assert!(
        json["valueType"].is_number(),
        "the composed param functionJson must carry a NUMERIC valueType, not omit it \
         or carry a string — its absence makes fw 1.8.45 silently discard the whole \
         preset on a later import: {sent}"
    );
}

// A bake on a device-authored scened preset is MASKED by every full-param scene overlay
// (HW, Hiwatt slot 31: the overlays governed the DSP while base stayed untouched), so the
// solved value must ALSO be written into each overlay that restated the base value —
// after the base write, one recall per mirror scene, then the ORIGINAL `lastLoadedScene`
// recalled so the save re-stamps it (HW: the FS save stamped 8 over the user's scene 3).
#[test]
fn write_fs_values_mirrors_the_bake_and_restores_the_saved_scene() {
    let sim = crate::sim_device::SimDevice::new()
        .with_preset_json(crate::sim_device::SCENED_PRESET_JSON)
        .with_saved_scene(30, Some(3));
    let mut s = Session::from_transport(Box::new(sim.clone()));
    let pending = vec![FsPendingWrite {
        switch: 1,
        lev: (
            "G1".to_string(),
            "amp".to_string(),
            "outputLevel".to_string(),
        ),
        write: FsWrite::Bake {
            clear_stale: None,
            mirror_scenes: vec![0, 2],
        },
        value: 0.42,
    }];
    write_fs_values_on_session(&mut s, ListIndex(30), &pending, Some(3)).expect("write");
    let ev = sim.events();
    let writes: Vec<i64> = ev
        .iter()
        .filter_map(|e| match e {
            crate::sim_device::SimEvent::ChangeParameter { scene, param, .. }
                if param == "outputLevel" =>
            {
                Some(*scene)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        writes,
        vec![crate::sim_device::SCENE_BASE, 0, 2],
        "bake writes base first, then each mirror scene's overlay: {ev:?}"
    );
    // The LAST scene recall before the save must be the preset's original scene 3.
    let last_recall = ev
        .iter()
        .rev()
        .find_map(|e| match e {
            crate::sim_device::SimEvent::LoadScene(scene) => Some(*scene),
            _ => None,
        })
        .expect("a scene recall");
    assert_eq!(
        last_recall, 3,
        "the save must re-stamp the original lastLoadedScene: {ev:?}"
    );
}

// (A3, presetLevel lane) The reported measurement bug: `measure_c`'s captures set
// `presetLevel` with NO scene recall, so on a preset whose saved `lastLoadedScene` is a
// FS scene (HW: preset 28, scene 3) the "base" ceiling `C` was measured in that scene —
// a different sound than the one being leveled. Base is a recall (wire slot 8), never an
// omission.
#[test]
fn arm_measurement_recalls_base_before_a_preset_level_capture() {
    let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(30)).expect("load_preset"); // activates saved scene 3, not base
    arm_measurement(&mut s, &LevelKnob::PresetLevel, 0.5, &[], None, None).expect("arm");
    let ev = sim.events();
    let recall = ev.iter().position(|e| {
        matches!(e, crate::sim_device::SimEvent::LoadScene(sc) if *sc == crate::session::BASE_SCENE_SLOT)
    });
    let set = ev
        .iter()
        .position(|e| matches!(e, crate::sim_device::SimEvent::PresetLevel(_)));
    assert!(
        recall.is_some() && recall < set,
        "the base recall must precede the presetLevel write: {ev:?}"
    );
}

// Isolation LAST: `load_scene` re-asserts the scene's own bypass state, so a forced
// bypass written BEFORE the recall is silently reverted and the capture measures a
// non-isolated sound. Every recall-bearing capture re-sends the full list after it.
#[test]
fn arm_measurement_writes_isolation_bypasses_after_the_scene_recall() {
    let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(30)).expect("load_preset");
    let knob = LevelKnob::Block {
        group_id: "G1".into(),
        node_id: "amp".into(),
        parameter_id: "outputLevel".into(),
        scene_slot: None,
    };
    arm_measurement(
        &mut s,
        &knob,
        0.6,
        &[("G1".to_string(), "fx".to_string(), true)],
        None,
        None,
    )
    .expect("arm");
    let ev = sim.events();
    let recall = ev
        .iter()
        .rposition(|e| matches!(e, crate::sim_device::SimEvent::LoadScene(_)));
    let bypass = ev.iter().position(
        |e| matches!(e, crate::sim_device::SimEvent::Bypass { node, on } if node == "fx" && *on),
    );
    assert!(
        bypass.is_some() && recall < bypass,
        "the isolation bypass must be written AFTER the last scene recall: {ev:?}"
    );
}

// The intended `presetLevel` must land AFTER the knob write (whose own recall — base or
// scene, inside `set_knobs` — runs the device's level-apply and would revert an earlier
// one) and BEFORE the engage. Written above the `set_knob` call it is silently reverted
// and the capture renders at the level the DEVICE HAS SAVED: the footswitch/scene lanes'
// measured 10.2 dB error. `None` must stay byte-identical to the old behaviour.
#[test]
fn arm_measurement_asserts_the_intended_preset_level_after_the_knobs_recall() {
    let knob = LevelKnob::Block {
        group_id: "G1".into(),
        node_id: "amp".into(),
        parameter_id: "outputLevel".into(),
        scene_slot: None,
    };
    let arm = |intended: Option<f32>| {
        let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(ListIndex(30)).expect("load_preset");
        arm_measurement(
            &mut s,
            &knob,
            0.6,
            &[("G1".to_string(), "fx".to_string(), true)],
            None,
            intended,
        )
        .expect("arm");
        sim.events()
    };

    let ev = arm(Some(0.42));
    let recall = ev
        .iter()
        .rposition(|e| matches!(e, crate::sim_device::SimEvent::LoadScene(_)));
    let level = ev
        .iter()
        .position(|e| matches!(e, crate::sim_device::SimEvent::PresetLevel(_)));
    let bypass = ev.iter().position(
        |e| matches!(e, crate::sim_device::SimEvent::Bypass { node, on } if node == "fx" && *on),
    );
    assert!(
        level.is_some() && recall < level && level < bypass,
        "the intended presetLevel must be written after the knob's recall and before the \
         isolation bypasses: {ev:?}"
    );
    assert!(
        matches!(
            ev.iter().find_map(|e| match e {
                crate::sim_device::SimEvent::PresetLevel(v) => Some(*v),
                _ => None,
            }),
            Some(v) if (v - 0.42).abs() < 1e-6
        ),
        "the asserted level must be the intended one: {ev:?}"
    );

    assert!(
        !arm(None)
            .iter()
            .any(|e| matches!(e, crate::sim_device::SimEvent::PresetLevel(_))),
        "`None` must write no presetLevel at all"
    );
}

// `arm_pair_measurement`'s naked-gap breaker must land a heartbeat strictly between the
// last write and the engage for the zero-write base arm — and ONLY that shape.
//
// WHAT THIS GATE CANNOT SEE: `SimDevice` has no clock, so only the STRUCTURAL fact (a
// heartbeat lands in the gap) is assertable, never the wall-time. Adding a second bare
// settle anywhere else in the choreography re-widens a gap past the proven-safe cadence
// while this assertion still passes.
#[test]
fn arm_pair_measurement_zero_write_base_arm_heartbeats_before_the_naked_engage() {
    let sim = crate::sim_device::SimDevice::new();
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(30)).expect("load_preset");
    arm_pair_measurement(&mut s, None, 0.5, &[]).expect("arm");
    // Stand in for the caller's engage (`measure_pair_at` calls `engage_measure_disengage`
    // next, which opens with exactly this write) — a real audio capture isn't reachable
    // offline, but the ordering fact under test is fully decided before that point.
    s.set_reamp_mode(true).expect("engage");
    let ev = sim.events();
    let last_command = ev
        .iter()
        .rposition(|e| {
            !matches!(
                e,
                crate::sim_device::SimEvent::Heartbeat | crate::sim_device::SimEvent::ReAmp(_)
            )
        })
        .expect("at least one write happened (the presetLevel set)");
    let engage = ev
        .iter()
        .position(|e| matches!(e, crate::sim_device::SimEvent::ReAmp(true)))
        .expect("the engage was sent");
    assert!(
        ev[last_command + 1..engage]
            .iter()
            .any(|e| matches!(e, crate::sim_device::SimEvent::Heartbeat)),
        "a zero-write base measurement must heartbeat between its last write and the \
         engage, or the naked ~600 ms idle gap latches the device's stationary floor: \
         {ev:?}"
    );
}

// Write-bearing base runs are untouched (HW-validated green): each write already breaks
// the idle itself, so the naked-gap breaker must stay off.
#[test]
fn arm_pair_measurement_does_not_heartbeat_when_writes_are_present() {
    let sim = crate::sim_device::SimDevice::new();
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(30)).expect("load_preset");
    arm_pair_measurement(
        &mut s,
        None,
        0.5,
        &[(
            "G1".to_string(),
            "amp".to_string(),
            "outputLevel".to_string(),
            0.4,
        )],
    )
    .expect("arm");
    let ev = sim.events();
    assert!(
        !ev.iter()
            .any(|e| matches!(e, crate::sim_device::SimEvent::Heartbeat)),
        "a write-bearing base measurement already breaks the idle on its own; the \
         naked-shape breaker must stay off here: {ev:?}"
    );
}

// Scene arm untouched: its `set_knob` already sits between two settles with no
// double-settle gap, so it must never gain the base-arm breaker either.
#[test]
fn arm_pair_measurement_scene_arm_never_heartbeats() {
    let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(30)).expect("load_preset");
    arm_pair_measurement(&mut s, Some(2), 0.5, &[]).expect("arm");
    let ev = sim.events();
    assert!(
        !ev.iter()
            .any(|e| matches!(e, crate::sim_device::SimEvent::Heartbeat)),
        "the scene arm's gap is already ≤300 ms; it must not gain the base-arm breaker: \
         {ev:?}"
    );
}

// A `"bypass"` knob value (0.0/1.0-encoded) must route through `change_parameter_bool`
// — the WIRE message is different (`ChangeParameter.boolVal`, field 7) from every other
// block param, so smuggling it through the float `change_parameter` call would set the
// wrong field and silently fail to write bypass at all.
#[test]
fn set_knob_value_only_routes_bypass_through_the_bool_wire_call() {
    let sim = crate::sim_device::SimDevice::new();
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(0)).expect("load_preset");
    let bypass_knob = LevelKnob::Block {
        group_id: "G1".into(),
        node_id: "amp".into(),
        parameter_id: "bypass".into(),
        scene_slot: None,
    };
    set_knob(&mut s, &bypass_knob, 1.0, None).expect("set_knob");
    assert_eq!(
        sim.bypass_write("amp"),
        Some(true),
        "a bypass knob value must land as a BOOL write, not a float dspUnitParameters one"
    );
}
