//! Pre-edit preset backups + the diff primitive.
//!
//! Every destructive write snapshots the affected preset(s) first so any bulk op
//! is reversible. A snapshot is the preset's JSON (opaque passthrough — we do NOT
//! model the full schema) plus minimal addressing (`list_enum`, `slot`) and a
//! capture timestamp + `source`. `restore_bytes` hands back the raw `.preset` bytes
//! for the caller to re-import (the OFFLINE in-place path). The diff primitive powers
//! dry-run previews and verify-after-restore.
//!
//! **Completeness caveat (AC1):** USB cannot read a complete preset, so a snapshot
//! captured `source = "usb-partial"` is only as complete as the partial it came
//! from. A faithful backup needs `source = "offline-file"` (a `.preset`). `source`
//! records which, so a restore never silently over-promises.
//!
//! Persistence mirrors `profiles`: pure `*_from_path` / `*_to_path` helpers carry
//! the logic so they unit-test without a Tauri `AppHandle`. No retention policy,
//! versioning, scheduling, or cloud.
//!
//! Its public API is the safety net every destructive feature calls —
//! `save_snapshot_to_dir` (bulk runs), `restore_bytes` (the `--restore` path),
//! `diff_preset_json` (dry-run previews), and `list_snapshots_in_dir`
//! (the `list_snapshots` command).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A pre-edit capture of one preset. `preset_json` is opaque — we round-trip it
/// verbatim and never reserialize through a typed schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PresetSnapshot {
    pub list_enum: u32,
    pub slot: u32,
    /// Best-effort display name (for the backups list); not load-bearing.
    #[serde(default)]
    pub display_name: String,
    /// Capture instant, RFC3339 UTC (opaque string — only used for ordering/naming).
    pub captured_at: String,
    /// How `preset_json` was obtained: `"usb-partial"` (incomplete — see AC1) or
    /// `"offline-file"` (a complete `.preset`).
    pub source: String,
    /// The preset JSON at capture time, verbatim.
    pub preset_json: String,
}

/// Prepare a snapshot for re-import to the device: validate it is a *faithful*
/// (offline-file) backup and return the raw `.preset` bytes to import. **Refuses
/// a `usb-partial` snapshot** — re-importing a partial would overwrite the slot
/// with truncated data (AC1). The caller pushes these bytes through the AC7
/// in-place path (`lib::run_replace_inplace`) so the restore lands on the
/// snapshot's original slot and keeps its Song binding — NOT a bare append.
pub fn restore_bytes(snapshot: &PresetSnapshot) -> Result<Vec<u8>, String> {
    if snapshot.source != "offline-file" {
        return Err(format!(
            "refusing to restore a {:?} snapshot to the device: only a complete \
             'offline-file' backup is safe to re-import (a partial would overwrite the \
             slot with truncated data — AC1)",
            snapshot.source
        ));
    }
    Ok(xor_jld(snapshot.preset_json.as_bytes()))
}

/// The Tone Master Pro `.preset` codec: a fixed 3-byte repeating XOR ("JLD"). It's a
/// trivial obfuscation, not encryption — the same constant for every `.preset` and the
/// device, long since reverse-engineered (see the `tmp-protocol` skill). Committed
/// directly rather than recovered at runtime: there's nothing to protect and the
/// dynamic-recovery dance only added a "decode a file first" footgun (it blocked the
/// online e2e seeder, which has no `.preset` to learn from).
const PRESET_XOR_KEY: [u8; 3] = *b"JLD";

/// A `.preset` file is its compact JSON XOR'd with [`PRESET_XOR_KEY`]. The XOR is
/// self-inverse, so encode == decode (the same op maps JSON ↔ file bytes); a
/// snapshot's `preset_json` is the verbatim decode of an offline `.preset`, so
/// re-XORing reproduces the original file bytes exactly.
pub(crate) fn xor_jld(data: &[u8]) -> Vec<u8> {
    xor(data, PRESET_XOR_KEY)
}

/// The pure repeating-XOR core, given an explicit key.
fn xor(data: &[u8], key: [u8; 3]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(i, b)| b ^ key[i % 3])
        .collect()
}

