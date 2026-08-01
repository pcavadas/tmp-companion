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

    let jobs = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))], -23.0, None).unwrap();
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
    let err = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))], -23.0, None).unwrap_err();
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
    let jobs = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))], -23.0, None).unwrap();
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
    let jobs = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))], -23.0, None).unwrap();
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
    let err = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))], -23.0, None).unwrap_err();
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
    let jobs = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))], -23.0, None).unwrap();
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
    // gtrSplit: stages=[Series{G1}], outputs={a: G2, b: G3} (HW-confirmed: each
    // output lane is one whole device group, not a bunched multi-group half).
    let doc = serde_json::json!({
        "audioGraph": { "template": "gtrSplit", "guitarNodes": {
            "G1": [], "G2": [ amp("ACD_TM59Bassman") ],
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
    let jobs = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))], -23.0, None).unwrap();
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
        -23.0,
        None,
    )
    .unwrap();
    assert_eq!(jobs.len(), 2);
    assert!(jobs[0].skip.is_some(), "bypassed-amp scene is skipped");
    assert!(jobs[0].knobs.is_empty());
    assert!(jobs[1].skip.is_none(), "active-amp scene levels normally");
    assert_eq!(jobs[1].knobs.len(), 1);
}

// --- scene_docs_from_saved: synthetic per-scene docs from a SAVED (field-8) preset ---

// A SAVED preset: one amp ON in the base with a sparse overlay in scene 0 (flips the amp
// active + bumps outputLevel + tweaks splitMix) and an empty overlay in scene 1.
fn saved_preset() -> serde_json::Value {
    serde_json::json!({
        "lastLoadedScene": 2,
        "audioGraph": {
            "template": "gtrSeries",
            "splitMix": { "balance": 0.5, "level": 0.8 },
            "guitarNodes": { "G1": [
                { "nodeId": "ampA", "FenderId": "ACD_TwinReverb",
                  "dspUnitParameters": { "bypass": true, "outputLevel": 0.4 } }
            ] },
            "micNodes": {}
        },
        "scenes": [
            { "guitarNodes": { "G1": {
                "ACD_TwinReverb": { "dspUnitParameters": { "bypass": false, "outputLevel": 0.9 } }
              } },
              "splitMix": { "balance": 0.1 } },
            { "guitarNodes": { "G1": {} } }
        ]
    })
}

// Base slot: the doc carries the WHOLE audioGraph (template + splitMix + base node params)
// so extract_active_graph reads the template and the un-overlaid base bypass.
#[test]
fn scene_docs_base_passes_template_through() {
    let (docs, restore) =
        scene_docs_from_saved(&saved_preset(), &[session::BASE_SCENE_SLOT]).unwrap();
    assert_eq!(restore, Some(2));
    let (slot, doc) = &docs[0];
    assert_eq!(*slot, session::BASE_SCENE_SLOT);
    let doc = doc.as_ref().unwrap();
    let ag = session::extract_active_graph(doc, None);
    assert_eq!(ag.template.as_deref(), Some("gtrSeries"));
    // Base scene = base node params, no overlay: amp is bypassed.
    assert_eq!(
        scenes::block_bypass_in_live_graph(doc, "G1", "ampA"),
        Some(true)
    );
}

// FS scene overlay flips the amp bypassed→active (visible via the production bypass reader).
#[test]
fn scene_docs_overlay_flips_bypass() {
    let (docs, _) = scene_docs_from_saved(&saved_preset(), &[0]).unwrap();
    let doc = docs[0].1.as_ref().unwrap();
    assert_eq!(
        scenes::block_bypass_in_live_graph(doc, "G1", "ampA"),
        Some(false),
        "scene 0 overlay activates the amp"
    );
}

// FS scene overlay's outputLevel is visible via the production extract_level_blocks.
#[test]
fn scene_docs_overlay_output_level_visible() {
    let (docs, _) = scene_docs_from_saved(&saved_preset(), &[0]).unwrap();
    let doc = docs[0].1.as_ref().unwrap();
    let ol = session::extract_level_blocks(doc)
        .into_iter()
        .find(|b| b.group_id == "G1" && b.node_id == "ampA" && b.parameter_id == "outputLevel")
        .map(|b| b.value);
    assert_eq!(ol, Some(0.9));
}

