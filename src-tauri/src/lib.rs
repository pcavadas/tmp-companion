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
// Benefit-aware headroom trade + the clamp-error taxonomy (pure; the device half is in
// `leveller.rs`).
mod headroom_trade;
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
mod param_class;
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
mod slot_read;
mod spectrum;
// Shared test-only fixtures/generators (`audio` onset tests, `leveller`
// onset-gate tests) — see the module doc.
#[cfg(test)]
pub(crate) mod test_support;
// `pub` so the `gen_samples` bin (a separate crate) can reach the shared
// catalog as `tmp_companion_lib::topologies`.
pub mod topologies;
// P5 external validation: the JSON-lines expectation log the measurement seams append
// to when `TMP_E2E_VALIDATE_LOG` is set. Inert (and unallocating) otherwise.
#[allow(dead_code)]
mod validate_log;
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
// `scene_overlay_for`/`RefusedScope` are NOT re-exported here (unlike their siblings): every
// call site that used to reach them through this crate-root seam (`scene_handle_rows`'s old
// home in `commands::level_scenes`) moved INTO `probe_api::scene_jobs` itself as part of the
// same extraction, so the only remaining callers resolve them directly, module-internally —
// re-exporting an unused name here is a dead `pub(crate) use`, caught by `-D warnings`.
pub(crate) use probe_api::scene_jobs::{
    base_handle_candidates_scanned, build_scene_jobs_with_handles, is_amp_model_id,
    is_amp_output_level_param, last_loaded_scene, prepass_scene_docs_via, read_saved_preset,
    read_saved_preset_complete, scan_node_graph, scene_handle_rows, scene_handle_rows_scanned,
    scene_overlay, scene_write_verdict_for_param, scenes_restating_base,
    warn_missing_restore_scene, SceneHandleSpec, SceneOverlay, SceneWriteVerdict,
};
// `pub`, not `pub(crate)`: `backup_read::BackupPresetRow.scene_handles`/`.base_handles` (both
// `pub` fields) embed these — see the types' own doc for why a less-visible type inside a
// more-visible field is a hard warning (`private_interfaces`). Same reachability the two
// types had when they lived in `commands::level_scenes` (reached via `pub use
// commands::level_scenes::*` below); moving them into `probe_api::scene_jobs` must not
// narrow that.
pub use probe_api::scene_jobs::{SceneHandleCandidate, SceneHandleRow};
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
// The truncation-aware saved-preset read seam — re-exported at the crate root so the
// ~20 probe_api call sites and the leveling commands keep addressing it as `crate::read_slot_preset_*`.
pub(crate) use slot_read::*;

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
    /// Every committed scenario fixture, decoded: `(listIndex, name, presetJson
    /// string, parsed presetJson)`. One reader, so a schema rename fails loudly in
    /// one place instead of making each gate below pass vacuously.
    fn fixtures() -> Vec<(u32, String, String, serde_json::Value)> {
        let path = std::path::Path::new("../e2e/fixtures/scenario-presets.json");
        assert!(
            path.is_file(),
            "{} is missing — it is git-tracked, so absence means a moved/renamed \
             fixture or a wrong relative path",
            path.display()
        );
        let raw = std::fs::read_to_string(path).expect("read fixture");
        let entries: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("fixture is JSON");
        let out: Vec<_> = entries
            .iter()
            .map(|e| {
                let js = e["presetJson"].as_str().expect("presetJson is text");
                (
                    e["listIndex"].as_u64().expect("listIndex") as u32,
                    e["name"].as_str().expect("name").to_string(),
                    js.to_string(),
                    serde_json::from_str(js).expect("presetJson parses"),
                )
            })
            .collect();
        assert_eq!(
            out.len(),
            11,
            "the scenario set is eleven presets at 400-410 (400-405 the original set, \
             406-409 the P3 leveling-doctor-fixtures additions, 410 the P4 Friedman-HBE-class \
             3-scene addition)"
        );
        out
    }

    /// The fixture at `list_index`, by its 0-based list index.
    fn fixture(list_index: u32) -> (String, String, serde_json::Value) {
        fixtures()
            .into_iter()
            .find(|(i, ..)| *i == list_index)
            .map(|(_, n, js, v)| (n, js, v))
            .unwrap_or_else(|| panic!("no scenario fixture at list index {list_index}"))
    }

    /// Catalog block ids whose `category` makes them a SPEAKER CABINET (or a raw IR)
    /// — the downstream block a bare amp head needs to satisfy the cab rule. Read
    /// from the same `tmp-model-guide.json` `scene_jobs::amp_model_ids` reads, so
    /// amp-ness and cab-ness can never disagree about what the catalog says.
    /// Catalog block ids whose `category` makes them a SPEAKER CABINET (or a raw IR)
    /// — the downstream block a bare amp head needs to satisfy the cab rule. Routed
    /// through the same category collector `scene_jobs::amp_model_ids` uses, so
    /// amp-ness and cab-ness can never disagree about what the catalog says.
    fn cab_model_ids() -> std::collections::HashSet<String> {
        let ids = crate::probe_api::scene_jobs::model_ids_by_category(|cat| {
            matches!(cat, "Cabinets" | "IR")
        });
        assert!(
            ids.len() > 50,
            "expected the catalog's cabinet rows ({} found) — a category rename would \
             otherwise make the cab rule pass vacuously",
            ids.len()
        );
        ids
    }

    /// Does this device FenderId carry its cabinet BAKED IN? Delegates to
    /// `scene_jobs::bakes_in_a_cab` so the cab-merged suffix rule can't drift between
    /// the production classifier and this gate.
    fn is_cab_merged_amp(model_id: &str) -> bool {
        crate::probe_api::scene_jobs::bakes_in_a_cab(model_id)
    }

    /// Every complete signal path through a preset, as ordered block lists — built
    /// from the PRODUCTION routing decoder (`session::extract_active_graph`), so the
    /// cab rule below reasons about the same lanes the app draws. A `Split` stage
    /// forks the path set; split-OUTPUT lanes fork it once more at the tail; the
    /// independent-rail templates (`gtrMicParallel`) replace it outright.
    /// Fork every path in `paths` into two, extending one copy with `a_blocks` and the
    /// other with `b_blocks` — the shared reshape both the `Split`-stage fork and the
    /// `graph.outputs` fork in [`signal_paths`] perform.
    fn fork(
        paths: &[Vec<crate::GraphNode>],
        a_blocks: &[crate::GraphNode],
        b_blocks: &[crate::GraphNode],
    ) -> Vec<Vec<crate::GraphNode>> {
        paths
            .iter()
            .flat_map(|p| {
                [a_blocks, b_blocks].map(|lane| {
                    let mut q = p.clone();
                    q.extend(lane.iter().cloned());
                    q
                })
            })
            .collect()
    }

    fn signal_paths(graph: &crate::ActiveGraph) -> Vec<Vec<crate::GraphNode>> {
        let mut paths: Vec<Vec<crate::GraphNode>> = vec![Vec::new()];
        for stage in &graph.stages {
            match stage {
                crate::Stage::Series { blocks } => {
                    for p in &mut paths {
                        p.extend(blocks.iter().cloned());
                    }
                }
                crate::Stage::Split { a, b } => {
                    paths = fork(&paths, a, b);
                }
            }
        }
        if let Some(outs) = &graph.outputs {
            paths = fork(&paths, &outs.a.blocks, &outs.b.blocks);
        }
        if let Some(lanes) = &graph.lanes {
            paths = lanes.iter().map(|l| l.blocks.clone()).collect();
        }
        paths
    }

    /// **THE CAB RULE** (standing user directive, enforced structurally so it cannot
    /// silently regress): every guitar amp in every committed fixture is a combo, an
    /// amp+cab-merged model (a cab/IR-suffixed id), or a bare head with a cabinet
    /// block DOWNSTREAM IN ITS OWN LANE. No bare heads — including in the incident
    /// fixtures, which used to be exempt by accident (`E2E Preset24` shipped four
    /// drives into a naked `ACD_TwinReverb65NoFx`).
    ///
    /// "Its own lane" is what makes this non-trivial: `E2E Hiwatt 3S` puts its head in
    /// the pre-split `G1` and a cab in EACH of the two `gtrParallel1` lanes (`G2`,
    /// `G3`), so a naive "later in the same group" check would red-light a
    /// device-authored preset that is perfectly cabbed. Hence the path walk.
    #[test]
    fn every_guitar_amp_in_every_fixture_reaches_a_cab() {
        let cabs = cab_model_ids();
        let mut amps_checked = 0usize;
        for (idx, name, _, preset) in fixtures() {
            let graph = crate::session::extract_active_graph(&preset, None);
            let paths = signal_paths(&graph);
            assert!(
                !paths.is_empty(),
                "{name} ({idx}): the routing decoder produced no signal path — a \
                 template rename would otherwise make this gate pass vacuously"
            );
            for path in &paths {
                for (i, node) in path.iter().enumerate() {
                    if !crate::is_amp_model_id(&node.model) {
                        continue;
                    }
                    amps_checked += 1;
                    if is_cab_merged_amp(&node.model) {
                        continue;
                    }
                    // The cab must also be LIVE. A `bypassed: true` cabinet satisfies a
                    // presence-only check while the player hears the amp bare — the exact
                    // sound the standing cab rule exists to forbid — so bypass state is part
                    // of the predicate, not decoration. (Base state only: this walks the base
                    // graph, and a scene that bypasses a cab is a per-scene authoring choice,
                    // not a fixture-wide violation.)
                    assert!(
                        path[i + 1..]
                            .iter()
                            .any(|n| cabs.contains(&n.model) && !n.bypassed),
                        "{name} ({idx}): amp {} ({}) has no UN-BYPASSED cabinet \
                         downstream in its lane [{}] — every fixture amp must be a \
                         combo, a cab-merged model, or a head + a live cab block",
                        node.node_id,
                        node.model,
                        path.iter()
                            .map(|n| n.model.as_str())
                            .collect::<Vec<_>>()
                            .join(" → ")
                    );
                }
            }
        }
        assert!(
            amps_checked >= 6,
            "expected every fixture's amps to be walked ({amps_checked} seen) — a \
             graph-decode change that emptied the node lists would pass vacuously"
        );
    }

    /// SIZE BUDGET. A slot-addressed saved-preset (`presetDataRequest`, field 8) read
    /// starts returning TAIL-TRUNCATED bodies somewhere around 17-20 KB, and the seed's
    /// own pristine/ownership probes are all substring scans over that body. Every
    /// fixture therefore stays under 16 KiB — with ONE named exemption: `E2E Hiwatt 3S`
    /// is a real device export kept BYTE-VERBATIM as the scene-conformance oracle, and
    /// it already sits at the cliff. Its size is pinned exactly rather than bounded, so
    /// an accidental edit to the one fixture nobody may edit fails here.
    #[test]
    fn e2e_fixtures_stay_inside_the_field8_read_budget() {
        const BUDGET: usize = 16 * 1024;
        // Includes the one byte the #r10 FIXTURE_SOURCE_STAMP costs over #r9 in this
        // fixture's own `info.source_id` — the stamp growing, not an edit to its substance.
        const HIWATT_BYTES: usize = 20_013;
        for (idx, name, js, _) in fixtures() {
            if name == "E2E Hiwatt 3S" {
                assert_eq!(
                    js.len(),
                    HIWATT_BYTES,
                    "{name} ({idx}) is the KEEP-VERBATIM device export — it must not be \
                     edited (it is the scene-conformance oracle and already sits at the \
                     field-8 truncation cliff)"
                );
                continue;
            }
            assert!(
                js.len() < BUDGET,
                "{name} ({idx}) serializes to {} bytes, over the {BUDGET}-byte field-8 \
                 budget — trim scene overlays (a per-scene splitMix alone costs ~730 B, \
                 paid once per scene)",
                js.len()
            );
        }
    }

    /// FX1 `E2E Rig` @ 400 — the scene/overlay + damage-signature fixture. Pins the
    /// structural facts `e2e/fixtures/COVERAGE.md` maps use-case rows onto, so a
    /// fixture edit that quietly drops one fails here rather than in a spec whose
    /// failure message says nothing about the cause.
    #[test]
    fn fx_rig_carries_the_scene_and_damage_cases() {
        let (name, _, p) = fixture(400);
        assert_eq!(name, "E2E Rig");
        assert_eq!(p["audioGraph"]["template"], "gtrSeries");
        assert_eq!(p["scenes"].as_array().expect("scenes").len(), 4);
        assert_eq!(p["lastLoadedScene"], 8, "base is the saved context");

        let g1 = p["audioGraph"]["guitarNodes"]["G1"]
            .as_array()
            .expect("G1 chain");
        let ids: Vec<&str> = g1
            .iter()
            .map(|n| n["FenderId"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            ids,
            [
                "ACD_TubeScreamer",
                "ACD_JC120",
                "ACD_TwinReverb65NoFx",
                "ACD_CabSimTMS",
                "ACD_Boost",
                "ACD_TMSpring63",
                "ACD_CryBabyQ535",
            ],
            "TWO amps (the amp-flip pair) sharing one downstream cab, then the raw-dB \
             boost, the wet-mix block and the all-Other-class block"
        );
        // The on-off drive stomp is saved ENGAGED (isActive true ⇒ bypass false).
        assert_eq!(g1[0]["dspUnitParameters"]["bypass"], false);

        let overlay = |scene: usize, node: &str| -> Option<&serde_json::Value> {
            p["scenes"][scene]["guitarNodes"]["G1"]
                .get(node)
                .map(|e| &e["dspUnitParameters"])
        };
        let is_bypass_only = |v: &serde_json::Value| {
            v.as_object()
                .expect("overlay params")
                .keys()
                .all(|k| ["bypass", "bypassType"].contains(&k.as_str()))
        };
        for (scene, active, idle) in [
            (0usize, "ACD_JC120", "ACD_TwinReverb65NoFx"),
            (1, "ACD_TwinReverb65NoFx", "ACD_JC120"), // the AMP FLIP
            (2, "ACD_JC120", "ACD_TwinReverb65NoFx"),
            (3, "ACD_JC120", "ACD_TwinReverb65NoFx"),
        ] {
            let a = overlay(scene, active).expect("active amp overlay");
            let b = overlay(scene, idle).expect("idle amp overlay");
            assert_eq!(
                a["bypass"], false,
                "scene {scene}: {active} is the live amp"
            );
            assert_eq!(b["bypass"], true, "scene {scene}: {idle} is flipped out");
            assert_eq!(
                a["outputLevel"], b["outputLevel"],
                "scene {scene}: BOTH amps must carry the SAME outputLevel — the offline \
                 capture model's stored-level probe takes the first G1 node carrying one, \
                 so unequal values would make the modelled ol_term depend on map order"
            );
            assert!(!is_bypass_only(a), "scene {scene}: amp overlays are FULL");
        }
        assert_eq!(
            overlay(2, "ACD_JC120").expect("ceiling amp")["outputLevel"],
            1.0,
            "scene 2 'Ceiling' is the headroom lowers_only / scene-clamp row"
        );
        // COVERAGE rows 6/8/9's real load-bearing fact — NOT "four distinct ceilings" (two of
        // the sidecar Cs are equal on purpose). What every spec keys on is that scene 2 is the
        // UNIQUE clamper: it alone sits at the knob's top with zero boost headroom, so it alone
        // clamps at every shipped target, and the other three keep room to be re-levelled into
        // (the redistribution fixture). Equalise another scene to 1.0 and the redistribution
        // gate stops having a single rescuable row.
        for s in [0usize, 1, 3] {
            let ol = overlay(s, "ACD_JC120").expect("amp overlay")["outputLevel"]
                .as_f64()
                .expect("outputLevel is a float");
            assert!(
                ol < 1.0,
                "scene {s} must keep boost headroom — scene 2 'Ceiling' is the ONLY \
                 zero-headroom scene (got outputLevel {ol})"
            );
        }
        // The Boost: a picker-visible isolated handle in scenes 0-2, Scene-Edit
        // DISABLED (bypass-only ⇒ shared_with_base refusal) in scene 3.
        for s in 0..3 {
            assert!(
                !is_bypass_only(overlay(s, "ACD_Boost").expect("boost overlay")),
                "scene {s}: the Boost handle is isolated (Scene Edit on)"
            );
        }
        assert!(
            is_bypass_only(overlay(3, "ACD_Boost").expect("boost overlay")),
            "scene 3 'Shared': the Boost overlay is bypass-only → shared_with_base"
        );
        // The all-Other-class block never gets an overlay: the SceneOverlay::Absent /
        // NeedsEnable case.
        for s in 0..4 {
            assert!(overlay(s, "ACD_CryBabyQ535").is_none());
        }

        // The two DOCTOR leveling-damage signatures, in the wire `ftsw` shape the
        // backup scan feeds `doctor::leveling_damage_hints`.
        let fs = crate::footswitch::enumerate_block_footswitches(&p["ftsw"], &p);
        let hints = crate::doctor::leveling_damage_hints(&fs);
        let kinds: Vec<_> = hints.iter().map(|h| h.kind).collect();
        assert!(
            kinds.contains(&crate::doctor::LevelingDamageKind::DeletedEffect)
                && kinds.contains(&crate::doctor::LevelingDamageKind::SweptOther),
            "E2E Rig must carry BOTH damage signatures (a zeroed wet mix and a swept \
             Other-class param); got {hints:?}"
        );
        // One UNLABELED block-acting switch (the empty-customLabel rendering case).
        assert!(
            fs.iter().any(|f| f.label.is_empty()),
            "E2E Rig must keep one unlabeled block-acting switch"
        );
    }

    /// FX2 `E2E Parallel` @ 403 — the joint-k / rebalance fixture.
    #[test]
    fn fx_parallel_runs_both_lane_amps() {
        let (name, _, p) = fixture(403);
        assert_eq!(name, "E2E Parallel");
        assert_eq!(p["audioGraph"]["template"], "gtrParallel1");
        assert_eq!(p["scenes"].as_array().expect("scenes").len(), 4);

        let lane = |g: &str| -> Vec<String> {
            p["audioGraph"]["guitarNodes"][g]
                .as_array()
                .expect("lane")
                .iter()
                .map(|n| n["FenderId"].as_str().unwrap_or_default().to_string())
                .collect()
        };
        assert_eq!(lane("G2"), ["ACD_JC120", "ACD_CabSimTMS"]);
        assert_eq!(lane("G3"), ["ACD_MarshallPlexi", "ACD_Mar412Cent100"]);
        for (g, node) in [("G2", "ampA"), ("G3", "ampB")] {
            let amp = p["audioGraph"]["guitarNodes"][g][0].clone();
            assert_eq!(amp["nodeId"], node);
            assert_eq!(amp["dspUnitParameters"]["bypass"], false, "both lanes live");
            assert_eq!(
                amp["dspUnitParameters"]["outputLevel"], 1.0,
                "both lane amps sit at outputLevel 1.0 in base and in every scene but the \
                 deliberate zero-authority one — they must MATCH each other, or the \
                 offline model's stored-level probe (first node carrying an outputLevel, \
                 G1..G7) would desync written/stored and the closed-form joint-k solve \
                 would stop converging in one step"
            );
        }
        for s in 0..4 {
            // Scene 2 "Clean" is saved with BOTH amps' output at ZERO — no authority over
            // the USB capture, so its job returns the ROUTING clamp, not a headroom one.
            let want = if s == 2 { 0.0 } else { 1.0 };
            for (g, node) in [("G2", "ampA"), ("G3", "ampB")] {
                assert_eq!(
                    p["scenes"][s]["guitarNodes"][g][node]["dspUnitParameters"]["outputLevel"],
                    want,
                    "scene {s} {node}"
                );
            }
            // The Bass-VI shared-knob shape: bypass-only in EVERY scene ⇒ the scene
            // handle picker must report scope "shared_with_base" everywhere.
            let kot = p["scenes"][s]["guitarNodes"]["G4"]["ACD_KingOfTone"]["dspUnitParameters"]
                .as_object()
                .expect("KingOfTone overlay");
            assert!(
                kot.keys()
                    .all(|k| ["bypass", "bypassType"].contains(&k.as_str())),
                "scene {s}: the post-merge KingOfTone overlay stays bypass-only"
            );
        }
        // The in-path mixer the rebalance lane drives.
        let mix1 = &p["audioGraph"]["splitMix"]["mixPoints"][0]["parameters"];
        assert!(mix1["levelA"].is_f64() && mix1["levelB"].is_f64());
        assert_ne!(
            mix1["levelA"], mix1["levelB"],
            "an authored, non-neutral mix"
        );
        // A switch-LINK radio group selecting the lane amps.
        let fs = crate::footswitch::enumerate_block_footswitches(&p["ftsw"], &p);
        let linked: Vec<_> = fs.iter().filter(|f| f.link_group.is_some()).collect();
        assert_eq!(linked.len(), 2, "a two-member switch-link radio group");
        assert_eq!(linked[0].link_group, linked[1].link_group);
    }

    /// FX3 `E2E Pedalboard` @ 401 — the SCENE-FREE fixture (copy/import + the Doctor's
    /// simple-chain apply), carrying the EXP, link-group and second-bank cases.
    #[test]
    fn fx_pedalboard_is_scene_free_with_exp_and_a_second_bank_switch() {
        let (name, _, p) = fixture(401);
        assert_eq!(name, "E2E Pedalboard");
        assert_eq!(p["audioGraph"]["template"], "gtrSeries");
        assert!(
            p["scenes"].as_array().expect("scenes").is_empty(),
            "FX3 is the ZERO-scene fixture"
        );
        // EXP: a volume pedal on exp1, a wah on exp2, and a TOE assign.
        assert_eq!(p["exp"]["exp1"][0]["nodeId"], "ACD_VolumePedal");
        assert_eq!(p["exp"]["exp2"][0]["nodeId"], "ACD_CryBabyQ535");
        assert_eq!(p["exp"]["toe"][0]["nodeId"], "ACD_CryBabyQ535");
        // A tempo-synced time-based block (noteDivision off "off" + a preset bpm).
        let trem = p["audioGraph"]["guitarNodes"]["G1"]
            .as_array()
            .expect("G1")
            .iter()
            .find(|n| n["FenderId"] == "ACD_TremoloBias")
            .expect("the tempo-synced block");
        assert_ne!(trem["dspUnitParameters"]["noteDivision"], "off");
        assert!(p["bpm"].as_f64().is_some_and(|b| b > 0.0));
        // A PARAM radio link group, and a switch on the SECOND bank (index >= 11).
        let fs = crate::footswitch::enumerate_block_footswitches(&p["ftsw"], &p);
        let radio: Vec<_> = fs
            .iter()
            .filter(|f| f.link_group == Some(3) && f.functions.iter().all(|x| x.func == "param"))
            .collect();
        assert_eq!(radio.len(), 2, "a two-member PARAM radio link group");
        assert!(
            fs.iter().any(|f| f.switch >= 11),
            "one block-acting switch must live on the second bank"
        );
    }

    /// FX4 `E2E Edge` @ 402 — the split-output / 8-scene fixture that also carries the
    /// Doctor's ONLINE oracle: Target 2's baked 2.6 kHz EQ ring, byte-verbatim.
    #[test]
    fn fx_edge_keeps_the_eq_ring_and_eight_scenes() {
        let (name, _, p) = fixture(402);
        assert_eq!(name, "E2E Edge");
        assert_eq!(p["audioGraph"]["template"], "gtrSplit");
        assert_eq!(p["scenes"].as_array().expect("scenes").len(), 8);
        assert_eq!(
            p["lastLoadedScene"], 3,
            "the saved context is a NON-base scene (the measurement-context case)"
        );
        assert!(
            p["outputMixerSettings"].is_object(),
            "a split-output preset keeps its outputMixerSettings"
        );
        // COVERAGE row 21's LOAD-BEARING fact, pinned explicitly rather than left to the
        // `is_object()` shape check: OUT 2 is routed AWAY from USB 1/2, which is the only
        // reason an isolated capture of the OUT-2 lane's footswitch (`ACD_KingOfTone`,
        // declared to the offline model as `offbranchSwitchNode`) reads dead air and the
        // leveller returns its routing clamp. Flip this to `true` and the off-branch gate
        // (`e2e_server_tests.rs::level_defaults_base_clamps_and_the_split_lane_footswitch_is_offbranch`)
        // stops testing anything, silently.
        assert_eq!(
            p["outputMixerSettings"]["USB12Input"]["out2"],
            serde_json::json!(false),
            "E2E Edge routes OUT 2 away from USB 1/2 — the off-branch routing-clamp case"
        );
        // THE ORACLE: filters 3 and 4 both ring at 2.6 kHz, +12 dB, Q 14. Doctor's
        // online `harsh`/`fizzy` diagnosis is measured against exactly these values.
        let eq = p["audioGraph"]["guitarNodes"]["G2"]
            .as_array()
            .expect("out1 lane")
            .iter()
            .find(|n| n["FenderId"] == "ACD_FiveBandParamEQ")
            .expect("the EQ-ring block")["dspUnitParameters"]
            .clone();
        for band in [3, 4] {
            assert_eq!(eq[format!("filter{band}frequency")], 2600.0);
            assert_eq!(eq[format!("filter{band}gaindb")], 12.0);
            assert_eq!(eq[format!("filter{band}q")], 14.0);
            assert_eq!(eq[format!("filter{band}bypass")], false);
        }
        // The three overlay states across the 8 scenes: FULL (isolated handle),
        // BYPASS-ONLY (shared_with_base) and ABSENT (NeedsEnable) — see COVERAGE.md
        // for why FX4 cannot afford a full overlay per node per scene.
        let ol = |s: usize, node: &str| -> Option<Vec<String>> {
            p["scenes"][s]["guitarNodes"]["G1"]
                .get(node)?
                .get("dspUnitParameters")?
                .as_object()
                .map(|m| m.keys().cloned().collect())
        };
        assert!(
            ol(0, "ACD_JC120").is_some_and(|k| k.len() > 2),
            "scene 0 FULL"
        );
        assert!(
            ol(4, "ACD_JC120").is_some_and(|k| k.len() > 2),
            "scene 4 FULL"
        );
        assert_eq!(
            ol(2, "ACD_JC120"),
            Some(vec!["bypass".into(), "bypassType".into()]),
            "scene 2's amp overlay is bypass-only"
        );
        for s in [1usize, 3, 5, 6, 7] {
            assert!(
                ol(s, "ACD_JC120").is_none(),
                "scene {s}: amp overlay ABSENT"
            );
        }

        // BUG→GATE (user-reported, "Friedman HBE" preset 28): `ACD_Boost` is bypassed in
        // BASE, and scene 3 "Solo" carries a BYPASS-ONLY overlay that flips it on — no
        // footswitch or EXP assign targets it, and every OTHER scene carries no overlay
        // at all — inherits base's bypass. That is exactly
        // `scene_jobs::shared_write_is_scene_local`'s shape: the
        // leak-to-base write a bypass-only overlay gets is audible ONLY in Solo, so the
        // picker must offer the handle ENABLED there (`scope: "isolated"`, not
        // `"shared_with_base"`) — see `level-setup.spec.ts`'s "402 Solo" coverage.
        assert_eq!(
            p["audioGraph"]["guitarNodes"]["G1"]
                .as_array()
                .expect("G1 chain")
                .iter()
                .find(|n| n["FenderId"] == "ACD_Boost")
                .expect("ACD_Boost lives in base G1")["dspUnitParameters"]["bypass"],
            true,
            "ACD_Boost is bypassed in base"
        );
        assert_eq!(
            ol(3, "ACD_Boost"),
            Some(vec!["bypass".into(), "bypassType".into()]),
            "scene 3 'Solo': ACD_Boost's overlay is bypass-only, un-bypassing it"
        );
        for s in [0usize, 1, 2, 4, 5, 6, 7] {
            assert!(
                ol(s, "ACD_Boost").is_none(),
                "scene {s}: ACD_Boost carries no overlay — inherits base's bypass"
            );
        }
        assert!(
            !crate::footswitch::node_targeted_by_assign(&p, "ACD_Boost"),
            "no footswitch or EXP assign may target ACD_Boost — otherwise the shared write \
             could be audible outside Solo through a path this scan doesn't model"
        );
        assert!(
            matches!(
                crate::probe_api::scene_jobs::scene_write_verdict_for_param(
                    &p,
                    3,
                    "ACD_Boost",
                    "gain",
                ),
                crate::probe_api::scene_jobs::SceneWriteVerdict::WriteDirect
            ),
            "scene 3 'Solo': a scene-scoped write of ACD_Boost.gain must be allowed through \
             as a scene-local base write, not refused as shared_with_base"
        );

        // NEGATIVE CONTROL / over-widening tripwire: the policy must still REFUSE where it
        // must. Scene 2's ACD_JC120 overlay is bypass-only too (COVERAGE row 12's
        // shared_with_base case), but JC120 is NOT bypassed in base (`bypass: false`
        // above) — `shared_write_is_scene_local`'s own base-bypass guard means the
        // leak-to-base write here can never be scene-local, so an over-widened policy
        // that let every bypass-only overlay through would turn this Refuse into a
        // WriteDirect. Pin the Refuse to catch that regression.
        assert!(
            matches!(
                crate::probe_api::scene_jobs::scene_write_verdict_for_param(
                    &p,
                    2,
                    "ACD_JC120",
                    "outputLevel",
                ),
                crate::probe_api::scene_jobs::SceneWriteVerdict::Refuse { .. }
            ),
            "scene 2: ACD_JC120's bypass-only overlay must still be refused as \
             shared_with_base — JC120 is already audible in base, so the write can never be \
             scene-local"
        );
    }

    /// 404/405 — the two INCIDENT fixtures. 404 is kept verbatim (its exact bytes are
    /// pinned by the size gate above); 405's amendment must not have disturbed the
    /// lazy-save incident's own shape: the four drive pedals, their block-acting
    /// switches and the amp node are all untouched, and only a cab was appended.
    ///
    /// Pins the CURRENT measurement shapes a leveling spec depends on (405's amp/pedal knobs,
    /// the C table, `leveledParams`) — not immutability; 405's values may be amended again.
    #[test]
    fn incident_fixtures_pin_their_measurement_shapes() {
        let (name, _, hiwatt) = fixture(404);
        assert_eq!(name, "E2E Hiwatt 3S");
        assert_eq!(hiwatt["lastLoadedScene"], 3, "the saved non-base context");
        assert_eq!(hiwatt["scenes"].as_array().expect("scenes").len(), 4);

        let (name, _, p24) = fixture(405);
        assert_eq!(name, "E2E Preset24");
        let ids: Vec<&str> = p24["audioGraph"]["guitarNodes"]["G1"]
            .as_array()
            .expect("G1")
            .iter()
            .map(|n| n["FenderId"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            ids,
            [
                "ACD_Plumes",
                "ACD_BluesDriver",
                "ACD_ObsessiveDrive",
                "ACD_Rat",
                "ACD_TwinReverb65NoFx",
                "ACD_CabSimTMS", // appended for the cab rule; nothing upstream moved
            ]
        );
        // BUG→GATE: each of 405's four drive pedals must author the parameter NAME the
        // real hardware actually exposes for that block. HW-confirmed from a field-8 read
        // of the user's own real "Plumes+BD2+OCD" preset (2026-08-31): the block's
        // parameters read `blend, bypass, bypassType, drive, filter, tone, volume` — there
        // is NO `level`. `ACD_Plumes` and `ACD_BluesDriver` do carry `level` in that same
        // read, and `ACD_Rat` carries `volume`, so `ACD_ObsessiveDrive` is the odd one out
        // despite superficially matching its Plumes/BluesDriver siblings. A fixture that
        // declares a parameter name the block does not have is INVISIBLE offline: the
        // sim's saturated-pedal model (`sim_device.rs::saturated_pedal_lufs`) keys
        // `leveledParams` purely by whatever name the fixture declares, so an offline run
        // stays self-consistent regardless of whether that name exists on the device.
        // Online, a write to a nonexistent parameter reaches the device, the capture never
        // responds, and the solver's flat-response branch clamps the row at zero — this is
        // exactly how the `ACD_ObsessiveDrive` "level" mistake was caught (footswitch 7's
        // row clamped to 0.0 while its siblings converged normally).
        for (node, param) in [
            ("ACD_Plumes", "level"),
            ("ACD_BluesDriver", "level"),
            ("ACD_ObsessiveDrive", "volume"),
            ("ACD_Rat", "volume"),
        ] {
            let node_json = p24["audioGraph"]["guitarNodes"]["G1"]
                .as_array()
                .expect("G1")
                .iter()
                .find(|n| n["FenderId"].as_str() == Some(node))
                .unwrap_or_else(|| panic!("405 has no {node} node"));
            assert!(
                node_json["dspUnitParameters"].get(param).is_some(),
                "405's {node} must author a {param:?} dspUnitParameter — HW-confirmed \
                 from a field-8 read of the real device; a wrong name here is invisible \
                 offline (see comment above)"
            );
            // And ONLY that one: a node carrying both names would satisfy the presence
            // check above while still shipping a parameter the block does not have, which
            // is the same invisible-offline defect in a form the presence check misses.
            let wrong = if param == "level" { "volume" } else { "level" };
            assert!(
                node_json["dspUnitParameters"].get(wrong).is_none(),
                "405's {node} must NOT author a {wrong:?} dspUnitParameter — the real \
                 block has no such control"
            );
        }
        // Plumes-regression amendment: the amp's outputLevel and the preset's own
        // presetLevel both moved (0.28/0.27, from 1.0/1.0) and both are now baked into
        // scenario-loudness.json's "405" C — a drift here silently invalidates that C.
        assert_eq!(
            p24["audioGraph"]["guitarNodes"]["G1"][4]["dspUnitParameters"]["outputLevel"], 0.28,
            "the saturated amp's own knob — the offline C table and the `leveledParams` \
             pedal curve both key off it"
        );
        assert_eq!(
            p24["audioGraph"]["presetLevel"], 0.27,
            "the preset's own presetLevel — every capture's PT term keys off it"
        );
        assert_eq!(
            p24["audioGraph"]["guitarNodes"]["G1"][3]["dspUnitParameters"]["bypass"], false,
            "Rat is now base-ON (was true) — the fixture's second measurement regime, \
             isolated to Rat's own footswitch capture"
        );
        assert!(p24["scenes"].as_array().expect("scenes").is_empty());
        let fs = crate::footswitch::enumerate_block_footswitches(&p24["ftsw"], &p24);
        assert_eq!(fs.len(), 4, "the four drive-pedal switches (ftsw 5-8)");

        let (name, _, friedman) = fixture(410);
        assert_eq!(name, "E2E Friedman 3S");
        assert_eq!(
            friedman["audioGraph"]["guitarNodes"]["G1"][1]["dspUnitParameters"]["outputLevel"], 1.0,
            "the base amp's outputLevel stays at 1.0 — load-bearing so a base capture is \
             never boosted and the fader is never written outside a scene job"
        );
        assert_eq!(
            friedman["lastLoadedScene"], 1,
            "loads into Lead, not base — the ≠-base premise this fixture exists for"
        );
    }

    /// A node's `dspUnitParameters.bypass`, by nodeId, walked from `p`'s base graph via
    /// the shared node-walk seam (`crate::audiograph::for_each_node` +
    /// `crate::audiograph::node_id`, which falls back to `FenderId`) rather than a
    /// hand-rolled `guitarNodes`-only walk — this also covers `micNodes`. `None` if
    /// the node is absent.
    fn base_node_bypass(p: &serde_json::Value, node_id: &str) -> Option<bool> {
        let mut found = None;
        crate::audiograph::for_each_node(p, |n| {
            let obj = serde_json::Value::Object(n.clone());
            if crate::audiograph::node_id(&obj) == Some(node_id) {
                found = n
                    .get("dspUnitParameters")
                    .and_then(|d| d.get("bypass"))
                    .and_then(serde_json::Value::as_bool);
            }
        });
        found
    }

    /// `E2E Combined Level` @ 406: the combined leveling fixture. Pins the structural
    /// facts the new-flow leveling specs build on: parallel BOTH-amps-active topology
    /// with one cab node per amp lane, a post-cab compressor always live in one
    /// lane (the trade/correction physics case), an FS acting on a block no scene ever
    /// overlays ("FS alone"), a scene literally named "BASE SCENE" that enables no
    /// footswitch ("scene alone"), and a distinct scene that DOES enable one ("Lead")
    /// — both judged rows sitting at wire index ≥ 1 (index 0 stays an unjudged filler:
    /// scene 0 of a 2-amp preset is never judged, since USB `loadScene(0)` can
    /// materialize a different amp state than the physical footswitch tap — see
    /// `danger.md`'s OPEN scene-0 item).
    #[test]
    fn fx_combined_level_carries_the_leveling_use_cases() {
        let cabs = cab_model_ids();
        let (name, _, p) = fixture(406);
        assert_eq!(name, "E2E Combined Level");
        assert_eq!(p["audioGraph"]["template"], "gtrParallel1");

        // One cab node per amp lane, no more. (Counted by catalog category, NOT via
        // blockcaps: the firmware's `ComboHalfStackCabinetsLimit` set does not count
        // `ACD_Mar412Cent100`, so cap weight and cab-node count differ here on purpose.)
        let mut cab_count = 0usize;
        let groups = p["audioGraph"]["guitarNodes"]
            .as_object()
            .expect("guitarNodes");
        for nodes in groups.values() {
            for n in nodes.as_array().into_iter().flatten() {
                if let Some(fid) = n["FenderId"].as_str() {
                    if cabs.contains(fid) {
                        cab_count += 1;
                    }
                }
            }
        }
        assert_eq!(
            cab_count, 2,
            "exactly one cab node per amp lane (2 total), no more"
        );

        // Both lane amps active (base state) — the "parallel both active" shape.
        assert_eq!(
            base_node_bypass(&p, "ampA"),
            Some(false),
            "ampA (Deluxe Reverb) live"
        );
        assert_eq!(
            base_node_bypass(&p, "ampB"),
            Some(false),
            "ampB (Marshall Plexi) live"
        );
        assert_eq!(
            p["audioGraph"]["guitarNodes"]["G2"][0]["FenderId"],
            "ACD_DeluxeReverb65NoFx"
        );
        assert_eq!(
            p["audioGraph"]["guitarNodes"]["G3"][0]["FenderId"],
            "ACD_MarshallPlexi"
        );

        // The post-cab compressor is ALWAYS ON (the trade/correction physics case).
        assert_eq!(
            p["audioGraph"]["guitarNodes"]["G3"][2]["FenderId"],
            "ACD_CompressorSimpleSoftKnee"
        );
        assert_eq!(
            base_node_bypass(&p, "compB"),
            Some(false),
            "post-cab comp is always live"
        );

        // FS alone: switch 1 (DRIVE) acts on ACD_TubeScreamer, and NO scene overlays it.
        let fs = crate::footswitch::enumerate_block_footswitches(&p["ftsw"], &p);
        let drive = fs
            .iter()
            .find(|f| {
                f.functions
                    .iter()
                    .any(|fun| fun.node_id == "ACD_TubeScreamer")
            })
            .expect("a DRIVE footswitch acting on ACD_TubeScreamer");
        assert_eq!(drive.switch, 1);
        let scenes = p["scenes"].as_array().expect("scenes");
        for scene in scenes {
            assert!(
                scene["guitarNodes"]["G1"].get("ACD_TubeScreamer").is_none(),
                "FS-alone target must never be overlaid by any scene"
            );
        }

        // Judged scenes sit at wire index >= 1: scene 0 is the unjudged filler.
        assert_eq!(scenes.len(), 3);

        // Scene-alone: "BASE SCENE" (literal name, judged, index >= 1) enables no FS.
        let base_scene_idx = scenes
            .iter()
            .position(|s| s["sceneName"] == "BASE SCENE")
            .expect("a scene literally named \"BASE SCENE\"");
        assert!(
            base_scene_idx >= 1,
            "BASE SCENE must be a judged row (wire index >= 1)"
        );
        let base_scene_states = scenes[base_scene_idx]["ftswStates"]
            .as_array()
            .expect("ftswStates");
        assert!(
            base_scene_states
                .iter()
                .all(|v| v == &serde_json::json!(false)),
            "BASE SCENE (scene alone) must enable no footswitch"
        );

        // Scene-that-also-enables-an-FS: "LEAD" un-bypasses ACD_KingOfTone (switch 2,
        // BOOST) via a Full overlay AND flags it in ftswStates.
        let lead_idx = scenes
            .iter()
            .position(|s| s["sceneName"] == "LEAD")
            .expect("a scene named \"LEAD\" that enables an FS");
        assert!(
            lead_idx >= 1,
            "the scene-that-enables-an-FS must be a judged row too"
        );
        assert_eq!(
            scenes[lead_idx]["guitarNodes"]["G4"]["ACD_KingOfTone"]["dspUnitParameters"]["bypass"],
            false,
            "LEAD un-bypasses the BOOST switch's block"
        );
        assert_eq!(
            scenes[lead_idx]["ftswStates"][2], true,
            "LEAD's ftswStates must reflect switch 2 (BOOST) engaged"
        );
    }

    /// `E2E Doctor Oracle` @ 407: the Doctor spectral-check
    /// oracle fixture. Pins the per-defect param constants TABLE BELOW, which MIRRORS
    /// the fixture JSON — the fixture JSON itself is the actual HW-tuning surface for
    /// the eventual real-Doctor-path verification, and the two must be edited in
    /// lockstep — and the "base fires nothing" precondition (every one of the 5
    /// defect blocks bypassed at rest). `buried` is DELIBERATELY absent: it only fires
    /// for `Family::Bass|BassVi` with a drive detected, both impossible on this
    /// guitar-topology fixture.
    #[test]
    fn fx_doctor_oracle_fires_nothing_in_base_and_carries_its_defect_table() {
        let (name, _, p) = fixture(407);
        assert_eq!(name, "E2E Doctor Oracle");

        // Base precondition: amp+cab live, defect blocks ENABLED-NEUTRAL via single-entry
        // ftsw rows — a dual-entry row gets the whole import gutted at commit (see
        // notes/gotchas.md's "A dual-entry footswitch row…" entry). Only comp1 (SPIKY's
        // on-off target) stays bypassed in base.
        assert_eq!(base_node_bypass(&p, "ACD_HiwattDR103CanMod"), Some(false));
        assert_eq!(base_node_bypass(&p, "cab1"), Some(false));
        for node in ["eq10", "peq5", "hlp1", "plate1"] {
            assert_eq!(
                base_node_bypass(&p, node),
                Some(false),
                "{node}: defect blocks ride enabled-neutral in base (single-entry rows)"
            );
        }
        assert_eq!(base_node_bypass(&p, "comp1"), Some(true));

        // Neutrality: an enabled block must be transparent, or base fires verdicts.
        let node = |id: &str| -> serde_json::Value {
            p["audioGraph"]["guitarNodes"]["G1"]
                .as_array()
                .expect("G1")
                .iter()
                .find(|n| n["nodeId"] == id)
                .unwrap_or_else(|| panic!("{id} missing"))
                .clone()
        };
        let dp = |id: &str| node(id)["dspUnitParameters"].clone();

        // Cab identity is LOAD-BEARING for the verdict table below: FIZZY and BRIGHT are
        // seam-only BECAUSE this cab (the V30 IR on both sims, plus its 7 kHz LPF) leaves
        // no air band for a boost to lift and no room for a whole-spectrum tilt.
        assert_eq!(
            node("cab1")["FenderId"],
            "ACD_CabSimTMS",
            "cab1 model identity"
        );
        let cab = dp("cab1");
        for k in ["cabsimid", "cab2simid"] {
            assert_eq!(cab[k], "Mar1960aV30Alt", "cab1.{k}");
        }
        assert_eq!(
            cab["cabsim2enabled"].as_bool(),
            Some(true),
            "cab1.cabsim2enabled (the second V30 is in the blend)"
        );
        for k in ["lpf", "lpfcab2"] {
            assert_eq!(cab[k].as_f64(), Some(7000.0), "cab1.{k}");
        }

        let eq = dp("eq10");
        for band in [
            "gain62hz",
            "gain125hz",
            "gain250hz",
            "gain500hz",
            "gain1khz",
            "gain2khz",
            "gain4khz",
            "gain8khz",
        ] {
            assert_eq!(eq[band].as_f64(), Some(0.0), "eq10.{band} neutral in base");
        }
        let peq = dp("peq5");
        for (k, v) in [
            ("filter2frequency", 420.0),
            ("filter2q", 8.0),
            ("filter2gaindb", 0.0),
            ("filter3frequency", 2600.0),
            ("filter3q", 14.0),
            ("filter3gaindb", 0.0),
        ] {
            assert_eq!(peq[k].as_f64(), Some(v), "peq5.{k}");
        }
        // Every peq5 filter is either explicitly bypassed or pinned to an explicit
        // NEUTRAL type-2 peaking shape: the firmware fills unauthored filters as
        // ACTIVE type-4 filters at 1 kHz (q 0.707, gaindb 0), and a gaindb-based
        // neutrality check cannot see them — HW-measured 2026-08-18, the default
        // filter1/filter5 pair cost ~20 dB of level and read as muddy+dark on an
        // otherwise clean chain. filter1 (BOOMY's 80 Hz bump) and filter4 (HARSH's
        // 1.8 kHz bump) are deliberately ENABLED-neutral defect vehicles.
        assert_eq!(
            peq["filter5bypass"].as_bool(),
            Some(true),
            "peq5.filter5bypass (firmware-default active filter)"
        );
        for k in [
            "filter1bypass",
            "filter2bypass",
            "filter3bypass",
            "filter4bypass",
        ] {
            assert_eq!(
                peq[k].as_bool(),
                Some(false),
                "peq5.{k} (defect filters stay live)"
            );
        }
        for (k, v) in [
            ("filter1type", 2.0),
            ("filter1frequency", 80.0),
            ("filter1q", 1.0),
            ("filter1gaindb", 0.0),
            ("filter4type", 2.0),
            ("filter4frequency", 1800.0),
            ("filter4q", 2.5),
            ("filter4gaindb", 0.0),
        ] {
            assert_eq!(peq[k].as_f64(), Some(v), "peq5.{k}");
        }
        let hlp = dp("hlp1");
        assert_eq!(hlp["hpffc"].as_f64(), Some(20.0), "hlp1 HPF open in base");
        assert_eq!(
            hlp["lpffc"].as_f64(),
            Some(20000.0),
            "hlp1 LPF open in base"
        );
        // Steep orders are LOAD-BEARING for THIN/DARK: at the firmware-default
        // order the hpffc/lpffc toggles read as broad tilt ("bright"/nothing)
        // instead of a local band cliff ("thin"/"dark") — HW-tuned 2026-08-18.
        assert_eq!(hlp["hpforder"].as_i64(), Some(3), "hlp1.hpforder");
        assert_eq!(hlp["lpforder"].as_i64(), Some(3), "hlp1.lpforder");
        let plate = dp("plate1");
        assert_eq!(plate["wetdrymix"].as_f64(), Some(0.0), "plate dry in base");
        assert_eq!(
            plate["decay"].as_f64(),
            Some(0.9),
            "WASHED's decay is pre-baked in base (inaudible at mix 0) — the switch \
             only moves wetdrymix, keeping the row single-entry. 0.9 (with the \
             switch's 0.9 mix) puts the aligned tail ratio well past the -10 dB \
             gate; at decay 0.7 / mix 0.65 it sat 0.4 dB over it (HW 2026-08-25)"
        );

        // The per-defect param table (label -> node -> (param, valueA, valueB)),
        // P0-informed, tune-later per the module's own report. ONE param entry per
        // switch row (the dual-entry import discard above); the filter anchors the
        // old multi-entry rows carried (RESONANT/BOXY frequency+Q) now live in the
        // BASE peq5 params asserted above. SPIKY is on-off only — no verified
        // CompressorSimpleSoftKnee controlId exists anywhere in this repo; inventing
        // one risks the exact "wrong id silently no-ops" trap doctor.rs's own
        // EQ10_BANDS comment warns about.
        type DefectRow = (u32, &'static str, &'static str, &'static str, f64, f64);
        let table: &[DefectRow] = &[
            (1, "CONTROL", "eq10", "gain1khz", 0.0, 0.0),
            (2, "MUDDY", "eq10", "gain250hz", 12.0, 0.0),
            // BOOMY is seam-only: even a +18 dB peaking bump at 80 Hz reads as a
            // broad low tilt (locals[lows] +1.7 vs the 2.5 gate) — the boomy rule
            // as calibrated wants a narrow low bump with QUIET low-mids, which no
            // single accepted param produced (HW 2026-08-18).
            (3, "BOOMY", "peq5", "filter1gaindb", 18.0, 0.0),
            // HARSH standalone is seam-only (locals[high-mids] saturates ~+1.4 at
            // q 1.5–2.5); the harsh VERDICT is covered online by RESONANT's q14
            // spike, which fires harsh+resonant together (HW 2026-08-18).
            (4, "HARSH", "peq5", "filter4gaindb", 15.0, 0.0),
            // FIZZY is seam-only on this chain: the V30 cab IR leaves nothing above
            // ~10 kHz for any EQ boost to lift (HW 2026-08-18: +12 dB at 16 kHz moved
            // the air band +0.2 dB), and an on-off row bypassing the cab poisons every
            // OTHER sound's capture through the doctor's force-bypass isolation. The
            // row validates the param-write seam; no fizzy verdict fires online.
            (5, "FIZZY", "eq10", "gain16khz", 12.0, 0.0),
            (6, "LOST", "eq10", "gain500hz", -12.0, 0.0),
            // BRIGHT is seam-only: no single param tilts the whole spectrum past
            // the 3 dB/oct bright gate through this cab, and the DR103's tone-stack
            // knobs (treble/presence) are ACCEPTED by the FS param seam but change
            // nothing in the captured audio (HW 2026-08-18) — kept as the amp-knob
            // write-acceptance probe.
            (
                7,
                "BRIGHT",
                "ACD_HiwattDR103CanMod",
                "treble",
                1.0,
                0.4000000059604645,
            ),
            // CUTTHRU is the deliberate HEALTHY-change row (CONTROL's audible
            // counterpart): a mid push that helps cut through, below every gate.
            (8, "CUTTHRU", "eq10", "gain500hz", 6.0, 0.0),
            // RESONANT's q14 +18 spike fires "harsh" AND "resonant" together.
            (9, "RESONANT", "peq5", "filter3gaindb", 18.0, 0.0),
            // +18, not +12: at +12 the boxy locals sat ON the gate (+2.9/+1.8
            // across two otherwise-identical sweeps) — a coin-flip oracle row.
            (10, "BOXY", "peq5", "filter2gaindb", 18.0, 0.0),
            // THIN needs a LOCAL lows cliff, not a broad tilt: at hpffc 800 the cut
            // reads as "bright" (tilt) and at 250 the tilt margin was 0.18 dB/oct —
            // inside the 3σ repeatability band. 200 Hz + the base's 4th-order slope
            // keeps locals[lows] deep while the tilt stays clear of the bright gate.
            (11, "THIN", "hlp1", "hpffc", 200.0, 20.0),
            (12, "DARK", "hlp1", "lpffc", 1100.0, 20000.0),
            (13, "WASHED", "plate1", "wetdrymix", 0.9, 0.0),
        ];

        let ftsw = p["ftsw"].as_array().expect("ftsw");
        for (idx, label, node, pid, va, vb) in table {
            let entries = ftsw[*idx as usize]
                .as_array()
                .unwrap_or_else(|| panic!("ftsw[{idx}] ({label}): expected an entry array"));
            assert_eq!(
                entries.len(),
                1,
                "ftsw[{idx}] ({label}): exactly ONE entry per row — fw 1.8.45 discards \
                 the whole import on a dual-entry row"
            );
            let e = &entries[0];
            assert_eq!(e["func"], "param", "ftsw[{idx}] ({label})");
            assert_eq!(e["nodeId"], *node, "ftsw[{idx}] ({label})");
            assert_eq!(e["parameterId"], *pid, "ftsw[{idx}] ({label})");
            assert_eq!(
                e["valueA"].as_f64(),
                Some(*va),
                "ftsw[{idx}] ({label}) valueA"
            );
            assert_eq!(
                e["valueB"].as_f64(),
                Some(*vb),
                "ftsw[{idx}] ({label}) valueB"
            );
            assert!(
                e["valueType"].is_number(),
                "ftsw[{idx}] ({label}): param entry must carry a numeric valueType"
            );
            // A defect switch must actually CHANGE something; CONTROL is the
            // deliberate no-op discriminator.
            if *label != "CONTROL" {
                assert!(
                    va != vb,
                    "ftsw[{idx}] ({label}): the defect switch is a no-op"
                );
            }
        }

        // SPIKY (14): the one on-off row — toggles comp1 into the chain.
        let spiky = ftsw[14].as_array().expect("ftsw[14] (SPIKY)");
        assert_eq!(spiky.len(), 1, "SPIKY: exactly one entry");
        assert_eq!(spiky[0]["func"], "on-off");
        assert_eq!(
            spiky[0]["nodes"][0]["nodeId"].as_str(),
            Some("comp1"),
            "SPIKY targets comp1"
        );

        // Scene rows (15/16) stay single-entry scene funcs.
        for idx in [15usize, 16] {
            let row = ftsw[idx].as_array().expect("scene row");
            assert_eq!(row.len(), 1, "ftsw[{idx}]: exactly one entry");
            assert_eq!(row[0]["func"], "scene", "ftsw[{idx}]");
        }
    }

    /// The minimal incident repros:
    /// the SMALLEST preset still reproducing each bug class, landing ALONGSIDE the
    /// originals (404/405 stay untouched — spec migration is a later pass, not this
    /// one). Pins each repro's own structural shape; size is bounded elsewhere (the
    /// 16 KiB per-fixture field-8 budget gate, `e2e_fixtures_stay_inside_the_field8_
    /// read_budget`) rather than compared against its original here.
    #[test]
    fn fx_minimal_incident_repros_pin_their_incident_shape() {
        // 408 "E2E Preset24 Min" — the lazy-save (stale-load) TIMING bug class needs
        // only ONE BAKE-eligible drive pedal, not 405's four (the incident is about
        // base-save -> FS-batch-load ordering, not pedal count).
        let (name, _, _p405) = fixture(405);
        assert_eq!(name, "E2E Preset24");
        let (name, _, p408) = fixture(408);
        assert_eq!(name, "E2E Preset24 Min");
        assert!(p408["scenes"].as_array().expect("scenes").is_empty());
        let fs408 = crate::footswitch::enumerate_block_footswitches(&p408["ftsw"], &p408);
        assert_eq!(
            fs408.len(),
            1,
            "the minimal repro needs exactly one drive pedal"
        );
        assert_eq!(
            p408["audioGraph"]["guitarNodes"]["G1"][1]["dspUnitParameters"]["outputLevel"], 1.0,
            "the saturated amp's own knob stays untouched — 408 is its own minimal fixture, \
             unaffected by 405's Plumes-regression amendment (see `incident_fixtures_pin_their_measurement_shapes`)"
        );

        // 409 "E2E Hiwatt Min" — the scene/overlay-conformance class needs only a
        // single amp + 2 scenes (one literally "Base Scene", matching 404's own real
        // device-exported case, level.spec.ts's header) rather than 404's full
        // 4-scene parallel-amp device export. It also carries deliberate non-empty
        // overlay content for the overlay-conformance class: a DRIVE on-off switch
        // and a scene that un-bypasses the block it targets.
        let (name, _, _p404) = fixture(404);
        assert_eq!(name, "E2E Hiwatt 3S");
        let (name, _, p409) = fixture(409);
        assert_eq!(name, "E2E Hiwatt Min");
        let scenes409 = p409["scenes"].as_array().expect("scenes");
        assert_eq!(scenes409.len(), 2);
        assert!(
            scenes409.iter().any(|s| s["sceneName"] == "Base Scene"),
            "must keep the literal \"Base Scene\" name — NOT the base sentinel — that \
             the real device export exercises (level.spec.ts's own header)"
        );

        let fs409 = crate::footswitch::enumerate_block_footswitches(&p409["ftsw"], &p409);
        let drive409: Vec<_> = fs409.iter().filter(|f| f.label == "DRIVE").collect();
        assert_eq!(
            drive409.len(),
            1,
            "409 must carry exactly one DRIVE footswitch"
        );
        assert_eq!(
            drive409[0].functions.len(),
            1,
            "DRIVE must carry exactly one function"
        );
        assert_eq!(drive409[0].functions[0].func, "on-off");
        assert_eq!(
            drive409[0].functions[0].node_id, "ACD_TubeScreamer",
            "DRIVE must target ACD_TubeScreamer"
        );

        let base_scene = scenes409
            .iter()
            .find(|s| s["sceneName"] == "Base Scene")
            .expect("a scene literally named \"Base Scene\"");
        assert_eq!(
            base_scene["guitarNodes"]["G1"]["ACD_TubeScreamer"]["dspUnitParameters"]["bypass"],
            false,
            "\"Base Scene\" un-bypasses ACD_TubeScreamer — the overlay's real content \
             the non-empty overlay-conformance class needs"
        );
    }

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
    #[test]
    fn e2e_fixtures_use_device_product_id_and_unique_preset_ids() {
        let entries = fixtures();
        assert!(!entries.is_empty(), "no fixture presets found to check");

        let mut ids = Vec::new();
        for (_, _, _, p) in &entries {
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
    /// drift lock between this file and `scenario-presets.json`) is now only HALF
    /// blind: it compares decoded `BackupPresetRow`s, and that struct has carried
    /// `preset_id` since #155, so a stale `preset_id` now fails that equality
    /// compare. It stays blind to `product_id` — that field never made it onto the
    /// struct — and even for `preset_id` an equality compare can't check the things
    /// this raw-bytes gate exists for: that every `preset_id` is non-empty and that
    /// no two presets in the fixture share one. This test reads the raw `presetJson`
    /// column directly (mirroring `backup_read::read_backup_archive`'s own
    /// LZ4-frame + tar + `sqlite3` decode) instead of going through
    /// `BackupPresetRow`, so `product_id` drift and `preset_id`
    /// uniqueness/non-emptiness are both actually checked.
    #[test]
    fn backup_fixture_uses_device_product_id_and_unique_preset_ids() {
        use std::io::Read;

        let path = std::path::Path::new("../e2e/fixtures/backup-fixture.bin");
        assert!(
            path.is_file(),
            "{} is missing — it is git-tracked, so absence means a moved/renamed \
             fixture or a wrong relative path, and skipping would pass this gate \
             vacuously",
            path.display()
        );
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

    /// Every `func == "param"` ftsw entry in `p`, in ONE walk: `(total param entries,
    /// customLabels of any whose valueType is NOT a JSON number)`. A single traversal
    /// feeds both the vacuity floor and the gate predicate below so they can't drift
    /// out of sync if one filter chain is edited and not the other; the same pair also
    /// proves the negative case actually flips the predicate (forked copy, `valueType`
    /// deleted). Deliberately does NOT look at the `exp` block: its entries legitimately
    /// carry the STRING `"valueType": "float"` (verified against a verbatim device
    /// export) and must not be gated by this shape.
    fn param_footswitch_value_types(p: &serde_json::Value) -> (usize, Vec<String>) {
        let params: Vec<&serde_json::Value> = p["ftsw"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|bank| bank.as_array().into_iter().flatten())
            .filter(|entry| entry["func"] == "param")
            .collect();
        let missing = params
            .iter()
            .filter(|entry| !entry["valueType"].is_number())
            .map(|entry| {
                entry["customLabel"]
                    .as_str()
                    .unwrap_or("<unlabeled>")
                    .to_string()
            })
            .collect();
        (params.len(), missing)
    }

    /// NON-REGRESSION GATE (HW bisect 2026-08-09, fw 1.8.45): importing a preset whose
    /// `ftsw` array carries a `func: "param"` entry with NO `valueType` field makes the
    /// device silently DISCARD the whole imported preset at its lazy commit and
    /// substitute the factory-default "Guitar" body. Every param-func footswitch entry in
    /// every committed fixture must therefore carry a NUMERIC `valueType`.
    #[test]
    fn every_param_footswitch_in_every_fixture_carries_a_numeric_value_type() {
        let mut checked = 0usize;
        for (idx, name, _, p) in fixtures() {
            let (total, missing) = param_footswitch_value_types(&p);
            checked += total;
            assert!(
                missing.is_empty(),
                "{name} ({idx}): param-func footswitches missing a numeric valueType: \
                 {missing:?} — fw 1.8.45 silently replaces the WHOLE imported preset with \
                 the factory-default body when a param-func switch lacks valueType (HW \
                 bisect 2026-08-09)"
            );
        }
        assert!(
            checked >= 4,
            "expected at least the four known param-func footswitches across the fixture \
             set ({checked} found) — a schema rename would otherwise make this gate pass \
             vacuously"
        );

        // NEGATIVE CHECK: fork E2E Rig's ftsw[9] ("VERB KILL") param entry with
        // `valueType` deleted and confirm the SAME helper then reports it missing, so
        // the assert above can't be vacuously true from a predicate that never actually
        // inspects `valueType`.
        let (_, _, rig) = fixture(400);
        let mut forked = rig.clone();
        let removed = forked["ftsw"][9][0]
            .as_object_mut()
            .expect("VERB KILL param entry")
            .remove("valueType");
        assert_eq!(
            removed,
            Some(serde_json::json!(2)),
            "E2E Rig ftsw[9] moved or lost its valueType — update this fork's index/value"
        );
        let (_, missing) = param_footswitch_value_types(&forked);
        assert!(
            !missing.is_empty(),
            "deleting valueType from a param entry must make the gate condition fail"
        );
    }

    /// NON-REGRESSION GATE (HW bisect 2026-08-18): every fixture ftsw ROW must carry at
    /// most ONE entry — a dual-entry row makes fw 1.8.45 silently replace the whole
    /// imported preset with an EMPTY body. See notes/gotchas.md's "A dual-entry footswitch
    /// row makes the firmware silently replace the whole imported preset with an EMPTY
    /// body" entry.
    #[test]
    fn every_fixture_footswitch_row_carries_at_most_one_entry() {
        fn stacked_rows(p: &serde_json::Value) -> Vec<usize> {
            p["ftsw"]
                .as_array()
                .map(|rows| {
                    rows.iter()
                        .enumerate()
                        .filter(|(_, r)| r.as_array().is_some_and(|a| a.len() > 1))
                        .map(|(i, _)| i)
                        .collect()
                })
                .unwrap_or_default()
        }
        let mut singles = 0usize;
        for (idx, name, _, p) in fixtures() {
            singles += p["ftsw"].as_array().map_or(0, |rows| {
                rows.iter()
                    .filter(|r| r.as_array().is_some_and(|a| a.len() == 1))
                    .count()
            });
            let stacked = stacked_rows(&p);
            assert!(
                stacked.is_empty(),
                "{name} ({idx}): ftsw rows {stacked:?} stack multiple entries — see \
                 notes/gotchas.md's dual-entry footswitch discard entry"
            );
        }
        assert!(
            singles >= 10,
            "expected the fixture set to carry populated single-entry rows ({singles} \
             found) — a schema rename would otherwise make this gate pass vacuously"
        );

        // NEGATIVE CHECK: fork the oracle's MUDDY row with a second entry and
        // confirm the same predicate flags exactly that row.
        let (_, _, oracle) = fixture(407);
        let mut forked = oracle.clone();
        let extra = forked["ftsw"][2][0].clone();
        assert!(
            extra["func"] == "param",
            "oracle ftsw[2] moved — update this fork's index"
        );
        forked["ftsw"][2]
            .as_array_mut()
            .expect("MUDDY row")
            .push(extra);
        assert_eq!(stacked_rows(&forked), vec![2]);
    }

    /// NON-REGRESSION GATE for the fixture-scene corruption class (real 1.8.45 unit,
    /// 2026-07-28). The device silently DROPS a preset's ENTIRE `scenes[]` (and
    /// re-stamps `info.source_id` to its placeholder) the first time a scene is
    /// materialised (`loadScene`) and the preset saved, when the scenes are not
    /// fully device-conformant. HW isolation (`probe --scene-write-cell`, recall-only,
    /// no scene-edit, no write): the hand-built "E2E Reference" wiped on every
    /// recall+save while the device-authored "E2E Hiwatt 3S" survived identical ops;
    /// conformance-rebuilding Reference/Realistic made them survive. This corrupted
    /// the on-device fixture after every ONLINE level run (specs green, slot 400
    /// marker-less, teardown's guarded clear refusing) — the failure mode this gate
    /// exists to keep dead.
    ///
    /// What "conformant" means (oracle: the device-authored Hiwatt scenes):
    ///   * every scene carries the full 12-key shape (a 3-key sparse scene wipes);
    ///   * `splitMix` holds BOTH `mixPoints` and `splitPoints` (scene shape: objects
    ///     keyed by nodeId — the missing `splitPoints` was the final discriminator);
    ///   * overlay numerics are floats (the device authors no int scene params);
    ///   * `ftswStates` is exactly as long as the preset's `ftsw` switch list.
    #[test]
    fn e2e_fixture_scenes_are_device_conformant() {
        const SCENE_KEYS: [&str; 12] = [
            "ampControl",
            "ftswStates",
            "fxLoop",
            "fxLoop1SceneEdit",
            "fxLoop2SceneEdit",
            "guitarNodes",
            "micNodes",
            "midi",
            "sceneName",
            "spillover",
            "splitMix",
            "uuid",
        ];
        // Per-fixture expected scene count (listIndex → count), replacing a single
        // cross-fixture sum: a failure now names the culprit fixture instead of just
        // reporting the total drifted. A slot absent here (401, 405, 408) is expected
        // to carry NO scenes. 406/407/409 are the P3 leveling-doctor-fixtures
        // additions: 406 "E2E Combined Level" carries 3 (SCRATCH filler at wire index
        // 0 + BASE SCENE + LEAD, both judged rows at index ≥1 — scene 0 of a 2-amp
        // preset is never judged, since USB `loadScene(0)` can materialize a
        // different amp state than the physical footswitch tap; see `danger.md`'s
        // OPEN scene-0 item); 407 "E2E Doctor Oracle" carries 2 (SCRATCH filler + the
        // scene-consistency oracle's own big-outputLevel-jump scene); 409
        // "E2E Hiwatt Min" carries 2 (the minimal scene/overlay-conformance repro). 410
        // "E2E Friedman 3S" (P4) carries 3, all judged FULL-overlay rows: Rhythm/Lead/
        // Base Scene, each carrying only its own amp outputLevel overlay.
        let expected_scenes: std::collections::HashMap<u32, usize> = [
            (400u32, 4usize),
            (402, 8),
            (403, 4),
            (404, 4),
            (406, 3),
            (407, 2),
            (409, 2),
            (410, 3),
        ]
        .into_iter()
        .collect();
        let entries = fixtures();
        for (idx, name, _, p) in &entries {
            let scenes = p["scenes"].as_array().map_or(0, Vec::len);
            let expected = expected_scenes.get(idx).copied().unwrap_or(0);
            assert_eq!(
                scenes, expected,
                "{name:?} ({idx}): expected {expected} scenes, found {scenes} — a schema \
                 rename that hid the scenes would pass this gate vacuously"
            );
        }
        for (_, name, _, p) in &entries {
            let ftsw_len = p["ftsw"].as_array().map_or(0, Vec::len);
            for scene in p["scenes"].as_array().into_iter().flatten() {
                let sn = scene["sceneName"].as_str().unwrap_or("<unnamed scene>");
                let mut keys: Vec<&str> = scene
                    .as_object()
                    .expect("scene is an object")
                    .keys()
                    .map(String::as_str)
                    .collect();
                keys.sort_unstable();
                assert_eq!(
                    keys, SCENE_KEYS,
                    "{name:?} scene {sn:?}: must carry exactly the 12-key device scene \
                     shape — a sparse scene makes the unit wipe the whole scenes[] on \
                     the first loadScene+save"
                );
                for part in ["mixPoints", "splitPoints"] {
                    assert!(
                        scene["splitMix"][part]
                            .as_object()
                            .is_some_and(|m| !m.is_empty()),
                        "{name:?} scene {sn:?}: splitMix.{part} must be a non-empty \
                         nodeId-keyed object (missing splitPoints was the HW-isolated \
                         wipe trigger)"
                    );
                }
                assert_eq!(
                    scene["ftswStates"].as_array().map_or(0, Vec::len),
                    ftsw_len,
                    "{name:?} scene {sn:?}: ftswStates must be as long as ftsw"
                );
                for group in ["guitarNodes", "micNodes"] {
                    for (gid, nodes) in scene[group].as_object().into_iter().flatten() {
                        for (nid, body) in nodes.as_object().into_iter().flatten() {
                            for (pk, pv) in
                                body["dspUnitParameters"].as_object().into_iter().flatten()
                            {
                                assert!(
                                    !pv.is_i64() && !pv.is_u64(),
                                    "{name:?} scene {sn:?} {group}/{gid}/{nid}.{pk}: \
                                     int-typed overlay param {pv} — the device authors \
                                     floats in scenes; write {pv}.0"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// Sidecar/fixture CROSS-CHECK: `scenario-loudness.json`'s C table references
    /// fixture shape — scene counts, `leveledParams` group/node/param triples,
    /// `offbranchSwitchNode` — that `scenario-presets.json` must actually carry. A
    /// mismatch fails SILENTLY as a flat response (the leveled-param predicate in
    /// `sim_device::model_lufs` simply never activates, or activates against a node
    /// that doesn't exist), which cost a wrong-root-cause debugging pass once
    /// (COVERAGE.md row 18's corrected cause). This gate makes that class loud.
    #[test]
    fn sidecar_references_resolve_against_the_fixtures() {
        let sidecar_raw = std::fs::read_to_string("../e2e/fixtures/scenario-loudness.json")
            .expect("read scenario-loudness.json");
        let sidecar: serde_json::Value =
            serde_json::from_str(&sidecar_raw).expect("scenario-loudness.json is JSON");
        let slots = sidecar["slots"]
            .as_object()
            .expect("sidecar has a slots object");
        assert!(!slots.is_empty(), "no sidecar slots found to check");

        let entries = fixtures();
        for (slot_key, entry) in slots {
            let idx: u32 = slot_key
                .parse()
                .unwrap_or_else(|_| panic!("sidecar slot key {slot_key:?} is not a list index"));
            // (a) a fixture exists at that list index.
            let (_, name, _, p) = entries
                .iter()
                .find(|(i, ..)| *i == idx)
                .unwrap_or_else(|| panic!("sidecar slot {idx} has no fixture at that list index"));

            // (b) a sidecar `scenes` array's length matches the fixture's own scene count.
            //
            // NON-VACUITY FLOOR: the length check only fires when the sidecar HAS a
            // `scenes` array, so a slot that simply drops its scene C table would sail
            // through — the fixture's scenes would then all model at the flat base C, a
            // silently flat response of exactly the kind COVERAGE row 18's corrected cause
            // was. So a fixture that carries scenes MUST have a sidecar array (the converse
            // is not required: a scene-free fixture legitimately declares none).
            let fixture_scenes = p["scenes"].as_array().map_or(0, Vec::len);
            let sidecar_scenes = entry.get("scenes").and_then(|v| v.as_array());
            assert!(
                fixture_scenes == 0 || sidecar_scenes.is_some(),
                "{name:?} ({idx}): the fixture carries {fixture_scenes} scenes but the \
                 sidecar declares no `scenes` C array — every scene would model at the flat \
                 base C (a silently flat response, not a loud failure)"
            );
            if let Some(sidecar_scenes) = sidecar_scenes {
                assert_eq!(
                    sidecar_scenes.len(),
                    fixture_scenes,
                    "{name:?} ({idx}): sidecar declares {} scene C values but the fixture \
                     carries {fixture_scenes} scenes",
                    sidecar_scenes.len()
                );
            }

            // (c) every `leveledParams` entry resolves to a real node + param.
            for lp in entry
                .get("leveledParams")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
            {
                let group = lp["group"].as_str().expect("leveledParams.group");
                let node = lp["node"].as_str().expect("leveledParams.node");
                let param = lp["param"].as_str().expect("leveledParams.param");
                let node_entry = p["audioGraph"]["guitarNodes"][group]
                    .as_array()
                    .unwrap_or_else(|| {
                        panic!(
                            "{name:?} ({idx}): leveledParams group {group:?} has no \
                             guitarNodes entry"
                        )
                    })
                    .iter()
                    .find(|n| n["nodeId"].as_str() == Some(node))
                    .unwrap_or_else(|| {
                        panic!(
                            "{name:?} ({idx}): leveledParams node {node:?} not found in \
                             guitarNodes.{group}"
                        )
                    });
                assert!(
                    node_entry
                        .get("dspUnitParameters")
                        .and_then(|d| d.get(param))
                        .is_some(),
                    "{name:?} ({idx}): leveledParams param {param:?} not found on {node:?}'s \
                     dspUnitParameters — the leveled-param curve would silently never \
                     activate"
                );
            }

            // (d) `offbranchSwitchNode`, where present, also resolves to a real node.
            if let Some(node) = entry.get("offbranchSwitchNode").and_then(|v| v.as_str()) {
                let groups = p["audioGraph"]["guitarNodes"]
                    .as_object()
                    .expect("guitarNodes is an object");
                let found = groups.values().any(|nodes| {
                    nodes
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|n| n["nodeId"].as_str() == Some(node))
                });
                assert!(
                    found,
                    "{name:?} ({idx}): offbranchSwitchNode {node:?} not found in any \
                     guitarNodes group"
                );
            }
        }

        // (e) NON-VACUITY FLOOR for `leveledParams`. Everything above only checks entries
        // that EXIST — delete a slot's whole `leveledParams` array and every assertion here
        // passes while the offline model silently reverts to the flat C law for that slot.
        // That is not hypothetical: it is COVERAGE row 18's corrected cause verbatim (400
        // declared none, `model_lufs` returned the same C at every probe, and the solve took
        // its no-authority `flat_response` exit without ever probing the wet floor).
        //
        // The two slots whose SPECS depend on a knob-driven curve are pinned by name:
        //   400 — `ACD_TMSpring63.mix` on the `wetMix` curve (row 18's wet-floor gates).
        //   405 — the four drive pedals' own level knobs on `saturatedPedal` (row 16's BAKE
        //         lane, `level-fs-preset24.spec.ts`'s whole premise).
        // The other slots deliberately declare none (the flat law IS their model), so this
        // is an explicit allowlist rather than a blanket "every slot must declare one".
        for want in [400u32, 405] {
            let declared = slots
                .get(&want.to_string())
                .and_then(|e| e.get("leveledParams"))
                .and_then(|v| v.as_array())
                .map_or(0, Vec::len);
            assert!(
                declared > 0,
                "sidecar slot {want} must declare at least one `leveledParams` entry — \
                 without it `sim_device::model_lufs` returns the same C at every probe and \
                 that slot's knob-driven specs pass against a flat response"
            );
        }
    }

    /// Every row `COVERAGE.md`'s coverage matrix marks as covered by a Playwright spec
    /// (its Spec cell names a `.spec.ts` file, parsed loosely — a drift alarm, not a
    /// parser) must be CITED by a `// COVERAGE row(s) N[, M...]` comment in **one of the
    /// spec files that row's OWN Spec cell names**. Without this, the matrix's own claim
    /// ("every structural fact is pinned by a test") is unverifiable prose — a row and the
    /// spec that's supposed to prove it can drift apart with nothing failing.
    ///
    /// PER-ROW, NOT UNIONED. The gate used to pool every citation in `e2e/specs/` into one
    /// set, so row N passed as long as ANY spec file mentioned N — a row whose Spec cell
    /// named `a.spec.ts` while only `b.spec.ts` cited it read green, which is precisely the
    /// drift the gate exists to catch (row 16 was in that state: cell → `level-fs-preset24
    /// .spec.ts`, citation → `level-setup.spec.ts`).
    ///
    /// Returns row → the spec filenames its cell names. A cell may name SEVERAL (rows 18
    /// and 37 legitimately do), and one may sit inside a parenthetical sentence rather
    /// than leading the cell (row 30), so filenames are pulled by pattern from anywhere in
    /// the cell and the requirement is "at least one of them cites the row".
    fn coverage_matrix_rows_citing_a_spec_ts(
        md: &str,
    ) -> std::collections::HashMap<u32, Vec<String>> {
        md.lines()
            .filter_map(|line| {
                let cells: Vec<&str> = line.split('|').map(str::trim).collect();
                if cells.len() < 4 {
                    return None;
                }
                let row: u32 = cells[1].parse().ok()?;
                let files = spec_filenames_in(cells[cells.len() - 2]);
                (!files.is_empty()).then_some((row, files))
            })
            .collect()
    }

    /// Every `<name>.spec.ts` filename mentioned anywhere in one Spec cell. A filename is
    /// the longest run of filename-ish characters ending at a `.spec.ts` occurrence —
    /// tolerant of the backticks, parentheses and prose the cells actually carry.
    fn spec_filenames_in(cell: &str) -> Vec<String> {
        const SUFFIX: &str = ".spec.ts";
        let mut out: Vec<String> = Vec::new();
        let mut from = 0usize;
        while let Some(rel) = cell.get(from..).and_then(|s| s.find(SUFFIX)) {
            let end = from + rel + SUFFIX.len();
            let stem_end = from + rel;
            let start = cell[..stem_end]
                .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_'))
                .map_or(0, |i| i + 1);
            let name = &cell[start..end];
            // A bare ".spec.ts" with no stem names no file — skip rather than record it.
            if name.len() > SUFFIX.len() && !out.iter().any(|n| n == name) {
                out.push(name.to_string());
            }
            from = end;
        }
        out
    }

    /// Row numbers cited by `// COVERAGE row N` / `// COVERAGE rows N, M, ...` comments in
    /// one spec file's text. Tolerant of the separators actually in use (comma, slash,
    /// whitespace) rather than a strict parser — a citation is read up to the first
    /// character that ISN'T a digit/`,`/`/`/space, then split into individual numbers on
    /// any non-digit run.
    fn coverage_row_citations(content: &str) -> std::collections::HashSet<u32> {
        let marker = "COVERAGE row";
        let bytes = content.as_bytes();
        let mut out = std::collections::HashSet::new();
        let mut search_from = 0usize;
        while let Some(rel) = content.get(search_from..).and_then(|s| s.find(marker)) {
            let start = search_from + rel + marker.len();
            let mut i = start;
            if bytes.get(i) == Some(&b's') {
                i += 1; // optional "rows"
            }
            let window_start = i;
            while matches!(bytes.get(i), Some(b'0'..=b'9' | b',' | b'/' | b' ')) {
                i += 1;
            }
            for tok in content[window_start..i].split(|c: char| !c.is_ascii_digit()) {
                if let Ok(n) = tok.parse::<u32>() {
                    out.insert(n);
                }
            }
            search_from = i.max(start + 1);
        }
        out
    }

    #[test]
    fn coverage_rows_marked_playwright_covered_are_cited_by_some_spec() {
        let md = std::fs::read_to_string("../e2e/fixtures/COVERAGE.md").expect("read COVERAGE.md");
        let covered_rows = coverage_matrix_rows_citing_a_spec_ts(&md);
        assert!(
            covered_rows.len() > 10,
            "expected a healthy number of Playwright-covered matrix rows ({} found) — a \
             table reformat that hid the Spec column would otherwise pass this gate \
             vacuously",
            covered_rows.len()
        );

        let specs_dir = std::path::Path::new("../e2e/specs");
        assert!(specs_dir.is_dir(), "{} is missing", specs_dir.display());
        // filename → the rows THAT file cites. Kept per-file (not pooled) so each row can
        // be checked against its own Spec cell's files.
        let mut cited_by_file: std::collections::HashMap<String, std::collections::HashSet<u32>> =
            std::collections::HashMap::new();
        let mut all_cited: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for entry in std::fs::read_dir(specs_dir).expect("read e2e/specs") {
            let path = entry.expect("dir entry").path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.ends_with(".spec.ts") {
                continue;
            }
            let content = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let rows = coverage_row_citations(&content);
            all_cited.extend(rows.iter().copied());
            cited_by_file.insert(name.to_string(), rows);
        }
        let spec_files_seen = cited_by_file.len();
        assert!(
            spec_files_seen > 10,
            "expected many *.spec.ts files under e2e/specs ({spec_files_seen} found) — a \
             directory layout change would otherwise make this gate pass vacuously"
        );
        assert!(
            all_cited.len() >= 8,
            "expected a healthy number of distinct cited rows ({} found) across every \
             spec file — a comment-format drift would otherwise pass this gate vacuously",
            all_cited.len()
        );

        // Each row must be cited BY ONE OF THE FILES ITS OWN CELL NAMES. A cell naming a
        // file that doesn't exist is its own failure (a rename that left the matrix behind).
        let mut missing: Vec<String> = Vec::new();
        let mut rows: Vec<(&u32, &Vec<String>)> = covered_rows.iter().collect();
        rows.sort_by_key(|(r, _)| **r);
        for (row, files) in rows {
            for f in files {
                assert!(
                    cited_by_file.contains_key(f),
                    "COVERAGE.md row {row}'s Spec cell names {f}, which does not exist \
                     under e2e/specs — fix the cell or the filename"
                );
            }
            if !files
                .iter()
                .any(|f| cited_by_file.get(f).is_some_and(|c| c.contains(row)))
            {
                missing.push(format!("row {row} → {}", files.join(" | ")));
            }
        }
        assert!(
            missing.is_empty(),
            "COVERAGE.md marks these rows as covered by a named Playwright spec, but NONE \
             of the files that row's own Spec cell names carries a `// COVERAGE row(s) N` \
             comment for it: {missing:?} — either add the citation inside the named spec, \
             or fix COVERAGE.md's Spec cell to name the file that actually covers it. (A \
             citation in some OTHER spec file no longer counts: that is the drift this \
             gate exists to catch.)"
        );
    }

    /// Locate `scripts/e2e.sh`'s default-SPECS resolve line and parse the space-separated
    /// spec names out of its literal `SPECS=(…)` — shared by the two gates below so the
    /// parse itself cannot drift between them (it did once: see the mirror gate's own
    /// history). Takes the file's already-read text rather than a path so a caller that
    /// needs the raw text too (for a different reason) only reads the file once.
    fn parse_e2e_default_online_specs(sh: &str) -> Vec<&str> {
        let set_line = sh
            .lines()
            .find(|l| l.contains("SPECS=(") && l.contains("all"))
            .expect("scripts/e2e.sh: the default-SPECS resolve line (SPECS=(…)) is gone");
        let after_open = set_line
            .split_once("SPECS=(")
            .expect("SPECS=( not found on the matched line")
            .1;
        let inside = after_open
            .split_once(')')
            .expect("SPECS=( has no closing paren on the matched line")
            .0;
        inside.split_whitespace().collect()
    }

    /// Every `*.online.spec.ts` must appear EITHER in `scripts/e2e.sh`'s default online
    /// SPECS line OR in [`ON_DEMAND_ONLINE_SPECS`] below. `doctor-apply.online` sat
    /// outside the hand-maintained literal and "had never run in either tier despite
    /// existing to be the one-off HW validation" (e2e.sh's own comment) — this gate makes
    /// that ACCIDENTAL failure mode impossible to repeat, while still allowing a spec to
    /// be DELIBERATELY on-demand-only (trade T1, ONLINE e2e consolidation) as long as
    /// that's a conscious, reviewed addition to the allowlist below, not a silent drop.
    #[test]
    fn every_online_spec_is_in_the_default_online_set() {
        // On-demand-only BY DESIGN, not by accident: each entry here must already be
        // documented at its own exclusion site in `scripts/e2e.sh` (a "run it explicitly
        // with…" note) and in its own spec file's header. Adding an entry here is how a
        // future spec opts OUT of the default online sweep — do so deliberately.
        const ON_DEMAND_ONLINE_SPECS: &[&str] = &["doctor-apply.online"];

        let sh = std::fs::read_to_string("../scripts/e2e.sh").expect("read scripts/e2e.sh");
        let default_specs = parse_e2e_default_online_specs(&sh);
        let mut checked = 0;
        for entry in std::fs::read_dir("../e2e/specs").expect("read e2e/specs") {
            let path = entry.expect("dir entry").path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(spec) = name.strip_suffix(".spec.ts") else {
                continue;
            };
            if !spec.ends_with(".online") {
                continue;
            }
            checked += 1;
            let in_default_set = default_specs.contains(&spec);
            let on_demand_by_design = ON_DEMAND_ONLINE_SPECS.contains(&spec);
            assert!(
                in_default_set || on_demand_by_design,
                "{name} exists but '{spec}' is neither in scripts/e2e.sh's default online \
                 SPECS set nor in this test's ON_DEMAND_ONLINE_SPECS allowlist — it would \
                 never run in ANY tier (offline testIgnores *.online.spec.ts). Add it to the \
                 default set, or — if it's deliberately on-demand-only — to the allowlist \
                 above with a matching exclusion note in scripts/e2e.sh."
            );
        }
        assert!(
            checked >= 2,
            "expected at least 2 *.online.spec.ts files ({checked} found) — a naming-scheme \
             change would otherwise make this gate pass vacuously"
        );
    }

    /// The online tier has TWO hand-maintained mirrors of the same spec set:
    /// `scripts/e2e.sh`'s default-arm `SPECS=(…)` literal (kept literal, not a command
    /// substitution, so the gate above can grep it — and for readability) and
    /// `scripts/gates.sh`'s `ONLINE_SPEC_SET="…"` line (what `--record-online` writes and
    /// `--check-online` requires). Drift between them means a spec that stamps but never
    /// runs, or runs but never gets a stamp requirement — this test parses both files the
    /// same way the sibling gate above parses e2e.sh (via the shared helper), and fails
    /// loud if they disagree.
    #[test]
    fn every_online_spec_set_mirror_matches_between_e2e_sh_and_gates_sh() {
        let sh = std::fs::read_to_string("../scripts/e2e.sh").expect("read scripts/e2e.sh");
        let mut e2e_specs = parse_e2e_default_online_specs(&sh);

        let gates = std::fs::read_to_string("../scripts/gates.sh").expect("read scripts/gates.sh");
        let spec_line = gates
            .lines()
            .find(|l| l.starts_with("ONLINE_SPEC_SET="))
            .expect("scripts/gates.sh: the ONLINE_SPEC_SET=\"…\" line is gone");
        let mut gates_specs: Vec<&str> = spec_line
            .trim_start_matches("ONLINE_SPEC_SET=")
            .trim_matches('"')
            .split_whitespace()
            .collect();

        assert!(
            !e2e_specs.is_empty() && !gates_specs.is_empty(),
            "parsed an empty spec set from e2e.sh ({e2e_specs:?}) or gates.sh ({gates_specs:?}) \
             — a naming-scheme change would otherwise make this gate pass vacuously"
        );
        e2e_specs.sort_unstable();
        gates_specs.sort_unstable();
        assert_eq!(
            e2e_specs, gates_specs,
            "scripts/e2e.sh's default SPECS=(…) literal and scripts/gates.sh's \
             ONLINE_SPEC_SET=\"…\" line have drifted apart — keep both hand-maintained \
             mirrors of the online tier in sync"
        );
    }

    /// `scripts/validate-hbe.sh` carries an executable SHELL MIRROR of
    /// [`probe_api::SCRATCH_SLOTS`] (its own guard refuses slots outside it). Its comment
    /// says "update this line in the SAME commit" — this gate enforces that instead of
    /// trusting it: the mirror drifted to 400–405 once while the zone was 400–409, which
    /// would have refused four legitimate scratch slots.
    #[test]
    fn validate_hbe_scratch_slots_mirror_matches_the_rust_declaration() {
        let sh =
            std::fs::read_to_string("../scripts/validate-hbe.sh").expect("read validate-hbe.sh");
        let line = sh
            .lines()
            .find(|l| l.starts_with("SCRATCH_SLOTS="))
            .expect("validate-hbe.sh: the SCRATCH_SLOTS mirror line is gone");
        let mirrored: Vec<u32> = line
            .trim_start_matches("SCRATCH_SLOTS=")
            .trim_matches('"')
            .split_whitespace()
            .map(|s| s.parse().expect("mirror entries are slot numbers"))
            .collect();
        assert_eq!(
            mirrored,
            crate::probe_api::SCRATCH_SLOTS.to_vec(),
            "scripts/validate-hbe.sh's SCRATCH_SLOTS mirror != probe_api::SCRATCH_SLOTS — \
             update the script in the same commit as the Rust declaration"
        );
    }
}
