//! TMP Companion — Tauri backend entry point.
//!
//! The app drives a USB-connected Fender Tone Master Pro in re-amp mode to
//! auto-level presets to a LUFS target via a closed loop:
//!   load preset → play sample → capture processed output → measure LUFS →
//!   adjust `presetLevel` → repeat until on target → save.
//!
//! Module layout (filled in milestone by milestone — see the plan):
//!   hid       — IOKit exclusive-seize HID transport (runloop thread)         [M1]
//!   proto     — hand-rolled FenderMessageTMS encode/decode                   [M1]
//!   session   — handshake + preset list / load / level / save / re-amp       [M1]
//!   audio     — cpal re-amp playback + capture, window alignment             [M2]
//!   lufs      — ebur128 loudness measurement                                 [M2]
//!   leveller  — the closed loop, emits progress events                       [M3]

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::State;

// Several builders/methods are exercised only from M2/M3 onward; silence
// dead-code noise until then without weakening warnings elsewhere.
#[allow(dead_code)]
mod audio;
mod audiograph;
mod audition;
mod backup;
mod backup_read;
mod blockcaps;
mod blocklib;
mod bulk_cmd;
mod bulkrun;
mod device_gate;
#[cfg(target_os = "macos")]
mod dock;
mod footswitch;
#[allow(dead_code)]
mod hid;
mod ir;
#[allow(dead_code)]
mod leveller;
mod library;
mod lint;
#[allow(dead_code)]
mod lufs;
mod migration;
mod monitor;
mod paramedit;
mod preset_io;
mod presetmeta;
mod probe_api;
mod profiles;
#[allow(dead_code)]
mod proto;
mod rename;
mod replace_inplace;
mod saved_blocks;
mod scenes;
mod search;
#[allow(dead_code)]
mod session;
#[cfg(any(test, feature = "e2e"))]
mod sim_device;
mod spectrum;
// `pub` so the `gen_samples` bin (a separate crate) can reach the shared
// catalog as `tmp_companion_lib::topologies`.
pub mod topologies;
mod variants;
mod watcher;

pub use backup_read::*;
pub(crate) use device_gate::*;
// The `probe_*` entry points (reachable as `<libcrate>::probe_xxx` for `bin/probe.rs`).
pub use probe_api::*;
// Interim seam: helpers that stayed-in-lib commands still call after the probe_api
// extraction (Phase 2). Explicit list documents the boundary until a later phase.
pub(crate) use probe_api::level::filter_amp_candidates;
pub(crate) use probe_api::scene_bench::knob_bounds;
pub(crate) use probe_api::scene_jobs::{build_scene_jobs, is_amp_output_level_param, prepass_scene_docs};
pub(crate) use probe_api::setlists::{read_setlist_list, read_setlist_songs};
pub(crate) use probe_api::slot_write::{discover_active_graph, load_then_discover_blocks};
pub(crate) use probe_api::songs::{converge_song_bpm, read_song_list, read_song_presets};
pub(crate) use probe_api::stimulus::{
    read_stimulus_calibrated, read_stimulus_calibrated_with_shortfall,
};
pub use replace_inplace::*;
pub use saved_blocks::*;

pub use session::PresetEntry;
use session::Session;
pub use session::{ActiveGraph, GraphNode, Stage};

#[macro_use]
mod commands;
mod bootstrap;
pub use bootstrap::run;
// The command modules' fns/types are crate-internal; this seam makes them nameable at
// the crate root for `bootstrap::run`'s `generate_handler!` and the e2e handler list.
// `bulk_replace`/`copy_apply`/`level_scenes` carry the wire enums/structs that were
// crate-public before the split (`CopyRepl` et al.), so their re-export stays `pub`
// to preserve that reachability (a `pub(crate)` cap would make serde-only fields read
// as dead code); the remaining modules expose only `pub(crate)` items.
pub use commands::{bulk_replace::*, copy_apply::*, level_scenes::*};
pub(crate) use commands::{
    device::*, edit_tools::*, held_edit::*, level_footswitch::*, level_preset::*, library::*,
    media::*, migration::*, presets::*, settings::*, setlists::*, songs::*,
};

