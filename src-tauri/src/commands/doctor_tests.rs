//! Unit tests for the Doctor command layer (moved from lib.rs `audition_tests`).
use super::*;
use crate::doctor;
use crate::error::SkipKind;

/// BUG→GATE (the silent-wrong-diagnosis class): a large preset's field-8 read is cut
/// before its `ftsw` section, so the isolation fallback has no footswitch assignments.
/// A FOOTSWITCH sound is DEFINED by those assignments — with none, the capture engages
/// nothing and diagnoses the base sound under the switch's name. That sound must be
/// reported as skipped, not measured. A BASE sound keeps the documented best-effort
/// degrade: its isolation only silences other switches, so a missing one shifts the
/// baseline without misnaming the sound.
///
/// Drives `resolve_sound_isolation` through its empty-graph branch with the cache
/// pre-seeded, so no device read happens.
#[test]
fn an_unreadable_ftsw_skips_a_footswitch_sound_but_not_a_base_sound() {
    let unreadable = |footswitch: Option<u32>| {
        let mut cache = std::collections::HashMap::new();
        // What the fallback read leaves behind when the body could not be read or its
        // `ftsw` tail never arrived.
        cache.insert(ListIndex(7), serde_json::Value::Null);
        resolve_sound_isolation(&[], &[], &[], None, footswitch, ListIndex(7), &mut cache)
    };
    let fs = unreadable(Some(2));
    let msg = fs.unresolved.expect("a footswitch sound must be skipped");
    assert!(msg.contains("footswitch 3"), "1-based switch label: {msg}");
    assert!(msg.contains("too large to read over USB"), "{msg}");

    assert!(
        unreadable(None).unresolved.is_none(),
        "a base sound still degrades to no isolation rather than erroring"
    );

    // A readable `ftsw` resolves normally — the guard must not misfire on a preset whose
    // switches are simply all empty (a legitimate saved state).
    let mut cache = std::collections::HashMap::new();
    cache.insert(ListIndex(7), serde_json::json!({ "ftsw": [[], [], []] }));
    assert!(
        resolve_sound_isolation(&[], &[], &[], None, Some(2), ListIndex(7), &mut cache)
            .unresolved
            .is_none()
    );
}

/// A DoctorInput node payload WITHOUT `params` (pre-params frontend) and one
/// WITH it both deserialize — `#[serde(default)]` keeps the wire
/// backward-compatible.
#[test]
fn doctor_node_params_are_optional_on_the_wire() {
    let json = r#"{
            "key": "p4", "listIndex": 4, "scene": null, "label": "Lead",
            "tag": null, "topologyId": null, "calibrationLufs": null,
            "nodes": [
                { "group_id": "G1", "node_id": "n1", "model": "ACD_TMLargeRoom" },
                { "group_id": "G1", "node_id": "n2", "model": "ACD_TenBandEQStereo",
                  "params": { "gain250hz": 2.0 } }
            ]
        }"#;
    let input: DoctorInput = serde_json::from_str(json).expect("DoctorInput deserializes");
    assert!(input.nodes[0].params.is_empty());
    assert_eq!(input.nodes[1].params.get("gain250hz"), Some(&2.0));
}

/// `footswitch` is optional on the wire (implicit serde Option-missing = None,
/// same as `scene`) and echoes through the result row unchanged.
#[test]
fn doctor_footswitch_is_optional_and_echoes_to_result() {
    // Absent → None (backward-compatible wire).
    let bare = r#"{ "key": "p4", "listIndex": 4, "label": "Base" }"#;
    let base: DoctorInput = serde_json::from_str(bare).expect("DoctorInput deserializes");
    assert_eq!(base.footswitch, None);
    // Present → Some, and the result row carries it through.
    let fs = r#"{ "key": "f4:0", "listIndex": 4, "footswitch": 0, "label": "FS1" }"#;
    let input: DoctorInput = serde_json::from_str(fs).expect("DoctorInput deserializes");
    assert_eq!(input.footswitch, Some(0));
    let row = sound_result(
        &input,
        doctor::StimulusKind::Synthetic,
        &SoundOutcome::Unrun,
    );
    let v = serde_json::to_value(&row).unwrap();
    assert_eq!(v["footswitch"], 0);
    // cutThrough serializes as an explicit null (never an omitted key) when
    // this sound has no estimate — errored sounds, degenerate ratios.
    assert_eq!(v["cutThrough"], serde_json::Value::Null);
}

