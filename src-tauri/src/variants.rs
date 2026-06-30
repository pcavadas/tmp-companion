//! Create variants: clone a source preset and apply a named recipe of
//! deltas (param edits, block swaps, tempo) to produce variants (single-coil,
//! humbucker, bass, FRFR, IEM, …), targeting empty slots only.
//!
//! OFFLINE: composes the existing edit ops (`paramedit`, `audiograph`, `presetmeta`)
//! over a clone. Recipes are data (deltas), not code. Empty slots are found by
//! observation (the marker is `--`; `requestNextEmptyPresetSlot` is dead on 1.7.75)
//! — see `session::is_empty_slot_name`. The device write (import to the empty slot)
//! is deferred to the manual runbook under the read-only policy.

use serde_json::Value;

use crate::audiograph::replace_block;
use crate::paramedit::{edit_param, ParamMode};
use crate::presetmeta::set_bpm;

/// One delta in a variant recipe.
#[derive(Debug, Clone, PartialEq)]
pub enum VariantEdit {
    /// Set a block parameter to an absolute value.
    SetParam {
        model: String,
        param: String,
        value: f64,
    },
    /// Swap a block model (B defaults).
    ReplaceBlock { from: String, to: String },
    /// Set the tempo.
    SetBpm(f64),
}

/// A named recipe: a display-name suffix + an ordered list of deltas.
#[derive(Debug, Clone, PartialEq)]
pub struct Recipe {
    pub name_suffix: String,
    pub edits: Vec<VariantEdit>,
}

/// Produce a variant: clone `source`, append the recipe's suffix to the display name,
/// and apply each delta in order. The clone keeps the source's structure + identity
/// fields except the name (the device write decides the new slot/identity).
pub fn apply_recipe(source: &Value, recipe: &Recipe) -> Result<Value, String> {
    let mut v = source.clone();
    // Rename: "<orig> <suffix>".
    let orig = v
        .pointer("/info/displayName")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if let Some(name) = v.pointer_mut("/info/displayName") {
        *name = Value::String(format!("{orig} {}", recipe.name_suffix).trim().to_string());
    }
    for edit in &recipe.edits {
        match edit {
            VariantEdit::SetParam {
                model,
                param,
                value,
            } => {
                edit_param(
                    &mut v,
                    model,
                    param,
                    ParamMode::Set(*value),
                    f64::MIN,
                    f64::MAX,
                );
            }
            VariantEdit::ReplaceBlock { from, to } => {
                replace_block(&mut v, from, to, &serde_json::Map::new());
            }
            VariantEdit::SetBpm(bpm) => set_bpm(&mut v, *bpm),
        }
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audiograph::count_nodes_with_id;

    fn source() -> Value {
        serde_json::json!({
            "info": { "displayName": "Twin Clean", "preset_id": "pid-src" },
            "bpm": 120.0,
            "audioGraph": { "guitarNodes": { "G1": [
                { "nodeId": "ACD_Twin", "dspUnitParameters": { "gain": 0.5 } },
                { "nodeId": "ACD_Comp", "dspUnitParameters": { "amount": 0.3 } }
            ] } }
        })
    }

    // AC — a recipe applies the expected deltas to a clone.
    #[test]
    fn recipe_applies_expected_deltas() {
        let recipe = Recipe {
            name_suffix: "(Bass)".into(),
            edits: vec![
                VariantEdit::SetParam {
                    model: "ACD_Twin".into(),
                    param: "gain".into(),
                    value: 0.2,
                },
                VariantEdit::ReplaceBlock {
                    from: "ACD_Comp".into(),
                    to: "ACD_BassComp".into(),
                },
                VariantEdit::SetBpm(90.0),
            ],
        };
        let v = apply_recipe(&source(), &recipe).unwrap();
        assert_eq!(v["info"]["displayName"], "Twin Clean (Bass)");
        assert_eq!(
            v["info"]["preset_id"], "pid-src",
            "identity preserved on the clone JSON"
        );
        assert_eq!(
            v["audioGraph"]["guitarNodes"]["G1"][0]["dspUnitParameters"]["gain"],
            0.2
        );
        assert_eq!(count_nodes_with_id(&v, "ACD_BassComp"), 1);
        assert_eq!(count_nodes_with_id(&v, "ACD_Comp"), 0);
        assert_eq!(v["bpm"], 90.0);
        // Source is untouched (clone semantics).
        assert_eq!(
            source()["audioGraph"]["guitarNodes"]["G1"][0]["dspUnitParameters"]["gain"],
            0.5
        );
    }

    // AC — clone + recipe re-encodes losslessly through the codec.
    #[test]
    fn clone_and_reencode_roundtrips() {
        let xor = crate::backup::xor_jld;
        let recipe = Recipe {
            name_suffix: "(IEM)".into(),
            edits: vec![VariantEdit::SetBpm(100.0)],
        };
        let v = apply_recipe(&source(), &recipe).unwrap();
        let s = serde_json::to_string(&v).unwrap();
        let decoded_again = String::from_utf8(xor(&xor(s.as_bytes()))).unwrap();
        assert_eq!(decoded_again, s);
        assert_eq!(
            serde_json::from_str::<Value>(&decoded_again).unwrap()["info"]["displayName"],
            "Twin Clean (IEM)"
        );
    }
}
