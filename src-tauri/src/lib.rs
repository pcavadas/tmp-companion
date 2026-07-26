//! TMP Companion — Tauri backend crate root.
//!
//! The app drives a USB-connected Fender Tone Master Pro in re-amp mode to
//! auto-level presets to a LUFS target: play a sample through the preset's DSP,
//! capture the processed USB-Out, measure LUFS, and solve the `presetLevel`
//! (one-shot open-loop) that hits the target.
//!
//! This file is the slim crate hub: the `mod` tree, the re-export seams that
//! make command/probe fns nameable at the crate root (`probe_api`, `commands`,
//! `bootstrap::run`, `e2e_server`), and the shared process state — `AppState`,
//! the `MONITOR_*` coordination statics, and `lock_ok`.

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
mod doctor;
#[cfg(feature = "e2e")]
mod e2e_server;
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
mod psd;
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
pub(crate) use probe_api::scene_jobs::{
    build_scene_jobs, is_amp_model_id, is_amp_output_level_param, prepass_scene_docs_via,
};
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
    device::*, doctor::*, edit_tools::*, held_edit::*, level_footswitch::*, level_preset::*,
    library::*, media::*, migration::*, presets::*, setlists::*, settings::*, songs::*, support::*,
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

#[cfg(feature = "e2e")]
pub(crate) use e2e_server::e2e_online;
#[cfg(feature = "e2e")]
pub(crate) use e2e_server::e2e_showcase;
#[cfg(feature = "e2e")]
pub use e2e_server::run_e2e_server;

/// Fixture invariants that must hold regardless of build features.
///
/// Deliberately NOT inside the `#[cfg(feature = "e2e")]` test module: that module
/// only compiles under `--features e2e`, and building with that feature is
/// forbidden here (it fabricates every LUFS and clobbers the production probe in
/// the shared target dir). A gate that only runs in a build nobody may make is
/// not a gate.
#[cfg(test)]
mod fixture_gates {

    /// NON-REGRESSION GATE for two defects found on a real 1.8.45 unit (2026-07-26).
    ///
    /// The e2e scenario fixtures were written with `info.product_id = "pro"`. Every
    /// preset the device itself creates uses **`tmStomp`**, and on the unit a `"pro"`
    /// preset is rejected with **"This preset was created using a newer firmware
    /// revision"** — the scene-selection ribbon refuses to open it. Any scene-related
    /// experiment or e2e step targeting these fixtures is silently invalid.
    ///
    /// The same fixtures also shared ONE `preset_id` across all four presets, which
    /// contradicts the documented invariant (`tmp-companion-data-model`: preset
    /// identity is "a UUID, unique per preset" and the join key for host-side
    /// metadata). Four presets sharing a key makes that mapping ambiguous.
    ///
    /// Skips when the fixture is absent (fresh `git worktree`), per repo convention.
    #[test]
    fn e2e_fixtures_use_device_product_id_and_unique_preset_ids() {
        let path = std::path::Path::new("../e2e/fixtures/scenario-presets.json");
        if !path.is_file() {
            eprintln!("skip: {} not present", path.display());
            return;
        }
        let raw = std::fs::read_to_string(path).expect("read fixture");
        let doc: serde_json::Value = serde_json::from_str(&raw).expect("fixture is JSON");
        let entries = doc.as_array().cloned().unwrap_or_else(|| {
            doc.get("presets")
                .and_then(|p| p.as_array())
                .cloned()
                .unwrap_or_default()
        });
        assert!(!entries.is_empty(), "no fixture presets found to check");

        let mut ids = Vec::new();
        for e in &entries {
            let Some(js) = e.get("presetJson").and_then(|v| v.as_str()) else {
                continue;
            };
            let p: serde_json::Value = serde_json::from_str(js).expect("presetJson parses");
            let info = &p["info"];
            let name = info["displayName"]
                .as_str()
                .unwrap_or("<unnamed>")
                .to_string();

            assert_eq!(
                info["product_id"].as_str(),
                Some("tmStomp"),
                "preset {name:?}: product_id must be \"tmStomp\" (what the device writes). \
             \"pro\" makes the unit report \"created using a newer firmware revision\" \
             and refuses scene selection."
            );
            let preset_id = info["preset_id"].as_str().unwrap_or_default().to_string();
            assert!(
                !preset_id.is_empty(),
                "preset {name:?}: preset_id must be present and non-empty — a missing field \
                 defaulting to \"\" would compare equal to another missing preset_id and pass \
                 the uniqueness check below vacuously"
            );
            ids.push((name, preset_id));
        }
        assert_eq!(
            ids.len(),
            entries.len(),
            "every fixture entry must expose a parseable presetJson; checked {} of {} — \
             a schema rename that made presetJson unreadable would otherwise leave `ids` \
             empty and this gate would pass vacuously",
            ids.len(),
            entries.len()
        );

        for (i, (n1, id1)) in ids.iter().enumerate() {
            for (n2, id2) in ids.iter().skip(i + 1) {
                assert_ne!(
                    id1, id2,
                    "presets {n1:?} and {n2:?} share preset_id {id1} — preset_id is the \
                 documented unique per-preset identity and the host-metadata join key"
                );
            }
        }
    }