// splitMix overlay replaces the overlaid key, base keys survive (shallow merge).
#[test]
fn scene_docs_split_mix_overlay_merges() {
    let (docs, _) = scene_docs_from_saved(&saved_preset(), &[0]).unwrap();
    let doc = docs[0].1.as_ref().unwrap();
    let split = doc.pointer("/audioGraph/splitMix").unwrap();
    assert_eq!(split.get("balance").and_then(|v| v.as_f64()), Some(0.1)); // overlaid
    assert_eq!(split.get("level").and_then(|v| v.as_f64()), Some(0.8)); // base survives
}

// A param the overlay lacks falls through to the base node (scene 1 is empty).
#[test]
fn scene_docs_empty_overlay_keeps_base() {
    let (docs, _) = scene_docs_from_saved(&saved_preset(), &[1]).unwrap();
    let doc = docs[0].1.as_ref().unwrap();
    assert_eq!(
        scenes::block_bypass_in_live_graph(doc, "G1", "ampA"),
        Some(true),
        "empty overlay → base bypass"
    );
}

// A requested FS scene index absent from scenes[] → whole-fn None (fall back to live).
#[test]
fn scene_docs_missing_scene_index_is_none() {
    // Only scenes 0 and 1 exist; requesting scene 5 must bail.
    assert!(scene_docs_from_saved(&saved_preset(), &[5]).is_none());
}

// A truncated scene entry (a string where an object was expected) → whole-fn None.
#[test]
fn scene_docs_truncated_scene_entry_is_none() {
    let mut p = saved_preset();
    p["scenes"][0] = serde_json::json!("truncated");
    assert!(scene_docs_from_saved(&p, &[0]).is_none());
}

// audioGraph missing → whole-fn None.
#[test]
fn scene_docs_no_audiograph_is_none() {
    let p = serde_json::json!({ "lastLoadedScene": 0, "scenes": [] });
    assert!(scene_docs_from_saved(&p, &[session::BASE_SCENE_SLOT]).is_none());
}

// lastLoadedScene absent → restore scene is None (docs still build).
#[test]
fn scene_docs_restore_scene_none_when_absent() {
    let mut p = saved_preset();
    p.as_object_mut().unwrap().remove("lastLoadedScene");
    let (_, restore) = scene_docs_from_saved(&p, &[session::BASE_SCENE_SLOT]).unwrap();
    assert_eq!(restore, None);
}

// ── saved-structure fallback (the oversized-audioGraph class) ───────────────────────────
// A preset whose audioGraph overruns the device's lean-session field-3 push (~3.4 KB
// observed; the prepass sessions empirically get the lean cut)
// loses `audioGraph.template` in EVERY live scene doc, which used to hard-fail the whole
// preset ("some presets just never scene-level" — the Hiwatt user report). The
// field-8 saved JSON still carries the template (it sits at the end of audioGraph, well
// inside the ~17 KB field-8 partial), so build_scene_jobs accepts it as the routing
// STRUCTURE while knob values keep coming from the live docs.
#[test]
fn scene_jobs_saved_fallback_supplies_missing_template() {
    // Live doc: amp + knob survive the cut, template does not.
    let doc = serde_json::json!({
        "audioGraph": { "guitarNodes": { "G1": [
            { "nodeId": "ACD_HiwattDR103CanMod", "FenderId": "ACD_HiwattDR103CanMod",
              "dspUnitParameters": { "bypass": false, "outputLevel": 0.5 } }
        ] } }
    });
    // Field-8 saved JSON: complete graph including the template.
    let saved = serde_json::json!({
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
    let jobs = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))], -23.0, Some(&saved)).unwrap();
    assert_eq!(jobs.len(), 1);
    assert!(jobs[0].skip.is_none(), "skip reason: {:?}", jobs[0].skip);
    assert_eq!(jobs[0].knobs.len(), 1);
}

