//! OFFLINE preset-library ingestion.
//!
//! The canonical full-preset source is a **folder of `.preset` files** produced by
//! Pro Control's one-shot "export to folder" (there is no reliable complete-preset
//! read over USB). This module
//! decodes that folder into an in-memory [`Library`] of [`LibraryRecord`]s, indexes
//! each via [`crate::search::index_preset`], and **reconciles** the files against
//! the live device slot list so a record can be addressed for a write.
//!
//! Reconciliation is by **display name** (the only join key the device list exposes —
//! `PresetEntry` is `{slot, name}`, no `preset_id`). Duplicate names are marked
//! ambiguous and get **no** `list_index`, so they are never selectable for a
//! destructive op (the same-address-space guard from the destructive-op lesson).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::backup;
use crate::search::{self, CategoryMap, Facets, PresetRecord};
use crate::session::{self, PresetEntry};

/// Decode a `.preset` file's raw bytes to its JSON string via the XOR codec.
/// The single production "bytes → JSON" helper. A `.preset` file is the compact
/// JSON under a self-inverse 3-byte XOR (the fixed `backup::xor_jld` key).
pub fn decode_preset_bytes(bytes: &[u8]) -> Result<String, String> {
    String::from_utf8(backup::xor_jld(bytes))
        .map_err(|e| format!("decoded .preset is not valid UTF-8: {e}"))
}

/// A preset doc's identity uuid (`info.preset_id`) — the ONE home of that JSON
/// walk, shared by the library ingest and the e2e stray-sweep ownership guard.
pub(crate) fn preset_id_of(value: &Value) -> Option<&str> {
    value.pointer("/info/preset_id").and_then(Value::as_str)
}

/// One `.preset` file ingested from the export folder, plus its reconciliation
/// against the live device. `decoded_json` is the canonical full preset JSON;
/// `list_index` is `Some` only once matched to a (uniquely-named) device slot.
#[derive(Debug, Clone, Serialize)]
pub struct LibraryRecord {
    pub file_path: PathBuf,
    pub display_name: String,
    pub preset_id: Option<String>,
    #[serde(skip)] // never crosses IPC — large; the DTO drops it
    pub decoded_json: String,
    pub facets: Facets,
    /// 0-based "My Presets" index on the connected device; `None` if unmatched or
    /// ambiguous (a duplicate name) — such a record is NOT writable.
    pub list_index: Option<u32>,
    /// The list the record belongs to. Only `1` (My Presets) is writable this effort.
    pub list_enum: u32,
}

impl LibraryRecord {
    /// Build a `bulkrun::PresetTarget` for a matched record (its `before_json` is the
    /// canonical OFFLINE decode). Errors if the record was never matched to a slot.
    pub fn to_target(&self) -> Result<crate::bulkrun::PresetTarget, String> {
        let list_index = self.list_index.ok_or_else(|| {
            format!(
                "preset {:?} is not matched to a device slot (unmatched or ambiguous name) — \
                 cannot target it for a write",
                self.display_name
            )
        })?;
        Ok(crate::bulkrun::PresetTarget {
            list_index,
            list_enum: self.list_enum,
            display_name: self.display_name.clone(),
            source: "offline-file".to_string(),
            before_json: self.decoded_json.clone(),
        })
    }

    /// The search-engine view (list_index + facets) for filtering. Uses
    /// `list_index = u32::MAX` sentinel when unmatched so it still indexes for search
    /// (search is read-only; selection-for-write re-checks `list_index.is_some()`).
    pub fn search_record(&self) -> PresetRecord {
        PresetRecord {
            list_index: self.list_index.unwrap_or(u32::MAX),
            facets: self.facets.clone(),
        }
    }
}

/// The ingested + reconciled preset library held in app state.
#[derive(Debug, Clone, Default)]
pub struct Library {
    /// The imported export folder — shown in the Library panel header (Phase 1 UI).
    #[allow(dead_code)]
    pub folder: PathBuf,
    pub records: Vec<LibraryRecord>,
}

/// Outcome of matching folder files to the live device list. Surfaced to the UI so
/// the user confirms before any write (the destructive-op guard).
#[derive(Debug, Clone, Default, Serialize)]
pub struct ReconcileReport {
    /// Files matched 1:1 to a uniquely-named device slot (writable).
    pub matched: usize,
    /// Files whose name was not found on the device (can't be edited in place).
    pub unmatched_files: Vec<String>,
    /// Occupied device slots with no corresponding file (can't be edited OFFLINE).
    pub unmatched_slots: Vec<String>,
    /// Names appearing more than once (on device and/or in the folder) — left
    /// unaddressable so a wrong-slot write is impossible.
    pub ambiguous: Vec<String>,
}

