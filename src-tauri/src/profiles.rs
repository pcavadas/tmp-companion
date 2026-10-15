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
    /// References `topologies::Topology::id` — or an alias id
    /// (`topologies::ALIASES`); every stimulus/params lookup resolves it via
    /// `topologies::canonical_id`.
    pub topology_id: String,
    /// Tier-2 measured real output loudness (K-weighted LUFS of the dry signal);
    /// `None` until calibrated. K-weighted, not flat RMS, so it tracks how hard
    /// the instrument actually drives the amp (bright pickups aren't under-counted).
    #[serde(default)]
    pub calibration_lufs: Option<f32>,
    /// Whether a stored Tier-2 DI capture exists for this profile, so leveling
    /// re-amps the player's own recording rather than the synthetic stimulus (#361).
    /// Derived from the capture store on every `get_store` ([`mark_captures`]) and
    /// never read back: it is neither deserialized nor written while false, so the
    /// store file never carries a stale value.
    #[serde(default, skip_deserializing, skip_serializing_if = "is_false")]
    pub has_capture: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
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
/// the real contours (ISO 226) flatten as SPL rises — at stage volume the
/// low-frequency energy a bass preset lives on contributes far more perceived
/// loudness than at bedroom level. The leveler compensates by adding a per-
/// instrument LU offset to the target (see [`playback_offset_lu`]).
///
/// The one SPL K-weighting holds at is about 60 dBA: ITU-R BS.1770-5, Annex 1,
/// Attachment 1, §4.1, reports the listening tests behind it (Benjamin 2004, AES
/// preprint 6233; Soulodre 2004, Canadian Acoustics 32(3) pp. 152–153, 97
/// subjects) with the reference reproduced at 60 dBA. That is bedroom level, so
/// `Quiet` is the anchor (#242).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackLevel {
    /// Bedroom / home practice, near the 60 dBA K-weighting was fitted at — the
    /// anchor; no compensation.
    Quiet,
    /// Rehearsal volume — partway up the contour family.
    Rehearsal,
    /// Stage volume — contours flattest, so bass gets the largest cut.
    #[default]
    Stage,
}

/// Which processed output(s) the user's rig actually PLAYS — the loudness meter
/// should hear what the listener hears. The TMP's USB 1/2 pair carries the stereo
/// output; a mono PA/FRFR summing L+R, or a rig wired from output 1 alone, hears a
/// different signal whenever the preset is genuinely stereo (ping-pong delays, wide
/// reverbs, stereo-split chains). Read by every OUTPUT-side measurement hub via
/// `audio::meter_channels` (see `audio::metered_frame` for the exact meters). The
/// three modes read IDENTICALLY on a dual-mono capture, so the default changes
/// nothing; the Doctor's spectral analysis is unaffected by design.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackChannels {
    /// Both outputs (USB 1/2), metered as standard 2-ch BS.1770.
    #[default]
    Stereo,
    /// A mono rig summing both outputs: `(L+R)/2` metered as mono, +3.0103 dB.
    MonoSum,
    /// Only output 1 (left) is connected: `L` metered as mono, +3.0103 dB.
    LeftOnly,
}

/// Equal-loudness playback compensation, in LU, added to the leveling target
/// for an instrument family (`topologies::Topology::instrument`). Guitar presets
/// are the spectral reference (0 at every level — within-family leveling is
/// contour-invariant). `Quiet` is 0 for every family: it is the ~60 dBA level
/// K-weighting was fitted at (see [`PlaybackLevel`]). Louder, the ear credits
/// low-frequency energy MORE than K-weighting does, so bass presets are leveled
/// lower the louder the playback. Values are deliberately coarse (the contours
/// vary by listener and rig): the 1.5 LU span of the earlier Stage-anchored table,
/// re-anchored at Quiet (#242), so every level sits 1.5 LU lower for bass than it
/// did. A per-capture correction from the capture's own spectrum is a later step.
pub fn playback_offset_lu(level: PlaybackLevel, instrument: &str) -> f64 {
    // Bass VI shares bass's low-fundamental Fletcher–Munson character (its E1 is
    // even lower), so it takes the same compensation.
    if instrument != "bass" && instrument != "bass-vi" {
        return 0.0;
    }
    match level {
        PlaybackLevel::Quiet => 0.0,
        PlaybackLevel::Rehearsal => -1.0,
        PlaybackLevel::Stage => -1.5,
    }
}

