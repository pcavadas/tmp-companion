//! TMP Companion — Tauri backend crate root.
//!
//! The app drives a USB-connected Fender Tone Master Pro in re-amp mode to
//! auto-level presets to a LUFS target: play a sample through the preset's DSP,
//! capture the processed USB-Out, measure LUFS, and solve the `presetLevel`
//! (one-shot open-loop) that hits the target.
//!
//! This file is the slim crate hub: the `mod` tree, the few crate-root re-exports the
//! bins and the public types need (`bootstrap::run`, `e2e_server`, the slot and graph
//! types), and the shared process state — `AppState`, the `MONITOR_*` coordination
//! statics, and `lock_ok`. Every other module is addressed by its path; there are no
//! glob re-exports at the crate root.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::State;

// The one app command list (`app_commands.rs`) and the one way to register it. Declared
// ahead of every `mod` below so both registration sites (`bootstrap`, `e2e_server`) see
// the macros in textual scope.
#[macro_use]
mod app_commands;
#[cfg(test)]
mod command_surface_tests;

/// `tauri::generate_handler!` over every app command in `app_commands.rs`, followed by
/// `$extra` (the e2e server's `e2e_*` helpers; the app passes none).
macro_rules! app_invoke_handler {
    ($($extra:tt)*) => {
        with_app_commands!(app_invoke_handler_with!($($extra)*))
    };
}

/// The callback `app_invoke_handler!` hands to `with_app_commands!`.
macro_rules! app_invoke_handler_with {
    (($($extra:tt)*) [$($m:ident :: $f:ident),* $(,)?]) => {
        ::tauri::generate_handler![$(crate::commands::$m::$f,)* $($extra)*]
    };
}

mod audio;
mod audiograph;
mod backup;
mod backup_read;
mod benchmark;
mod blockcaps;
mod blocklib;
mod bulk_cmd;
mod bulkrun;
mod device_gate;
// Active-graph and level-block discovery, shared by the preset/leveling commands and the probe.
mod discovery;
#[cfg(target_os = "macos")]
mod dock;
mod doctor;
mod doctor_audio_oracle;
// The online-e2e scenario seed, shared by `probe --seed-scenario` and the e2e bridge server.
mod e2e_seed;
#[cfg(feature = "e2e")]
mod e2e_server;
#[cfg(feature = "e2e")]
mod e2e_ui_bridge;
mod error;
mod footswitch;
// Crash-safe writes for user data (`write_atomic`), and the app config dir for the CLI.
mod fsutil;
// Benefit-aware headroom trade + the clamp-error taxonomy (pure; the device half is in
// `leveller::trade`).
mod headroom_trade;
mod hid;
mod ir;
mod leveller;
mod library;
mod lufs;
mod monitor;
mod param_class;
mod paramedit;
mod preset_io;
mod presetmeta;
pub mod probe_api;
mod profiles;
mod proto;
mod psd;
mod replace_inplace;
mod rig;
#[cfg(test)]
mod rig_tests;
mod saved_blocks;
// Scene-leveling job planning (amp + knob classification, job build), shared by the
// scene-leveling commands, the leveller and the probe.
mod scene_jobs;
mod scenes;
mod search;
mod session;
// Setlist reads, shared by the setlist/song commands and the probe.
mod setlist_io;
#[cfg(any(test, feature = "e2e"))]
mod sim_device;
// `pub` so the `probe` bin parses and passes typed slots (`tmp_companion_lib::slot`).
pub mod slot;
mod slot_read;
// Song reads and the song BPM write, shared by the song commands, in-place replace and the probe.
mod song_io;
// Stimulus + WAV I/O and the dry-DI capture, shared by leveling, Doctor and calibration.
mod stimulus;
// Shared test-only fixtures/generators (`audio` onset tests, `leveller`
// onset-gate tests) — see the module doc.
#[cfg(test)]
pub(crate) mod test_support;
// `pub` so the `gen_samples` bin (a separate crate) can reach the shared
// catalog as `tmp_companion_lib::topologies`.
pub mod topologies;
// Lossless device traces for online e2e runs; inert unless `TMP_TRACE_DIR` is set.
// Most of it is reached only from the e2e server and the seed, so default builds see it unused.
mod trace;
// The remote tier's re-amp audio client (`TMP_E2E_REMOTE_AUDIO`): e2e builds and tests only.
#[cfg(any(feature = "e2e", test))]
mod remote_audio;
// P5 external validation: the JSON-lines expectation log the measurement seams append
// to when `TMP_E2E_VALIDATE_LOG` is set. Inert (and unallocating) otherwise.
mod validate_log;
mod watcher;

// `pub`, not `pub(crate)`: `backup_read::BackupPresetRow.scene_handles`/`.base_handles` (both
// `pub` fields) embed these — a less-visible type inside a more-visible field is a hard
// warning (`private_interfaces`).
pub use scene_jobs::{SceneHandleCandidate, SceneHandleRow};

pub use session::PresetEntry;
use session::Session;
pub use session::{ActiveGraph, GraphNode, Stage};
pub use slot::{DeviceSlot, ListIndex};

mod bootstrap;
mod commands;
pub use bootstrap::run;
// `pub` so the wire-only `fenderId` of `CopyRepl::Ir` (sent by the UI and checked by the
// Copy e2e tests, never read by the apply) is not reported as dead code.
pub use commands::copy_apply::CopyRepl;

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
}

use std::sync::atomic::{AtomicBool, Ordering::SeqCst};

/// Monitor intent: when set, the persistent device monitor (`monitor.rs`) owns the
/// idle HID seize, streams unsolicited unit pushes, and publishes the startup
/// snapshot. `connect_device` sets this after releasing any old UI session; commands
/// borrow the device through `DEVICE_OP_LOCK` + pause/ack.
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

/// A monitor THREAD actually exists in this process — set by [`monitor::spawn`].
///
/// [`MONITOR_ENABLED`] means "the monitor owns the device", which is not the same thing:
/// `e2e_server` sets it in BOTH tiers to get the reconnect skip in
/// `with_released_seize_blocking`, but it never calls `monitor::spawn` (only `bootstrap`
/// does). Waiting for [`MONITOR_PAUSED_ACK`] there waits for a thread that cannot answer,
/// so every bridged command paid the full `PAUSE_WAIT_TRIES × PAUSE_WAIT_STEP_MS` budget —
/// measured at 1.14 s for a trivial command. Gate the wait on a thread EXISTING, which is
/// the precise condition, rather than on which e2e tier is running.
pub(crate) static MONITOR_SPAWNED: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "e2e")]
pub(crate) use e2e_server::e2e_offline_fake;
#[cfg(feature = "e2e")]
pub use e2e_server::e2e_online;
#[cfg(feature = "e2e")]
pub(crate) use e2e_server::e2e_remote;
#[cfg(feature = "e2e")]
pub(crate) use e2e_server::e2e_remote_audio;
#[cfg(feature = "e2e")]
pub(crate) use e2e_server::e2e_remote_time_scale;
#[cfg(feature = "e2e")]
pub use e2e_server::e2e_remote_time_scale_label;
#[cfg(feature = "e2e")]
pub(crate) use e2e_server::e2e_showcase;
#[cfg(feature = "e2e")]
pub use e2e_server::run_e2e_server;
#[cfg(feature = "e2e")]
pub use e2e_server::{e2e_mode_refusal, e2e_remote_audio_label, e2e_remote_label};

// Fixture invariants that must hold regardless of build features (test-only).
#[cfg(test)]
mod fixture_gates;
