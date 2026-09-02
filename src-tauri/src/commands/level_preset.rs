//! Preset-level (`presetLevel`) leveling command + stimulus resolution + calibration.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// One leveling job from the UI: a preset slot + the LUFS target to hit.
///
/// PER-ROW HANDLE AND TARGET (the base row's half of the contract). `target_lufs` is THIS
/// row's target — the user's per-row override when they set one, else the global; the wizard
/// resolves that before dispatch, so nothing here needs a "global vs override" flag.
/// `block_*` is the row's optional user-chosen HANDLE: present = level by driving that block
/// control, absent = the **`presetLevel` pseudo-handle**, which is the base row's default and
/// the only handle no block can express (it is the preset's master amplitude, not a node
/// param). Scene rows carry the same pair on `SceneLevelJobArg`, footswitch rows on
/// `FootswitchLevelJob` — three row kinds, one shape.
#[derive(serde::Deserialize)]
pub(crate) struct LevelJob {
    slot: u32,
    target_lufs: f64,
    /// Persist the computed `presetLevel` to the preset (SaveCurrentPreset).
    save: bool,
    /// Selected instrument's pickup topology id → its bundled stimulus WAV.
    topology_id: Option<String>,
    /// Tier-2 calibration: the profile's measured real output (K-weighted LUFS).
    /// When set, the stimulus is scaled to this loudness before injection.
    calibration_lufs: Option<f32>,
    /// Optional explicit stimulus override (takes precedence over `topology_id`).
    stimulus_path: Option<String>,
    /// Instrument profile id: when it has a stored Tier-2 DI capture, that WAV is
    /// the stimulus (injected verbatim), overriding the synthetic topology sample.
    #[serde(default)]
    profile_id: Option<String>,
    /// Block-knob leveling: when all three are set, level by driving this block
    /// control (ChangeParameter, closed loop) instead of the master `presetLevel`.
    /// Coordinates come from `list_level_blocks`.
    block_group_id: Option<String>,
    block_node_id: Option<String>,
    block_parameter_id: Option<String>,
    /// The block param's current value (from `list_level_blocks`) — used to pick
    /// closed-loop search bounds (amplitude 0..1 vs dB-unit) without re-enumerating.
    block_value: Option<f32>,
}

/// Enumerate a preset's level-type block controls so the UI can offer them as
/// leveling knobs. Loads `slot` then reconnects (discovery handshake) to read its
/// `audioGraph` — runs with the app's seize released, like the leveling commands.
#[tauri::command]
pub(crate) async fn list_level_blocks(
    state: State<'_, AppState>,
    slot: u32,
) -> Result<Vec<session::LevelBlock>, String> {
    let blocks = with_released_seize(state.session.clone(), move || {
        load_then_discover_blocks(slot)
    })
    .await?;
    log::info!(
        "list_level_blocks slot={slot}: {} block(s): {}",
        blocks.len(),
        blocks
            .iter()
            .map(|b| format!("[{}]{}={:.3}", b.model_id, b.parameter_id, b.value))
            .collect::<Vec<_>>()
            .join(" ")
    );
    Ok(blocks)
}
/// Resolve the stimulus WAV for the profile-UNAWARE callers (audition/spectrum/
/// migration). Precedence:
/// `TMP_E2E_STIMULUS` (e2e) → explicit path → selected topology WAV →
/// `TMP_LEVELLER_STIMULUS` env → the default bundled synthetic sample.
pub(crate) fn resolve_stimulus<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    explicit: Option<String>,
    topology_id: Option<String>,
) -> Result<String, String> {
    resolve_stimulus_with_capture(app, explicit, topology_id, None).map(|(p, _)| p)
}

/// The leveling variant: also consults the profile's stored Tier-2 DI capture and
/// returns the EFFECTIVE calibration scalar — `None` when the capture won, so a
/// real DI is injected VERBATIM (never re-scaled). Enforcing the no-scaling rule
/// inside this seam means a future leveling caller cannot forget it.
pub(crate) fn resolve_stimulus_for_leveling<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    explicit: Option<String>,
    topology_id: Option<String>,
    profile_id: Option<&str>,
    calibration_lufs: Option<f32>,
) -> Result<(String, Option<f32>), String> {
    let (path, from_capture) =
        resolve_stimulus_with_capture(app, explicit, topology_id, profile_id)?;
    Ok((path, if from_capture { None } else { calibration_lufs }))
}

