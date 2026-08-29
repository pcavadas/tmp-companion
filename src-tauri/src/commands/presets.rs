//! Active-preset reads, scene scans, and preset load/move/rename/delete commands.
#![allow(clippy::too_many_arguments)]
use crate::*;

// ── Active-preset signal chain: live reads + deliberate writes ──────────────────
// The connected device is the single source of truth.
// Reads parse the field-3 partial (block strip) and the songListResponse.
// Writes are DELIBERATE — every one fires only on an explicit human click in the
// ritual UI (confirm → write → read-back verify); none ever runs unattended.

/// The active preset's signal-chain graph for the "now playing" strip
/// (blocks + routing, read live via the field-78 discovery handshake). No load —
/// reads whatever preset is currently active on the device.
#[tauri::command]
pub(crate) async fn read_active_preset(
    state: State<'_, AppState>,
) -> Result<session::ActiveGraph, String> {
    with_released_seize(state.session.clone(), move || {
        discover_active_graph().map(|(graph, _)| graph)
    })
    .await
}

/// The monitor's CURRENT cached graph — the startup snapshot's graph, which
/// `monitor::refresh_snapshot_graph` keeps current on every field-3 push. A cheap
/// no-device-I/O, no-lock read (mirrors `list_presets`'s snapshot path) that lets a
/// freshly-mounted view re-seed its hero after a graphless connect, without the
/// heavy `read_active_preset` discovery. `None` when the cache has no graph yet.
#[tauri::command]
pub(crate) async fn current_graph() -> Result<Option<session::ActiveGraph>, String> {
    Ok(monitor::startup_graph())
}

/// Scene metadata for one preset, returned by the pure-lazy field-8 read.
#[derive(Clone, Serialize)]
pub(crate) struct PresetScenes {
    pub(crate) scenes: Vec<String>,
    pub(crate) fs: Vec<Option<u32>>,
    /// Block-acting footswitches (on/off + parameter change), with leveling-candidate
    /// params — empty when the preset has none.
    footswitches: Vec<footswitch::FootswitchInfo>,
}

pub(crate) fn decode_preset_scenes(json: &[u8]) -> Result<PresetScenes, String> {
    let live = session::decode_plain_preset_live(json)
        .ok_or_else(|| "could not parse preset scene JSON".to_string())?;
    let scenes = live
        .scene_names
        .ok_or_else(|| "preset scene JSON truncated before scenes".to_string())?;
    let map = live.ftsw.as_ref().map(footswitch::scene_fs_map);
    let fs = (0..scenes.len())
        .map(|i| {
            map.as_ref()
                .and_then(|m| m.get(&(i as u32)).copied())
                .map(|sw| sw + 1)
        })
        .collect();
    // Block-acting footswitches need the FULL preset (dspUnitParameters), which the
    // ActiveGraph drops — re-parse the raw field-8 JSON (tolerant: it survives the
    // scene-tail truncation; ftsw + audioGraph are well before it).
    let footswitches = match (
        session::tolerant_parse_json(&String::from_utf8_lossy(json)),
        live.ftsw.as_ref(),
    ) {
        (Some(preset), Some(ftsw)) => footswitch::enumerate_block_footswitches(ftsw, &preset),
        _ => Vec::new(),
    };
    Ok(PresetScenes {
        scenes,
        fs,
        footswitches,
    })
}

pub(crate) fn read_preset_scenes_fresh(list_index: u32) -> Result<PresetScenes, String> {
    read_slot_scenes_raw(list_index).map(|(_, scenes)| scenes)
}

/// One fresh field-8 read returning BOTH the raw bytes (for truncation detection) and
/// the decoded scenes — the connect/drain/read incantation lives here once so the
/// fresh and complete-read paths cannot drift.
fn read_slot_scenes_raw(list_index: u32) -> Result<(Vec<u8>, PresetScenes), String> {
    let mut s = Session::connect()?;
    s.drain_until_quiet(250, 20)?;
    let json = s
        .read_slot_preset_json(list_index + 1)?
        .ok_or_else(|| format!("no preset scene data returned for slot {}", list_index + 1))?;
    let scenes = decode_preset_scenes(&json)?;
    Ok((json, scenes))
}

