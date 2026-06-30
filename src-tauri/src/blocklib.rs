//! Block library & copy-block-params: save a block's full parameter set as
//! a named library entry (a JSON store in the app config dir, mirroring `profiles.rs`),
//! and copy it into the matching-model block across selected presets (OFFLINE param
//! write).
//!
//! Same-model only: an entry captures `dspUnitParameters` for one `nodeId`, and apply
//! writes them into blocks of that exact model. No cross-model translation.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::audiograph::{for_each_node, for_each_node_mut};

/// A saved block: a name, the block model (`nodeId`), and its full parameter set.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BlockTemplate {
    pub name: String,
    pub model: String,
    pub params: Map<String, Value>,
}

/// Capture the first block of model `model` from `preset` into a named library entry
/// (all its `dspUnitParameters`). `None` if the preset has no such block.
pub fn capture_block(preset: &Value, model: &str, name: &str) -> Option<BlockTemplate> {
    let mut found = None;
    for_each_node(preset, |node| {
        if found.is_some() {
            return; // keep the first match
        }
        let id = node
            .get("nodeId")
            .and_then(Value::as_str)
            .or_else(|| node.get("FenderId").and_then(Value::as_str));
        if id == Some(model) {
            let params = node
                .get("dspUnitParameters")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            found = Some(BlockTemplate {
                name: name.to_string(),
                model: model.to_string(),
                params,
            });
        }
    });
    found
}

/// Copy a template's params into every matching-model block in `preset`. Returns the
/// number of blocks written (0 = preset has no block of that model → skip).
pub fn apply_block(preset: &mut Value, template: &BlockTemplate) -> usize {
    for_each_node_mut(preset, |node| {
        let id = node
            .get("nodeId")
            .and_then(Value::as_str)
            .or_else(|| node.get("FenderId").and_then(Value::as_str));
        if id != Some(template.model.as_str()) {
            return false;
        }
        node.insert(
            "dspUnitParameters".into(),
            Value::Object(template.params.clone()),
        );
        true
    })
}

/// Persist the library (Vec of entries) to `path` as pretty JSON.
pub fn save_library_to_path(path: &Path, lib: &[BlockTemplate]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    }
    let json = serde_json::to_string_pretty(lib).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Load the library from `path`; a missing file is an empty library.
pub fn load_library_from_path(path: &Path) -> Result<Vec<BlockTemplate>, String> {
    match std::fs::read(path) {
        Ok(b) => serde_json::from_slice(&b).map_err(|e| format!("parse {}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("read {}: {e}", path.display())),
    }
}

/// Bulk-run operation: apply a library entry to the matching-model block.
pub struct ApplyBlockOp {
    pub template: BlockTemplate,
}
impl crate::bulkrun::Operation for ApplyBlockOp {
    fn label(&self) -> String {
        format!(
            "apply block '{}' ({})",
            self.template.name, self.template.model
        )
    }
    fn transform(&self, t: &crate::bulkrun::PresetTarget) -> Result<Option<String>, String> {
        let mut v: Value =
            serde_json::from_str(&t.before_json).map_err(|e| format!("parse: {e}"))?;
        if apply_block(&mut v, &self.template) == 0 {
            return Ok(None); // no matching-model block → skip
        }
        Ok(Some(serde_json::to_string(&v).map_err(|e| e.to_string())?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(model: &str, params: Value) -> Value {
        serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "nodeId": model, "dspUnitParameters": params }
            ] } }
        })
    }

    // AC1 — saving a block captures all its params.
    #[test]
    fn save_block_captures_all_params() {
        let p = preset(
            "ACD_AC30",
            serde_json::json!({ "gain": 0.7, "bass": 0.4, "master": 0.6 }),
        );
        let t = capture_block(&p, "ACD_AC30", "AC30 reference").unwrap();
        assert_eq!(t.model, "ACD_AC30");
        assert_eq!(t.name, "AC30 reference");
        assert_eq!(t.params.len(), 3);
        assert_eq!(t.params["gain"], 0.7);
        // No such model → None.
        assert!(capture_block(&p, "ACD_Other", "x").is_none());
    }

    // AC2 — apply writes params into the matching-model block only.
    #[test]
    fn apply_writes_matching_model_only() {
        let t = BlockTemplate {
            name: "AC30 reference".into(),
            model: "ACD_AC30".into(),
            params: [("gain".to_string(), serde_json::json!(0.9))]
                .into_iter()
                .collect(),
        };
        // Matching preset → written.
        let mut p = preset("ACD_AC30", serde_json::json!({ "gain": 0.1, "bass": 0.5 }));
        assert_eq!(apply_block(&mut p, &t), 1);
        assert_eq!(
            p["audioGraph"]["guitarNodes"]["G1"][0]["dspUnitParameters"],
            serde_json::json!({ "gain": 0.9 })
        );
        // Non-matching preset → skipped (0).
        let mut other = preset("ACD_Twin", serde_json::json!({ "gain": 0.3 }));
        assert_eq!(apply_block(&mut other, &t), 0);
        assert_eq!(
            other["audioGraph"]["guitarNodes"]["G1"][0]["dspUnitParameters"]["gain"],
            0.3
        );
    }

    // AC4 — library entries persist on disk across restarts.
    #[test]
    fn library_store_roundtrips_on_disk() {
        let dir = std::env::temp_dir().join(format!("tmp-blocklib-{}", std::process::id()));
        let path = dir.join("blocks.json");
        let lib = vec![
            BlockTemplate {
                name: "AC30".into(),
                model: "ACD_AC30".into(),
                params: [("gain".to_string(), serde_json::json!(0.7))]
                    .into_iter()
                    .collect(),
            },
            BlockTemplate {
                name: "Gate".into(),
                model: "ACD_Gate".into(),
                params: Map::new(),
            },
        ];
        save_library_to_path(&path, &lib).unwrap();
        assert_eq!(load_library_from_path(&path).unwrap(), lib);
        // Missing file → empty library.
        assert!(load_library_from_path(&dir.join("nope.json"))
            .unwrap()
            .is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
