//! Unit tests for the Doctor command layer (moved from lib.rs `audition_tests`).
use super::*;
use crate::doctor;

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
    // A save invalidates (clear_doctor_before_cache is what doctor_save calls).
    clear_doctor_before_cache();
    assert_eq!(before_cache_get(&key), None);
}