/// Shared precedence chain (ORDER IS LOAD-BEARING): `TMP_E2E_STIMULUS` (e2e) →
/// explicit path → the profile's stored Tier-2 DI capture → selected topology WAV
/// → `TMP_LEVELLER_STIMULUS` env → the default bundled synthetic sample. The bool
/// reports whether the profile's Tier-2 DI capture won (`true`) — the Doctor uses
/// it to pick its threshold table (a real DI shifts the measured band balance
/// systematically).
pub(crate) fn resolve_stimulus_with_capture<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    explicit: Option<String>,
    topology_id: Option<String>,
    profile_id: Option<&str>,
) -> Result<(String, bool), String> {
    // Offline e2e: a fixed repo stimulus WAV (MockRuntime can't resolve bundle resources).
    #[cfg(feature = "e2e")]
    if let Ok(p) = std::env::var("TMP_E2E_STIMULUS") {
        if !p.is_empty() {
            return Ok((p, false));
        }
    }
    if let Some(p) = explicit.filter(|p| !p.is_empty()) {
        return Ok((p, false));
    }
    if let Some(id) = profile_id.filter(|s| !s.is_empty()) {
        if let Some(p) = profiles::existing_capture_for(app, id) {
            log::info!(
                "resolve_stimulus: profile {id} → captured DI stimulus {}",
                p.display()
            );
            return Ok((p.to_string_lossy().into_owned(), true));
        }
        log::info!("resolve_stimulus: profile {id} has no captured DI → synthetic fallback");
    }
    if let Some(tid) = topology_id.filter(|t| !t.is_empty()) {
        return topology_wav_path(app, &tid).map(|p| (p, false));
    }
    if let Ok(p) = std::env::var("TMP_LEVELLER_STIMULUS") {
        if !p.is_empty() {
            return Ok((p, false));
        }
    }
    topology_wav_path(app, topologies::DEFAULT_TOPOLOGY_ID).map(|p| (p, false))
}
/// Fletcher–Munson playback compensation for a leveling job: the LU offset added
/// to the target, from the store's playback level × the stimulus topology's
/// instrument family. Equal-LUFS is equal-loudness only at the SPL the K-weighting
/// curve approximates (~stage volume); at quieter playback the equal-loudness
/// contours steepen and a bass preset matched at equal LUFS sits perceptibly
/// quieter, so its target is raised (see `profiles::playback_offset_lu`). `None` /
/// unknown topology falls back to the guitar default (offset 0); `Stage` (the
/// store default) is always 0, so legacy stores level exactly as before.
pub(crate) fn playback_offset_for<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    topology_id: Option<&str>,
) -> f64 {
    let level = profiles::load(app)
        .map(|s| s.playback_level)
        .unwrap_or_default();
    profiles::playback_offset_lu(level, stimulus_instrument(topology_id))
}

/// The instrument family a leveling job's stimulus belongs to (`None` / unknown
/// topology = the guitar default).
pub(crate) fn stimulus_instrument(topology_id: Option<&str>) -> &'static str {
    topologies::by_id(topology_id.unwrap_or(topologies::DEFAULT_TOPOLOGY_ID))
        .map(|t| t.instrument)
        .unwrap_or("guitar")
}

/// The Phase-2 boost candidate: the base amp's `outputLevel` knob job, classified by the SAME
/// amp classifier a scene batch run uses (`build_scene_jobs_with_handles`), scoped to base
/// (`session::BASE_SCENE_SLOT`). `None` on ANY refusal — an unusual routing (parallel/split/
/// mic), no amp-`outputLevel` candidate, a classifier error, or an amp `force` already
/// force-bypasses (boosting an amp that's forced OFF for this isolated capture is nonsensical)
/// — degrades to today's plain path, never a hard error: boost is an OPPORTUNISTIC extra, not
/// a leveling prerequisite.
///
/// Stamps `force_bypass = force` onto the returned job itself (`build_scene_jobs_with_handles`
/// always emits `force_bypass: Vec::new()` — isolation is the CALLER's to stamp, see
/// `level_scenes_apply_batched`). Owning the stamp HERE, in one named helper, is the fix:
/// without it `jointk_one_scene` → `apply_levels` → `set_knobs`'s base recall (`load_scene`)
/// would revert the forced-bypass footswitches to their SAVED state mid-solve, so every capture
/// in the boost's measure/verify/secant loop would read base-with-pedals-on instead of the
/// isolated base the plan was computed against — silently wrong by whatever those pedals add,
/// with `verify_lufs` still reading on-target.
fn base_boost_candidate(
    preset: &serde_json::Value,
    target_lufs: f64,
    force: &[(String, String, bool)],
) -> Option<leveller::SceneJob> {
    let candidates = filter_amp_candidates(session::extract_level_blocks(preset));
    build_scene_jobs_with_handles(
        &[session::BASE_SCENE_SLOT],
        &candidates,
        &[(session::BASE_SCENE_SLOT, Some(preset.clone()))],
        target_lufs,
        Some(preset),
        &[],
    )
    .ok()
    .and_then(|jobs| jobs.into_iter().next())
    .filter(|job| job.skip.is_none() && job.knobs.len() == 1)
    .filter(|job| match &job.knobs[0].knob {
        leveller::LevelKnob::Block {
            group_id, node_id, ..
        } => !force.iter().any(|(g, n, _)| g == group_id && n == node_id),
        leveller::LevelKnob::PresetLevel => true,
    })
    .map(|mut job| {
        job.force_bypass = force.to_vec();
        job
    })
}

