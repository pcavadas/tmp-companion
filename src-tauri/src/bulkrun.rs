//! Bulk-run engine: selection · dry-run · safe apply · before/after report · revert.
//!
//! The shared harness every bulk feature plugs into. A feature supplies an
//! [`Operation`] (a pure preset-JSON transform) and a [`PresetIo`] (the LIVE/OFFLINE
//! persistence + read-back seam); this engine supplies selection iteration,
//! dry-run preview, snapshot-before-write ([`crate::backup`]), apply, verify,
//! a flat before/after report, and single-level revert.
//!
//! Path-agnostic by construction: the engine only ever sees preset JSON. LIVE vs
//! OFFLINE specifics live entirely in the `PresetIo` impl a feature provides.
//!
//! Exactly one `Operation` interface (features register statically); no parallelism
//! (the device is single-session → run sequentially); single-level revert via the
//! snapshot (no undo stack); a flat report (no diff viewer).

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::backup::{self, FieldChange, PresetSnapshot};

/// A preset selected for a run, carrying its captured "before" state. `before_json`
/// is the canonical full preset JSON (an OFFLINE `.preset` decode, or a USB partial);
/// `source` records its fidelity (`"offline-file"` | `"usb-partial"`) so the
/// snapshot never over-promises. `list_index` is the 0-based
/// "My Presets" index (`session` translates to the device's 1-based userSlot).
#[derive(Debug, Clone)]
pub struct PresetTarget {
    pub list_index: u32,
    pub list_enum: u32,
    pub display_name: String,
    pub source: String,
    pub before_json: String,
}

/// A bulk operation: a pure transform from a target's before-state to its
/// after-state. `Ok(None)` = this preset is not a target (skipped); `Ok(Some(json))`
/// = the edited JSON; `Err` = this preset errored (reported per-preset, never fatal
/// to the run). Path-agnostic — it does not know whether it will be persisted LIVE
/// or OFFLINE.
pub trait Operation {
    fn label(&self) -> String;
    fn transform(&self, target: &PresetTarget) -> Result<Option<String>, String>;
}

/// The persistence + read-back seam. A LIVE impl drives the device; an OFFLINE impl
/// re-encodes + re-imports; the in-memory mock backs the unit tests. The engine
/// owns snapshot-before-write and report assembly *around* these calls.
///
/// `verify` decides whether a write landed given the (possibly partial) read-back —
/// the io owns this because only it knows its read fidelity (a USB read-back is a
/// truncated partial, so a LIVE io compares just the field(s) it can actually read,
/// while the default compares the full JSON for OFFLINE round-trips + the mock).
pub trait PresetIo {
    fn write(&mut self, target: &PresetTarget, after_json: &str) -> Result<(), String>;
    fn read_back(&mut self, target: &PresetTarget) -> Result<String, String>;
    fn verify(&self, after_json: &str, read_back: &str) -> Result<bool, String> {
        Ok(backup::diff_preset_json(after_json, read_back)?.is_empty())
    }
}

// ─── dry-run ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeStatus {
    Changed,
    Unchanged,
    Skipped,
    Error,
}

/// What WOULD happen to one preset under an operation (dry-run preview).
#[derive(Debug, Clone, Serialize)]
pub struct DryRunEntry {
    pub list_index: u32,
    pub display_name: String,
    pub status: ChangeStatus,
    pub changes: Vec<FieldChange>,
    pub error: Option<String>,
}

/// Compute, per selected preset, what the operation WOULD change — writing nothing
/// (no `PresetIo` is even passed, so a write is structurally impossible). AC1.
pub fn dry_run(targets: &[PresetTarget], op: &dyn Operation) -> Vec<DryRunEntry> {
    targets
        .iter()
        .map(|t| {
            let base = |status, changes, error| DryRunEntry {
                list_index: t.list_index,
                display_name: t.display_name.clone(),
                status,
                changes,
                error,
            };
            match op.transform(t) {
                Ok(None) => base(ChangeStatus::Skipped, vec![], None),
                Err(e) => base(ChangeStatus::Error, vec![], Some(e)),
                Ok(Some(after)) => match backup::diff_preset_json(&t.before_json, &after) {
                    Ok(changes) if changes.is_empty() => {
                        base(ChangeStatus::Unchanged, vec![], None)
                    }
                    Ok(changes) => base(ChangeStatus::Changed, changes, None),
                    Err(e) => base(ChangeStatus::Error, vec![], Some(format!("diff: {e}"))),
                },
            }
        })
        .collect()
}