/// Read every `*.preset` in `folder`, decode + index each. Per-file errors are
/// collected (a single bad file does not fail the import) and returned alongside the
/// records. `categories` classifies block ids into amp/cab facets (empty = graceful).
pub fn load_library_from_dir(
    folder: &Path,
    categories: &CategoryMap,
) -> Result<(Vec<LibraryRecord>, Vec<String>), String> {
    let entries =
        std::fs::read_dir(folder).map_err(|e| format!("read folder {}: {e}", folder.display()))?;
    let mut records = Vec::new();
    let mut errors = Vec::new();
    for entry in entries {
        let path = match entry {
            Ok(e) => e.path(),
            Err(e) => {
                errors.push(format!("dir entry: {e}"));
                continue;
            }
        };
        if path
            .extension()
            .and_then(|x| x.to_str())
            .map(|x| x.eq_ignore_ascii_case("preset"))
            != Some(true)
        {
            continue;
        }
        match ingest_file(&path, categories) {
            Ok(rec) => records.push(rec),
            Err(e) => errors.push(format!("{}: {e}", path.display())),
        }
    }
    // Stable order by display name for a deterministic UI list.
    records.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok((records, errors))
}

fn ingest_file(path: &Path, categories: &CategoryMap) -> Result<LibraryRecord, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read: {e}"))?;
    let json = decode_preset_bytes(&bytes)?;
    let value: Value = serde_json::from_str(&json).map_err(|e| format!("parse JSON: {e}"))?;
    let display_name = value
        .pointer("/info/displayName")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let preset_id = preset_id_of(&value).map(str::to_string);
    let facets = search::index_preset(&value, categories);
    Ok(LibraryRecord {
        file_path: path.to_path_buf(),
        display_name,
        preset_id,
        decoded_json: json,
        facets,
        list_index: None,
        list_enum: 1,
    })
}

