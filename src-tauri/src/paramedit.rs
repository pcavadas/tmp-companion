//! Bulk parameter edit: set / offset / scale one block parameter across presets,
//! with range clamping + out-of-range flagging.
//!
//! The target-value math + skip/clamp logic live here (the testable core); the LIVE
//! apply is `proto::change_parameter` (golden-tested as
//! `change_parameter_structure_roundtrips`). The `transform` here edits the param in
//! the preset JSON, which a LIVE io translates to a `change_parameter` send. Base
//! scene only; same-model targeting; presets without the block/param are skipped.

use serde_json::Value;

use crate::audiograph::for_each_node_mut;

/// How to compute the new parameter value from the old.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamMode {
    /// Set to an absolute value.
    Set(f64),
    /// Add a delta (may be negative).
    Offset(f64),
    /// Multiply by a factor.
    Scale(f64),
}

/// Compute the target value for `old` under `mode`, clamped to `[min, max]`. Returns
/// `(value, clamped)` where `clamped` is true if the raw result fell outside range.
pub fn compute_target(old: f64, mode: ParamMode, min: f64, max: f64) -> (f64, bool) {
    let raw = match mode {
        ParamMode::Set(v) => v,
        ParamMode::Offset(d) => old + d,
        ParamMode::Scale(k) => old * k,
    };
    let clamped = raw.clamp(min, max);
    (clamped, (clamped - raw).abs() > f64::EPSILON)
}

/// Result of editing one preset.
#[derive(Debug, Clone, PartialEq)]
pub struct EditOutcome {
    /// Blocks whose param was edited.
    pub edited: usize,
    /// True if any edit was clamped to the parameter's range.
    pub clamped: bool,
}

/// Edit parameter `param` of every block of model `model` in `preset` under `mode`,
/// clamping to `[min, max]`. Only nodes that actually have a numeric `param` are
/// touched. Returns the outcome (0 edits = skip).
pub fn edit_param(
    preset: &mut Value,
    model: &str,
    param: &str,
    mode: ParamMode,
    min: f64,
    max: f64,
) -> EditOutcome {
    let mut clamped_any = false;
    let edited = for_each_node_mut(preset, |node| {
        let id = node
            .get("nodeId")
            .and_then(Value::as_str)
            .or_else(|| node.get("FenderId").and_then(Value::as_str));
        if id != Some(model) {
            return false;
        }
        let Some(params) = node
            .get_mut("dspUnitParameters")
            .and_then(Value::as_object_mut)
        else {
            return false;
        };
        let Some(old) = params.get(param).and_then(Value::as_f64) else {
            return false; // block lacks this (numeric) param → skip
        };
        let (new, clamped) = compute_target(old, mode, min, max);
        clamped_any |= clamped;
        params.insert(param.to_string(), serde_json::json!(new));
        true
    });
    EditOutcome {
        edited,
        clamped: clamped_any,
    }
}

/// Bulk-run operation: edit one block param across a preset. Skips presets
/// without the target block/param.
pub struct ParamEditOp {
    pub model: String,
    pub param: String,
    pub mode: ParamMode,
    pub min: f64,
    pub max: f64,
}
impl crate::bulkrun::Operation for ParamEditOp {
    fn label(&self) -> String {
        format!("edit {}.{} ({:?})", self.model, self.param, self.mode)
    }
    fn transform(&self, t: &crate::bulkrun::PresetTarget) -> Result<Option<String>, String> {
        let mut v: Value =
            serde_json::from_str(&t.before_json).map_err(|e| format!("parse: {e}"))?;
        if edit_param(
            &mut v,
            &self.model,
            &self.param,
            self.mode,
            self.min,
            self.max,
        )
        .edited
            == 0
        {
            return Ok(None);
        }
        Ok(Some(serde_json::to_string(&v).map_err(|e| e.to_string())?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    // AC1 — set / offset / scale compute the expected target.
    #[test]
    fn compute_target_set_offset_scale() {
        let cases = [
            (0.6, ParamMode::Set(0.5), 0.5, false),
            (0.6, ParamMode::Offset(0.3), 0.9, false),
            (0.6, ParamMode::Offset(-0.2), 0.4, false),
            (0.8, ParamMode::Scale(0.5), 0.4, false),
        ];
        for (old, mode, want, want_clamped) in cases {
            let (got, clamped) = compute_target(old, mode, 0.0, 1.0);
            assert!(
                approx(got, want),
                "old={old} mode={mode:?} got={got} want={want}"
            );
            assert_eq!(clamped, want_clamped);
        }
    }

    // AC (clamp) — out-of-range results clamp and flag.
    #[test]
    fn clamp_out_of_range_and_flag() {
        let (v, clamped) = compute_target(0.8, ParamMode::Offset(0.5), 0.0, 1.0);
        assert!(approx(v, 1.0) && clamped, "clamped high");
        let (v2, c2) = compute_target(0.2, ParamMode::Offset(-0.5), 0.0, 1.0);
        assert!(approx(v2, 0.0) && c2, "clamped low");
    }

    // AC — presets without the target block/param are skipped (edited 0 / op None).
    #[test]
    fn skips_presets_without_block() {
        use crate::bulkrun::{Operation, PresetTarget};
        let p = serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "nodeId": "ACD_Twin", "dspUnitParameters": { "outputLevel": 0.6 } }
            ] } }
        });
        // Wrong model → skip.
        let mut p2 = p.clone();
        assert_eq!(
            edit_param(
                &mut p2,
                "ACD_Other",
                "outputLevel",
                ParamMode::Set(0.5),
                0.0,
                1.0
            )
            .edited,
            0
        );
        // Right model, missing param → skip.
        let mut p3 = p.clone();
        assert_eq!(
            edit_param(
                &mut p3,
                "ACD_Twin",
                "noSuchParam",
                ParamMode::Set(0.5),
                0.0,
                1.0
            )
            .edited,
            0
        );
        // Op transform returns None for a non-target preset.
        let op = ParamEditOp {
            model: "ACD_Other".into(),
            param: "outputLevel".into(),
            mode: ParamMode::Set(0.5),
            min: 0.0,
            max: 1.0,
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

    // The edit lands on the matching block + flags clamping.
    #[test]
    fn edit_param_writes_matching_block() {
        let mut p = serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "nodeId": "ACD_Gate", "dspUnitParameters": { "threshold": 0.8 } },
                { "nodeId": "ACD_Twin", "dspUnitParameters": { "threshold": 0.5 } }
            ] } }
        });
        let out = edit_param(
            &mut p,
            "ACD_Gate",
            "threshold",
            ParamMode::Offset(0.5),
            0.0,
            1.0,
        );
        assert_eq!(out.edited, 1);
        assert!(out.clamped, "0.8+0.5 clamps to 1.0");
        let g1 = &p["audioGraph"]["guitarNodes"]["G1"];
        assert_eq!(g1[0]["dspUnitParameters"]["threshold"], 1.0);
        assert_eq!(
            g1[1]["dspUnitParameters"]["threshold"], 0.5,
            "other model untouched"
        );
    }
}
