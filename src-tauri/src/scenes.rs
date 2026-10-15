//! Scene helpers for leveling: the per-scene amp pick (live-graph bypass read) and
//! the common-target normalization math. Scene names and the live scene index come
//! from the unit (the monitor's field-3 decode), not from this module.

use serde_json::Value;

/// The node object at `group_id`/`node_id` under `preset`'s `audioGraph.guitarNodes`
/// (an array per group; matched by instance via `audiograph::instance_id`). Shared
/// node-lookup for [`block_bypass_in_live_graph`] and the scene-repair diff
/// (`scene_jobs::node_dsp_params`) — both need "find this node in a
/// materialized `audioGraph` doc" and previously duplicated the pointer/array/find walk.
pub fn guitar_node<'a>(preset: &'a Value, group_id: &str, node_id: &str) -> Option<&'a Value> {
    preset
        .pointer(&format!("/audioGraph/guitarNodes/{group_id}"))?
        .as_array()?
        .iter()
        .find(|n| crate::audiograph::instance_id(n) == Some(node_id))
}

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
    guitar_node(preset, group_id, node_id)?
        .pointer("/dspUnitParameters/bypass")
        .and_then(Value::as_bool)
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
}
