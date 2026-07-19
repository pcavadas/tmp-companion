//! Per-scene leveling commands + setlist common-target leveling.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// One resolved amp knob: `(group_id, node_id, current_outputLevel)`.
pub(crate) type AmpKnobSpec = (String, String, f32);
/// A candidate leveling knob for `level_scenes_apply` — the frontend passes EVERY
/// amp-level candidate (it owns amp-ness via the models catalog); the backend picks
/// PER SCENE the one whose block is actually ON in that scene.
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LevelBlockArg {
    pub(crate) group_id: String,
    pub(crate) node_id: String,
    pub(crate) parameter_id: String,
    pub(crate) value: f32,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SceneLevelProgressItem {
    scene_slot: u32,
    status: String,
    result: Option<leveller::LevelResult>,
    message: Option<String>,
}

/// One scene-leveling request from the wizard: a wire scene slot + its OWN loudness
/// target. Per-job targets (mirroring `FootswitchLevelJob`) let a preset with a mix of
/// targets level in ONE batch — one prepass, one runner, one deferred save.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SceneLevelJobArg {
    scene_slot: u32,
    target_lufs: f64,
}

/// Wire payload for `tmp://leveling-lufs` — the advisory live measured loudness streamed
/// while a leveling capture runs, so the UI can show a "measuring…" readout. ADVISORY: this
/// is the loudness at the reference level, NOT the final preset level (the result row is the
/// confirm). `momentary` is the current hop's plain RMS in dB (decorative fuel for the live
/// VU bars, not the solve). Mirrored in `src/lib/types.ts`.
#[derive(Clone, serde::Serialize)]
pub(crate) struct LiveLufsEvent {
    lufs: f64,
    momentary: f64,
}

/// RAII guard: installs an advisory live-LUFS sink that emits `tmp://leveling-lufs` for the
/// lifetime of a leveling run, clearing it on drop (incl. unwind). Every leveling command
/// runs serialized under the device-op lock, so only one guard is ever live at a time.
pub(crate) struct LiveLufsGuard;

impl LiveLufsGuard {
    pub(crate) fn install<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Self {
        use tauri::Emitter;
        audio::set_live_lufs_sink(Box::new(move |lufs, momentary| {
            let _ = app.emit("tmp://leveling-lufs", LiveLufsEvent { lufs, momentary });
        }));
        LiveLufsGuard
    }
}

impl Drop for LiveLufsGuard {
    fn drop(&mut self) {
        audio::clear_live_lufs_sink();
    }
}

pub(crate) static SCENE_LEVEL_CANCEL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
pub(crate) fn cancel_scene_leveling() {
    SCENE_LEVEL_CANCEL.store(true, SeqCst);
}

fn pick_scene_level_knob(
    slot: u32,
    scene: u32,
    candidates: &[LevelBlockArg],
) -> Result<(leveller::LevelKnob, f32, f32, f32), String> {
    let scene_slot = if scene >= session::BASE_SCENE_SLOT {
        None
    } else {
        Some(scene)
    };
    // ONE rich session (HW-rearchitected): heartbeat warmup → loads
    // via send_and_collect → live doc from the accumulated field-3 pushes. The
    // old connect → load → drop → connect_for_discovery chain is broken on fw
    // 1.8.45 twice over: a close chased by a re-open wedges the device's next
    // exclusive open (0xe00002c5 lockout), and field-78 kills field-3 delivery
    // for its whole session anyway. After each load the raw accumulator is
    // cleared so the doc reflects the POST-scene live state (the pick must read
    // the sounding graph, never stale pre-scene pushes).
    let live_doc = {
        let mut s = Session::connect()?;
        for _ in 0..16 {
            s.heartbeat()?;
            s.pump_collect(120)?;
        }
        s.raw.clear();
        s.send_and_collect(&proto::load_preset((slot + 1) as u64, 1), 300)?;
        for _ in 0..8 {
            s.heartbeat()?;
            s.pump_collect(200)?;
        }
        if let Some(sl) = scene_slot {
            s.raw.clear();
            s.send_and_collect(&proto::load_scene(sl as u64), 300)?;
            for _ in 0..8 {
                s.heartbeat()?;
                s.pump_collect(200)?;
            }
        }
        s.current_preset_value()?
    };
    for c in candidates {
        log::info!(
            "pick_scene_level_knob scene={scene} candidate {}/{}/{} live_bypass={:?}",
            c.group_id,
            c.node_id,
            c.parameter_id,
            scenes::block_bypass_in_live_graph(&live_doc, &c.group_id, &c.node_id),
        );
    }
    let picked = candidates
        .iter()
        .filter(|c| is_amp_output_level_param(&c.parameter_id))
        .find(|c| {
            scenes::block_bypass_in_live_graph(&live_doc, &c.group_id, &c.node_id) == Some(false)
        })
        .ok_or_else(|| format!("no active amp outputLevel control found for scene slot {scene}"))?;
    let (lo, hi) = knob_bounds(picked.value);
    Ok((
        leveller::LevelKnob::Block {
            group_id: picked.group_id.clone(),
            node_id: picked.node_id.clone(),
            parameter_id: picked.parameter_id.clone(),
            scene_slot,
        },
        lo,
        hi,
        picked.value,
    ))
}

