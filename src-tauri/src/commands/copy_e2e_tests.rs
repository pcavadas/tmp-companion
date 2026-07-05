//! e2e tests (sim device) for the Copy/Level held-session orchestration.
#![cfg(test)]

mod audition_copy_tests {
    use super::super::*;
    #[test]
    fn copy_job_round_trips_from_frontend_json() {
        let json = r#"{
            "listIndex": 7,
            "name": "Lead Tone",
            "ops": [
                { "kind": "replace", "group": "G1", "nodeId": "ACD_TwinReverb",
                  "repl": { "kind": "model", "fenderId": "ACD_HiwattDR103CanMod" } },
                { "kind": "replace", "group": "M1", "nodeId": "ACD_CabSimTMS",
                  "repl": { "kind": "saved", "fenderId": "ACD_CabSimTMS", "index": 3 } },
                { "kind": "insert", "group": "G1", "beforeFenderId": "ACD_Comp",
                  "repl": { "kind": "ir", "fenderId": "ACD_UserIRTMS", "file": "Oversize.wav" } },
                { "kind": "insert", "group": "G2", "beforeFenderId": null,
                  "repl": { "kind": "model", "fenderId": "ACD_Klon" } },
                { "kind": "remove", "group": "G1", "nodeId": "ACD_Comp" }
            ]
        }"#;
        let job: CopyJob = serde_json::from_str(json).expect("CopyJob deserializes");
        assert_eq!(job.list_index, 7);
        assert_eq!(job.name, "Lead Tone");
        assert_eq!(job.ops.len(), 5);

        // op0: replace → model
        let CopyOp::Replace {
            group,
            node_id,
            repl,
        } = &job.ops[0]
        else {
            panic!("replace")
        };
        assert_eq!(group, "G1");
        assert_eq!(node_id, "ACD_TwinReverb");
        let CopyRepl::Model { fender_id } = repl else {
            panic!("model repl")
        };
        assert_eq!(fender_id, "ACD_HiwattDR103CanMod");

        // op1: replace → saved (index)
        let CopyOp::Replace { repl, .. } = &job.ops[1] else {
            panic!("replace")
        };
        let CopyRepl::Saved { fender_id, index } = repl else {
            panic!("saved repl")
        };
        assert_eq!(fender_id, "ACD_CabSimTMS");
        assert_eq!(*index, 3);

        // op2: insert BEFORE a FenderId → ir (file)
        let CopyOp::Insert {
            group,
            before_fender_id,
            repl,
        } = &job.ops[2]
        else {
            panic!("insert")
        };
        assert_eq!(group, "G1");
        assert_eq!(before_fender_id.as_deref(), Some("ACD_Comp"));
        let CopyRepl::Ir { fender_id, file } = repl else {
            panic!("ir repl")
        };
        assert_eq!(fender_id, "ACD_UserIRTMS");
        assert_eq!(file, "Oversize.wav");
        // The IR's insert id resolves to the placeholder, not the catalog id.
        assert_eq!(repl.insert_fender_id(), "ACD_UserIRTMS");

        // op3: insert APPEND (before = None) → model
        let CopyOp::Insert {
            before_fender_id,
            repl,
            ..
        } = &job.ops[3]
        else {
            panic!("insert")
        };
        assert!(before_fender_id.is_none(), "null beforeFenderId → append");
        assert_eq!(repl.insert_fender_id(), "ACD_Klon");

        // op4: remove
        let CopyOp::Remove { group, node_id } = &job.ops[4] else {
            panic!("remove")
        };
        assert_eq!(group, "G1");
        assert_eq!(node_id, "ACD_Comp");
    }

    /// A `copy_apply` Channel item carries the `BulkReplaceItem` fields plus an OPTIONAL
    /// `graph` that is OMITTED from the wire when `None` (`skip_serializing_if`), so the
    /// no-graph case mirrors `BulkReplaceItem` exactly and the TS `graph?` field stays
    /// optional.
    #[test]
    fn copy_apply_item_serializes_like_bulk_replace_item() {
        let item = CopyApplyItem {
            slot: 7,
            name: "Lead Tone".to_string(),
            outcome: "updated".to_string(),
            detail: "5 op(s)".to_string(),
            graph: None,
        };
        let v = serde_json::to_value(&item).unwrap();
        assert_eq!(v["slot"], 7);
        assert_eq!(v["name"], "Lead Tone");
        assert_eq!(v["outcome"], "updated");
        assert_eq!(v["detail"], "5 op(s)");
        assert!(
            v.get("graph").is_none(),
            "None graph is omitted from the wire"
        );
    }
}

