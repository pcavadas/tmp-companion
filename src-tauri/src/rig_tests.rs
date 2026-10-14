//! Leveling orchestration over a PRIVATE rig (#277): each test drives a real orchestrator —
//! `level_preset_impl`, the scene batch (`level_scenes_oneshot`) and the footswitch batch
//! (`run_footswitch_batch`) — against its own `SimDevice` and capture model, installed on its
//! own thread with `rig::scoped`. No transport factory, no `LIVE` fake, no `SERIAL` lock: they
//! run in parallel under plain `cargo test --lib`, with or without `--features e2e`.
//!
//! The preset is the committed `E2E Rig` fixture (4 scenes, a JC120 and a Twin amp, a Boost
//! on footswitch 2), served at a NON-scenario slot so both builds render the same document.
//! The capture model is the leveller's own law on that document: the active amp's
//! `outputLevel` and `presetLevel` are linear amplitude, the engaged Boost's `gain` is dB.

use std::rc::Rc;

use crate::commands::level_footswitch::{run_footswitch_batch, FootswitchLevelJob};
use crate::commands::ProgressStatus;
use crate::leveller::{self, KnobTarget, LevelKnob, LevelOptions, SceneJob};
use crate::rig;
use crate::session::Session;
use crate::sim_device::{Injection, Rendered, SimDevice, SimEvent, SimRig};
use crate::slot::ListIndex;

/// A non-scenario slot: the e2e build's scenario echo never claims it.
const SLOT: ListIndex = ListIndex(20);
/// The model's loudness with every amplitude control at 1.0 (LUFS).
const AMP_C: f64 = -10.0;
/// The Boost's authored `gain` (dB) — the model's zero point for it.
const BOOST_AUTHORED: f64 = 2.5;
/// The fixture's stored `audioGraph.presetLevel`.
const FIXTURE_PL: f64 = 0.32;
const AMPS: [&str; 2] = ["ACD_JC120", "ACD_TwinReverb65NoFx"];

/// `E2E Rig`'s presetJson, read from the committed fixture.
fn rig_preset_json() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../e2e/fixtures/scenario-presets.json"
    );
    let all: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).expect("scenario fixture")).expect("json");
    all.as_array()
        .and_then(|a| a.iter().find(|p| p["name"] == "E2E Rig"))
        .and_then(|p| p["presetJson"].as_str())
        .expect("E2E Rig presetJson")
        .to_string()
}

/// The capture model: the first un-bypassed amp sets the sound; no active amp is silence.
fn model(r: &mut Rendered) -> Option<f64> {
    let amp = AMPS
        .into_iter()
        .find(|n| r.bypassed("G1", n) == Some(false))?;
    let out = r.param("G1", amp, "outputLevel")?;
    let boost = if r.bypassed("G1", "ACD_Boost") == Some(false) {
        r.param("G1", "ACD_Boost", "gain").unwrap_or(BOOST_AUTHORED) - BOOST_AUTHORED
    } else {
        0.0
    };
    Some(AMP_C + 20.0 * f64::from(r.preset_level()).log10() + 20.0 * out.log10() + boost)
}

/// A fresh fake serving the fixture, and a private rig over it.
fn rig() -> (SimDevice, Rc<SimRig>) {
    let dev = SimDevice::new().with_saved_preset_json(&rig_preset_json());
    let rig = Rc::new(SimRig::new(dev.clone(), model));
    (dev, rig)
}

