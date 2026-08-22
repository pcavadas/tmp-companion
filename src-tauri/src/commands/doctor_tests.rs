//! Unit tests for the Doctor command layer (moved from lib.rs `audition_tests`).
use super::*;
use crate::doctor;

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
        cache.insert(7u32, serde_json::Value::Null);
        resolve_sound_isolation(&[], &[], None, footswitch, 7, &mut cache)
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
    cache.insert(7u32, serde_json::json!({ "ftsw": [[], [], []] }));
    assert!(
        resolve_sound_isolation(&[], &[], None, Some(2), 7, &mut cache)
            .unresolved
            .is_none()
    );
}

/// The exact camelCase JSON the Doctor apply frontend sends deserializes into
/// [`DoctorApplyJob`] — a `param` op and an `insert_node` op (the DoctorOp tag
/// values + field renames pinned by doctor.rs's `doctor_op_serializes_camel_case`).
#[test]
fn doctor_apply_job_round_trips_from_frontend_json() {
    let json = r#"{
            "listIndex": 4,
            "name": "Lead Tone",
            "ops": [
                { "kind": "param", "groupId": "G1", "nodeId": "ACD_CabSimTMS",
                  "param": "lpf", "value": 8000.0 },
                { "kind": "insert_node", "groupId": "G1", "beforeFenderId": null,
                  "fenderId": "ACD_TenBandEQStereo", "params": [["gain250hz", -3.0]] }
            ],
            "topologyId": "guitar-humbucker",
            "calibrationLufs": -18.0
        }"#;
    let job: DoctorApplyJob = serde_json::from_str(json).expect("DoctorApplyJob deserializes");
    assert_eq!(job.list_index, 4);
    assert_eq!(job.name, "Lead Tone");
    assert_eq!(job.topology_id.as_deref(), Some("guitar-humbucker"));
    assert_eq!(job.calibration_lufs, Some(-18.0));
    assert_eq!(job.ops.len(), 2);
    assert!(matches!(job.ops[0], doctor::DoctorOp::Param { .. }));
    match &job.ops[1] {
        doctor::DoctorOp::InsertNode {
            fender_id,
            before_fender_id,
            params,
            ..
        } => {
            assert_eq!(fender_id, "ACD_TenBandEQStereo");
            assert!(before_fender_id.is_none());
            assert_eq!(params[0], ("gain250hz".to_string(), -3.0));
        }
        other => panic!("expected InsertNode, got {other:?}"),
    }
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
    let row = DoctorSoundResult {
        key: input.key,
        list_index: input.list_index,
        scene: input.scene,
        footswitch: input.footswitch,
        label: input.label,
        tag: input.tag,
        diags: Vec::new(),
        integrated_lufs: 0.0,
        tail_ratio_db: 0.0,
        balance_db: Vec::new(),
        band_labels: Vec::new(),
        cut_through: None,
        error: None,
        skipped_band_count: 0,
    };
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
    let row = DoctorSoundResult {
        key: "p4".to_string(),
        list_index: 4,
        scene: None,
        footswitch: None,
        label: "Base".to_string(),
        tag: None,
        diags: Vec::new(),
        integrated_lufs: 0.0,
        tail_ratio_db: 0.0,
        balance_db: Vec::new(),
        band_labels: Vec::new(),
        cut_through: Some(doctor::CutThrough {
            contrast_db: 12.5,
            factory_percentile: Some(63.2),
            advisory: false,
        }),
        error: None,
        skipped_band_count: 0,
    };
    let v = serde_json::to_value(&row).unwrap();
    assert_eq!(v["cutThrough"]["contrastDb"], 12.5);
    assert_eq!(v["cutThrough"]["factoryPercentile"], 63.2);
    assert_eq!(v["cutThrough"]["advisory"], false);
}

#[test]
fn doctor_apply_result_serializes_camel_case() {
    let r = DoctorApplyResult {
        before_clip: "data:audio/wav;base64,AAA".into(),
        after_clip: "data:audio/wav;base64,BBB".into(),
    };
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["beforeClip"], "data:audio/wav;base64,AAA");
    assert_eq!(v["afterClip"], "data:audio/wav;base64,BBB");
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
        let Some(nid) = obj.get("nodeId").and_then(serde_json::Value::as_str) else {
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
    PrevSound { list_index, wrote }
}

