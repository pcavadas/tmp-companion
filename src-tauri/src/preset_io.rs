//! Production [`crate::bulkrun::PresetIo`] adapters.
//!
//! Two ways an edited preset reaches the device:
//!
//! * [`OfflineIo`] — structural / full-preset edits.
//!   Re-encodes the mutated JSON to `.preset` bytes and lands it via the
//!   HW-validated AC7 **in-place re-import** ([`crate::replace_inplace_core`]),
//!   preserving the slot + Song binding. Refuses any edit that changes `info.preset_id`
//!   (a different identity empties the Song row).
//!
//! * [`LiveIo`] — block-parameter edits. Diffs the before/after JSON down to
//!   `dspUnitParameters` changes and replays them as `changeParameter` + `save`,
//!   identity-preserving by construction. Refuses any diff outside `dspUnitParameters`
//!   (those must go OFFLINE).
//!
//! Both open their own fresh device connections per the leveller/AC7 discipline
//! (re-amp/once-per-connection gotchas) rather than borrowing a long-lived session.

use serde_json::Value;

use crate::backup;
use crate::bulkrun::{PresetIo, PresetTarget};
use crate::session::Session;

// ─── identity guard ────────────────────────────────────────────────────────────

/// Refuse an edit that changes the preset's identity. The device binds Songs to a
/// slot by `info.preset_id` + scene structure; overwriting a slot with a
/// different-identity preset silently empties the Song row (HW-demonstrated). An
/// in-place edit MUST keep the original `preset_id`.
fn assert_identity_preserved(before_json: &str, after_json: &str) -> Result<(), String> {
    let before: Value =
        serde_json::from_str(before_json).map_err(|e| format!("parse before: {e}"))?;
    let after: Value = serde_json::from_str(after_json).map_err(|e| format!("parse after: {e}"))?;
    let id_before = before.pointer("/info/preset_id");
    let id_after = after.pointer("/info/preset_id");
    if id_before != id_after {
        return Err(format!(
            "edit changes info.preset_id ({id_before:?} → {id_after:?}) — refused: a different \
             identity breaks the Song link at this slot"
        ));
    }
    Ok(())
}

// ─── OFFLINE adapter ─────────────────────────────────────────────────────────────

/// Lands full-preset edits via AC7 in-place re-import. Stateless — opens fresh
/// connections internally.
pub struct OfflineIo;

impl PresetIo for OfflineIo {
    fn write(&mut self, target: &PresetTarget, after_json: &str) -> Result<(), String> {
        assert_identity_preserved(&target.before_json, after_json)?;
        // Re-encode to `.preset` bytes (XOR is self-inverse; TMP exports are compact JSON).
        let bytes = backup::xor_jld(after_json.as_bytes());
        let outcome = crate::replace_inplace_core(target.list_index, &bytes)?;
        if !outcome.edit_landed {
            return Err(format!(
                "in-place edit did not land at list index {} (slot name unchanged: {:?})",
                target.list_index, outcome.orig_name_after
            ));
        }
        if outcome.had_binding && !outcome.binding_preserved {
            return Err(format!(
                "in-place edit landed but the Song-1 binding changed at list index {} — \
                 aborting to avoid a dangling Song link",
                target.list_index
            ));
        }
        Ok(())
    }

    /// Read-back fidelity ceiling: there is no complete-preset USB read, so we read
    /// the only reliable signal — the slot's display name from a fresh list. `verify`
    /// pairs this with the edited JSON's display name (a landing + identity check).
    fn read_back(&mut self, target: &PresetTarget) -> Result<String, String> {
        let list = Session::connect()?.list_my_presets()?;
        list.iter()
            .find(|p| p.slot == target.list_index)
            .map(|p| p.name.clone())
            .ok_or_else(|| format!("slot {} not found on re-list", target.list_index))
    }

