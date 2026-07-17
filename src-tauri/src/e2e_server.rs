//! Offline-UI-e2e backend (`--features e2e`): a windowless MockRuntime app whose REAL
//! commands are invoked over HTTP by the Playwright `bridge-client`. The transport
//! factory routes every device open to a shared `SimDevice`, a fixture startup snapshot
//! makes the app appear connected (no monitor thread), and the bulk backup is served
//! from the built fixture blob — so the real React UI in Chromium drives the real Rust
//! backend down to the (faked) unit. No window, no HTTP-framework dependency: a localhost
//! `std::net` server wrapping `tauri::test::get_ipc_response`. Request/response only —
//! the V1 Copy/Level journeys complete on the command's return value, not on Channels.
//! The one source of truth for the e2e mode: `TMP_E2E_ONLINE` set ⇒ drive the REAL device
//! (no SimDevice factory, real re-amp, real device backup); unset ⇒ the offline fake. Read
//! by `run_e2e_server`, the `/sim/reset` guard, and `audio::reamp_capture`.

use crate::*;

#[cfg(feature = "e2e")]
pub(crate) fn e2e_online() -> bool {
    std::env::var("TMP_E2E_ONLINE").is_ok()
}

/// SHOWCASE mode (`TMP_E2E_SHOWCASE=1`, the marketing-screenshot tour): serves the curated
/// `e2e/fixtures/showcase/` library AND lets `doctor_check` inject curated `SoundProfile`s
/// (`doctor::showcase_profile`) so the Doctor Results page renders real diagnoses instead of
/// the offline "All clear" — the offline fake capture is a stimulus passthrough, so every
/// sound would otherwise measure identically. Read only in the offline tier.
#[cfg(feature = "e2e")]
pub(crate) fn e2e_showcase() -> bool {
    std::env::var("TMP_E2E_SHOWCASE").is_ok()
}

#[cfg(feature = "e2e")]
pub fn run_e2e_server() {
    use std::net::TcpListener;

    let online = e2e_online();
    // OFFLINE only: default the backup fixture so `read_library_via_backup` decodes it
    // through the real backup path. ONLINE must stream the REAL device backup, so the var
    // must be UNSET — affirmatively CLEAR it (don't just skip the default), or a stale
    // `TMP_E2E_BACKUP_FIXTURE` inherited from a prior offline shell would silently divert
    // the online tier to the fixture instead of the plugged-in unit's real library.
    if online {
        std::env::remove_var("TMP_E2E_BACKUP_FIXTURE");
    } else if std::env::var("TMP_E2E_BACKUP_FIXTURE").is_err() {
        std::env::set_var(
            "TMP_E2E_BACKUP_FIXTURE",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../e2e/fixtures/backup-fixture.bin"
            ),
        );
    }
    // The leveling stimulus (MockRuntime can't resolve bundle resources) — a committed WAV.
    if std::env::var("TMP_E2E_STIMULUS").is_err() {
        std::env::set_var(
            "TMP_E2E_STIMULUS",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/resources/samples/guitar-humbucker.wav"
            ),
        );
    }
    // ONLINE (`TMP_E2E_ONLINE=1`): drive the REAL device — no transport factory, so every
    // Session opens real `Hid`. One real handshake seeds the startup snapshot so
    // connect/list serve it (no Wry-typed monitor on the MockRuntime). The default OFFLINE
    // path installs the `SimDevice` factory + fixture snapshot instead. The server keeps
    // serving either way (a device-absent online run surfaces the error to the spec).
    if online {
        match e2e_seed_online_snapshot() {
            Ok(()) => eprintln!("e2e_server: ONLINE — seeded snapshot from the real device"),
            Err(e) => eprintln!("e2e_server: ONLINE — device handshake failed: {e}"),
        }
    } else {
        e2e_install_offline_fake();
    }

    let app = tauri::test::mock_builder()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            app_info,
            connect_device,
            list_presets,
            read_library_via_backup,
            copy_apply,
            cancel_copy_apply,
            get_store,
            set_auto_install_updates,
            level_preset,
            doctor_check,
            cancel_doctor_check,
            doctor_apply,
            doctor_save,
            doctor_discard,
            list_songs,
            read_setlists,
            add_song,
            rename_song,
            remove_song,
            create_song_full,
            update_song_full,
            list_setlist_songs,
            add_setlist,
            rename_setlist,
            remove_setlist,
            add_setlist_songs,
            remove_setlist_song,
            move_setlist_song,
            e2e_seed_scenario,
            e2e_mark_seeded,
            e2e_clear_strays,
            e2e_clear_preset,
            e2e_load_preset,
            e2e_reamp_off
        ])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build e2e mock app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("build e2e webview");

    let port: u16 = std::env::var("TMP_E2E_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(7600);
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind e2e port");
    let mode = if online {
        "ONLINE / real device"
    } else {
        "offline / SimDevice"
    };
    eprintln!("e2e_server: listening on http://127.0.0.1:{port} ({mode})");
    // Single-threaded serial accept: Playwright runs `workers:1` (the device is
    // exclusive-seize), and the webview handle stays on this one thread.
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        e2e_handle_conn(&webview, &mut stream);
    }
}

