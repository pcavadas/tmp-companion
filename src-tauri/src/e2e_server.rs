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

/// The OFFLINE fake transport is installed in THIS process: every device "op" is an
/// in-process `SimDevice` call, so the hardware settle sleeps and the monitor pause-ack
/// wait have nothing to wait for. Armed ONLY by the two installers below.
///
/// Deliberately NOT `!e2e_online()` (what `audio::reamp_capture` uses). That predicate is
/// true in ANY `--features e2e` build with `TMP_E2E_ONLINE` unset — including
/// `cargo test --lib --features e2e`, where it would zero the settles for the whole test
/// binary. The unit tests install their fake through raw `set_factory`/`set_live`, never
/// through `e2e_install_offline_fake`, so this flag stays FALSE there.
///
/// `device_gate`'s `settles_are_full_length_unless_the_offline_fake_is_installed` is the
/// gate on exactly that, and `gates.sh` + `ci.yml` run `cargo test --lib` BOTH with and
/// without `--features e2e` so the guarded branch is actually executed somewhere — without
/// the e2e lane the guard compiles but never runs.
#[cfg(feature = "e2e")]
static OFFLINE_FAKE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Is the offline SimDevice fake installed in this process? See [`OFFLINE_FAKE`].
#[cfg(feature = "e2e")]
pub(crate) fn e2e_offline_fake() -> bool {
    OFFLINE_FAKE.load(std::sync::atomic::Ordering::SeqCst)
}

/// The scenario presets are known seeded-and-OWNERSHIP-VERIFIED for this server process:
/// set by a successful seed (in-process `e2e_seed_scenario`, or the runner's fresh-process
/// `probe --seed-scenario` via its `e2e_mark_seeded` POST) and invalidated by any scenario
/// clear. Lets every `ensureScenario` call after the first skip the multi-second (and
/// lockout-prone — the reason `scripts/e2e.sh` seeds out-of-process) in-process re-verify:
/// nothing between specs can change scenario ownership without going through a clear.
///
/// Also invalidated by [`note_structural_save`] — a same-run STRUCTURAL save (copy,
/// doctor-save) over a resident fixture slot changes what's actually on the device
/// without going through a clear, so the fast path above would otherwise assert on
/// mutilated fixture content for every spec after the one that saved.
#[cfg(feature = "e2e")]
static SCENARIO_VERIFIED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Commands whose SUCCESS persists a STRUCTURAL/body mutation over a resident fixture
/// slot — as opposed to a value-only write (e.g. leveling's `presetLevel`/`outputLevel`
/// saves), which within-run drift is deliberately tolerated via spec ORDERING (doctor.online
/// must run before level.online — see `scripts/e2e.sh`). A structural save invalidates
/// [`SCENARIO_VERIFIED`] so the NEXT spec's `ensureScenario` re-verifies the device and
/// re-imports only what drifted, rather than trusting a fixture that's since been
/// mutilated (root cause: `copy.spec.ts` stripping E2E Edge's trailing EQ block,
/// then `doctor.spec.ts` asserting on the resonance that block used to carry).
///
/// Any new command that saves structural edits to a fixture slot must join this list —
/// nothing else invalidates the fast path for it. Pinned by
/// `e2e_server_tests::note_structural_save_flags_structural_saves_only`.
#[cfg(feature = "e2e")]
const STRUCTURAL_SAVE_CMDS: [&str; 2] = ["copy_apply", "doctor_save"];

/// Clear [`SCENARIO_VERIFIED`] when `cmd` is a [`STRUCTURAL_SAVE_CMDS`] member. Call
/// ONLY after a command's invoke SUCCEEDED — an `Err` means the command aborted before
/// its save (e.g. `copy_apply` bailing before `copy_apply_one` reaches the save), so
/// nothing persisted and there is nothing to invalidate. Value-only leveling saves are
/// deliberately excluded from the set: adding them would cost a device re-verify per
/// spec inside the HID open-lockout danger window (`.claude/rules/danger.md`), and
/// within-run value drift is already handled by ordering doctor.online before level.online.
#[cfg(feature = "e2e")]
fn note_structural_save(cmd: &str) {
    if STRUCTURAL_SAVE_CMDS.contains(&cmd) {
        SCENARIO_VERIFIED.store(false, std::sync::atomic::Ordering::SeqCst);
    }
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

/// Minimal stderr logger, the twin of `probe`'s: the shared library modules
/// (leveller floor guards, the footswitch ceiling prepass, session retries)
/// diagnose through `log::*`, which is silently DROPPED without an installed
/// logger. In the app tauri-plugin-log owns it; this harness had NO logger at
/// all, so an ONLINE run's whole device-side diagnosis was invisible — the
/// `fs prepass switch=N ceiling=…` lines that explain a clamped row went
/// nowhere, and the server log carried only its two startup lines.
#[cfg(feature = "e2e")]
struct StderrLog;

#[cfg(feature = "e2e")]
impl log::Log for StderrLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }
    fn flush(&self) {}
}

