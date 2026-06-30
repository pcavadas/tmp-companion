//! IR management: bulk relink (replace one IR file with another, preserving cuts +
//! level) and assign a Speaker Impedance Curve (SIC).
//!
//! OFFLINE: an IR reference lives in a block's `dspUnitParameters` — `file` (the IR
//! filename) + `sicid` (the SIC) + `hpf`/`lpf` (cuts) + `outputlevel`. The user IR
//! block is `ACD_UserIRTMS`; we identify an IR node by the presence of a `file`
//! param so the matcher is robust to id variants.
//!
//! Relink preserves cuts + level by touching ONLY `file`. The set of "available" IR
//! files is supplied by the caller (the app knows the device's IR slots; tests pass
//! one).

use std::collections::HashSet;

use serde_json::Value;

use crate::audiograph::for_each_node_mut;

/// True if a node carries an IR reference (has a `file` string param).
fn ir_file_of(node: &serde_json::Map<String, Value>) -> Option<String> {
    node.get("dspUnitParameters")
        .and_then(Value::as_object)
        .and_then(|p| p.get("file"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Relink IR references `from` → `to` in one preset, preserving every other IR
/// param (cuts `hpf`/`lpf`, `outputlevel`, `sicid`). Returns the number relinked.
pub fn relink_ir(preset: &mut Value, from: &str, to: &str) -> usize {
    for_each_node_mut(preset, |node| {
        let is_target = ir_file_of(node).as_deref() == Some(from);
        if !is_target {
            return false;
        }
        if let Some(p) = node
            .get_mut("dspUnitParameters")
            .and_then(Value::as_object_mut)
        {
            p.insert("file".into(), Value::String(to.to_string()));
        }
        true
    })
}

/// Assign `sicid` to every IR node (those with a `file` param). If `only_files` is
/// `Some`, restrict to IR nodes referencing one of those files. Returns the count.
pub fn set_sic(preset: &mut Value, sicid: &str, only_files: Option<&HashSet<String>>) -> usize {
    for_each_node_mut(preset, |node| {
        let Some(file) = ir_file_of(node) else {
            return false;
        };
        if let Some(set) = only_files {
            if !set.contains(&file) {
                return false;
            }
        }
        if let Some(p) = node
            .get_mut("dspUnitParameters")
            .and_then(Value::as_object_mut)
        {
            p.insert("sicid".into(), Value::String(sicid.to_string()));
        }
        true
    })
}

/// Bulk-run operation: relink IR `from` → `to`. Skips presets without `from`.
pub struct RelinkIrOp {
    pub from: String,
    pub to: String,
}
impl crate::bulkrun::Operation for RelinkIrOp {
    fn label(&self) -> String {
        format!("relink IR {} → {}", self.from, self.to)
    }
    fn transform(&self, t: &crate::bulkrun::PresetTarget) -> Result<Option<String>, String> {
        let mut v: Value =
            serde_json::from_str(&t.before_json).map_err(|e| format!("parse: {e}"))?;
        if relink_ir(&mut v, &self.from, &self.to) == 0 {
            return Ok(None);
        }
        Ok(Some(serde_json::to_string(&v).map_err(|e| e.to_string())?))
    }
}

/// Bulk-run operation: assign a SIC to IR blocks (optionally only those
/// referencing `only_files`).
pub struct SetSicOp {
    pub sicid: String,
    pub only_files: Option<HashSet<String>>,
}
impl crate::bulkrun::Operation for SetSicOp {
    fn label(&self) -> String {
        format!("set SIC = {}", self.sicid)
    }
    fn transform(&self, t: &crate::bulkrun::PresetTarget) -> Result<Option<String>, String> {
        let mut v: Value =
            serde_json::from_str(&t.before_json).map_err(|e| format!("parse: {e}"))?;
        if set_sic(&mut v, &self.sicid, self.only_files.as_ref()) == 0 {
            return Ok(None);
        }
        Ok(Some(serde_json::to_string(&v).map_err(|e| e.to_string())?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ir_preset(file: &str, hpf: f64, lpf: f64, level: f64) -> Value {
        serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "nodeId": "ACD_UserIRTMS", "dspUnitParameters": {
                    "file": file, "sicid": "OldSic", "hpf": hpf, "lpf": lpf, "outputlevel": level, "bypass": false
                } }
            ] } }
        })
    }
    fn set(list: &[&str]) -> HashSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    // AC2 — relink replaces the file, preserving cuts + level.
    #[test]
    fn relink_preserves_cuts_and_level() {
        let mut p = ir_preset("Foo.wav", 80.0, 12000.0, -16.0);
        let n = relink_ir(&mut p, "Foo.wav", "Baz.wav");
        assert_eq!(n, 1);
        let params = &p["audioGraph"]["guitarNodes"]["G1"][0]["dspUnitParameters"];
        assert_eq!(params["file"], "Baz.wav");
        assert_eq!(params["hpf"], 80.0, "low-cut preserved");
        assert_eq!(params["lpf"], 12000.0, "high-cut preserved");
        assert_eq!(params["outputlevel"], -16.0, "level preserved");
        // A preset without the from-file is left alone.
        let mut other = ir_preset("Other.wav", 0.0, 1.0, 0.0);
        assert_eq!(relink_ir(&mut other, "Foo.wav", "Baz.wav"), 0);
    }

    // AC4 — SIC assignment writes the sicid field (optionally file-scoped).
    #[test]
    fn set_sic_writes_field() {
        let mut p = ir_preset("Foo.wav", 0.0, 1.0, 0.0);
        assert_eq!(set_sic(&mut p, "Marshall4x12_closed", None), 1);
        assert_eq!(
            p["audioGraph"]["guitarNodes"]["G1"][0]["dspUnitParameters"]["sicid"],
            "Marshall4x12_closed"
        );
        // file-scoped: a non-matching file is skipped.
        let mut p2 = ir_preset("Foo.wav", 0.0, 1.0, 0.0);
        assert_eq!(set_sic(&mut p2, "X", Some(&set(&["Other.wav"]))), 0);
    }
}
