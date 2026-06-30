//! OFFLINE `audioGraph` node operations shared by the structural-edit features
//! (block-replace; bypass; insert/delete/move build on this).
//!
//! All operate on decoded preset JSON (the `.preset` codec output), since the partial
//! USB JSON can't round-trip a changed block list. A node lives at
//! `audioGraph.{guitarNodes,micNodes}.<group>[]` and looks like
//! `{ "FenderId": "ACD_…", "nodeId": "ACD_…", "nodeType": "dspUnit",
//! "dspUnitParameters": { … } }`.
//!
//! Generic node ops over the `audioGraph` tree — we deliberately do NOT model the full
//! block schema.

use serde_json::{Map, Value};

/// The two node graphs a preset carries.
const GRAPHS: [&str; 2] = ["guitarNodes", "micNodes"];

/// A node's id (`nodeId`, falling back to `FenderId`).
pub fn node_id(node: &Value) -> Option<&str> {
    node.get("nodeId")
        .and_then(Value::as_str)
        .or_else(|| node.get("FenderId").and_then(Value::as_str))
}

/// Visit every node object under `audioGraph.*Nodes.<group>[]` mutably, in a stable
/// order. `f` returns `true` if it mutated the node; the total count is returned.
pub fn for_each_node_mut(
    preset: &mut Value,
    mut f: impl FnMut(&mut Map<String, Value>) -> bool,
) -> usize {
    let mut changed = 0;
    let Some(ag) = preset.get_mut("audioGraph").and_then(Value::as_object_mut) else {
        return 0;
    };
    for graph in GRAPHS {
        let Some(groups) = ag.get_mut(graph).and_then(Value::as_object_mut) else {
            continue;
        };
        // Iterate groups in sorted key order so behaviour is deterministic.
        let mut keys: Vec<String> = groups.keys().cloned().collect();
        keys.sort();
        for k in keys {
            let Some(nodes) = groups.get_mut(&k).and_then(Value::as_array_mut) else {
                continue;
            };
            for node in nodes.iter_mut() {
                if let Some(obj) = node.as_object_mut() {
                    if f(obj) {
                        changed += 1;
                    }
                }
            }
        }
    }
    changed
}

/// Visit every node object under `audioGraph.*Nodes.<group>[]` read-only, in the same
/// stable (sorted-group) order as [`for_each_node_mut`]. The read-only companion the
/// scan paths (block/IR/scene/search inventories) share instead of re-walking the tree.
pub fn for_each_node(preset: &Value, mut f: impl FnMut(&Map<String, Value>)) {
    let Some(ag) = preset.get("audioGraph").and_then(Value::as_object) else {
        return;
    };
    for graph in GRAPHS {
        let Some(groups) = ag.get(graph).and_then(Value::as_object) else {
            continue;
        };
        let mut keys: Vec<&String> = groups.keys().collect();
        keys.sort();
        for k in keys {
            for node in groups[k].as_array().into_iter().flatten() {
                if let Some(obj) = node.as_object() {
                    f(obj);
                }
            }
        }
    }
}

/// Flat block roster for the backup library read: `(group, node_id, fender_id)` for
/// every real (FenderId-bearing) node, in the same stable sorted-group order as
/// [`for_each_node`]. The frontend derives the Step-1 "blocks present" list, per-preset
/// counts, and per-preset CPU total (via the model-cpu table) from this — no extra
/// device round-trip, since it rides the backup stream the app already pulls on connect.
pub fn roster(preset: &Value) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let Some(ag) = preset.get("audioGraph").and_then(Value::as_object) else {
        return out;
    };
    for graph in GRAPHS {
        let Some(groups) = ag.get(graph).and_then(Value::as_object) else {
            continue;
        };
        let mut keys: Vec<&String> = groups.keys().collect();
        keys.sort();
        for k in keys {
            for node in groups[k].as_array().into_iter().flatten() {
                let fid = node.get("FenderId").and_then(Value::as_str);
                let nid = node.get("nodeId").and_then(Value::as_str).or(fid);
                if let (Some(nid), Some(fid)) = (nid, fid) {
                    out.push((k.clone(), nid.to_string(), fid.to_string()));
                }
            }
        }
    }
    out
}

/// Count nodes whose id equals `id` across the graph.
pub fn count_nodes_with_id(preset: &Value, id: &str) -> usize {
    let mut n = 0;
    for graph in GRAPHS {
        if let Some(groups) = preset
            .pointer(&format!("/audioGraph/{graph}"))
            .and_then(Value::as_object)
        {
            for nodes in groups.values() {
                if let Some(arr) = nodes.as_array() {
                    n += arr.iter().filter(|node| node_id(node) == Some(id)).count();
                }
            }
        }
    }
    n
}