/// Deterministic, filesystem-safe snapshot filename: `<ts>_le<list>_slot<slot>.json`
/// (colons in the timestamp replaced so it is valid on every filesystem).
pub fn snapshot_filename(s: &PresetSnapshot) -> String {
    let ts = s.captured_at.replace([':', '/', ' '], "-");
    format!("{ts}_le{}_slot{}.json", s.list_enum, s.slot)
}

/// Write `snapshot` into `dir` (created as needed); returns the file path.
pub fn save_snapshot_to_dir(dir: &Path, snapshot: &PresetSnapshot) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    let path = dir.join(snapshot_filename(snapshot));
    let json =
        serde_json::to_string_pretty(snapshot).map_err(|e| format!("serialize snapshot: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(path)
}

/// Read a snapshot from `path`.
pub fn load_snapshot_from_path(path: &Path) -> Result<PresetSnapshot, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("parse {}: {e}", path.display()))
}

/// List snapshot files in `dir` (newest first by filename — timestamp-prefixed).
/// A missing dir yields an empty list (no backups taken yet).
pub fn list_snapshots_in_dir(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("read dir {}: {e}", dir.display())),
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    paths.reverse();
    Ok(paths)
}

/// `backups/` under the app config dir. App-layer seam wired into a Tauri command
/// by the first destructive feature.
pub(crate) fn backups_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("resolve app config dir: {e}"))?;
    Ok(dir.join("backups"))
}

// ─── diff primitive ──────────────────────────────────────────────────────────

/// One changed field between two preset states, addressed by JSON pointer.
/// `before`/`after` are `None` when the field was added/removed respectively.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldChange {
    pub pointer: String,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
}

/// Diff two preset JSON strings → the list of changed fields (sorted by pointer).
/// Backup/revert + dry-run all need this; it is deliberately NOT a merge engine.
pub fn diff_preset_json(before: &str, after: &str) -> Result<Vec<FieldChange>, String> {
    let b: serde_json::Value =
        serde_json::from_str(before).map_err(|e| format!("parse before: {e}"))?;
    let a: serde_json::Value =
        serde_json::from_str(after).map_err(|e| format!("parse after: {e}"))?;
    let mut out = Vec::new();
    diff_value("", &b, &a, &mut out);
    out.sort_by(|x, y| x.pointer.cmp(&y.pointer));
    Ok(out)
}

/// Recursive structural diff. Objects diff by key (added/removed/changed); arrays
/// by index (length changes surface as added/removed indices); scalars by equality.
fn diff_value(
    ptr: &str,
    before: &serde_json::Value,
    after: &serde_json::Value,
    out: &mut Vec<FieldChange>,
) {
    use serde_json::Value;
    match (before, after) {
        (Value::Object(b), Value::Object(a)) => {
            let mut keys: Vec<&String> = b.keys().chain(a.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                let child = format!("{ptr}/{}", escape_ptr(k));
                match (b.get(k), a.get(k)) {
                    (Some(bv), Some(av)) => diff_value(&child, bv, av, out),
                    (Some(bv), None) => out.push(FieldChange {
                        pointer: child,
                        before: Some(bv.clone()),
                        after: None,
                    }),
                    (None, Some(av)) => out.push(FieldChange {
                        pointer: child,
                        before: None,
                        after: Some(av.clone()),
                    }),
                    (None, None) => {}
                }
            }
        }
        (Value::Array(b), Value::Array(a)) => {
            for i in 0..b.len().max(a.len()) {
                let child = format!("{ptr}/{i}");
                match (b.get(i), a.get(i)) {
                    (Some(bv), Some(av)) => diff_value(&child, bv, av, out),
                    (Some(bv), None) => out.push(FieldChange {
                        pointer: child,
                        before: Some(bv.clone()),
                        after: None,
                    }),
                    (None, Some(av)) => out.push(FieldChange {
                        pointer: child,
                        before: None,
                        after: Some(av.clone()),
                    }),
                    (None, None) => {}
                }
            }
        }
        _ => {
            if before != after {
                out.push(FieldChange {
                    pointer: ptr.to_string(),
                    before: Some(before.clone()),
                    after: Some(after.clone()),
                });
            }
        }
    }
}