/// End-to-end tests of the held-session Copy/Level orchestration over the in-memory
/// `sim_device::SimDevice` fake — NO hardware. These exercise the real command-layer
/// state machine (`copy_apply_one`'s load → re-arm → confirm-gated edits → identity-
/// preserving save) and the two safety invariants that previously only ever ran on the
/// device: a `presetError` is NEVER followed by a save, and the cold first edit's silent
/// drop is retried. The pure edit→op diff is unit-tested elsewhere (`copyModel` in
/// Vitest, `bulk_cmd` in Rust); this is the wire-driving the diff feeds.
#[cfg(test)]
mod copy_level_e2e_tests {
    use super::super::*;
    use crate::session::Session;
    use crate::sim_device::{SimDevice, SimEvent};

    fn model_replace(group: &str, node: &str, fender: &str) -> CopyOp {
        CopyOp::Replace {
            group: group.into(),
            node_id: node.into(),
            repl: CopyRepl::Model {
                fender_id: fender.into(),
            },
        }
    }

    fn one_replace_job(slot: u32, name: &str) -> CopyJob {
        CopyJob {
            list_index: slot,
            name: name.into(),
            ops: vec![model_replace("G1", "n2", "ACD_DeluxeReverb65")],
        }
    }

    /// Drive `copy_apply_one` over a configured fake device, returning the outcome item
    /// plus the ordered device actions the fake recorded.
    fn run_copy(sim: SimDevice, job: &CopyJob, save: bool) -> (CopyApplyItem, Vec<SimEvent>) {
        let mut s = Session::from_transport(Box::new(sim.clone()));
        let item = copy_apply_one(&mut s, job, save).unwrap();
        (item, sim.events())
    }

    #[test]
    fn copy_replace_happy_path_loads_confirms_renames_and_saves_in_order() {
        let (item, ev) = run_copy(SimDevice::new(), &one_replace_job(5, "Stadium Lead"), true);
        assert_eq!(item.outcome, "updated");
        assert_eq!(
            ev,
            vec![
                SimEvent::Loaded(5),
                SimEvent::Replace {
                    group: "G1".into(),
                    node_id: "n2".into(),
                    fender_id: "ACD_DeluxeReverb65".into(),
                },
                SimEvent::Renamed("Stadium Lead".into()),
                SimEvent::Saved(5),
            ]
        );
    }

    #[test]
    fn copy_multi_op_applies_remove_replace_insert_in_order_then_saves() {
        let job = CopyJob {
            list_index: 3,
            name: "Clean Verse".into(),
            ops: vec![
                CopyOp::Remove {
                    group: "G1".into(),
                    node_id: "n1".into(),
                },
                model_replace("G1", "n2", "ACD_Klon"),
                CopyOp::Insert {
                    group: "G1".into(),
                    before_fender_id: Some("ACD_Klon".into()),
                    repl: CopyRepl::Model {
                        fender_id: "ACD_TapeEcho".into(),
                    },
                },
            ],
        };
        let (item, ev) = run_copy(SimDevice::new(), &job, true);
        assert_eq!(item.outcome, "updated");
        assert_eq!(
            ev,
            vec![
                SimEvent::Loaded(3),
                SimEvent::Remove {
                    group: "G1".into(),
                    node_id: "n1".into(),
                },
                SimEvent::Replace {
                    group: "G1".into(),
                    node_id: "n2".into(),
                    fender_id: "ACD_Klon".into(),
                },
                SimEvent::Insert {
                    group: "G1".into(),
                    before: Some("ACD_Klon".into()),
                    fender_id: "ACD_TapeEcho".into(),
                },
                SimEvent::Renamed("Clean Verse".into()),
                SimEvent::Saved(3),
            ]
        );
    }