/// Drop every per-scene parameter override for block `id`. A preset stores each
/// scene's full block state under `scenes[].{guitarNodes,micNodes}.<group>.<FenderId>`;
/// on a model swap the old block's scene-scoped tweaks are meaningless for the new
/// model, and on a removal they must go too — so both paths drop them (the surviving
/// block then plays its base params in every scene). Returns entries removed.
fn drop_scene_overrides(preset: &mut Value, id: &str) -> usize {
    let mut removed = 0;
    let Some(scenes) = preset.get_mut("scenes").and_then(Value::as_array_mut) else {
        return 0;
    };
    for scene in scenes.iter_mut() {
        for graph in GRAPHS {
            let Some(groups) = scene.get_mut(graph).and_then(Value::as_object_mut) else {
                continue;
            };
            for blocks in groups.values_mut() {
                if let Some(obj) = blocks.as_object_mut() {
                    if obj.remove(id).is_some() {
                        removed += 1;
                    }
                }
            }
        }
    }
    removed
}

/// Retarget footswitch on-off assignments that point at block `from`. A footswitch is
/// `ftsw[i][].nodes[] = { groupId, nodeId }`; on a replace we point it at `to` (the
/// switch keeps controlling the swapped-in block), on a remove (`to == None`) we drop
/// the now-dangling node ref (the switch may end up controlling nothing, a valid
/// empty state). Returns the number of refs retargeted/removed.
fn retarget_ftsw(preset: &mut Value, from: &str, to: Option<&str>) -> usize {
    let mut n = 0;
    let Some(switches) = preset.get_mut("ftsw").and_then(Value::as_array_mut) else {
        return 0;
    };
    for sw in switches.iter_mut() {
        let Some(layers) = sw.as_array_mut() else {
            continue;
        };
        for layer in layers.iter_mut() {
            let Some(nodes) = layer.get_mut("nodes").and_then(Value::as_array_mut) else {
                continue;
            };
            match to {
                Some(to) => {
                    for nd in nodes.iter_mut() {
                        if nd.get("nodeId").and_then(Value::as_str) == Some(from) {
                            if let Some(o) = nd.as_object_mut() {
                                o.insert("nodeId".into(), Value::String(to.to_string()));
                                n += 1;
                            }
                        }
                    }
                }
                None => {
                    let before = nodes.len();
                    nodes.retain(|nd| nd.get("nodeId").and_then(Value::as_str) != Some(from));
                    n += before - nodes.len();
                }
            }
        }
    }
    n
}

/// Replace every block whose id is `from` with model `to` (id + FenderId), resetting
/// its parameters to `b_defaults`. Returns the number of nodes replaced. `nodeType` is
/// preserved. Presets without `from` are left untouched (count 0). When at least one
/// node is replaced, the block's stale per-scene overrides are dropped and any
/// footswitch on-off assignment is retargeted to `to`, so no scene/footswitch is left
/// referencing the old block.
pub fn replace_block(
    preset: &mut Value,
    from: &str,
    to: &str,
    b_defaults: &Map<String, Value>,
) -> usize {
    let n = for_each_node_mut(preset, |node| {
        let is_target = node
            .get("nodeId")
            .and_then(Value::as_str)
            .or_else(|| node.get("FenderId").and_then(Value::as_str))
            == Some(from);
        if !is_target {
            return false;
        }
        node.insert("nodeId".into(), Value::String(to.to_string()));
        node.insert("FenderId".into(), Value::String(to.to_string()));
        node.insert(
            "dspUnitParameters".into(),
            Value::Object(b_defaults.clone()),
        );
        true
    });
    if n > 0 {
        drop_scene_overrides(preset, from);
        retarget_ftsw(preset, from, Some(to));
    }
    n
}

/// Set the `dspUnitParameters.bypass` flag to `bypass` on every block whose id is in
/// `ids` (base scene only). Returns the number of matching blocks set.
pub fn set_bypass(
    preset: &mut Value,
    ids: &std::collections::HashSet<String>,
    bypass: bool,
) -> usize {
    for_each_node_mut(preset, |node| {
        let id = node
            .get("nodeId")
            .and_then(Value::as_str)
            .or_else(|| node.get("FenderId").and_then(Value::as_str));
        let hit = id.map(|i| ids.contains(i)).unwrap_or(false);
        if !hit {
            return false;
        }
        let params = node
            .entry("dspUnitParameters")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(obj) = params.as_object_mut() {
            obj.insert("bypass".into(), Value::Bool(bypass));
        }
        true
    })
}

