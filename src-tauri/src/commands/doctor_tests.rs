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
        error: None,
    };
    let v = serde_json::to_value(&row).unwrap();
    assert_eq!(v["footswitch"], 0);
}

#[test]
fn doctor_validate_ops_rejects_scene_trim() {
    let ok = vec![doctor::DoctorOp::Param {
        group_id: "G1".into(),
        node_id: "ACD_CabSimTMS".into(),
        param: "lpf".into(),
        value: 8000.0,
    }];
    assert!(doctor_validate_ops(&ok).is_ok());
    let bad = vec![doctor::DoctorOp::SceneTrim {
        scene: 1,
        target_delta_db: 2.0,
    }];
    assert!(doctor_validate_ops(&bad).is_err());
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