/// The three pro-grade live levels shipped by default (and seeded into any store
/// that predates the `targets` field). Names match modeler vocab (Kemper/Helix/QC);
/// all ≤ −19 LUFS to stay under the device's ~−17 LUFS re-amp tap ceiling
/// (PR2 re-baseline: convention-adjusted from the mono-era ~−20 LUFS ceiling by
/// the +3.0103 dB dual-mono shift, pending hardware re-validation — see
/// `notes/leveling.md`'s metering-convention section).
fn default_targets() -> Vec<Target> {
    vec![
        Target {
            name: "Rhythm".into(),
            lufs: -23.0,
        },
        Target {
            name: "Crunch".into(),
            lufs: -21.0,
        },
        Target {
            name: "Lead".into(),
            lufs: -19.0,
        },
    ]
}

/// Current meter convention this store's `targets` are expressed in. `1` = 2-ch
/// BS.1770 over the processed USB pair (this PR); `0` (or the field ABSENT —
/// `#[serde(default)]`, every store written before this PR) = the mono-era
/// single-channel convention, +3.0103 dB quieter for the same audible loudness.
pub const METER_CONVENTION_STEREO: u8 = 1;

/// The exact dB a mono-era stored target needs to keep producing the SAME
/// audible loudness under the new stereo convention — see `lufs.rs`'s module
/// header for why this is algebraically exact, not just empirically measured.
const MONO_TO_STEREO_SHIFT_LU: f64 = 3.0103;

/// One-shot mono-era → stereo-era target migration (D3): if `store.meter_convention`
/// predates [`METER_CONVENTION_STEREO`], shift every stored target by
/// [`MONO_TO_STEREO_SHIFT_LU`] (rounded to 0.1 LU — the UI's own quantizer step)
/// and stamp the new convention. Idempotent: a store already AT the current
/// convention is returned unchanged (`migrated: false`), so calling this twice
/// in a row (e.g. a `load` immediately followed by a `save_targets`-style
/// load-mutate-save) never double-shifts. `calibration_lufs` is INPUT-side and
/// is never touched here.
///
/// `targets_present_in_file` is the non-obvious part: `Store::targets` is
/// `#[serde(default = "default_targets")]`, and those defaults are ALREADY
/// expressed in the CURRENT (stereo) convention (see `default_targets`'s doc
/// comment). A file that predates the `targets` field entirely has no
/// `meter_convention` key either — serde reads both as their defaults, so a
/// naive caller can't tell "genuinely mono-era targets that need shifting"
/// apart from "no targets key at all, serde just seeded current-convention
/// defaults". Shifting the latter would silently persist -20/-18/-16, above
/// `default_targets()`'s own <= -19 ceiling. So when `targets` was ABSENT
/// from the file, this only stamps the convention — never shifts — while a
/// file that DID have a `targets` key (mono-era or otherwise) shifts exactly
/// as before.
fn migrate_meter_convention(mut store: Store, targets_present_in_file: bool) -> (Store, bool) {
    if store.meter_convention >= METER_CONVENTION_STEREO {
        return (store, false);
    }
    if targets_present_in_file {
        for t in &mut store.targets {
            t.lufs = ((t.lufs + MONO_TO_STEREO_SHIFT_LU) * 10.0).round() / 10.0;
        }
    }
    store.meter_convention = METER_CONVENTION_STEREO;
    (store, true)
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
    /// Which output(s) the rig plays (Settings → Playback). Older stores default
    /// to `Stereo` = the meter every store before this field leveled with.
    #[serde(default)]
    pub playback_channels: PlaybackChannels,
    /// Auto-download updates in the background (Settings → App updates).
    #[serde(default = "default_true")]
    pub auto_install_updates: bool,
    /// See [`METER_CONVENTION_STEREO`]/[`migrate_meter_convention`]. Absent (any
    /// store written before this PR) deserializes to `0` — mono-era — via
    /// `#[serde(default)]`.
    #[serde(default)]
    pub meter_convention: u8,
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
            playback_channels: PlaybackChannels::default(),
            auto_install_updates: true,
            // A fresh install's `default_targets()` are ALREADY expressed in the
            // current (stereo) convention — stamp it directly rather than routing
            // through `migrate_meter_convention`, which would try to shift them
            // a second time.
            meter_convention: METER_CONVENTION_STEREO,
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
/// A store loaded from a REAL file runs the mono→stereo meter-convention
/// migration (D3): if it predates [`METER_CONVENTION_STEREO`], its targets are
/// shifted in memory and the migrated store is persisted back immediately, so
/// every subsequent load (this run or a future one) sees the already-migrated
/// convention and never re-shifts. A persist failure here (read-only config
/// dir, full disk, permissions) is logged and swallowed, NOT propagated: this
/// is the first load after an app update, so turning a cosmetic
/// save-back-now-vs-next-launch difference into a hard settings-load failure
/// would break the app's whole startup on what should be a soft warning —
/// the returned store is correctly migrated for this session regardless, and
/// the write is simply retried on the next `load_from_path` call.
pub fn load_from_path(path: &Path) -> Result<Store, String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            // Parsed to a raw `Value` FIRST so the migration can tell "the file
            // genuinely had a `targets` key" apart from "serde seeded
            // `default_targets()` because the key was absent" — see
            // `migrate_meter_convention`'s doc comment for why that distinction
            // is load-bearing.
            let value: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|e| format!("parse {}: {e}", path.display()))?;
            let targets_present_in_file = value.get("targets").is_some();
            let store: Store = serde_json::from_value(value)
                .map_err(|e| format!("parse {}: {e}", path.display()))?;
            let (store, migrated) = migrate_meter_convention(store, targets_present_in_file);
            if migrated {
                if let Err(e) = save_to_path(path, &store) {
                    log::warn!(
                        "meter-convention migration persist failed for {}: {e} \
                         (will retry on next load; this session's store is migrated)",
                        path.display()
                    );
                }
            }
            Ok(store)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
        Err(e) => Err(format!("read {}: {e}", path.display())),
    }
}

