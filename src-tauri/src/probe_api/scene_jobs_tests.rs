//! Scene-leveling job-planning unit tests (sibling of `scene_jobs.rs`).

use super::*;

#[test]
fn amp_output_level_param_is_output_level_only() {
    assert!(is_amp_output_level_param("outputLevel"));
    assert!(!is_amp_output_level_param("output"));
    assert!(!is_amp_output_level_param("outputlevel"));
    assert!(!is_amp_output_level_param("level"));
    assert!(!is_amp_output_level_param("brightvolume"));
    assert!(!is_amp_output_level_param("mastervolume"));
    assert!(!is_amp_output_level_param("normalvolume"));
    assert!(!is_amp_output_level_param("volume"));
}

#[test]
fn amp_model_id_matches_merged_cab_ir_variant() {
    // Bare amp bid (separate cab block).
    assert!(is_amp_model_id("ACD_HiwattDR103CanMod"));
    // Amp+cab combo block carries a merged "CabIR" suffix the catalog bid lacks
    // → stripped to the bare bid.
    assert!(is_amp_model_id("ACD_HiwattDR103CanModCabIR"));
    // Reverb amps are catalogued WITH the suffix → must match directly (check-first),
    // NOT be over-stripped to a non-existent bare bid.
    assert!(is_amp_model_id("ACD_PrincetonReverb68CabIRConvRvb"));
    // Wet amp id whose base is catalogued ONLY with the NoFx token: strips to the
    // bare id (which misses), then the +NoFx bridge matches …BlondeVibratoNoFx.
    assert!(is_amp_model_id(
        "ACD_DeluxeReverb65BlondeVibratoCabIRConvRvb"
    ));
    // A non-amp block is still rejected (and +NoFx must not conjure a false match).
    assert!(!is_amp_model_id("ACD_TMReverse"));
}

#[test]
fn scene_jobs_prefer_active_amp_output_level_over_preamp_volume() {
    let doc = serde_json::json!({
        "audioGraph": { "template": "gtrSeries", "guitarNodes": { "G1": [
            {
                "nodeId": "ACD_HiwattDR103CanMod",
                "FenderId": "ACD_HiwattDR103CanMod",
                "dspUnitParameters": {
                    "bypass": false,
                    "brightvolume": 0.5,
                    "outputLevel": 1.0
                }
            }
        ] } }
    });
    let candidates = vec![
        LevelBlockArg {
            group_id: "G1".to_string(),
            node_id: "ACD_HiwattDR103CanMod".to_string(),
            parameter_id: "brightvolume".to_string(),
            value: 0.5,
        },
        LevelBlockArg {
            group_id: "G1".to_string(),
            node_id: "ACD_HiwattDR103CanMod".to_string(),
            parameter_id: "outputLevel".to_string(),
            value: 0.34,
        },
    ];

    let jobs = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))]).unwrap();
    let leveller::LevelKnob::Block { parameter_id, .. } = &jobs[0].knobs[0].knob else {
        panic!("expected block knob");
    };
    assert_eq!(parameter_id, "outputLevel");
    assert_eq!(jobs[0].knobs[0].current, 1.0);
}

#[test]
fn scene_jobs_reject_preamp_volume_as_level_control() {
    let doc = serde_json::json!({
        "audioGraph": { "template": "gtrSeries", "guitarNodes": { "G1": [
            {
                "nodeId": "ACD_HiwattDR103CanMod",
                "FenderId": "ACD_HiwattDR103CanMod",
                "dspUnitParameters": {
                    "bypass": false,
                    "mastervolume": 1.0
                }
            }
        ] } }
    });
    let candidates = vec![LevelBlockArg {
        group_id: "G1".to_string(),
        node_id: "ACD_HiwattDR103CanMod".to_string(),
        parameter_id: "mastervolume".to_string(),
        value: 1.0,
    }];

    // The Hiwatt is an active amp but its only candidate is a preamp volume, not
    // outputLevel → the scene is skipped with a reason, not leveled on the wrong knob.
    let err = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))]).unwrap_err();
    assert!(err.contains("outputLevel"), "got: {err}");
}