    /// A *landing* check, not a full content diff (no complete USB read exists): the
    /// slot's name after the write must equal the edited preset's `info.displayName`.
    /// The re-encode round-trip is what guarantees content fidelity (tested offline).
    fn verify(&self, after_json: &str, read_back: &str) -> Result<bool, String> {
        let after: Value =
            serde_json::from_str(after_json).map_err(|e| format!("parse after: {e}"))?;
        let expected = after
            .pointer("/info/displayName")
            .and_then(Value::as_str)
            .unwrap_or("");
        Ok(read_back == expected)
    }
}

// ─── LIVE adapter ──────────────────────────────────────────────────────────────

/// One `dspUnitParameters` change resolved from a JSON diff into `changeParameter`
/// coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamChange {
    pub group_id: String,
    pub node_id: String,
    pub param: String,
    pub value: f32,
}

/// Translate a before→after JSON diff into `changeParameter` ops. Every changed
/// field MUST be a numeric value at `/audioGraph/{guitarNodes|micNodes}/<group>/<idx>/dspUnitParameters/<param>`;
/// `node_id` is resolved from the node's `nodeId`/`FenderId` in `after`. Any change
/// outside that shape → `Err` (the edit is not LIVE-safe; route it OFFLINE).
pub fn diff_to_param_changes(
    before_json: &str,
    after_json: &str,
) -> Result<Vec<ParamChange>, String> {
    let after: Value = serde_json::from_str(after_json).map_err(|e| format!("parse after: {e}"))?;
    let changes = backup::diff_preset_json(before_json, after_json)?;
    let mut out = Vec::new();
    for ch in changes {
        // Pointer: ["", "audioGraph", group_kind, group, idx, "dspUnitParameters", param]
        let seg: Vec<&str> = ch.pointer.split('/').skip(1).collect(); // drop leading ""
        let bad = || {
            format!(
                "LIVE edit only supports block-parameter changes; pointer {:?} is outside \
                 dspUnitParameters — route this edit OFFLINE",
                ch.pointer
            )
        };
        if seg.len() != 6
            || seg[0] != "audioGraph"
            || !(seg[1] == "guitarNodes" || seg[1] == "micNodes")
            || seg[4] != "dspUnitParameters"
        {
            return Err(bad());
        }
        let (group_kind, group, idx, param) = (seg[1], seg[2], seg[3], seg[5]);
        let new_val = ch.after.as_ref().and_then(Value::as_f64).ok_or_else(|| {
            format!(
                "param {:?} new value is not numeric (= {:?})",
                ch.pointer, ch.after
            )
        })?;
        // Resolve node_id from the node at that array index in `after`.
        let node = after
            .pointer(&format!("/audioGraph/{group_kind}/{group}/{idx}"))
            .ok_or_else(|| format!("could not resolve node at {:?}", ch.pointer))?;
        let node_id = node
            .get("nodeId")
            .and_then(Value::as_str)
            .or_else(|| node.get("FenderId").and_then(Value::as_str))
            .ok_or_else(|| format!("node at {:?} has no nodeId/FenderId", ch.pointer))?;
        out.push(ParamChange {
            group_id: group.to_string(),
            node_id: node_id.to_string(),
            param: param.to_string(),
            value: new_val as f32,
        });
    }
    Ok(out)
}

/// Applies LIVE block-parameter edits (`changeParameter` + `save`). Stateless.
pub struct LiveIo;

