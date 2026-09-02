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
fn scene_overlay_full_carries_bypass() {
    let p = saved_preset();
    // Base node is "ampA"; the scene-0 overlay is keyed by its FenderId. It carries a KNOB
    // (`outputLevel`) alongside `bypass`, so it is a FULL overlay — the node's Scene Edit
    // flag is on and the enable-dropped write lands on the overlay.
    let SceneOverlay::Full(params) = scene_overlay(&p, 0, "ampA") else {
        panic!("scene 0 fully overlays ampA");
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
        SceneOverlay::Full(_)
    ));
}

// An overlay that does NOT carry bypass: still FULL (it carries a knob), but the bake gate
// must not trip.
#[test]
fn scene_overlay_full_without_bypass() {
    let mut p = saved_preset();
    p["scenes"][1]["guitarNodes"]["G1"]["ACD_TwinReverb"] =
        serde_json::json!({ "dspUnitParameters": { "outputLevel": 0.7 } });
    let SceneOverlay::Full(params) = scene_overlay(&p, 1, "ampA") else {
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

// ── per-node bake gate (`scene_overlays_change_param`) — REMOVED 2026-08-19 ────────────
//
// The whole "does any scene overlay CHANGE this node's param" gate this section used to pin
// is gone: `plan_footswitch_jobs`'s assign gate no longer reads scene data at all — it decides
// bake-vs-assign purely off whether the switch already carries a `param` fn for the selected
// control. The dozen-plus VALUE/truncation/ambiguity cases that lived here (plus the
// truncation-only `without_scene0_bypass` fixture) existed solely to pin that removed
// function; they went with it. `bake_gate_preset` survives BELOW — `restating_base_skips_a_
// bypass_only_scene_which_inherits_the_bake` still needs its in-range `lastLoadedScene` — and
// `scene_overlay_shape_classifies_bypass_only_vs_full` further down still stands too; both pin
// `scene_overlay`/`scenes_restating_base`, neither of which this rule change touched.

// `saved_preset`'s `lastLoadedScene` (2) is out of range for its 2-entry `scenes[]`; the
// mirror-target fixture below needs it in range, so it pins that itself.
fn bake_gate_preset() -> serde_json::Value {
    let mut p = saved_preset();
    p["lastLoadedScene"] = serde_json::json!(0);
    p
}

// ── three-state overlay split: Full vs BypassOnly (HW-verified fw 1.8.45) ───────────────
//
// A scene's per-node overlay can be BYPASS-ONLY — the block's Scene Edit flag is DISABLED,
// so the scene carries only the bypass family and SHARES the node's knobs with base. A
// scene-context param write with the enable dropped against such a node lands on BASE
// (measured: base gain 2.5 → 7.0, the bypass-only overlay unchanged, other scenes' full
// overlays untouched). Classifying it as a knob overlay is the bug this split fixes, so
// each shape below is pinned exactly.

/// `saved_preset()` with scene 0's `ampA` overlay replaced by `params`. `pub(crate)`
/// so `commands::doctor_tests`' `bypass_only_conflict` tests share this fixture
/// shape instead of re-typing it (both scan `scene_overlay` against `ampA`/`G1`/
/// `ACD_TwinReverb`).
pub(crate) fn with_scene0_overlay(params: serde_json::Value) -> serde_json::Value {
    let mut p = saved_preset();
    p["scenes"][0]["guitarNodes"]["G1"]["ACD_TwinReverb"] =
        serde_json::json!({ "dspUnitParameters": params });
    p
}

// Overlay-shape table: which param maps classify BypassOnly (device Scene Edit flag
// disabled, knobs shared with base) vs Full (a genuine knob overlay). `bypassType` and
// `clipState` are per-block STATE keys widened into BYPASS_ONLY_KEYS (m4), not knobs — an
// overlay of only those plus `bypass` must stay BypassOnly. An EMPTY param map overlays no
// knob at all so it shares base exactly like the flag-disabled case (pinned so nobody "fixes"
// it to Full — that would authorise the leak-to-base write). ONE real knob (`outputLevel`)
// alongside bypass is enough to flip the whole overlay to Full.
#[test]
fn scene_overlay_shape_classifies_bypass_only_vs_full() {
    let cases: [(&str, serde_json::Value, bool); 5] = [
        ("bypass alone", serde_json::json!({ "bypass": false }), true),
        (
            "bypass + bypassType (firmware companion enum)",
            serde_json::json!({ "bypass": true, "bypassType": "Post" }),
            true,
        ),
        (
            "bypass + clipState (per-block state, not a knob)",
            serde_json::json!({ "bypass": true, "clipState": "off" }),
            true,
        ),
        (
            "bypass + outputLevel (one real knob rides along)",
            serde_json::json!({ "bypass": false, "outputLevel": 0.4 }),
            false,
        ),
        (
            "empty param map (no knob overlaid at all)",
            serde_json::json!({}),
            true,
        ),
    ];
    for (label, params, expect_bypass_only) in cases {
        let p = with_scene0_overlay(params);
        match scene_overlay(&p, 0, "ampA") {
            SceneOverlay::BypassOnly(_) => assert!(
                expect_bypass_only,
                "{label}: classified BypassOnly, expected Full"
            ),
            SceneOverlay::Full(_) => assert!(
                !expect_bypass_only,
                "{label}: classified Full, expected BypassOnly"
            ),
            _ => panic!("{label}: expected Full or BypassOnly"),
        }
    }
}

// Mirror rule: a BypassOnly scene is NOT a mirror target. Its knobs are shared with base, so
// it already follows base for the leveled param and inherits the bake automatically —
// writing a mirror there would be the leak-to-base write the split exists to prevent.
#[test]
fn restating_base_skips_a_bypass_only_scene_which_inherits_the_bake() {
    let mut p = bake_gate_preset();
    // Scene 0: a FULL overlay restating base's outputLevel (0.4) → a genuine mirror target.
    p["scenes"][0]["guitarNodes"]["G1"]["ACD_TwinReverb"] =
        serde_json::json!({ "dspUnitParameters": { "bypass": true, "outputLevel": 0.4 } });
    // Scene 1: bypass-only → shares base's knobs → nothing to mirror.
    p["scenes"][1]["guitarNodes"]["G1"]["ACD_TwinReverb"] =
        serde_json::json!({ "dspUnitParameters": { "bypass": false } });
    assert!(matches!(
        scene_overlay(&p, 1, "ampA"),
        SceneOverlay::BypassOnly(_)
    ));
    assert_eq!(
        scenes_restating_base(&p, "ampA", "outputLevel"),
        vec![0],
        "only the FULL restating overlay is mirrored; the sharing scene inherits the bake"
    );
}

// ── Part B: audibility-guarded BypassOnly shared write ──────────────────────────────────
//
// BUG (preset 28 "Friedman HBE", `ACD_Boost`/`gain`): base bypassed, `gain` 2.5. Dirt (scene
// 0) and Crunch (scene 3) carry FULL overlays (their own bypass + gain). Clean (scene 1) is
// bypass-only and stays bypassed. Solo (scene 2) is bypass-only and UN-bypassed — it is the
// ONLY scene that can hear a plain leak-to-base write, because every OTHER scene either stays
// bypassed or pins `gain` with its own overlay. `scene_write_verdict` used to refuse this
// outright ("shares knobs with base"); `shared_write_is_scene_local` clears it.

/// `hbe_boost_preset()`'s node id/param, named once so the matrix tests below don't repeat
/// the literals. `pub(crate)` alongside the preset builder — `commands::doctor_tests`'
/// `bypass_only_conflict` matrix reuses both rather than re-typing the anatomy.
pub(crate) const HBE_NODE: &str = "boost";
pub(crate) const HBE_PARAM: &str = "gain";

/// The bug's exact shape: one node (`boost`/`ACD_Boost`, group G1) bypassed in base with
/// `gain` 2.5, and 4 scenes — Dirt/Crunch (Full, own gain), Clean (bypass-only, stays
/// bypassed), Solo (bypass-only, un-bypassed — the sole audible scene for a shared write).
/// `pub(crate)` so `commands::doctor_tests` can drive `bypass_only_conflict` (the OTHER
/// consumer of `scene_write_verdict_for_param`) against the exact same anatomy instead of
/// re-typing it.
pub(crate) fn hbe_boost_preset() -> serde_json::Value {
    serde_json::json!({
        "lastLoadedScene": 2,
        "audioGraph": { "guitarNodes": { "G1": [
            { "nodeId": "boost", "FenderId": "ACD_Boost",
              "dspUnitParameters": { "bypass": true, "gain": 2.5 } }
        ] } },
        "scenes": [
            // Dirt: Full overlay, own gain — pins `gain` against base.
            { "guitarNodes": { "G1": {
                "ACD_Boost": { "dspUnitParameters": { "bypass": true, "gain": 5.0 } } } } },
            // Clean: bypass-only, stays bypassed — the leak is silent here.
            { "guitarNodes": { "G1": {
                "ACD_Boost": { "dspUnitParameters": { "bypass": true } } } } },
            // Solo: bypass-only, un-bypassed — the ONLY scene that can hear the leak.
            { "guitarNodes": { "G1": {
                "ACD_Boost": { "dspUnitParameters": { "bypass": false } } } } },
            // Crunch: Full overlay, own gain — pins `gain` against base.
            { "guitarNodes": { "G1": {
                "ACD_Boost": { "dspUnitParameters": { "bypass": true, "gain": 6.0 } } } } }
        ]
    })
}

#[test]
fn shared_write_is_scene_local_true_only_for_the_solo_scene() {
    assert!(matches!(
        scene_overlay(&hbe_boost_preset(), 2, HBE_NODE),
        SceneOverlay::BypassOnly(_)
    ));
    assert!(
        shared_write_is_scene_local(&hbe_boost_preset(), 2, HBE_NODE, HBE_PARAM),
        "Solo is the only scene the shared write is audible in"
    );
    // Clean (scene 1) is ALSO bypass-only, but stays bypassed — the predicate is asked
    // about scene 1 itself now, and it must refuse (the leak is silent there, so writing it
    // there would be pointless AND it is not the scene answering "audible only here").
    assert!(!shared_write_is_scene_local(
        &hbe_boost_preset(),
        1,
        HBE_NODE,
        HBE_PARAM
    ));
}

#[test]
fn shared_write_is_scene_local_false_when_a_second_scene_is_audible_without_pinning() {
    let mut p = hbe_boost_preset();
    // Clean (scene 1) un-bypassed too, with NO Full overlay pinning `gain` there — now BOTH
    // scene 1 and Solo would hear the shared write, so neither is scene-LOCAL to it.
    p["scenes"][1]["guitarNodes"]["G1"]["ACD_Boost"] =
        serde_json::json!({ "dspUnitParameters": { "bypass": false } });
    assert!(!shared_write_is_scene_local(&p, 2, HBE_NODE, HBE_PARAM));
}

#[test]
fn shared_write_is_scene_local_false_when_base_carries_no_bypass_key() {
    let mut p = hbe_boost_preset();
    p["audioGraph"]["guitarNodes"]["G1"][0]["dspUnitParameters"] =
        serde_json::json!({ "gain": 2.5 });
    assert!(
        !shared_write_is_scene_local(&p, 2, HBE_NODE, HBE_PARAM),
        "a missing base bypass key can't confirm the shared value is silent in base"
    );
}

#[test]
fn shared_write_is_scene_local_false_when_audible_in_base() {
    let mut p = hbe_boost_preset();
    p["audioGraph"]["guitarNodes"]["G1"][0]["dspUnitParameters"]["bypass"] =
        serde_json::json!(false);
    assert!(!shared_write_is_scene_local(&p, 2, HBE_NODE, HBE_PARAM));
}

#[test]
fn shared_write_is_scene_local_false_on_truncated_scenes() {
    let mut p = hbe_boost_preset();
    // A scene footswitch assignment naming an index the 4-entry `scenes[]` doesn't reach —
    // the same truncation signature `max_referenced_scene` exists to catch.
    p["ftsw"] = serde_json::json!([[
        { "func": "scene", "sceneSlot": 5, "isActive": true }
    ]]);
    assert!(!shared_write_is_scene_local(&p, 2, HBE_NODE, HBE_PARAM));
}

#[test]
fn shared_write_is_scene_local_false_when_a_footswitch_on_off_row_targets_the_node() {
    let mut p = hbe_boost_preset();
    p["ftsw"] = serde_json::json!([[
        { "func": "on-off", "isActive": true,
          "nodes": [{ "groupId": "G1", "nodeId": "boost" }] }
    ]]);
    assert!(!shared_write_is_scene_local(&p, 2, HBE_NODE, HBE_PARAM));
}

#[test]
fn shared_write_is_scene_local_false_when_a_footswitch_param_function_targets_the_node() {
    let mut p = hbe_boost_preset();
    p["ftsw"] = serde_json::json!([[
        { "func": "param", "groupId": "G1", "nodeId": "boost", "parameterId": "gain",
          "valueA": 0.0, "valueB": 1.0 }
    ]]);
    assert!(!shared_write_is_scene_local(&p, 2, HBE_NODE, HBE_PARAM));
}

#[test]
fn shared_write_is_scene_local_false_when_an_exp_binding_targets_the_node() {
    let mut p = hbe_boost_preset();
    p["exp"] = serde_json::json!({
        "exp1": [
            { "func": "param", "groupId": "G1", "nodeId": "boost", "paramId": "gain",
              "heel": 0.0, "toe": 1.0 }
        ]
    });
    assert!(!shared_write_is_scene_local(&p, 2, HBE_NODE, HBE_PARAM));
}

// The three escapes `footswitch::node_targeted_by_assign` catches that the old, local
// `scene_jobs::node_targeted_by_ftsw_or_exp` copy it replaced did NOT: a `toe` jack (the old
// scan only checked `exp1`/`exp2`), an object-shaped jack body (the old scan assumed an array),
// and a non-`"param"` func naming the node (the old scan matched `func:"param"` only). Each
// must still make `shared_write_is_scene_local` refuse, exactly like the exp1/param case above.

#[test]
fn shared_write_is_scene_local_false_when_a_toe_jack_assign_targets_the_node() {
    let mut p = hbe_boost_preset();
    p["exp"] = serde_json::json!({
        "toe": [
            { "func": "param", "groupId": "G1", "nodeId": "boost", "paramId": "gain",
              "heel": 0.0, "toe": 1.0 }
        ]
    });
    assert!(!shared_write_is_scene_local(&p, 2, HBE_NODE, HBE_PARAM));
}

#[test]
fn shared_write_is_scene_local_false_when_an_object_shaped_exp1_targets_the_node() {
    let mut p = hbe_boost_preset();
    // No array wrapper — a single assignment object directly under the jack key.
    p["exp"] = serde_json::json!({ "exp1": { "func": "volume", "nodeId": "boost" } });
    assert!(!shared_write_is_scene_local(&p, 2, HBE_NODE, HBE_PARAM));
}

#[test]
fn shared_write_is_scene_local_false_when_a_wah_func_exp_binding_targets_the_node() {
    let mut p = hbe_boost_preset();
    p["exp"] = serde_json::json!({
        "exp2": [
            { "func": "wah", "groupId": "G1", "nodeId": "boost", "heel": 0.0, "toe": 1.0 }
        ]
    });
    assert!(!shared_write_is_scene_local(&p, 2, HBE_NODE, HBE_PARAM));
}

#[test]
fn shared_write_is_scene_local_false_on_an_ambiguous_base_node_match() {
    let mut p = hbe_boost_preset();
    // A second base node answering to the SAME nodeId, in the mic graph — `base_node_matches`
    // now reads two hits for "boost", which the predicate refuses outright.
    p["audioGraph"]["micNodes"] = serde_json::json!({ "M1": [
        { "nodeId": "boost", "FenderId": "ACD_Boost", "dspUnitParameters": { "bypass": true } }
    ] });
    assert!(!shared_write_is_scene_local(&p, 2, HBE_NODE, HBE_PARAM));
}

// ── verdict matrix: `scene_write_verdict_for_param` on the same doc ─────────────────────

#[test]
fn scene_write_verdict_for_param_allows_the_solo_write_direct_with_no_enable() {
    let verdict = scene_write_verdict_for_param(&hbe_boost_preset(), 2, HBE_NODE, HBE_PARAM);
    assert!(
        matches!(verdict, SceneWriteVerdict::WriteDirect),
        "the audibility-guarded shared write must be allowed through as WriteDirect"
    );
    assert!(
        !matches!(verdict, SceneWriteVerdict::NeedsEnable),
        "must NEVER enable Scene Edit here — that would reseed the overlay and wipe the \
         scene's bypass flip"
    );
}

#[test]
fn scene_write_verdict_for_param_refuses_when_the_leak_is_not_scene_local() {
    // Clean (scene 1) stays bypassed — the leak is silent there, so the predicate refuses
    // (nothing gained by writing it), and the verdict keeps today's Refuse wording.
    let verdict = scene_write_verdict_for_param(&hbe_boost_preset(), 1, HBE_NODE, HBE_PARAM);
    match verdict {
        SceneWriteVerdict::Refuse { scope, reason } => {
            assert_eq!(scope, RefusedScope::SharedWithBase);
            assert!(reason.contains("shares") && reason.contains(HBE_NODE));
        }
        _ => panic!("expected Refuse"),
    }
}

#[test]
fn scene_write_verdict_for_param_full_overlay_arm_unchanged() {
    // Dirt (scene 0): a genuine Full overlay — WriteDirect regardless of the audibility
    // guard, exactly like the paramless rule, and lands on the overlay (not base).
    assert!(matches!(
        scene_write_verdict_for_param(&hbe_boost_preset(), 0, HBE_NODE, HBE_PARAM),
        SceneWriteVerdict::WriteDirect
    ));
}

#[test]
fn scene_write_verdict_for_param_absent_overlay_arm_unchanged() {
    // A second node no scene ever mentions — Absent in every scene, unaffected by the
    // BypassOnly-only audibility guard.
    let mut p = hbe_boost_preset();
    p["audioGraph"]["guitarNodes"]["G1"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "nodeId": "delay", "FenderId": "ACD_TapeDelay",
            "dspUnitParameters": { "bypass": true, "level": 0.5 }
        }));
    assert!(matches!(
        scene_write_verdict_for_param(&p, 2, "delay", "level"),
        SceneWriteVerdict::NeedsEnable
    ));
}