/// Join `records` to the live `device_list` by display name, filling `list_index` on
/// uniquely-matched records and returning a report. A name that occurs more than once
/// (in either the folder or the device list) is left `list_index = None` (ambiguous):
/// addressing it for a write could hit the wrong slot.
pub fn reconcile_with_device(
    records: &mut [LibraryRecord],
    device_list: &[PresetEntry],
) -> ReconcileReport {
    // Count names on each side. Owns its keys (String) so it doesn't borrow
    // `records`, which we iterate mutably below.
    let mut file_name_count: HashMap<String, usize> = HashMap::new();
    for r in records.iter() {
        *file_name_count.entry(r.display_name.clone()).or_default() += 1;
    }
    let mut device_by_name: HashMap<&str, Vec<u32>> = HashMap::new();
    for e in device_list.iter() {
        if !session::is_empty_slot_name(&e.name) {
            device_by_name
                .entry(e.name.as_str())
                .or_default()
                .push(e.slot);
        }
    }

    let mut report = ReconcileReport::default();
    let mut ambiguous: std::collections::BTreeSet<String> = Default::default();
    let mut matched_slots: std::collections::HashSet<u32> = Default::default();

    for r in records.iter_mut() {
        let name = r.display_name.clone();
        let dup_in_files = file_name_count.get(name.as_str()).copied().unwrap_or(0) > 1;
        match device_by_name.get(name.as_str()) {
            Some(slots) if slots.len() == 1 && !dup_in_files => {
                r.list_index = Some(slots[0]);
                matched_slots.insert(slots[0]);
                report.matched += 1;
            }
            Some(_) => {
                // Name occurs multiple times on the device and/or in the folder.
                r.list_index = None;
                ambiguous.insert(name);
            }
            None => {
                r.list_index = None;
                report.unmatched_files.push(name);
            }
        }
    }

    for e in device_list.iter() {
        if !session::is_empty_slot_name(&e.name)
            && !matched_slots.contains(&e.slot)
            && !ambiguous.contains(&e.name)
        {
            report
                .unmatched_slots
                .push(format!("{} (slot {})", e.name, e.slot));
        }
    }
    report.ambiguous = ambiguous.into_iter().collect();
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_preset(dir: &Path, file: &str, json: &str) {
        let bytes = backup::xor_jld(json.as_bytes()); // encode == decode
        std::fs::write(dir.join(file), bytes).unwrap();
    }

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tmp-lib-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn decode_round_trips_xor() {
        let json = r#"{"info":{"displayName":"Clean Twin"}}"#;
        let bytes = backup::xor_jld(json.as_bytes());
        assert_eq!(decode_preset_bytes(&bytes).unwrap(), json);
    }

    #[test]
    fn loads_and_indexes_a_folder() {
        let dir = tmpdir("load");
        write_preset(
            &dir,
            "a.preset",
            r#"{"info":{"displayName":"Alpha","preset_id":"id-a"},"audioGraph":{"template":"Instrument Series","presetLevel":0.8}}"#,
        );
        write_preset(&dir, "b.preset", r#"{"info":{"displayName":"Bravo"}}"#);
        std::fs::write(dir.join("notes.txt"), b"ignore me").unwrap(); // non-.preset skipped

        let (recs, errs) = load_library_from_dir(&dir, &CategoryMap::new()).unwrap();
        assert!(errs.is_empty(), "errors: {errs:?}");
        assert_eq!(recs.len(), 2);
        // Sorted by name: Alpha, Bravo.
        assert_eq!(recs[0].display_name, "Alpha");
        assert_eq!(recs[0].preset_id.as_deref(), Some("id-a"));
        assert_eq!(recs[0].facets.template, "Instrument Series");
        assert_eq!(recs[0].facets.preset_level, Some(0.8));
        assert_eq!(recs[1].preset_id, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_file_is_collected_not_fatal() {
        let dir = tmpdir("bad");
        write_preset(&dir, "ok.preset", r#"{"info":{"displayName":"Good"}}"#);
        std::fs::write(dir.join("bad.preset"), b"\xff\xfe not xor-json").unwrap();
        let (recs, errs) = load_library_from_dir(&dir, &CategoryMap::new()).unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!(errs.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn rec(name: &str) -> LibraryRecord {
        LibraryRecord {
            file_path: PathBuf::from(format!("{name}.preset")),
            display_name: name.to_string(),
            preset_id: None,
            decoded_json: format!(r#"{{"info":{{"displayName":"{name}"}}}}"#),
            facets: Facets::default(),
            list_index: None,
            list_enum: 1,
        }
    }
    fn entry(slot: u32, name: &str) -> PresetEntry {
        PresetEntry {
            slot,
            name: name.to_string(),
        }
    }

    #[test]
    fn reconcile_matches_unique_names_and_flags_the_rest() {
        let mut recs = vec![rec("Clean Twin"), rec("Lead Boost"), rec("Orphan File")];
        let device = vec![
            entry(0, "Clean Twin"),
            entry(1, "Lead Boost"),
            entry(2, "On Device Only"),
            entry(3, "--"), // empty slot ignored
        ];
        let report = reconcile_with_device(&mut recs, &device);
        assert_eq!(recs[0].list_index, Some(0));
        assert_eq!(recs[1].list_index, Some(1));
        assert_eq!(recs[2].list_index, None); // not on device
        assert_eq!(report.matched, 2);
        assert_eq!(report.unmatched_files, vec!["Orphan File"]);
        assert_eq!(report.unmatched_slots, vec!["On Device Only (slot 2)"]);
    }

    #[test]
    fn reconcile_marks_duplicate_names_ambiguous_and_unaddressable() {
        let mut recs = vec![rec("Dup"), rec("Dup"), rec("Unique")];
        let device = vec![entry(0, "Dup"), entry(1, "Unique")];
        let report = reconcile_with_device(&mut recs, &device);
        // Both "Dup" records stay None — a wrong-slot write is impossible.
        assert!(recs[0].list_index.is_none() && recs[1].list_index.is_none());
        assert_eq!(recs[2].list_index, Some(1));
        assert_eq!(report.ambiguous, vec!["Dup"]);
        assert_eq!(report.matched, 1);
    }

    #[test]
    fn ambiguous_device_name_is_not_matched() {
        // Same name twice on the device → ambiguous even with one file.
        let mut recs = vec![rec("Twice")];
        let device = vec![entry(0, "Twice"), entry(5, "Twice")];
        let report = reconcile_with_device(&mut recs, &device);
        assert_eq!(recs[0].list_index, None);
        assert_eq!(report.ambiguous, vec!["Twice"]);
    }
}
