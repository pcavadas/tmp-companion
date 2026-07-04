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