#[cfg(test)]
mod base_boost_candidate_tests {
    use super::*;

    fn series_amp_preset() -> serde_json::Value {
        serde_json::json!({
            "audioGraph": { "template": "gtrSeries", "guitarNodes": { "G1": [
                {
                    "nodeId": "amp",
                    "FenderId": "ACD_TwinReverb65NoFx",
                    "dspUnitParameters": { "bypass": false, "outputLevel": 0.42 }
                }
            ] } }
        })
    }

    // A classifiable single amp with no isolation in the way: the candidate carries the amp's
    // knob and the isolation list is stamped onto it whole, even when empty.
    #[test]
    fn a_single_amp_candidate_is_returned_with_force_bypass_stamped() {
        let preset = series_amp_preset();
        let job = base_boost_candidate(&preset, -23.0, &[]).expect("one amp candidate");
        assert_eq!(job.knobs.len(), 1);
        assert!(job.force_bypass.is_empty());
        let leveller::LevelKnob::Block {
            group_id, node_id, ..
        } = &job.knobs[0].knob
        else {
            panic!("expected a Block knob");
        };
        assert_eq!((group_id.as_str(), node_id.as_str()), ("G1", "amp"));

        let force = [("G1".to_string(), "other".to_string(), true)];
        let job = base_boost_candidate(&preset, -23.0, &force).expect("still classifiable");
        assert_eq!(job.force_bypass, force, "the isolation list rides whole");
    }

    // An amp the isolation list itself force-bypasses can't be boosted (it's forced OFF for
    // this isolated capture) — the candidate must refuse rather than boost a silenced amp.
    #[test]
    fn an_amp_already_force_bypassed_yields_no_candidate() {
        let preset = series_amp_preset();
        let force = [("G1".to_string(), "amp".to_string(), true)];
        assert!(base_boost_candidate(&preset, -23.0, &force).is_none());
    }

    // No outputLevel candidate at all (e.g. an empty graph) degrades to `None`, never an error.
    #[test]
    fn no_amp_candidates_yields_no_error_just_none() {
        let preset = serde_json::json!({ "audioGraph": { "guitarNodes": {} } });
        assert!(base_boost_candidate(&preset, -23.0, &[]).is_none());
    }
}

