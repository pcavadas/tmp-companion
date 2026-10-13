//! Probe entry point: batched-live per-scene leveling benchmark harness.
//!
//! It also holds the retired live-batched runner ([`level_scenes_live_batched`]) and its
//! live controllers ([`SceneLevelStrategy`]): kept only for `probe --bench-scene-leveling`
//! and for the tests that show why the hybrid controller won, and moved here out of the
//! production `leveller` module. The app levels scenes with
//! `leveller::level_scenes_oneshot`.

use super::stimulus::probe_stimulus_path;
use crate::audio;
use crate::error::LevelError;
use crate::leveller::{
    coord_to_knob, knob_search_space, knob_to_coord, load_preset_isolated, measure_processed,
    reamp_off_fresh, restore_after_unsaved_error, restore_saved_preset, secant_step, set_knob,
    set_knob_value_only, BatchedSceneOutcome, CancelMode, Flatness, KnobTarget, SceneJob,
    Verification, BATCH_TRUST_DB, KNOB_TOL_LU, RATE, SETTLE_AFTER_REAMP_MS, SETTLE_AFTER_SET_MS,
};
use crate::proto;
use crate::scene_jobs::build_scene_jobs;
use crate::scene_jobs::filter_amp_candidates;
use crate::session;
use crate::session::Session;
use crate::slot::ListIndex;
use crate::stimulus::read_stimulus_calibrated;
use crate::SCENE_LEVEL_CANCEL;
use serde::Serialize;
use std::sync::atomic::Ordering::SeqCst;
use std::time::Duration;