#[test]
fn skip_load_only_for_a_clean_ok_same_preset_scene_chain() {
    // The one allowed case: same preset, previous sound clean, current is a scene.
    assert!(doctor_skip_load(Some(&prev(3, false)), 3, true));
    // First sound of the run — and any sound after an ERRORED one (the loop resets
    // prev to None on error) — never skips.
    assert!(!doctor_skip_load(None, 3, true));
    // Different preset → reload.
    assert!(!doctor_skip_load(Some(&prev(2, false)), 3, true));
    // Previous sound wrote force-bypasses (base/footswitch) → reload.
    assert!(!doctor_skip_load(Some(&prev(3, true)), 3, true));
    // Base/footswitch sounds always reload, even after a clean scene.
    assert!(!doctor_skip_load(Some(&prev(3, false)), 3, false));
}

// --- floor_error_for: silent-inject guard on the Doctor's capture spread ---

#[test]
fn floor_error_for_flags_a_flat_capture_against_a_lively_stimulus() {
    assert_eq!(floor_error_for(0.01, 6.0), Some(leveller::FLOOR_READ_ERR));
}

#[test]
fn floor_error_for_clears_a_live_capture() {
    assert_eq!(floor_error_for(4.0, 6.0), None);
}

#[test]
fn floor_error_for_disarms_on_a_near_stationary_stimulus() {
    // stimulus spread ≤ STATIONARY_STIM_LU (0.30) can't discriminate by spread —
    // the guard must not fire even though the capture itself reads flat.
    assert_eq!(floor_error_for(0.01, 0.2), None);
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
    let iso = resolve_sound_isolation(&nodes, &infos, Some(0), None, 5, &mut cache);
    assert!(iso.bypass.is_empty() && iso.params.is_empty());
    // Graph absent too — the pre-existing empty-nodes behavior for scenes.
    let iso = resolve_sound_isolation(&[], &infos, Some(0), None, 5, &mut cache);
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
    let iso = resolve_sound_isolation(&nodes, &infos, None, Some(2), 5, &mut cache);
    assert_eq!(
        iso.params,
        vec![("G1".into(), "MOD".into(), "gain".into(), 0.9_f32)]
    );
    let _ = preset; // fixture kept alive alongside its enumerated infos
}

// --- bypass_only_conflict: fix P3-2, refuse a scene-context write that would leak to base ---

/// A minimal preset carrying one node (`ampA`/`ACD_TwinReverb`, group G1) with
/// scene 0's overlay set to `overlay_params` — the ONE fixture shape
/// `scene_overlay` itself is pinned against, shared from its own test module.
use crate::probe_api::scene_jobs::scene_jobs_tests::with_scene0_overlay as preset_with_scene0_overlay;
use crate::probe_api::scene_jobs::scene_jobs_tests::{hbe_boost_preset, HBE_NODE, HBE_PARAM};

fn param_op(node_id: &str) -> doctor::DoctorOp {
    doctor::DoctorOp::Param {
        group_id: "G1".to_string(),
        node_id: node_id.to_string(),
        param: "outputLevel".to_string(),
        value: 0.6,
    }
}

#[test]
fn bypass_only_conflict_refuses_on_a_bypass_only_overlay() {
    // scene 0's overlay carries ONLY the bypass family — Scene Edit is OFF, the
    // node's knobs are shared with base (`SceneWriteVerdict::Refuse`).
    let preset = preset_with_scene0_overlay(serde_json::json!({ "bypass": false }));
    let ops = vec![param_op("ampA")];
    let reason = bypass_only_conflict(&preset, 0, &ops).expect("BypassOnly refuses");
    assert!(reason.contains("ampA"));
}

#[test]
fn bypass_only_conflict_allows_a_full_overlay() {
    // scene 0's overlay carries a real knob alongside bypass — Scene Edit is ON,
    // the write lands in the overlay, not base (`SceneWriteVerdict::WriteDirect`).
    let preset =
        preset_with_scene0_overlay(serde_json::json!({ "bypass": false, "outputLevel": 0.2 }));
    let ops = vec![param_op("ampA")];
    assert_eq!(bypass_only_conflict(&preset, 0, &ops), None);
}

#[test]
fn bypass_only_conflict_ignores_insert_node_ops() {
    // InsertNode is never scene-scoped (block topology is shared across every
    // scene) — a BypassOnly overlay on an unrelated node must not block it.
    let preset = preset_with_scene0_overlay(serde_json::json!({ "bypass": false }));
    let ops = vec![doctor::DoctorOp::InsertNode {
        group_id: "G1".to_string(),
        before_fender_id: None,
        fender_id: "ACD_TenBandEQStereo".to_string(),
        params: Vec::new(),
    }];
    assert_eq!(bypass_only_conflict(&preset, 0, &ops), None);
}

