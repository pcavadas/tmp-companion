//! Tests for the reordered run: the prepass, the headroom trade and the redistribution
//! ordering.

use super::*;
use crate::headroom_trade::ClampKind;

fn knob(node: &str, scene: Option<u32>, current: f32) -> KnobTarget {
    KnobTarget {
        knob: LevelKnob::Block {
            group_id: "G1".into(),
            node_id: node.into(),
            parameter_id: "outputLevel".into(),
            scene_slot: scene,
        },
        lo: 0.0,
        hi: 1.0,
        current,
    }
}

fn job(scene_slot: u32, target: f64, knobs: Vec<KnobTarget>) -> SceneJob {
    SceneJob {
        scene_slot,
        target_lufs: target,
        knobs,
        skip: None,
        rebalanceable: false,
        handle: None,
        prepass: None,
        force_bypass: Vec::new(),
    }
}

fn measured(job: SceneJob, asis: f64, spread: f64) -> SceneJob {
    SceneJob {
        prepass: Some(ScenePrepass { asis, spread }),
        ..job
    }
}

// THE REORDER'S LOAD-BEARING PROPERTY: a job that already carries its prepass reading
// must resolve its prologue from that reading and NOT touch the device. There is no
// hardware in a unit test, so a prologue that still measured would fail to connect —
// this passing IS the proof the capture was skipped, and the values pass through
// verbatim.
#[test]
fn a_job_carrying_a_prepass_reading_resolves_without_measuring() {
    let j = measured(job(2, -18.0, vec![knob("amp", Some(2), 0.5)]), -21.25, 4.5);
    let p = scene_prologue(&j, &[], None).expect("no device work");
    assert!((p.asis - -21.25).abs() < 1e-9, "{}", p.asis);
    assert!((p.spread - 4.5).abs() < 1e-9);
}

// The amp-fader ceiling IS an exact extrapolation: `outputLevel` is linear in dB with
// full authority (HW), so the top of the range is `asis + 20·log10(LEVEL_MAX / max_cur)`
// — the same `k_cap` the joint-k solver clamps to, so the two cannot disagree.
#[test]
fn the_scene_ceiling_extrapolates_the_fader_to_its_top_bound() {
    let j = measured(job(0, -20.0, vec![knob("amp", Some(0), 0.5)]), -26.0, 2.0);
    let c = scene_ceiling_lufs(&j).expect("an amp row has a ceiling");
    assert!((c - (-26.0 + 6.0206)).abs() < 1e-3, "{c}");
    // A fader already at the top has no headroom left: the ceiling IS the as-is reading.
    let maxed = measured(job(0, -20.0, vec![knob("amp", Some(0), 1.0)]), -26.0, 2.0);
    assert!((scene_ceiling_lufs(&maxed).expect("ceiling") - -26.0).abs() < 1e-9);
    // A parallel merge is bounded by the LOUDEST lane (the first to hit the cap).
    let merge = measured(
        job(
            0,
            -20.0,
            vec![knob("a", Some(0), 0.5), knob("b", Some(0), 0.25)],
        ),
        -26.0,
        2.0,
    );
    assert!((scene_ceiling_lufs(&merge).expect("ceiling") - (-26.0 + 6.0206)).abs() < 1e-3);
}

// A USER-HANDLE row answers `None` ON PURPOSE: an arbitrary block param has no
// algebraically predictable response, so extrapolating one would be exactly the taper
// model this codebase refuses to build. Same for a skip job and an unmeasured job.
#[test]
fn a_handle_row_a_skip_row_and_an_unmeasured_row_report_no_ceiling() {
    let mut handle_row = measured(job(1, -20.0, vec![knob("pedal", Some(1), 0.5)]), -26.0, 2.0);
    handle_row.handle = Some(FsParamTarget::new("ACD_Plumes", "level", 0.5));
    assert_eq!(scene_ceiling_lufs(&handle_row), None);

    let mut skip = measured(job(1, -20.0, Vec::new()), -26.0, 2.0);
    skip.skip = Some("no active amp".into());
    assert_eq!(scene_ceiling_lufs(&skip), None);

    assert_eq!(
        scene_ceiling_lufs(&job(1, -20.0, vec![knob("amp", Some(1), 0.5)])),
        None,
        "no prepass reading ⇒ no ceiling to report"
    );
}