pub fn probe_bench_scene_leveling(
    slots: Vec<ListIndex>,
    target_lufs: f64,
    topology_id: String,
    out_path: String,
) -> Result<String, String> {
    if slots.is_empty() {
        return Err("no preset slots supplied".to_string());
    }
    if !target_lufs.is_finite() {
        return Err("target LUFS must be finite".to_string());
    }
    let stim_path = probe_stimulus_path(&topology_id)?;
    let cal = std::env::var("TMP_LEVELLER_CAL_LUFS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    let stim = read_stimulus_calibrated(&stim_path, cal)?;
    let save = matches!(
        std::env::var("TMP_SCENE_LEVEL_SAVE").ok().as_deref(),
        Some("1" | "true" | "yes")
    );
    let include_base = !matches!(
        std::env::var("TMP_SCENE_LEVEL_INCLUDE_BASE")
            .ok()
            .as_deref(),
        Some("0" | "false" | "no")
    );
    // BatchedLive only: the phase-1 matrix established the
    // per-scene numbers (legacy 80-93 s/scene; isolated live rows 22-45 s/scene
    // dominated by per-scene session ceremony; liveSecant noise-fragile,
    // proportional stalls on compressed knobs, full jumps overshoot steep ones).
    // BatchedLive amortizes ONE session + ONE re-amp engage across a preset's
    // scenes with trust-region slope jumps — the <10 s/scene candidate.
    let mut rows = Vec::<SceneLevelBenchmarkRow>::new();
    // Stream each row to disk as it lands (an OOM reboot lost a full 40-minute
    // run that only wrote at the end).
    let jsonl_path = format!("{out_path}.jsonl");
    let emit = |row: &SceneLevelBenchmarkRow| {
        if let Ok(line) = serde_json::to_string(row) {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&jsonl_path)
            {
                let _ = writeln!(f, "{line}");
            }
        }
    };
    // The bench glue chains many short sessions; 800 ms between them is the
    // EMPIRICAL sweet spot (HW A/B): at 800 ms the open after a
    // live-cadence session close succeeded 4/4; at 1500 ms it failed 0xe00002c5
    // even on a freshly power-cycled unit — the device seems to accept a QUICK
    // re-open after a close, then lock out for minutes. Do not "safely" widen.
    let session_gap = || std::thread::sleep(std::time::Duration::from_millis(800));

    for slot in slots {
        // Scenes AND level blocks from ONE rich session (the `--scenes-load`
        // recipe + the monitor's graph-from-push-bodies approach): heartbeat
        // warmup → send_and_collect LoadPreset → pump; the load's pushes carry
        // BOTH the sceneListResponse(125) and the field-3 currentPresetDataChanged
        // (full audioGraph) on a live-cadence session. One session because every
        // alternative failed on HW: field-8 reads and extra re-opens
        // wedge the device's next exclusive open, and `connect_for_discovery`'s
        // field-78 burst kills field-3 delivery for its whole session on 1.8.45.
        eprintln!(
            "[bench] preset {:03}: load + scene + block-discovery session…",
            slot.device_slot()
        );
        type PresetIntel = (
            Option<Vec<String>>,
            Vec<session::LevelBlock>,
            Vec<(u32, Option<serde_json::Value>)>,
        );
        let intel: Result<PresetIntel, String> = (|| {
            let mut s = Session::connect()?;
            // Dense heartbeats ~2 s: the device only pushes unsolicited data
            // (125, PresetLoaded, field-3) on a live-cadence session.
            for _ in 0..16 {
                s.heartbeat()?;
                s.pump_silent(120)?;
            }
            s.raw.clear();
            s.send_and_collect(&proto::load_my_preset(slot.device_slot()), 300)?;
            let mut scenes: Option<Vec<String>> = None;
            let mut seen = 0usize;
            // Keep pumping past the 125 hit — the multi-packet field-3 push
            // (block discovery) needs the extra turns to finish arriving.
            for _ in 0..10 {
                s.heartbeat()?;
                s.pump_silent(200)?;
                let bodies = s.push_bodies();
                for b in bodies.iter().skip(seen) {
                    if let Some(names) = session::decode_scene_list(b) {
                        scenes = Some(names);
                    }
                }
                seen = bodies.len();
            }
            if scenes.is_none() {
                // Explicit request fallback — WITHOUT raw.clear() (the field-3
                // payload for block discovery lives in the accumulated raw).
                let _ = s.send_and_collect(&proto::scene_list_request(), 300);
                for _ in 0..4 {
                    if let Some(names) = s
                        .push_bodies()
                        .iter()
                        .find_map(|b| session::decode_scene_list(b))
                    {
                        scenes = Some(names);
                        break;
                    }
                    let _ = s.pump_collect_alive(200);
                }
            }
            let blocks = s.current_preset_blocks()?;
            // Un-engaged per-scene doc pre-pass (same session): loadScene →
            // field-3 push → the scene's LIVE doc, for the knob pick + that
            // scene's actual knob value. Must happen BEFORE any engage — the
            // device pushes no field-3 while re-amp is engaged.
            let mut docs: Vec<(u32, Option<serde_json::Value>)> =
                vec![(session::BASE_SCENE_SLOT, s.current_preset_value().ok())];
            if let Some(names) = &scenes {
                for idx in 0..names.len() as u32 {
                    s.raw.clear();
                    s.send_and_collect(&proto::load_scene(idx as u64), 300)?;
                    let mut doc = None;
                    for _ in 0..4 {
                        s.heartbeat()?;
                        s.pump_silent(150)?;
                        if let Ok(v) = s.current_preset_value() {
                            doc = Some(v);
                            break;
                        }
                    }
                    docs.push((idx, doc));
                }
            }
            Ok((scenes, blocks, docs))
        })();
        let (scenes, discovered, docs) = match intel {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "[bench] preset {:03}: intel session failed: {e}",
                    slot.device_slot()
                );
                let row = SceneLevelBenchmarkRow {
                    preset_slot: slot,
                    ui_label: format!("{:03}", slot.device_slot()),
                    scene_slot: session::BASE_SCENE_SLOT,
                    scene_name: "Base".to_string(),
                    strategy: SceneLevelStrategy::BatchedLive,
                    elapsed_ms: 0,
                    capture_windows: 0,
                    parameter_writes: 0,
                    final_lufs: None,
                    error_lu: None,
                    final_output_level: None,
                    clamped: false,
                    saved: false,
                    failure: Some(e),
                };
                emit(&row);
                rows.push(row);
                continue;
            }
        };
        if scenes.is_none() {
            eprintln!(
                "[bench] preset {:03}: no scene list harvested; Base only",
                slot.device_slot()
            );
        }
        session_gap();
        let mut scene_rows: Vec<(u32, String)> = if include_base {
            vec![(session::BASE_SCENE_SLOT, "Base".to_string())]
        } else {
            Vec::new()
        };
        if let Some(names) = scenes {
            scene_rows.extend(
                names
                    .into_iter()
                    .enumerate()
                    .map(|(idx, name)| (idx as u32, name)),
            );
        }
        if scene_rows.is_empty() {
            let row = SceneLevelBenchmarkRow {
                preset_slot: slot,
                ui_label: format!("{:03}", slot.device_slot()),
                scene_slot: session::BASE_SCENE_SLOT,
                scene_name: "Base".to_string(),
                strategy: SceneLevelStrategy::BatchedLive,
                elapsed_ms: 0,
                capture_windows: 0,
                parameter_writes: 0,
                final_lufs: None,
                error_lu: None,
                final_output_level: None,
                clamped: false,
                saved: false,
                failure: Some("no scene rows harvested".to_string()),
            };
            emit(&row);
            rows.push(row);
            continue;
        }

        let candidates = filter_amp_candidates(discovered);
        if candidates.is_empty() {
            for (scene_slot, scene_name) in &scene_rows {
                let row = SceneLevelBenchmarkRow {
                    preset_slot: slot,
                    ui_label: format!("{:03}", slot.device_slot()),
                    scene_slot: *scene_slot,
                    scene_name: scene_name.clone(),
                    strategy: SceneLevelStrategy::BatchedLive,
                    elapsed_ms: 0,
                    capture_windows: 0,
                    parameter_writes: 0,
                    final_lufs: None,
                    error_lu: None,
                    final_output_level: None,
                    clamped: false,
                    saved: false,
                    failure: Some("no amp outputLevel controls found".to_string()),
                };
                emit(&row);
                rows.push(row);
            }
            continue;
        }

        SCENE_LEVEL_CANCEL.store(false, SeqCst);
        eprintln!(
            "[bench] preset {:03}: batched-live run over {} scene rows…",
            slot.device_slot(),
            scene_rows.len()
        );
        let wire_slots: Vec<u32> = scene_rows.iter().map(|(s, _)| *s).collect();
        // THE field-8 read for this bench run — the routing-structure fallback AND the raw
        // scene overlays `set_knobs` needs for its Scene Edit decision.
        let saved = crate::scene_jobs::read_saved_preset(slot);
        let jobs =
            match build_scene_jobs(&wire_slots, &candidates, &docs, target_lufs, saved.as_ref()) {
                Ok(jobs) => jobs,
                Err(e) => {
                    for (scene_slot, scene_name) in &scene_rows {
                        let row = SceneLevelBenchmarkRow {
                            preset_slot: slot,
                            ui_label: format!("{:03}", slot.device_slot()),
                            scene_slot: *scene_slot,
                            scene_name: scene_name.clone(),
                            strategy: SceneLevelStrategy::BatchedLive,
                            elapsed_ms: 0,
                            capture_windows: 0,
                            parameter_writes: 0,
                            final_lufs: None,
                            error_lu: None,
                            final_output_level: None,
                            clamped: false,
                            saved: false,
                            failure: Some(e.clone()),
                        };
                        emit(&row);
                        rows.push(row);
                    }
                    continue;
                }
            };
        let t0 = std::time::Instant::now();
        let outcome = level_scenes_live_batched(
            slot,
            &jobs,
            &stim,
            save,
            saved.as_ref(),
            |_, _| {},
            || SCENE_LEVEL_CANCEL.load(SeqCst),
        );
        match outcome {
            Ok(outcomes) => {
                for o in outcomes {
                    let name = scene_rows
                        .iter()
                        .find(|(s, _)| *s == o.scene_slot)
                        .map(|(_, n)| n.clone())
                        .unwrap_or_default();
                    match (&o.failure, o.final_lufs) {
                        (None, Some(lufs)) => eprintln!(
                            "[bench]   → scene {} ({name}): {:.2} LUFS (err {:+.2} LU) in {:.1}s clamped={} windows={} writes={}",
                            o.scene_slot,
                            lufs,
                            lufs - target_lufs,
                            o.elapsed_ms as f64 / 1000.0,
                            o.clamped,
                            o.windows,
                            o.writes,
                        ),
                        _ => eprintln!(
                            "[bench]   → scene {} ({name}): FAILED in {:.1}s: {}",
                            o.scene_slot,
                            o.elapsed_ms as f64 / 1000.0,
                            o.failure.as_deref().unwrap_or("?"),
                        ),
                    }
                    let row = SceneLevelBenchmarkRow {
                        preset_slot: slot,
                        ui_label: format!("{:03}", slot.device_slot()),
                        scene_slot: o.scene_slot,
                        scene_name: name,
                        strategy: SceneLevelStrategy::BatchedLive,
                        elapsed_ms: o.elapsed_ms,
                        capture_windows: o.windows,
                        parameter_writes: o.writes,
                        final_lufs: o.final_lufs,
                        error_lu: o.final_lufs.map(|l| l - target_lufs),
                        final_output_level: o.final_level,
                        clamped: o.clamped,
                        saved: save && o.failure.is_none(),
                        failure: o.failure,
                    };
                    emit(&row);
                    rows.push(row);
                }
                eprintln!(
                    "[bench] preset {:03}: batched-live run total {:.1}s",
                    slot.device_slot(),
                    t0.elapsed().as_millis() as f64 / 1000.0
                );
            }
            Err(e) => {
                eprintln!(
                    "[bench] preset {:03}: batched-live run FAILED: {e}",
                    slot.device_slot()
                );
                let row = SceneLevelBenchmarkRow {
                    preset_slot: slot,
                    ui_label: format!("{:03}", slot.device_slot()),
                    scene_slot: session::BASE_SCENE_SLOT,
                    scene_name: "Base".to_string(),
                    strategy: SceneLevelStrategy::BatchedLive,
                    elapsed_ms: t0.elapsed().as_millis(),
                    capture_windows: 0,
                    parameter_writes: 0,
                    final_lufs: None,
                    error_lu: None,
                    final_output_level: None,
                    clamped: false,
                    saved: false,
                    failure: Some(e.to_string()),
                };
                emit(&row);
                rows.push(row);
            }
        }
        session_gap();
    }

    let json = serde_json::to_string_pretty(&rows).map_err(|e| format!("serialize report: {e}"))?;
    std::fs::write(&out_path, json).map_err(|e| format!("write {out_path}: {e}"))?;
    Ok(format!(
        "wrote {} benchmark rows to {out_path} (target {target_lufs:.1} LUFS, topology {topology_id})\n",
        rows.len()
    ))
}

