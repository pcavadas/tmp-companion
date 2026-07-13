//! Pure-derivation unit tests for the overlay A/B arm (sibling of `overlay_ab.rs`).
//! These cover `overlay_effective_amp` (base + sparse scene overlay → effective
//! `{bypass, outputLevel}`) and the base-scene + amp-enumeration helpers — no device.

use super::*;

fn amp_node(fid: &str, bypass: bool, ol: f64) -> Value {
    serde_json::json!({
        "nodeId": fid, "FenderId": fid,
        "dspUnitParameters": { "bypass": bypass, "outputLevel": ol }
    })
}

// Overlay flips bypass: base amp is ON (bypass=false); the scene overlay sets bypass=true.
#[test]
fn overlay_flips_bypass() {
    let base = amp_node("ACD_HiwattDR103CanMod", false, 0.5);
    let scene = serde_json::json!({
        "guitarNodes": { "G1": {
            "ACD_HiwattDR103CanMod": { "dspUnitParameters": { "bypass": true } }
        } }
    });
    let eff = overlay_effective_amp(
        &base,
        Some(&scene),
        "G1",
        "ACD_HiwattDR103CanMod",
        "ACD_HiwattDR103CanMod",
    )
    .unwrap();
    assert_eq!(eff.bypass, Some(true), "overlay bypass wins over base");
    assert_eq!(
        eff.output_level,
        Some(0.5),
        "outputLevel falls through to base"
    );
}

// Overlay sets outputLevel: base ol=0.5, overlay ol=0.9 → effective 0.9.
#[test]
fn overlay_sets_output_level() {
    let base = amp_node("ACD_TM59Bassman", false, 0.5);
    let scene = serde_json::json!({
        "guitarNodes": { "G2": {
            "ACD_TM59Bassman": { "dspUnitParameters": { "outputLevel": 0.9 } }
        } }
    });
    let eff = overlay_effective_amp(
        &base,
        Some(&scene),
        "G2",
        "ACD_TM59Bassman",
        "ACD_TM59Bassman",
    )
    .unwrap();
    assert_eq!(eff.output_level, Some(0.9));
    assert_eq!(eff.bypass, Some(false), "bypass falls through to base");
}

// A param absent from the overlay (empty overlay for this amp) → the base value.
#[test]
fn param_absent_from_overlay_uses_base() {
    let base = amp_node("ACD_TwinReverb", true, 0.3);
    // Scene present, but the overlay carries no entry for this amp at all.
    let scene = serde_json::json!({ "guitarNodes": { "G1": {} } });
    let eff = overlay_effective_amp(
        &base,
        Some(&scene),
        "G1",
        "ACD_TwinReverb",
        "ACD_TwinReverb",
    )
    .unwrap();
    assert_eq!(eff.bypass, Some(true));
    assert_eq!(eff.output_level, Some(0.3));
}

// Overlay keyed by nodeId (not FenderId) still resolves via the nodeId fallback.
#[test]
fn overlay_keyed_by_node_id_fallback() {
    let base = serde_json::json!({
        "nodeId": "node-7", "FenderId": "ACD_TwinReverb",
        "dspUnitParameters": { "bypass": false, "outputLevel": 0.5 }
    });
    let scene = serde_json::json!({
        "guitarNodes": { "G1": { "node-7": { "dspUnitParameters": { "bypass": true } } } }
    });
    let eff = overlay_effective_amp(&base, Some(&scene), "G1", "ACD_TwinReverb", "node-7").unwrap();
    assert_eq!(eff.bypass, Some(true));
}

// Scene index missing from scenes[] (caller passes None) → None/skip.
#[test]
fn scene_index_missing_is_skip() {
    let base = amp_node("ACD_TwinReverb", false, 0.5);
    assert!(overlay_effective_amp(&base, None, "G1", "ACD_TwinReverb", "ACD_TwinReverb").is_none());
}

// A malformed/truncated scene entry (not a JSON object) → None/skip.
#[test]
fn malformed_scene_entry_is_skip() {
    let base = amp_node("ACD_TwinReverb", false, 0.5);
    // Truncation / corruption can leave a non-object where a scene object was expected.
    assert!(overlay_effective_amp(
        &base,
        Some(&Value::Null),
        "G1",
        "ACD_TwinReverb",
        "ACD_TwinReverb"
    )
    .is_none());
    assert!(overlay_effective_amp(
        &base,
        Some(&serde_json::json!("truncated")),
        "G1",
        "ACD_TwinReverb",
        "ACD_TwinReverb"
    )
    .is_none());
}

// The base scene (no overlay) reads the base node's params verbatim.
#[test]
fn base_effective_reads_base_params() {
    let base = amp_node("ACD_HiwattDR103CanMod", true, 0.42);
    let eff = base_effective_amp(&base);
    assert_eq!(eff.bypass, Some(true));
    assert_eq!(eff.output_level, Some(0.42));
}

// amp_nodes enumerates only amp models from the guitar graph (skips non-amps).
#[test]
fn amp_nodes_enumerates_only_amps() {
    let preset = serde_json::json!({
        "audioGraph": { "guitarNodes": {
            "G1": [ { "nodeId": "ACD_TMReverse", "FenderId": "ACD_TMReverse",
                      "dspUnitParameters": { "bypass": false } } ],
            "G2": [ amp_node("ACD_TM59Bassman", false, 0.5) ]
        } }
    });
    let amps = amp_nodes(&preset);
    assert_eq!(amps.len(), 1, "only the amp node is kept");
    assert_eq!(amps[0].group, "G2");
    assert_eq!(amps[0].node_id, "ACD_TM59Bassman");
}