// A fallback that is ITSELF template-less (e.g. a truncated field-8 read) must not rescue
// classification — same honest hard error as before the fallback existed.
#[test]
fn scene_jobs_saved_fallback_without_template_still_errors() {
    let doc = serde_json::json!({
        "audioGraph": { "guitarNodes": { "G1": [
            { "nodeId": "ACD_HiwattDR103CanMod", "FenderId": "ACD_HiwattDR103CanMod",
              "dspUnitParameters": { "bypass": false, "outputLevel": 0.5 } }
        ] } }
    });
    let saved = serde_json::json!({
        "audioGraph": { "guitarNodes": { "G1": [] } }
    });
    let candidates = vec![LevelBlockArg {
        group_id: "G1".into(),
        node_id: "ACD_HiwattDR103CanMod".into(),
        parameter_id: "outputLevel".into(),
        value: 0.5,
    }];
    let err =
        build_scene_jobs(&[7], &candidates, &[(7, Some(doc))], -23.0, Some(&saved)).unwrap_err();
    assert!(err.contains("routing"), "got: {err}");
}

// ── raw scene-overlay presence (the SceneEdit-enable + bake gates) ──────────────────────
// `scene_overlay` answers from the RAW saved scene, never the merged graph: enabling Scene
// Edit on a node that already HAS an overlay reseeds (wipes) it, and omitting the enable on
// a node that has none leaks the write to base — so Present/Absent/Unknown must be exact.