/// Lock a state mutex, recovering the guard if a previous holder panicked and poisoned it
/// (`into_inner`). These mutexes guard single-writer state (the session slot, the library,
/// the run registry, the monitor caches); recovery is always the right move — a poisoned
/// `unwrap()` would otherwise brick the always-running monitor or every future device op.
/// Used at every lock site across lib.rs / monitor.rs / watcher.rs.
pub(crate) fn lock_ok<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod lock_ok_tests {
    use super::lock_ok;
    use std::sync::{Arc, Mutex};

    #[test]
    fn recovers_a_poisoned_mutex_instead_of_panicking() {
        let m = Arc::new(Mutex::new(5));
        let m2 = Arc::clone(&m);
        // Poison the mutex: a thread panics while holding the lock.
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("poison the mutex");
        })
        .join();
        assert!(m.lock().is_err(), "the mutex must be poisoned");
        // A plain .lock().unwrap() would panic here; lock_ok recovers the guard.
        assert_eq!(*lock_ok(&m), 5);
        *lock_ok(&m) = 9;
        assert_eq!(*lock_ok(&m), 9);
    }
}

/// Shared device session. `None` until the user connects. Behind an `Arc<Mutex>`
/// so blocking HID work can run off the UI thread via `spawn_blocking`.
#[derive(Default)]
pub(crate) struct AppState {
    session: Arc<Mutex<Option<Session>>>,
    /// The imported OFFLINE `.preset` library (None until `import_library`). The
    /// canonical full-preset source every bulk feature edits.
    library: Arc<Mutex<Option<library::Library>>>,
    /// Completed bulk runs, keyed by run_id, so `bulk_revert` can restore one.
    runs: Arc<Mutex<bulk_cmd::RunRegistry>>,
    /// Rendered audition clips, keyed by slot+topology, so re-auditioning
    /// skips the re-amp pass. Session-scoped (see `audition` module caveat).
    clip_cache: Arc<Mutex<audition::ClipCache>>,
}

use std::sync::atomic::{AtomicBool, Ordering::SeqCst};

/// Monitor intent: when set, the persistent device monitor (`monitor.rs`) owns the
/// idle HID seize, streams unsolicited unit pushes, and publishes the startup
/// snapshot. `connect_device` sets this after releasing any old UI session; commands
/// borrow the device through `DEVICE_OP_LOCK` + pause/ack. `stop_live_sync` is kept
/// for diagnostics/settings paths that explicitly need to reclaim a UI session.
pub(crate) static MONITOR_ENABLED: AtomicBool = AtomicBool::new(false);

/// A command (holding [`DEVICE_OP_LOCK`]) asks the persistent device monitor to
/// yield its exclusive HID seize so the command can open its own connection
/// without a `0xe00002c5` collision. Set true while a command's [`MonitorPauseGuard`]
/// is alive; cleared on its Drop. The monitor polls this every pump iteration.
pub(crate) static MONITOR_PAUSE_REQ: AtomicBool = AtomicBool::new(false);
/// The monitor has dropped its `Session` (its seize is free) in response to a pause
/// request. The command waits (bounded) for this ack before proceeding. Cleared by
/// the monitor when it resumes after the request clears.
pub(crate) static MONITOR_PAUSED_ACK: AtomicBool = AtomicBool::new(false);