    #[test]
    fn copy_preset_error_is_never_followed_by_a_save() {
        // The device REJECTS the edit (presetError 53) — copy must report an error and
        // MUST NOT rename or save (an unconfirmed save corrupted a real preset).
        let (item, ev) = run_copy(
            SimDevice::new().with_reject_at(1),
            &one_replace_job(5, "Stadium Lead"),
            true,
        );
        assert_eq!(item.outcome, "error");
        assert!(item.detail.contains("NOT saved"), "detail: {}", item.detail);
        assert!(
            !ev.iter()
                .any(|e| matches!(e, SimEvent::Saved(_) | SimEvent::Renamed(_))),
            "a rejected edit must not save/rename: {ev:?}"
        );
    }

    #[test]
    fn copy_cold_first_edit_silent_drop_is_retried_then_saved() {
        // The first structural edit after a fresh load is silently DROPPED; the held
        // path retries it once and then confirms + saves.
        let (item, ev) = run_copy(
            SimDevice::new().with_drop_first(),
            &one_replace_job(5, "Stadium Lead"),
            true,
        );
        assert_eq!(item.outcome, "updated");
        let replaces = ev
            .iter()
            .filter(|e| matches!(e, SimEvent::Replace { .. }))
            .count();
        assert_eq!(replaces, 2, "the dropped edit + its retry: {ev:?}");
        assert!(ev.iter().any(|e| matches!(e, SimEvent::Saved(5))));
    }

    #[test]
    fn copy_dry_run_applies_edits_but_does_not_save() {
        let (item, ev) = run_copy(SimDevice::new(), &one_replace_job(5, "Stadium Lead"), false);
        assert_eq!(item.outcome, "updated");
        assert!(ev.iter().any(|e| matches!(e, SimEvent::Replace { .. })));
        assert!(
            !ev.iter()
                .any(|e| matches!(e, SimEvent::Saved(_) | SimEvent::Renamed(_))),
            "dry run must not persist: {ev:?}"
        );
    }

    #[test]
    fn copy_empty_op_list_is_skipped_without_touching_the_device() {
        let job = CopyJob {
            list_index: 2,
            name: "Untouched".into(),
            ops: vec![],
        };
        let (item, ev) = run_copy(SimDevice::new(), &job, true);
        assert_eq!(item.outcome, "skipped");
        assert!(ev.is_empty(), "no ops → no device traffic");
    }

    #[test]
    fn level_setter_roundtrip_echoes_and_records() {
        // The Level seam over the fake: set_preset_level's wire encoding round-trips and
        // the device's PresetLevelChanged(77) echo parses back to the value sent.
        let sim = SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        let echo = s.set_preset_level(0.5).unwrap();
        assert!((echo.expect("level echo") - 0.5).abs() < 1e-6);
        s.save_current_preset(7).unwrap();
        assert_eq!(
            sim.events(),
            vec![SimEvent::PresetLevel(0.5), SimEvent::Saved(7)]
        );
    }

    #[test]
    fn copy_partial_failure_saves_only_the_confirmed_targets() {
        // The multi-target copy loop runs every job on ONE held session. With the SECOND
        // target's edit rejected (presetError), the FIRST must still confirm + save and the
        // rejected one must NOT save — a batch is partial-success, never all-or-nothing, and
        // a rejected target never gets a wrong-content save. (The frontend then patches the
        // cache for the "updated" target only — CopyView.tsx's `outcome === "updated"` gate.)
        let sim = SimDevice::new().with_reject_at(2);
        let mut s = Session::from_transport(Box::new(sim.clone()));
        let a = copy_apply_one(&mut s, &one_replace_job(5, "Target A"), true).unwrap();
        let b = copy_apply_one(&mut s, &one_replace_job(6, "Target B"), true).unwrap();
        assert_eq!(a.outcome, "updated");
        assert_eq!(b.outcome, "error");
        let ev = sim.events();
        assert!(
            ev.contains(&SimEvent::Saved(5)),
            "confirmed target A must save: {ev:?}"
        );
        assert!(
            !ev.contains(&SimEvent::Saved(6)),
            "rejected target B must NOT save: {ev:?}"
        );
    }