/// Install the offline fake: the shared `SimDevice` transport factory + a fixture startup
/// snapshot (keep its presets in sync with `e2e/fixtures/backup-fixture.bin` — the
/// build script lists them). Re-callable to reset device state between specs (`/sim/reset`).
#[cfg(feature = "e2e")]
fn e2e_install_offline_fake() {
    // SHOWCASE (`TMP_E2E_SHOWCASE=1`, the marketing-screenshot tour): drive the whole app
    // from the curated, non-personal `e2e/fixtures/showcase/` library instead of the
    // 3-preset test scenario. The committed `.bin` (built from `showcase.json` by the
    // `build_showcase_fixture` generator) is the SAME device-backup shape, so `read_*`
    // decode it unchanged; we just point the env there, derive the preset list + hero graph
    // from it, and seed the curated song/setlist names. No test-gate path touches this.
    if std::env::var("TMP_E2E_SHOWCASE").is_ok() {
        e2e_install_showcase();
        return;
    }
    let sim = crate::sim_device::SimDevice::new();
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sim.clone())));
    // The 3 scenario presets at slots 400/401/402 — same slots the online tier seeds by
    // cloning, and the same presets baked into the backup fixture, so one set of specs
    // runs in both modes. `ensureScenario` finds them present offline and skips seeding.
    let presets = vec![
        session::PresetEntry {
            slot: 400,
            name: "E2E Reference".into(),
        },
        session::PresetEntry {
            slot: 401,
            name: "E2E Target 1".into(),
        },
        session::PresetEntry {
            slot: 402,
            name: "E2E Target 2".into(),
        },
    ];
    MONITOR_ENABLED.store(true, SeqCst);
    monitor::e2e_install_snapshot(Some("1.8.45".into()), presets, None);
}