#[test]
fn scene_write_verdict_for_param_unknown_overlay_arm_unchanged() {
    // No `scenes` array at all (mirrors a truncated field-8 read) — presence unanswerable.
    let p = serde_json::json!({});
    match scene_write_verdict_for_param(&p, 0, HBE_NODE, HBE_PARAM) {
        SceneWriteVerdict::Refuse { scope, reason } => {
            assert_eq!(scope, RefusedScope::Unknown);
            assert!(reason.contains("truncated"));
        }
        SceneWriteVerdict::WriteDirect => {
            panic!("expected Refuse(Unknown), got WriteDirect")
        }
        SceneWriteVerdict::NeedsEnable => panic!("expected Refuse(Unknown), got NeedsEnable"),
    }
}

// ─────────────── G-D1a: an unusable scene doc must REFUSE, never fall back to base ───────────
// HW motivation (Friedman HBE, device slot 28): the preset carries two guitar amps — `ACD_BE100`
// active in base and `ACD_TwinReverb65NoFx` bypassed there. When a scene's doc does not arrive,
// `classify_scene_knobs`' `None => !nd.bypassed` arm resolves bypass from the BASE graph, so the
// scene is leveled on whichever amp base leaves on. On a scene that swaps amps that knob is not in
// the scene's signal path at all: the solve sweeps it, nothing moves, and the row reports a
// `no_authority` clamp that reads exactly like a genuine ceiling.
//
// A missing amp knob is NOT a levelable outcome, so the honest answer is this row's own `skip`
// (the lane's per-scene-skip rule — never a batch abort). Deciding from base is the bug.
fn two_amp_base_saved() -> serde_json::Value {
    serde_json::json!({
        "audioGraph": { "template": "gtrSeries", "guitarNodes": { "G1": [
            { "nodeId": "ACD_BE100", "FenderId": "ACD_BE100",
              "dspUnitParameters": { "bypass": false, "outputLevel": 1.0 } },
            { "nodeId": "ACD_TwinReverb65NoFx", "FenderId": "ACD_TwinReverb65NoFx",
              "dspUnitParameters": { "bypass": true, "outputLevel": 0.28 } }
        ] } }
    })
}