/// Does a field-8 [`PresetScenes`] read look like it landed on the scene-tail cut
/// (notes/gotchas.md's field-8 slot-addressed-read entry — the partial is
/// device-truncated at a PER-SLOT-DETERMINISTIC size, so retrying cannot help)? Either
/// tell fires:
/// - a repaired scene name is empty — the tolerant unwind lands mid-object, so the
///   scene survives but its `sceneName` sits past the cut, or
/// - `scenes.scenes.len()` doesn't cover every scene the doc itself REFERENCES
///   ([`footswitch::max_referenced_scene`]). It returns an INDEX, so the strict tell
///   is `len() <= max_ref` — a doc referencing index 3 needs >= 4 scenes.
///
/// Conservative in the other direction: an unparseable doc (`doc: None`) reads as NOT
/// truncated (nothing left to compare `scenes` against) rather than forcing every
/// caller through the backup fallback. `doc` is the tolerant parse of the SAME raw
/// bytes `scenes` was decoded from — parsed once by the caller and threaded through.
pub(crate) fn preset_scenes_look_truncated(
    doc: Option<&serde_json::Value>,
    scenes: &PresetScenes,
) -> bool {
    if scenes.scenes.iter().any(String::is_empty) {
        return true;
    }
    let Some(doc) = doc else {
        return false;
    };
    footswitch::max_referenced_scene(doc).is_some_and(|m| scenes.scenes.len() <= m as usize)
}

/// Build [`PresetScenes`] from the backup row matching `list_index`, guarded on BOTH
/// slot and name — see [`read_preset_scenes_complete`]'s doc comment for why. `field8_name`
/// is the field-8 partial's own `info.displayName` (read from the TRUNCATION-PROOF prefix,
/// well before the scene-tail cut). Pure (no device I/O), so it's testable without hardware.
pub(crate) fn scenes_from_backup_row(
    list_index: u32,
    field8_name: &str,
    rows: &[BackupPresetRow],
) -> Result<PresetScenes, String> {
    let device_slot = i64::from(list_index) + 1;
    let row = rows.iter().find(|r| r.slot == device_slot).ok_or_else(|| {
        format!(
            "read_preset_scenes_complete: backup has no row for slot {device_slot} \
             (field-8 name {field8_name:?})"
        )
    })?;
    if row.name != field8_name {
        return Err(format!(
            "read_preset_scenes_complete: backup row for slot {device_slot} is named \
             {:?}, but the field-8 partial for the same slot says {field8_name:?} — \
             refusing a scene list that may belong to a different preset",
            row.name
        ));
    }
    // scene_count == -1 means the backup row's presetJson didn't parse (backup_read.rs)
    // — its empty `scenes` would silently re-enter the exact under-enumeration this
    // fallback exists to eliminate, so refuse instead of returning zero scenes.
    if row.scene_count < 0 {
        return Err(format!(
            "read_preset_scenes_complete: backup row for slot {device_slot} \
             ({field8_name:?}) has an unparseable presetJson (scene_count=-1) — \
             refusing its empty scene list"
        ));
    }
    Ok(PresetScenes {
        scenes: row.scenes.iter().map(|s| s.name.clone()).collect(),
        fs: row.scenes.iter().map(|s| s.fs).collect(),
        footswitches: row.footswitches.clone(),
    })
}