#[cfg(feature = "e2e")]
pub fn run_e2e_server() {
    use std::net::TcpListener;

    if log::set_logger(&StderrLog).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }

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
            // The frontend polls this on every mount (`scheduleLibraryScan`); leaving it
            // unregistered made every call throw "Command current_graph not found" — 16
            // console errors per test, and no way for a graphless snapshot to recover.
            current_graph,
            read_library_via_backup,
            copy_apply,
            cancel_copy_apply,
            get_store,
            set_auto_install_updates,
            level_preset,
            list_level_blocks,
            level_scenes_apply_batched,
            list_scene_level_handles,
            common_reachable_target,
            cancel_scene_leveling,
            level_footswitches_apply,
            list_footswitch_scene_contexts,
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
            e2e_reamp_off,
            e2e_measure_sound
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
    // Single-threaded serial accept: the webview handle stays on this one thread. Offline
    // runs N of these PROCESSES (one per Playwright worker, `e2e/fixtures/port.ts`); the
    // parallelism is across processes, never inside one server.
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
    // Arm BEFORE the showcase branch below so both fakes are covered from here, and before
    // any device work can run: from this point every "device" op is an in-process SimDevice
    // call, so hardware settles and the monitor pause-ack have nothing to wait for.
    OFFLINE_FAKE.store(true, std::sync::atomic::Ordering::SeqCst);
    // A fresh sim device gets a fresh save registry: a witness left over from a previous
    // spec's save would make the next spec's first leveling load wait out the whole commit
    // window against a doc that can never match it (`/sim/reset` is the between-spec seam).
    crate::leveller::clear_slot_save_registry();
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
    crate::sim_device::set_live(&sim); // expose its event log to /sim/events
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sim.clone())));
    // The 10 scenario presets at slots 400-409 — same slots the online tier seeds by
    // cloning, and the same presets baked into the backup fixture, so one set of specs
    // runs in both modes. `ensureScenario` finds them present offline and skips seeding.
    // Keep this list in sync with `e2e/fixtures/scenario.ts`'s `SCENARIO` array (and the
    // twin inline snapshot `e2e_server_tests.rs` builds for its own in-process tests) —
    // nothing derives one from the other.
    let presets = vec![
        session::PresetEntry {
            slot: 400,
            name: "E2E Rig".into(),
        },
        session::PresetEntry {
            slot: 401,
            name: "E2E Pedalboard".into(),
        },
        session::PresetEntry {
            slot: 402,
            name: "E2E Edge".into(),
        },
        session::PresetEntry {
            slot: 403,
            name: "E2E Parallel".into(),
        },
        session::PresetEntry {
            slot: 404,
            name: "E2E Hiwatt 3S".into(),
        },
        session::PresetEntry {
            slot: 405,
            name: "E2E Preset24".into(),
        },
        session::PresetEntry {
            slot: 406,
            name: "E2E Combined Level".into(),
        },
        session::PresetEntry {
            slot: 407,
            name: "E2E Doctor Oracle".into(),
        },
        session::PresetEntry {
            slot: 408,
            name: "E2E Preset24 Min".into(),
        },
        session::PresetEntry {
            slot: 409,
            name: "E2E Hiwatt Min".into(),
        },
    ];
    // Hero graph, decoded from the SAME backup fixture `read_library_via_backup` already
    // serves — the showcase installer below does the identical thing for its curated `.bin`.
    // The fixture holds all 10 scenario presets (device slots 401-410, list 400-409), so
    // the hero is simply its first entry; there is no preset 001 offline to prefer.
    //
    // Not cosmetic: with the snapshot's graph `None`, the frontend's `startScanAfterGraph`
    // (`src/lib/scheduleLibraryScan.ts`) polls `current_graph` 16 × 500 ms before it will
    // start the library scan — 8 s of dead time on EVERY test, measured as essentially the
    // whole cost of `level.spec.ts`'s leveling-free "enumerates …" test.
    let graph = std::env::var("TMP_E2E_BACKUP_FIXTURE")
        .ok()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| read_backup_archive(&b).ok())
        .and_then(|res| res.presets.first().map(|r| r.graph.clone()));
    MONITOR_ENABLED.store(true, SeqCst);
    monitor::e2e_install_snapshot(Some("1.8.45".into()), presets, graph);
}