/// Install the SHOWCASE offline fake (marketing screenshots). Points the backup-fixture
/// env at the curated `.bin`, decodes it to derive the preset list + the active preset's
/// hero graph (so the Level chain paints), and seeds the SimDevice with the curated
/// song/setlist names read from `showcase.json` (those names aren't in the decoded archive
/// result; the `.bin` carries presets + graph + song↔preset bindings). Best-effort: any
/// read failure falls back to an empty library rather than panicking the server.
#[cfg(feature = "e2e")]
fn e2e_install_showcase() {
    let bin = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../e2e/fixtures/showcase/showcase-fixture.bin"
    );
    std::env::set_var("TMP_E2E_BACKUP_FIXTURE", bin);
    let json = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../e2e/fixtures/showcase/showcase.json"
    );

    // The single curated source — parsed once (`firmware`, `activeSlot`, song/setlist names
    // come from here; presets + graph come from the `.bin`). Null on any read/parse error,
    // so the indexing below all yields empties and the server still boots.
    let spec = std::fs::read_to_string(json)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .unwrap_or(serde_json::Value::Null);
    let names = |key: &str| {
        spec[key]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        x.as_str()
                            .or_else(|| x["name"].as_str())
                            .map(str::to_string)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    // Curated song / setlist names for the live read-back (Songs tab main list).
    let sim = crate::sim_device::SimDevice::new().with_songs(names("songs"), names("setlists"));
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sim.clone())));

    // Preset list + hero graph, decoded from the same curated `.bin`.
    let (presets, graph) = match std::fs::read(bin)
        .ok()
        .and_then(|b| read_backup_archive(&b).ok())
    {
        Some(res) => {
            // PresetEntry.slot is the 0-based LIST INDEX; the DB `slot` (i64) is index + 1.
            let presets = res
                .presets
                .iter()
                .map(|r| session::PresetEntry {
                    slot: (r.slot - 1).max(0) as u32,
                    name: r.name.clone(),
                })
                .collect();
            // Hero = the `activeSlot` preset's routed graph.
            let active = spec["activeSlot"].as_u64().unwrap_or(0);
            let graph = res
                .presets
                .iter()
                .find(|r| r.slot as u64 == active)
                .map(|r| r.graph.clone());
            (presets, graph)
        }
        None => (Vec::new(), None),
    };

    let firmware = spec["firmware"].as_str().unwrap_or("1.8.45").to_string();
    MONITOR_ENABLED.store(true, SeqCst);
    monitor::e2e_install_snapshot(Some(firmware), presets, graph);
}

/// e2e ONLINE seam: one real-device handshake → install the startup snapshot (firmware +
/// My Presets) so `connect_device` / `list_presets` serve it WITHOUT a monitor thread; no
/// transport factory is installed, so every command opens the real seized `Hid`. The
/// graph stays `None` (the hero just won't paint a live chain); the journeys don't need
/// it. Requires the device plugged in + Pro Control closed.
#[cfg(feature = "e2e")]
fn e2e_seed_online_snapshot() -> Result<(), String> {
    let mut s = session::Session::connect_with_firmware()?;
    let fw = s.firmware_version();
    let presets = s.list_my_presets()?;
    drop(s); // release the seize; commands reopen via with_released_seize
    MONITOR_ENABLED.store(true, SeqCst);
    monitor::e2e_install_snapshot(fw, presets, None);
    Ok(())
}

/// Patch ONE slot's name in the startup snapshot's preset list so the UI's snapshot-backed
/// list (the Level tab) reflects a scratch-slot clone/clear immediately. Done locally from
/// the KNOWN write rather than a device re-read — `list_my_presets` lags its own writes
/// (read-after-write propagation), so an immediate re-read installs a stale list.
#[cfg(feature = "e2e")]
fn e2e_patch_snapshot_slot(slot: u32, name: &str) {
    let Some(snap) = monitor::startup_snapshot() else {
        return;
    };
    let mut presets = snap.presets;
    if let Some(e) = presets.iter_mut().find(|p| p.slot == slot) {
        e.name = name.to_string();
    }
    monitor::e2e_install_snapshot(snap.firmware, presets, snap.graph);
}

/// ONLINE-e2e DETERMINISTIC scratch setup: sweep stray imports, then place the THREE
/// committed scenario presets (`e2e/fixtures/scenario-presets.json` — the SAME
/// presetJsons baked into the offline backup fixture) at their list indices
/// (400/401/402). The heavy lifting lives in `probe_api::seed_scenario` — shared with
/// `probe --seed-scenario`, which the RUNNER prefers (a fresh process per seed, run
/// before the server starts, dodges the in-process `0xe00002c5` open lockout that
/// aborted in-spec seeds). This command is the fallback for specs run without the
/// runner, and the offline no-op (SimDevice presets already present → per-preset skip).
#[cfg(feature = "e2e")]
#[tauri::command]
async fn e2e_seed_scenario(state: State<'_, AppState>) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        let o = probe_api::seed_scenario::seed_scenario_core()?;
        e2e_patch_swept(&o.swept);
        e2e_mark_seeded_snapshot()?;
        Ok(())
    })
    .await
}

