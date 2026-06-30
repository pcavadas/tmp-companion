//! Bulk-operation command plumbing (foundation): the IPC-facing [`OpSpec`], the
//! [`build_operation`] factory that turns it into a concrete [`crate::bulkrun::Operation`],
//! the LIVE/OFFLINE [`IoPath`] routing, and the in-memory [`RunRegistry`] that lets
//! `bulk_revert` find a prior run's report.
//!
//! The thin `#[tauri::command]` wrappers (`bulk_dry_run`/`bulk_apply`/`bulk_revert`)
//! live in `lib.rs` with `AppState` + `with_released_seize`; this module is the pure,
//! unit-tested core they call.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

use crate::audiograph::BulkBypassOp;
use crate::blocklib::{ApplyBlockOp, BlockTemplate};
use crate::bulkrun::{Operation, RunReport};
use crate::footswitch::FootswitchLayoutOp;
use crate::ir::{RelinkIrOp, SetSicOp};
use crate::paramedit::{ParamEditOp, ParamMode};
use crate::presetmeta::{SetBpmOp, SetOnLoadMidiOp};

/// Which `PresetIo` adapter a run uses. ParamEdit is identity-preserving via the live
/// `changeParameter` setter; everything else edits full structure and re-imports OFFLINE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoPath {
    Live,
    Offline,
}

/// Serde-friendly mirror of [`ParamMode`] (the engine enum carries an `f64` payload
/// but no serde derive). Wire form: `{ "mode": "offset", "value": -0.1 }`.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(tag = "mode", content = "value", rename_all = "lowercase")]
pub enum ParamModeSpec {
    Set(f64),
    Offset(f64),
    Scale(f64),
}
impl From<ParamModeSpec> for ParamMode {
    fn from(s: ParamModeSpec) -> Self {
        match s {
            ParamModeSpec::Set(v) => ParamMode::Set(v),
            ParamModeSpec::Offset(v) => ParamMode::Offset(v),
            ParamModeSpec::Scale(v) => ParamMode::Scale(v),
        }
    }
}

/// The bulk operation the UI requests, one variant per wired engine. Internally
/// tagged (`{ "type": "ParamEdit", ... }`) so the TS layer sends a discriminated union.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum OpSpec {
    /// Set/offset/scale a block parameter (LIVE).
    ParamEdit {
        model: String,
        param: String,
        mode: ParamModeSpec,
        min: f64,
        max: f64,
    },
    /// Set bypass on/off for a set of block ids (OFFLINE).
    BulkBypass { ids: Vec<String>, bypass: bool },
    /// Relink an IR file reference (OFFLINE).
    RelinkIr { from: String, to: String },
    /// Set the Speaker Impedance Curve id (OFFLINE).
    SetSic {
        sicid: String,
        #[serde(default)]
        only_files: Option<Vec<String>>,
    },
    /// Set the preset BPM (OFFLINE).
    SetBpm { bpm: f64 },
    /// Replace the on-load MIDI messages, enforcing the firmware cap (OFFLINE).
    SetOnLoadMidi { msgs: Vec<Value>, cap: usize },
    /// Copy a saved block's params into matching-model blocks (OFFLINE).
    ApplyBlock { template: BlockTemplate },
    /// Overwrite the footswitch / EXP layout (OFFLINE).
    FootswitchLayout {
        ftsw: Value,
        #[serde(default)]
        exp: Option<Value>,
    },
}

impl OpSpec {
    /// The persistence path this op routes through.
    pub fn io_path(&self) -> IoPath {
        match self {
            // The lone LIVE op: a single block param via changeParameter (identity-safe).
            OpSpec::ParamEdit { .. } => IoPath::Live,
            // Everything else edits full structure / non-param fields → OFFLINE re-import.
            _ => IoPath::Offline,
        }
    }
}