/// Level one preset to its target (the real, one-shot open-loop path). The
/// leveller opens its own fresh connections (load → measure → set), so the work
/// runs with the app's seize released (see `with_released_seize`).
#[tauri::command]
pub(crate) async fn level_preset<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    job: LevelJob,
) -> Result<leveller::LevelResult, String> {
    let LevelJob {
        slot,
        target_lufs,
        save,
        topology_id,
        calibration_lufs,
        stimulus_path,
        profile_id,
        block_group_id,
        block_node_id,
        block_parameter_id,
        block_value,
    } = job;
    let offset_lu = playback_offset_for(&app, topology_id.as_deref());
    if offset_lu != 0.0 {
        log::info!("level_preset slot={slot}: playback compensation {offset_lu:+.1} LU on target {target_lufs:.1}");
    }
    let target_lufs = target_lufs + offset_lu;
    let (stim_path, calibration_lufs) = resolve_stimulus_for_leveling(
        &app,
        stimulus_path,
        topology_id,
        profile_id.as_deref(),
        calibration_lufs,
    )?;
    // A block knob is selected only when all three coordinates are present;
    // otherwise level the master `presetLevel` (the validated one-shot path).
    let block = match (block_group_id, block_node_id, block_parameter_id) {
        (Some(g), Some(n), Some(p)) if !g.is_empty() && !n.is_empty() && !p.is_empty() => {
            Some((g, n, p))
        }
        _ => None,
    };
    // Reset the cooperative cancel flag for this run; `cancel_preset_leveling` sets it
    // (it only flips the atomic — no device lock — so it runs while this op holds it).
    PRESET_LEVEL_CANCEL.store(false, SeqCst);
    let app_evt = app.clone();
    with_released_seize(state.session.clone(), move || {
        // Stream advisory live LUFS while each capture runs (dropped at closure end).
        let _lufs = LiveLufsGuard::install(app_evt);
        let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;
        let mut opts = leveller::LevelOptions { save, verify: true, ..Default::default() };
        let cancelled = || PRESET_LEVEL_CANCEL.load(SeqCst);
        let mut previous_level: Option<f32> = None;
        let result = match block {
            Some((group_id, node_id, parameter_id)) => {
                // CLASS GATE + RANGE, replacing the value-sniffed `knob_bounds(current)`:
                // the picker can offer ANY block param, and sweeping a non-level control
                // (a distortion knob, say) changes the sound the player wrote rather than
                // its loudness — the exact hazard the footswitch lane already refuses.
                // `param_class::classify` keys on the block's FenderId, so this arm now
                // ALWAYS pays the one field-8 read the saving path already paid; the same
                // read still supplies `restore_scene`. An unreadable preset REFUSES rather
                // than guessing a class — the safe direction, and no offline spec exercises
                // this arm (every fixture job sends a null block triple).
                let saved = crate::read_saved_preset(slot);
                let Some(preset) = saved.as_ref() else {
                    return Err(format!(
                        "could not read preset {} to classify {parameter_id} on {node_id} — \
                         leveling an unclassified parameter could change the sound instead of \
                         its loudness",
                        slot + 1
                    ));
                };
                // `block_value` — the value the picker displayed — stays the wet-floor
                // anchor, falling back to the saved graph. Everything else (FenderId
                // resolution, classification, the shared refusal) is
                // `FsParamTarget::classified`, so this lane and the scene HANDLE lane
                // cannot answer the same control two ways.
                let authored = block_value
                    .or_else(|| node_param_f64(preset, &node_id, &parameter_id).map(|v| v as f32))
                    .unwrap_or(0.0);
                let target =
                    leveller::FsParamTarget::classified(preset, &node_id, &parameter_id, authored)?;
                let (lo, hi) = target.bounds();
                // A saving block-knob run measures/applies in base context too, so its save
                // must re-stamp the original `lastLoadedScene` like the whole-preset arm.
                if save {
                    opts.restore_scene = crate::last_loaded_scene(preset);
                    crate::warn_missing_restore_scene(
                        "level_preset(block)",
                        slot,
                        preset,
                        opts.restore_scene,
                    );
                }
                let knob = leveller::LevelKnob::Block { group_id, node_id, parameter_id, scene_slot: None };
                // Pre-dispatch cancel: nothing has touched the device yet — early-return
                // (the leveller bails at its own pre-measure checkpoint) so the run-end
                // backstop below is skipped, mirroring the None arm's cancel path.
                if cancelled() {
                    return leveller::level_preset_block(slot, &stim, &knob, lo, hi, target_lufs, opts, cancelled);
                }
                leveller::level_preset_block(slot, &stim, &knob, lo, hi, target_lufs, opts, cancelled)
            }
            None => {
                // Isolate the Base measurement: force EVERY footswitch on/off block OFF so we
                // measure the clean base sound, not "base + whatever pedals are saved on".
                // ponytail: costs one ~1 s preset read per Base run (even presets with no FS
                // blocks) — and, on a preset whose field-8 tail is CUT, a whole device backup
                // (`slot_read`'s complete-or-fail re-read, 60 s cap) before the run starts.
                // Optimization path: thread an all-on/off force-list hint from the frontend
                // backup scan onto LevelJob (NOT footswitchesPerIndex — that's filtered to
                // levelable-param switches, while isolation needs ALL on-off blocks). That hint
                // would remove BOTH costs at once: the startup backup scan is complete by
                // construction, so it needs no truncation fallback at all.
                if cancelled() {
                    // `previous_level` is still None here (the isolation read below hasn't
                    // run yet) — fine, since `level_preset` bails at the pre-measure cancel
                    // checkpoint and returns Err, never reaching the field that would use it.
                    return leveller::level_preset(
                        slot,
                        &stim,
                        target_lufs,
                        opts,
                        &[],
                        previous_level,
                        cancelled,
                    );
                }
                // BASE MEANS BASE — every footswitch-owned on-off block is forced OFF, so the
                // measurement describes the preset with nothing switched on. A preset saved
                // with a pedal engaged does NOT measure that pedal here; that sound is its own
                // footswitch row's job (user directive, 2026-08-20; the HW evidence behind it
                // is in `notes/leveling.md`).
                //
                // `ftsw` sits at the field-8 truncation cliff, so a short read yields a short
                // force list — pedals left on, and the wrong `presetLevel` SAVED. The shared
                // read REFUSES on a truncated `ftsw` rather than leveling a guess. Refusing
                // here returns BEFORE the run-end `reamp_off_guaranteed` backstop, safe only
                // because every step above is a pure read: nothing has engaged re-amp yet.
                let (preset, has_fs_scenes, force, restore_scene) =
                    crate::commands::doctor::read_base_isolation(slot)?;
                // The original `lastLoadedScene` must be re-stamped by the save: the
                // base-context measurement leaves base active, and saving there would
                // rewrite the preset's on-load scene to base (HW, Hiwatt slot 31).
                previous_level = audiograph::preset_level(&preset).map(|v| v as f32);
                opts.restore_scene = restore_scene;
                crate::warn_missing_restore_scene("level_preset", slot, &preset, opts.restore_scene);
                log::info!(
                    "level_preset slot={slot}: base isolation forces {} footswitch-owned \
                     block(s) off",
                    force.len()
                );
                // ⟦BOOST⟧ Derive the base amp candidate for the plan-then-apply routing.
                let base_amp = base_boost_candidate(&preset, target_lufs, &force);
                log::info!(
                    "level_preset slot={slot}: base boost candidate {:?}",
                    base_amp.as_ref().map(|j| j.knobs[0].knob.label())
                );
                leveller::level_preset_impl(
                    slot,
                    &stim,
                    target_lufs,
                    opts,
                    &force,
                    previous_level,
                    leveller::BoostContext {
                        base_amp,
                        saved: Some(&preset),
                        has_fs_scenes,
                    },
                    cancelled,
                )
            }
        };
        // `previous_level` rides the result for the re-run idempotency skip (see
        // `level_unchanged`) — not a user-facing revert; design 1a has no in-app restore
        // of any kind. Only staple when this run actually SAVED (and the leveller left it
        // unset) — the leveller returns `previous_level: None` itself both for its own
        // idempotency skip AND the no-signal clamp, and neither has anything to staple.
        let result = result.map(|mut r| {
            if r.saved && r.previous_level.is_none() {
                r.previous_level = previous_level;
            }
            r
        });
        match &result {
            Ok(r) => log::info!(
                "level_preset slot={} save={} measured={:.2} LUFS target={:.2} LUFS final_level={:.4} verify={:?}",
                r.slot,
                r.saved,
                r.measured_lufs,
                r.target_lufs,
                r.final_level,
                r.verify_lufs,
            ),
            Err(e) => log::warn!("level_preset slot={slot} save={save} failed: {e}"),
        }
        // Run-end backstop, success or failure (see `reamp_off_guaranteed`: the
        // device drops an in-session OFF sent after ~1 s of idle — every capture).
        leveller::reamp_off_guaranteed("level_preset");
        result
    })
    .await
}
/// Cooperative cancel for [`level_preset`] (base-preset leveling) — set by
/// `cancel_preset_leveling`, reset at the command's start, read via a closure passed into
/// `leveller::level_preset`/`level_preset_block`, which bail before the apply+save.
static PRESET_LEVEL_CANCEL: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub(crate) fn cancel_preset_leveling() {
    PRESET_LEVEL_CANCEL.store(true, SeqCst);
    // Also wake the in-flight capture/settle waits (see `device_gate::OP_ABORT`).
    crate::request_op_abort();
}