// ───────────────── The retired live-batched runner (bench only) ─────────────────

const LIVE_SETTLE_MS: u64 = 350;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "retired controller, kept for the tests that show why hybrid won"
    )
)]
const LIVE_MAX_ITERS: u32 = 5;

/// Live-controller flavors. Only `LiveHybrid` ships (the batched runner's
/// controller — the benchmark winner); the others remain as
/// `next_live_coord` branches exercised by the unit tests, documenting WHY
/// hybrid won (secant is noise-fragile, fixed-gain proportional stalls on
/// compressed responses, Fractal-style full meter-match jumps overshoot —
/// Fractal itself ships no auto-leveler, just a ~300 ms VU meter the player
/// matches manually, FM9 manual p.62). `BatchedLive` labels the shipped
/// whole-preset runner in benchmark rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum SceneLevelStrategy {
    LiveSecant,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "retired controller, kept for the tests that show why hybrid won"
        )
    )]
    LiveProportional,
    LiveHybrid,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "retired controller, kept for the tests that show why hybrid won"
        )
    )]
    FractalStyle,
    /// One preset load + one stream pair per preset; one re-amp engage per
    /// scene; trust-region slope jumps (`level_scenes_live_batched`).
    BatchedLive,
}

#[derive(Debug, Clone, Serialize)]
struct SceneLevelBenchmarkRow {
    pub preset_slot: ListIndex,
    pub ui_label: String,
    pub scene_slot: u32,
    pub scene_name: String,
    pub strategy: SceneLevelStrategy,
    pub elapsed_ms: u128,
    pub capture_windows: u32,
    pub parameter_writes: u32,
    pub final_lufs: Option<f64>,
    pub error_lu: Option<f64>,
    pub final_output_level: Option<f32>,
    pub clamped: bool,
    pub saved: bool,
    pub failure: Option<String>,
}