/// Offline-UI-e2e backend (`--features e2e`): a windowless MockRuntime app whose REAL
/// commands are invoked over HTTP by the Playwright `bridge-client`. The transport
/// factory routes every device open to a shared `SimDevice`, a fixture startup snapshot
/// makes the app appear connected (no monitor thread), and the bulk backup is served
/// from the built fixture blob — so the real React UI in Chromium drives the real Rust
/// backend down to the (faked) unit. No window, no HTTP-framework dependency: a localhost
/// `std::net` server wrapping `tauri::test::get_ipc_response`. Request/response only —
/// the V1 Copy/Level journeys complete on the command's return value, not on Channels.
/// The one source of truth for the e2e mode: `TMP_E2E_ONLINE` set ⇒ drive the REAL device
/// (no SimDevice factory, real re-amp, real device backup); unset ⇒ the offline fake. Read
/// by `run_e2e_server`, the `/sim/reset` guard, and `audio::reamp_capture`.
#[cfg(feature = "e2e")]
pub(crate) fn e2e_online() -> bool {
    std::env::var("TMP_E2E_ONLINE").is_ok()
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
            level_preset,
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

/// ONLINE-e2e DETERMINISTIC scratch setup: import the THREE committed scenario presets
/// (`e2e/fixtures/scenario-presets.json` — the SAME presetJsons baked into the offline
/// backup fixture) into their list indices (400/401/402). So both modes run the identical
/// fixed presets, validated against known blocks, rather than a clone of whatever is on the
/// unit. Each is placed in-place via [`replace_inplace_core`] (import → land → save to slot
/// → clear the scratch landing, guarded). The target slots start EMPTY (the 400+ scratch
/// zone); [`e2e_clear_preset`] returns them to empty. Idempotent at the spec layer
/// (`ensureScenario` skips when they already exist — i.e. offline).
#[cfg(feature = "e2e")]
#[tauri::command]
async fn e2e_seed_scenario(state: State<'_, AppState>) -> Result<(), String> {
    #[derive(serde::Deserialize)]
    struct ScenarioPreset {
        #[serde(rename = "listIndex")]
        list_index: u32,
        name: String,
        #[serde(rename = "presetJson")]
        preset_json: String,
    }
    let path = std::env::var("TMP_E2E_SCENARIO_PRESETS").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../e2e/fixtures/scenario-presets.json"
        )
        .into()
    });
    let raw = std::fs::read(&path).map_err(|e| format!("scenario presets {path}: {e}"))?;
    let presets: Vec<ScenarioPreset> =
        serde_json::from_slice(&raw).map_err(|e| format!("parse scenario presets: {e}"))?;
    with_released_seize(state.session.clone(), move || {
        for (i, p) in presets.iter().enumerate() {
            // ponytail: real-TMP HID open-lockout recovery, plus a deliberate fresh-connect
            // import. replace_inplace_core does its post-import "where did it land?" list read
            // on a FRESH Session::connect() — a full handshake forces the device to
            // re-enumerate the just-imported slot. A held single session is faster but its
            // re-arm list read does NOT reflect a fresh import (read-after-write lag; the
            // device only re-enumerates inside the recognized full handshake), so the scratch
            // slot is invisible and seeding fails. So we keep the fresh-connect path and pay
            // the open-lockout tax with an 8 s quiet gap between presets, which lets the
            // device recover (offline SimDevice has no lockout, and the offline specs use the
            // baked fixture, so this only bites online). ~92 s for three presets — fine for
            // one-time e2e setup; correctness over the held-session speedup.
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_secs(8));
            }
            // A `.preset` file is `xor_jld(compact JSON)`; `import_preset` adds the outer LZ4.
            let bytes = backup::xor_jld(p.preset_json.as_bytes());
            replace_inplace_core(p.list_index, &bytes)?;
            e2e_patch_snapshot_slot(p.list_index, &p.name);
        }
        Ok(())
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
        let list = s.list_my_presets_strict()?;
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
mod e2e_server_spike {
    use super::*;
    use tauri::test::MockRuntime;
    use tauri::WebviewWindow;