/// Snapshot patch for slots the stray sweep freed — no device I/O.
#[cfg(feature = "e2e")]
fn e2e_patch_swept(swept: &[u32]) {
    for slot in swept {
        e2e_patch_snapshot_slot(*slot, "Empty");
    }
}

/// Patch the startup snapshot so the UI's snapshot-backed preset list shows the three
/// scenario presets at their slots — no device I/O. Called after any successful seed:
/// in-process (above) or the runner's fresh-process `probe --seed-scenario` (which
/// can't touch this process's snapshot, so the runner POSTs `e2e_mark_seeded` next).
#[cfg(feature = "e2e")]
fn e2e_mark_seeded_snapshot() -> Result<(), String> {
    for p in probe_api::seed_scenario::scenario_spec()? {
        e2e_patch_snapshot_slot(p.list_index, &p.name);
    }
    Ok(())
}

#[cfg(feature = "e2e")]
#[tauri::command]
async fn e2e_mark_seeded() -> Result<(), String> {
    e2e_mark_seeded_snapshot()
}

/// ONLINE-e2e recovery arm: sweep stray scenario imports out of the user's bank
/// (fail-closed: only exact scenario-name matches at wrong slots, off a
/// completeness-floored tolerant list). Invoked by spec teardown + the e2e.sh recovery
/// so an aborted seed can never leave test junk on the unit past the run.
#[cfg(feature = "e2e")]
#[tauri::command]
async fn e2e_clear_strays(state: State<'_, AppState>) -> Result<usize, String> {
    with_released_seize(state.session.clone(), move || {
        let swept = probe_api::seed_scenario::sweep_strays_core()?;
        e2e_patch_swept(&swept);
        Ok(swept.len())
    })
    .await
}

/// ONLINE-e2e scratch teardown: clear scratch slot `slot` (0-based list index), restoring
/// the empty state. SAFETY: refuses unless the slot currently holds `expect_name` (read in
/// the same session) — so a wrong index can never clear a real preset.
#[cfg(feature = "e2e")]
#[tauri::command]
async fn e2e_clear_preset(
    state: State<'_, AppState>,
    slot: u32,
    expect_name: String,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?;
        // Tolerant read (strict fails on back-to-back lean sessions — see
        // replace_inplace_with): a truncated list leaves the slot absent → the
        // guard below refuses (fail-closed).
        let list = s.list_my_presets()?;
        let entry = list
            .get(slot as usize)
            .ok_or_else(|| format!("slot {slot} out of range"))?;
        if entry.name != expect_name {
            return Err(format!(
                "refusing to clear slot {slot}: expected '{expect_name}', found '{}'",
                entry.name
            ));
        }
        s.clear_user_preset(slot)?;
        e2e_patch_snapshot_slot(slot, "Empty");
        Ok(())
    })
    .await
}

/// ONLINE-e2e end-of-scenario state: recall a preset (0-based list index) on the unit so
/// the test leaves it on a known preset (001 = index 0). Non-destructive (a load, no save).
#[cfg(feature = "e2e")]
#[tauri::command]
async fn e2e_load_preset(state: State<'_, AppState>, slot: u32) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        Session::connect()?.load_preset(slot)
    })
    .await
}

