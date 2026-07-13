//! Unit tests for the overlay A/B arm (sibling of `overlay_ab.rs`). The saved-side
//! per-scene overlay derivation now routes through `scene_jobs::scene_docs_from_saved`
//! (covered by `scene_jobs_tests.rs`), so only the arm's own amp-enumeration helper is
//! pinned here — no device.

use super::*;

fn amp_node(fid: &str, bypass: bool, ol: f64) -> Value {
    serde_json::json!({
        "nodeId": fid, "FenderId": fid,
        "dspUnitParameters": { "bypass": bypass, "outputLevel": ol }
    })
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