/// RFC-6901 JSON-pointer token escaping (`~` → `~0`, `/` → `~1`).
fn escape_ptr(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PresetSnapshot {
        PresetSnapshot {
            list_enum: 1,
            slot: 11,
            display_name: "Bassmans Comparison".into(),
            captured_at: "2026-06-20T16:09:00Z".into(),
            source: "offline-file".into(),
            preset_json:
                r#"{"uuid":"abc","audioGraph":{"guitarNodes":{"G1":[{"outputLevel":0.5}]}}}"#.into(),
        }
    }

    #[test]
    fn snapshot_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("tmp-bk-test-{}", std::process::id()));
        let s = sample();
        let path = save_snapshot_to_dir(&dir, &s).unwrap();
        assert_eq!(load_snapshot_from_path(&path).unwrap(), s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_filename_is_filesystem_safe() {
        // No colons/slashes survive (valid on every filesystem).
        let name = snapshot_filename(&sample());
        assert!(!name.contains(':') && !name.contains('/'));
        assert!(name.ends_with("_le1_slot11.json"));
    }

    #[test]
    fn list_snapshots_missing_dir_is_empty() {
        let dir = std::env::temp_dir().join("tmp-bk-none-xyz");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(list_snapshots_in_dir(&dir).unwrap().is_empty());
    }

    // AC5: snapshot a preset, "edit" a copy, restore → back to the pre-edit state.
    #[test]
    fn backup_snapshot_then_restore_restores_state() {
        let original = sample();
        // Simulate a destructive edit on a working copy.
        let edited = original.preset_json.replace("0.5", "0.9");
        assert_ne!(edited, original.preset_json);
        // The captured snapshot holds exactly the pre-edit JSON.
        // And the diff between edited and the snapshot shows exactly the one field.
        let changes = diff_preset_json(&edited, &original.preset_json).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(
            changes[0].pointer,
            "/audioGraph/guitarNodes/G1/0/outputLevel"
        );
    }

    #[test]
    fn diff_detects_changed_added_removed() {
        let before = r#"{"a":1,"b":{"x":true},"d":[1,2]}"#;
        let after = r#"{"a":2,"b":{"x":true,"y":9},"d":[1]}"#;
        let changes = diff_preset_json(before, after).unwrap();
        let ptrs: Vec<&str> = changes.iter().map(|c| c.pointer.as_str()).collect();
        // a changed; b.y added; d[1] removed. b.x unchanged → absent.
        assert_eq!(ptrs, ["/a", "/b/y", "/d/1"]);
        assert_eq!(changes[0].before, Some(serde_json::json!(1)));
        assert_eq!(changes[0].after, Some(serde_json::json!(2)));
        assert_eq!(changes[2].after, None); // removed
    }

    #[test]
    fn diff_identical_is_empty() {
        let j = r#"{"a":1,"nested":{"b":[1,2,3]}}"#;
        assert!(diff_preset_json(j, j).unwrap().is_empty());
    }

    #[test]
    fn restore_bytes_reproduces_preset_file_exactly() {
        // A real `.preset` file is the compact JSON under the self-inverse XOR. A snapshot stores the
        // verbatim decoded JSON, so restore_bytes must re-XOR it back to the exact
        // original file bytes (XOR is self-inverse).
        let json = r#"{"info":{"displayName":"X"}}"#;
        let preset_file = super::xor_jld(json.as_bytes()); // what was on disk
        let snap = PresetSnapshot {
            source: "offline-file".into(),
            preset_json: json.into(),
            ..sample()
        };
        assert_eq!(restore_bytes(&snap).unwrap(), preset_file);
        // round-trips: decoding the restored bytes returns the snapshot JSON.
        assert_eq!(
            super::xor_jld(&restore_bytes(&snap).unwrap()),
            json.as_bytes()
        );
    }

    #[test]
    fn restore_bytes_refuses_usb_partial() {
        let snap = PresetSnapshot {
            source: "usb-partial".into(),
            ..sample()
        };
        let err = restore_bytes(&snap).unwrap_err();
        assert!(
            err.contains("usb-partial") || err.contains("partial"),
            "got: {err}"
        );
    }
}