/// Write the store to `path` (pretty JSON), creating parent dirs as needed, through
/// [`crate::fsutil::write_atomic`]. Atomic matters here because `load_from_path`'s
/// meter-convention migration calls this AUTOMATICALLY and swallows its error: without
/// the temp-file swap, a write that died mid-flight (full disk, permissions) would leave
/// a truncated, unparseable `profiles.json` that the next launch's `load_from_path`
/// would hard-fail on, turning a cosmetic persist failure into total loss of profiles
/// and slot assignments.
pub fn save_to_path(path: &Path, store: &Store) -> Result<(), String> {
    let json = serde_json::to_string_pretty(store).map_err(|e| format!("serialize store: {e}"))?;
    crate::fsutil::write_atomic(path, json.as_bytes())
}

/// Copy `store`'s playback-channels choice into the process-wide meter mode every
/// output-side measurement hub reads (`audio::meter_channels`).
pub fn apply_meter_channels(store: &Store) {
    crate::audio::set_meter_channels(store.playback_channels);
}

/// Startup sync of the meter mode from the persisted store (the app's `setup` and the
/// e2e server's). A load failure is logged and leaves the default (Stereo) — the same
/// meter as before the setting existed — rather than failing startup.
pub fn sync_meter_channels<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    match load(app) {
        Ok(store) => apply_meter_channels(&store),
        Err(e) => log::warn!("playback-channels sync skipped (store load failed): {e}"),
    }
}

/// Load the store for the running app (config-dir resolved).
pub fn load<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<Store, String> {
    load_from_path(&store_path(app)?)
}