/// Complete-JSON fallback for [`read_preset_scenes_fresh`]. The field-8 partial's scene
/// tail truncates at a per-slot-deterministic size — retrying the same read cannot help
/// (notes/gotchas.md's field-8 entry). When [`preset_scenes_look_truncated`] fires, this
/// instead reads the preset's COMPLETE `presetJson` off the device backup
/// ([`Session::device_backup`] + [`read_backup_archive`], the same decode
/// `read_library_via_backup` uses) — a fresh connection, since the backup is its own
/// multi-second transfer and re-amp rules keep it off any held session.
///
/// Name-guarded (danger.md's address-space rule — this enumeration feeds per-scene
/// `outputLevel` writes + saves): a second connection sits between the two reads, so the
/// backup row is accepted ONLY when its slot AND name match the field-8 partial's own
/// `info.displayName`. A scene list silently read from the WRONG preset would level the
/// wrong sounds.
pub(crate) fn read_preset_scenes_complete(list_index: u32) -> Result<PresetScenes, String> {
    let (json, scenes) = read_slot_scenes_raw(list_index)?;
    let doc = session::tolerant_parse_json(&String::from_utf8_lossy(&json));
    if !preset_scenes_look_truncated(doc.as_ref(), &scenes) {
        return Ok(scenes);
    }
    let field8_name = doc
        .as_ref()
        .and_then(|d| d.pointer("/info/displayName"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let max_ref = doc.as_ref().and_then(footswitch::max_referenced_scene);
    log::warn!(
        "read_preset_scenes_complete: slot {} {field8_name:?} scene list truncated by the \
         field-8 partial ({} of {} scenes) — reading the complete list via device backup",
        list_index + 1,
        scenes.scenes.len(),
        max_ref
            .map(|m| format!("≥{}", m + 1))
            .unwrap_or_else(|| "?".to_string()),
    );

    let mut s = Session::connect()?;
    let (blob, _stats) = s.device_backup(60, |_| {})?;
    drop(s);
    let backup = read_backup_archive(&blob)?;
    scenes_from_backup_row(list_index, &field8_name, &backup.presets)
}

/// Pure-lazy scene read for one preset. It never loads the preset: the command reads
/// slot-addressed field-8 plaintext JSON (`presetDataRequest` → `presetDataChanged`)
/// and decodes scene names + real footswitch tags from `ftsw`. It first tries the
/// monitor's metadata lane; when the monitor is not live it falls back to the proven
/// pause + fresh-session path.
#[tauri::command]
pub(crate) async fn read_preset_scenes(
    state: State<'_, AppState>,
    list_index: u32,
) -> Result<PresetScenes, String> {
    if let Some(result) = monitor::try_metadata_read(list_index) {
        match result {
            Ok(Some(json)) => return decode_preset_scenes(&json),
            Ok(None) => {
                log::info!("read_preset_scenes: monitor lane returned no data; falling back")
            }
            Err(e) => return Err(e),
        }
    }
    with_released_seize(state.session.clone(), move || {
        read_preset_scenes_fresh(list_index)
    })
    .await
}
/// One streamed row of the Level dialog's selected-preset scene scan. `result`
/// is `None` when the slot read went unanswered or undecodable — the dialog
/// renders that preset as scanned-with-no-scenes (block roles still level it).
#[derive(Clone, Serialize)]
pub(crate) struct SceneScanItem {
    list_index: u32,
    result: Option<PresetScenes>,
}

/// Cooperative cancel for [`scan_preset_scenes`] — set by `cancel_scene_scan`
/// ("Skip — load during the run" / closing the dialog), checked between reads.
static SCENE_SCAN_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
pub(crate) fn cancel_scene_scan() {
    SCENE_SCAN_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Batch scene scan for the Level dialog: ONE dedicated lean session reading
/// every selected preset's field-8 plaintext JSON back-to-back (the HW-proven
/// `scan_all_scenes` / `probe --scenes-passive` recipe, ~0.5 s per preset),
/// streaming each preset's scenes over `on_result` as it lands so rows render
/// progressively. NON-DESTRUCTIVE — zero LoadPreset; the device's active preset
/// only ever changes later, in the post-disclaimer leveling RUN. Per-preset
/// monitor-lane reads (`read_preset_scenes`) pay ~3× per read in heartbeat
/// contention + IPC; batches must use this instead.
#[tauri::command]
pub(crate) async fn scan_preset_scenes(
    state: State<'_, AppState>,
    list_indices: Vec<u32>,
    on_result: tauri::ipc::Channel<SceneScanItem>,
) -> Result<(), String> {
    SCENE_SCAN_CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?;
        // Drain the handshake flood before the first re-armed read (a read
        // fired mid-flood is dropped device-side — the classic 0/25).
        s.drain_until_quiet(250, 20)?;
        for &idx in &list_indices {
            if SCENE_SCAN_CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            // Per-read failures degrade to a `None` row instead of aborting the
            // sweep — one unanswered slot must not strand the remaining rows.
            let result = match s.read_slot_preset_json(idx + 1) {
                Ok(Some(json)) => decode_preset_scenes(&json).ok(),
                Ok(None) | Err(_) => None,
            };
            let _ = on_result.send(SceneScanItem {
                list_index: idx,
                result,
            });
        }
        Ok(())
    })
    .await
}

/// One row of the active preset's live scene list (`sceneListResponse`). `fs` is the
/// best-effort footswitch tag — `None` for now (FS-tag RE is out of scope; the UI
/// renders an em-dash for null). Mirrors the monitor's `tmp://scene-list` rows.
#[derive(Serialize)]
pub(crate) struct SceneListRow {
    name: String,
    fs: Option<u32>,
}

/// Fetch the ACTIVE preset's scene list on demand — `sceneListRequest` (field 126).
/// The canonical scene-row source is the monitor's field-3 decode (the preset JSON's
/// `scenes[]`, pushed on every device change AND in the connect handshake); the unit
/// pushes `sceneListResponse(125)` itself only on an actual preset SWITCH. This
/// command is a manual diagnostic top-up. Routed through `with_released_seize`
/// so it serializes via `DEVICE_OP_LOCK` (pausing the monitor) like every device op.
#[tauri::command]
pub(crate) async fn request_scene_list(
    state: State<'_, AppState>,
) -> Result<Vec<SceneListRow>, String> {
    with_released_seize(state.session.clone(), move || {
        let names = Session::connect()?.request_scene_list()?;
        Ok(names
            .into_iter()
            .map(|name| SceneListRow { name, fs: None })
            .collect())
    })
    .await
}
/// Make a preset the active one on the amp (`loadPreset`). A DELIBERATE action —
/// it switches the live tone — so it's a kebab item, never a row-tap. `list_index`
/// is 0-based; `session.load_preset` adds the device +1.
#[tauri::command]
pub(crate) async fn load_preset_on_amp(
    state: State<'_, AppState>,
    list_index: u32,
) -> Result<(), String> {
    let arc = state.session.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Fast path: while live-sync is on, fire the loadPreset on the monitor's
        // persistent session (~0.2 s) instead of the release→handshake→reconnect
        // bookend (~2 s). Falls back to the classic path when the lane isn't live.
        if let Some(r) = monitor::try_live_op(monitor::LiveOp::LoadPreset(list_index)) {
            return r;
        }
        with_released_seize_blocking(arc, move || {
            let mut s = Session::connect()?;
            s.load_preset(list_index)
        })
    })
    .await
    .map_err(|e| format!("device task failed: {e}"))?
}

