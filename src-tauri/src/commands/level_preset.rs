//! Preset-level (`presetLevel`) leveling command + stimulus resolution + calibration.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// One leveling job from the UI: a preset slot + the LUFS target to hit.
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
/// migration/doctor — the Doctor deliberately keeps the synthetic stimulus until
/// its thresholds are recalibrated against captures). Precedence:
/// `TMP_E2E_STIMULUS` (e2e) → explicit path → selected topology WAV →
/// `TMP_LEVELLER_STIMULUS` env → the default bundled synthetic sample.
pub(crate) fn resolve_stimulus<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    explicit: Option<String>,
    topology_id: Option<String>,
) -> Result<String, String> {
    resolve_stimulus_impl(app, explicit, topology_id, None).map(|(p, _)| p)
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
    let (path, from_capture) = resolve_stimulus_impl(app, explicit, topology_id, profile_id)?;
    Ok((path, if from_capture { None } else { calibration_lufs }))
}

/// Shared precedence chain (ORDER IS LOAD-BEARING): `TMP_E2E_STIMULUS` (e2e) →
/// explicit path → the profile's stored Tier-2 DI capture → selected topology WAV
/// → `TMP_LEVELLER_STIMULUS` env → the default bundled synthetic sample.
fn resolve_stimulus_impl<R: tauri::Runtime>(
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
        let opts = leveller::LevelOptions { save, verify: true, ..Default::default() };
        let cancelled = || PRESET_LEVEL_CANCEL.load(SeqCst);
        let mut previous_level: Option<f32> = None;
        let result = match block {
            Some((group_id, node_id, parameter_id)) => {
                let (lo, hi) = knob_bounds(block_value.unwrap_or(0.5));
                let knob = leveller::LevelKnob::Block { group_id, node_id, parameter_id, scene_slot: None };
                leveller::level_preset_block(slot, &stim, &knob, lo, hi, target_lufs, opts, cancelled)
            }
            None => {
                // Isolate the Base measurement: force EVERY footswitch on/off block OFF so we
                // measure the clean base sound, not "base + whatever pedals are saved on".
                // ponytail: costs one ~1 s preset read per Base run (even presets with no FS
                // blocks). Optimization path: thread an all-on/off force-list hint from the
                // frontend backup scan onto LevelJob (NOT footswitchesPerIndex — that's filtered
                // to levelable-param switches, while isolation needs ALL on-off blocks).
                if cancelled() {
                    return leveller::level_preset(slot, &stim, target_lufs, opts, &[], cancelled);
                }
                // Best-effort: isolation is a quality improvement, not a precondition for
                // leveling at all. A read hiccup (or, offline, a preset-read the fake device
                // doesn't model) must not fail the whole Base run — degrade to no isolation
                // (pre-this-feature behavior) instead of propagating the error.
                let force_bypass: Vec<(String, String, bool)> = match read_slot_preset_parsed(slot)
                {
                    Ok((preset, _, _)) => {
                        // The same read carries the pre-run presetLevel — the revert anchor.
                        previous_level = audiograph::preset_level(&preset).map(|v| v as f32);
                        footswitch::all_onoff_blocks(
                            preset.get("ftsw").unwrap_or(&serde_json::Value::Null),
                        )
                        .into_iter()
                        .map(|(g, n)| (g, n, true))
                        .collect()
                    }
                    Err(e) => {
                        log::warn!(
                            "level_preset slot={slot}: base-isolation preset read failed ({e}), leveling without isolation"
                        );
                        Vec::new()
                    }
                };
                // The isolation read opened (or tried to open) its own session either way —
                // gap before level_preset reconnects, else the quick reopen risks the HID
                // open-lockout (0xe00002c5).
                std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
                leveller::level_preset(slot, &stim, target_lufs, opts, &force_bypass, cancelled)
            }
        };
        // The revert anchor rides the result (Summary "Restore original"). In-memory
        // only: a restart-surviving restore is a follow-up that ships WITH its reader UI.
        let result = result.map(|mut r| {
            r.previous_level = previous_level;
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
}

/// Restore a preset's `presetLevel` to its pre-leveling snapshot value (the
/// Summary "Restore original" action). A device WRITE (set + save), serialized
/// and seize-released like every leveling write. `presetLevel` only — scene and
/// footswitch `outputLevel` writes are not revertable from here (UI copy says so).
#[tauri::command]
pub(crate) async fn restore_preset_level(
    state: State<'_, AppState>,
    slot: u32,
    level: f32,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        log::info!("restore_preset_level slot={slot} level={level:.4}");
        leveller::restore_preset_level(slot, level)
    })
    .await
}
/// What one Tier-2 calibration measured, plus its quality caveats.
/// Mirrored in `src/lib/types.ts` (`CalibrateResult`).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CalibrateResult {
    /// Measured K-weighted loudness of the dry capture (stored on the profile).
    lufs: f32,
    /// The dry tap (USB-Out 3, no limiter) hit 0 dBFS — the measurement is biased
    /// LOW (clipped transients flatten the brightness K-weighting credits).
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

/// A band counts as "covered" (actually played) when its energy is within this
/// many dB of the loudest band in the capture — anything quieter reads as
/// unexcited (never played, or fully masked).
const BAND_COVERAGE_DB: f64 = 30.0;

/// Per-band coverage rule for a calibration capture: `bands` are linear energies
/// (as returned by `spectrum::band_energies`); a band is "covered" when within
/// [`BAND_COVERAGE_DB`] of the loudest band. Pure — no I/O, unit-tested below.
fn coverage(bands: &[f64]) -> Vec<bool> {
    let loudest = bands.iter().copied().fold(0.0f64, f64::max).max(1e-12);
    let loudest_db = 10.0 * loudest.log10();
    bands
        .iter()
        .map(|&b| loudest_db - 10.0 * b.max(1e-12).log10() <= BAND_COVERAGE_DB)
        .collect()
}

#[cfg(test)]
mod coverage_tests {
    use super::coverage;

    #[test]
    fn flat_bands_are_all_covered() {
        assert_eq!(coverage(&[1.0, 1.0, 1.0, 1.0, 1.0, 1.0]), vec![true; 6]);
    }

    #[test]
    fn sparse_take_covers_only_the_played_bands() {
        // 2 loud bands, 4 essentially dead (well below the 30 dB floor).
        let bands = [1.0, 1e-9, 1e-9, 1e-9, 1e-9, 1.0];
        assert_eq!(
            coverage(&bands),
            vec![true, false, false, false, false, true]
        );
    }

    #[test]
    fn thirty_db_boundary() {
        // Loudest = 0 dB (power 1.0). Exactly −30 dB (power 1e-3) still counts;
        // just past it (−30.5 dB) does not.
        let at_boundary = 10f64.powf(-30.0 / 10.0);
        let past_boundary = 10f64.powf(-30.5 / 10.0);
        assert_eq!(coverage(&[1.0, at_boundary]), vec![true, true]);
        assert_eq!(coverage(&[1.0, past_boundary]), vec![true, false]);
    }
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
    with_released_seize(state.session.clone(), move || {
        let (mono, peak) = crate::probe_api::stimulus::capture_dry_di(secs)?;
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
        let clipped = peak >= 0.99;

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
        let band_coverage = coverage(&spectrum::band_energies(&mono, 48_000.0, family.bands()));
        let band_labels: Vec<String> = family.labels().iter().map(|s| s.to_string()).collect();

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
