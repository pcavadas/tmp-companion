//! Probe entry point: batched-live per-scene leveling benchmark harness.

use super::level::filter_amp_candidates;
use super::scene_jobs::build_scene_jobs;
use super::stimulus::probe_stimulus_path;
use super::stimulus::read_stimulus_calibrated;
use crate::leveller;
use crate::proto;
use crate::session;
use crate::session::Session;
use crate::SCENE_LEVEL_CANCEL;
use std::sync::atomic::Ordering::SeqCst;

pub fn probe_bench_scene_leveling(
    slots: Vec<u32>,
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
    let mut rows = Vec::<leveller::SceneLevelBenchmarkRow>::new();
    // Stream each row to disk as it lands (an OOM reboot lost a full 40-minute
    // run that only wrote at the end).
    let jsonl_path = format!("{out_path}.jsonl");
    let emit = |row: &leveller::SceneLevelBenchmarkRow| {
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
            slot + 1
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
                s.pump_collect(120)?;
            }
            s.raw.clear();
            s.send_and_collect(&proto::load_preset((slot + 1) as u64, 1), 300)?;
            let mut scenes: Option<Vec<String>> = None;
            let mut seen = 0usize;
            // Keep pumping past the 125 hit — the multi-packet field-3 push
            // (block discovery) needs the extra turns to finish arriving.
            for _ in 0..10 {
                s.heartbeat()?;
                s.pump_collect(200)?;
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
                    let _ = s.pump_collect(200);
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
                        s.pump_collect(150)?;
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
                eprintln!("[bench] preset {:03}: intel session failed: {e}", slot + 1);
                let row = leveller::SceneLevelBenchmarkRow {
                    preset_slot: slot,
                    ui_label: format!("{:03}", slot + 1),
                    scene_slot: session::BASE_SCENE_SLOT,
                    scene_name: "Base".to_string(),
                    strategy: leveller::SceneLevelStrategy::BatchedLive,
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
                slot + 1
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
            let row = leveller::SceneLevelBenchmarkRow {
                preset_slot: slot,
                ui_label: format!("{:03}", slot + 1),
                scene_slot: session::BASE_SCENE_SLOT,
                scene_name: "Base".to_string(),
                strategy: leveller::SceneLevelStrategy::BatchedLive,
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
                let row = leveller::SceneLevelBenchmarkRow {
                    preset_slot: slot,
                    ui_label: format!("{:03}", slot + 1),
                    scene_slot: *scene_slot,
                    scene_name: scene_name.clone(),
                    strategy: leveller::SceneLevelStrategy::BatchedLive,
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
            slot + 1,
            scene_rows.len()
        );
        let wire_slots: Vec<u32> = scene_rows.iter().map(|(s, _)| *s).collect();
        let jobs = match build_scene_jobs(&wire_slots, &candidates, &docs, target_lufs, None) {
            Ok(jobs) => jobs,
            Err(e) => {
                for (scene_slot, scene_name) in &scene_rows {
                    let row = leveller::SceneLevelBenchmarkRow {
                        preset_slot: slot,
                        ui_label: format!("{:03}", slot + 1),
                        scene_slot: *scene_slot,
                        scene_name: scene_name.clone(),
                        strategy: leveller::SceneLevelStrategy::BatchedLive,
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
        let outcome = leveller::level_scenes_live_batched(
            slot,
            &jobs,
            &stim,
            save,
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
                    let row = leveller::SceneLevelBenchmarkRow {
                        preset_slot: slot,
                        ui_label: format!("{:03}", slot + 1),
                        scene_slot: o.scene_slot,
                        scene_name: name,
                        strategy: leveller::SceneLevelStrategy::BatchedLive,
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
                    slot + 1,
                    t0.elapsed().as_millis() as f64 / 1000.0
                );
            }
            Err(e) => {
                eprintln!(
                    "[bench] preset {:03}: batched-live run FAILED: {e}",
                    slot + 1
                );
                let row = leveller::SceneLevelBenchmarkRow {
                    preset_slot: slot,
                    ui_label: format!("{:03}", slot + 1),
                    scene_slot: session::BASE_SCENE_SLOT,
                    scene_name: "Base".to_string(),
                    strategy: leveller::SceneLevelStrategy::BatchedLive,
                    elapsed_ms: t0.elapsed().as_millis(),
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

/// Closed-loop search bounds for a block knob, from its current value: amplitude
/// params (0..1) search the full [0,1]; dB-unit params (current outside [0,1],
/// e.g. an IR `outputlevel`) search a ±window around the current value, capped at
/// 0 dB on top.
pub(crate) fn knob_bounds(current: f32) -> (f32, f32) {
    if (0.0..=1.0).contains(&current) {
        (0.0, 1.0)
    } else {
        (current - 18.0, (current + 6.0).min(0.0))
    }
}
