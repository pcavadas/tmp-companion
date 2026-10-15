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
pub(crate) const GRAPHS: [&str; 2] = ["guitarNodes", "micNodes"];

/// The preset's master `presetLevel` (`audioGraph.presetLevel`, linear amplitude).
/// Shared by search indexing and the leveling revert snapshot.
pub(crate) fn preset_level(preset: &Value) -> Option<f64> {
    preset
        .get("audioGraph")
        .and_then(|a| a.get("presetLevel"))
        .and_then(Value::as_f64)
}

/// A typed read view over one `audioGraph` node object. It borrows the JSON map, so every
/// key the app does not model (`nodeType`, firmware extras) survives a round-trip untouched.
///
/// A node carries two identifiers, and they answer different questions:
/// - the **model** (`FenderId`, e.g. `ACD_TubeScreamer`) says WHICH block it is;
/// - the **instance** (`nodeId`) says WHICH COPY it is. The firmware mints it per group as the
///   model id, then `<model>_1`, `<model>_2`… for repeats ([`mint_node_id`]).
///
/// Model-level work (migration, bulk replace by model, block counts, block templates) matches
/// with [`Node::matches_model`]; instance-level work (live edits, insert anchors, scene
/// overlays, footswitch refs) with [`Node::matches_instance`]. These two accessors are the ONLY
/// place the `nodeId` ↔ `FenderId` fallback lives.
#[derive(Clone, Copy, Debug)]
pub struct Node<'a>(&'a Map<String, Value>);

/// Unmodelled keys stay reachable as plain JSON (`node.get("nodeType")`).
impl std::ops::Deref for Node<'_> {
    type Target = Map<String, Value>;
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<'a> Node<'a> {
    pub fn new(obj: &'a Map<String, Value>) -> Self {
        Node(obj)
    }

    /// The view over `v`, if it is a JSON object.
    pub fn of(v: &'a Value) -> Option<Self> {
        v.as_object().map(Node)
    }

    fn str_key(self, key: &str) -> Option<&'a str> {
        self.0.get(key).and_then(Value::as_str)
    }

    /// The block model (`FenderId`), falling back to `nodeId` for a graph that carries no
    /// `FenderId` (the firmware's first instance id IS the model id).
    pub fn model(self) -> Option<&'a str> {
        self.str_key("FenderId").or_else(|| self.str_key("nodeId"))
    }

    /// The block instance (`nodeId`), falling back to `FenderId` for a graph that carries no
    /// `nodeId` (the firmware mints the model id for a group's first instance).
    pub fn instance(self) -> Option<&'a str> {
        self.str_key("nodeId").or_else(|| self.str_key("FenderId"))
    }

    /// The node's `dspUnitParameters`, when it carries an object there.
    pub fn params(self) -> Option<&'a Map<String, Value>> {
        self.0.get("dspUnitParameters").and_then(Value::as_object)
    }

    /// Is this node a block of model `model`? Matches every instance of it (`X`, `X_1`, …).
    pub fn matches_model(self, model: &str) -> bool {
        self.model() == Some(model)
    }

    /// Is this node the one instance `node_id`?
    pub fn matches_instance(self, node_id: &str) -> bool {
        self.instance() == Some(node_id)
    }
}

/// [`Node::instance`] of a node held as a `Value` (`None` when it is not an object).
pub fn instance_id(node: &Value) -> Option<&str> {
    Node::of(node).and_then(Node::instance)
}

/// The firmware's id for a new node of model `model` in a group already using `ids`
/// (fw 1.8.58 `FUN_00c7fd80`): the model id, or the first free `<model>_<n>`. The simulator's
/// insert/replace mints with it, so it mints the way the unit does and two blocks in one group
/// never share an id (a duplicate id at load orphans the first unit).
#[cfg(any(test, feature = "e2e"))]
pub fn mint_node_id(ids: &[String], model: &str) -> String {
    if !ids.iter().any(|id| id == model) {
        return model.to_string();
    }
    (1..)
        .map(|n| format!("{model}_{n}"))
        .find(|id| !ids.contains(id))
        .expect("an unbounded range always finds a free id")
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

/// Visit every node under `audioGraph.*Nodes.<group>[]` read-only as `(graph, group, node)`,
/// in the same stable (sorted-group) order as [`for_each_node_mut`]. The ONE read-only walk:
/// [`for_each_node`], [`roster`] and the counts below are all built on it.
pub fn walk_nodes<'a>(preset: &'a Value, mut f: impl FnMut(&'a str, &'a str, Node<'a>)) {
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
                if let Some(node) = Node::of(node) {
                    f(graph, k, node);
                }
            }
        }
    }
}