fn live_window_lufs(live: &audio::LiveReamp, window_ms: u64) -> Result<f64, LevelError> {
    let cap = live.recent_capture(window_ms)?;
    let lufs = measure_processed(&cap)?.integrated_lufs;
    if lufs.is_finite() {
        Ok(lufs)
    } else {
        Err(LevelError::from("no finite live LUFS measurement"))
    }
}

/// Pure live-controller step: from the current point `(coord, measured)` (c-space
/// knob coordinate → measured LUFS), the PREVIOUS distinct point (`None` on the
/// first step), and the target, return the next c-space coordinate, clamped to
/// `[c_lo, c_hi]`. Pure so each strategy is unit-testable against a fake
/// loudness source (see `tests::simulate_live`).
///
/// - `LiveHybrid`: one-shot predicted jump first (slope ≈ 1 dB per dB of knob in
///   c-space — the validated amplitude model), then secant trims from the two
///   real measured points.
/// - `LiveSecant`: conservative half-error probe first so the secant gets a real
///   local slope estimate, then pure secant.
/// - `LiveProportional`: bounded-gain (0.75) nudges toward the target.
/// - `FractalStyle`: full meter-match jump every step — paired with the SHORT
///   capture window in `live_window_ms()` (Fractal's fast-meter posture).
fn next_live_coord(
    strategy: SceneLevelStrategy,
    iter: u32,
    current: (f32, f64),
    prev: Option<(f32, f64)>,
    target: f64,
    (c_lo, c_hi): (f32, f32),
) -> f32 {
    let (coord, measured) = current;
    let err = (target - measured) as f32;
    let full = coord + err;
    let stepped = match strategy {
        SceneLevelStrategy::LiveHybrid | SceneLevelStrategy::LiveSecant => match prev {
            Some((pa, py)) if iter > 0 => {
                secant_step((pa, py), (coord, measured), target, Flatness::Knob, None)
                    .unwrap_or(full)
            }
            _ if strategy == SceneLevelStrategy::LiveSecant => coord + 0.5 * err,
            _ => full,
        },
        SceneLevelStrategy::LiveProportional => coord + 0.75 * err,
        _ => full, // FractalStyle (and the defensive default): meter-match jump
    };
    stepped.clamp(c_lo, c_hi)
}