/// Permanently clear a user slot (`clearUserPreset`) — DESTRUCTIVE, no undo. Goes
/// through [`guarded_clear`]: a fresh non-destructive read in the SAME 1-based
/// device-slot space must confirm the slot still holds `expect_name` before the
/// clear fires (the lesson from the off-by-one that erased real presets). The §4
/// confirm + read-back verify happen in the UI; this is the safe primitive.
#[tauri::command]
pub(crate) async fn delete_preset(
    state: State<'_, AppState>,
    list_index: u32,
    expect_name: String,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        guarded_clear(list_index, &expect_name)
    })
    .await
}

/// Reorder a user preset (`moveUserPreset`). DESTRUCTIVE to slot positions (no
/// undo). 0-based list indices; `session.move_user_preset` adds the device +1.
#[tauri::command]
pub(crate) async fn move_preset(
    state: State<'_, AppState>,
    from: u32,
    to: u32,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?;
        s.move_user_preset(from, to)
    })
    .await
}

/// Rename a preset in place: load it, `renameCurrentPreset`, then
/// `saveCurrentPreset` over its own slot (Pro Control's rename = rename + save).
/// DESTRUCTIVE (permanent) and it LOADS the slot (switches the live tone), so it's
/// a deliberate confirmed action. `list_index` is 0-based.
#[tauri::command]
pub(crate) async fn rename_save_preset(
    state: State<'_, AppState>,
    list_index: u32,
    name: String,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?;
        // Capture the target's CURRENT name so conn2 can confirm the right preset is
        // active before renaming+saving it — a dropped load would otherwise rename+save
        // a DIFFERENT preset over this slot.
        let name_before = s
            .list_my_presets()?
            .into_iter()
            .find(|p| p.slot == list_index)
            .map(|p| p.name)
            .ok_or_else(|| format!("rename target list index {list_index} out of range"))?;
        s.load_preset(list_index)?;
        drop(s);
        crate::settle(std::time::Duration::from_millis(RECONNECT_AFTER_MS));
        let mut s = Session::connect()?;
        s.confirm_active(list_index, Some(&name_before))?;
        s.rename_current_preset(&name)?;
        s.save_current_preset(list_index)
    })
    .await
}