/// Visit every node object under `audioGraph.*Nodes.<group>[]` read-only, in the same
/// stable (sorted-group) order as [`for_each_node_mut`]. The read-only companion the
/// scan paths (block/IR/scene/search inventories) share instead of re-walking the tree.
pub fn for_each_node<'a>(preset: &'a Value, mut f: impl FnMut(Node<'a>)) {
    walk_nodes(preset, |_, _, node| f(node));
}

/// A numeric `dspUnitParameter` of `node_id` (e.g. a footswitch lev param's current value =
/// `valueB`). The last matching node wins, as the walk visits them.
pub(crate) fn node_param_f64(preset: &Value, node_id: &str, param: &str) -> Option<f64> {
    let mut found = None;
    for_each_node(preset, |node| {
        if node.matches_instance(node_id) {
            found = node
                .params()
                .and_then(|p| p.get(param))
                .and_then(|v| v.as_f64());
        }
    });
    found
}

/// Flat block roster for the backup library read: `(group, node_id, fender_id)` for
/// every real (FenderId-bearing) node, in the same stable sorted-group order as
/// [`for_each_node`]. The frontend derives the Step-1 "blocks present" list, per-preset
/// counts, and per-preset CPU total (via the model-cpu table) from this — no extra
/// device round-trip, since it rides the backup stream the app already pulls on connect.
pub fn roster(preset: &Value) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    walk_nodes(preset, |_, group, node| {
        if let (Some(nid), Some(fid)) = (
            node.instance(),
            node.0.get("FenderId").and_then(Value::as_str),
        ) {
            out.push((group.to_string(), nid.to_string(), fid.to_string()));
        }
    });
    out
}

/// The roster row `(group, node_id, fender_id)` for `node`. An exact INSTANCE match wins;
/// only when no instance carries that id does a model id resolve, to that model's first
/// instance (callers that hold a `FenderId` for a single-instance block). So `X_1` resolves
/// to `X_1`, never to its sibling `X`. Shared by `scene_jobs::scene_overlay` and
/// `leveller::FsParamTarget::from_preset` so the two can't drift.
pub fn roster_entry(preset: &Value, node: &str) -> Option<(String, String, String)> {
    let roster = roster(preset);
    let pos = roster
        .iter()
        .position(|(_, nid, _)| nid == node)
        .or_else(|| roster.iter().position(|(_, _, fid)| fid == node))?;
    roster.into_iter().nth(pos)
}

/// Count the blocks of model `model` across the graph — every instance (`X`, `X_1`, …).
pub fn count_model_nodes(preset: &Value, model: &str) -> usize {
    let mut n = 0;
    for_each_node(preset, |node| n += usize::from(node.matches_model(model)));
    n
}

