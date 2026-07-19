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
    let mut s = Session::connect()?;
    s.drain_until_quiet(250, 20)?;
    let json = s
        .read_slot_preset_json(list_index + 1)?
        .ok_or_else(|| format!("no preset scene data returned for slot {}", list_index + 1))?;
    decode_preset_scenes(&json)
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
        std::thread::sleep(std::time::Duration::from_millis(RECONNECT_AFTER_MS));
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
                std::thread::sleep(std::time::Duration::from_millis(RECONNECT_AFTER_MS));
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
    // Offline e2e: decode a built fixture blob (LZ4-frame(tar(normalDb.db3)), the exact
    // device shape) through the SAME `read_backup_archive` path instead of streaming the
    // bulk backup over USB — faking that multi-chunk wire stream buys no fidelity the
    // real decode (lz4 → tar → sqlite → audiograph) doesn't already exercise.
    #[cfg(feature = "e2e")]
    if let Ok(path) = std::env::var("TMP_E2E_BACKUP_FIXTURE") {
        let blob = std::fs::read(&path).map_err(|e| format!("e2e backup fixture {path}: {e}"))?;
        return read_backup_archive(&blob);
    }
    use tauri::Emitter;
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?;
        let (blob, _stats) = s.device_backup(60, move |p| {
            let _ = app.emit("tmp://backup-progress", p);
        })?;
        drop(s); // release the HID seize before host-side decode
        read_backup_archive(&blob)
    })
    .await
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