/// Level ONE scene the capture-per-connection way (`level_preset_block`): pick
/// the scene's knob from its live graph, then closed-loop with fresh re-amp
/// captures. The legacy `level_scenes_apply` path; the shipped batched flow is
/// `level_scenes_apply_batched` → `leveller::level_scenes_oneshot` (or
/// `level_scenes_rebalance` for the parallel-amp option) — NOT the retired
/// bench-only `level_scenes_live_batched` (see notes/leveling.md).
fn level_one_scene_legacy(
    slot: u32,
    scene: u32,
    candidates: &[LevelBlockArg],
    stimulus: &[f32],
    target_lufs: f64,
    save: bool,
) -> Result<leveller::LevelResult, String> {
    let (knob, lo, hi, _current) = pick_scene_level_knob(slot, scene, candidates)?;
    // 800 ms before the leveller's first fresh connect — the empirical safe gap
    // after a rich-session close (shorter chases trip the device's open lockout).
    std::thread::sleep(std::time::Duration::from_millis(800));
    let opts = leveller::LevelOptions {
        save,
        verify: true,
        ..Default::default()
    };
    leveller::level_preset_block(slot, stimulus, &knob, lo, hi, target_lufs, opts, || false)
}

/// Per-scene leveling APPLY (chosen mechanism: enable scene mode on the amp
/// block, level only the amp `outputLevel` control). For each selected scene, drive
/// the scene's ACTIVE amp's `outputLevel` knob closed-loop to `target_lufs` with
/// per-block Scene Edit enabled —
/// so the level lands on that scene's overlay, not the base. The knob is resolved
/// PER SCENE from `candidates` by the scene overlay's `bypass` (HW-found:
/// a preset can carry several amps with scenes swapping which is live — leveling a
/// bypassed amp's knob measures flat and clamps).
/// `scene_slots` are the WIRE slots: 0-based `scenes[]` indices for FS scenes;
/// `session::BASE_SCENE_SLOT` (8) = the base/preset value (levelled WITHOUT scene-edit
/// — a preset load activates base, so no scene recall is needed).
/// DEVICE WRITE when `save` — opt-in, gated by the read-only HW policy + the leveling
/// overlay confirm. Reuses `level_preset_block` (the scene context rides the knob and
/// is re-asserted on every connection). Each scene is a self-contained leveling pass.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn level_scenes_apply(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    slot: u32,
    scene_slots: Vec<u32>,
    candidates: Vec<LevelBlockArg>,
    target_lufs: f64,
    save: bool,
    topology_id: Option<String>,
    calibration_lufs: Option<f32>,
) -> Result<Vec<leveller::LevelResult>, String> {
    if !candidates
        .iter()
        .any(|c| is_amp_output_level_param(&c.parameter_id))
    {
        return Err("per-scene leveling needs at least one amp outputLevel candidate".to_string());
    }
    if scene_slots.is_empty() {
        return Err("no scenes selected".to_string());
    }
    let target_lufs = target_lufs + playback_offset_for(&app, topology_id.as_deref());
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;
        let run = || -> Result<Vec<leveller::LevelResult>, String> {
            let mut results = Vec::with_capacity(scene_slots.len());
            for scene in &scene_slots {
                let r = level_one_scene_legacy(
                    slot,
                    *scene,
                    &candidates,
                    &stim,
                    target_lufs,
                    save,
                )?;
                log::info!(
                    "level_scenes_apply slot={slot} scene={scene} save={save} final_level={:.4} measured={:.2} clamped={}",
                    r.final_level, r.measured_lufs, r.clamped,
                );
                results.push(r);
            }
            Ok(results)
        };
        let result = run();
        // Run-end backstop, success or failure (see `reamp_off_guaranteed`: the
        // device drops an in-session OFF sent after ~1 s of idle — every capture).
        leveller::reamp_off_guaranteed("level_scenes_apply");
        result
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn level_scenes_apply_batched<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    slot: u32,
    jobs: Vec<SceneLevelJobArg>,
    candidates: Vec<LevelBlockArg>,
    save: bool,
    rebalance: bool,
    topology_id: Option<String>,
    calibration_lufs: Option<f32>,
    profile_id: Option<String>,
    on_result: tauri::ipc::Channel<SceneLevelProgressItem>,
) -> Result<Vec<leveller::LevelResult>, String> {
    if !candidates
        .iter()
        .any(|c| is_amp_output_level_param(&c.parameter_id))
    {
        return Err("per-scene leveling needs at least one amp outputLevel candidate".to_string());
    }
    if jobs.is_empty() {
        return Err("no scenes selected".to_string());
    }
    SCENE_LEVEL_CANCEL.store(false, SeqCst);
    // Playback compensation is one offset for the whole batch; each job's own target
    // gets it added below (the per-scene targets differ, the offset does not).
    let offset = playback_offset_for(&app, topology_id.as_deref());
    let (stim_path, calibration_lufs) = resolve_stimulus_for_leveling(
        &app,
        None,
        topology_id,
        profile_id.as_deref(),
        calibration_lufs,
    )?;
    let app_evt = app.clone();
    with_released_seize(state.session.clone(), move || {
        // Stream advisory live LUFS while each capture runs (dropped at closure end).
        let _lufs = LiveLufsGuard::install(app_evt);
        let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;
        let scene_slots: Vec<u32> = jobs.iter().map(|j| j.scene_slot).collect();
        let run_batched = |save_run: bool| -> Result<Vec<leveller::BatchedSceneOutcome>, String> {
            // Un-engaged pre-pass (scene docs → jobs), then the ONE-SHOT runner:
            // amp `outputLevel` is linear in dB, so each scene is measured once at a
            // reference level (ISOLATED fresh re-amp capture) and solved exactly — the
            // BatchedLive shared-stream loop mis-measured scenes (HW).
            // `restore_scene` = the preset's original active scene: the batch-end
            // single save recalls it first so the preset persists in the same
            // base/scene/footswitch state it was loaded in.
            // DARK: overlay path validated by `probe --overlay-ab` (76/76 scene-amp pairs,
            // 0 bypass mismatches) but adoption is a gated follow-up — flip to `true` then
            // (see prepass_scene_docs_via's adoption-time TODO). `false` = live prepass today.
            let (docs, restore_scene) = prepass_scene_docs_via(slot, &scene_slots, false)?;
            // Inter-session HID gap: the prepass session has just closed; the one-shot
            // runner opens a fresh one. Reuse the leveller's HW-proven open-after-close
            // gap (was a hard-coded 800, copied from the bench). build_scene_jobs below
            // is pure CPU, so this is the only wait here.
            std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
            // `build_scene_jobs` stamps a base target on every job; override each with its
            // OWN wire job's offset-adjusted target (match by scene slot) so a mixed-target
            // preset levels in this ONE batch. `jobs` is non-empty (guarded above).
            let base_target = jobs[0].target_lufs + offset;
            let mut scene_jobs = build_scene_jobs(&scene_slots, &candidates, &docs, base_target)?;
            // Error on ANY slot mismatch between the built jobs and the wire jobs — a silent
            // default (especially NaN, which `.min(k_cap)` would collapse to the cap and slam
            // the amp) must never reach a solve.
            for sj in scene_jobs.iter_mut() {
                let arg = jobs
                    .iter()
                    .find(|j| j.scene_slot == sj.scene_slot)
                    .ok_or_else(|| {
                        format!("built scene job slot {} has no wire target", sj.scene_slot)
                    })?;
                if !arg.target_lufs.is_finite() {
                    return Err(format!(
                        "scene slot {} has a non-finite target ({})",
                        arg.scene_slot, arg.target_lufs
                    ));
                }
                sj.target_lufs = arg.target_lufs + offset;
            }
            if let Some(j) = jobs
                .iter()
                .find(|j| !scene_jobs.iter().any(|sj| sj.scene_slot == j.scene_slot))
            {
                return Err(format!(
                    "requested scene slot {} produced no scene job",
                    j.scene_slot
                ));
            }
            let on_scene = |scene, done: Option<&leveller::BatchedSceneOutcome>| {
                let _ = on_result.send(scene_progress_item(slot, save_run, scene, done));
            };
            let cancelled = || SCENE_LEVEL_CANCEL.load(SeqCst);
            // `rebalance` (opt-in) equalizes a path-MERGE scene's two lanes before joint-k;
            // non-mergeable scenes fall through to the same joint-k either way.
            if rebalance {
                leveller::level_scenes_rebalance(
                    slot,
                    &scene_jobs,
                    &stim,
                    save_run,
                    restore_scene,
                    on_scene,
                    cancelled,
                )
            } else {
                leveller::level_scenes_oneshot(
                    slot,
                    &scene_jobs,
                    &stim,
                    save_run,
                    restore_scene,
                    on_scene,
                    cancelled,
                )
            }
        };
        // Per-scene leveling drives ONLY the active amp's `outputLevel`. When a scene
        // can't reach target even at the knob's limit it CLAMPS and reports the achieved
        // loudness — we do NOT raise the global `presetLevel` to compensate. Raising it
        // lifts EVERY other scene off-target (presetLevel is the Base's job, settled once
        // before the scene pass), and HW the old boost-and-rerun drove
        // presetLevel to 1.0 and blew preset 001's loud scenes 5–7 LU over target.
        let outcome = run_batched(save);
        let result = match outcome {
            Ok(outcomes) => Ok(outcomes
                .iter()
                .filter(|o| o.failure.is_none())
                .map(|o| outcome_to_level_result(slot, save, o))
                .collect()),
            Err(e) if e == leveller::CANCELLED => {
                let _ = on_result.send(SceneLevelProgressItem {
                    scene_slot: session::BASE_SCENE_SLOT,
                    status: "cancelled".to_string(),
                    result: None,
                    message: Some(e),
                });
                Ok(Vec::new())
            }
            Err(e) => Err(e),
        };
        leveller::reamp_off_guaranteed("level_scenes_apply_batched");
        result
    })
    .await
}