    #[test]
    fn copy_apply_refuses_an_over_cap_insert_end_to_end() {
        // The device does NOT enforce the 5 firmware block-count caps (C-1, see
        // `blockcaps.rs`'s module docs) — this is the guard's REAL test: an over-cap
        // edit must be refused HERE, before it ever reaches the device, not by any
        // device response (the fake would happily confirm it, like the real firmware).
        let sim = SimDevice::new().with_preset_json(
            r#"{"audioGraph":{"guitarNodes":{"G1":[
                {"FenderId":"ACD_AC30BrilliantCabIR","nodeId":"n1"},
                {"FenderId":"ACD_AC30NormalCabIR","nodeId":"n2"}
            ]}}}"#,
        );
        let mut s = Session::from_transport(Box::new(sim.clone()));
        let job = CopyJob {
            list_index: 5,
            name: "Stadium Lead".into(),
            ops: vec![CopyOp::Insert {
                group: "G1".into(),
                before_fender_id: None,
                repl: CopyRepl::Model {
                    fender_id: "ACD_Ampeg66B15CabIR".into(), // a 3rd cabinet member
                },
            }],
        };
        let item = copy_apply_one(&mut s, &job, true).unwrap();
        assert_eq!(item.outcome, "error");
        assert!(
            item.detail.contains("ComboHalfStackCabinetsLimit"),
            "detail: {}",
            item.detail
        );
        let ev = sim.events();
        assert!(
            !ev.iter().any(|e| matches!(
                e,
                SimEvent::Insert { .. } | SimEvent::Saved(_) | SimEvent::Renamed(_)
            )),
            "an over-cap insert must never reach the device, let alone save: {ev:?}"
        );
    }
    // ── PR2: confirm-before-save write safety (Session::confirm_active) ──

    #[test]
    fn confirm_active_ok_when_slot_echo_matches() {
        // A load the device echoed (PresetLoaded) confirms via the SLOT identity.
        let sim = SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.send_and_collect(&crate::proto::load_preset(6, 1), 50)
            .unwrap(); // dev slot 6 = 0-based list index 5
        assert!(
            s.confirm_active(5, None).is_ok(),
            "slot echo should confirm: loaded={:?}",
            s.loaded_slot()
        );
        s.save_current_preset(5).unwrap();
        assert!(sim.events().iter().any(|e| matches!(e, SimEvent::Saved(5))));
    }

    #[test]
    fn confirm_active_errs_and_blocks_save_when_load_dropped() {
        // No PresetLoaded echo and no matching active name (a dropped load) ⇒ confirm
        // errs, and a caller using `?` never reaches the save (no wrong-content write).
        let sim = SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        let attempt = |s: &mut Session| -> Result<(), String> {
            s.confirm_active(7, Some("Target"))?;
            s.save_current_preset(7)?; // MUST NOT run
            Ok(())
        };
        assert!(attempt(&mut s).is_err(), "unconfirmed load must not save");
        assert!(
            !sim.events().iter().any(|e| matches!(e, SimEvent::Saved(_))),
            "no save on an unconfirmed load: {:?}",
            sim.events()
        );
    }

    #[test]
    fn confirm_active_errs_when_a_different_preset_is_active() {
        // The device says a DIFFERENT slot is active — a possibly-duplicate name must not
        // override the contradicting slot echo, so confirm errs (never edit the wrong one).
        let sim = SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.send_and_collect(&crate::proto::load_preset(4, 1), 50)
            .unwrap(); // dev slot 4 = 0-based list index 3
        assert!(
            s.confirm_active(5, Some("Target")).is_err(),
            "slot 3 active but target 5 — must not confirm"
        );
    }

    // ── PR2: migration snapshot-before-write (AC2) ──

    #[test]
    fn migration_refuses_to_snapshot_into_an_unwritable_dir() {
        // A path whose parent is not a directory ⇒ the snapshot fails; migration_apply
        // then skips the write and keeps the preset revertible.
        let bad = std::path::Path::new("/dev/null/tmp-companion-cannot-mkdir");
        let r = crate::commands::migration::snapshot_before_migrate(bad, 3, "Cliff", r#"{"info":{"preset_id":"x"}}"#);
        assert!(r.is_err(), "unwritable backup dir must refuse: {r:?}");
    }

    #[test]
    fn migration_snapshot_captures_the_pre_edit_json() {
        let dir = std::env::temp_dir().join(format!(
            "tmp-companion-migsnap-{}",
            crate::bulkrun::now_stamp()
        ));
        let before = r#"{"info":{"preset_id":"abc","displayName":"Cliff"}}"#;
        let p = crate::commands::migration::snapshot_before_migrate(&dir, 3, "Cliff", before).unwrap();
        let content = std::fs::read_to_string(&p).unwrap();
        assert!(
            content.contains("abc"),
            "snapshot must carry the pre-edit json: {content}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