fn two_amp_candidates() -> Vec<LevelBlockArg> {
    vec![
        LevelBlockArg {
            group_id: "G1".to_string(),
            node_id: "ACD_BE100".to_string(),
            parameter_id: "outputLevel".to_string(),
            value: 1.0,
        },
        LevelBlockArg {
            group_id: "G1".to_string(),
            node_id: "ACD_TwinReverb65NoFx".to_string(),
            parameter_id: "outputLevel".to_string(),
            value: 0.28,
        },
    ]
}

#[test]
fn a_scene_whose_doc_never_arrived_skips_instead_of_classifying_against_base() {
    let saved = two_amp_base_saved();
    // The live prepass pushes `(scene, None)` when no field-3 doc materialises for the recall.
    let jobs = build_scene_jobs(
        &[3],
        &two_amp_candidates(),
        &[(3, None)],
        -23.0,
        Some(&saved),
    )
    .unwrap();

    assert!(
        jobs[0].skip.is_some(),
        "a scene with no doc must skip; instead it was classified with knobs {:?}",
        jobs[0].knobs
    );
}

#[test]
fn a_scene_whose_doc_is_partial_skips_instead_of_classifying_against_base() {
    // The REALISTIC shape: the doc EXISTS but was cut before the amp nodes, so every
    // `block_bypass_in_live_graph` lookup past the cut returns `None` — the identical fallback,
    // with no `Value::Null` anywhere. A gate keyed only on the null case would miss this.
    let saved = two_amp_base_saved();
    let partial = serde_json::json!({ "audioGraph": { "guitarNodes": { "G1": [] } } });
    let jobs = build_scene_jobs(
        &[3],
        &two_amp_candidates(),
        &[(3, Some(partial))],
        -23.0,
        Some(&saved),
    )
    .unwrap();

    assert!(
        jobs[0].skip.is_some(),
        "a scene whose doc is missing its amp nodes must skip; instead it was classified with \
         knobs {:?}",
        jobs[0].knobs
    );
}

