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
        }
    }
}

/// `profiles.json` under the app config dir.
fn store_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("resolve app config dir: {e}"))?;
    Ok(dir.join("profiles.json"))
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

/// Persist the store for the running app.
pub fn save(app: &tauri::AppHandle, store: &Store) -> Result<(), String> {
    save_to_path(&store_path(app)?, store)
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