/// Build the concrete engine [`Operation`] from a spec. Infallible today (every
/// variant maps directly to an existing op), but returns `Result` so future
/// validation (e.g. unknown model id) has a home.
pub fn build_operation(spec: &OpSpec) -> Result<Box<dyn Operation>, String> {
    Ok(match spec {
        OpSpec::ParamEdit {
            model,
            param,
            mode,
            min,
            max,
        } => Box::new(ParamEditOp {
            model: model.clone(),
            param: param.clone(),
            mode: (*mode).into(),
            min: *min,
            max: *max,
        }),
        OpSpec::BulkBypass { ids, bypass } => Box::new(BulkBypassOp {
            ids: ids.iter().cloned().collect(),
            bypass: *bypass,
        }),
        OpSpec::RelinkIr { from, to } => Box::new(RelinkIrOp {
            from: from.clone(),
            to: to.clone(),
        }),
        OpSpec::SetSic { sicid, only_files } => Box::new(SetSicOp {
            sicid: sicid.clone(),
            only_files: only_files.as_ref().map(|v| v.iter().cloned().collect()),
        }),
        OpSpec::SetBpm { bpm } => Box::new(SetBpmOp { bpm: *bpm }),
        OpSpec::SetOnLoadMidi { msgs, cap } => Box::new(SetOnLoadMidiOp {
            msgs: msgs.clone(),
            cap: *cap,
        }),
        OpSpec::ApplyBlock { template } => Box::new(ApplyBlockOp {
            template: template.clone(),
        }),
        OpSpec::FootswitchLayout { ftsw, exp } => Box::new(FootswitchLayoutOp {
            ftsw: ftsw.clone(),
            exp: exp.clone(),
        }),
    })
}

// ─── run registry ──────────────────────────────────────────────────────────────

/// A completed run kept so `bulk_revert` can restore it. Records the io path so
/// revert rebuilds the same adapter.
#[derive(Debug, Clone)]
pub struct StoredRun {
    pub report: RunReport,
    /// Human label of the op that ran — surfaced in the run-history UI (Phase 1).
    #[allow(dead_code)]
    pub op_label: String,
}

/// In-memory `run_id → StoredRun`. Lives in `AppState`. (Snapshots also persist on
/// disk under `backups/`, so a disk-backed revert across app restarts is a possible
/// follow-up; the registry covers the within-session case.)
#[derive(Debug, Default)]
pub struct RunRegistry {
    runs: HashMap<String, StoredRun>,
}

impl RunRegistry {
    pub fn insert(&mut self, run_id: String, run: StoredRun) {
        self.runs.insert(run_id, run);
    }
    pub fn get(&self, run_id: &str) -> Option<&StoredRun> {
        self.runs.get(run_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from_json(s: &str) -> OpSpec {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn paramedit_spec_deserializes_and_routes_live() {
        let spec = from_json(
            r#"{"type":"ParamEdit","model":"ACD_Amp","param":"gain","mode":{"mode":"offset","value":-0.1},"min":0.0,"max":1.0}"#,
        );
        assert_eq!(spec.io_path(), IoPath::Live);
        let op = build_operation(&spec).unwrap();
        assert!(op.label().to_lowercase().contains("gain"));
    }

    #[test]
    fn bulk_bypass_and_setbpm_build() {
        let bypass = from_json(r#"{"type":"BulkBypass","ids":["n1","n2"],"bypass":true}"#);
        assert_eq!(bypass.io_path(), IoPath::Offline);
        build_operation(&bypass).unwrap();
        let bpm = from_json(r#"{"type":"SetBpm","bpm":120.0}"#);
        let op = build_operation(&bpm).unwrap();
        assert!(op.label().contains("120"));
    }

    #[test]
    fn relink_ir_and_setsic_build() {
        build_operation(&from_json(
            r#"{"type":"RelinkIr","from":"old.wav","to":"new.wav"}"#,
        ))
        .unwrap();
        build_operation(&from_json(r#"{"type":"SetSic","sicid":"sic-1"}"#)).unwrap();
    }

    #[test]
    fn registry_round_trips() {
        let mut reg = RunRegistry::default();
        reg.insert(
            "run-1".into(),
            StoredRun {
                report: RunReport::default(),
                op_label: "x".into(),
            },
        );
        assert!(reg.get("run-1").is_some());
        assert!(reg.get("nope").is_none());
    }
}