#[test]
fn a_scene_whose_saved_overlay_is_unclassifiable_skips_instead_of_losing_its_headroom_silently() {
    // The live doc classifies CLEANLY — one un-bypassed amp, so knobs build and the row looks
    // perfectly levelable. The defect is on the OTHER input: the SAVED doc cannot answer this
    // scene's overlay (`scene_body` → `None` ⇒ `SceneOverlay::Unknown`), so the headroom trade
    // scores the row `benefits: false` via `benefits_from_base_raise` and buys it no headroom —
    // and it clamps with nothing red. The write path already refuses `Unknown`
    // (`scene_write_verdict`), so this row was never going to be written: refusing it HERE, at
    // build time, is what makes the loss visible instead of silent (and saves it an engage).
    // The saved doc CLAIMS overlay authority — it carries a `scenes` array — but that array
    // stops at index 1 while the batch levels scene 3, so `scene_body` can't answer and the
    // overlay reads `Unknown`. On the batched command path the doc is complete-or-fail, so this
    // is genuine malformation rather than a truncated tail, and it must not pass as levelable.
    let mut saved = two_amp_base_saved();
    saved["scenes"] = serde_json::json!([
        { "guitarNodes": { "G1": {} } },
        { "guitarNodes": { "G1": {} } }
    ]);
    let live = serde_json::json!({
        "audioGraph": { "template": "gtrSeries", "guitarNodes": { "G1": [
            { "nodeId": "ACD_BE100", "FenderId": "ACD_BE100",
              "dspUnitParameters": { "bypass": false, "outputLevel": 1.0 } },
            { "nodeId": "ACD_TwinReverb65NoFx", "FenderId": "ACD_TwinReverb65NoFx",
              "dspUnitParameters": { "bypass": true, "outputLevel": 0.28 } }
        ] } }
    });
    let jobs = build_scene_jobs(
        &[3],
        &two_amp_candidates(),
        &[(3, Some(live))],
        -23.0,
        Some(&saved),
    )
    .unwrap();

    assert!(
        jobs[0].skip.is_some(),
        "a scene whose saved overlay cannot be classified must skip and say so; instead it was \
         built as a levelable row with knobs {:?}, which the trade then silently scores as a \
         non-beneficiary",
        jobs[0].knobs
    );
}