/// Install the SHOWCASE offline fake (marketing screenshots). Points the backup-fixture
/// env at the curated `.bin`, decodes it to derive the preset list + the active preset's
/// hero graph (so the Level chain paints), and seeds the SimDevice with the curated
/// song/setlist names read from `showcase.json` (those names aren't in the decoded archive
/// result; the `.bin` carries presets + graph + song↔preset bindings). Best-effort: any
/// read failure falls back to an empty library rather than panicking the server.
#[cfg(feature = "e2e")]
fn e2e_install_showcase() {
    // Also armed here (not only via `e2e_install_offline_fake`) so a direct call still
    // gets the offline timing regime — showcase installs a fake transport just the same.
    OFFLINE_FAKE.store(true, std::sync::atomic::Ordering::SeqCst);
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
fn e2e_patch_snapshot_slot(slot: u32, name: &str) -> bool {
    let Some(snap) = monitor::startup_snapshot() else {
        return false;
    };
    let mut presets = snap.presets;
    let Some(e) = presets.iter_mut().find(|p| p.slot == slot) else {
        return false;
    };
    e.name = name.to_string();
    monitor::e2e_install_snapshot(snap.firmware, presets, snap.graph);
    true
}

/// ONLINE-e2e DETERMINISTIC scratch setup: sweep stray imports, then place EVERY
/// committed scenario preset (`e2e/fixtures/scenario-presets.json` — the SAME
/// presetJsons baked into the offline backup fixture) at its list index
/// (400-409; the spec drives the slot set, nothing here hardcodes it). The heavy lifting lives in `probe_api::seed_scenario` — shared with
/// `probe --seed-scenario`, which the RUNNER prefers (a fresh process per seed, run
/// before the server starts, dodges the in-process `0xe00002c5` open lockout that
/// aborted in-spec seeds). This command is the fallback for specs run without the
/// runner, and the offline no-op (SimDevice presets already present → per-preset skip).
#[cfg(feature = "e2e")]
#[tauri::command]
async fn e2e_seed_scenario(state: State<'_, AppState>) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    if SCENARIO_VERIFIED.load(Ordering::SeqCst) {
        // Already seeded + ownership-verified this run (runner seed or a prior
        // call) and nothing has cleared since — skip the device re-verify.
        return e2e_mark_seeded_snapshot();
    }
    with_released_seize(state.session.clone(), move || {
        // Pristine self-repair is ONLINE-only (see `seed_scenario_core`): offline the
        // suite's own leveling drifts the sim's slots by design.
        let o = probe_api::seed_scenario::seed_scenario_core(e2e_online())?;
        e2e_patch_swept(&o.swept);
        e2e_mark_seeded_snapshot()?;
        SCENARIO_VERIFIED.store(true, Ordering::SeqCst);
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
    let mut missing = Vec::new();
    for p in probe_api::seed_scenario::scenario_spec()? {
        if !e2e_patch_snapshot_slot(p.list_index, &p.name) {
            missing.push(p.list_index);
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "startup snapshot doesn't cover scenario slot(s) {missing:?} — the UI preset \
             list won't show the seeded names (likely a tail-truncated snapshot list)"
        ));
    }
    Ok(())
}

#[cfg(feature = "e2e")]
#[tauri::command]
async fn e2e_mark_seeded() -> Result<(), String> {
    e2e_mark_seeded_snapshot()?;
    // The runner POSTs this only after its ownership-verified fresh-process
    // `probe --seed-scenario` succeeded — the flag inherits that verification.
    SCENARIO_VERIFIED.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// ONLINE-e2e recovery arm: sweep stray scenario imports out of the user's bank
/// (fail-closed: only exact scenario-name matches at wrong slots, off a
/// completeness-floored tolerant list). Invoked by spec teardown + the e2e.sh recovery
/// so an aborted seed can never leave test junk on the unit past the run.
#[cfg(feature = "e2e")]
#[tauri::command]
async fn e2e_clear_strays(state: State<'_, AppState>) -> Result<usize, String> {
    with_released_seize(state.session.clone(), move || {
        SCENARIO_VERIFIED.store(false, std::sync::atomic::Ordering::SeqCst);
        let swept = probe_api::seed_scenario::sweep_strays_core()?;
        e2e_patch_swept(&swept);
        Ok(swept.len())
    })
    .await
}

/// ONLINE-e2e scratch teardown: clear scratch slot `slot` (0-based list index), restoring
/// the empty state. SAFETY: refuses unless the slot currently holds `expect_name` (read in
/// the same session) — so a wrong index can never clear a real preset — and, ONLINE, also
/// requires the seed's fixture content marker (a name is not ownership: a user preset
/// coincidentally named "E2E Pedalboard" must never be cleared; fail-closed). Offline the
/// marker check is skipped — SimDevice state is disposable and serves no field-8 bodies.
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
        if e2e_online() {
            s.drain_until_quiet(250, 20)?;
            // Manifest OR content marker: a fixture a spec has levelled+saved has lost
            // its injected markers (the device rewrites the body on save), and a
            // marker-only check would refuse to clean up the harness's OWN preset —
            // stranding it and blocking the next run's seed.
            if !probe_api::seed_scenario::slot_is_ours(&mut s, slot, &expect_name) {
                return Err(format!(
                    "refusing to clear slot {slot}: '{expect_name}' matches by name but this \
                     harness has no record of seeding it — not seed-owned"
                ));
            }
        }
        SCENARIO_VERIFIED.store(false, std::sync::atomic::Ordering::SeqCst);
        s.clear_user_preset(slot)?;
        // Verify before releasing: `clear_user_preset` returning Ok is not proof the slot
        // is actually empty — HW-observed (2026-07-27) a preset with scene+footswitch
        // leveling left its slot still holding real content right after a successful-
        // looking clear (a dropped write, or a deferred save landing late; either way the
        // clear didn't take). Releasing on that would strand the slot with NEITHER
        // manifest NOR marker — the original bug `forget_seeded` exists to prevent, just
        // reached from the other direction. Re-read and only release once the slot reads
        // empty; still occupied means the harness KEEPS the claim, so the next seed's
        // "ours but dirty" arm reimports it fresh instead of refusing forever.
        //
        // `clear_user_preset` is fire-and-forget with no ACK, so read immediately and the
        // list can still show pre-clear state (the same reason `replace_inplace_with`
        // settles 800ms after its own clear). And an ABSENT entry — a truncated list, or
        // a failed read — is UNKNOWN, not empty: only a POSITIVELY observed empty name
        // releases the claim, so a degraded read keeps it instead of stranding the slot.
        crate::settle(std::time::Duration::from_millis(800));
        let now_empty = s
            .list_my_presets()
            .ok()
            .and_then(|list| list.get(slot as usize).map(|e| e.name.clone()))
            .is_some_and(|name| session::is_empty_slot_name(&name));
        if now_empty {
            probe_api::seed_scenario::forget_seeded(slot);
            e2e_patch_snapshot_slot(slot, "Empty");
        } else {
            log::warn!(
                "e2e_clear_preset: slot {slot} not observed empty after a successful \
                 clear_user_preset — keeping the seed manifest claim so the next seed can \
                 reimport it instead of stranding it"
            );
        }
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

/// STRICT-HARNESS measure for the post-leveling audio gate (`level.online.spec.ts`
/// online, `level-fs-preset24.spec.ts` offline): re-measure one sound of `slot` exactly
/// as the leveling lane measured it (scene as-is / base isolation / footswitch engaged
/// state with the saved ASSIGN `valueA` re-played) and return its integrated LUFS, so
/// the spec can assert the SAVED preset actually renders at the leveling target. USABLE
/// OFFLINE now that `sim_device`'s capture model is physics-faithful (base/scene C +
/// the leveled-param drive-pedal curve) — a stale comment here once called the offline
/// fake "a stimulus passthrough" (true before that model existed; every sound would
/// have "measured" identically, a vacuous gate). Any slot the loudness sidecar doesn't
/// cover still measures a flat default C, which is deterministic but not physically
/// meaningful — fine for a spec that only exercises a covered scenario slot.
/// The leveled-param coordinates a footswitch re-measure replays (see
/// `e2e_measure_sound` — the SPEC owns these, mirroring what it fed the leveling
/// lane, so no server-side picker exists to diverge from the wizard's choice).
#[cfg(feature = "e2e")]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FsLevRef {
    group_id: String,
    node_id: String,
    parameter_id: String,
}

/// P5 EXTERNAL-VALIDATION metadata for one re-measure (optional; omit and the command
/// behaves exactly as before). The SPEC supplies it because the spec is what knows the
/// target it drove the leveling lane with and what that lane reported back — the server
/// keeps no cross-command memory, and inferring it from a result vec by position is the
/// mislabeling bug this redesign removed (see `LevelResult::scene_slot`).
///
/// Emission still only happens when `TMP_E2E_VALIDATE_LOG` is set, so passing this on an
/// ordinary `bun run e2e` costs nothing.
#[cfg(feature = "e2e")]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ValidateArg {
    /// What the leveling run promised this sound would render at.
    target_lufs: f64,
    /// The run's own verdicts, forwarded so the consumer can SKIP (not FAIL) a row that
    /// was never reachable or whose save did not verify.
    #[serde(default)]
    clamped: bool,
    #[serde(default)]
    persist_mismatch: Option<bool>,
}

