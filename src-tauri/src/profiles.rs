//! Instrument profiles + their persisted store.
//!
//! A profile names a real instrument the user owns ("Telecaster", "Metal Ibanez")
//! and links it to a shipped pickup `topology_id` (see `topologies`). The leveler
//! re-amps that topology's stimulus when leveling a preset the profile is assigned
//! to, so loudness is matched against the instrument the preset is actually played
//! with. `calibration_lufs` is the Tier-2 real-output measurement (K-weighted
//! loudness of the dry signal); `None` means use the topology's shipped drive level.
//!
//! Persistence is a single JSON file in the app config dir — deliberately minimal
//! (no schema versioning/migration). The pure `*_from_path` / `*_to_path` helpers
//! carry the logic so they unit-test without a Tauri `AppHandle`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A user-defined instrument linked to a shipped pickup topology.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    /// Stable id (UI-generated, e.g. a uuid/timestamp string).
    pub id: String,
    /// Free-text instrument name ("Telecaster").
    pub name: String,
    /// References `topologies::Topology::id`.
    pub topology_id: String,
    /// Tier-2 measured real output loudness (K-weighted LUFS of the dry signal);
    /// `None` until calibrated. K-weighted, not flat RMS, so it tracks how hard
    /// the instrument actually drives the amp (bright pickups aren't under-counted).
    #[serde(default)]
    pub calibration_lufs: Option<f32>,
}

/// A named loudness target the user applies per preset (e.g. "Lead" → −22 LUFS).
/// Pure UI/config data — the leveler only ever receives the resolved `lufs` value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Target {
    pub name: String,
    pub lufs: f64,
}

/// The playback loudness the rig is matched at. Equal-LUFS is equal-loudness at
/// ONE SPL only: BS.1770's K-weighting is a fixed equal-loudness snapshot, while
/// the real contours (Fletcher–Munson) flatten as SPL rises — at stage volume the
/// low-frequency energy a bass preset lives on contributes far more perceived
/// loudness than at bedroom level. The leveler compensates by adding a per-
/// instrument LU offset to the target (see [`playback_offset_lu`]).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackLevel {
    /// Bedroom / home practice — contours steepest, bass needs the most help.
    Quiet,
    /// Rehearsal volume — partway up the contour family.
    Rehearsal,
    /// Stage volume — the K-weighting design point; no compensation.
    #[default]
    Stage,
}

/// Fletcher–Munson playback compensation, in LU, added to the leveling target
/// for an instrument family (`topologies::Topology::instrument`). Guitar presets
/// are the spectral reference (0 at every level — within-family leveling is
/// contour-invariant); bass presets are leveled hotter the quieter the playback,
/// because K-weighting under-credits their low-frequency energy relative to
/// perception at low SPL. Values are deliberately coarse (the contours vary by
/// listener and rig); `Stage` is always 0 so the default changes nothing.
pub fn playback_offset_lu(level: PlaybackLevel, instrument: &str) -> f64 {
    if instrument != "bass" {
        return 0.0;
    }
    match level {
        PlaybackLevel::Quiet => 1.5,
        PlaybackLevel::Rehearsal => 0.5,
        PlaybackLevel::Stage => 0.0,
    }
}

/// The three pro-grade live levels shipped by default (and seeded into any store
/// that predates the `targets` field). Names match modeler vocab (Kemper/Helix/QC);
/// all ≤ −22 LUFS to stay under the device's ~−20 LUFS re-amp tap ceiling.
fn default_targets() -> Vec<Target> {
    vec![
        Target {
            name: "Rhythm".into(),
            lufs: -26.0,
        },
        Target {
            name: "Crunch".into(),
            lufs: -24.0,
        },
        Target {
            name: "Lead".into(),
            lufs: -22.0,
        },
    ]
}

/// The whole persisted store: the profile list, which profile each preset slot
/// is assigned to, and the user's named loudness targets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Store {
    #[serde(default)]
    pub profiles: Vec<Profile>,
    /// slot → profile id. Serde stringifies integer map keys in JSON.
    #[serde(default)]
    pub profile_by_slot: HashMap<u32, String>,
    /// User-editable loudness targets; seeded with `default_targets()` for fresh
    /// installs (via `Default`) and for older stores missing the key (serde default).
    #[serde(default = "default_targets")]
    pub targets: Vec<Target>,
    /// The playback loudness leveling compensates for (older stores default to
    /// `Stage` = no compensation, preserving their existing leveling baseline).
    #[serde(default)]
    pub playback_level: PlaybackLevel,
    /// Auto-download updates in the background (Settings → App updates).
    #[serde(default = "default_true")]
    pub auto_install_updates: bool,
}