/// Bulk-run operation: set the bypass state of all blocks of a chosen type
/// (`ids`) across a preset. `transform` returns `None` (skip) when no `ids` block is
/// present, else the edited JSON.
pub struct BulkBypassOp {
    pub ids: std::collections::HashSet<String>,
    pub bypass: bool,
}

impl crate::bulkrun::Operation for BulkBypassOp {
    fn label(&self) -> String {
        format!(
            "{} {} block type(s)",
            if self.bypass { "bypass" } else { "enable" },
            self.ids.len()
        )
    }
    fn transform(&self, t: &crate::bulkrun::PresetTarget) -> Result<Option<String>, String> {
        let mut v: Value =
            serde_json::from_str(&t.before_json).map_err(|e| format!("parse: {e}"))?;
        if set_bypass(&mut v, &self.ids, self.bypass) == 0 {
            return Ok(None); // not a target
        }
        Ok(Some(serde_json::to_string(&v).map_err(|e| e.to_string())?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn defaults(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn preset_with(node_id: &str, params: Value) -> Value {
        serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "FenderId": node_id, "nodeId": node_id, "nodeType": "dspUnit", "dspUnitParameters": params }
            ] } }
        })
    }

    // AC1 — every A node becomes a valid B node at the same path with B defaults.
    #[test]
    fn swap_model_a_to_b_in_audiograph() {
        let mut p = preset_with("ACD_TubeScreamer", serde_json::json!({ "drive": 0.7 }));
        let bd = defaults(&[
            ("gain", serde_json::json!(0.1)),
            ("level", serde_json::json!(0.5)),
        ]);
        let n = replace_block(&mut p, "ACD_TubeScreamer", "ACD_KlonCentaur", &bd);
        assert_eq!(n, 1);
        let node = &p["audioGraph"]["guitarNodes"]["G1"][0];
        assert_eq!(node["nodeId"], "ACD_KlonCentaur");
        assert_eq!(node["FenderId"], "ACD_KlonCentaur");
        assert_eq!(node["nodeType"], "dspUnit"); // preserved
        assert_eq!(
            node["dspUnitParameters"],
            serde_json::json!({ "gain": 0.1, "level": 0.5 })
        );
    }

    // AC2 (round-trip) — mutate the decoded fixture, re-encode to `.preset` bytes
    // (XOR), decode again → equals the mutated JSON (the OFFLINE re-import contract).
    // Auto-skips when the gitignored fixture is absent.
    #[test]
    fn reencode_roundtrips() {
        let xor = crate::backup::xor_jld;
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/Guitar.preset");
        let Ok(file) = std::fs::read(&path) else {
            eprintln!("skip: fixture {} absent", path.display());
            return;
        };
        let json_str = String::from_utf8(xor(&file)).unwrap();
        let mut v: Value = serde_json::from_str(&json_str).unwrap();

        // Guitar.preset has ACD_KlonCentaur; swap it to ACD_Pugilist.
        assert!(count_nodes_with_id(&v, "ACD_KlonCentaur") >= 1);
        let n = replace_block(&mut v, "ACD_KlonCentaur", "ACD_Pugilist", &Map::new());
        assert!(n >= 1);

        // Re-encode → .preset bytes → decode again, and confirm losslessness.
        let mutated = serde_json::to_string(&v).unwrap();
        let preset_bytes = xor(mutated.as_bytes());
        let decoded_again = String::from_utf8(xor(&preset_bytes)).unwrap();
        assert_eq!(decoded_again, mutated, "XOR re-encode round-trips exactly");
        let reparsed: Value = serde_json::from_str(&decoded_again).unwrap();
        assert_eq!(count_nodes_with_id(&reparsed, "ACD_KlonCentaur"), 0);
        assert!(count_nodes_with_id(&reparsed, "ACD_Pugilist") >= 1);
    }

    fn ids(list: &[&str]) -> HashSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    // AC1 — bypass flips for every matching block; others untouched.
    #[test]
    fn set_bypass_for_block_type() {
        let mut p = serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "nodeId": "ACD_RevA", "dspUnitParameters": { "bypass": false, "mix": 0.4 } },
                { "nodeId": "ACD_RevB", "dspUnitParameters": { "bypass": false } },
                { "nodeId": "ACD_Drive", "dspUnitParameters": { "bypass": false } }
            ] } }
        });
        let n = set_bypass(&mut p, &ids(&["ACD_RevA", "ACD_RevB"]), true);
        assert_eq!(n, 2);
        let g1 = &p["audioGraph"]["guitarNodes"]["G1"];
        assert_eq!(g1[0]["dspUnitParameters"]["bypass"], true);
        assert_eq!(
            g1[0]["dspUnitParameters"]["mix"], 0.4,
            "other params untouched"
        );
        assert_eq!(g1[1]["dspUnitParameters"]["bypass"], true);
        assert_eq!(
            g1[2]["dspUnitParameters"]["bypass"], false,
            "non-target untouched"
        );
    }

    // AC3 — a preset without the block type is skipped (op → None).
    #[test]
    fn skip_presets_without_block() {
        use crate::bulkrun::{Operation, PresetTarget};
        let p = preset_with("ACD_Drive", serde_json::json!({ "bypass": false }));
        let op = BulkBypassOp {
            ids: ids(&["ACD_RevA"]),
            bypass: true,
        };
        let t = PresetTarget {
            list_index: 0,
            list_enum: 1,
            display_name: "x".into(),
            source: "offline-file".into(),
            before_json: serde_json::to_string(&p).unwrap(),
        };
        assert_eq!(op.transform(&t).unwrap(), None);
    }

    // AC2 — bypassing a block in the fixture re-encodes losslessly.
    #[test]
    fn bypass_reencode_roundtrips() {
        let xor = crate::backup::xor_jld;
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/Guitar.preset");
        let Ok(file) = std::fs::read(&path) else {
            eprintln!("skip: fixture absent");
            return;
        };
        let mut v: Value = serde_json::from_str(&String::from_utf8(xor(&file)).unwrap()).unwrap();
        assert_eq!(set_bypass(&mut v, &ids(&["ACD_UserIRTMS"]), true), 1);
        let mutated = serde_json::to_string(&v).unwrap();
        let decoded_again = String::from_utf8(xor(&xor(mutated.as_bytes()))).unwrap();
        assert_eq!(decoded_again, mutated);
        let reparsed: Value = serde_json::from_str(&decoded_again).unwrap();
        // Find the IR node and confirm bypass=true survived the round-trip.
        let mut found = false;
        for nodes in reparsed["audioGraph"]["guitarNodes"]
            .as_object()
            .unwrap()
            .values()
        {
            if let Some(arr) = nodes.as_array() {
                for n in arr {
                    if node_id(n) == Some("ACD_UserIRTMS") {
                        assert_eq!(n["dspUnitParameters"]["bypass"], true);
                        found = true;
                    }
                }
            }
        }
        assert!(found, "IR node present after round-trip");
    }

    fn ids_of(preset: &Value, group: &str) -> Vec<String> {
        preset
            .pointer(&format!("/audioGraph/guitarNodes/{group}"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|n| node_id(n).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    // ─── remove + scene/ftsw ref rewrite ──────────────────────────────────────

    /// A preset carrying A and B in G1, a per-scene override for A, and a footswitch
    /// on-off assignment pointing at A — the shape the rewrite helpers must clean up.
    fn preset_with_scene_and_ftsw() -> Value {
        serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "FenderId": "ACD_A", "nodeId": "ACD_A", "nodeType": "dspUnit", "dspUnitParameters": { "gain": 0.5 } },
                { "FenderId": "ACD_B", "nodeId": "ACD_B", "nodeType": "dspUnit", "dspUnitParameters": { "tone": 0.3 } }
            ] } },
            "scenes": [
                { "guitarNodes": { "G1": {
                    "ACD_A": { "dspUnitParameters": { "bypass": true } },
                    "ACD_B": { "dspUnitParameters": { "bypass": false } }
                } } }
            ],
            "ftsw": [
                [ { "func": "on-off", "nodes": [ { "groupId": "G1", "nodeId": "ACD_A" } ] } ],
                [ { "func": "scene", "sceneSlot": 0 } ]
            ]
        })
    }

    #[test]
    fn replace_block_retargets_scene_and_ftsw() {
        let mut v = preset_with_scene_and_ftsw();
        let n = replace_block(
            &mut v,
            "ACD_A",
            "ACD_C",
            &defaults(&[("vol", serde_json::json!(0.7))]),
        );
        assert_eq!(n, 1);
        // node became C with B-defaults.
        assert_eq!(
            ids_of(&v, "G1"),
            vec!["ACD_C".to_string(), "ACD_B".to_string()]
        );
        // stale scene override for A dropped (the new block plays base params).
        assert!(v["scenes"][0]["guitarNodes"]["G1"].get("ACD_A").is_none());
        // footswitch on-off now controls C.
        assert_eq!(
            v["ftsw"][0][0]["nodes"][0]["nodeId"],
            serde_json::json!("ACD_C")
        );
    }

    #[test]
    fn roster_lists_every_block_with_group() {
        let v = preset_with_scene_and_ftsw();
        let r = roster(&v);
        assert_eq!(
            r,
            vec![
                ("G1".to_string(), "ACD_A".to_string(), "ACD_A".to_string()),
                ("G1".to_string(), "ACD_B".to_string(), "ACD_B".to_string()),
            ]
        );
    }
}