/// Fill each profile's derived `has_capture` from the capture store under
/// `config_dir`. Only the UI read (`get_store`) calls this, so a store that is
/// loaded, edited and saved never writes the flag back.
pub(crate) fn mark_captures(config_dir: &Path, store: &mut Store) {
    for p in &mut store.profiles {
        p.has_capture = existing_capture(config_dir, &p.id).is_some();
    }
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

/// Store a profile's calibration capture:
/// - `refused` (a take past the #361 clip severity rule) → touch nothing and return
///   `Ok(false)`. A refused take never deletes the capture already stored: a good
///   earlier take keeps leveling, and with none the synthetic stimulus stays.
/// - else write the mono 48 kHz f32 samples via temp-file + sync + rename
///   (`fsutil::commit`, which replaces any earlier capture atomically; no torn WAV
///   can pass an existence check) and return `Ok(true)`. A write failure is an
///   `Err` so the caller fails the command BEFORE persisting the scalar, and the
///   earlier capture is left as it was.
pub(crate) fn store_capture(
    config_dir: &Path,
    profile_id: &str,
    samples: &[f32],
    refused: bool,
) -> Result<bool, String> {
    if refused {
        return Ok(false);
    }
    let path = capture_wav_path_in(config_dir, profile_id);
    let parent = path
        .parent()
        .ok_or_else(|| "capture path has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    let tmp = path.with_extension("wav.part");
    let tmp_str = tmp.to_str().ok_or("capture temp path not UTF-8")?;
    crate::stimulus::write_wav_mono(tmp_str, samples, 48_000)?;
    // Synced before the rename, so a crash can't leave a renamed but empty WAV (#280).
    crate::fsutil::commit(&tmp, &path).map_err(|e| format!("store capture: {e}"))?;
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
                    has_capture: false,
                },
                Profile {
                    id: "p2".into(),
                    name: "Metal Ibanez".into(),
                    topology_id: "guitar-active".into(),
                    calibration_lufs: Some(-9.5),
                    has_capture: false,
                },
            ],
            profile_by_slot: by_slot,
            targets: vec![Target {
                name: "Lead".into(),
                lufs: -22.0,
            }],
            playback_level: PlaybackLevel::Rehearsal,
            playback_channels: PlaybackChannels::MonoSum,
            auto_install_updates: false,
            meter_convention: METER_CONVENTION_STEREO,
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
        // And pre-playback_channels stores: Stereo = the meter they leveled with.
        assert_eq!(store.playback_channels, PlaybackChannels::Stereo);
    }

    #[test]
    fn playback_channels_default_stereo_and_round_trip() {
        assert_eq!(Store::default().playback_channels, PlaybackChannels::Stereo);
        // An existing on-disk store with no `playback_channels` key loads as Stereo.
        let dir = std::env::temp_dir().join(format!("tmp-lvl-pc-test-{}", std::process::id()));
        let path = dir.join("profiles.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            &path,
            r#"{"profiles":[],"profile_by_slot":{},"targets":[],"meter_convention":1}"#,
        )
        .unwrap();
        assert_eq!(
            load_from_path(&path).unwrap().playback_channels,
            PlaybackChannels::Stereo
        );
        // Each choice persists through disk under its wire name.
        for (mode, wire) in [
            (PlaybackChannels::Stereo, "stereo"),
            (PlaybackChannels::MonoSum, "mono_sum"),
            (PlaybackChannels::LeftOnly, "left_only"),
        ] {
            assert_eq!(serde_json::to_value(mode).unwrap(), wire);
            let mut store = load_from_path(&path).unwrap();
            store.playback_channels = mode;
            save_to_path(&path, &store).unwrap();
            assert_eq!(load_from_path(&path).unwrap().playback_channels, mode);
        }
        let _ = std::fs::remove_dir_all(&dir);
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

    #[test]
    fn fresh_default_store_is_already_stamped_stereo_and_never_shifted() {
        // A brand-new install's `default_targets()` are ALREADY in the current
        // convention — `Store::default()` must stamp `meter_convention` directly,
        // not fall through a migration that would shift them a second time.
        let store = Store::default();
        assert_eq!(store.meter_convention, METER_CONVENTION_STEREO);
        assert_eq!(store.targets, default_targets());
    }

    #[test]
    fn load_from_path_does_not_shift_serde_seeded_default_targets() {
        // MAJOR data-integrity fix: `targets` is `#[serde(default =
        // "default_targets")]`, and those defaults are ALREADY expressed in
        // the current (stereo) convention. A file that predates the `targets`
        // field entirely (no key in the JSON at all) makes serde SEED
        // `default_targets()` while `meter_convention` deserializes absent →
        // 0 (mono-era). The migration must not mistake that serde-seeded,
        // already-current-convention data for a real mono-era value and shift
        // it +3.0103 dB — that would silently persist -20/-18/-16, above
        // `default_targets()`'s own documented <= -19 ceiling, pushing
        // leveling runs into clamping. The convention stamp must still be
        // written so the question is never re-asked.
        let dir = tmp_dir("migrate-no-targets-key-at-all");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("profiles.json");
        std::fs::write(&path, r#"{"profiles":[],"profile_by_slot":{}}"#).unwrap();

        let loaded = load_from_path(&path).unwrap();
        assert_eq!(
            loaded.targets,
            default_targets(),
            "serde-seeded defaults must not be shifted: {:?}",
            loaded.targets
        );
        assert_eq!(loaded.meter_convention, METER_CONVENTION_STEREO);

        // The stamp must still be persisted so the question is never re-asked,
        // even though nothing was shifted.
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(
            raw.contains("\"meter_convention\": 1"),
            "the stereo stamp must be persisted even though targets weren't shifted: {raw}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mono_era_store_migrates_targets_on_load_and_stamps_the_convention() {
        // D3: a hand-authored mono-era file (no `meterConvention` key at all —
        // `#[serde(default)]` reads it as 0) must migrate on `load_from_path`.
        let dir = tmp_dir("migrate-on-load");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("profiles.json");
        std::fs::write(
            &path,
            r#"{"targets":[{"name":"Rhythm","lufs":-26.0},{"name":"Lead","lufs":-22.0}]}"#,
        )
        .unwrap();
        let loaded = load_from_path(&path).unwrap();
        assert_eq!(loaded.meter_convention, METER_CONVENTION_STEREO);
        // -26.0 + 3.0103 = -22.9897, rounded to the UI's 0.1 LU step = -23.0.
        // -22.0 + 3.0103 = -18.9897, rounded = -19.0.
        assert!(
            (loaded.targets[0].lufs - (-23.0)).abs() < 1e-9,
            "{:?}",
            loaded.targets
        );
        assert!(
            (loaded.targets[1].lufs - (-19.0)).abs() < 1e-9,
            "{:?}",
            loaded.targets
        );
        // The migration must have persisted back to disk — re-reading the RAW
        // bytes (not `load_from_path`, which would migrate again if it hadn't)
        // shows the stamped convention.
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(
            raw.contains("\"meter_convention\": 1"),
            "migration did not persist the stamped convention: {raw}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migration_is_idempotent_across_repeated_loads() {
        // Loading an already-migrated store a second (and third) time must NOT
        // shift its targets again — the exact failure mode a `meter_convention`
        // stamp exists to prevent.
        let dir = tmp_dir("migrate-idempotent");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("profiles.json");
        std::fs::write(&path, r#"{"targets":[{"name":"Rhythm","lufs":-26.0}]}"#).unwrap();
        let once = load_from_path(&path).unwrap();
        let twice = load_from_path(&path).unwrap();
        let thrice = load_from_path(&path).unwrap();
        assert_eq!(once.targets, twice.targets);
        assert_eq!(twice.targets, thrice.targets);
        assert_eq!(thrice.meter_convention, METER_CONVENTION_STEREO);
        assert!((once.targets[0].lufs - (-23.0)).abs() < 1e-9);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migration_survives_a_save_targets_style_round_trip() {
        // The advisor-flagged real failure mode: `commands::settings::save_targets`
        // loads the WHOLE store (running the migration), overwrites ONLY `.targets`
        // with the caller's new values, then saves the WHOLE store back. If the
        // stamped `meter_convention` were dropped or not carried through that
        // wholesale overwrite, the NEXT load would re-shift the just-saved (already
        // current-convention) targets. Emulate that exact load → mutate-targets →
        // save → reload sequence with the pure path-based functions.
        let dir = tmp_dir("migrate-save-targets");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("profiles.json");
        std::fs::write(&path, r#"{"targets":[{"name":"Rhythm","lufs":-26.0}]}"#).unwrap();

        let mut store = load_from_path(&path).unwrap(); // migrates + persists
        assert_eq!(store.meter_convention, METER_CONVENTION_STEREO);
        store.targets = vec![Target {
            name: "Custom".into(),
            lufs: -19.0,
        }];
        save_to_path(&path, &store).unwrap(); // the `save_targets` command's own save

        let reloaded = load_from_path(&path).unwrap();
        assert_eq!(reloaded.meter_convention, METER_CONVENTION_STEREO);
        assert_eq!(reloaded.targets, store.targets, "no re-shift on reload");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn calibration_lufs_is_never_touched_by_the_migration() {
        // calibration_lufs is INPUT-side (Tier-2 dry-DI reference) — the migration
        // must only ever touch `targets`.
        let dir = tmp_dir("migrate-calibration-untouched");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("profiles.json");
        std::fs::write(
            &path,
            r#"{"targets":[{"name":"Rhythm","lufs":-26.0}],
                "profiles":[{"id":"p1","name":"Tele","topology_id":"guitar-singlecoil","calibration_lufs":-9.5}]}"#,
        )
        .unwrap();
        let loaded = load_from_path(&path).unwrap();
        assert_eq!(loaded.profiles[0].calibration_lufs, Some(-9.5));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migration_persist_failure_still_returns_the_migrated_store_not_an_error() {
        // Advisor-flagged regression: before the fix, `load_from_path` propagated
        // the post-migration `save_to_path` error with `?`, turning a cosmetic
        // persistence failure (read-only config dir, full disk, permissions) into
        // a HARD settings-load failure — on exactly the first launch after an
        // update, when the migration runs. Force that persist to fail and assert
        // `load_from_path` still returns Ok with the in-memory-migrated store.
        //
        // Fix 2 made `save_to_path` atomic (temp-file + rename), which needs
        // only DIRECTORY write permission, not permission on the destination
        // file itself — a read-only `profiles.json` no longer blocks a rename
        // over it. So the failure is injected into the temp-file write
        // (`FailTempWrite`), not via file or directory permissions — root
        // ignores those, and cloud sessions run as root.
        let dir = tmp_dir("migrate-persist-failure");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("profiles.json");
        std::fs::write(&path, r#"{"targets":[{"name":"Rhythm","lufs":-26.0}]}"#).unwrap();

        // Scoped so the injection is off again the instant `load_from_path`
        // returns — before any assertion or cleanup — even on a panic.
        let result = {
            let _fail = crate::fsutil::FailTempWrite::on();
            load_from_path(&path)
        };

        let store = result.expect("a failed migration persist must not fail the load");
        assert_eq!(store.meter_convention, METER_CONVENTION_STEREO);
        // -26.0 + 3.0103 = -22.9897, rounded to the UI's 0.1 LU step = -23.0 — the
        // migrated value must still come back even though it was never written.
        assert!(
            (store.targets[0].lufs - (-23.0)).abs() < 1e-9,
            "{:?}",
            store.targets
        );
        // And the on-disk bytes were genuinely NOT updated (the failure was real).
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(
            raw.contains("-26.0") && !raw.contains("meter_convention"),
            "expected the write-back to have failed, but the file changed: {raw}"
        );
        let _ = std::fs::remove_dir_all(&dir);
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
    fn store_capture_refused_keeps_the_stored_capture() {
        // #361: a take refused for clipping must not delete a good earlier capture.
        let dir = tmp_dir("clip");
        let path = capture_wav_path_in(&dir, "p1");
        let good = [0.1f32, -0.1, 0.2, -0.2];
        store_capture(&dir, "p1", &good, false).unwrap();
        assert!(path.is_file(), "sanity: capture written");
        let stored = store_capture(&dir, "p1", &[1.0; 8], true).unwrap();
        assert!(!stored, "a refused take stores nothing");
        let reader = hound::WavReader::open(&path).expect("the earlier capture survives");
        let kept: Vec<f32> = reader.into_samples::<f32>().map(Result::unwrap).collect();
        assert_eq!(kept, good, "the earlier capture is unchanged");
        // With no earlier capture, a refused take leaves none.
        let stored = store_capture(&dir, "p2", &[1.0; 8], true).unwrap();
        assert!(!stored);
        assert_eq!(existing_capture(&dir, "p2"), None);
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
    fn store_capture_replaces_content() {
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
            has_capture: false,
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
    fn playback_offsets_compensate_bass_only_above_quiet() {
        // Guitar is the spectral reference at every playback level.
        for lvl in [
            PlaybackLevel::Quiet,
            PlaybackLevel::Rehearsal,
            PlaybackLevel::Stage,
        ] {
            assert_eq!(playback_offset_lu(lvl, "guitar"), 0.0);
        }
        // Quiet is the anchor (K-weighting's ~60 dBA design level, #242); bass
        // is leveled lower as playback gets louder.
        assert_eq!(playback_offset_lu(PlaybackLevel::Quiet, "bass"), 0.0);
        assert_eq!(playback_offset_lu(PlaybackLevel::Rehearsal, "bass"), -1.0);
        assert_eq!(playback_offset_lu(PlaybackLevel::Stage, "bass"), -1.5);
        // Bass VI takes the same compensation as bass.
        assert_eq!(playback_offset_lu(PlaybackLevel::Quiet, "bass-vi"), 0.0);
        assert_eq!(
            playback_offset_lu(PlaybackLevel::Rehearsal, "bass-vi"),
            -1.0
        );
        assert_eq!(playback_offset_lu(PlaybackLevel::Stage, "bass-vi"), -1.5);
    }
}