fn default_true() -> bool {
    true
}

// Manual `Default` (not derived) so the missing-file path — `load_from_path` returns
// `Store::default()` — seeds the three default targets rather than an empty Vec.
impl Default for Store {
    fn default() -> Self {
        Store {
            profiles: Vec::new(),
            profile_by_slot: HashMap::new(),
            targets: default_targets(),
            playback_level: PlaybackLevel::default(),
            auto_install_updates: true,
        }
    }
}

/// The app config dir (`profiles.json` + `captures/` live here).
pub(crate) fn app_config_dir<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<PathBuf, String> {
    use tauri::Manager;
    app.path()
        .app_config_dir()
        .map_err(|e| format!("resolve app config dir: {e}"))
}

/// `profiles.json` under the app config dir.
fn store_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    Ok(app_config_dir(app)?.join("profiles.json"))
}

/// Read the store from `path`; a missing file yields the default (empty) store.
pub fn load_from_path(path: &Path) -> Result<Store, String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|e| format!("parse {}: {e}", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
        Err(e) => Err(format!("read {}: {e}", path.display())),
    }
}

/// Write the store to `path` (pretty JSON), creating parent dirs as needed.
pub fn save_to_path(path: &Path, store: &Store) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(store).map_err(|e| format!("serialize store: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Load the store for the running app (config-dir resolved).
pub fn load<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<Store, String> {
    load_from_path(&store_path(app)?)
}

/// Persist the store for the running app. Generic over the runtime (like `load`)
/// so runtime-generic commands (e.g. `set_auto_install_updates`) can call it.
pub fn save<R: tauri::Runtime>(app: &tauri::AppHandle<R>, store: &Store) -> Result<(), String> {
    save_to_path(&store_path(app)?, store)
}

// ───────────────────────── Tier-2 calibration capture store ─────────────────────────
//
// A profile's calibration capture is the dry-DI WAV recorded during `calibrate_profile`;
// leveling injects it verbatim as the re-amp stimulus (no synthetic scaling). Stored at
// `<app_config_dir>/captures/<profile_id>.wav`. The pure `*_in` helpers take the config
// dir so they unit-test without an `AppHandle`.

/// Map a UI-generated profile id to a single safe filename stem — profile ids are
/// uuid/timestamp strings, but never trust them as path components (a `../` id must
/// not escape the captures dir). Non `[A-Za-z0-9_-]` chars collapse to `_`.
fn sanitize_id(profile_id: &str) -> String {
    profile_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// `<config_dir>/captures/<sanitized-id>.wav` — pure, no filesystem touch.
pub(crate) fn capture_wav_path_in(config_dir: &Path, profile_id: &str) -> PathBuf {
    config_dir
        .join("captures")
        .join(format!("{}.wav", sanitize_id(profile_id)))
}

/// The stored capture path for a profile, only if the file exists.
pub(crate) fn existing_capture(config_dir: &Path, profile_id: &str) -> Option<PathBuf> {
    let p = capture_wav_path_in(config_dir, profile_id);
    p.is_file().then_some(p)
}

/// The stored capture path for a profile of the running app, only if it exists.
pub(crate) fn existing_capture_for<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    profile_id: &str,
) -> Option<PathBuf> {
    existing_capture(&app_config_dir(app).ok()?, profile_id)
}

/// Store (or clear) a profile's calibration capture, keeping the on-disk WAV
/// consistent with the scalar `calibrate_profile` is about to save:
/// - ALWAYS unlink any existing capture first (a clipped/aborted run must not leave
///   a stale WAV paired with a fresh scalar).
/// - `clipped` → store nothing, return `Ok(false)` (leveling falls back to synthetic).
/// - else write the mono 48 kHz f32 samples via temp-file + rename (no torn WAV can
///   pass an existence check) and return `Ok(true)`. A write failure is an `Err` so
///   the caller fails the command BEFORE persisting the scalar.
pub(crate) fn store_capture(
    config_dir: &Path,
    profile_id: &str,
    samples: &[f32],
    clipped: bool,
) -> Result<bool, String> {
    let path = capture_wav_path_in(config_dir, profile_id);
    let _ = std::fs::remove_file(&path);
    if clipped {
        return Ok(false);
    }
    let parent = path
        .parent()
        .ok_or_else(|| "capture path has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    let tmp = path.with_extension("wav.part");
    let tmp_str = tmp.to_str().ok_or("capture temp path not UTF-8")?;
    crate::probe_api::stimulus::write_wav_mono(tmp_str, samples, 48_000)?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename capture {}: {e}", path.display()))?;
    Ok(true)
}

/// Unlink the stored captures for `ids` (best-effort — a failed unlink is logged,
/// never an error). Keeps all capture-file ownership in this module; the config
/// dir is resolved once for the whole batch.
pub(crate) fn unlink_captures<R: tauri::Runtime>(app: &tauri::AppHandle<R>, ids: &[String]) {
    let Ok(dir) = app_config_dir(app) else { return };
    for id in ids {
        if let Some(path) = existing_capture(&dir, id) {
            if let Err(e) = std::fs::remove_file(&path) {
                log::warn!("could not unlink stale capture {id}: {e}");
            }
        }
    }
}

/// Profile capture ids to unlink after a profile-list edit: ids REMOVED from the
/// store, plus RETAINED ids whose `topology_id` changed (re-picking the pickup must
/// not keep leveling with the old instrument's captured DI). A rename-only edit
/// keeps the same id + topology, so its capture survives — as does an alias↔parent
/// relabel (same stimulus, e.g. Humbucker→P90), hence the CANONICAL-id compare.
pub(crate) fn captures_to_unlink(old: &Store, new: &Store) -> Vec<String> {
    let new_by_id: HashMap<&str, &Profile> =
        new.profiles.iter().map(|p| (p.id.as_str(), p)).collect();
    old.profiles
        .iter()
        .filter(|op| match new_by_id.get(op.id.as_str()) {
            None => true, // removed
            Some(np) => {
                crate::topologies::canonical_id(&np.topology_id)
                    != crate::topologies::canonical_id(&op.topology_id)
            }
        })
        .map(|op| op.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_store() -> Store {
        let mut by_slot = HashMap::new();
        by_slot.insert(11u32, "p1".to_string());
        Store {
            profiles: vec![
                Profile {
                    id: "p1".into(),
                    name: "Telecaster".into(),
                    topology_id: "guitar-singlecoil".into(),
                    calibration_lufs: None,
                },
                Profile {
                    id: "p2".into(),
                    name: "Metal Ibanez".into(),
                    topology_id: "guitar-active".into(),
                    calibration_lufs: Some(-9.5),
                },
            ],
            profile_by_slot: by_slot,
            targets: vec![Target {
                name: "Lead".into(),
                lufs: -22.0,
            }],
            playback_level: PlaybackLevel::Rehearsal,
            auto_install_updates: false,
        }
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("tmp-lvl-test-{}", std::process::id()));
        let path = dir.join("profiles.json");
        let store = sample_store();
        save_to_path(&path, &store).unwrap();
        let back = load_from_path(&path).unwrap();
        assert_eq!(store, back);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_default() {
        let path = std::env::temp_dir().join("tmp-lvl-does-not-exist-xyz/profiles.json");
        let _ = std::fs::remove_file(&path);
        assert_eq!(load_from_path(&path).unwrap(), Store::default());
    }

    #[test]
    fn default_store_ships_three_levels() {
        let store = Store::default();
        let names: Vec<&str> = store.targets.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Rhythm", "Crunch", "Lead"]);
    }

    #[test]
    fn legacy_json_without_targets_is_seeded() {
        // A store written before the `targets` field existed must migrate to the
        // three shipped defaults rather than deserializing to an empty list.
        let legacy = r#"{"profiles":[],"profile_by_slot":{}}"#;
        let store: Store = serde_json::from_str(legacy).unwrap();
        assert_eq!(store.targets, default_targets());
        // Likewise pre-playback_level stores: Stage = no compensation, so a
        // migrated store levels exactly as it did before the field existed.
        assert_eq!(store.playback_level, PlaybackLevel::Stage);
    }

    #[test]
    fn legacy_json_without_auto_install_updates_is_true() {
        // A store written before the `auto_install_updates` field existed must
        // migrate to opt-in-on (background updates default enabled), matching
        // `Store::default()`.
        let legacy = r#"{"profiles":[],"profile_by_slot":{}}"#;
        let store: Store = serde_json::from_str(legacy).unwrap();
        assert!(store.auto_install_updates);
        assert!(Store::default().auto_install_updates);
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "tmp-cap-test-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn store_capture_clipped_unlinks_and_stores_nothing() {
        let dir = tmp_dir("clip");
        let path = capture_wav_path_in(&dir, "p1");
        // Pre-existing capture on disk.
        store_capture(&dir, "p1", &[0.1, -0.1, 0.2, -0.2], false).unwrap();
        assert!(path.is_file(), "sanity: capture written");
        // A clipped run must delete it and store nothing.
        let stored = store_capture(&dir, "p1", &[0.5; 8], true).unwrap();
        assert!(!stored, "clipped run stores nothing");
        assert!(!path.is_file(), "clipped run unlinks the stale capture");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_capture_writes_48k_mono_f32() {
        let dir = tmp_dir("write");
        let samples: Vec<f32> = (0..1234).map(|i| (i as f32 * 0.001).sin() * 0.3).collect();
        let stored = store_capture(&dir, "p1", &samples, false).unwrap();
        assert!(stored);
        let path = capture_wav_path_in(&dir, "p1");
        let reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 48_000);
        assert_eq!(spec.sample_format, hound::SampleFormat::Float);
        assert_eq!(reader.len() as usize, samples.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_capture_unlink_first_replaces_content() {
        let dir = tmp_dir("replace");
        store_capture(&dir, "p1", &[0.0; 100], false).unwrap();
        store_capture(&dir, "p1", &[0.0; 42], false).unwrap();
        let path = capture_wav_path_in(&dir, "p1");
        let reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.len() as usize, 42, "second write replaces the first");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn existing_capture_presence_and_no_escape() {
        let dir = tmp_dir("exist");
        assert_eq!(existing_capture(&dir, "p1"), None);
        store_capture(&dir, "p1", &[0.1; 4], false).unwrap();
        assert!(existing_capture(&dir, "p1").is_some());
        // A traversal-shaped id must not escape the captures dir.
        let evil = existing_capture(&dir, "../evil");
        assert_eq!(evil, None);
        let evil_path = capture_wav_path_in(&dir, "../evil");
        assert!(
            evil_path.starts_with(dir.join("captures")),
            "sanitized id stays inside captures: {}",
            evil_path.display()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn captures_to_unlink_lists_removed_and_retopologized() {
        let prof = |id: &str, topo: &str| Profile {
            id: id.into(),
            name: id.into(),
            topology_id: topo.into(),
            calibration_lufs: Some(-9.0),
        };
        let old = Store {
            profiles: vec![
                prof("keep", "guitar-singlecoil"),
                prof("removed", "guitar-humbucker"),
                prof("retopo", "guitar-singlecoil"),
                prof("rename", "bass-singlecoil"),
                prof("aliaskeep", "guitar-humbucker"),
            ],
            ..Store::default()
        };
        let new = Store {
            profiles: vec![
                prof("keep", "guitar-singlecoil"),
                prof("retopo", "guitar-active"), // pickup re-picked
                prof("aliaskeep", "guitar-p90"), // alias↔parent relabel, same stimulus
                Profile {
                    name: "renamed".into(),
                    ..prof("rename", "bass-singlecoil")
                },
            ],
            ..Store::default()
        };
        let mut ids = captures_to_unlink(&old, &new);
        ids.sort();
        assert_eq!(ids, vec!["removed".to_string(), "retopo".to_string()]);
    }

    #[test]
    fn playback_offsets_compensate_bass_only_below_stage() {
        // Guitar is the spectral reference at every playback level.
        for lvl in [
            PlaybackLevel::Quiet,
            PlaybackLevel::Rehearsal,
            PlaybackLevel::Stage,
        ] {
            assert_eq!(playback_offset_lu(lvl, "guitar"), 0.0);
        }
        // Bass compensation grows as playback gets quieter; Stage is the
        // K-weighting design point (always 0, the legacy baseline).
        assert_eq!(playback_offset_lu(PlaybackLevel::Stage, "bass"), 0.0);
        assert_eq!(playback_offset_lu(PlaybackLevel::Rehearsal, "bass"), 0.5);
        assert_eq!(playback_offset_lu(PlaybackLevel::Quiet, "bass"), 1.5);
    }
}