/// Recall a scene on the device — `loadScene` (PresetMessage field 101). `scene_slot`
/// is the **0-based** `scenes[]` index within the active preset;
/// `session::BASE_SCENE_SLOT` (8) recalls the base scene (the wire constant — HW-proven
/// by the `--loadscene 1` → scenes[1] "Reverb" activegraph diff + base echoing slot 8
/// even on a 0-scene preset). The proto's `LoadScene` addresses
/// a scene of the CURRENT preset, with no preset addressing of its own. So when
/// `list_index` is `Some`, the preset is loaded first (its own connection — a load
/// and a scene-recall in the SAME connection would have the load override the
/// scene), then a fresh connection recalls the scene; when `None`, the scene is
/// recalled on whatever preset is already active. A DELIBERATE action (it switches
/// the live tone), mirroring `load_preset_on_amp`. `list_index` is 0-based;
/// `session.load_preset` adds the device +1.
#[tauri::command]
pub(crate) async fn load_scene_on_amp(
    state: State<'_, AppState>,
    list_index: Option<u32>,
    scene_slot: u32,
) -> Result<(), String> {
    let arc = state.session.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Fast path — ACTIVE-preset scene recall only (`list_index == None`, the
        // shipped UI's normal case): fire the loadScene on the monitor's live
        // session. The `Some` case keeps the classic two-connection path — a load
        // and a scene-recall in the SAME connection have the load override the
        // scene (see the doc above), and that hazard is untested on the monitor's
        // long-lived session.
        if list_index.is_none() {
            if let Some(r) = monitor::try_live_op(monitor::LiveOp::LoadScene(scene_slot)) {
                return r;
            }
        }
        with_released_seize_blocking(arc, move || {
            if let Some(idx) = list_index {
                let mut s = Session::connect()?;
                s.load_preset(idx)?;
                drop(s);
                crate::settle(std::time::Duration::from_millis(RECONNECT_AFTER_MS));
            }
            Session::connect()?.load_scene(scene_slot)
        })
    })
    .await
    .map_err(|e| format!("device task failed: {e}"))?
}
/// Read the full preset/scene library via the device backup (one `BackupRequest` →
/// tar.lz4 stream → in-memory decode). Emits `tmp://backup-progress`
/// ([`session::BackupProgress`]) as the transfer advances so the UI can drive a
/// determinate progress bar (the chunk percentage is exact). Read-only on the
/// device; nothing persists (archive in RAM, temp DB deleted). Routed through
/// `with_released_seize` so it serializes via `DEVICE_OP_LOCK` (pausing the monitor)
/// like every device op.
#[tauri::command]
pub(crate) async fn read_library_via_backup<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<BackupReadResult, String> {
    // Resolved BEFORE the `app` handle is moved into the progress-emit closure below
    // (only the `PathBuf` needs to travel in) — best-effort, `None` (logged) when the
    // config dir can't be resolved.
    let settings_path = device_settings_path(&app);

    // Offline e2e: decode a built fixture blob (LZ4-frame(tar(normalDb.db3)), the exact
    // device shape) through the SAME `read_backup_archive` path instead of streaming the
    // bulk backup over USB — faking that multi-chunk wire stream buys no fidelity the
    // real decode (lz4 → tar → sqlite → audiograph) doesn't already exercise.
    #[cfg(feature = "e2e")]
    if let Ok(path) = std::env::var("TMP_E2E_BACKUP_FIXTURE") {
        let blob = std::fs::read(&path).map_err(|e| format!("e2e backup fixture {path}: {e}"))?;
        let mut result = read_backup_archive(&blob)?;
        persist_device_settings(settings_path.as_deref(), &mut result);
        return Ok(result);
    }
    use tauri::Emitter;
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?;
        let (blob, _stats) = s.device_backup(60, move |p| {
            let _ = app.emit("tmp://backup-progress", p);
        })?;
        drop(s); // release the HID seize before host-side decode
        let mut result = read_backup_archive(&blob)?;
        persist_device_settings(settings_path.as_deref(), &mut result);
        Ok(result)
    })
    .await
}