// --- Live-prepass misses are repaired from the saved preset, never from base ---

// G-D1a's two-amp base, plus the SCENES the real "Friedman HBE" carries. Scene 1 ("Clean") is
// the swap scene: its overlay inverts the base pair — `ACD_BE100` off, `ACD_TwinReverb65NoFx`
// on at its own outputLevel of 0.45. That is the scene base fallback gets wrong, and the one
// the live prepass skipped on HW (2026-09-01) by pushing no usable doc for it.
fn two_amp_swap_preset() -> serde_json::Value {
    let mut p = two_amp_base_saved();
    p["lastLoadedScene"] = serde_json::json!(8);
    p["scenes"] = serde_json::json!([
        { "guitarNodes": { "G1": {
            "ACD_BE100": { "dspUnitParameters": { "bypass": false, "outputLevel": 0.97 } },
            "ACD_TwinReverb65NoFx": { "dspUnitParameters": { "bypass": true } }
          } } },
        { "guitarNodes": { "G1": {
            "ACD_BE100": { "dspUnitParameters": { "bypass": true } },
            "ACD_TwinReverb65NoFx": { "dspUnitParameters": { "bypass": false, "outputLevel": 0.45 } }
          } } }
    ]);
    p
}

// The repair predicate keys on the AMP question, so it catches BOTH live-prepass failure
// shapes — the scene that pushed nothing at all, and the scene whose doc arrived cut before
// the amp nodes. The partial is the one a `Null` check would miss while it produces the
// identical wrong-amp fallback.
#[test]
fn scenes_missing_amp_bypass_flags_absent_and_partial_docs() {
    let (docs, _) = scene_docs_from_saved(&two_amp_swap_preset(), &[0]).unwrap();
    let answerable = docs[0].1.clone().unwrap();
    let partial = serde_json::json!({
        "audioGraph": { "template": "gtrSeries", "guitarNodes": { "G1": [
            { "nodeId": "ACD_TubeScreamer", "dspUnitParameters": { "bypass": false } }
        ] } }
    });
    let docs = vec![
        (0u32, Some(answerable)),
        (1u32, None),
        (2u32, Some(partial)),
    ];
    assert_eq!(
        scenes_missing_amp_bypass(&saved_structure(), &docs),
        vec![1, 2],
        "the absent doc AND the amp-less partial both need repair; the answerable one does not"
    );
}