/// Build the streamed progress row for one scene step — `None` = the step just STARTED
/// (spinner), `Some(outcome)` = it finished (a `done` result or an `error` message). Shared
/// by `level_scenes_apply_batched` + `redistribute_headroom` so their per-row wire shape can't
/// drift.
fn scene_progress_item(
    slot: u32,
    save: bool,
    scene: u32,
    done: Option<&leveller::BatchedSceneOutcome>,
) -> SceneLevelProgressItem {
    match done {
        None => SceneLevelProgressItem {
            scene_slot: scene,
            status: "active".to_string(),
            result: None,
            message: None,
        },
        Some(o) => match &o.failure {
            None => SceneLevelProgressItem {
                scene_slot: scene,
                status: "done".to_string(),
                result: Some(outcome_to_level_result(slot, save, o)),
                message: None,
            },
            Some(e) => SceneLevelProgressItem {
                scene_slot: scene,
                status: "error".to_string(),
                result: None,
                message: Some(e.clone()),
            },
        },
    }
}

/// Map a [`leveller::BatchedSceneOutcome`] onto the frontend's `LevelResult`
/// contract (the batched runner's outcome is per-scene; `verify_lufs` carries
/// the final measured window).
fn outcome_to_level_result(
    slot: u32,
    save: bool,
    o: &leveller::BatchedSceneOutcome,
) -> leveller::LevelResult {
    let lufs = o.final_lufs.unwrap_or(f64::NAN);
    leveller::LevelResult {
        slot,
        ref_level: o.final_level.unwrap_or(0.0),
        measured_lufs: lufs,
        constant_c: f64::NAN,
        final_level: o.final_level.unwrap_or(0.0),
        // Per-scene target lives on the outcome (a batch can mix targets).
        target_lufs: o.target_lufs,
        predicted_lufs: lufs,
        clamped: o.clamped,
        saved: save,
        verify_lufs: o.final_lufs,
        iterations: o.windows.max(o.writes),
        dynamic_spread_lu: o.dynamic_spread_lu,
        clamp_reason: o.clamp_reason.clone(),
        verify_by_ear: o.verify_by_ear,
        // Scene rows write amp outputLevel, not presetLevel — nothing to revert here.
        previous_level: None,
        // Scene path: no predicted true peak this cycle (only the one-shot presetLevel
        // path in `level_preset` estimates it).
        true_peak_dbtp: None,
    }
}