/// What one Tier-2 calibration measured, plus its quality caveats.
/// Mirrored in `src/lib/types.ts` (`CalibrateResult`).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CalibrateResult {
    /// Measured K-weighted loudness of the dry capture (stored on the profile).
    lufs: f32,
    /// The dry tap (USB-Out 3, no limiter) FLAT-TOPPED at full scale (a sustained
    /// run, not a lone pick transient — see `is_clipped_capture`) — the measurement
    /// is biased LOW (clipped transients flatten the brightness K-weighting credits).
    clipped: bool,
    /// The topology stimulus cannot be scaled up to `lufs` without clipping (the
    /// 0.99 peak cap in `read_stimulus_calibrated_with_shortfall`): leveling will
    /// drive the amp this many LU softer than the real instrument. `None` = reachable
    /// OR a capture was stored (the capture IS the stimulus, so a shortfall can't arise).
    stimulus_shortfall_lu: Option<f32>,
    /// Short-term-max − integrated (LU) of the dry capture — how dynamic the take
    /// was (a wide spread means quiet passages the gated integrated metric discards).
    spread_lu: f64,
    /// Per-band excitation of the capture (same family band layout as the Doctor
    /// engine, `doctor::Family::bands`): `true` when the band was actually played.
    band_coverage: Vec<bool>,
    /// Player-facing labels for `band_coverage`, in lockstep index-for-index.
    band_labels: Vec<String>,
}