// AFTER A TRADE, the two halves of the preset move differently and the re-target must
// reflect exactly that: a benefiting sound gained the raise EXACTLY (module header), while
// every other reading routed through the base FADER — not algebraically predictable
// (module header) — and is therefore DROPPED so its own solve re-measures rather than
// trusting a guess.
#[test]
fn a_landed_trade_shifts_benefiting_readings_and_drops_the_rest() {
    let mut jobs = vec![
        measured(job(0, -18.0, vec![knob("amp", Some(0), 1.0)]), -24.0, 2.0),
        measured(job(1, -18.0, vec![knob("amp", Some(1), 0.5)]), -20.0, 2.0),
        measured(
            job(
                crate::session::BASE_SCENE_SLOT,
                -18.0,
                vec![knob("amp", None, 0.8)],
            ),
            -18.0,
            2.0,
        ),
    ];
    retarget_prepass_after_trade(&mut jobs, 4.0, |sc| sc == 0);
    assert_eq!(
        jobs[0].prepass.map(|p| p.asis),
        Some(-20.0),
        "the benefiting scene gained EXACTLY the raise"
    );
    assert_eq!(
        jobs[0].prepass.map(|p| p.spread),
        Some(2.0),
        "a pure gain shift leaves the dynamics spread alone"
    );
    assert_eq!(
        jobs[1].prepass, None,
        "a net-zero scene's reading routed through the fader — it must be re-measured"
    );
    assert_eq!(
        jobs[2].prepass, None,
        "base is never kept: its own hold already verified it at the new level"
    );
}

// THE CLAMP TAXONOMY, in one table. Order is load-bearing — a routing failure outranks a
// wet floor, which outranks an ordinary headroom clamp — and an unclamped row never
// names a cause at all.
#[test]
fn the_clamp_taxonomy_names_one_cause_per_shape() {
    assert_eq!(ClampKind::from_flags(false, false, None), None);
    assert_eq!(
        ClampKind::from_flags(false, true, Some("no signal on USB 1/2")),
        None,
        "not clamped ⇒ no cause, whatever the other flags say"
    );
    assert_eq!(
        ClampKind::from_flags(true, false, None),
        Some(ClampKind::SceneCeiling)
    );
    assert_eq!(
        ClampKind::from_flags(true, true, None),
        Some(ClampKind::WetFloor)
    );
    assert_eq!(
        ClampKind::from_flags(true, true, Some("no signal on USB 1/2")),
        Some(ClampKind::NoAuthority),
        "a sound that never reached USB 1/2 did not run out of wet floor"
    );
}

// ⟦A1⟧ AN AUTHOR-MUTED BASE LANE IS NOT A FLOOR PIN. A preset whose base carries a muted
// amp (`outputLevel = 0`, the player's own choice) has a lane sitting at/below
// `BASE_FADER_FLOOR` in EVERY solve — `joint_k_floor` deliberately lets it ride and the
// joint factor never moves it. A hold that then stalls MID-RANGE (the bounded secant out
// of captures, every audible lane still well inside its range) must report
// `PartialTrade`: calling it `TradeFloor` both words the wrong cause on the wire and
// spends the one bounded re-plan chasing fader room that was never the problem.
#[test]
fn a_muted_base_lane_does_not_turn_a_mid_range_stall_into_a_floor_pin() {
    use crate::headroom_trade::BASE_FADER_FLOOR;
    // The solve's own direction-aware answer: the muted lane was never MOVED to the floor,
    // and the audible lane sits mid-range — so no bound pinned this solve.
    let base = [0.0f32, 0.6];
    let pinned = joint_levels_pinned(
        &[0.0, 0.4],
        &base,
        BASE_FADER_FLOOR,
        -20.0, // achieved
        -23.0, // target: QUIETER than achieved, i.e. the direction the fader blocks
    );
    assert_eq!(
        pinned, None,
        "an author-muted lane is not a solved floor pin"
    );
    assert_eq!(
        trade_hold_failure_kind(Some(ClampKind::SceneCeiling), pinned),
        ClampKind::PartialTrade,
        "a mid-range stall backs the pair out; it did not run out of base fader"
    );
    // The GENUINE floor pin still reports itself — the fix must not blind the one retry.
    let floored = joint_levels_pinned(
        &[0.0, BASE_FADER_FLOOR],
        &base,
        BASE_FADER_FLOOR,
        -20.0,
        -23.0,
    );
    assert_eq!(floored, Some(PinnedBound::Floor));
    assert_eq!(
        trade_hold_failure_kind(Some(ClampKind::SceneCeiling), floored),
        ClampKind::TradeFloor
    );
    // Routing outranks everything: a sound that never reached USB 1/2 is no floor case.
    assert_eq!(
        trade_hold_failure_kind(Some(ClampKind::NoAuthority), floored),
        ClampKind::NoAuthority
    );
    // A TOP pin is not something a base hold can pay for either — it backs out.
    let maxed = joint_levels_pinned(&[1.0], &[0.6], BASE_FADER_FLOOR, -30.0, -23.0);
    assert_eq!(maxed, Some(PinnedBound::Max));
    assert_eq!(
        trade_hold_failure_kind(Some(ClampKind::SceneCeiling), maxed),
        ClampKind::PartialTrade
    );
}