// ───────────────────────── Gain-budget redistribution (PR5) ─────────────────────────

/// One touched knob's PRE-redistribution value — the Restore anchor. `scene_slot` `None` =
/// the base amp (plain write); `Some(i)` = the i-th FS scene overlay (scene-edit write).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviousKnob {
    group_id: String,
    node_id: String,
    scene_slot: Option<u32>,
    value: f32,
}

/// Result of a redistribution: the per-sound outcomes + the values it rewrote, recorded for
/// the Summary's one-click Restore (presetLevel + every touched amp `outputLevel`).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RedistributeResult {
    results: Vec<leveller::LevelResult>,
    previous_preset_level: f32,
    previous_knobs: Vec<PreviousKnob>,
    delta_db: f64,
    new_preset_level: f32,
}

/// Give clamped scenes headroom by redistributing the gain budget (loud-preset class,
/// single-amp v1): raise `presetLevel` by `delta` and re-level the base amp + every scene
/// back to target, so clamped scenes gain headroom while non-clamped sounds stay on target.
/// `jobs` are the WHOLE preset's sounds — base (`session::BASE_SCENE_SLOT`) + every FS scene —
/// each with its OWN target. `worst_clamped_deficit_db` (from the run: max `target − achieved`
/// over the clamped scenes) drives `delta` together with the preset's read-back presetLevel
/// headroom and the down-room before the lowest compensated knob hits the silence floor.
/// Opt-in (the Summary action) + reversible (returns the recorded previous values).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn redistribute_headroom<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    slot: u32,
    jobs: Vec<SceneLevelJobArg>,
    candidates: Vec<LevelBlockArg>,
    worst_clamped_deficit_db: f64,
    topology_id: Option<String>,
    calibration_lufs: Option<f32>,
    profile_id: Option<String>,
    on_result: tauri::ipc::Channel<SceneLevelProgressItem>,
) -> Result<RedistributeResult, String> {
    if !candidates
        .iter()
        .any(|c| is_amp_output_level_param(&c.parameter_id))
    {
        return Err("redistribution needs at least one amp outputLevel candidate".to_string());
    }
    if jobs.is_empty() {
        return Err("no sounds to redistribute".to_string());
    }
    if !worst_clamped_deficit_db.is_finite() || worst_clamped_deficit_db <= 0.0 {
        return Err("redistribution needs a positive clamped-scene deficit".to_string());
    }
    SCENE_LEVEL_CANCEL.store(false, SeqCst);
    let offset = playback_offset_for(&app, topology_id.as_deref());
    let (stim_path, calibration_lufs) = resolve_stimulus_for_leveling(
        &app,
        None,
        topology_id,
        profile_id.as_deref(),
        calibration_lufs,
    )?;
    let app_evt = app.clone();
    with_released_seize(state.session.clone(), move || {
        let _lufs = LiveLufsGuard::install(app_evt);
        let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;
        let scene_slots: Vec<u32> = jobs.iter().map(|j| j.scene_slot).collect();

        // Prepass: ONE rich session loads the preset + harvests each sound's live doc (the
        // pre-raise presetLevel + per-sound current outputLevel). No re-amp yet.
        let (docs, restore_scene) = prepass_scene_docs_via(slot, &scene_slots, false)?;
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
        let base_target = jobs[0].target_lufs + offset;
        let mut scene_jobs = build_scene_jobs(&scene_slots, &candidates, &docs, base_target)?;
        // Stamp each job with its OWN (offset-adjusted) target.
        for sj in scene_jobs.iter_mut() {
            let arg = jobs
                .iter()
                .find(|j| j.scene_slot == sj.scene_slot)
                .ok_or_else(|| format!("built job slot {} has no wire target", sj.scene_slot))?;
            if !arg.target_lufs.is_finite() {
                return Err(format!("scene slot {} has a non-finite target", arg.scene_slot));
            }
            sj.target_lufs = arg.target_lufs + offset;
        }
        // Reverse-check (mirrors `level_scenes_apply_batched`): a requested sound that produced
        // NO job is a silent drop — fail loudly rather than redistribute a partial sound set.
        if let Some(j) = jobs
            .iter()
            .find(|j| !scene_jobs.iter().any(|sj| sj.scene_slot == j.scene_slot))
        {
            return Err(format!(
                "requested sound slot {} produced no redistribution job",
                j.scene_slot
            ));
        }

        // Read the pre-raise presetLevel from the prepass docs (any sound's audioGraph).
        let preset_level = docs
            .iter()
            .find_map(|(_, d)| d.as_ref().and_then(audiograph::preset_level))
            .ok_or_else(|| "could not read the preset's current presetLevel".to_string())?
            as f32;
        // Record the previous values (pl + every touched knob) BEFORE any write — the Restore
        // anchor. `current` on each job knob is the sound's pre-raise outputLevel.
        let previous_knobs: Vec<PreviousKnob> = scene_jobs
            .iter()
            .flat_map(|sj| {
                sj.knobs.iter().filter_map(|kt| match &kt.knob {
                    leveller::LevelKnob::Block {
                        group_id,
                        node_id,
                        scene_slot,
                        ..
                    } => Some(PreviousKnob {
                        group_id: group_id.clone(),
                        node_id: node_id.clone(),
                        scene_slot: *scene_slot,
                        value: kt.current,
                    }),
                    leveller::LevelKnob::PresetLevel => None,
                })
            })
            .collect();
        // delta = min(worst clamped deficit, presetLevel headroom, down-room before the
        // lowest compensated knob hits the floor).
        let min_knob = scene_jobs
            .iter()
            .flat_map(|sj| sj.knobs.iter().map(|kt| kt.current))
            .fold(f32::INFINITY, f32::min);
        let delta_db = leveller::redistribute_delta_db(preset_level, worst_clamped_deficit_db, min_knob);
        if delta_db <= 1e-3 {
            return Err(
                "no headroom to redistribute (presetLevel already near max, or a knob at the floor) \
                 — try re-leveling to a lower common target instead"
                    .to_string(),
            );
        }
        let new_preset_level = (f64::from(preset_level) * 10f64.powf(delta_db / 20.0)).min(1.0) as f32;

        let on_scene = |scene, done: Option<&leveller::BatchedSceneOutcome>| {
            let _ = on_result.send(scene_progress_item(slot, true, scene, done));
        };
        let cancelled = || SCENE_LEVEL_CANCEL.load(SeqCst);
        let outcome = leveller::redistribute_clamped_headroom(
            slot,
            new_preset_level,
            &scene_jobs,
            &stim,
            restore_scene,
            on_scene,
            cancelled,
        );
        let result = match outcome {
            Ok(outcomes) => Ok(RedistributeResult {
                results: outcomes
                    .iter()
                    .filter(|o| o.failure.is_none())
                    .map(|o| outcome_to_level_result(slot, true, o))
                    .collect(),
                previous_preset_level: preset_level,
                previous_knobs,
                delta_db,
                new_preset_level,
            }),
            Err(e) if e == leveller::CANCELLED => {
                let _ = on_result.send(SceneLevelProgressItem {
                    scene_slot: session::BASE_SCENE_SLOT,
                    status: "cancelled".to_string(),
                    result: None,
                    message: Some(e.clone()),
                });
                Err(e)
            }
            Err(e) => Err(e),
        };
        leveller::reamp_off_guaranteed("redistribute_headroom");
        result
    })
    .await
}