#[cfg(feature = "e2e")]
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn e2e_measure_sound(
    app: tauri::AppHandle<tauri::test::MockRuntime>,
    state: State<'_, AppState>,
    slot: u32,
    scene: Option<u32>,
    footswitch: Option<u32>,
    topology_id: String,
    lev: Option<FsLevRef>,
    validate: Option<ValidateArg>,
) -> Result<f64, String> {
    let stim_path = resolve_stimulus(&app, None, Some(topology_id))?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        // The label/identity is built from THIS call's own coordinates — the sound being
        // measured names itself, so a mid-batch failure upstream can never shift a row.
        let row = validate.map(|v| {
            let base = match (scene, footswitch) {
                (Some(s), _) => crate::validate_log::ValidationRow::scene(slot, s, v.target_lufs),
                (None, Some(sw)) => {
                    crate::validate_log::ValidationRow::footswitch(slot, sw, v.target_lufs)
                }
                (None, None) => crate::validate_log::ValidationRow::base(slot, v.target_lufs),
            };
            base.with_flags(v.clamped, v.persist_mismatch)
        });
        if scene.is_some() {
            return leveller::measure_sound_asis_strict(
                slot,
                scene,
                &[],
                None,
                &stim,
                row.as_ref(),
            )
            .map(|l| l.integrated_lufs);
        }
        // Base / footswitch context: the ONE shared isolation derivation the leveling +
        // Doctor lanes use (`doctor_force_bypass` over the SAVED doc) — not a private copy.
        let saved = read_saved_preset(slot)
            .ok_or_else(|| format!("field-8 read failed for slot {slot}"))?;
        // BOTH contexts isolate, through the SAME derivation, because that is what
        // `commands::level_preset` and `commands::level_footswitch` each solve for: a base
        // row is the preset with every footswitch-owned on-off block forced off, a footswitch
        // row is that one switch engaged with every sibling off. `footswitch` (None for base)
        // is the only thing that differs, and `doctor_force_bypass` already branches on it.
        //
        // THIS MUST TRACK `level_preset`'s DEFINITION OF BASE, in the same commit, or the
        // yardstick judges a sound no run ever targeted — and it fails silently, as a level
        // miss. HW has now demonstrated the trap in BOTH directions on "Plumes+BD2+OCD":
        // when the lane measured as-saved and this still isolated, base verified at -23.00
        // in-run and re-measured -32.54 (exactly 20*log10(0.2916/0.8744), the ratio of the
        // two presetLevels); reverse the pair and the error simply changes sign. The preset
        // was correct both times; the yardstick was not.
        let force =
            crate::commands::doctor::doctor_force_bypass(&saved["ftsw"], &saved, footswitch);
        // An ASSIGN switch's engaged sound = the leveled param at its saved `valueA`; a
        // BAKED (or assignment-less) switch needs no write. The leveled param TRIPLE comes
        // from the CALLER — the spec owns the same pinned coordinates it fed the leveling
        // lane — so there is no second in-server param picker to diverge from the wizard's
        // `defaultParamIndex` choice.
        //
        // The two ways of writing nothing are NOT the same, so this resolves the anchor
        // through `FsAssignAnchor` rather than a bare `Option`: writing nothing because the
        // switch has no assignment on this node is correct (every `on-off` switch in the
        // Hiwatt fixture bakes, and its engaged sound IS the saved state), but writing
        // nothing because the switch assigns some OTHER parameter — or because the matching
        // function's `valueA` is unusable — silently measures the BASE sound and files it
        // under the switch's identity. An external validator sees a plausible number on a
        // correctly-named row and cannot tell. Fail loudly instead.
        let fs_value = match (footswitch, lev) {
            (Some(sw), Some(l)) => {
                match footswitch::resolve_assign_anchor(
                    &saved["ftsw"],
                    sw,
                    &l.node_id,
                    &l.parameter_id,
                ) {
                    footswitch::FsAssignAnchor::Value(v) => {
                        Some(((l.group_id, l.node_id, l.parameter_id), v as f32))
                    }
                    footswitch::FsAssignAnchor::NoAssignment => None,
                    footswitch::FsAssignAnchor::Mismatch(why) => {
                        return Err(format!("slot {slot}: {why}"))
                    }
                }
            }
            _ => None,
        };
        leveller::measure_sound_asis_strict(slot, None, &force, fs_value, &stim, row.as_ref())
            .map(|l| l.integrated_lufs)
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
        // `online` is AUTHORITATIVE for mode-split specs: the Playwright process does NOT
        // inherit TMP_E2E_ONLINE (only the server subprocess does), so specs read it here,
        // never from process.env. (e2e.sh's readiness curl only checks the 200, not the body.)
        ("GET", "/health") => (
            "200 OK",
            serde_json::to_vec(&json!({ "ok": true, "online": e2e_online() })).unwrap_or_default(),
        ),
        // Verification-harness read endpoints (see e2e/specs/level.spec.ts's and
        // e2e/specs/level.online.spec.ts's idempotency tests).
        ("GET", "/reamp/counters") => {
            use std::sync::atomic::Ordering;
            let on = crate::session::REAMP_ON_COUNT.load(Ordering::Relaxed);
            let off = crate::session::REAMP_OFF_COUNT.load(Ordering::Relaxed);
            (
                "200 OK",
                serde_json::to_vec(&json!({ "on": on, "off": off })).unwrap_or_default(),
            )
        }
        ("GET", "/sim/events") => (
            "200 OK",
            serde_json::to_vec(&crate::sim_device::live_events()).unwrap_or_default(),
        ),
        // Capture-fault injection (PR3 spec 4): arm slot N's NEXT offline capture to
        // return silence once (→ the leveller's no-signal path). Body: {"slot": N}.
        // No-op online (no fake installed).
        ("POST", "/sim/fault") => {
            let slot = serde_json::from_slice::<serde_json::Value>(body)
                .ok()
                .and_then(|v| v.get("slot").and_then(serde_json::Value::as_u64));
            match slot {
                Some(n) => {
                    crate::sim_device::arm_capture_fault(n as u32);
                    ("200 OK", b"{\"ok\":true}".to_vec())
                }
                None => (
                    "400 Bad Request",
                    b"{\"ok\":false,\"error\":\"missing slot\"}".to_vec(),
                ),
            }
        }
        // Lazy-commit latency override (stale-load incident spec): arm the ALREADY-running
        // offline fake's commit latency, since a per-test env var can't reach a server
        // process that started before the test did (`sim_device::set_commit_latency`'s
        // doc). Body: {"ms": N}. No-op online (no fake installed).
        ("POST", "/sim/commit-latency") => {
            let ms = serde_json::from_slice::<serde_json::Value>(body)
                .ok()
                .and_then(|v| v.get("ms").and_then(serde_json::Value::as_u64));
            match ms {
                Some(n) => {
                    crate::sim_device::set_commit_latency(n);
                    ("200 OK", b"{\"ok\":true}".to_vec())
                }
                None => (
                    "400 Bad Request",
                    b"{\"ok\":false,\"error\":\"missing ms\"}".to_vec(),
                ),
            }
        }
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
            let cmd_for_save_check = cmd.clone();
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
                    // Success only — an Err means the command aborted before its save,
                    // so there is nothing persisted to invalidate the flag for.
                    note_structural_save(&cmd_for_save_check);
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