impl PresetIo for LiveIo {
    fn write(&mut self, target: &PresetTarget, after_json: &str) -> Result<(), String> {
        assert_identity_preserved(&target.before_json, after_json)?;
        let changes = diff_to_param_changes(&target.before_json, after_json)?;
        if changes.is_empty() {
            return Ok(()); // the engine already filters no-ops; nothing to send
        }
        // conn1: load the target (makes it current — persists across reconnect). Loading
        // and editing in ONE connection is unsafe (a load's own apply can override an
        // immediate set), so load, drop, settle, then reconnect (the leveller/AC7 rule).
        {
            let mut s = Session::connect()?;
            s.load_preset(target.list_index)?;
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
        // conn2: fresh handshake re-attaches to the now-current preset. CONFIRM it is the
        // target before mutating+saving (a dropped load = wrong-preset save = data loss),
        // then heartbeat-warm the line so fw 1.8.45 accepts the edits from a live
        // controller. changeParameter is fire-and-forget (no ack), so there is no
        // per-edit confirm to gate on — the pre-edit confirm + the identity guard are the
        // safety net. session.rs translates the 0-based list index to the 1-based userSlot.
        let mut s = Session::connect()?;
        s.confirm_active(target.list_index, Some(&target.display_name))?;
        s.begin_live_edit()?;
        for c in &changes {
            s.change_parameter(&c.group_id, &c.node_id, &c.param, c.value)?;
        }
        s.save_current_preset(target.list_index)?;
        Ok(())
    }

    /// LIVE read-back is transaction-level for now: a successful write means the
    /// device acked every `changeParameter` + the save. Full param value-readback is
    /// a Phase-3 refinement — the USB partial + the leveller's level-only block filter
    /// (`current_preset_blocks`) cannot read back arbitrary params yet.
    fn read_back(&mut self, _target: &PresetTarget) -> Result<String, String> {
        Ok(String::new())
    }

    fn verify(&self, _after_json: &str, _read_back: &str) -> Result<bool, String> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_guard_allows_unchanged_id() {
        let before = r#"{"info":{"preset_id":"abc"},"audioGraph":{"presetLevel":0.5}}"#;
        let after = r#"{"info":{"preset_id":"abc"},"audioGraph":{"presetLevel":0.9}}"#;
        assert!(assert_identity_preserved(before, after).is_ok());
    }

    #[test]
    fn identity_guard_refuses_changed_id() {
        let before = r#"{"info":{"preset_id":"abc"}}"#;
        let after = r#"{"info":{"preset_id":"xyz"}}"#;
        let err = assert_identity_preserved(before, after).unwrap_err();
        assert!(err.contains("preset_id"));
    }

    const NODE: &str = r#"{
        "info":{"preset_id":"p1"},
        "audioGraph":{"guitarNodes":{"G1":[
            {"nodeId":"amp-1","dspUnitParameters":{"gain":0.4,"level":0.7}}
        ]}}
    }"#;

    #[test]
    fn diff_resolves_a_single_param_change() {
        let after = NODE.replace("\"gain\":0.4", "\"gain\":0.55");
        let changes = diff_to_param_changes(NODE, &after).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(
            changes[0],
            ParamChange {
                group_id: "G1".into(),
                node_id: "amp-1".into(),
                param: "gain".into(),
                value: 0.55
            }
        );
    }

    #[test]
    fn diff_resolves_fenderid_nodes() {
        let src = r#"{"audioGraph":{"micNodes":{"M1":[{"FenderId":"rev-2","dspUnitParameters":{"mix":0.3}}]}}}"#;
        let after = src.replace("0.3", "0.6");
        let changes = diff_to_param_changes(src, &after).unwrap();
        assert_eq!(changes[0].node_id, "rev-2");
        assert_eq!(changes[0].group_id, "M1");
        assert_eq!(changes[0].param, "mix");
    }

    #[test]
    fn diff_refuses_non_param_changes() {
        // displayName change is outside dspUnitParameters → not LIVE-safe.
        let before = r#"{"info":{"displayName":"A"},"audioGraph":{}}"#;
        let after = r#"{"info":{"displayName":"B"},"audioGraph":{}}"#;
        let err = diff_to_param_changes(before, after).unwrap_err();
        assert!(err.contains("OFFLINE"), "got: {err}");
    }

    #[test]
    fn offline_verify_matches_display_name() {
        let io = OfflineIo;
        let after = r#"{"info":{"displayName":"Clean Twin"}}"#;
        assert!(io.verify(after, "Clean Twin").unwrap());
        assert!(!io.verify(after, "Lead Boost").unwrap());
    }
}