/// One-click Restore for a redistribution: write the recorded pre-redistribution values
/// (presetLevel + every touched amp `outputLevel`) back and save — the reverse of the atomic
/// write, on ONE session (base recall before save). Name-guarded (the run recorded the slot's
/// display name); a drifted list fails loudly rather than restoring onto a different preset.
#[tauri::command]
pub(crate) async fn restore_redistribution(
    state: State<'_, AppState>,
    slot: u32,
    preset_level: f32,
    knobs: Vec<PreviousKnob>,
    expected_name: String,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        let writes: Vec<leveller::PrevKnobWrite> = knobs
            .iter()
            .map(|k| leveller::PrevKnobWrite {
                group_id: k.group_id.clone(),
                node_id: k.node_id.clone(),
                scene_slot: k.scene_slot,
                value: k.value,
            })
            .collect();
        let r = leveller::restore_redistribution(slot, preset_level, &writes, &expected_name);
        leveller::reamp_off_guaranteed("restore_redistribution");
        r
    })
    .await
}

/// Headroom (LU) below the quietest-capable preset's ceiling when auto-picking
/// the setlist common target. Small margin so the floor preset isn't clamped.
const SETLIST_HEADROOM_LU: f64 = 1.0;

/// One preset in a setlist leveling job: its slot + the instrument profile's
/// topology (resolved to that instrument's stimulus).
#[derive(serde::Deserialize)]
pub(crate) struct SetlistJobEntry {
    slot: u32,
    topology_id: Option<String>,
    calibration_lufs: Option<f32>,
}