/// Set the `dspUnitParameters.bypass` flag to `bypass` on every block whose MODEL is in
/// `ids` — every instance of it (base scene only). Returns the number of matching blocks set.
pub fn set_bypass(
    preset: &mut Value,
    ids: &std::collections::HashSet<String>,
    bypass: bool,
) -> usize {
    for_each_node_mut(preset, |node| {
        let hit = Node::new(node).model().is_some_and(|m| ids.contains(m));
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
mod preset_level_tests {
    use super::*;

    #[test]
    fn reads_audiograph_preset_level_and_tolerates_absence() {
        let with = serde_json::json!({"audioGraph": {"presetLevel": 0.8}});
        assert_eq!(preset_level(&with), Some(0.8));
        assert_eq!(preset_level(&serde_json::json!({"audioGraph": {}})), None);
        assert_eq!(preset_level(&serde_json::json!({})), None);
        // Non-numeric value is absence, not a panic.
        let bad = serde_json::json!({"audioGraph": {"presetLevel": "loud"}});
        assert_eq!(preset_level(&bad), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slot::ListIndex;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn preset_with(node_id: &str, params: Value) -> Value {
        serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "FenderId": node_id, "nodeId": node_id, "nodeType": "dspUnit", "dspUnitParameters": params }
            ] } }
        })
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
            list_index: ListIndex(0),
            list_enum: 1,
            display_name: "x".into(),
            source: crate::backup::SnapshotSource::OfflineFile,
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
                    if instance_id(n) == Some("ACD_UserIRTMS") {
                        assert_eq!(n["dspUnitParameters"]["bypass"], true);
                        found = true;
                    }
                }
            }
        }
        assert!(found, "IR node present after round-trip");
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

    // ─── duplicate instances of one model (`X`, `X_1`) ────────────────────────

    /// Two instances of one model in G1 (`ACD_TS`, `ACD_TS_1`), plus a different block, each
    /// TS instance with its own scene overlay and footswitch ref.
    fn preset_with_two_instances() -> Value {
        serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "FenderId": "ACD_TS", "nodeId": "ACD_TS", "dspUnitParameters": { "drive": 0.2 } },
                { "FenderId": "ACD_TS", "nodeId": "ACD_TS_1", "dspUnitParameters": { "drive": 0.8 } },
                { "FenderId": "ACD_Klon", "nodeId": "ACD_Klon", "dspUnitParameters": {} }
            ] } },
            "scenes": [
                { "guitarNodes": { "G1": {
                    "ACD_TS": { "dspUnitParameters": { "bypass": true } },
                    "ACD_TS_1": { "dspUnitParameters": { "bypass": false } },
                    "ACD_Klon": { "dspUnitParameters": { "bypass": true } }
                } } }
            ],
            "ftsw": [
                [ { "func": "on-off", "nodes": [ { "groupId": "G1", "nodeId": "ACD_TS" } ] } ],
                [ { "func": "on-off", "nodes": [ { "groupId": "G1", "nodeId": "ACD_TS_1" } ] } ]
            ]
        })
    }

    #[test]
    fn node_view_separates_model_from_instance() {
        let p = preset_with_two_instances();
        let second = Node::of(&p["audioGraph"]["guitarNodes"]["G1"][1]).unwrap();
        assert_eq!(second.model(), Some("ACD_TS"));
        assert_eq!(second.instance(), Some("ACD_TS_1"));
        assert!(second.matches_model("ACD_TS"));
        assert!(!second.matches_instance("ACD_TS"));
        // A graph without one of the two keys falls back to the other.
        let bare = serde_json::json!({ "nodeId": "amp" });
        assert_eq!(Node::of(&bare).unwrap().model(), Some("amp"));
        let fid_only = serde_json::json!({ "FenderId": "ACD_TS" });
        assert_eq!(instance_id(&fid_only), Some("ACD_TS"));
    }

    #[test]
    fn count_model_nodes_counts_every_instance() {
        let p = preset_with_two_instances();
        assert_eq!(count_model_nodes(&p, "ACD_TS"), 2);
        assert_eq!(
            count_model_nodes(&p, "ACD_TS_1"),
            0,
            "an instance id is not a model"
        );
        assert_eq!(count_model_nodes(&p, "ACD_Klon"), 1);
    }

    #[test]
    fn roster_entry_prefers_the_exact_instance() {
        let p = preset_with_two_instances();
        let (_, nid, fid) = roster_entry(&p, "ACD_TS_1").unwrap();
        assert_eq!((nid.as_str(), fid.as_str()), ("ACD_TS_1", "ACD_TS"));
        // A model id resolves to its first instance.
        assert_eq!(roster_entry(&p, "ACD_TS").unwrap().1, "ACD_TS");
    }

    #[test]
    fn mint_node_id_follows_the_firmware_rule() {
        let ids = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(mint_node_id(&ids(&[]), "X"), "X");
        assert_eq!(mint_node_id(&ids(&["X"]), "X"), "X_1");
        assert_eq!(mint_node_id(&ids(&["X", "X_1", "X_3"]), "X"), "X_2");
    }
}