/// Per-scene capture window for the BATCHED live runner. Shorter than
/// `LIVE_WINDOW_MS`: the batched run amortizes session + engage ceremony, so
/// the window is the dominant per-trim cost; 2 s of the looped stimulus is the
/// speed/accuracy compromise (final accuracy still gated at `KNOB_TOL_LU`).
const BATCH_WINDOW_MS: u64 = 2000;
const BATCH_MAX_TRIMS: u32 = 4;

/// The single knob for the single-knob paths; errors if `job` is a multi-knob (parallel)
/// job or a skip job, which this probe-only runner can't solve.
fn solo(job: &SceneJob) -> Result<&KnobTarget, LevelError> {
    if let Some(reason) = &job.skip {
        return Err(LevelError::from(reason.clone()));
    }
    match job.knobs.as_slice() {
        [one] => Ok(one),
        n => Err(LevelError::Device(format!(
            "this leveling path supports a single amp knob per scene, got {} \
             (a parallel-merged scene needs the joint-k runner)",
            n.len()
        ))),
    }
}

/// BATCHED live scene leveling — the fast path. The preset loads ONCE and the
/// stimulus/capture streams run ONCE for the whole preset; each scene then gets
/// a lean engage connection: `set_knob` (scene recall + Scene Edit + start
/// value) → engage re-amp → measure on the shared stream → trust-region slope
/// jumps via live `changeParameter` (audible mid-engage, HW-proven) → re-amp
/// OFF → drop. One ENGAGE PER SCENE is mandatory: re-amp latches the ACTIVE
/// SCENE at engage — `loadScene` mid-engage is inaudible (HW: all 9
/// scenes of an 8-scene preset measured the identical audio on one engage).
///
/// `jobs` come from the caller's un-engaged pre-pass (live doc per scene →
/// knob + bounds + that scene's current value). `save` persists once at the
/// end; otherwise the stored preset is reloaded.
fn level_scenes_live_batched(
    slot: ListIndex,
    jobs: &[SceneJob],
    stimulus: &[f32],
    save: bool,
    saved: Option<&serde_json::Value>,
    mut on_scene: impl FnMut(u32, Option<&BatchedSceneOutcome>),
    mut cancelled: impl FnMut() -> bool,
) -> Result<Vec<BatchedSceneOutcome>, LevelError> {
    let result = (|| {
        // Load in its own connection (set-after-load override + engage latch).
        load_preset_isolated(slot, CancelMode::Cleanup)?;

        // ONE pair of CoreAudio streams for the whole preset (between engages
        // they just carry silence). Rebuilding streams per scene both wasted
        // ~0.5 s/scene and churned coreaudiod.
        let live = audio::LiveReamp::start(stimulus, RATE)?;

        let mut outcomes = Vec::with_capacity(jobs.len());
        for job in jobs {
            if cancelled() {
                return Err(LevelError::Cancelled);
            }
            on_scene(job.scene_slot, None);
            let t0 = std::time::Instant::now();
            let mut windows = 0u32;
            let mut writes = 0u32;

            let scene_result = (|windows: &mut u32,
                                 writes: &mut u32|
             -> Result<(f64, f32, bool), LevelError> {
                // This closed-loop runner is single-knob only (probe benchmark path);
                // a parallel-merged scene must use the joint-k `level_scenes_oneshot`.
                let kt = solo(job)?;
                // Fresh engage connection per scene: scene recall + Scene Edit
                // + start value ride `set_knob` BEFORE the engage (latch rule).
                let mut s = Session::connect()?;
                set_knob(&mut s, &kt.knob, kt.current.clamp(kt.lo, kt.hi), saved)?;
                *writes += 1;
                crate::settle(Duration::from_millis(SETTLE_AFTER_SET_MS));
                let _ = s.set_reamp_mode(true)?;
                crate::settle(Duration::from_millis(SETTLE_AFTER_REAMP_MS));

                let (log_space, c_lo, c_hi) = knob_search_space(kt.lo, kt.hi);
                let mut coord =
                    knob_to_coord(kt.current.clamp(kt.lo, kt.hi), log_space).clamp(c_lo, c_hi);
                crate::settle(Duration::from_millis(LIVE_SETTLE_MS + BATCH_WINDOW_MS));
                let mut measured = live_window_lufs(&live, BATCH_WINDOW_MS)?;
                *windows += 1;
                let mut best = (coord, measured);
                let mut prev: Option<(f32, f64)> = None;

                for iter in 0..BATCH_MAX_TRIMS {
                    if cancelled() {
                        return Err(LevelError::Cancelled);
                    }
                    if (best.1 - job.target_lufs).abs() <= KNOB_TOL_LU {
                        break;
                    }
                    let raw_next = next_live_coord(
                        SceneLevelStrategy::LiveHybrid,
                        iter,
                        (coord, measured),
                        prev,
                        job.target_lufs,
                        (c_lo, c_hi),
                    );
                    // Trust region: bound each move (full computed jumps
                    // overshot steep knobs by ~6 LU on HW).
                    let next = (coord + (raw_next - coord).clamp(-BATCH_TRUST_DB, BATCH_TRUST_DB))
                        .clamp(c_lo, c_hi);
                    if (next - coord).abs() < 1e-3 {
                        break;
                    }
                    let next_value = coord_to_knob(next, log_space, kt.lo, kt.hi);
                    set_knob_value_only(&mut s, &kt.knob, next_value)?;
                    *writes += 1;
                    crate::settle(Duration::from_millis(LIVE_SETTLE_MS + BATCH_WINDOW_MS));
                    let lufs = live_window_lufs(&live, BATCH_WINDOW_MS)?;
                    *windows += 1;
                    if (lufs - job.target_lufs).abs() < (best.1 - job.target_lufs).abs() {
                        best = (next, lufs);
                    }
                    prev = Some((coord, measured));
                    coord = next;
                    measured = lufs;
                }

                // Land on the best point if the loop ended elsewhere.
                let best_value = coord_to_knob(best.0, log_space, kt.lo, kt.hi);
                if (best.0 - coord).abs() > 1e-4 {
                    set_knob_value_only(&mut s, &kt.knob, best_value)?;
                    *writes += 1;
                    crate::settle(Duration::from_millis(LIVE_SETTLE_MS + BATCH_WINDOW_MS));
                    best.1 = live_window_lufs(&live, BATCH_WINDOW_MS)?;
                    *windows += 1;
                }
                let _ = s.set_reamp_mode(false);
                Ok((
                    best.1,
                    best_value,
                    (best.1 - job.target_lufs).abs() > KNOB_TOL_LU,
                ))
            })(&mut windows, &mut writes);

            let outcome = match scene_result {
                Ok((lufs, level, clamped)) => BatchedSceneOutcome {
                    scene_slot: job.scene_slot,
                    target_lufs: job.target_lufs,
                    final_lufs: Some(lufs),
                    final_level: Some(level),
                    clamped,
                    windows,
                    writes,
                    elapsed_ms: t0.elapsed().as_millis(),
                    failure: None,
                    dynamic_spread_lu: None, // live windows carry no full-capture meter
                    clamp_reason: None,
                    verify_by_ear: false,
                    persist_mismatch: None,
                    // The probe-only bench runner reports no taxonomy: it has no
                    // `clamp_reason`/wet-floor inputs to derive one from.
                    clamp_kind: None,
                    // Its result IS a live-window measurement of the written value.
                    verification: Verification::Verified,
                },
                Err(e) if e.is_cancelled() => return Err(e),
                Err(e) => BatchedSceneOutcome {
                    scene_slot: job.scene_slot,
                    target_lufs: job.target_lufs,
                    final_lufs: None,
                    final_level: None,
                    clamped: false,
                    windows,
                    writes,
                    elapsed_ms: t0.elapsed().as_millis(),
                    failure: Some(e.to_string()),
                    ..Default::default()
                },
            };
            on_scene(job.scene_slot, Some(&outcome));
            outcomes.push(outcome);
        }

        drop(live);
        if save {
            let mut s = Session::connect()?;
            s.save_current_preset(slot)?;
        } else {
            restore_saved_preset(slot)?;
        }
        Ok(outcomes)
    })();
    // Guaranteed fresh OFF — interrupted live streams can strand re-amp even if
    // the in-session OFF was sent.
    reamp_off_fresh("level_scenes_live_batched");
    restore_after_unsaved_error(slot, save, result)
}