/// `<app_config_dir>/support/device-settings.json` — where a backup read's captured
/// `settingsBackup` bytes ([`BackupReadResult::settings_bytes`]) land for a future
/// "support bundle" export. `None` (logged) when the config dir can't be resolved;
/// never fails the backup read.
pub(crate) fn device_settings_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Option<std::path::PathBuf> {
    match profiles::app_config_dir(app) {
        Ok(dir) => Some(dir.join("support").join("device-settings.json")),
        Err(e) => {
            log::warn!("read_library_via_backup: could not resolve app config dir for device-settings.json: {e}");
            None
        }
    }
}

/// Write `result.settings_bytes` (if present) to `path` (temp file + rename — mirrors
/// `profiles::store_capture`'s atomic-write pattern), then clear the field so it never
/// lingers past this call. Best-effort on both counts (no `path`, or a write failure):
/// logged, never fails the backup read.
fn persist_device_settings(path: Option<&std::path::Path>, result: &mut BackupReadResult) {
    let Some(bytes) = result.settings_bytes.take() else {
        return;
    };
    let Some(path) = path else { return };
    let write = || -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        let tmp = path.with_extension("json.part");
        std::fs::write(&tmp, &bytes).map_err(|e| format!("write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))
    };
    if let Err(e) = write() {
        log::warn!("read_library_via_backup: could not persist device-settings.json: {e}");
    }
}

/// List the user's saved blocks (`RequestAllBlockPresets` → `AllBlockPresetsResponse`).
/// Instant (rides one handshake burst, no 22 s backup). Read-only. Powers the Bulk
/// Block Edit Step-3 "Your saved blocks" palette (incl. saved dual-cabs).
///
/// The in-burst `135` read can transiently MISS on a cold/first connect (the device
/// doesn't answer that round — HW-observed: 1st cold read returned no `136`,
/// the next two succeeded). So retry independent fresh reads until the response lands
/// (mirrors [`read_song_list`]'s fail-closed retry) rather than spuriously surfacing an
/// empty saved-block palette. Each attempt early-exits the moment the `136` arrives.
#[tauri::command]
pub(crate) async fn list_saved_blocks(
    state: State<'_, AppState>,
) -> Result<Vec<SavedBlock>, String> {
    with_released_seize(state.session.clone(), move || {
        for _attempt in 0..4 {
            let mut s =
                Session::connect_with_burst_request(&proto::request_all_block_presets(Some(2)))?;
            for _ in 0..8 {
                if let Some(blob) = find_block_presets_blob(&s.push_bodies()) {
                    return parse_block_presets_map(&blob);
                }
                s.pump_collect(250)?;
            }
        }
        Err("device sent no allBlockPresetsResponse after retries".to_string())
    })
    .await
}

/// List the user's impulse responses (`UserIRListRequest` → `UserIRListResponse`).
/// Instant + read-only. Powers the Bulk Block Edit Step-3 "Your impulse responses"
/// palette. Returns an empty list when the device has no user IRs loaded.
#[tauri::command]
pub(crate) async fn list_user_irs(state: State<'_, AppState>) -> Result<Vec<UserIr>, String> {
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?; // handshake already issues userIRListRequest(batch 2)
                                         // A standalone re-send + a few pump windows in case the burst reply was missed.
        s.heartbeat()?;
        s.pump_collect(80)?;
        s.send_and_collect(&proto::userir_field2(2), 500)?;
        for _ in 0..5 {
            s.heartbeat()?;
            s.pump_collect(200)?;
        }
        let bodies = s.push_bodies();
        drop(s);
        Ok(find_user_irs(&bodies))
    })
    .await
}

