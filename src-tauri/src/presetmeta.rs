//! OFFLINE edits to preset-level (non-`audioGraph`) fields: tempo (`bpm`) and on-load
//! MIDI messages (`onLoadMidiMsgs`), with the firmware per-preset MIDI message cap
//! enforced.
//!
//! Only fields whose home is the preset JSON top level; one value per run per field.
//! Per-block tempo *divisions* live inside a synced block's `dspUnitParameters` (e.g. a
//! delay's subdivision) and are reached through the generic block-parameter edit, not
//! here — keeping this module to genuine preset-home fields.

use serde_json::Value;

/// Set the preset tempo (`bpm`, a top-level float).
pub fn set_bpm(preset: &mut Value, bpm: f64) {
    if let Some(obj) = preset.as_object_mut() {
        obj.insert("bpm".into(), serde_json::json!(bpm));
    }
}

/// Replace the preset's on-load MIDI messages (`onLoadMidiMsgs`). Refuses to exceed
/// the firmware per-preset cap (`cap`), which the app supplies from the firmware
/// limits; tests pass a small cap.
pub fn set_onload_midi(preset: &mut Value, msgs: Vec<Value>, cap: usize) -> Result<(), String> {
    if msgs.len() > cap {
        return Err(format!(
            "{} MIDI messages exceeds the per-preset cap of {cap}",
            msgs.len()
        ));
    }
    if let Some(obj) = preset.as_object_mut() {
        obj.insert("onLoadMidiMsgs".into(), Value::Array(msgs));
    }
    Ok(())
}

/// Bulk-run operation: set the tempo across selected presets.
pub struct SetBpmOp {
    pub bpm: f64,
}

impl crate::bulkrun::Operation for SetBpmOp {
    fn label(&self) -> String {
        format!("set bpm = {:.2}", self.bpm)
    }
    fn transform(&self, t: &crate::bulkrun::PresetTarget) -> Result<Option<String>, String> {
        let mut v: Value =
            serde_json::from_str(&t.before_json).map_err(|e| format!("parse: {e}"))?;
        set_bpm(&mut v, self.bpm);
        Ok(Some(serde_json::to_string(&v).map_err(|e| e.to_string())?))
    }
}

/// Bulk-run operation: replace the on-load MIDI messages across selected presets,
/// enforcing the firmware per-preset cap (caller-supplied — not hardcoded, since the
/// limit is a firmware fact).
pub struct SetOnLoadMidiOp {
    pub msgs: Vec<Value>,
    pub cap: usize,
}

impl crate::bulkrun::Operation for SetOnLoadMidiOp {
    fn label(&self) -> String {
        format!("set {} on-load MIDI msg(s)", self.msgs.len())
    }
    fn transform(&self, t: &crate::bulkrun::PresetTarget) -> Result<Option<String>, String> {
        let mut v: Value =
            serde_json::from_str(&t.before_json).map_err(|e| format!("parse: {e}"))?;
        set_onload_midi(&mut v, self.msgs.clone(), self.cap)?;
        Ok(Some(serde_json::to_string(&v).map_err(|e| e.to_string())?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // AC1 — set bpm + MIDI messages on a fixture; cap enforced.
    #[test]
    fn set_bpm_divisions_midi() {
        let mut p = serde_json::json!({ "bpm": 120.0, "onLoadMidiMsgs": Value::Null });
        set_bpm(&mut p, 90.0);
        assert_eq!(p["bpm"], 90.0);

        // Under the cap → set.
        let msgs = vec![serde_json::json!({ "cc": 7, "val": 100 })];
        assert!(set_onload_midi(&mut p, msgs, 8).is_ok());
        assert_eq!(p["onLoadMidiMsgs"].as_array().unwrap().len(), 1);
    }

    // AC (cap) — exceeding the per-preset MIDI cap errors and does not mutate.
    #[test]
    fn respects_midi_message_cap() {
        let mut p = serde_json::json!({ "onLoadMidiMsgs": Value::Null });
        let msgs: Vec<Value> = (0..5).map(|i| serde_json::json!({ "cc": i })).collect();
        let err = set_onload_midi(&mut p, msgs, 3).unwrap_err();
        assert!(err.contains("cap"), "got: {err}");
        assert_eq!(
            p["onLoadMidiMsgs"],
            Value::Null,
            "no mutation on cap violation"
        );
    }

    // AC — the on-load-MIDI bulk op transforms a target and enforces the cap.
    #[test]
    fn onload_midi_op_transforms_and_caps() {
        use crate::bulkrun::{Operation, PresetTarget};
        let target = PresetTarget {
            list_index: 0,
            list_enum: 1,
            display_name: "P0".to_string(),
            source: "offline-file".to_string(),
            before_json: r#"{"bpm":120.0,"onLoadMidiMsgs":null}"#.to_string(),
        };
        let op = SetOnLoadMidiOp {
            msgs: vec![serde_json::json!({ "cc": 7, "val": 64 })],
            cap: 8,
        };
        let out = op.transform(&target).unwrap().unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["onLoadMidiMsgs"].as_array().unwrap().len(), 1);

        let too_many = SetOnLoadMidiOp {
            msgs: (0..4).map(|i| serde_json::json!({ "cc": i })).collect(),
            cap: 2,
        };
        assert!(too_many.transform(&target).is_err(), "over-cap is rejected");
    }

    // AC2 — setting bpm on the fixture re-encodes losslessly (the OFFLINE .preset round-trip).
    #[test]
    fn reencode_roundtrips() {
        let xor = crate::backup::xor_jld;
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/Guitar.preset");
        let Ok(file) = std::fs::read(&path) else {
            eprintln!("skip: fixture absent");
            return;
        };
        let mut v: Value = serde_json::from_str(&String::from_utf8(xor(&file)).unwrap()).unwrap();
        set_bpm(&mut v, 100.0);
        let mutated = serde_json::to_string(&v).unwrap();
        let decoded_again = String::from_utf8(xor(&xor(mutated.as_bytes()))).unwrap();
        assert_eq!(decoded_again, mutated);
        let reparsed: Value = serde_json::from_str(&decoded_again).unwrap();
        assert_eq!(reparsed["bpm"], 100.0);
    }
}