/// `DoctorSoundResult.cutThrough` carries the estimate's three fields
/// verbatim, camelCase, when present.
#[test]
fn doctor_sound_result_cut_through_serializes_camel_case() {
    let base: DoctorInput =
        serde_json::from_str(r#"{ "key": "p4", "listIndex": 4, "label": "Base" }"#).unwrap();
    let mut row = sound_result(&base, doctor::StimulusKind::Synthetic, &SoundOutcome::Unrun);
    row.cut_through = Some(doctor::CutThrough {
        contrast_db: 12.5,
        factory_percentile: Some(63.2),
        advisory: false,
    });
    let v = serde_json::to_value(&row).unwrap();
    assert_eq!(v["cutThrough"]["contrastDb"], 12.5);
    assert_eq!(v["cutThrough"]["factoryPercentile"], 63.2);
    assert_eq!(v["cutThrough"]["advisory"], false);
}

/// A failed sound carries its failure's typed kind beside the text, so the screen words it
/// by kind (a silent capture is refined by the preset's silence hint) and never reads the
/// text. A measured or unrun sound has neither.
#[test]
fn a_failed_sound_carries_its_typed_kind_on_the_wire() {
    let base: DoctorInput =
        serde_json::from_str(r#"{ "key": "p4", "listIndex": 4, "label": "Base" }"#).unwrap();
    let failed = sound_result(
        &base,
        doctor::StimulusKind::Synthetic,
        &SoundOutcome::Failed(
            LevelError::NoSignal.to_string(),
            LevelError::NoSignal.skip_kind(),
        ),
    );
    let v = serde_json::to_value(&failed).unwrap();
    assert_eq!(v["status"], "error");
    assert_eq!(v["errorKind"], "no_signal");
    assert_eq!(v["error"], "no signal captured");
    let unrun = sound_result(&base, doctor::StimulusKind::Synthetic, &SoundOutcome::Unrun);
    let v = serde_json::to_value(&unrun).unwrap();
    assert_eq!(v["errorKind"], serde_json::Value::Null);
}

/// A silent capture (a footswitch that routes to OUT2, an amp at zero) reaches Results as
/// the typed no-signal kind, so the row says why from the preset's silence hint. Before,
/// the analysis refused it with untyped text and the row could only say "Couldn't check
/// this sound."
#[test]
fn a_silent_capture_is_the_typed_no_signal_kind() {
    let silent = crate::lufs::measure_mono(&vec![0.0_f32; 48_000 * 2], 48_000).unwrap();
    assert!(!silent.integrated_lufs.is_finite());
    let err = audible_or_no_signal(&silent).unwrap_err();
    assert_eq!(err.skip_kind(), crate::error::SkipKind::NoSignal);

    let tone: Vec<f32> = (0..48_000 * 2)
        .map(|i| 0.25 * (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin())
        .collect();
    let audible = crate::lufs::measure_mono(&tone, 48_000).unwrap();
    assert!(audible_or_no_signal(&audible).is_ok());
}

// ── doctor_force_bypass: isolation force-list per sound (base / footswitch) ──

/// A preset with three block-acting switches (switch 0 → DRIVE, switch 1 → MOD —
/// both OFF in base; switch 2 → BD2, saved ON in base with `isActive:true` — the
/// preset-024 "saved with the switch engaged" shape) plus a shared CAB block no
/// switch touches. The exact JSON shape is what `all_onoff_blocks` /
/// `siblings_off_excluding` / `engaged_bypass_for_switch` parse
/// (`ftsw`=array-of-switches, on-off assign = `{func,nodes:[{groupId,nodeId}]}`).
fn force_bypass_fixture() -> serde_json::Value {
    serde_json::json!({
        "audioGraph": { "guitarNodes": { "G1": [
            { "nodeId": "DRV", "FenderId": "DRV", "dspUnitParameters": { "bypass": true } },
            { "nodeId": "MOD", "FenderId": "MOD", "dspUnitParameters": { "bypass": true } },
            { "nodeId": "BD2", "FenderId": "BD2", "dspUnitParameters": { "bypass": false } },
            { "nodeId": "CAB", "FenderId": "CAB", "dspUnitParameters": { "bypass": false } }
        ]}, "micNodes": {} },
        "ftsw": [
            [{ "func": "on-off", "nodes": [{ "groupId": "G1", "nodeId": "DRV" }], "isActive": false }],
            [{ "func": "on-off", "nodes": [{ "groupId": "G1", "nodeId": "MOD" }], "isActive": false }],
            [{ "func": "on-off", "nodes": [{ "groupId": "G1", "nodeId": "BD2" }], "isActive": true }],
        ]
    })
}

#[test]
fn doctor_force_bypass_base_forces_all_onoff_blocks_off() {
    let p = force_bypass_fixture();
    let out = doctor_force_bypass(&p["ftsw"], &p, None);
    // Every switch's on/off block forced off (bypass=true) — including one the
    // preset was SAVED with engaged; shared CAB absent.
    assert!(out.contains(&("G1".into(), "DRV".into(), true)));
    assert!(out.contains(&("G1".into(), "MOD".into(), true)));
    assert!(out.contains(&("G1".into(), "BD2".into(), true)));
    assert!(!out.iter().any(|(_, n, _)| n == "CAB"));
    assert_eq!(out.len(), 3);
}

#[test]
fn doctor_force_bypass_footswitch_forces_own_on_others_off() {
    let p = force_bypass_fixture();
    let out = doctor_force_bypass(&p["ftsw"], &p, Some(0));
    // Switch 0's own DRV forced ON (saved off + isActive:false → engaged is the
    // flip), the other switches' blocks off.
    assert!(out.contains(&("G1".into(), "DRV".into(), false)));
    assert!(out.contains(&("G1".into(), "MOD".into(), true)));
    assert!(out.contains(&("G1".into(), "BD2".into(), true)));
    assert!(!out.iter().any(|(_, n, _)| n == "CAB"));
    assert_eq!(out.len(), 3, "no duplicates");
}

#[test]
fn doctor_force_bypass_saved_engaged_block_still_forced_on_for_its_switch() {
    // REGRESSION (HW, preset 024 "TR+BD2+BMP"): BD2 saved ON in base with its on-off
    // `isActive:true` (the preset was saved with the switch engaged). The old
    // unconditional "flip of saved bypass" forced it OFF during its own switch's
    // capture — the Doctor diagnosed the base sound instead. isActive:true ⇒ the
    // saved state IS the engaged state.
    let p = force_bypass_fixture();
    let out = doctor_force_bypass(&p["ftsw"], &p, Some(2));
    assert!(
        out.contains(&("G1".into(), "BD2".into(), false)),
        "own block forced ON"
    );
    assert!(out.contains(&("G1".into(), "DRV".into(), true)));
    assert!(out.contains(&("G1".into(), "MOD".into(), true)));
    assert_eq!(out.len(), 3);
}

#[test]
fn doctor_force_bypass_null_ftsw_degrades_to_empty() {
    let p = force_bypass_fixture();
    let null = serde_json::Value::Null;
    // Offline / SimDevice: no ftsw → nothing to isolate, for base AND footswitch.
    assert!(doctor_force_bypass(&null, &p, None).is_empty());
    assert!(doctor_force_bypass(&null, &p, Some(0)).is_empty());
}

// ── base_isolation_or_refuse: the shared probe/production "read → refuse-or-force" step ──
//
// Phase 5 isolation-parity fix (2026-08-31 bisect): `probe --levelpreset`'s Base leg used
// to pass an empty `&[]` force list, measuring "base as saved" instead of production's
// isolated base — a divergence that solved C=-11.95 as-saved vs production's C≈-28.2 on
// "Plumes+BD2+OCD". Both paths now share ONE fn, `base_isolation_or_refuse`; these two
// gates pin that sharing can't silently regress.

#[test]
fn base_isolation_or_refuse_force_list_matches_doctor_force_bypass_directly() {
    // Gate (a): probe's force list and production's must be the literal SAME list off the
    // SAME preset fixture — proven by `base_isolation_or_refuse` deriving it via
    // `doctor_force_bypass` verbatim, not a hand-maintained copy that could drift.
    let p = force_bypass_fixture();
    let (_, force, _) = base_isolation_or_refuse(Ok(p.clone()), ListIndex(26))
        .expect("a readable preset must never refuse");
    let direct = doctor_force_bypass(&p["ftsw"], &p, None);
    assert_eq!(
        force, direct,
        "base_isolation_or_refuse must derive the isolation list via doctor_force_bypass \
         verbatim — the ONE shared definition of \"base\""
    );
}

#[test]
fn base_isolation_or_refuse_refuses_on_a_truncated_ftsw_read_exactly_like_production_always_has() {
    // Gate (b): `SimDevice` always serves whole bodies (`read_slot_preset_complete`'s own
    // doc), so the truncated-field-8 refusal can never be driven from an offline fixture
    // via a real read — this hands the fn the read's `Err` directly, standing in for what
    // `read_slot_preset_complete` returns on a preset too large to read over USB, the same
    // way `an_unreadable_ftsw_skips_a_footswitch_sound_but_not_a_base_sound` stands in a
    // synthetic cache entry for an unreadable body.
    let err = base_isolation_or_refuse(
        Err(SlotReadError::SectionsCut {
            sections: vec!["ftsw".into()],
            message: "slot 27: the preset is too large to read over USB — its ftsw section(s) \
                      were cut off the field-8 read, and the preset list did not answer with \
                      the slot's name, so a backup re-read cannot be addressed safely. \
                      Refusing rather than acting on a partial preset"
                .to_string(),
        }),
        ListIndex(26),
    )
    .expect_err("an unreadable ftsw must refuse, never guess a force list");
    assert!(
        err.reason
            .contains("could not read preset 27's footswitch assignments"),
        "the wrapped refusal must name the 1-based slot and the footswitch read: {err}"
    );
    assert!(
        err.reason
            .contains("leveling without them would save a level solved for the wrong sound"),
        "the refusal must explain WHY, matching production's long-standing base-arm message: \
         {err}"
    );
    assert_eq!(
        err.kind,
        SkipKind::FootswitchesUnreadable,
        "a cut `ftsw` is the preset's footswitches not being read"
    );
}

/// A base read that failed outright — no preset data, a body that did not parse — is the
/// PRESET not being read, the kind the scene lane gives the same read, never its footswitches.
#[test]
fn base_isolation_or_refuse_names_a_failed_read_as_the_preset_not_its_footswitches() {
    for message in ["no preset data for slot 27", "preset JSON did not parse"] {
        let err = base_isolation_or_refuse(
            Err(SlotReadError::Unreadable(message.to_string())),
            ListIndex(26),
        )
        .expect_err("a failed read must refuse");
        assert_eq!(err.kind, SkipKind::PresetUnreadable, "{message}: {err}");
        assert!(
            err.reason.contains(message),
            "the cause stays in the text: {err}"
        );
    }
    // A cut that took more than `ftsw` is more than the footswitches.
    let err = base_isolation_or_refuse(
        Err(SlotReadError::SectionsCut {
            sections: vec!["audioGraph".into(), "ftsw".into()],
            message: "cut".into(),
        }),
        ListIndex(26),
    )
    .expect_err("a cut read must refuse");
    assert_eq!(err.kind, SkipKind::PresetUnreadable, "{err}");
}

// ── derived_force_bypass: OFFLINE isolation, oracle-equivalent to doctor_force_bypass ──
//
// The isolation-delete's core proof: `derived_force_bypass` (walks the backup scan's
// already-enumerated `FootswitchInfo` + `DoctorNode`s, no device read) must reproduce
// `doctor_force_bypass` (walks the live field-8 `ftsw`/preset JSON) byte-for-byte, as
// SETS, on the same data — for base and every footswitch sound.

/// `doctor::DoctorNode`s built from a preset's SAVED bypass states — the test-side
/// stand-in for what the frontend threads through as `DoctorInput.nodes` (sourced
/// from the backup scan's `ActiveGraph.nodes`). Only `node_id` + `bypassed` drive
/// the isolation derivation; the rest stay at defaults.
fn nodes_from(preset: &serde_json::Value) -> Vec<doctor::DoctorNode> {
    let mut out = Vec::new();
    crate::audiograph::for_each_node(preset, |obj| {
        let Some(nid) = obj.instance() else {
            return;
        };
        let bypassed = obj
            .get("dspUnitParameters")
            .and_then(|p| p.get("bypass"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        out.push(doctor::DoctorNode {
            group_id: String::new(),
            node_id: nid.to_string(),
            model: nid.to_string(),
            bypassed,
            cab_sim_id: None,
            cab_sim2_enabled: None,
            params: std::collections::HashMap::new(),
        });
    });
    out
}

/// 5 block-acting switches: 0 = normal (off in base, `isActive:false` — the HW
/// correlation), 1 = saved-ENGAGED (preset-024 BD2 shape: ON in base with
/// `isActive:true`), 2 = param-only (no on-off — must contribute nothing to the
/// on-off derivation), 3 & 4 = SHARE one on-off node (the shared-node edge). A CAB
/// node no switch touches stays in the graph (dedup/exclusion must never sweep it in).
fn iso_ab_fixture() -> (serde_json::Value, Vec<footswitch::FootswitchInfo>) {
    let preset = serde_json::json!({
        "audioGraph": { "guitarNodes": { "G1": [
            { "nodeId": "DRV", "FenderId": "DRV", "dspUnitParameters": { "bypass": true } },
            { "nodeId": "BD2", "FenderId": "BD2", "dspUnitParameters": { "bypass": false } },
            { "nodeId": "MOD", "FenderId": "MOD", "dspUnitParameters": { "bypass": false, "gain": 0.4 } },
            { "nodeId": "SHARE", "FenderId": "SHARE", "dspUnitParameters": { "bypass": true } },
            { "nodeId": "CAB", "FenderId": "CAB", "dspUnitParameters": { "bypass": false } }
        ]}, "micNodes": {} },
        "ftsw": [
            [{ "func": "on-off", "nodes": [{ "groupId": "G1", "nodeId": "DRV" }], "isActive": false }],
            [{ "func": "on-off", "nodes": [{ "groupId": "G1", "nodeId": "BD2" }], "isActive": true }],
            [{ "func": "param", "groupId": "G1", "nodeId": "MOD", "parameterId": "gain",
               "valueA": 0.9, "valueB": 0.4, "isActive": false }],
            [{ "func": "on-off", "nodes": [{ "groupId": "G1", "nodeId": "SHARE" }], "isActive": false }],
            [{ "func": "on-off", "nodes": [{ "groupId": "G1", "nodeId": "SHARE" }], "isActive": false }],
        ]
    });
    let infos = footswitch::enumerate_block_footswitches(&preset["ftsw"], &preset);
    (preset, infos)
}

#[test]
fn derived_force_bypass_matches_the_live_engine_on_every_sound() {
    let (preset, infos) = iso_ab_fixture();
    let nodes = nodes_from(&preset);
    let ftsw = &preset["ftsw"];

    // Base, then every block-acting switch (incl. the param-only switch 2, whose
    // isolation is empty-own — same on both engines).
    let cases: Vec<Option<u32>> = std::iter::once(None)
        .chain(infos.iter().map(|fi| Some(fi.switch)))
        .collect();
    assert_eq!(cases.len(), 6, "base + 5 switches");
    for case in cases {
        let mut old = doctor_force_bypass(ftsw, &preset, case);
        let mut derived = footswitch::derived_force_bypass(&infos, &saved_bypass_map(&nodes), case);
        old.sort();
        derived.sort();
        assert_eq!(old, derived, "mismatch for footswitch={case:?}");
    }
}

// --- consecutive-scene load skip (doctor_skip_load) ---

fn prev(list_index: u32, wrote: bool) -> PrevSound {
    PrevSound {
        list_index: ListIndex(list_index),
        wrote,
    }
}

#[test]
fn skip_load_only_for_a_clean_ok_same_preset_scene_chain() {
    // The one allowed case: same preset, previous sound clean, current is a scene.
    assert!(doctor_skip_load(Some(&prev(3, false)), ListIndex(3), true));
    // First sound of the run — and any sound after an ERRORED one (the loop resets
    // prev to None on error) — never skips.
    assert!(!doctor_skip_load(None, ListIndex(3), true));
    // Different preset → reload.
    assert!(!doctor_skip_load(Some(&prev(2, false)), ListIndex(3), true));
    // Previous sound wrote force-bypasses (base/footswitch) → reload.
    assert!(!doctor_skip_load(Some(&prev(3, true)), ListIndex(3), true));
    // Base/footswitch sounds always reload, even after a clean scene.
    assert!(!doctor_skip_load(
        Some(&prev(3, false)),
        ListIndex(3),
        false
    ));
}

// --- capture_suspect: what a Doctor capture must pass before it is diagnosed (#246) ---

#[test]
fn capture_suspect_flags_a_flat_capture_against_a_lively_stimulus() {
    assert_eq!(
        capture_suspect(true, true, 0.01, 6.0),
        Some(CaptureSuspect::Floor)
    );
    // A floor read usually has no onset either; the floor is the named cause.
    assert_eq!(
        capture_suspect(false, false, 0.01, 6.0),
        Some(CaptureSuspect::Floor)
    );
}

#[test]
fn capture_suspect_clears_a_live_complete_capture() {
    assert_eq!(capture_suspect(true, true, 4.0, 6.0), None);
}

#[test]
fn capture_suspect_floor_check_disarms_on_a_near_stationary_stimulus() {
    // stimulus momentary spread ≤ STATIONARY_STIM_LU (2.0) can't discriminate —
    // the floor check must not fire even though the capture itself reads flat.
    assert_eq!(capture_suspect(true, true, 0.01, 0.2), None);
}

// BUG→GATE (#246): an un-aligned onset used to only WARN, then every rule ran on the
// whole buffer with every band covered. It must never reach the diagnosis, even when
// the floor check is disarmed.
#[test]
fn capture_suspect_refuses_an_unaligned_onset() {
    assert_eq!(
        capture_suspect(false, true, 4.0, 6.0),
        Some(CaptureSuspect::NoOnset)
    );
    assert_eq!(
        capture_suspect(false, true, 0.01, 0.2),
        Some(CaptureSuspect::NoOnset)
    );
    assert_eq!(CaptureSuspect::NoOnset.into_error(), LevelError::FloorRead);
}

// BUG→GATE (#246): a capture that ends before the body did read the −80 dB "bone
// dry" tail sentinel and silently dropped a wet preset's `washed` verdict.
#[test]
fn capture_suspect_refuses_a_capture_with_no_tail() {
    assert_eq!(
        capture_suspect(true, false, 4.0, 6.0),
        Some(CaptureSuspect::NoTail)
    );
    assert_eq!(
        CaptureSuspect::NoTail.into_error(),
        LevelError::Device(TRUNCATED_CAPTURE_ERR.to_string())
    );
}

// #246 end to end on real stimulus data: a stationary floor captured where the
// shipped stimulus should be has no energy step, so the onset seam reports it
// un-aligned, and the capture is refused rather than diagnosed.
#[test]
fn a_stationary_floor_capture_is_refused_not_diagnosed() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("resources/samples/guitar-humbucker.wav");
    let stim = leveller::doctor_stim_slice(
        crate::stimulus::read_wav_mono(path.to_str().unwrap())
            .expect("shipped stimulus")
            .0,
    );
    let stim_spread = leveller::stimulus_momentary_spread(&stim);
    // A deterministic low-level stationary hiss, as long as a real capture.
    let mut x: u64 = 7;
    let floor: Vec<f32> = (0..stim.len() + 48_000)
        .map(|_| {
            x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            ((x >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 1e-3
        })
        .collect();
    let onset = leveller::doctor_onset(&stim, &floor, leveller::RATE);
    let loud = crate::lufs::measure_mono(&floor, leveller::RATE).expect("finite floor");
    let suspect = capture_suspect(
        onset.confident(),
        floor.len() > onset.body_end(),
        loud.momentary_spread_lu,
        stim_spread,
    );
    assert!(
        suspect.is_some(),
        "a stationary floor must be refused, onset {onset:?}"
    );
}

// --- skipped_band_count: fix P3-4, SNR-gate transparency ---

#[test]
fn skipped_band_count_counts_false_entries() {
    assert_eq!(skipped_band_count(Some(&[true, true, true])), 0);
    assert_eq!(skipped_band_count(Some(&[true, false, true, false])), 2);
    assert_eq!(skipped_band_count(Some(&[false, false])), 2);
}

#[test]
fn skipped_band_count_absent_coverage_reads_zero() {
    // No coverage computed at all (errored/showcase sound) reads as 0, not
    // "everything gated".
    assert_eq!(skipped_band_count(None), 0);
}

// --- resolve_sound_isolation: fix P3-1, scenes ride their saved overlay state ---

#[test]
fn resolve_sound_isolation_never_writes_for_a_scene_sound() {
    // A scene sound must get NO force-bypass write — graph present (the bug: it
    // used to route through `derived_force_bypass` with the base-forcing shape,
    // `fs=None`, identical to the base isolation) AND graph absent (already
    // correct pre-fix, must stay so). Base/footswitch are untouched by this fix
    // and are covered by the `doctor_force_bypass`/`derived_force_bypass` tests
    // elsewhere in this file.
    let (preset, infos) = iso_ab_fixture();
    let nodes = nodes_from(&preset);
    let mut cache = std::collections::HashMap::new();
    let iso = resolve_sound_isolation(&nodes, &infos, &[], Some(0), None, ListIndex(5), &mut cache);
    assert!(iso.bypass.is_empty() && iso.params.is_empty());
    // Graph absent too — the pre-existing empty-nodes behavior for scenes.
    let iso = resolve_sound_isolation(&[], &infos, &[], Some(0), None, ListIndex(5), &mut cache);
    assert!(iso.bypass.is_empty() && iso.params.is_empty());
}

// --- derived_param_writes: the FS-sound param twin of the isolation parity test ---

#[test]
fn derived_param_writes_matches_the_live_engine_on_every_sound() {
    // The param derivation exists twice (offline `FootswitchInfo` walk vs live
    // `ftsw` JSON `param_fn_values`) exactly like the isolation split above —
    // the two must agree on base + every switch, including the param-only
    // switch 2 (valueA 0.9) and the on-off-only switches (no param writes).
    let (preset, infos) = iso_ab_fixture();
    let ftsw = &preset["ftsw"];
    let cases: Vec<Option<u32>> = std::iter::once(None)
        .chain(infos.iter().map(|fi| Some(fi.switch)))
        .collect();
    for case in cases {
        let derived = footswitch::derived_param_writes(&infos, case);
        let live: Vec<(String, String, String, f32)> = case
            .map(|sw| {
                footswitch::param_fn_values(ftsw, sw)
                    .into_iter()
                    .map(|(g, n, p, a, _b)| (g, n, p, a))
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(derived, live, "mismatch for footswitch={case:?}");
    }
    // And the param-only switch actually yields its engaged write — the case the
    // Doctor used to capture as the base sound.
    assert_eq!(
        footswitch::derived_param_writes(&infos, Some(2)),
        vec![("G1".into(), "MOD".into(), "gain".into(), 0.9_f32)]
    );
}

#[test]
fn resolve_sound_isolation_carries_param_writes_for_a_param_only_switch() {
    // A param-only footswitch SOUND: no on-off isolation of its own beyond the
    // siblings, and its `params` must carry the engaged valueA — with `wrote`
    // semantics downstream keyed on either list being non-empty.
    let (preset, infos) = iso_ab_fixture();
    let nodes = nodes_from(&preset);
    let mut cache = std::collections::HashMap::new();
    let iso = resolve_sound_isolation(&nodes, &infos, &[], None, Some(2), ListIndex(5), &mut cache);
    assert_eq!(
        iso.params,
        vec![("G1".into(), "MOD".into(), "gain".into(), 0.9_f32)]
    );
    let _ = preset; // fixture kept alive alongside its enumerated infos
}

/// One `hpf` param op — the fixture both `apply_ops_under_scene` tests apply.
fn hpf_op() -> Vec<doctor::DoctorOp> {
    vec![doctor::DoctorOp::Param {
        group_id: "G1".into(),
        node_id: "amp".into(),
        param: "hpf".into(),
        value: 90.0,
    }]
}

// `apply_ops_under_scene` — the op-ORDER fix: a scene recall must precede every
// prescription write, or a bare `changeParameter` lands wherever the connection
// happens to default to (the preset's saved `lastLoadedScene`), not the diagnosed
// scene. Split out of `ops_session` (which needs a real `Session::connect()` +
// `confirm_active`'s "My Presets" list echo — not modeled by `SimDevice`) so this
// ordering is unit-testable offline.
#[test]
fn apply_ops_under_scene_recalls_before_writing() {
    let sim = crate::sim_device::SimDevice::new();
    let mut s = crate::session::Session::from_transport(Box::new(sim.clone()));
    apply_ops_under_scene(&mut s, Some(2), &hpf_op()).expect("apply_ops_under_scene");
    let ev = sim.events();
    let scene_edit_pos = ev
        .iter()
        .position(|e| matches!(e, crate::sim_device::SimEvent::LoadScene(2)));
    let write_pos = ev.iter().position(|e| {
        matches!(
            e,
            crate::sim_device::SimEvent::ChangeParameter { scene: 2, param, .. } if param == "hpf"
        )
    });
    assert!(
        matches!((scene_edit_pos, write_pos), (Some(r), Some(w)) if r < w),
        "LoadScene(2) must precede the scene-2 write: {ev:?}"
    );
}

// The base case: `scene: None` recalls `BASE_SCENE_SLOT`, not "no recall at all"
// — a bare write with no recall lands in whatever scene the connection defaults
// to (the preset's saved `lastLoadedScene`), which can silently differ from base.
#[test]
fn apply_ops_under_scene_recalls_base_explicitly_for_none() {
    let sim = crate::sim_device::SimDevice::new().with_saved_scene(0, Some(3));
    let mut s = crate::session::Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(0)).expect("load_preset"); // activates saved scene 3, not base
    apply_ops_under_scene(&mut s, None, &hpf_op()).expect("apply_ops_under_scene");
    let ev = sim.events();
    assert!(
        ev.iter().any(|e| matches!(
            e,
            crate::sim_device::SimEvent::ChangeParameter {
                scene: crate::sim_device::SCENE_BASE,
                param,
                ..
            } if param == "hpf"
        )),
        "a None scene must write base, not the leftover saved scene 3: {ev:?}"
    );
}

fn insert_op(before: Option<&str>, fender_id: &str) -> doctor::DoctorOp {
    doctor::DoctorOp::InsertNode {
        group_id: "G1".to_string(),
        before_node_id: before.map(str::to_string),
        fender_id: fender_id.to_string(),
        params: Vec::new(),
    }
}

fn g1_roster(nodes: &[(&str, &str)]) -> Vec<crate::blockcaps::RosterEntry> {
    let nodes: Vec<_> = nodes
        .iter()
        .map(|(fid, id)| serde_json::json!({ "FenderId": fid, "nodeId": id }))
        .collect();
    crate::blockcaps::roster_from_preset(
        &serde_json::json!({ "audioGraph": { "guitarNodes": { "G1": nodes } } }),
    )
}

#[test]
fn doctor_inserts_are_refused_on_a_missing_anchor_or_over_budget() {
    // fw 1.8.58 aborts its server on both, so neither may reach the device.
    let roster = g1_roster(&[("ACD_GuitarSynth", "synth"), ("ACD_GuitarSynth", "synth_1")]);
    assert!(check_doctor_inserts(&roster, &[insert_op(Some("synth_1"), "ACD_Blackbox")]).is_ok());
    let miss = check_doctor_inserts(
        &roster,
        &[insert_op(Some("ACD_GuitarSynth_1"), "ACD_Blackbox")],
    )
    .expect_err("an anchor the group lacks");
    assert!(miss.contains("not sent"), "{miss}");
    // 36 + 36 + 9.5 = 81.5 > 76.5.
    let over = check_doctor_inserts(&roster, &[insert_op(None, "ACD_LoFi")])
        .expect_err("over the CPU budget");
    assert!(over.contains("ProcessorUtilization"), "{over}");
}

/// An insert whose confirm is lost but which landed is read back, not re-sent, and its
/// param follow-up still targets the block the device minted.
#[test]
fn doctor_insert_whose_confirm_is_lost_is_read_back_not_resent() {
    use crate::sim_device::{SimDevice, SimEvent};
    let doc = format!(
        r#"{{"audioGraph":{{"template":"gtrSeries","guitarNodes":{{"G1":[{},{}]}}}},"zzTail":"{}"}}"#,
        r#"{"FenderId":"ACD_Twin57","nodeId":"n1","dspUnitParameters":{"bypass":false}}"#,
        r#"{"FenderId":"ACD_ChorusCE2","nodeId":"n2","dspUnitParameters":{"bypass":false}}"#,
        "x".repeat(200)
    );
    let sim = SimDevice::new()
        .with_preset_json(&doc)
        .with_lost_first_confirm();
    let mut s = crate::session::Session::from_transport(Box::new(sim.clone()));
    let op = doctor::DoctorOp::InsertNode {
        group_id: "G1".to_string(),
        before_node_id: Some("n2".to_string()),
        fender_id: "ACD_TubeScreamer".to_string(),
        params: vec![("level".to_string(), 0.5)],
    };
    apply_doctor_ops(&mut s, &[op]).expect("the landed insert is confirmed by its read-back");
    let ev = sim.events();
    let inserts = ev
        .iter()
        .filter(|e| matches!(e, SimEvent::Insert { .. }))
        .count();
    assert_eq!(inserts, 1, "re-sent after a lost confirm: {ev:?}");
    assert!(
        ev.iter().any(|e| matches!(
            e,
            SimEvent::ChangeParameter { node, param, .. }
                if node == "ACD_TubeScreamer" && param == "level"
        )),
        "the param must target the minted id: {ev:?}"
    );
}

/// A working copy cut inside `audioGraph` under-counts the blocks the CPU budget and the
/// anchors are checked against, so Doctor refuses on it and sends no insert.
#[test]
fn doctor_refuses_inserts_on_a_read_cut_inside_the_audio_graph() {
    use crate::sim_device::{SimDevice, SimEvent};
    // No tail pad: the sim's field-3 reassembly drops the last ≤60 B, which here cuts
    // through `template` into the last node — a tolerant parse still yields n1.
    let doc = format!(
        r#"{{"audioGraph":{{"guitarNodes":{{"G1":[{},{}]}},"template":"gtrSeries"}}}}"#,
        r#"{"FenderId":"ACD_Twin57","nodeId":"n1","dspUnitParameters":{"bypass":false}}"#,
        r#"{"FenderId":"ACD_ChorusCE2","nodeId":"n2","dspUnitParameters":{"bypass":false}}"#,
    );
    let sim = SimDevice::new().with_preset_json(&doc);
    let mut s = crate::session::Session::from_transport(Box::new(sim.clone()));
    let op = doctor::DoctorOp::InsertNode {
        group_id: "G1".to_string(),
        before_node_id: None,
        fender_id: "ACD_TubeScreamer".to_string(),
        params: vec![],
    };
    apply_doctor_ops(&mut s, &[op]).expect_err("a cut read is refused");
    let inserts = sim
        .events()
        .iter()
        .filter(|e| matches!(e, SimEvent::Insert { .. }))
        .count();
    assert_eq!(inserts, 0, "inserted on a cut read");
}

// --- #190: a stopped run never fabricates a measurement ---

fn item(
    key: &str,
    scene: Option<u32>,
    footswitch: Option<u32>,
) -> (DoctorInput, String, doctor::StimulusKind) {
    let input = DoctorInput {
        key: key.to_string(),
        list_index: ListIndex(4),
        scene,
        footswitch,
        label: key.to_string(),
        sound_name: String::new(),
        tag: None,
        topology_id: None,
        calibration_lufs: None,
        profile_id: None,
        nodes: Vec::new(),
        footswitches: Vec::new(),
        scene_overrides: Vec::new(),
    };
    (input, String::new(), doctor::StimulusKind::Synthetic)
}

/// A capture at `lufs` with a dry tail and target-curve bands (no findings of its own).
fn measured(lufs: f64, tail_ratio_db: f64, nodes: Vec<doctor::DoctorNode>) -> SoundOutcome {
    SoundOutcome::Measured {
        profile: doctor::SoundProfile {
            bands: doctor::target_curve(doctor::Family::Guitar)
                .iter()
                .map(|d| 10f64.powf(d / 10.0))
                .collect(),
            integrated_lufs: lufs,
            spread_lu: 0.0,
            momentary_spread_lu: 0.0,
            tail_ratio_db,
            air_flatness: 0.5,
            peaks: Vec::new(),
            stim_bands: None,
        },
        coverage: None,
        nodes,
    }
}

fn base_and_scenes() -> Vec<(DoctorInput, String, doctor::StimulusKind)> {
    vec![
        item("base", None, None),
        item("s0", Some(0), None),
        item("s1", Some(1), None),
    ]
}

/// BUG→GATE #190: Stop before the first capture. Every sound is `unrun` with no loudness —
/// before the fix each one read 0.0 LUFS with no error and rendered as "All clear".
#[test]
fn a_run_stopped_before_its_first_capture_reports_every_sound_unrun() {
    let items = base_and_scenes();
    let outcomes = vec![SoundOutcome::Unrun; items.len()];
    let presets = assemble_results(&items, &outcomes);
    assert_eq!(presets.len(), 1);
    for s in &presets[0].sounds {
        assert_eq!(s.status, SoundStatus::Unrun, "{}", s.key);
        assert_eq!(s.integrated_lufs, None, "{}", s.key);
        assert_eq!(s.tail_ratio_db, None, "{}", s.key);
        assert!(s.diags.is_empty() && s.error.is_none(), "{}", s.key);
    }
    assert!(presets[0].scene_consistency.is_none());
    let v = serde_json::to_value(&presets[0].sounds[0]).unwrap();
    assert_eq!(v["status"], "unrun");
    assert_eq!(v["integratedLufs"], serde_json::Value::Null);
}

/// BUG→GATE #190: Stop between the base and its scenes. The base is measured at −20 LUFS;
/// the scenes were never captured. Before the fix they entered scene consistency as 0 LUFS
/// readings — a fabricated +20 dB jump.
#[test]
fn a_run_stopped_between_base_and_scenes_keeps_unrun_scenes_out_of_consistency() {
    let items = base_and_scenes();
    let outcomes = vec![
        measured(-20.0, -40.0, Vec::new()),
        SoundOutcome::Unrun,
        SoundOutcome::Unrun,
    ];
    let p = &assemble_results(&items, &outcomes)[0];
    assert_eq!(p.sounds[0].status, SoundStatus::Measured);
    assert_eq!(p.sounds[0].integrated_lufs, Some(-20.0));
    assert_eq!(p.sounds[1].status, SoundStatus::Unrun);
    assert_eq!(p.sounds[2].status, SoundStatus::Unrun);
    assert!(
        p.scene_consistency.is_none(),
        "no scene was measured, so there is nothing to compare: {:?}",
        p.scene_consistency
    );
}

#[test]
fn consistency_compares_only_measured_sounds() {
    let items = base_and_scenes();
    // A real +8 dB scene jump, plus a failed scene that must not enter the table.
    let outcomes = vec![
        measured(-20.0, -40.0, Vec::new()),
        measured(-12.0, -40.0, Vec::new()),
        SoundOutcome::Failed(
            "no signal captured".to_string(),
            crate::error::SkipKind::NoSignal,
        ),
    ];
    let p = &assemble_results(&items, &outcomes)[0];
    assert_eq!(p.sounds[2].status, SoundStatus::Error);
    assert_eq!(p.sounds[2].integrated_lufs, None);
    let rows = &p
        .scene_consistency
        .as_ref()
        .expect("an 8 dB jump is flagged")
        .rows;
    assert!(
        rows.iter().all(|r| r.name != "s1"),
        "the failed scene stays out: {rows:?}"
    );
    assert!(rows.iter().any(|r| r.name == "s0"));
}

/// The steps a volume jump writes name the sound as the screen does ("Lead", "footswitch 3"),
/// never by its wire label, which also names the preset. The rows and `worst_name` keep the
/// label, because the frontend finds each sound by it.
#[test]
fn consistency_steps_use_the_on_screen_name_and_rows_keep_the_label() {
    let named = |key: &str, scene, footswitch, label: &str, name: &str| {
        let (mut input, url, kind) = item(key, scene, footswitch);
        input.label = label.to_string();
        input.sound_name = name.to_string();
        (input, url, kind)
    };
    // (louder sound, its label, its on-screen name)
    let cases = [
        (Some(1), None, "Tweed Warm · Lead", "Lead"),
        (Some(0), None, "Tweed Warm · Intro", "Intro"),
        (None, Some(2), "Tweed Warm · Boost", "footswitch 3"),
    ];
    for (scene, footswitch, label, name) in cases {
        let items = vec![
            named("base", None, None, "Tweed Warm · Base", "Main sound"),
            named("other", scene, footswitch, label, name),
        ];
        let outcomes = vec![
            measured(-20.0, -40.0, Vec::new()),
            measured(-14.0, -40.0, Vec::new()),
        ];
        let p = &assemble_results(&items, &outcomes)[0];
        let sc = p
            .scene_consistency
            .as_ref()
            .expect("a 6 dB jump is flagged");
        assert_eq!(sc.worst_name, label);
        assert_eq!(
            sc.rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["Tweed Warm · Base", label]
        );
        assert!(!sc.rx.is_empty());
        for r in &sc.rx {
            for text in [&r.title, &r.detail] {
                assert!(!text.contains(" · "), "{text:?}");
                assert!(!text.contains("Tweed Warm"), "{text:?}");
            }
        }
        assert!(sc.rx[0].title.contains(name), "{:?}", sc.rx[0].title);
    }
}

// --- #191: one effective graph per sound ---

/// A preset whose plate reverb is SAVED BYPASSED (its `level` makes its switches offer a
/// level param — the UI's selectable gate; Mix alone is never offered): footswitch 1 turns it on, footswitch 2
/// jumps its `wetdrymix` (a parameter function), footswitch 3 jumps the amp's
/// non-allowlisted `gain`. Scene 0's overlay turns the plate on; scene 1 has no overlay.
fn wet_by_switch_fixture() -> serde_json::Value {
    serde_json::json!({
        "audioGraph": { "guitarNodes": { "G1": [
            { "nodeId": "amp", "FenderId": "ACD_TweedDeluxe",
              "dspUnitParameters": { "bypass": false, "gain": 0.5 } },
            { "nodeId": "cab1", "FenderId": "ACD_CabSimTMS",
              "dspUnitParameters": { "bypass": false, "cabsimid": "Some412" } },
            { "nodeId": "plate1", "FenderId": "ACD_TMLargePlate",
              "dspUnitParameters": { "bypass": true, "wetdrymix": 0.3, "level": 0.5 } }
        ]}, "micNodes": {} },
        "ftsw": [
            [{ "func": "on-off", "nodes": [{ "groupId": "G1", "nodeId": "plate1" }], "isActive": false }],
            [{ "func": "param", "groupId": "G1", "nodeId": "plate1", "parameterId": "wetdrymix",
               "valueA": 0.8, "valueB": 0.3, "valueType": 0, "isActive": false }],
            [{ "func": "param", "groupId": "G1", "nodeId": "amp", "parameterId": "gain",
               "valueA": 0.9, "valueB": 0.5, "valueType": 0, "isActive": false }],
        ],
        "scenes": [
            { "guitarNodes": { "G1": { "plate1": {
                "dspUnitParameters": { "bypass": false, "wetdrymix": 0.7 } } } } },
            { "guitarNodes": {} }
        ]
    })
}

/// The fixture's sound as `doctor_check` builds it: the backup scan's graph, footswitch
/// roster and scene overrides — the same derivations the frontend receives.
fn fixture_sound(
    scene: Option<u32>,
    footswitch: Option<u32>,
) -> (DoctorInput, Vec<doctor::DoctorNode>) {
    let preset = wet_by_switch_fixture();
    let nodes: Vec<doctor::DoctorNode> = crate::session::extract_active_graph(&preset, None)
        .nodes
        .iter()
        .map(doctor::DoctorNode::from_graph_node)
        .collect();
    let infos = footswitch::enumerate_block_footswitches(&preset["ftsw"], &preset);
    let scene_overrides = scene
        .map(|s| crate::scene_jobs::scene_node_overrides(&preset)[s as usize].clone())
        .unwrap_or_default();
    let (mut input, _, _) = item("fx", scene, footswitch);
    input.nodes = nodes;
    input.footswitches = infos;
    input.scene_overrides = scene_overrides;
    let mut cache = std::collections::HashMap::new();
    let iso = resolve_sound_isolation(
        &input.nodes,
        &input.footswitches,
        &input.scene_overrides,
        scene,
        footswitch,
        ListIndex(4),
        &mut cache,
    );
    (input, iso.graph)
}

fn plate(graph: &[doctor::DoctorNode]) -> &doctor::DoctorNode {
    graph
        .iter()
        .find(|n| n.node_id == "plate1")
        .expect("plate in graph")
}

/// Diagnose a hot-tailed capture of `graph` through the command's own assembly.
fn diag_keys_on(input: DoctorInput, graph: Vec<doctor::DoctorNode>) -> Vec<&'static str> {
    let items = vec![(input, String::new(), doctor::StimulusKind::Synthetic)];
    let outcomes = vec![measured(-20.0, 0.0, graph)];
    assemble_results(&items, &outcomes)[0].sounds[0]
        .diags
        .iter()
        .map(|d| d.diag.key)
        .collect()
}

#[test]
fn the_fixture_switches_are_selectable_in_the_ui() {
    // The Doctor's select list only offers a footswitch sound whose switch has
    // `level_params` — the fixture must exercise sounds a player can actually pick.
    let preset = wet_by_switch_fixture();
    let infos = footswitch::enumerate_block_footswitches(&preset["ftsw"], &preset);
    for sw in [0, 1] {
        let fi = infos
            .iter()
            .find(|f| f.switch == sw)
            .expect("switch enumerated");
        assert!(
            !fi.level_params.is_empty(),
            "switch {sw} has no level_params"
        );
    }
}

/// BUG→GATE #191: a footswitch that turns on a saved-bypassed reverb is captured WET, so it
/// gets the full wash tail and can be diagnosed `washed`. Before the fix the tail and the
/// diagnosis read the base graph (reverb bypassed): a short tail and `washed` gated off.
#[test]
fn a_footswitch_enabling_a_bypassed_reverb_is_diagnosed_wet() {
    let (input, graph) = fixture_sound(None, Some(0));
    assert!(!plate(&graph).bypassed);
    assert_eq!(
        doctor::doctor_tail_ms(&graph),
        crate::leveller::DOCTOR_TAIL_MS
    );
    assert!(diag_keys_on(input, graph).contains(&"washed"));
}

/// The opposing case: the base sound is captured with every switched block forced OFF, so
/// it is dry — short tail, and a hot tail reading never earns `washed`.
#[test]
fn the_base_sound_is_diagnosed_with_switched_blocks_forced_off() {
    let (input, graph) = fixture_sound(None, None);
    assert!(plate(&graph).bypassed);
    assert_eq!(
        doctor::doctor_tail_ms(&graph),
        crate::leveller::DOCTOR_TAIL_DRY_MS
    );
    assert!(!diag_keys_on(input, graph).contains(&"washed"));
}

/// A wet block that base isolation forces OFF must not reach a prescription either: a
/// saved-ON switched reverb on the base sound is captured off, so it is not "active".
#[test]
fn base_isolation_turns_off_a_saved_on_switched_reverb() {
    let mut preset = wet_by_switch_fixture();
    preset["audioGraph"]["guitarNodes"]["G1"][2]["dspUnitParameters"]["bypass"] = false.into();
    let nodes: Vec<doctor::DoctorNode> = crate::session::extract_active_graph(&preset, None)
        .nodes
        .iter()
        .map(doctor::DoctorNode::from_graph_node)
        .collect();
    assert!(!plate(&nodes).bypassed, "saved ON in base");
    let infos = footswitch::enumerate_block_footswitches(&preset["ftsw"], &preset);
    let (mut input, _, _) = item("fx", None, None);
    input.nodes = nodes;
    input.footswitches = infos;
    let iso = resolve_sound_isolation(
        &input.nodes,
        &input.footswitches,
        &[],
        None,
        None,
        ListIndex(4),
        &mut Default::default(),
    );
    let graph = iso.graph;
    assert!(plate(&graph).bypassed, "the capture forced it off");
    assert_eq!(
        doctor::doctor_tail_ms(&graph),
        crate::leveller::DOCTOR_TAIL_DRY_MS
    );
}

/// A parameter-function switch jumps the plate's `wetdrymix` to its `valueA` for the
/// capture — the diagnosed graph carries that value, not the saved one.
#[test]
fn a_param_function_jump_reaches_the_diagnosed_graph() {
    let (_, graph) = fixture_sound(None, Some(1));
    let mix = plate(&graph).params.get("wetdrymix").copied();
    assert!(
        mix.is_some_and(|m| (m - 0.8).abs() < 1e-6),
        "wetdrymix = {mix:?}"
    );
}

/// A param jump outside the Doctor allowlist never enters the graph's params.
#[test]
fn a_non_allowlisted_param_jump_does_not_leak_into_params() {
    let (_, graph) = fixture_sound(None, Some(2));
    let amp = graph.iter().find(|n| n.node_id == "amp").unwrap();
    assert!(!amp.params.contains_key("gain"), "{:?}", amp.params);
}

/// BUG→GATE #191 (scene half): scene 0's saved overlay turns the plate on and sets its
/// mix, so the scene sound is diagnosed wet with the overlay's mix. Scene 1 has no overlay
/// and stays on the base graph.
#[test]
fn a_scene_overlay_reaches_the_diagnosed_graph() {
    let (input, graph) = fixture_sound(Some(0), None);
    assert!(!plate(&graph).bypassed);
    assert_eq!(plate(&graph).params.get("wetdrymix"), Some(&0.7));
    assert_eq!(
        doctor::doctor_tail_ms(&graph),
        crate::leveller::DOCTOR_TAIL_MS
    );
    assert!(diag_keys_on(input, graph).contains(&"washed"));

    let (_, graph) = fixture_sound(Some(1), None);
    assert!(plate(&graph).bypassed);
    assert_eq!(plate(&graph).params.get("wetdrymix"), Some(&0.3));
}

#[test]
fn effective_nodes_ignores_unknown_nodes_and_keeps_an_unknown_graph_unknown() {
    let stray = doctor::NodeOverride {
        group_id: "G9".into(),
        node_id: "nope".into(),
        bypassed: Some(false),
        params: std::collections::HashMap::new(),
    };
    assert!(doctor::effective_nodes(&[], std::slice::from_ref(&stray)).is_empty());
    let (_, base) = fixture_sound(None, None);
    assert_eq!(
        format!("{:?}", doctor::effective_nodes(&base, &[stray])),
        format!("{base:?}")
    );
}

/// Doctor's job scene comes from the frontend, so it is proven against the stored
/// document before any recall: a scene-less preset refuses outright; a past-the-end index
/// refuses only when the read delivered `scenes` whole.
#[test]
fn a_doctor_scene_the_stored_preset_cannot_back_is_refused() {
    use crate::commands::doctor::doctor_scene_refusal;
    let none = serde_json::json!({"scenes": []});
    let two = serde_json::json!({"scenes": [{"sceneName": "A"}, {"sceneName": "B"}]});
    assert!(doctor_scene_refusal(&none, 2, true).is_some());
    assert!(doctor_scene_refusal(&none, 2, false).is_some());
    assert!(doctor_scene_refusal(&two, 1, true).is_none());
    assert!(doctor_scene_refusal(&two, 2, true).is_some());
    assert!(
        doctor_scene_refusal(&two, 2, false).is_none(),
        "a cut list proves nothing about a high index"
    );
}