#[cfg(test)]
mod truncation_fallback_tests {
    use super::*;

    fn scene_switch(slot: u64, label: &str) -> serde_json::Value {
        serde_json::json!({ "func": "scene", "sceneSlot": slot, "customLabel": label, "isActive": true })
    }

    fn decoded(raw: &[u8]) -> PresetScenes {
        decode_preset_scenes(raw).expect("well-formed fixture should decode")
    }

    #[test]
    fn empty_name_tell_fires_even_when_no_scene_index_is_underreferenced() {
        // Mid-object-cut shape: 3 scene objects, the 3rd missing `sceneName` (the
        // tolerant repair's signature — the object survives, the name doesn't). `ftsw`
        // only reaches sceneSlot 1, so the COUNT tell alone would read `false` — this
        // fixture isolates the empty-name tell.
        let raw = serde_json::json!({
            "ftsw": [[scene_switch(0, "Dirt")], [scene_switch(1, "Crunch")]],
            "lastLoadedScene": 0,
            "scenes": [{"sceneName": "Dirt"}, {"sceneName": "Crunch"}, {}],
        })
        .to_string();
        let scenes = decoded(raw.as_bytes());
        assert_eq!(scenes.scenes, vec!["Dirt", "Crunch", ""]);
        let doc = session::tolerant_parse_json(&raw);
        assert!(preset_scenes_look_truncated(doc.as_ref(), &scenes));
    }

    #[test]
    fn referenced_index_beyond_scenes_len_fires_with_every_name_present() {
        // 3 complete names (no empty-name tell), but a footswitch references
        // sceneSlot 3 — an INDEX that needs >= 4 scenes. HBE ANATOMY's own shape: the
        // 4th scene (referenced by its footswitch) is missing entirely, not merely
        // unnamed.
        let raw = serde_json::json!({
            "ftsw": [[scene_switch(3, "Clean")]],
            "lastLoadedScene": 0,
            "scenes": [
                {"sceneName": "Dirt"}, {"sceneName": "Crunch"}, {"sceneName": "Solo"}
            ],
        })
        .to_string();
        let scenes = decoded(raw.as_bytes());
        assert!(scenes.scenes.iter().all(|n| !n.is_empty()));
        let doc = session::tolerant_parse_json(&raw);
        assert!(preset_scenes_look_truncated(doc.as_ref(), &scenes));
    }

    #[test]
    fn complete_four_scene_doc_is_not_truncated() {
        // `lastLoadedScene: 8` is the wire BASE sentinel, not a `scenes[]` index — a
        // preset saved in base is a common real state, and if the BASE_SCENE_SLOT
        // exclusion in `footswitch::max_referenced_scene` regressed, 8 would outrank
        // the real max (3) and this 4-scene, complete doc would misread as truncated.
        let raw = serde_json::json!({
            "ftsw": [[scene_switch(3, "Clean")]],
            "lastLoadedScene": 8,
            "scenes": [
                {"sceneName": "Dirt"}, {"sceneName": "Crunch"},
                {"sceneName": "Solo"}, {"sceneName": "Clean"}
            ],
        })
        .to_string();
        let scenes = decoded(raw.as_bytes());
        assert_eq!(scenes.scenes.len(), 4);
        let doc = session::tolerant_parse_json(&raw);
        assert!(!preset_scenes_look_truncated(doc.as_ref(), &scenes));
    }