// A doc cut BETWEEN the two amps answers the first (BE100) and not the second (the Twin the
// swap scene needs): it must be flagged for repair and refused by the classifier, never
// classified with the Twin's bypass resolved from base.
#[test]
fn a_doc_cut_between_two_amps_is_flagged_and_refused() {
    let cut = serde_json::json!({
        "audioGraph": { "template": "gtrSeries", "guitarNodes": { "G1": [
            { "nodeId": "ACD_BE100", "dspUnitParameters": { "bypass": true } }
        ] } }
    });
    let (answerable, _) = scene_docs_from_saved(&two_amp_swap_preset(), &[0]).unwrap();
    let docs = vec![(0u32, answerable[0].1.clone()), (1u32, Some(cut.clone()))];
    assert_eq!(
        scenes_missing_amp_bypass(&saved_structure(), &docs),
        vec![1]
    );
    let structure = session::extract_active_graph(&two_amp_base_saved(), None);
    let err = classify_scene_knobs(&structure, &cut, &two_amp_candidates()).unwrap_err();
    assert!(err.contains("every amp"), "{err}");
}

/// The complete saved graph every repair scan takes its amp roster from.
fn saved_structure() -> session::ActiveGraph {
    session::extract_active_graph(&two_amp_base_saved(), None)
}

