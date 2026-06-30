//! Scene helpers for leveling: the per-scene amp pick (live-graph bypass read) and
//! the common-target normalization math. Scene names and the live scene index come
//! from the unit (the monitor's field-3 decode), not from this module.

use serde_json::Value;

use crate::leveller::common_target;

/// A block's `bypass` state in the LIVE `audioGraph` (`audioGraph.guitarNodes[group]`,
/// an array of nodes each carrying `nodeId`/`FenderId`). `None` = the block wasn't
/// found / has no `bypass`.
///
/// Drives the PER-SCENE amp pick for scene leveling: a preset can carry several amps
/// with scenes swapping which is active (HW-found: leveling the Twin's knob
/// in a scene that runs the other amp measured flat → clamped), so the leveling knob
/// must belong to the amp ON in the currently-loaded scene. Reads the live graph (with
/// the scene loaded over USB), NOT the stored `scenes[]` overlay — the overlay flags
/// proved unstable (the same scene read opposite amp states in different read contexts).
pub fn block_bypass_in_live_graph(preset: &Value, group_id: &str, node_id: &str) -> Option<bool> {
    let nodes = preset
        .pointer(&format!("/audioGraph/guitarNodes/{group_id}"))?
        .as_array()?;
    nodes
        .iter()
        .find(|n| crate::audiograph::node_id(n) == Some(node_id))
        .and_then(|n| n.pointer("/dspUnitParameters/bypass"))
        .and_then(Value::as_bool)
}

/// Per-scene normalization deltas to a common target (MEASURE math): given each
/// scene's measured ceiling `cs[i]`, the target is `min(C) − headroom`
/// (`leveller::common_target`) and the per-scene delta is `target − cs[i]` (≤ 0 LU,
/// the trim each scene needs so all scenes match in loudness). `None` if empty.
pub fn normalize_scene_targets(cs: &[f64], headroom_lu: f64) -> Option<Vec<f64>> {
    let target = common_target(cs, headroom_lu)?;
    Some(cs.iter().map(|c| target - c).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The per-scene amp pick's bypass resolver reads the LIVE audioGraph.
    #[test]
    fn block_bypass_in_live_graph_reads_the_amp_state() {
        // Two amps: A on, B bypassed (nodeId- and FenderId-keyed both resolve).
        let p = serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "nodeId": "ampA", "dspUnitParameters": { "bypass": false } },
                { "FenderId": "ampB", "dspUnitParameters": { "bypass": true } }
            ] } }
        });
        assert_eq!(block_bypass_in_live_graph(&p, "G1", "ampA"), Some(false));
        assert_eq!(block_bypass_in_live_graph(&p, "G1", "ampB"), Some(true));
        // Unknown block / unknown group → None.
        assert_eq!(block_bypass_in_live_graph(&p, "G1", "nope"), None);
        assert_eq!(block_bypass_in_live_graph(&p, "G9", "ampA"), None);
    }

    // AC — scene normalization: deltas to min(C) − headroom (MEASURE math).
    #[test]
    fn normalize_targets_to_common() {
        let cs = vec![-22.0, -25.0, -20.0];
        let deltas = normalize_scene_targets(&cs, 1.0).unwrap();
        // target = min(-25,-22,-20) - 1 = -26.
        assert!((deltas[0] - (-26.0 - -22.0)).abs() < 1e-9); // -4
        assert!((deltas[1] - (-26.0 - -25.0)).abs() < 1e-9); // -1
        assert!((deltas[2] - (-26.0 - -20.0)).abs() < 1e-9); // -6
        assert!(
            deltas.iter().all(|d| *d <= 0.0),
            "all scenes trim down to the common floor"
        );
        assert!(normalize_scene_targets(&[], 1.0).is_none());
    }
}
