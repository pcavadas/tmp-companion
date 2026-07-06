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
/// Resolve the stimulus WAV: explicit path → selected topology → `TMP_LEVELLER_STIMULUS`
/// env → the default bundled synthetic sample.
pub(crate) fn resolve_stimulus<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    explicit: Option<String>,
    topology_id: Option<String>,
) -> Result<String, String> {
    // Offline e2e: a fixed repo stimulus WAV (MockRuntime can't resolve bundle resources).
    #[cfg(feature = "e2e")]
    if let Ok(p) = std::env::var("TMP_E2E_STIMULUS") {
        if !p.is_empty() {
            return Ok(p);
        }
    }
    if let Some(p) = explicit.filter(|p| !p.is_empty()) {
        return Ok(p);
    }
    if let Some(tid) = topology_id.filter(|t| !t.is_empty()) {
        return topology_wav_path(app, &tid);
    }
    if let Ok(p) = std::env::var("TMP_LEVELLER_STIMULUS") {
        if !p.is_empty() {
            return Ok(p);
        }
    }
    topology_wav_path(app, topologies::DEFAULT_TOPOLOGY_ID)
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
    let stim_path = resolve_stimulus(&app, stimulus_path, topology_id)?;
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
/// What one Tier-2 calibration measured, plus its two quality caveats.
/// Mirrored in `src/lib/types.ts` (`CalibrateResult`).
#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct CalibrateResult {
    /// Measured K-weighted loudness of the dry capture (stored on the profile).
    lufs: f32,
    /// The dry tap (USB-Out 3, no limiter) hit 0 dBFS — the measurement is biased
    /// LOW (clipped transients flatten the brightness K-weighting credits).
    clipped: bool,
    /// The topology stimulus cannot be scaled up to `lufs` without clipping (the
    /// 0.99 peak cap in `read_stimulus_calibrated_with_shortfall`): leveling will
    /// drive the amp this many LU softer than the real instrument. `None` = reachable.
    stimulus_shortfall_lu: Option<f32>,
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
        // Force normal mode so the dry instrument flows on USB-Out 3.
        if let Ok(mut s) = Session::connect() {
            let _ = s.set_reamp_mode(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        let cap = audio::capture_input(secs.clamp(2.0, 30.0), 48_000)?;

        let peak = cap.channel_peak(audio::DRY_INSTRUMENT_IN_CH);
        if peak < 1e-4 {
            return Err("no instrument signal captured — play continuously during \
                        calibration (guitar in the front INSTRUMENT input, volume up)"
                .to_string());
        }
        // K-weighted loudness (perceptual), not flat RMS — see read_stimulus_calibrated.
        let lufs =
            lufs::measure_mono(&cap.channel(audio::DRY_INSTRUMENT_IN_CH), 48_000)?.integrated_lufs;
        if !lufs.is_finite() {
            return Err("captured signal too quiet to measure — play louder/longer".to_string());
        }
        let lufs = lufs as f32;

        let mut store = profiles::load(&app2)?;
        let p = store
            .profiles
            .iter_mut()
            .find(|p| p.id == profile_id)
            .ok_or_else(|| format!("unknown profile '{profile_id}'"))?;
        p.calibration_lufs = Some(lufs);
        let topology_id = p.topology_id.clone();
        profiles::save(&app2, &store)?;

        // Best-effort caveats (calibration is already persisted; a WAV-resolution
        // failure must not fail the command).
        let stimulus_shortfall_lu = resolve_stimulus(&app2, None, Some(topology_id))
            .and_then(|path| read_stimulus_calibrated_with_shortfall(&path, Some(lufs)))
            .map(|(_, shortfall)| shortfall)
            .unwrap_or(None);
        Ok(CalibrateResult {
            lufs,
            clipped: peak >= 0.99,
            stimulus_shortfall_lu,
        })
    })
    .await
}