// A prepass-decided clamp reports the LOUDEST the sound can actually be, at the handle
// value that produced it, with the shared cause — and writes nothing.
#[test]
fn a_ceiling_clamp_reports_the_measured_ceiling_and_writes_nothing() {
    let handle = FsParamTarget::new("ACD_Plumes", "level", 0.5);
    let ceiling = FsCeiling {
        ceiling_lufs: -25.4,
        spread_lu: 3.1,
        unreachable: true,
    };
    let r = fs_result_from_ceiling(7, -18.0, &handle, &ceiling, "baked");
    assert!(r.clamped);
    assert_eq!(r.clamp_kind, Some(ClampKind::SceneCeiling));
    assert_eq!(r.clamp_reason, None, "a headroom clamp is reason-less");
    assert!(!r.wet_floor);
    assert!(!r.unconverged, "nothing was searched, so nothing stalled");
    assert!(!r.saved);
    assert_eq!(r.predicted_lufs, -25.4);
    assert_eq!(r.final_value, handle.bounds().1, "the handle's top bound");
    assert_eq!(r.iterations, 1, "the ONE prepass capture it rests on");
}

// THE CEILING PROBE'S PURE HALF. The capture puts the LEVELING HANDLE at the top of its
// own (classified, wet-floor-aware) range and writes it LAST, so it wins over any `param`
// function of the same switch addressing the same control — the ceiling is the handle at
// its top, by definition. Every other function still rides at its ENGAGED (`valueA`) value,
// because the ceiling has to describe the sound the switch actually makes.
#[test]
fn the_ceiling_probe_pins_the_handle_at_its_top_bound_and_writes_it_last() {
    let handle = FsParamTarget::new("ACD_Plumes", "level", 0.5);
    let states = crate::footswitch::SwitchStates {
        engaged_bypass: vec![("G1".into(), "ACD_Plumes".into(), false)],
        disengaged_bypass: vec![("G1".into(), "ACD_Plumes".into(), true)],
        params: vec![
            // A function on ANOTHER control rides at its engaged value…
            ("G1".into(), "ACD_Boost".into(), "gain".into(), 2.5, 0.0),
            // …and one on the HANDLE ITSELF is dropped: the top bound replaces it.
            ("G1".into(), "ACD_Plumes".into(), "level".into(), 0.4, 0.2),
        ],
    };
    let probe = FsCeilingProbe {
        scene: Some(2),
        states: &states,
        handle: ("G1".into(), "ACD_Plumes".into(), handle.clone()),
    };
    let params = probe.ceiling_params();
    let (_, hi) = handle.bounds();
    assert_eq!(
        params.last().map(|p| (p.1.as_str(), p.2.as_str(), p.3)),
        Some(("ACD_Plumes", "level", hi)),
        "the handle write is appended LAST, at the top of its range: {params:?}"
    );
    assert_eq!(
        params
            .iter()
            .filter(|(_, n, p, _)| n == "ACD_Plumes" && p == "level")
            .count(),
        1,
        "the switch's own function on the handle is replaced, not duplicated: {params:?}"
    );
    assert!(
        params
            .iter()
            .any(|(_, n, p, v)| n == "ACD_Boost" && p == "gain" && *v == 2.5),
        "every OTHER function keeps its ENGAGED value: {params:?}"
    );
}

// The skip margin is deliberately much wider than the lane's acceptance band: a ceiling
// read at handle-max can clip, and a FALSE clamp is a silent product bug while a
// needlessly-solved row is only slow.
#[test]
fn the_ceiling_skip_margin_leaves_marginal_rows_to_the_solve() {
    // The margin is wider than the FS lane's acceptance band by construction, so a row
    // that merely misses `ACCEPT_TOL_LU` still gets its solve.
    assert!(!fs_target_beyond_ceiling(-25.0, -25.0 + ACCEPT_TOL_LU));
    assert!(!fs_target_beyond_ceiling(
        -25.0,
        -25.0 + FS_CEILING_SKIP_MARGIN_LU
    ));
    assert!(fs_target_beyond_ceiling(
        -25.0,
        -25.0 + FS_CEILING_SKIP_MARGIN_LU + 0.01
    ));
    // A target BELOW the ceiling is always reachable, and a non-finite read never
    // decides anything (the solve takes over).
    assert!(!fs_target_beyond_ceiling(-25.0, -40.0));
    assert!(!fs_target_beyond_ceiling(f64::NEG_INFINITY, -18.0));
}
