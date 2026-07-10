//! Profile store, loudness targets, playback level, and pickup-topology commands.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// A shipped pickup topology surfaced to the UI (Settings "Pickups" dropdown).
/// Mirrors the display-relevant fields of `topologies::Topology`; synth params
/// stay backend-only.
#[derive(Serialize)]
pub(crate) struct TopologyInfo {
    id: String,
    label: String,
    instrument: String,
}

/// List the shipped pickup topologies (the catalog backing instrument profiles).
/// Supersedes `list_samples` for the UI — profiles reference a topology by `id`.
#[tauri::command]
pub(crate) fn list_pickup_topologies() -> Vec<TopologyInfo> {
    topologies::TOPOLOGIES
        .iter()
        .map(|t| TopologyInfo {
            id: t.id.to_string(),
            label: t.label.to_string(),
            instrument: t.instrument.to_string(),
        })
        .collect()
}

/// Load the persisted profile store (instrument profiles + per-slot assignments).
#[tauri::command]
pub(crate) fn get_store<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<profiles::Store, String> {
    profiles::load(&app)
}

/// Replace the profile list (keeps the per-slot assignment map intact).
#[tauri::command]
pub(crate) fn save_profiles(
    app: tauri::AppHandle,
    profiles: Vec<profiles::Profile>,
) -> Result<(), String> {
    let mut store = self::profiles::load(&app)?;
    store.profiles = profiles;
    // Drop assignments that now point at a deleted profile.
    let live: std::collections::HashSet<&str> =
        store.profiles.iter().map(|p| p.id.as_str()).collect();
    store
        .profile_by_slot
        .retain(|_, id| live.contains(id.as_str()));
    self::profiles::save(&app, &store)
}

/// Replace the user's loudness targets (the named live levels edited in Settings).
#[tauri::command]
pub(crate) fn save_targets(
    app: tauri::AppHandle,
    targets: Vec<profiles::Target>,
) -> Result<(), String> {
    let mut store = self::profiles::load(&app)?;
    store.targets = targets;
    self::profiles::save(&app, &store)
}

/// Set the playback loudness leveling compensates for (Settings "Playback level").
#[tauri::command]
pub(crate) fn set_playback_level(
    app: tauri::AppHandle,
    level: profiles::PlaybackLevel,
) -> Result<(), String> {
    let mut store = self::profiles::load(&app)?;
    store.playback_level = level;
    self::profiles::save(&app, &store)
}

/// Toggle background auto-download of app updates (Settings → App updates).
/// Generic over the runtime (like `get_store`) so it also registers on the e2e
/// MockRuntime handler.
#[tauri::command]
pub(crate) fn set_auto_install_updates<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    on: bool,
) -> Result<(), String> {
    let mut store = self::profiles::load(&app)?;
    store.auto_install_updates = on;
    self::profiles::save(&app, &store)
}

/// Resolve a topology id to its bundled stimulus WAV path in the resource dir.
/// Returns an error for an unknown id or unbundled WAV.
pub(crate) fn topology_wav_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    topology_id: &str,
) -> Result<String, String> {
    use tauri::Manager;
    topologies::by_id(topology_id)
        .ok_or_else(|| format!("unknown pickup topology '{topology_id}'"))?;
    let res = app
        .path()
        .resolve(
            format!("resources/samples/{topology_id}.wav"),
            tauri::path::BaseDirectory::Resource,
        )
        .map_err(|e| format!("resolve topology wav: {e}"))?;
    res.to_str()
        .map(str::to_string)
        .ok_or_else(|| "topology wav path not UTF-8".to_string())
}