// Parallel-merged (gtrParallel1): an amp in each split lane (G2 | G3), no post-merge
// amp → BOTH amps become the joint-k knob set (not just the first).
#[test]
fn scene_jobs_parallel_merged_picks_both_lane_amps() {
    let amp = |fid: &str| {
        serde_json::json!({
            "nodeId": fid, "FenderId": fid,
            "dspUnitParameters": { "bypass": false, "outputLevel": 0.5 }
        })
    };
    let doc = serde_json::json!({
        "audioGraph": { "template": "gtrParallel1", "guitarNodes": {
            "G1": [],
            "G2": [ amp("ACD_TM59Bassman") ],
            "G3": [ amp("ACD_HiwattDR103CanMod") ]
        } }
    });
    let candidates = vec![
        LevelBlockArg {
            group_id: "G2".into(),
            node_id: "ACD_TM59Bassman".into(),
            parameter_id: "outputLevel".into(),
            value: 0.5,
        },
        LevelBlockArg {
            group_id: "G3".into(),
            node_id: "ACD_HiwattDR103CanMod".into(),
            parameter_id: "outputLevel".into(),
            value: 0.5,
        },
    ];
    let jobs = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))]).unwrap();
    assert_eq!(
        jobs[0].knobs.len(),
        2,
        "both lane amps drive together (joint-k)"
    );
    let groups: std::collections::HashSet<_> = jobs[0]
        .knobs
        .iter()
        .map(|kt| match &kt.knob {
            leveller::LevelKnob::Block { group_id, .. } => group_id.clone(),
            _ => panic!("block knob"),
        })
        .collect();
    assert!(groups.contains("G2") && groups.contains("G3"));
}

// gtrParallel1 with a post-merge amp (G4, after the G2|G3 split) → that single amp is
// the series master, NOT a 2-knob joint-k.
#[test]
fn scene_jobs_post_merge_amp_is_single_master() {
    let amp = |fid: &str| {
        serde_json::json!({
            "nodeId": fid, "FenderId": fid,
            "dspUnitParameters": { "bypass": false, "outputLevel": 0.5 }
        })
    };
    let doc = serde_json::json!({
        "audioGraph": { "template": "gtrParallel1", "guitarNodes": {
            "G1": [], "G2": [], "G3": [],
            "G4": [ amp("ACD_HiwattDR103CanMod") ]
        } }
    });
    let candidates = vec![LevelBlockArg {
        group_id: "G4".into(),
        node_id: "ACD_HiwattDR103CanMod".into(),
        parameter_id: "outputLevel".into(),
        value: 0.5,
    }];
    let jobs = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))]).unwrap();
    assert_eq!(jobs[0].knobs.len(), 1);
}

// No known template (truncated read) → skip with a reason, NEVER a silent
// single-amp series fallback.
#[test]
fn scene_jobs_skip_when_template_unknown() {
    let doc = serde_json::json!({
        "audioGraph": { "guitarNodes": { "G1": [
            { "nodeId": "ACD_TwinReverb", "FenderId": "ACD_TwinReverb",
              "dspUnitParameters": { "bypass": false, "outputLevel": 0.5 } }
        ] } }
    });
    let candidates = vec![LevelBlockArg {
        group_id: "G1".into(),
        node_id: "ACD_TwinReverb".into(),
        parameter_id: "outputLevel".into(),
        value: 0.5,
    }];
    let err = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))]).unwrap_err();
    assert!(err.contains("routing"), "got: {err}");
}