/// Level a whole setlist to one common loudness target so switching presets (and
/// instruments) on stage causes no jump. Measures every preset's ceiling, picks a
/// target just below the quietest, and applies it to all. Like `level_preset`, it
/// releases the app's seize, runs, then re-establishes the UI session.
#[tauri::command]
pub(crate) async fn level_setlist(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    entries: Vec<SetlistJobEntry>,
    save: bool,
) -> Result<leveller::SetlistResult, String> {
    if entries.is_empty() {
        return Err("no presets selected to level".to_string());
    }
    // Resolve each entry's stimulus path + playback compensation on the UI
    // thread (needs AppHandle; the store is read ONCE for the whole setlist).
    // The common target stays one loudness; a bass entry's offset rides its own
    // effective target inside the leveller.
    let playback = profiles::load(&app)
        .map(|s| s.playback_level)
        .unwrap_or_default();
    let resolved: Vec<(u32, String, Option<f32>, f64)> = entries
        .into_iter()
        .map(|e| {
            let offset_lu = profiles::playback_offset_lu(
                playback,
                stimulus_instrument(e.topology_id.as_deref()),
            );
            resolve_stimulus(&app, None, e.topology_id)
                .map(|p| (e.slot, p, e.calibration_lufs, offset_lu))
        })
        .collect::<Result<_, _>>()?;
    with_released_seize(state.session.clone(), move || {
        // Own each stimulus (calibrated if the profile has a real-output level),
        // then borrow into entries for the leveller.
        let stims: Vec<(u32, Vec<f32>, f64)> = resolved
            .into_iter()
            .map(|(slot, path, cal, off)| {
                read_stimulus_calibrated(&path, cal).map(|s| (slot, s, off))
            })
            .collect::<Result<_, _>>()?;
        let lvl_entries: Vec<leveller::SetlistEntry> = stims
            .iter()
            .map(|(slot, s, off)| leveller::SetlistEntry {
                slot: *slot,
                stimulus: s,
                offset_lu: *off,
            })
            .collect();
        let result = leveller::level_setlist(&lvl_entries, SETLIST_HEADROOM_LU, 0.5, save);
        leveller::reamp_off_guaranteed("level_setlist");
        result
    })
    .await
}
/// Measure each scene's ceiling loudness (re-amp + `loadScene` per scene)
/// and return the per-scene gain offsets to a common target (MEASURE — drives the
/// device; HW-pending). Supersedes hand-entered C values when hardware is present.
#[tauri::command]
pub(crate) async fn level_scenes(
    app: tauri::AppHandle,
    slot: u32,
    scene_count: u32,
    topology_id: Option<String>,
    headroom_lu: f64,
    state: State<'_, AppState>,
) -> Result<Vec<f64>, String> {
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        let cs = leveller::capture_scene_ceilings(slot, scene_count, &stim)?;
        scenes::normalize_scene_targets(&cs, headroom_lu)
            .ok_or_else(|| "no finite scene loudness measured".to_string())
    })
    .await
}