/// Fraction of 500 ms windows whose RMS is within 30 dB of the loudest window's —
/// a coarse "did the player actually keep playing?" gate for a calibration capture.
/// ponytail: crude broadband-energy heuristic — a sustained hum or one held note
/// reads as fully active. Upgrade to per-window spectral/hum discrimination only if
/// false accepts show up in the field.
fn active_window_fraction(samples: &[f32], sample_rate: u32) -> f64 {
    let win = (sample_rate as usize / 2).max(1); // 500 ms
    let rms: Vec<f64> = samples
        .chunks(win)
        .map(|w| {
            let sum: f64 = w.iter().map(|&x| (x as f64) * (x as f64)).sum();
            (sum / w.len() as f64).sqrt()
        })
        .collect();
    let loudest = rms.iter().copied().fold(0.0f64, f64::max);
    if rms.is_empty() || loudest <= 0.0 {
        return 0.0;
    }
    let thresh = loudest * 10f64.powf(-30.0 / 20.0); // within 30 dB of the loudest
    rms.iter().filter(|&&r| r >= thresh).count() as f64 / rms.len() as f64
}

#[cfg(test)]
mod activity_tests {
    use super::active_window_fraction;

    const SR: u32 = 48_000;

    #[test]
    fn mostly_silent_capture_fails_the_gate() {
        // 8 s buffer, only the first 1.5 s carries a tone; the rest is silence.
        let mut buf = vec![0.0f32; (SR as usize) * 8];
        for (i, s) in buf.iter_mut().take((SR as usize) * 3 / 2).enumerate() {
            *s = (i as f32 * 0.05).sin() * 0.4;
        }
        assert!(active_window_fraction(&buf, SR) < 0.5);
    }

    #[test]
    fn continuous_pluck_train_passes_the_gate() {
        // A pluck every 300 ms (gap ≤ 0.5 s) across 8 s: every 500 ms window has
        // pluck energy, so the active fraction is high.
        let mut buf = vec![0.0f32; (SR as usize) * 8];
        let step = (SR as usize) * 3 / 10; // 300 ms
        let mut start = 0;
        while start < buf.len() {
            for k in 0..(SR as usize / 4) {
                // 250 ms decaying pluck
                if start + k >= buf.len() {
                    break;
                }
                let env = (-(k as f32) / (SR as f32 * 0.08)).exp();
                buf[start + k] += (k as f32 * 0.06).sin() * 0.5 * env;
            }
            start += step;
        }
        assert!(active_window_fraction(&buf, SR) >= 0.5);
    }
}