#[test]
fn bypass_only_conflict_refuses_on_an_absent_overlay_too() {
    // Fix P3-2 widening: scene 1 of the fixture carries NO overlay for "ampA" at
    // all (`SceneOverlay::Absent` → `SceneWriteVerdict::NeedsEnable`) — Doctor has
    // no enable/repair pass, so this now refuses too instead of leaking to base
    // (previously a prose-only, unenforced limitation).
    let preset = preset_with_scene0_overlay(serde_json::json!({ "bypass": false }));
    let ops = vec![param_op("ampA")];
    let reason = bypass_only_conflict(&preset, 1, &ops).expect("Absent overlay refuses");
    assert!(reason.contains("ampA"));
}

#[test]
fn bypass_only_conflict_refuses_on_an_unknown_overlay() {
    // No `scenes` array at all (mirrors a truncated field-8 read — 22/25 real
    // presets read "scenes unknown") — `scene_write_verdict_for_param` can't tell Absent
    // from a cut, so it refuses rather than risk either write shape.
    let preset = serde_json::json!({});
    let ops = vec![param_op("ampA")];
    let reason = bypass_only_conflict(&preset, 0, &ops).expect("Unknown overlay refuses");
    assert!(reason.contains("ampA"));
}

/// BUG→GATE (widened policy's second consumer, "Friedman HBE" preset 28):
/// `bypass_only_conflict` calls the SAME `scene_write_verdict_for_param` the scene-leveling
/// lane does, so its own audibility-cleared allow arm (`WriteDirect{lands_on_base:true}`)
/// must reach Doctor too, not just the leveller. `hbe_boost_preset()` (shared from
/// `scene_jobs_tests`) is the exact anatomy: `boost`/`ACD_Boost` bypassed in base, a
/// bypass-only un-bypass overlay in ONLY scene 2 "Solo", no footswitch/EXP assign targeting
/// it, and every other scene either pins its own `gain` (Full) or stays bypassed
/// (bypass-only) — Solo is the sole scene the shared write is audible in. Doctor's prescribe
/// for Solo must NOT report a conflict here.
#[test]
fn bypass_only_conflict_allows_the_audibility_cleared_shared_write() {
    let preset = hbe_boost_preset();
    let ops = vec![doctor::DoctorOp::Param {
        group_id: "G1".to_string(),
        node_id: HBE_NODE.to_string(),
        param: HBE_PARAM.to_string(),
        value: 4.0,
    }];
    assert_eq!(
        bypass_only_conflict(&preset, 2, &ops),
        None,
        "Solo (scene 2): the shared write is audible only here, so Doctor must be allowed \
         through, not refused as shared_with_base"
    );
}

// --- doctor_apply BEFORE-clip cache ---
//
// BEFORE_CACHE is a process-global static; cargo runs tests in parallel, so the
// tests that mutate it must serialize on this lock or they stomp each other's
// entries (a hard-to-spot cross-contamination — same pattern as
// `e2e_server_tests::SERIAL`).
static BEFORE_CACHE_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn before_cache_hits_only_the_exact_sound_and_stimulus() {
    let _guard = crate::lock_ok(&BEFORE_CACHE_SERIAL);
    clear_doctor_before_cache();
    let key: BeforeKey = (
        7,
        "Lead".into(),
        "/stim/tele.wav".into(),
        Some(0xC196_0000),
        None,
        None,
    );
    before_cache_put(key.clone(), "clip-a".into());
    assert_eq!(before_cache_get(&key), Some("clip-a".into()));
    // Any identity change misses: renamed preset, different stimulus, different cal.
    assert_eq!(
        before_cache_get(&(
            7,
            "Lead 2".into(),
            "/stim/tele.wav".into(),
            key.3,
            key.4,
            key.5
        )),
        None
    );
    assert_eq!(
        before_cache_get(&(
            7,
            "Lead".into(),
            "/stim/strat.wav".into(),
            key.3,
            key.4,
            key.5
        )),
        None
    );
    assert_eq!(
        before_cache_get(&(
            7,
            "Lead".into(),
            "/stim/tele.wav".into(),
            None,
            key.4,
            key.5
        )),
        None
    );
    // ...or a different scene/footswitch of the SAME preset.
    assert_eq!(
        before_cache_get(&(
            7,
            "Lead".into(),
            "/stim/tele.wav".into(),
            key.3,
            Some(1),
            key.5
        )),
        None
    );
    assert_eq!(
        before_cache_get(&(
            7,
            "Lead".into(),
            "/stim/tele.wav".into(),
            key.3,
            key.4,
            Some(2)
        )),
        None
    );
    // A save invalidates (clear_doctor_before_cache is what doctor_save calls).
    clear_doctor_before_cache();
    assert_eq!(before_cache_get(&key), None);
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
    s.load_preset(0).expect("load_preset"); // activates saved scene 3, not base
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
