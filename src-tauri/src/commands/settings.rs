//! Profile store, loudness targets, playback level, and pickup-topology commands.
use crate::{profiles, topologies, Serialize};

/// A shipped pickup topology surfaced to the UI (Settings "Pickups" dropdown).
/// Mirrors the display-relevant fields of `topologies::Topology`; synth params
/// stay backend-only.
#[derive(Serialize)]
pub(crate) struct TopologyInfo {
    id: String,
    label: String,
    instrument: String,
}

/// List the shipped pickup topologies (the catalog backing instrument profiles)
/// plus the pickup ALIASES (familiar names — P90, Filter'Tron… — as selectable
/// rows resolving to a parent topology's stimulus; see `topologies::ALIASES`).
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
        .chain(topologies::ALIASES.iter().map(|a| {
            // Guarded by topologies::tests::aliases_resolve — fail loudly over
            // silently dropping a picker row on a typo'd parent id.
            let parent =
                topologies::by_id(a.topology_id).expect("alias points at a shipped topology");
            TopologyInfo {
                id: a.id.to_string(),
                label: a.label.to_string(),
                instrument: parent.instrument.to_string(),
            }
        }))
        .collect()
}

/// Load the persisted profile store (instrument profiles + per-slot assignments),
/// with each profile's `has_capture` filled from the capture store so the UI can
/// show whether it levels with the player's recording or the test signal (#361).
#[tauri::command]
pub(crate) fn get_store<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<profiles::Store, String> {
    let mut store = profiles::load(&app)?;
    if let Ok(dir) = profiles::app_config_dir(&app) {
        profiles::mark_captures(&dir, &mut store);
    }
    Ok(store)
}

/// Replace the profile list (keeps the per-slot assignment map intact).
#[tauri::command]
pub(crate) fn save_profiles<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    profiles: Vec<profiles::Profile>,
) -> Result<(), String> {
    let old = self::profiles::load(&app)?;
    let mut store = old.clone();
    store.profiles = profiles;
    // Drop assignments that now point at a deleted profile (an explicit "None" stays).
    store.drop_stale_assignments();
    // Unlink calibration captures whose profile was removed or re-topologized (a
    // re-picked pickup must not keep leveling with the old instrument's DI).
    let stale = self::profiles::captures_to_unlink(&old, &store);
    self::profiles::save(&app, &store)?;
    self::profiles::unlink_captures(&app, &stale);
    Ok(())
}

/// Remember the instrument chosen for one preset (0-based list index `slot`) in the app's own
/// store on this computer — never on the Tone Master Pro. `profile_id` is an instrument
/// profile's id or `"none"`, an explicit "None" that is saved and counts as chosen; a later
/// choice overwrites it. Level's Set up and the Doctor both start from the saved value.
#[tauri::command]
pub(crate) fn set_preset_instrument<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    slot: crate::ListIndex,
    profile_id: String,
) -> Result<(), String> {
    let mut store = self::profiles::load(&app)?;
    store.set_preset_instrument(slot.0, &profile_id)?;
    self::profiles::save(&app, &store)
}

/// Replace the user's loudness targets (the named live levels edited in Settings).
#[tauri::command]
pub(crate) fn save_targets<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    targets: Vec<profiles::Target>,
) -> Result<(), String> {
    let mut store = self::profiles::load(&app)?;
    store.targets = targets;
    self::profiles::save(&app, &store)
}

/// Set the playback loudness leveling compensates for (Settings "Playback level").
#[tauri::command]
pub(crate) fn set_playback_level<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    level: profiles::PlaybackLevel,
) -> Result<(), String> {
    let mut store = self::profiles::load(&app)?;
    store.playback_level = level;
    self::profiles::save(&app, &store)
}

/// Set which output(s) the rig plays (Settings "Playback" → Output): the meter every
/// output-side measurement hub uses from the next capture on. The process-wide mode is
/// updated only AFTER the store saved, so the meter never runs ahead of what persists.
#[tauri::command]
pub(crate) fn set_playback_channels<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    channels: profiles::PlaybackChannels,
) -> Result<(), String> {
    let mut store = self::profiles::load(&app)?;
    store.playback_channels = channels;
    self::profiles::save(&app, &store)?;
    self::profiles::apply_meter_channels(&store);
    Ok(())
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

/// Resolve a topology (or alias) id to its bundled stimulus WAV path in the
/// resource dir. Returns an error for an unknown id or unbundled WAV.
pub(crate) fn topology_wav_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    topology_id: &str,
) -> Result<String, String> {
    use tauri::Manager;
    // Aliases carry no WAV of their own — the parent topology's id is the stem.
    let topo = topologies::by_id(topology_id)
        .ok_or_else(|| format!("unknown pickup topology '{topology_id}'"))?;
    let res = app
        .path()
        .resolve(
            format!("resources/samples/{}.wav", topo.id),
            tauri::path::BaseDirectory::Resource,
        )
        .map_err(|e| format!("resolve topology wav: {e}"))?;
    res.to_str()
        .map(str::to_string)
        .ok_or_else(|| "topology wav path not UTF-8".to_string())
}