// ─── apply ───────────────────────────────────────────────────────────────────

/// The outcome of applying an operation to one preset.
#[derive(Debug, Clone, Serialize)]
pub struct RunEntry {
    pub list_index: u32,
    pub display_name: String,
    /// True iff a write was attempted AND succeeded (the slot's state changed).
    pub changed: bool,
    /// True iff a changed write read back as intended (per the io's `verify`), or the
    /// preset was a no-op/skip (nothing to verify).
    pub verified: bool,
    pub error: Option<String>,
    /// Set once the pre-write snapshot is on disk — present even if the write then
    /// fails, so the preset stays revertible (AC2).
    pub snapshot_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RunReport {
    pub entries: Vec<RunEntry>,
}

impl RunReport {
    pub fn changed(&self) -> usize {
        self.entries.iter().filter(|e| e.changed).count()
    }
    /// Presets that were changed AND verified (the success count the UI reports).
    pub fn verified(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.changed && e.verified)
            .count()
    }
    pub fn errors(&self) -> usize {
        self.entries.iter().filter(|e| e.error.is_some()).count()
    }
}

/// Apply `op` to every target sequentially, snapshotting each before its
/// write (AC2), verifying after (AC3), and collecting a per-preset result (AC3). A
/// mid-run write failure leaves the already-snapshotted presets revertible (AC2).
pub fn apply(
    targets: &[PresetTarget],
    op: &dyn Operation,
    io: &mut dyn PresetIo,
    backup_dir: &Path,
) -> RunReport {
    RunReport {
        entries: targets
            .iter()
            .map(|t| apply_one(t, op, io, backup_dir))
            .collect(),
    }
}

fn apply_one(
    t: &PresetTarget,
    op: &dyn Operation,
    io: &mut dyn PresetIo,
    backup_dir: &Path,
) -> RunEntry {
    let mut e = RunEntry {
        list_index: t.list_index,
        display_name: t.display_name.clone(),
        changed: false,
        verified: false,
        error: None,
        snapshot_path: None,
    };

    // 1) Compute the edit. None = not a target; Err = per-preset failure.
    let after = match op.transform(t) {
        Ok(None) => {
            e.verified = true; // nothing to do — vacuously fine
            return e;
        }
        Err(err) => {
            e.error = Some(err);
            return e;
        }
        Ok(Some(a)) => a,
    };

    // 2) No-op edits (after == before) need neither a snapshot nor a write.
    match backup::diff_preset_json(&t.before_json, &after) {
        Ok(d) if d.is_empty() => {
            e.verified = true;
            return e;
        }
        Err(err) => {
            e.error = Some(format!("diff: {err}"));
            return e;
        }
        Ok(_) => {}
    }

    // 3) AC2 — snapshot the pre-edit state BEFORE writing, so a write failure (or a
    // later preset's failure) still leaves this one revertible.
    let snap = PresetSnapshot {
        list_enum: t.list_enum,
        slot: t.list_index,
        display_name: t.display_name.clone(),
        captured_at: now_stamp(),
        source: t.source.clone(),
        preset_json: t.before_json.clone(),
    };
    match backup::save_snapshot_to_dir(backup_dir, &snap) {
        Ok(p) => e.snapshot_path = Some(p),
        Err(err) => {
            e.error = Some(format!("snapshot: {err}"));
            return e; // refuse to write without a backup
        }
    }

    // 4) Write.
    if let Err(err) = io.write(t, &after) {
        e.error = Some(format!("write: {err}"));
        return e; // snapshot already taken → revertible
    }
    e.changed = true;

    // 5) AC3 — verify by reading back + comparing (fidelity per the io).
    match io.read_back(t) {
        Ok(rb) => match io.verify(&after, &rb) {
            Ok(true) => e.verified = true,
            Ok(false) => {
                e.error = Some("verify mismatch: read-back differs from intended edit".into())
            }
            Err(err) => e.error = Some(format!("verify: {err}")),
        },
        Err(err) => e.error = Some(format!("read-back: {err}")),
    }
    e
}