#[cfg(test)]
mod live_controller_tests {
    use super::*;
    use crate::leveller::KNOB_TOL_LU;

    // ---- live-controller (next_live_coord) against a fake loudness source ----

    /// Drive `next_live_coord` against a fake device response `respond(coord) →
    /// LUFS` exactly the way `level_preset_block_live` does (same best-tracking,
    /// same stop conditions). Returns (measurement steps after the seed, best
    /// LUFS, best coord).
    fn simulate_live(
        strategy: SceneLevelStrategy,
        respond: impl Fn(f32) -> f64,
        start_coord: f32,
        target: f64,
        c_lo: f32,
        c_hi: f32,
    ) -> (u32, f64, f32) {
        let mut coord = start_coord.clamp(c_lo, c_hi);
        let mut measured = respond(coord);
        let mut best = (coord, measured);
        let mut prev: Option<(f32, f64)> = None;
        let mut steps = 0u32;
        for iter in 0..LIVE_MAX_ITERS {
            if (best.1 - target).abs() <= KNOB_TOL_LU {
                break;
            }
            let next = next_live_coord(
                strategy,
                iter,
                (coord, measured),
                prev,
                target,
                (c_lo, c_hi),
            );
            if (next - coord).abs() < 1e-3 {
                break;
            }
            let y = respond(next);
            steps += 1;
            if (y - target).abs() < (best.1 - target).abs() {
                best = (next, y);
            }
            prev = Some((coord, measured));
            coord = next;
            measured = y;
        }
        (steps, best.1, best.0)
    }