// Mic-only routing has no guitar amp the instrument re-amp can drive → the scene is
// SKIPPED (per-scene, not a hard error); we level only what reaches USB 1/2.
#[test]
fn scene_jobs_skip_mic_only_no_guitar_amp() {
    let doc = serde_json::json!({
        "audioGraph": { "template": "micSeries", "guitarNodes": { "G1": [] },
            "micNodes": { "M1": [
                { "nodeId": "ACD_HiwattDR103CanMod", "FenderId": "ACD_HiwattDR103CanMod",
                  "dspUnitParameters": { "bypass": false, "outputLevel": 0.5 } }
            ] } }
    });
    let candidates = vec![LevelBlockArg {
        group_id: "M1".into(),
        node_id: "ACD_HiwattDR103CanMod".into(),
        parameter_id: "outputLevel".into(),
        value: 0.5,
    }];
    let jobs = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))]).unwrap();
    assert!(
        jobs[0].skip.as_deref().unwrap_or("").contains("guitar amp"),
        "got: {:?}",
        jobs[0].skip
    );
}

// Split-output (gtrSplit): an amp in each output lane (OUT 1 / OUT 2) → both join the
// joint-k set, measured at USB 1/2. No routing read; the user controls what's on USB.
#[test]
fn scene_jobs_split_output_joint_ks_both_output_lanes() {
    let amp = |fid: &str| {
        serde_json::json!({
            "nodeId": fid, "FenderId": fid,
            "dspUnitParameters": { "bypass": false, "outputLevel": 0.5 }
        })
    };
    // gtrSplit: stages=[Series{G1}], outputs={a: G2-G4, b: G5-G7}.
    let doc = serde_json::json!({
        "audioGraph": { "template": "gtrSplit", "guitarNodes": {
            "G1": [], "G2": [ amp("ACD_TM59Bassman") ], "G3": [], "G4": [],
            "G5": [ amp("ACD_HiwattDR103CanMod") ], "G6": [], "G7": []
        } }
    });
    let candidates = vec![
        LevelBlockArg {
            group_id: "G2".into(),
            node_id: "ACD_TM59Bassman".into(),
            parameter_id: "outputLevel".into(),
            value: 0.5,
        },
        LevelBlockArg {
            group_id: "G5".into(),
            node_id: "ACD_HiwattDR103CanMod".into(),
            parameter_id: "outputLevel".into(),
            value: 0.5,
        },
    ];
    let jobs = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))]).unwrap();
    assert_eq!(
        jobs[0].knobs.len(),
        2,
        "both output-lane amps drive together"
    );
}

// A per-SCENE issue (this scene bypasses its only amp) becomes a SKIP job, NOT a hard
// error — one bad scene must not abort the batch (the runner reports it skipped).
#[test]
fn scene_jobs_per_scene_skip_does_not_abort() {
    let bypassed = serde_json::json!({
        "audioGraph": { "template": "gtrSeries", "guitarNodes": { "G1": [
            { "nodeId": "ACD_HiwattDR103CanMod", "FenderId": "ACD_HiwattDR103CanMod",
              "dspUnitParameters": { "bypass": true, "outputLevel": 0.5 } }
        ] } }
    });
    let active = serde_json::json!({
        "audioGraph": { "template": "gtrSeries", "guitarNodes": { "G1": [
            { "nodeId": "ACD_HiwattDR103CanMod", "FenderId": "ACD_HiwattDR103CanMod",
              "dspUnitParameters": { "bypass": false, "outputLevel": 0.5 } }
        ] } }
    });
    let candidates = vec![LevelBlockArg {
        group_id: "G1".into(),
        node_id: "ACD_HiwattDR103CanMod".into(),
        parameter_id: "outputLevel".into(),
        value: 0.5,
    }];
    let jobs = build_scene_jobs(
        &[0, 1],
        &candidates,
        &[(0, Some(bypassed)), (1, Some(active))],
    )
    .unwrap();
    assert_eq!(jobs.len(), 2);
    assert!(jobs[0].skip.is_some(), "bypassed-amp scene is skipped");
    assert!(jobs[0].knobs.is_empty());
    assert!(jobs[1].skip.is_none(), "active-amp scene levels normally");
    assert_eq!(jobs[1].knobs.len(), 1);
}