/// ONLINE-e2e safety teardown: disengage re-amp on a fresh connection. The re-amp latch is
/// device-side and survives the HID release, so a Level run KILLED mid-capture (a Playwright
/// timeout tearing down the server) would otherwise leave the unit input-muted. The Level
/// flow's own in-session `set_reamp_mode(false)` doesn't run on an abrupt kill — this is the
/// belt-and-braces OFF the scenario teardown calls. No-op offline (the fake never engages
/// re-amp), so it's harmless on the offline path.
#[cfg(feature = "e2e")]
#[tauri::command]
async fn e2e_reamp_off(state: State<'_, AppState>) -> Result<(), String> {
    if !e2e_online() {
        return Ok(());
    }
    with_released_seize(state.session.clone(), move || {
        Session::connect()?.set_reamp_mode(false).map(|_| ())
    })
    .await
}

/// Parse one HTTP/1.1 request and reply. Routes: `POST /invoke` (the command bridge),
/// `POST /sim/reset` (fresh device state), `GET /health`, `OPTIONS` (CORS preflight).
#[cfg(feature = "e2e")]
fn e2e_handle_conn(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    stream: &mut std::net::TcpStream,
) {
    use std::io::{BufRead, BufReader, Read, Write};

    let Ok(clone) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(clone);
    let mut req_line = String::new();
    if reader.read_line(&mut req_line).is_err() {
        return;
    }
    let mut it = req_line.split_whitespace();
    let method = it.next().unwrap_or("").to_string();
    let path = it.next().unwrap_or("").to_string();
    let mut content_len = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            return;
        }
        let t = line.trim_end();
        if t.is_empty() {
            break;
        }
        if let Some(v) = t
            .strip_prefix("Content-Length:")
            .or_else(|| t.strip_prefix("content-length:"))
        {
            content_len = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; content_len];
    if content_len > 0 && reader.read_exact(&mut body).is_err() {
        return;
    }

    let (status, payload) = e2e_route(webview, &method, &path, &body);
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: content-type\r\nAccess-Control-Allow-Methods: POST,GET,OPTIONS\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        payload.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&payload);
    let _ = stream.flush();
}

/// Map a request to `(status, json body)`. `/invoke` wraps the command result in an
/// `{ok,data}` / `{ok,error}` envelope the bridge-client unwraps into resolve/reject.
#[cfg(feature = "e2e")]
fn e2e_route(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    method: &str,
    path: &str,
    body: &[u8],
) -> (&'static str, Vec<u8>) {
    use serde_json::json;
    if method == "OPTIONS" {
        return ("200 OK", Vec::new());
    }
    match (method, path) {
        ("GET", "/health") => ("200 OK", b"{\"ok\":true}".to_vec()),
        ("POST", "/sim/reset") => {
            // ONLINE: the real device IS the state — re-installing the offline fake (a
            // SimDevice factory) would clobber it, so the reset is a no-op online.
            if !e2e_online() {
                e2e_install_offline_fake();
            }
            ("200 OK", b"{\"ok\":true}".to_vec())
        }
        ("POST", "/invoke") => {
            let req: serde_json::Value = serde_json::from_slice(body).unwrap_or(json!({}));
            let cmd = req
                .get("cmd")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let args = req.get("args").cloned().unwrap_or(json!({}));
            let request = tauri::webview::InvokeRequest {
                cmd,
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            };
            let env = match tauri::test::get_ipc_response(webview, request) {
                Ok(b) => {
                    let data = b
                        .deserialize::<serde_json::Value>()
                        .unwrap_or(serde_json::Value::Null);
                    json!({ "ok": true, "data": data })
                }
                Err(e) => json!({ "ok": false, "error": e }),
            };
            ("200 OK", serde_json::to_vec(&env).unwrap_or_default())
        }
        _ => (
            "404 Not Found",
            b"{\"ok\":false,\"error\":\"not found\"}".to_vec(),
        ),
    }
}

#[cfg(all(test, feature = "e2e"))]
#[path = "e2e_server_tests.rs"]
mod e2e_server_spike;