    /// Ideal amplitude knob (the validated `20·log10` model): LUFS = coord + C.
    /// Hybrid's one-shot jump and FractalStyle's meter-match both land in ONE step.
    #[test]
    fn live_hybrid_and_fractal_converge_in_one_step_on_unit_gain() {
        let plant = |c: f32| c as f64 - 26.0; // C = -26 at coord 0
        for strategy in [
            SceneLevelStrategy::LiveHybrid,
            SceneLevelStrategy::FractalStyle,
        ] {
            let (steps, lufs, _) = simulate_live(strategy, plant, -6.0, -28.0, -60.0, 0.0);
            assert_eq!(steps, 1, "{strategy:?}");
            assert!(
                (lufs - (-28.0)).abs() <= KNOB_TOL_LU,
                "{strategy:?} lufs={lufs}"
            );
        }
    }

    /// Compressive response (0.5 LU per dB of knob — e.g. leveling through a
    /// limiter-ish chain): the secant-based strategies recover the real slope and
    /// converge; pure proportional's bounded gain (0.75·err on a 0.5 slope ⇒
    /// residual ×0.625/step) cannot reach the ±0.3 LU gate within the cap.
    #[test]
    fn live_secant_strategies_beat_proportional_on_compressive_response() {
        let plant = |c: f32| 0.5 * c as f64 - 26.0;
        for strategy in [
            SceneLevelStrategy::LiveHybrid,
            SceneLevelStrategy::LiveSecant,
        ] {
            let (steps, lufs, _) = simulate_live(strategy, plant, 0.0, -22.0, -60.0, 30.0);
            assert!(steps <= 3, "{strategy:?} steps={steps}");
            assert!(
                (lufs - (-22.0)).abs() <= KNOB_TOL_LU,
                "{strategy:?} lufs={lufs}"
            );
        }
        let (_, lufs_prop, _) = simulate_live(
            SceneLevelStrategy::LiveProportional,
            plant,
            0.0,
            -22.0,
            -60.0,
            30.0,
        );
        assert!(
            (lufs_prop - (-22.0)).abs() > KNOB_TOL_LU,
            "proportional unexpectedly converged: {lufs_prop}"
        );
    }