#[test]
fn scene_overlay_present_carries_bypass() {
    let p = saved_preset();
    // Base node is "ampA"; the scene-0 overlay is keyed by its FenderId.
    let SceneOverlay::Present(params) = scene_overlay(&p, 0, "ampA") else {
        panic!("scene 0 overlays ampA");
    };
    assert_eq!(params.get("bypass").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(
        params.get("outputLevel").and_then(|v| v.as_f64()),
        Some(0.9)
    );
    assert!(params.contains_key("bypass"));
}

// The FenderId is accepted as the node key too (callers hold either id).
#[test]
fn scene_overlay_resolves_by_fender_id() {
    let p = saved_preset();
    assert!(matches!(
        scene_overlay(&p, 0, "ACD_TwinReverb"),
        SceneOverlay::Present(_)
    ));
}

// An overlay that does NOT carry bypass: Present, but the bake gate must not trip.
#[test]
fn scene_overlay_present_without_bypass() {
    let mut p = saved_preset();
    p["scenes"][1]["guitarNodes"]["G1"]["ACD_TwinReverb"] =
        serde_json::json!({ "dspUnitParameters": { "outputLevel": 0.7 } });
    let SceneOverlay::Present(params) = scene_overlay(&p, 1, "ampA") else {
        panic!("scene 1 overlays ampA");
    };
    assert!(!params.contains_key("bypass"));
    assert_eq!(
        params.get("outputLevel").and_then(|v| v.as_f64()),
        Some(0.7)
    );
}

// The scene exists and carries no entry for the node → Absent (the enable is REQUIRED).
#[test]
fn scene_overlay_absent_when_node_not_in_scene() {
    let p = saved_preset();
    assert!(matches!(scene_overlay(&p, 1, "ampA"), SceneOverlay::Absent));
}

// A node that isn't in the graph at all is still Absent for that scene (nothing to reseed).
#[test]
fn scene_overlay_absent_for_unknown_node() {
    let p = saved_preset();
    assert!(matches!(scene_overlay(&p, 0, "nope"), SceneOverlay::Absent));
}

// Scene index past `scenes[]` (the field-8 tail truncation class) → Unknown, NOT Absent.
#[test]
fn scene_overlay_unknown_when_scene_index_missing() {
    let p = saved_preset();
    assert!(matches!(
        scene_overlay(&p, 5, "ampA"),
        SceneOverlay::Unknown
    ));
}

// `scenes` cut off entirely (it sits at the document tail) → Unknown.
#[test]
fn scene_overlay_unknown_when_scenes_key_absent() {
    let mut p = saved_preset();
    p.as_object_mut().unwrap().remove("scenes");
    assert!(matches!(
        scene_overlay(&p, 0, "ampA"),
        SceneOverlay::Unknown
    ));
}

// A truncated scene entry (non-object) → Unknown.
#[test]
fn scene_overlay_unknown_when_scene_entry_truncated() {
    let mut p = saved_preset();
    p["scenes"][0] = serde_json::json!("truncated");
    assert!(matches!(
        scene_overlay(&p, 0, "ampA"),
        SceneOverlay::Unknown
    ));
}

// An overlay entry whose body isn't a param object is a truncated read, not "no overlay".
#[test]
fn scene_overlay_unknown_when_params_not_object() {
    let mut p = saved_preset();
    p["scenes"][0]["guitarNodes"]["G1"]["ACD_TwinReverb"] = serde_json::json!("cut");
    assert!(matches!(
        scene_overlay(&p, 0, "ampA"),
        SceneOverlay::Unknown
    ));
}

// Base is not an overlay context: Unknown, so no caller can derive an enable from it.
#[test]
fn scene_overlay_base_slot_is_unknown() {
    let p = saved_preset();
    assert!(matches!(
        scene_overlay(&p, session::BASE_SCENE_SLOT, "ampA"),
        SceneOverlay::Unknown
    ));
}

// ── per-node bake gate: does ANY scene overlay CHANGE this node's param ────────────────

// `saved_preset`'s `lastLoadedScene` (2) is out of range for its 2-entry `scenes[]`, which the
// bake gate correctly reads as a cut tail — so the gate's own fixture pins it in range.
fn bake_gate_preset() -> serde_json::Value {
    let mut p = saved_preset();
    p["lastLoadedScene"] = serde_json::json!(0);
    p
}

// Strip scene 0's `bypass` (keeping the overlay) so only a truncation signal can trip the gate.
fn without_scene0_bypass() -> serde_json::Value {
    let mut p = bake_gate_preset();
    p["scenes"][0]["guitarNodes"]["G1"]["ACD_TwinReverb"]["dspUnitParameters"] =
        serde_json::json!({ "outputLevel": 0.9 });
    p
}

// Scene 0 overlays `bypass: false` over a base `bypass: true` — a real FLIP.
#[test]
fn change_param_true_when_a_scene_flips_bypass() {
    assert!(scene_overlays_change_param(
        &bake_gate_preset(),
        "ampA",
        "bypass"
    ));
}

#[test]
fn change_param_false_when_no_scene_overlays_bypass() {
    assert!(!scene_overlays_change_param(
        &without_scene0_bypass(),
        "ampA",
        "bypass"
    ));
}

// The DEVICE-AUTHORED shape: the unit writes the FULL param set into every scene overlay, so
// the key is always present. Only a value that DIFFERS from base changes what the scene
// renders — an overlay restating base must read `false` for every param it carries, else the
// gate collapses to "this preset has scenes" on every preset the unit itself wrote.
#[test]
fn change_param_false_when_a_full_overlay_restates_the_base_values() {
    let mut p = bake_gate_preset();
    p["scenes"][0]["guitarNodes"]["G1"]["ACD_TwinReverb"]["dspUnitParameters"] =
        serde_json::json!({ "bypass": true, "outputLevel": 0.4 });
    assert!(!scene_overlays_change_param(&p, "ampA", "bypass"));
    assert!(!scene_overlays_change_param(&p, "ampA", "outputLevel"));
}

// Pin the `SCENE_PARAM_EPS` (1e-6) tolerance itself: the restating test above uses exact
// values, so it would stay green if the compare silently became `!=`. A float-noise delta
// (1e-7) must read unchanged; a real (if small) authored delta (1e-4) must read changed.
#[test]
fn change_param_tolerance_splits_float_noise_from_a_real_delta() {
    let mut p = bake_gate_preset();
    p["scenes"][0]["guitarNodes"]["G1"]["ACD_TwinReverb"]["dspUnitParameters"] =
        serde_json::json!({ "bypass": true, "outputLevel": 0.400_000_1 });
    assert!(
        !scene_overlays_change_param(&p, "ampA", "outputLevel"),
        "1e-7 off base is float noise, not an authored change"
    );
    p["scenes"][0]["guitarNodes"]["G1"]["ACD_TwinReverb"]["dspUnitParameters"] =
        serde_json::json!({ "bypass": true, "outputLevel": 0.4001 });
    assert!(
        scene_overlays_change_param(&p, "ampA", "outputLevel"),
        "1e-4 off base is a real authored delta"
    );
}

// The leveled-param check the footswitch gate's second call makes: base 0.4 → scene 0.9.
#[test]
fn change_param_true_when_a_scene_changes_a_non_bypass_param() {
    assert!(scene_overlays_change_param(
        &bake_gate_preset(),
        "ampA",
        "outputLevel"
    ));
}

// Two graph nodes answering to the same id: `scene_overlay` resolves the overlay off the
// roster's FIRST match, so a value comparison could pit one instance's base against the
// other's overlay. Ambiguous identity is unreadable state → conservative `true`.
#[test]
fn change_param_true_when_the_base_node_is_ambiguous() {
    let mut p = bake_gate_preset();
    p["scenes"][0]["guitarNodes"]["G1"]["ACD_TwinReverb"]["dspUnitParameters"] =
        serde_json::json!({ "bypass": true, "outputLevel": 0.4 });
    let dup = p["audioGraph"]["guitarNodes"]["G1"][0].clone();
    p["audioGraph"]["guitarNodes"]["G1"] = serde_json::json!([dup.clone(), dup]);
    assert!(scene_overlays_change_param(&p, "ampA", "outputLevel"));
}

// The ambiguity case a per-overlay guard CANNOT catch: the duplicate lives in another group
// (the same block in both parallel lanes) and the scene overlays only THAT copy. `scene_overlay`
// looks under the first instance's group, finds nothing and reports `Absent` — so without the
// up-front refusal this bakes off a scene state never read.
#[test]
fn change_param_true_when_a_duplicate_instance_is_the_one_the_scene_overlays() {
    let mut p = bake_gate_preset();
    let dup = p["audioGraph"]["guitarNodes"]["G1"][0].clone();
    p["audioGraph"]["guitarNodes"]["G2"] = serde_json::json!([dup]);
    // Only G2's copy is overlaid; G1's (the roster's first match) is not mentioned at all.
    p["scenes"][0] = serde_json::json!({ "guitarNodes": { "G2": {
        "ACD_TwinReverb": { "dspUnitParameters": { "outputLevel": 0.9 } }
    } } });
    assert!(scene_overlays_change_param(&p, "ampA", "outputLevel"));
}

#[test]
fn change_param_false_for_a_sceneless_preset() {
    let mut p = bake_gate_preset();
    p["scenes"] = serde_json::json!([]);
    // A preset with no scenes sits on base, which is not a `scenes[]` index.
    p["lastLoadedScene"] = serde_json::json!(session::BASE_SCENE_SLOT);
    assert!(!scene_overlays_change_param(&p, "ampA", "bypass"));
}

// The truncation case the bake gate exists for: `scenes` absent → unknown → never bake.
#[test]
fn change_param_true_when_scenes_key_absent() {
    let mut p = bake_gate_preset();
    p.as_object_mut().unwrap().remove("scenes");
    assert!(scene_overlays_change_param(&p, "ampA", "bypass"));
}

#[test]
fn change_param_true_when_a_scene_entry_is_truncated() {
    let mut p = without_scene0_bypass();
    p["scenes"][1] = serde_json::json!("truncated");
    assert!(scene_overlays_change_param(&p, "ampA", "bypass"));
}

// The tail cut that SHORTENS `scenes[]`: the dropped scenes are invisible to a plain
// `0..len` walk, so the gate must catch them via the indices the document still references
// (`lastLoadedScene` here — scene 1 with only scene 0 left in the array).
#[test]
fn change_param_true_when_scenes_array_is_cut_short() {
    let mut p = without_scene0_bypass();
    let scene0 = p["scenes"][0].clone();
    p["scenes"] = serde_json::json!([scene0]);
    p["lastLoadedScene"] = serde_json::json!(1);
    assert!(scene_overlays_change_param(&p, "ampA", "bypass"));
}

// Same cut, evidenced by a FOOTSWITCH scene assignment instead of `lastLoadedScene`.
#[test]
fn change_param_true_when_a_footswitch_references_a_dropped_scene() {
    let mut p = without_scene0_bypass();
    let scene0 = p["scenes"][0].clone();
    p["scenes"] = serde_json::json!([scene0]);
    p["ftsw"] = serde_json::json!([[{ "func": "scene", "sceneSlot": 3, "isActive": true }]]);
    assert!(scene_overlays_change_param(&p, "ampA", "bypass"));
}

// A node no scene mentions is bakeable (a plain preset whose scenes touch other blocks).
#[test]
fn change_param_false_for_a_node_no_scene_mentions() {
    assert!(!scene_overlays_change_param(
        &bake_gate_preset(),
        "otherNode",
        "bypass"
    ));
}