// ─── revert ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct RevertEntry {
    pub list_index: u32,
    pub restored: bool,
    pub error: Option<String>,
}

/// Restore every preset the run touched (those with a snapshot) to its pre-run
/// state (AC4). Loads each snapshot and writes the captured JSON back through `io`.
/// Presets that were skipped/no-op (no snapshot) are left untouched.
pub fn revert(report: &RunReport, io: &mut dyn PresetIo) -> Vec<RevertEntry> {
    let mut out = Vec::new();
    for entry in &report.entries {
        let Some(path) = &entry.snapshot_path else {
            continue;
        };
        let mut r = RevertEntry {
            list_index: entry.list_index,
            restored: false,
            error: None,
        };
        match backup::load_snapshot_from_path(path) {
            Ok(snap) => {
                let target = PresetTarget {
                    list_index: snap.slot,
                    list_enum: snap.list_enum,
                    display_name: snap.display_name.clone(),
                    source: snap.source.clone(),
                    before_json: snap.preset_json.clone(),
                };
                match io.write(&target, &snap.preset_json) {
                    Ok(()) => r.restored = true,
                    Err(e) => r.error = Some(e),
                }
            }
            Err(e) => r.error = Some(e),
        }
        out.push(r);
    }
    out
}

/// Zero-padded epoch-millis stamp for the snapshot filename. Opaque/non-load-bearing
/// (the snapshot contract only uses it for ordering/naming), and zero-padding keeps
/// the lexical sort in `backup::list_snapshots_in_dir` chronological.
fn now_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{ms:020}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// In-memory "device": `list_index → current JSON`. `write` replaces, `read_back`
    /// returns. `fail_write_on` simulates a device write failure on one slot.
    struct MockIo {
        store: HashMap<u32, String>,
        writes: usize,
        fail_write_on: Option<u32>,
    }
    impl MockIo {
        fn new(initial: &[(u32, &str)]) -> Self {
            MockIo {
                store: initial.iter().map(|(k, v)| (*k, v.to_string())).collect(),
                writes: 0,
                fail_write_on: None,
            }
        }
    }
    impl PresetIo for MockIo {
        fn write(&mut self, t: &PresetTarget, after: &str) -> Result<(), String> {
            if self.fail_write_on == Some(t.list_index) {
                return Err("simulated device write failure".into());
            }
            self.writes += 1;
            self.store.insert(t.list_index, after.to_string());
            Ok(())
        }
        fn read_back(&mut self, t: &PresetTarget) -> Result<String, String> {
            self.store
                .get(&t.list_index)
                .cloned()
                .ok_or_else(|| "no such slot".into())
        }
    }

    /// Sets `audioGraph.outputLevel` to a fixed value; a preset lacking that field is
    /// "not a target" (skipped) — exercising the skip path.
    struct SetOutputLevel {
        value: f64,
    }
    impl Operation for SetOutputLevel {
        fn label(&self) -> String {
            format!("set outputLevel = {}", self.value)
        }
        fn transform(&self, t: &PresetTarget) -> Result<Option<String>, String> {
            let mut v: serde_json::Value =
                serde_json::from_str(&t.before_json).map_err(|e| e.to_string())?;
            let Some(p) = v.pointer_mut("/audioGraph/outputLevel") else {
                return Ok(None);
            };
            *p = serde_json::json!(self.value);
            Ok(Some(serde_json::to_string(&v).map_err(|e| e.to_string())?))
        }
    }

    fn target(idx: u32, json: &str) -> PresetTarget {
        PresetTarget {
            list_index: idx,
            list_enum: 1,
            display_name: format!("P{idx}"),
            source: "offline-file".into(),
            before_json: json.into(),
        }
    }
    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tmp-bulkrun-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    const HAS: &str = r#"{"audioGraph":{"outputLevel":0.8}}"#;
    const HASNT: &str = r#"{"audioGraph":{}}"#;

    // AC1 — dry-run lists the change set and writes nothing.
    #[test]
    fn dryrun_lists_changes_and_writes_nothing() {
        let targets = vec![target(0, HAS), target(1, HASNT)];
        let entries = dry_run(&targets, &SetOutputLevel { value: 0.5 });
        assert_eq!(entries[0].status, ChangeStatus::Changed);
        assert_eq!(entries[0].changes.len(), 1);
        assert_eq!(entries[0].changes[0].pointer, "/audioGraph/outputLevel");
        assert_eq!(entries[1].status, ChangeStatus::Skipped);
        // Writes nothing: dry_run takes no PresetIo (no write is possible), and the
        // targets are left byte-for-byte unmodified.
        assert_eq!(targets[0].before_json, HAS);
    }

    // AC2 — apply snapshots each target BEFORE writing; a failed write still leaves a
    // recoverable snapshot on disk.
    #[test]
    fn apply_snapshots_before_write() {
        let targets = vec![target(0, HAS)];
        let mut io = MockIo::new(&[(0, HAS)]);
        io.fail_write_on = Some(0);
        let dir = tmpdir("ac2");
        let report = apply(&targets, &SetOutputLevel { value: 0.5 }, &mut io, &dir);

        let e = &report.entries[0];
        assert!(!e.changed, "write failed → not changed");
        assert!(e.error.as_ref().unwrap().contains("write"));
        let snap_path = e
            .snapshot_path
            .as_ref()
            .expect("snapshot must be taken before the write");
        assert!(snap_path.exists(), "snapshot file persisted → revertible");
        let snap = backup::load_snapshot_from_path(snap_path).unwrap();
        assert_eq!(
            snap.preset_json, HAS,
            "snapshot captured the pre-edit state"
        );
        assert_eq!(io.writes, 0, "the failing write never mutated the store");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // AC3 — apply collects a per-preset result across changed / skipped / errored.
    #[test]
    fn apply_collects_per_preset_results() {
        let bad = r#"not json"#;
        let targets = vec![target(0, HAS), target(1, HASNT), target(2, bad)];
        let mut io = MockIo::new(&[(0, HAS), (1, HASNT), (2, bad)]);
        let dir = tmpdir("ac3");
        let report = apply(&targets, &SetOutputLevel { value: 0.5 }, &mut io, &dir);

        // slot 0: changed + verified, no error.
        assert!(report.entries[0].changed && report.entries[0].verified);
        assert!(report.entries[0].error.is_none());
        // slot 1: skipped (no outputLevel) — not changed, no error, no snapshot.
        assert!(!report.entries[1].changed && report.entries[1].error.is_none());
        assert!(report.entries[1].snapshot_path.is_none());
        // slot 2: errored (transform parse failure).
        assert!(report.entries[2].error.is_some() && !report.entries[2].changed);

        assert_eq!(report.changed(), 1);
        assert_eq!(report.verified(), 1);
        assert_eq!(report.errors(), 1);
        assert!(
            io.store[&0].contains("0.5"),
            "the one valid edit was written"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // AC4 — "revert run" restores every preset the run touched.
    #[test]
    fn revert_run_restores_all_touched() {
        let a = r#"{"audioGraph":{"outputLevel":0.8}}"#;
        let b = r#"{"audioGraph":{"outputLevel":0.2}}"#;
        let targets = vec![target(0, a), target(1, b)];
        let mut io = MockIo::new(&[(0, a), (1, b)]);
        let dir = tmpdir("ac4");

        let report = apply(&targets, &SetOutputLevel { value: 0.5 }, &mut io, &dir);
        assert_eq!(report.changed(), 2);
        assert!(io.store[&0].contains("0.5") && io.store[&1].contains("0.5"));

        let rev = revert(&report, &mut io);
        assert_eq!(rev.len(), 2);
        assert!(
            rev.iter().all(|r| r.restored),
            "every touched preset restored"
        );
        // Back to the originals (compare structurally — key order may differ).
        assert!(backup::diff_preset_json(&io.store[&0], a)
            .unwrap()
            .is_empty());
        assert!(backup::diff_preset_json(&io.store[&1], b)
            .unwrap()
            .is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