    /// The transport factory + startup snapshot are process-GLOBAL; cargo runs tests in
    /// parallel, so the factory-installing tests must hold this for their whole body or
    /// they stomp each other's fake (a hard-to-spot cross-contamination).
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        lock_ok(&SERIAL)
    }

    /// Invoke a command through the SAME IPC path the HTTP bridge uses: a JSON body in,
    /// the command's JSON response out (or its error value).
    fn invoke(
        webview: &WebviewWindow<MockRuntime>,
        cmd: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, serde_json::Value> {
        tauri::test::get_ipc_response(
            webview,
            tauri::webview::InvokeRequest {
                cmd: cmd.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            },
        )
        .map(|b| b.deserialize::<serde_json::Value>().expect("json body"))
    }

    /// The full OFFLINE Copy journey driven through the real backend exactly as the UI
    /// drives it — connect → list presets → read the library → copy_apply — with the
    /// device replaced by a `SimDevice` (via the transport factory) and the bulk backup
    /// replaced by the built fixture blob. This is "UI to unit" minus the browser: every
    /// command runs for real over the mock IPC; only the USB transport + the snapshot are
    /// faked. The HTTP bridge + Playwright layer reuses this exact wiring.
    #[test]
    fn offline_copy_journey_through_real_backend() {
        use std::sync::atomic::Ordering::SeqCst;
        let _serial = serial();

        // One shared fake: every Session::connect* (command lane) clones it.
        let sim = crate::sim_device::SimDevice::new();
        let sim_for_factory = sim.clone();
        crate::session::e2e_transport::set_factory(Box::new(move || {
            Box::new(sim_for_factory.clone())
        }));
        // The library read decodes the fixture blob through the real backup path.
        std::env::set_var(
            "TMP_E2E_BACKUP_FIXTURE",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../e2e/fixtures/backup-fixture.bin"
            ),
        );
        // Pre-fill the startup snapshot so connect/list serve it with no monitor thread —
        // the 3 scenario presets at slots 400/401/402 (matching the backup fixture).
        let presets = vec![
            crate::session::PresetEntry {
                slot: 400,
                name: "E2E Reference".into(),
            },
            crate::session::PresetEntry {
                slot: 401,
                name: "E2E Target 1".into(),
            },
            crate::session::PresetEntry {
                slot: 402,
                name: "E2E Target 2".into(),
            },
        ];
        MONITOR_ENABLED.store(true, SeqCst);
        monitor::e2e_install_snapshot(Some("1.8.45".into()), presets, None);

        let app = tauri::test::mock_builder()
            .manage(AppState::default())
            .invoke_handler(tauri::generate_handler![
                connect_device,
                list_presets,
                read_library_via_backup,
                copy_apply
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .expect("build webview");

        // 1. connect → the pre-filled snapshot (firmware).
        let conn = invoke(&webview, "connect_device", serde_json::json!({})).expect("connect");
        assert_eq!(
            conn.get("firmware").and_then(|v| v.as_str()),
            Some("1.8.45")
        );

        // 2. list presets → the snapshot's 3 fixture entries.
        let list = invoke(&webview, "list_presets", serde_json::json!({})).expect("list");
        assert_eq!(list.as_array().map(|a| a.len()), Some(3), "presets: {list}");

        // 3. read the library via the fixture backup → 3 rows, decoded graphs.
        let lib =
            invoke(&webview, "read_library_via_backup", serde_json::json!({})).expect("library");
        let rows = lib
            .get("presets")
            .and_then(|p| p.as_array())
            .expect("library presets array");
        assert_eq!(rows.len(), 3, "library rows: {lib}");
        assert!(
            rows.iter()
                .any(|r| r.get("graph").is_some_and(|g| !g.is_null())),
            "at least one library row carries a decoded signal graph: {lib}"
        );

        // 4. copy_apply a dry-run replace on the target → outcome "updated", NOTHING saved.
        // The job is the exact camelCase wire shape `CopyJob`/`CopyOp`/`CopyRepl` accept
        // (the input-only structs the frontend's `diffToOps` produces). The fake confirms
        // any structural edit, so the nodeId need not match a fixture node.
        let jobs = serde_json::json!([{
            "listIndex": 401,
            "name": "E2E Target 1",
            "ops": [{
                "kind": "replace",
                "group": "G1",
                "nodeId": "ACD_PhaserP90",
                "repl": { "kind": "model", "fenderId": "ACD_KingOfTone" }
            }]
        }]);
        let items = invoke(
            &webview,
            "copy_apply",
            serde_json::json!({ "jobs": jobs, "save": false, "onResult": "__CHANNEL__:0" }),
        )
        .expect("copy_apply");
        let items = items.as_array().expect("copy items");
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].get("outcome").and_then(|v| v.as_str()),
            Some("updated"),
            "copy outcome: {items:?}"
        );
        let ev = sim.events();
        assert!(
            ev.iter()
                .any(|e| matches!(e, crate::sim_device::SimEvent::Replace { .. })),
            "the replace reached the fake: {ev:?}"
        );
        assert!(
            !ev.iter()
                .any(|e| matches!(e, crate::sim_device::SimEvent::Saved(_))),
            "dry run must not save: {ev:?}"
        );
    }

    /// The Level journey's measure→solve→apply path runs end-to-end OFFLINE: the device
    /// goes through the `SimDevice` factory and the re-amp capture through the
    /// `--features e2e` audio fake (`audio::reamp_capture` returns the stimulus), so the
    /// leveler produces a finite `C` / final level with no hardware. Proves the audio
    /// seam the UI Level run depends on; loudness fidelity stays the online tier's job.
    #[test]
    fn offline_level_preset_runs_against_the_fake_audio() {
        let _serial = serial();
        let sim = crate::sim_device::SimDevice::new();
        let sim_for_factory = sim.clone();
        crate::session::e2e_transport::set_factory(Box::new(move || {
            Box::new(sim_for_factory.clone())
        }));

        // 0.5 s of a 440 Hz tone at 48 kHz — non-silent so the loudness meter is finite.
        let rate = 48_000usize;
        let stim: Vec<f32> = (0..rate / 2)
            .map(|i| 0.2 * (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin())
            .collect();

        let opts = crate::leveller::LevelOptions {
            save: false,
            verify: true,
            ..Default::default()
        };
        let r =
            crate::leveller::level_preset(0, &stim, -30.0, opts, || false).expect("level_preset");
        assert!(
            r.final_level.is_finite() && r.final_level > 0.0,
            "solved a finite level: {r:?}"
        );
        assert!(
            r.measured_lufs.is_finite(),
            "measured a finite loudness: {r:?}"
        );
        // Dry run — the fake recorded a level set but no save.
        let ev = sim.events();
        assert!(
            ev.iter()
                .any(|e| matches!(e, crate::sim_device::SimEvent::PresetLevel(_))),
            "the level setter reached the fake: {ev:?}"
        );
        assert!(
            !ev.iter()
                .any(|e| matches!(e, crate::sim_device::SimEvent::Saved(_))),
            "save:false must not save: {ev:?}"
        );
    }

    /// Songs CRUD through the real backend over the mock IPC: the SimDevice models the
    /// song wire protocol (list / add / rename / remove), so `list_songs` reads the seed,
    /// a write mutates it, and the read-back reflects the change — the Songs tab's
    /// read-after-write contract, with no hardware.
    #[test]
    fn offline_songs_crud_through_real_backend() {
        let _serial = serial();
        let sim = crate::sim_device::SimDevice::new();
        let sim_for_factory = sim.clone();
        crate::session::e2e_transport::set_factory(Box::new(move || {
            Box::new(sim_for_factory.clone())
        }));

        let app = tauri::test::mock_builder()
            .manage(AppState::default())
            .invoke_handler(tauri::generate_handler![
                list_songs,
                read_setlists,
                add_song,
                rename_song,
                remove_song
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .expect("build webview");

        // Seed: 2 songs, 1 setlist.
        let songs = invoke(&webview, "list_songs", serde_json::json!({})).expect("list_songs");
        assert_eq!(
            songs.as_array().map(|a| a.len()),
            Some(2),
            "seed songs: {songs}"
        );
        let setlists =
            invoke(&webview, "read_setlists", serde_json::json!({})).expect("read_setlists");
        assert_eq!(
            setlists.as_array().map(|a| a.len()),
            Some(1),
            "seed setlists: {setlists}"
        );

        // Add → read-back reflects it.
        let after_add = invoke(
            &webview,
            "add_song",
            serde_json::json!({ "name": "Soundcheck" }),
        )
        .expect("add_song");
        assert_eq!(
            after_add.as_array().map(|a| a.len()),
            Some(3),
            "after add: {after_add}"
        );
        assert!(sim.song_names().iter().any(|n| n == "Soundcheck"));

        // Remove the first → back to 2.
        let after_rm = invoke(
            &webview,
            "remove_song",
            serde_json::json!({ "slot": 1, "expectName": "Opening Set" }),
        )
        .expect("remove_song");
        assert_eq!(
            after_rm.as_array().map(|a| a.len()),
            Some(2),
            "after remove: {after_rm}"
        );
        assert!(!sim.song_names().iter().any(|n| n == "Opening Set"));
    }
}