// The roster must come from the COMPLETE saved graph, never from the live docs: when the only
// doc is the one cut between the two amps, a roster read off that doc lists one amp and the
// doc answers for itself (red under the docs-derived roster).
#[test]
fn a_cut_only_doc_is_flagged_off_the_saved_roster() {
    let cut = serde_json::json!({
        "audioGraph": { "template": "gtrSeries", "guitarNodes": { "G1": [
            { "nodeId": "ACD_BE100", "dspUnitParameters": { "bypass": true } }
        ] } }
    });
    let docs = vec![(1u32, Some(cut))];
    assert_eq!(
        scenes_missing_amp_bypass(&saved_structure(), &docs),
        vec![1]
    );
}

// The repair itself, end to end on the HW shape: two scenes the live prepass could not answer
// (one pushed nothing, one arrived cut before the amps) are filled from the saved preset, the
// scene that DID answer is left alone, and the swap scene ends up on its own amp.
#[test]
fn repair_scene_docs_fills_only_the_unanswerable_scenes() {
    let saved = two_amp_swap_preset();
    let (answerable, _) = scene_docs_from_saved(&saved, &[0]).unwrap();
    let mut docs = vec![
        (0u32, answerable[0].1.clone()),
        // Scene 1 pushed nothing at all.
        (1u32, None),
    ];
    // Mark scene 0's doc so we can prove the repair did not touch it.
    docs[0].1.as_mut().unwrap()["__probe"] = serde_json::json!("untouched");

    let needy = scenes_missing_amp_bypass(&saved_structure(), &docs);
    assert_eq!(
        needy,
        vec![1],
        "only the scene that pushed nothing needs repair"
    );
    assert!(repair_scene_docs_from(&mut docs, &saved, &needy));
    assert_eq!(docs[0].1.as_ref().unwrap()["__probe"], "untouched");

    let structure = session::extract_active_graph(&two_amp_base_saved(), None);
    let (knobs, _) = classify_scene_knobs(
        &structure,
        docs[1].1.as_ref().unwrap(),
        &two_amp_candidates(),
    )
    .expect("the repaired scene must classify");
    assert_eq!(knobs[0].1, "ACD_TwinReverb65NoFx");
}

// A saved preset that cannot answer either (truncated `scenes`) repairs NOTHING and leaves the
// docs as they were — the classifier's refusal is still what the row reports.
#[test]
fn repair_scene_docs_leaves_docs_untouched_when_the_saved_preset_cannot_answer() {
    let mut truncated = two_amp_swap_preset();
    truncated["scenes"] = serde_json::json!([]);
    let mut docs = vec![(1u32, None)];
    let needy = scenes_missing_amp_bypass(&saved_structure(), &docs);
    assert!(!repair_scene_docs_from(&mut docs, &truncated, &needy));
    assert!(docs[0].1.is_none(), "no doc was invented");
}