    /// LiveSecant's first move is the conservative half-error probe (NOT the full
    /// jump) — the seed point that distinguishes it from LiveHybrid.
    #[test]
    fn live_secant_first_step_is_half_gain_probe() {
        let next = next_live_coord(
            SceneLevelStrategy::LiveSecant,
            0,
            (-10.0, -28.0),
            None,
            -22.0,
            (-60.0, 0.0),
        );
        assert!((next - (-7.0)).abs() < 1e-4, "next={next}"); // -10 + 0.5·6
        let hybrid = next_live_coord(
            SceneLevelStrategy::LiveHybrid,
            0,
            (-10.0, -28.0),
            None,
            -22.0,
            (-60.0, 0.0),
        );
        assert!((hybrid - (-4.0)).abs() < 1e-4, "hybrid={hybrid}"); // full jump
    }

    /// Unreachable target: every strategy pins at the top bound and stops (the
    /// equal-coord break), leaving the best point at the ceiling — the `clamped`
    /// signal upstream.
    #[test]
    fn live_strategies_clamp_at_unreachable_ceiling() {
        let plant = |c: f32| c as f64 - 26.0; // ceiling at coord 0 → -26 LUFS max
        for strategy in [
            SceneLevelStrategy::LiveHybrid,
            SceneLevelStrategy::LiveSecant,
            SceneLevelStrategy::LiveProportional,
            SceneLevelStrategy::FractalStyle,
        ] {
            let (steps, lufs, coord) = simulate_live(strategy, plant, -6.0, -20.0, -60.0, 0.0);
            assert!(steps <= LIVE_MAX_ITERS, "{strategy:?}");
            assert!((coord - 0.0).abs() < 1e-3, "{strategy:?} coord={coord}");
            assert!((lufs - (-26.0)).abs() < 1e-6, "{strategy:?} lufs={lufs}");
        }
    }
}