    /// Same gate as above, applied to `backup-fixture.bin` — the OTHER committed
    /// fixture the same defect class can hide in.
    ///
    /// `backup_read::tests::scenario_fixture_matches_scenario_presets_json` (the
    /// drift lock between this file and `scenario-presets.json`) does NOT catch a
    /// stale `product_id`/`preset_id`: it compares decoded `BackupPresetRow`s, and
    /// that struct never carries either field, so two archives that disagree on
    /// them still compare equal. This test reads the raw `presetJson` column
    /// directly (mirroring `backup_read::read_backup_archive`'s own LZ4-frame +
    /// tar + `sqlite3` decode) instead of going through `BackupPresetRow`, so the
    /// two fields the drift lock is blind to are actually checked.
    #[test]
    fn backup_fixture_uses_device_product_id_and_unique_preset_ids() {
        use std::io::Read;

        let path = std::path::Path::new("../e2e/fixtures/backup-fixture.bin");
        if !path.is_file() {
            eprintln!("skip: {} not present", path.display());
            return;
        }
        let blob = std::fs::read(path).expect("read backup-fixture.bin");

        let mut tar_bytes = Vec::new();
        lz4_flex::frame::FrameDecoder::new(std::io::Cursor::new(&blob))
            .read_to_end(&mut tar_bytes)
            .expect("LZ4-frame decode");
        let mut db_bytes = None;
        let mut ar = tar::Archive::new(std::io::Cursor::new(&tar_bytes));
        for entry in ar.entries().expect("tar entries") {
            let mut e = entry.expect("tar entry");
            let path = e
                .path()
                .expect("tar entry path")
                .to_string_lossy()
                .into_owned();
            if path == "databaseBackup" || path.ends_with("normalDb.db3") {
                let mut buf = Vec::new();
                e.read_to_end(&mut buf).expect("tar extract db");
                db_bytes = Some(buf);
            }
        }
        let db_bytes = db_bytes.expect("databaseBackup entry present");

        // Deleted on every exit (including a panic from an `expect` below), mirroring
        // `backup_read::read_backup_archive`'s own `TempDb` guard — without it a
        // failed assertion here leaks the extracted DB into the temp dir.
        struct TempDb(std::path::PathBuf);
        impl Drop for TempDb {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let db_path = std::env::temp_dir().join(format!(
            "tmp-companion-fixture-gate-{}.db3",
            std::process::id()
        ));
        std::fs::write(&db_path, &db_bytes).expect("write temp db");
        let _guard = TempDb(db_path.clone());
        let out = std::process::Command::new("sqlite3")
            .arg("-json")
            .arg(&db_path)
            .arg("SELECT displayName, presetJson FROM UserPresets")
            .output()
            .expect("run sqlite3");
        assert!(out.status.success(), "sqlite3 query failed: {out:?}");
        let rows: serde_json::Value =
            serde_json::from_slice(&out.stdout).expect("sqlite3 -json output parses");
        let rows = rows.as_array().expect("UserPresets rows");
        assert!(!rows.is_empty(), "no UserPresets rows found to check");

        let mut ids = Vec::new();
        for row in rows {
            let name = row["displayName"]
                .as_str()
                .unwrap_or("<unnamed>")
                .to_string();
            let js = row["presetJson"].as_str().expect("presetJson is text");
            let p: serde_json::Value = serde_json::from_str(js).expect("presetJson parses");
            let info = &p["info"];

            assert_eq!(
                info["product_id"].as_str(),
                Some("tmStomp"),
                "preset {name:?}: product_id must be \"tmStomp\" — see \
                 e2e_fixtures_use_device_product_id_and_unique_preset_ids for why"
            );
            let preset_id = info["preset_id"].as_str().unwrap_or_default().to_string();
            assert!(
                !preset_id.is_empty(),
                "preset {name:?}: preset_id must be present and non-empty — a missing field \
                 defaulting to \"\" would compare equal to another missing preset_id and pass \
                 the uniqueness check below vacuously"
            );
            ids.push((name, preset_id));
        }
        assert_eq!(
            ids.len(),
            rows.len(),
            "every UserPresets row must expose a parseable presetJson; checked {} of {} — \
             a schema rename would otherwise leave `ids` empty and this gate would pass \
             vacuously",
            ids.len(),
            rows.len()
        );

        for (i, (n1, id1)) in ids.iter().enumerate() {
            for (n2, id2) in ids.iter().skip(i + 1) {
                assert_ne!(
                    id1, id2,
                    "presets {n1:?} and {n2:?} share preset_id {id1} in backup-fixture.bin"
                );
            }
        }
    }
}
