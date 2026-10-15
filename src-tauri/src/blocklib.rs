//! Block library & copy-block-params: save a block's full parameter set as
//! a named library entry (a JSON store in the app config dir, mirroring `profiles.rs`),
//! and copy it into the matching-model block across selected presets (OFFLINE param
//! write).
//!
//! Same-model only: an entry captures `dspUnitParameters` for one `nodeId`, and apply
//! writes them into blocks of that exact model. No cross-model translation.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::audiograph::for_each_node_mut;

/// A saved block: a name, the block model (`nodeId`), and its full parameter set.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BlockTemplate {
    pub name: String,
    pub model: String,
    pub params: Map<String, Value>,
}

/// Copy a template's params into every matching-model block in `preset`. Returns the
/// number of blocks written (0 = preset has no block of that model → skip).
pub fn apply_block(preset: &mut Value, template: &BlockTemplate) -> usize {
    for_each_node_mut(preset, |node| {
        if !crate::audiograph::Node::new(node).matches_model(&template.model) {
            return false;
        }
        node.insert(
            "dspUnitParameters".into(),
            Value::Object(template.params.clone()),
        );
        true
    })
}

/// Bulk-run operation: apply a library entry to the matching-model block.
pub struct ApplyBlockOp {
    pub template: BlockTemplate,
}
impl crate::bulkrun::Operation for ApplyBlockOp {
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
}