/// A short non-silent stimulus (0.5 s at 440 Hz, 48 kHz).
fn stim() -> Vec<f32> {
    let rate = 48_000usize;
    (0..rate / 2)
        .map(|i| 0.2 * (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin())
        .collect()
}

/// `danger.md`'s run-end contract, read off the wire: the run engaged at least once, never
/// engaged more often than it disengaged, and its LAST re-amp write was an OFF. Plus the #279
/// gate: the run sent nothing the fake does not model.
fn assert_run_ended_clean(dev: &SimDevice) {
    let ev = dev.events();
    let on = ev.iter().filter(|e| **e == SimEvent::ReAmp(true)).count();
    let off = ev.iter().filter(|e| **e == SimEvent::ReAmp(false)).count();
    assert!(on > 0, "the run never engaged re-amp: {ev:?}");
    assert!(off >= on, "{on} engages but only {off} disengages");
    let last = ev.iter().rev().find_map(|e| match e {
        SimEvent::ReAmp(v) => Some(*v),
        _ => None,
    });
    assert_eq!(last, Some(false), "the run must end with re-amp OFF");
    assert_eq!(
        dev.unmodeled(),
        Vec::<(u32, u32)>::new(),
        "the run sent request kinds the fake does not model"
    );
}

fn saves(dev: &SimDevice) -> usize {
    dev.events()
        .iter()
        .filter(|e| matches!(e, SimEvent::Saved(_)))
        .count()
}

/// `level_preset_impl` measures `C` on its own rig, solves the exact `presetLevel`, verifies
/// it on target and saves it — the three-connection shape, over a private fake.
#[test]
fn level_preset_impl_solves_verifies_and_saves_on_a_private_rig() {
    let (dev, rig) = rig();
    let target = -25.0;
    let opts = LevelOptions {
        save: true,
        verify: true,
        ..Default::default()
    };
    let stim = stim();
    let r = rig::scoped(rig.clone(), || {
        leveller::level_preset_impl(
            SLOT,
            &stim,
            target,
            opts,
            &[],
            None,
            leveller::BoostContext::default(),
            || false,
        )
    })
    .expect("level_preset_impl");

    // Base renders the JC120 at its authored 0.35 and the Boost at its authored gain.
    let c = AMP_C + 20.0 * 0.35f64.log10();
    assert!((r.constant_c - c).abs() < 0.1, "C {c:.2}: {r:?}");
    let want = 10f64.powf((target - c) / 20.0) as f32;
    assert!(
        (r.final_level - want).abs() < 0.01,
        "solved {want:.4}: {r:?}"
    );
    let v = r.verify_lufs.expect("a verify capture");
    assert!((v - target).abs() < 0.25, "verified on target: {r:?}");
    assert!(r.saved && !r.clamped, "{r:?}");
    assert!(rig.captures() >= 2, "a reference and a verify capture");
    assert_eq!(saves(&dev), 1, "one save");
    let last_level = dev.events().iter().rev().find_map(|e| match e {
        SimEvent::PresetLevel(v) => Some(*v),
        _ => None,
    });
    assert!(
        last_level.is_some_and(|l| (l - r.final_level).abs() < 1e-6),
        "the solved level is the last one written: {last_level:?}"
    );
    assert_run_ended_clean(&dev);
}

/// A Stop pressed on THIS rig cancels the run before anything moves, and no other rig sees it.
#[test]
fn a_stop_on_one_rig_cancels_only_that_rig() {
    let (dev, rig) = rig();
    rig.request_abort();
    let stim = stim();
    let r = rig::scoped(rig.clone(), || {
        leveller::level_preset_impl(
            SLOT,
            &stim,
            -25.0,
            LevelOptions::default(),
            &[],
            None,
            leveller::BoostContext::default(),
            || false,
        )
    });
    assert!(
        matches!(&r, Err(e) if e.is_cancelled()),
        "a Stop is a cancel: {r:?}"
    );
    assert_eq!(rig.captures(), 0, "nothing was captured");
    assert!(
        !dev.events().contains(&SimEvent::ReAmp(true)),
        "a run stopped before its first capture never engages"
    );
    assert!(!crate::device_gate::op_abort_requested());
}

/// The scene batch levels two scenes' amps, each in its own scene, defers both writes and
/// persists them with ONE save.
#[test]
fn the_scene_batch_levels_each_scene_and_saves_once_on_a_private_rig() {
    let (dev, rig) = rig();
    let preset: serde_json::Value = serde_json::from_str(&rig_preset_json()).expect("json");
    let target = -26.0;
    let job = |scene: u32, amp: &str, current: f32| SceneJob {
        scene_slot: scene,
        target_lufs: target,
        knobs: vec![KnobTarget {
            knob: LevelKnob::Block {
                group_id: "G1".into(),
                node_id: amp.into(),
                parameter_id: "outputLevel".into(),
                scene_slot: Some(scene),
            },
            lo: 0.0,
            hi: 1.0,
            current,
        }],
        skip: None,
        rebalanceable: false,
        handle: None,
        prepass: None,
        force_bypass: Vec::new(),
    };
    // Scene 0 plays the JC120 at 0.35, scene 1 the Twin at 0.45.
    let jobs = [job(0, AMPS[0], 0.35), job(1, AMPS[1], 0.45)];
    let stim = stim();
    let mut seen = Vec::new();
    let outcomes = rig::scoped(rig.clone(), || {
        leveller::make_current(SLOT)?;
        leveller::level_scenes_oneshot(
            SLOT,
            &jobs,
            &stim,
            true,
            None,
            Some(&preset),
            None,
            &[],
            |scene, done| {
                if done.is_some() {
                    seen.push(scene);
                }
            },
            |_| {},
            || false,
        )
        .map_err(|e| e.to_string())
    })
    .expect("scene batch");

    assert_eq!(seen, vec![0, 1], "each scene reported in order");
    assert_eq!(outcomes.len(), 2);
    for o in &outcomes {
        assert!(o.failure.is_none() && !o.clamped, "{o:?}");
        let lufs = o.final_lufs.expect("a measured result");
        assert!((lufs - target).abs() < 0.25, "on target: {o:?}");
        // Linear-amplitude fader: the exact solve for this model.
        let want = 10f64.powf((target - AMP_C - 20.0 * FIXTURE_PL.log10()) / 20.0) as f32;
        let level = o.final_level.expect("a solved level");
        assert!((level - want).abs() < 0.01, "solved {want:.4}: {o:?}");
        assert!(o.writes > 0, "{o:?}");
    }
    // Each write landed in its own scene's overlay, never on base.
    for (scene, amp) in [(0i64, AMPS[0]), (1, AMPS[1])] {
        let w = dev.param_write(scene, "G1", amp, "outputLevel");
        assert!(w.is_some(), "scene {scene}'s {amp} was written: {w:?}");
        assert_eq!(
            dev.param_write(crate::sim_device::SCENE_BASE, "G1", amp, "outputLevel"),
            None,
            "a scene write must not land on base"
        );
    }
    assert_eq!(saves(&dev), 1, "the batch persists with ONE save");
    assert_run_ended_clean(&dev);
}

/// A scene stopped after it wrote a probe value is put back before the batch save: Stop is
/// pressed during scene 1's verify capture (the first at a fader other than the Twin's authored
/// 0.45), which reads off target, so the correction capture is cancelled. The cancel path's ONE
/// save must persist scene 0's solve and scene 1 as the
/// player left it, not the probe.
#[test]
fn a_stopped_scene_is_rolled_back_before_the_batch_save() {
    let dev = SimDevice::new().with_saved_preset_json(&rig_preset_json());
    let me: Rc<std::cell::OnceCell<std::rc::Weak<SimRig>>> = Rc::default();
    let me2 = me.clone();
    let rig = Rc::new(SimRig::new(dev.clone(), move |r| {
        let twin_on =
            r.bypassed("G1", AMPS[1]) == Some(false) && r.bypassed("G1", AMPS[0]) != Some(false);
        if twin_on && (r.param("G1", AMPS[1], "outputLevel")? - 0.45).abs() > 1e-3 {
            if let Some(rig) = me2.get().and_then(std::rc::Weak::upgrade) {
                rig.request_abort();
            }
            // 2 LU off the open-loop solve, so the verify asks for a correction capture,
            // which the Stop then cancels.
            return model(r).map(|l| l + 2.0);
        }
        model(r)
    }));
    me.set(Rc::downgrade(&rig)).expect("set once");
    let preset: serde_json::Value = serde_json::from_str(&rig_preset_json()).expect("json");
    let job = |scene: u32, amp: &str, current: f32| SceneJob {
        scene_slot: scene,
        target_lufs: -26.0,
        knobs: vec![KnobTarget {
            knob: LevelKnob::Block {
                group_id: "G1".into(),
                node_id: amp.into(),
                parameter_id: "outputLevel".into(),
                scene_slot: Some(scene),
            },
            lo: 0.0,
            hi: 1.0,
            current,
        }],
        skip: None,
        rebalanceable: false,
        handle: None,
        prepass: None,
        force_bypass: Vec::new(),
    };
    let jobs = [job(0, AMPS[0], 0.35), job(1, AMPS[1], 0.45)];
    let stim = stim();
    let r = rig::scoped(rig.clone(), || {
        leveller::make_current(SLOT)?;
        leveller::level_scenes_oneshot(
            SLOT,
            &jobs,
            &stim,
            true,
            None,
            Some(&preset),
            None,
            &[],
            |_, _| {},
            |_| {},
            || false,
        )
        .map_err(|e| e.to_string())
    });
    assert!(r.is_err(), "a Stop ends the batch as a cancel: {r:?}");
    let twin = dev
        .param_write(1, "G1", AMPS[1], "outputLevel")
        .expect("scene 1's Twin was written");
    assert!(
        (twin - 0.45).abs() < 1e-4,
        "scene 1's Twin must be back at its authored 0.45, not a probe value: {twin}"
    );
    let jc = dev
        .param_write(0, "G1", AMPS[0], "outputLevel")
        .expect("scene 0's JC120 was written");
    assert!((jc - 0.35).abs() > 0.01, "scene 0 keeps its solve: {jc}");
    assert_eq!(
        saves(&dev),
        1,
        "the stopped batch still persists with ONE save"
    );
    assert_run_ended_clean(&dev);
}

/// The footswitch batch reads the saved document, reads the row's ceiling, solves the Boost's
/// gain onto the target, and — on a dry run — writes nothing and reloads.
#[test]
fn the_footswitch_batch_solves_its_row_on_a_private_rig() {
    let (dev, rig) = rig();
    let target = -25.0;
    let jobs = [FootswitchLevelJob {
        switch: 2,
        lev_group_id: "G1".into(),
        lev_node_id: "ACD_Boost".into(),
        lev_parameter_id: "gain".into(),
        target_lufs: target,
        scene_context: None,
    }];
    let stim = stim();
    let mut progress = Vec::new();
    let rows = rig::scoped(rig.clone(), || {
        run_footswitch_batch(
            SLOT,
            &jobs,
            false,
            &stim,
            0.0,
            &mut |item| progress.push(item),
            &|| false,
        )
    })
    .expect("footswitch batch");

    assert_eq!(rows.len(), 1, "{rows:?}");
    let r = &rows[0];
    assert_eq!(r.switch, 2);
    assert!(r.clamp_reason.is_none() && !r.clamped, "{r:?}");
    // Base at the stored level with the JC120 at 0.35; the gain is dB on top of that.
    let base = AMP_C + 20.0 * FIXTURE_PL.log10() + 20.0 * 0.35f64.log10();
    let want = BOOST_AUTHORED + (target - base);
    assert!(
        (f64::from(r.final_value) - want).abs() < 0.3,
        "solved gain {want:.2}: {r:?}"
    );
    assert!((r.predicted_lufs - target).abs() < 0.3, "{r:?}");
    assert!(!r.saved, "a dry run saves nothing");
    assert_eq!(saves(&dev), 0, "a dry run never saves");

    let statuses: Vec<ProgressStatus> = progress.iter().map(|p| p.status).collect();
    assert_eq!(
        statuses,
        vec![
            ProgressStatus::Active,
            ProgressStatus::Active,
            ProgressStatus::Done
        ],
        "prepass, solve, result"
    );
    assert_eq!(
        progress[0].message.as_deref(),
        Some(leveller::PREPASS_ACTIVE_MSG),
        "the prepass announces itself"
    );
    assert!(progress.iter().all(|p| p.switch == 2));
    assert!(progress[2].result.is_some());
    assert_run_ended_clean(&dev);
}

/// The working copy's chain as `group/node/model`, read the way the UI's "now playing" strip
/// and the Doctor's live-node check read it.
fn live_chain() -> Vec<String> {
    let (graph, _) = crate::discover_active_graph().expect("discover_active_graph");
    graph
        .nodes
        .iter()
        .map(|n| format!("{}/{}/{}", n.group_id, n.node_id, n.model))
        .collect()
}

/// BUG→GATE (online e2e `doctor-apply.online.spec.ts`, the post-discard read; HW 2026-10-14,
/// fw 1.8.58, 3/3 at a 100 ms settle): a Doctor discard read back the block it had just
/// thrown away. The unit keeps sending a session's queued replies after the host closes it
/// and hands what is left to the next session inside the 0.75 s drop, ahead of that
/// session's own; the restore's lean session closed before its handshake copy of the DIRTY
/// working copy was sent, so the next read got it first and longest-wins kept it. Both
/// fixes are pinned: every restore and isolated load now ends with a lapse, so the unit
/// discards that queue (`queued_flood_reports` is empty), and the read takes the latest
/// copy (`latest_json_payload_reads_past_a_longer_inherited_copy`).
#[test]
fn a_discarded_edit_never_reads_back_from_the_queued_handshake_copy() {
    let (dev, rig) = rig();
    assert_eq!(dev.arm(Injection::QueuedHandshakeFlood), "reachable");
    let (saved, applied, restored) = rig::scoped(rig.clone(), || {
        leveller::make_current(SLOT).expect("make_current");
        let saved = live_chain();
        let mut s = Session::connect().expect("connect");
        assert!(
            s.insert_node("G1", None, "ACD_TenBandEQStereo")
                .expect("insert_node"),
            "the fake confirms the insert"
        );
        drop(s);
        let applied = live_chain();
        leveller::restore_saved_preset(SLOT).expect("restore_saved_preset");
        assert_eq!(
            dev.queued_flood_reports(),
            None,
            "the restore must leave no queued replies for the next session"
        );
        (saved, applied, live_chain())
    });
    assert_eq!(
        applied.len(),
        saved.len() + 1,
        "the applied block reads back before the discard: {applied:?}"
    );
    assert_eq!(restored, saved, "the discard restores the saved chain");
}

/// The injection's own contract, so the gate above cannot pass on a fake that never queues:
/// a lean session closed straight after its handshake leaves its field-3 copy queued, the
/// next connection receives it ahead of its own replies, and a lapse instead discards it.
#[test]
fn the_queued_handshake_copy_reaches_the_next_session_unless_it_lapses() {
    let (dev, rig) = rig();
    dev.arm(Injection::QueuedHandshakeFlood);
    rig::scoped(rig.clone(), || {
        drop(Session::connect_lean().expect("connect_lean"));
        assert!(
            dev.queued_flood_reports().is_some_and(|n| n > 0),
            "a lean session's field-3 copy is still queued when it closes"
        );
        // The next connection gets it with its very first reply.
        let s = Session::connect_lean().expect("connect_lean");
        assert!(
            s.json_payload_len() > 0,
            "the inherited copy lands on the next session's handshake"
        );
        drop(s);
        let mut s = Session::connect_lean().expect("connect_lean");
        s.lapse().expect("lapse");
        drop(s);
        assert_eq!(
            dev.queued_flood_reports(),
            None,
            "a lapse past the drop leaves nothing for the next session"
        );
        leveller::load_preset_isolated(SLOT, leveller::CancelMode::Cleanup)
            .expect("load_preset_isolated");
        assert_eq!(
            dev.queued_flood_reports(),
            None,
            "an isolated load ends with the lapse too"
        );
    });
}
