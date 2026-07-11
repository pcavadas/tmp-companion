//! Device connect/status/list + sample enumeration Tauri commands.
use crate::*;

#[derive(Serialize)]
pub(crate) struct AppInfo {
    name: String,
    version: String,
}

/// Frontend handshake on mount — confirms the backend is reachable.
///
/// Version comes from the Tauri config (`tauri.conf.json`), NOT
/// `CARGO_PKG_VERSION`: semantic-release only bumps `tauri.conf.json`, so
/// `Cargo.toml`'s version stays at the `0.0.0-development` placeholder even in a
/// real release build. `package_info().version` is the field that gets bumped.
#[tauri::command]
pub(crate) fn app_info<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> AppInfo {
    AppInfo {
        name: "TMP Companion".to_string(),
        version: app.package_info().version.to_string(),
    }
}

/// Result of a combined connect+discover handshake. The frontend receives both
/// the firmware version AND the active signal graph in one shot, eliminating the
/// separate `read_active_preset` round-trip that previously doubled the connect time.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectResult {
    firmware: Option<String>,
    graph: Option<session::ActiveGraph>,
}

/// Start the monitor-owned startup session and wait for its first snapshot. The
/// monitor's single handshake supplies firmware + preset list + (usually) the active
/// graph, so startup doesn't serialize separate HID sessions for connect/list/live.
///
/// IDEMPOTENT against an already-running monitor (the webview-reload case): when
/// `MONITOR_ENABLED` is already set, the live pump never re-runs its one-shot
/// handshake, so clearing the snapshot here would wait 8 s for a snapshot that can
/// never be re-produced. Instead the already-enabled path serves the cached
/// snapshot (its graph is kept current by `monitor::refresh_snapshot_graph` on
/// every field-3 push) — or, when the monitor is mid-connect/device-absent, polls
/// WITHOUT clearing so the monitor's own connect error / next snapshot surfaces.
#[tauri::command]
pub(crate) async fn connect_device(state: State<'_, AppState>) -> Result<ConnectResult, String> {
    let arc = state.session.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<ConnectResult, String> {
        if !MONITOR_ENABLED.load(SeqCst) {
            // Genuinely-disabled path (first connect / post-stop): fresh start.
            *lock_ok(&arc) = None;
            monitor::reset_startup_state();
            MONITOR_ENABLED.store(true, SeqCst);
        }
        // Shared poll: snapshot | monitor connect error | 8 s deadline. On the
        // already-enabled-with-snapshot path the first iteration returns at once.
        let t0 = std::time::Instant::now();
        let deadline = std::time::Duration::from_secs(8);
        loop {
            if let Some(snapshot) = monitor::startup_snapshot() {
                log::info!(
                    "connect_device: monitor snapshot in {} ms, firmware={:?}, graph={}",
                    t0.elapsed().as_millis(),
                    snapshot.firmware,
                    if snapshot.graph.is_some() {
                        "ok"
                    } else {
                        "none"
                    }
                );
                return Ok(ConnectResult {
                    firmware: snapshot.firmware,
                    graph: snapshot.graph,
                });
            }
            if let Some(e) = monitor::last_connect_error() {
                if !e.contains("no TMP found") && !e.contains("IOHIDDeviceSetReport failed") {
                    log::warn!("connect_device failed via monitor: {e}");
                }
                return Err(e);
            }
            if t0.elapsed() >= deadline {
                return Err("monitor startup timed out waiting for TMP handshake".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    })
    .await
    .map_err(|e| format!("connect task failed: {e}"))?
}

// (There is intentionally no `disconnect_device` command — the app has no manual
// Connect/Disconnect buttons, nothing invoked it, and a sync command that drops the
// seize can't be serialized by the device-op gate without risking a main-thread
// stall. The session is released by `with_released_seize` / `connect_device` only.)

/// Enumerate the device's "My Presets" list. Under the app-level monitor this is a
/// snapshot read (no HID, no device-op lock), so the list paints with connect. A
/// fresh-session fallback remains for monitor-disabled diagnostic contexts.
#[tauri::command]
pub(crate) async fn list_presets(state: State<'_, AppState>) -> Result<Vec<PresetEntry>, String> {
    let arc = state.session.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<PresetEntry>, String> {
        if MONITOR_ENABLED.load(SeqCst) {
            if let Some(snapshot) = monitor::startup_snapshot() {
                return Ok(snapshot.presets);
            }
            if let Some(e) = monitor::last_connect_error() {
                return Err(e);
            }
            return Err("not connected — monitor startup snapshot is not ready".to_string());
        }
        let _op = lock_device_op();
        let maybe_session = {
            let mut guard = lock_ok(&arc);
            guard.take()
        };
        if let Some(mut session) = maybe_session {
            let result = session.list_my_presets_strict();
            *lock_ok(&arc) = Some(session);
            return result;
        }
        Session::connect()?.list_my_presets_strict()
    })
    .await
    .map_err(|e| format!("list task failed: {e}"))?
}
/// A bundled stimulus sample the user can pick per preset.
#[derive(Serialize)]
pub(crate) struct SampleInfo {
    /// Display label (file stem, e.g. "humbucker").
    name: String,
    /// Absolute path passed back as `stimulus_path`.
    path: String,
}

/// List the synthetic stimulus samples bundled in the app's resource dir.
#[tauri::command]
pub(crate) fn list_samples(app: tauri::AppHandle) -> Result<Vec<SampleInfo>, String> {
    use tauri::Manager;
    let dir = app
        .path()
        .resolve("resources/samples", tauri::path::BaseDirectory::Resource)
        .map_err(|e| format!("resolve samples dir: {e}"))?;
    let mut out = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("read {dir:?}: {e}"))?;
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) == Some("wav") {
            let name = p
                .file_stem()
                .and_then(|x| x.to_str())
                .unwrap_or("?")
                .to_string();
            if let Some(path) = p.to_str() {
                out.push(SampleInfo {
                    name,
                    path: path.to_string(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}
/// Stop live-sync: clear `MONITOR_ENABLED` (the monitor drops its seize on its next
/// poll), then re-establish the persistent UI session so `list_presets` / commands
/// work as before. Idempotent. Returns the firmware version of the reclaimed session
/// (like `connect_device`), or `None` if the reconnect didn't carry it / no device.
#[tauri::command]
pub(crate) async fn stop_live_sync(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let arc = state.session.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Disable FIRST so the monitor releases its seize, THEN take the device-op gate
        // (which pauses + waits for the monitor to drop) before reconnecting the UI
        // session. Without the gate the reconnect could race the monitor's last seize.
        MONITOR_ENABLED.store(false, SeqCst);
        let _op = lock_device_op();
        *lock_ok(&arc) = None;
        let fw = match Session::connect_with_firmware() {
            Ok(s) => {
                let fw = s.firmware_version();
                *lock_ok(&arc) = Some(s);
                fw
            }
            Err(_) => None, // no device / not ready — UI session stays None (as before)
        };
        log::info!("live-sync stopped — UI session reclaimed (fw={fw:?})");
        fw
    })
    .await
    .map_err(|e| format!("stop_live_sync task failed: {e}"))
}