/// Tier-2 calibration: capture the dry instrument (USB-Out 3) for `secs` while
/// the user plays their real guitar, measure its K-weighted loudness (LUFS), store
/// it on the profile's `calibration_lufs`, and return the measured value plus the
/// clip/stimulus-ceiling caveats. The device must be in normal mode with the
/// guitar in the front INSTRUMENT input.
#[tauri::command]
pub(crate) async fn calibrate_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    secs: f32,
) -> Result<CalibrateResult, String> {
    let app2 = app.clone();
    let settings_path = crate::commands::presets::device_settings_path(&app);
    with_released_seize(state.session.clone(), move || {
        // #124 pre-flight: the device mixer's USB 3 strip, from the settings snapshot
        // the startup backup read persisted (`support/device-settings.json`). The
        // snapshot can be STALE — the mixer may have been touched since connecting —
        // and the two halves treat that risk DIFFERENTLY on purpose:
        //
        // - MUTE only ever EXPLAINS a take that produced nothing. It is handed to
        //   `capture_dry_di`, which consults it solely on its silent-take path, so a
        //   take that lands despite a "muted" snapshot is simply a newer mixer state
        //   and wins. It is deliberately NOT prepended to arbitrary capture errors —
        //   doing so turned a mid-capture unplug into a confident "USB 3 is MUTED".
        // - The POST/off-unity FADER does veto a take that landed, because a landed
        //   take persists the capture as the leveling stimulus (injected verbatim at
        //   gain 1), so a fader-scaled one corrupts every later re-amp invisibly.
        //   `usb3_fader_fault` carries the full reasoning and the replug recovery.
        let strip = crate::backup_read::read_settings_snapshot(settings_path.as_deref())
            .and_then(|json| crate::backup_read::usb3_strip(&json));
        let (mono, _peak) = crate::probe_api::stimulus::capture_dry_di(secs, strip.as_ref())?;
        if let Some(f) = strip
            .as_ref()
            .and_then(crate::probe_api::stimulus::usb3_fader_fault)
        {
            return Err(f);
        }
        // Reject a capture that's mostly silence (a valid capture becomes the stimulus,
        // so a few plucks + long gaps would inject a mostly-dead re-amp signal).
        if active_window_fraction(&mono, 48_000) < 0.5 {
            return Err(
                "play continuously during calibration — too much silence in the capture"
                    .to_string(),
            );
        }
        // K-weighted loudness (perceptual), not flat RMS — see read_stimulus_calibrated.
        let loudness = lufs::measure_mono(&mono, 48_000)?;
        if !loudness.integrated_lufs.is_finite() {
            return Err("captured signal too quiet to measure — play louder/longer".to_string());
        }
        let lufs = loudness.integrated_lufs as f32;
        let spread_lu = loudness.spread_lu();
        // Flat-top clip check, not a bare sample-peak: a lone hot pick transient is
        // not clipping (see is_clipped_capture) — the old `peak >= 0.99` gate
        // rejected good takes whose only "clip" was a sub-ms attack apex.
        let clipped = crate::probe_api::stimulus::is_clipped_capture(&mono);

        // Store the capture (or clear a stale one on a clipped run) BEFORE persisting the
        // scalar — a WAV write failure fails the whole command so the scalar never lands
        // paired with a torn/absent capture.
        let capture_stored = profiles::store_capture(
            &profiles::app_config_dir(&app2)?,
            &profile_id,
            &mono,
            clipped,
        )?;

        let mut store = profiles::load(&app2)?;
        let p = store
            .profiles
            .iter_mut()
            .find(|p| p.id == profile_id)
            .ok_or_else(|| format!("unknown profile '{profile_id}'"))?;
        p.calibration_lufs = Some(lufs);
        let topology_id = p.topology_id.clone();
        profiles::save(&app2, &store)?;

        // Per-band excitation of the capture, in the profile's family band layout
        // (same bands the Doctor engine diagnoses with) — surfaces whether the
        // player actually covered the instrument's range, not just "played enough".
        let family = doctor::Family::from_topology(
            topologies::by_id(&topology_id)
                .map(|t| t.instrument)
                .unwrap_or("guitar"),
        );
        let band_coverage = doctor::band_coverage(&mono, family);
        let band_labels = family.labels_owned();

        // With a stored capture the stimulus IS the capture (gain 1) — a synthetic
        // shortfall is impossible, so skip the computation (the old warning would be
        // false). Otherwise report the best-effort synthetic-scaling shortfall.
        let stimulus_shortfall_lu = if capture_stored {
            None
        } else {
            resolve_stimulus(&app2, None, Some(topology_id))
                .and_then(|path| read_stimulus_calibrated_with_shortfall(&path, Some(lufs)))
                .map(|(_, shortfall)| shortfall)
                .unwrap_or(None)
        };
        Ok(CalibrateResult {
            lufs,
            clipped,
            stimulus_shortfall_lu,
            spread_lu,
            band_coverage,
            band_labels,
        })
    })
    .await
}