    fn backup_row(slot: i64, name: &str, scenes: Vec<SceneInfo>) -> BackupPresetRow {
        BackupPresetRow {
            slot,
            name: name.to_string(),
            scene_count: scenes.len() as i64,
            scenes,
            amp_candidates: Vec::new(),
            base_active_amp_count: 0,
            blocks: Vec::new(),
            graph: session::ActiveGraph::default(),
            footswitches: vec![footswitch::FootswitchInfo {
                switch: 1,
                label: "Boost".to_string(),
                link_group: None,
                functions: Vec::new(),
                level_params: Vec::new(),
                all_params: Vec::new(),
            }],
            silence_hint: None,
            scene_handles: Vec::new(),
            base_handles: Vec::new(),
        }
    }

    #[test]
    fn name_mismatch_is_refused_naming_both_names() {
        // Non-overlapping names (neither is a substring of the other) so the two
        // `contains` checks below can't both pass off a single name in the message.
        let rows = vec![backup_row(
            25,
            "Guitar Layers",
            vec![SceneInfo {
                name: "Dirt".to_string(),
                fs: Some(1),
            }],
        )];
        let err = match scenes_from_backup_row(24, "HBE ANATOMY", &rows) {
            Ok(_) => panic!("mismatched name must be refused"),
            Err(e) => e,
        };
        assert!(err.contains("Guitar Layers"), "{err}");
        assert!(err.contains("HBE ANATOMY"), "{err}");
    }

    #[test]
    fn no_matching_slot_is_refused() {
        let rows = vec![backup_row(3, "Other Preset", vec![])];
        let err = match scenes_from_backup_row(24, "HBE ANATOMY", &rows) {
            Ok(_) => panic!("no row for the slot must be refused"),
            Err(e) => e,
        };
        assert!(err.contains("25"), "{err}"); // list_index 24 → 1-based device slot 25
    }

    #[test]
    fn unparseable_backup_row_is_refused_not_returned_as_zero_scenes() {
        // scene_count == -1 marks a backup row whose presetJson didn't parse
        // (backup_read.rs) — slot and name both match, but accepting its empty
        // `scenes` would silently re-enter the under-enumeration this fallback
        // exists to eliminate.
        let mut row = backup_row(25, "HBE ANATOMY", vec![]);
        row.scene_count = -1;
        let err = match scenes_from_backup_row(24, "HBE ANATOMY", &[row]) {
            Ok(_) => panic!("an unparseable backup row must be refused"),
            Err(e) => e,
        };
        assert!(err.contains("unparseable"), "{err}");
    }

    #[test]
    fn matching_row_maps_names_fs_and_footswitches() {
        let rows = vec![backup_row(
            25,
            "HBE ANATOMY",
            vec![
                SceneInfo {
                    name: "Dirt".to_string(),
                    fs: Some(1),
                },
                SceneInfo {
                    name: "Crunch".to_string(),
                    fs: None,
                },
                SceneInfo {
                    name: "Solo".to_string(),
                    fs: Some(3),
                },
                SceneInfo {
                    name: "Clean".to_string(),
                    fs: Some(4),
                },
            ],
        )];
        let scenes = scenes_from_backup_row(24, "HBE ANATOMY", &rows).expect("matching row");
        assert_eq!(scenes.scenes, vec!["Dirt", "Crunch", "Solo", "Clean"]);
        assert_eq!(scenes.fs, vec![Some(1), None, Some(3), Some(4)]);
        assert_eq!(scenes.footswitches.len(), 1);
        assert_eq!(scenes.footswitches[0].label, "Boost");
    }
}
