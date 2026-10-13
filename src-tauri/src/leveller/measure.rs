//! Measurement: the floor-read guard, `C` measurement ([`measure_c`]), the capture seams every
//! lane shares (`capture_full_at*`, [`capture_on_session`], the strict as-is measure), the
//! processed-loudness verdicts, and the engage → measure → disengage connection shapes.

use super::*;

/// Fresh-connect, recall BASE, set `level` (NO preset load — the current preset is already
/// the one we want), engage re-amp once, capture, and return the processed pair's loudness.
/// The one-shot `presetLevel` case of `measure_knob_at` (which owns the base recall — see
/// `arm_measurement`: without it the capture measures the preset's saved `lastLoadedScene`).
pub(super) fn measure_at_level(
    stimulus: &[f32],
    level: f32,
    force_bypass: &[(String, String, bool)],
) -> Result<lufs::Loudness, LevelError> {
    // No intended-level assert: `level` IS the `presetLevel` being measured here.
    measure_knob_at(
        stimulus,
        &LevelKnob::PresetLevel,
        level,
        force_bypass,
        None,
        None,
    )
}

// ───────────────────────── Floor-read guard ─────────────────────────
//
// A silent/failed re-amp inject captures the device's STATIONARY OUTPUT FLOOR, which
// is finite LUFS and solves into a plausible-looking level (HW: 19/20 floor reads in
// one sweep; a full solve+verify landed 0.00 LU error on pure floor). The tell is the
// MOMENTARY-loudness spread (P95 − P10 of the 400 ms momentary loudness): the plucked
// stimulus (10 notes 0.6 s apart) swings ~6 LU at that window on every shipped WAV and
// still ~0.7–1.4 LU after 40 dB of hard saturation, while a stationary floor reads
// ≈ 0.1. The 3 s short-term window this guard used to read averages ~5 plucks, so the
// shipped stimuli measured only 0.26–0.32 LU there and the guard disarmed itself on 7 of
// 8 of them (and on all 8 Doctor slices, whose 3 s stimulus IS one short-term window).
// The guard is still stimulus-aware: a stimulus with no momentary dynamics of its own
// (a near-stationary DI capture) can't discriminate, so it DISARMS the trip.
//
// There is no level-shift rescue: the floor is post-DSP, so `presetLevel` scales it by
// the same 20·log10 a real signal follows (`notes/gotchas.md`'s floor-read entry), and a
// "did the capture track the level change" confirm passes a floor read as real.

/// Trip gate: a capture whose momentary spread is at or below this is suspected of being
/// a floor read. A stationary floor reads ≈ 0.1 LU; the shipped stimuli through 40 dB of
/// hard saturation (a harsher compressor than any library preset) still read ≥ 0.67 LU.
pub(crate) const FLOOR_TRIP_LU: f64 = 0.4;
/// A stimulus whose own momentary spread is at or below this can't discriminate a floor
/// from a live capture — skip the trip. The shipped stimuli read 5.9–6.2 LU (Doctor
/// slice 5.8–6.5 LU).
pub(crate) const STATIONARY_STIM_LU: f64 = 2.0;
/// Quiet gap before the guard's retry — 5 s recovered 9/9 flagged rows on HW
/// (`probe --stim-ab`).
pub(crate) const FLOOR_RETRY_GAP_MS: u64 = 5_000;

/// Should this capture be suspected as a floor read? Both arguments are MOMENTARY
/// spreads (`lufs::Loudness::momentary_spread_lu`).
pub(crate) fn floor_suspect(capture_momentary_lu: f64, stimulus_momentary_lu: f64) -> bool {
    stimulus_momentary_lu > STATIONARY_STIM_LU && capture_momentary_lu <= FLOOR_TRIP_LU
}

/// A floor-guarded measurement's outcome. `StillFlat` carries the retry's loudness for
/// the caller's log; every caller turns it into [`LevelError::FloorRead`].
pub(crate) enum GuardOutcome {
    Live(lufs::Loudness),
    StillFlat(lufs::Loudness),
}

/// Run `measure`; if the capture looks like a floor read ([`floor_suspect`]: no
/// momentary dynamics at all), wait `gap` and retry ONCE with the same settings (heals a
/// transient inject failure). A capture still flat after the retry is reported as
/// [`GuardOutcome::StillFlat`], not swallowed.
pub(crate) fn measure_floor_guarded(
    mut measure: impl FnMut() -> Result<lufs::Loudness, LevelError>,
    stimulus_momentary_lu: f64,
    gap: Duration,
) -> Result<GuardOutcome, LevelError> {
    let first = measure()?;
    if !floor_suspect(first.momentary_spread_lu, stimulus_momentary_lu) {
        return Ok(GuardOutcome::Live(first));
    }
    log::warn!(
        "floor guard: capture momentary spread {:.2} LU ≤ {FLOOR_TRIP_LU} — suspected silent \
         inject, retrying once",
        first.momentary_spread_lu
    );
    settle_or_cancel(gap.as_millis() as u64)?;
    let second = measure()?;
    if floor_suspect(second.momentary_spread_lu, stimulus_momentary_lu) {
        return Ok(GuardOutcome::StillFlat(second));
    }
    Ok(GuardOutcome::Live(second))
}

/// The call-site shape every guarded measurement uses: guard `measure`, collapse a
/// persistent flat read to the honest [`LevelError::FloorRead`].
pub(crate) fn require_live(
    measure: impl FnMut() -> Result<lufs::Loudness, LevelError>,
    stimulus: &[f32],
) -> Result<lufs::Loudness, LevelError> {
    match measure_floor_guarded(
        measure,
        stimulus_momentary_spread(stimulus),
        Duration::from_millis(FLOOR_RETRY_GAP_MS),
    )? {
        GuardOutcome::Live(l) => Ok(l),
        GuardOutcome::StillFlat(l) => {
            log::warn!(
                "floor guard: still flat after the retry (momentary spread {:.2} LU at {:.2} \
                 LUFS) — reporting a floor read",
                l.momentary_spread_lu,
                l.integrated_lufs
            );
            Err(LevelError::FloorRead)
        }
    }
}

/// The stimulus's own momentary spread (arms the floor guard). A measurement failure
/// DISARMS the guard (0.0) — never turn a metering hiccup into false floor errors; floor
/// reads then pass exactly as they did before the guard existed.
pub(crate) fn stimulus_momentary_spread(stimulus: &[f32]) -> f64 {
    match lufs::measure_mono(stimulus, RATE) {
        Ok(l) => l.momentary_spread_lu,
        Err(e) => {
            log::warn!("floor guard disarmed: stimulus spread unmeasurable ({e})");
            0.0
        }
    }
}

/// One value `recall_reassert_save` re-writes between the pre-save scene recall and the save
/// itself — see that function's doc for why.
#[derive(Debug, Clone)]
pub(crate) enum Reassert {
    PresetLevel(f32),
    Param {
        group: String,
        node: String,
        param: String,
        value: f32,
    },
}

impl Reassert {
    /// The common single-value shape — an owned `presetLevel` re-assert.
    pub(super) fn preset_level_only(pl: Option<f32>) -> Vec<Reassert> {
        pl.map(Reassert::PresetLevel).into_iter().collect()
    }
}

/// Scan `ftsw` for a `param` function targeting `(node, param)` on ANY switch and return its
/// `valueA` — the Assign write shape, which lives in the footswitch table, never in
/// `dspUnitParameters`. No switch index needed: a leveled block param is targeted by at most
/// one footswitch function in practice, so this walks every switch through the footswitch
/// module's own accessor.
pub(super) fn ftsw_value_a(ftsw: &serde_json::Value, node: &str, param: &str) -> Option<f64> {
    (0..ftsw.as_array()?.len() as u32)
        .find_map(|sw| crate::footswitch::existing_param_fn_value_a(ftsw, sw, node, param))
}

/// What one reference capture yields: the loudness reading, the solved model
/// constant, and the capture's dynamics spread (see `LevelResult::dynamic_spread_lu`).
#[derive(Debug, Clone, Copy)]
pub struct MeasuredC {
    /// The `presetLevel` the reading was taken at — the requested reference, or half of
    /// it when the reference capture clipped ([`measure_c`]'s clip rescue).
    pub ref_level: f32,
    /// Captured integrated LUFS at `ref_level`.
    pub measured_lufs: f64,
    /// Solved `C` in `LUFS = 20·log10(level) + C` (= max reachable LUFS).
    pub c: f64,
    /// Short-term-max − integrated of the same capture (LU).
    pub dynamic_spread_lu: f64,
    /// True peak (dBTP) of the reference capture — the basis for the one-shot
    /// path's PREDICTED true peak at the solved level (see `predicted_true_peak_dbtp`).
    pub true_peak_dbtp: f64,
    /// Reference captures the solve used: 1, or 2–3 with the cross-check.
    pub captures: u32,
}

/// Two reference captures at different `presetLevel`s must solve the same `C` within this
/// (LU) or a third capture decides. ~2.5σ of the ~0.12 LU run-to-run capture noise, far
/// under the ~3 LU outliers on record (`notes/leveling.md`).
pub(crate) const C_AGREE_TOL_LU: f64 = 0.3;

/// The cross-check capture's level: −3 dB from the reference (×0.7), or +3 dB when that
/// would fall under the 0.05 clamp. Different enough to double as a level-shift sanity
/// check, close enough that the C-model's ~0.2 LU linearity error stays inside the band.
pub(crate) fn cross_check_level(ref_level: f32) -> f32 {
    if ref_level * 0.7 >= 0.05 {
        ref_level * 0.7
    } else {
        (ref_level * 1.4).min(1.0)
    }
}

/// Median of three (the cross-check's tiebreak): the one reading two others disagree
/// about can't move it.
pub(super) fn median3(a: f64, b: f64, c: f64) -> f64 {
    a.max(b).min(a.min(b).max(c))
}

/// Conn 1+2 seam: load `slot` (own connection, since set-after-load is overridden
/// in-connection), then measure its captured loudness at `ref_level` on a fresh
/// connection, and solve `C` in `LUFS = 20·log10(level) + C`. `C` is the preset's
/// max reachable captured loudness — of its BASE sound: every capture here recalls base
/// first (`arm_measurement`), since a load activates the preset's saved `lastLoadedScene`
/// and an unrecalled capture would solve `C` for that scene instead.
pub fn measure_c(
    slot: ListIndex,
    stimulus: &[f32],
    ref_level: f32,
    force_bypass: &[(String, String, bool)],
) -> Result<MeasuredC, LevelError> {
    measure_c_checked(slot, stimulus, ref_level, force_bypass, false)
}

/// [`measure_c`] with an optional CROSS-CHECK, for a lane whose result nothing re-measures
/// afterwards (the base row of a footswitch preset, where the verify is impossible — see
/// `level_preset_impl`): a second capture at [`cross_check_level`] must solve the same `C`
/// within [`C_AGREE_TOL_LU`], otherwise a third capture at the reference takes the median.
/// A single silently-wrong capture (the 2.9 LU-low outliers on record) can then no longer
/// set the saved level on its own.
///
/// CLIP RESCUE: a reference capture that clipped ([`LevelError::Clipped`]) is re-taken
/// ONCE at half the reference level, on its own fresh connection like every capture here
/// (one engage per connection, the level set before the engage). Still clipped there → the
/// error stands.
pub(crate) fn measure_c_checked(
    slot: ListIndex,
    stimulus: &[f32],
    ref_level: f32,
    force_bypass: &[(String, String, bool)],
    cross_check: bool,
) -> Result<MeasuredC, LevelError> {
    let mut ref_level = ref_level.clamp(0.05, 1.0);
    load_preset_isolated(slot, CancelMode::Abortable)?;
    // No load → the set inside measure_at_level sticks on the now-current preset.
    let at =
        |level: f32| require_live(|| measure_at_level(stimulus, level, force_bypass), stimulus);
    let loudness = match at(ref_level) {
        Err(LevelError::Clipped) if ref_level / 2.0 >= 0.05 => {
            ref_level /= 2.0;
            log::warn!(
                "measure_c slot={slot}: the reference capture clipped — re-measuring at \
                 presetLevel {ref_level:.3}"
            );
            at(ref_level)?
        }
        other => other?,
    };
    let c_of = |l: &lufs::Loudness, level: f32| l.integrated_lufs - 20.0 * (level as f64).log10();
    let c1 = c_of(&loudness, ref_level);
    let (c, captures) = if cross_check {
        settle_or_cancel(SETTLE_AFTER_SET_MS)?;
        let level2 = cross_check_level(ref_level);
        let c2 = c_of(&at(level2)?, level2);
        if (c1 - c2).abs() <= C_AGREE_TOL_LU {
            ((c1 + c2) / 2.0, 2)
        } else {
            log::warn!(
                "measure_c slot={slot}: reference captures disagree on C ({c1:.2} vs {c2:.2}) \
                 — taking a third"
            );
            settle_or_cancel(SETTLE_AFTER_SET_MS)?;
            let c3 = c_of(&at(ref_level)?, ref_level);
            (median3(c1, c2, c3), 3)
        }
    } else {
        (c1, 1)
    };
    Ok(MeasuredC {
        ref_level,
        measured_lufs: loudness.integrated_lufs,
        c,
        dynamic_spread_lu: loudness.spread_lu(),
        true_peak_dbtp: loudness.true_peak_dbtp,
        captures,
    })
}

/// PREDICTED true peak (dBTP) at `final_level`, extrapolated from the reference
/// capture's measured true peak: `presetLevel` is a linear post-chain amplitude
/// control (see the module doc), so true peak moves by the same 20·log10(ratio) as
/// the solved loudness. An ESTIMATE, never a re-measurement — used only by the
/// one-shot `presetLevel` path (`level_preset`).
pub(crate) fn predicted_true_peak_dbtp(ref_tp_dbtp: f64, ref_level: f32, final_level: f32) -> f64 {
    ref_tp_dbtp + 20.0 * (final_level.max(1e-6) as f64 / ref_level.max(1e-6) as f64).log10()
}

/// Shared MEASURE seam behind `capture_full` and `doctor_capture`: load `slot` in
/// its own connection, settle, drop; fresh-connect → recall the scene ON THE
/// CAPTURE CONNECTION → set the reference level → engage re-amp once →
/// `audio::reamp_capture(.., tail_ms)` → guaranteed re-amp off.
///
/// The scene MUST be loaded on the capture connection, not the load connection:
/// the preset survives the load→capture reconnect but **the active scene does
/// not** (see `set_knob`'s "scene + scene-edit don't survive the leveller's
/// reconnects" — HW). Loading it only in the dropped load connection measured
/// whatever scene the unit was already on, so every scene read the same signal.
///
/// `scene`/`skip_load` interact: when this call just freshly loaded the preset
/// (`!skip_load`), nothing unsaved is at risk, so `scene: None` means "recall
/// BASE explicitly" — omitting the recall used to silently measure whatever
/// scene the connection defaulted to (the preset's saved `lastLoadedScene`), not
/// base. When `skip_load` is set, the caller is asserting the device ALREADY
/// holds exactly the state to measure (Doctor's consecutive-scene chain, or a
/// caller preserving unsaved edits made on an earlier connection — see
/// `doctor_capture_current`'s doc), so `scene: None` there means "don't touch
/// scene state at all" — a recall would risk REVERTING those unsaved edits
/// before this capture ever ran (the same hazard `capture_on_session` exists to
/// avoid for `doctor_apply`'s step (c)). `tail_ms` is `CAPTURE_TAIL_MS` for
/// every existing `capture_full` call, so that path is otherwise unchanged by
/// this extraction.
pub(super) fn capture_full_at(
    slot: ListIndex,
    scene: Option<u32>,
    force_bypass: &[(String, String, bool)],
    stimulus: &[f32],
    ref_level: Option<f32>,
    tail_ms: u64,
    skip_load: bool,
) -> Result<audio::Capture, LevelError> {
    capture_full_at_params(
        slot,
        scene,
        force_bypass,
        &[],
        stimulus,
        ref_level,
        tail_ms,
        skip_load,
    )
}

/// [`capture_full_at`] plus `fs_params`: live param writes (a footswitch's `param`
/// functions at their engaged `valueA`) issued AFTER the scene recall and BEFORE the
/// isolation bypasses — `measure_fs_state`'s order, for the same reason (the recall
/// reverts earlier unsaved writes on the session). Without these writes a footswitch
/// sound's capture reflects only its on-off flips, so a param-function jump (level
/// change, defect shaping) would be silently absent from the measured audio.
#[allow(clippy::too_many_arguments)]
pub(super) fn capture_full_at_params(
    slot: ListIndex,
    scene: Option<u32>,
    force_bypass: &[(String, String, bool)],
    fs_params: &[(String, String, String, f32)],
    stimulus: &[f32],
    ref_level: Option<f32>,
    tail_ms: u64,
    skip_load: bool,
) -> Result<audio::Capture, LevelError> {
    // Names this capture in the trace (inert unless recording); an outer caller's name wins.
    let _trace_ctx = crate::trace::capture_ctx("capture", Some(slot.get()), scene, None);
    // The settles here are `sleep_abortable`: a Stop pressed anywhere in the ~1.9 s of
    // settling that brackets a capture bails immediately instead of being noticed only at
    // the next step seam. Safe to leave from any of these points — nothing is engaged or
    // written yet before the re-amp ON below.
    if !skip_load {
        load_preset_isolated(slot, CancelMode::Abortable)?;
    }
    let mut s = Session::connect_lean()?;
    let recall = if skip_load {
        scene
    } else {
        Some(scene.unwrap_or(crate::session::BASE_SCENE_SLOT))
    };
    // Re-assert the scene on THIS (capture) connection before setting the level —
    // load_scene recalls the scene's own state, so it must precede the reference-
    // level write, not follow it.
    if let Some(scene) = recall {
        s.load_scene(scene)?;
        settle_or_cancel(SETTLE_AFTER_SET_MS)?;
    }
    for (g, n, p, v) in fs_params {
        s.change_parameter(g, n, p, *v)?;
    }
    capture_on_session(&mut s, force_bypass, stimulus, ref_level, tail_ms)
}

/// Force-bypass isolation → optional reference level → engage → `reamp_capture` →
/// guaranteed re-amp off, on an ALREADY-OPEN session. When there is NEITHER an
/// isolation write NOR a reference level (the NAKED shape), two `heartbeat`s are
/// interleaved into the pre-engage settles so the engage never lands on a long
/// idle gap after the caller's scene recall — see the block comment on that
/// branch below. Does NOT load a preset or recall a scene — the caller does
/// that first, if at all, since re-`load_scene`ing
/// between writes reverts the prior write's unsaved value (`set_knobs`'s doc); this
/// seam exists precisely so a caller that has ALREADY applied unsaved edits on `s`
/// (Doctor's `ops_session`) can capture them without a further recall silently
/// discarding them. Shared tail of `capture_full_at` (which recalls the scene,
/// then calls this) and `doctor_capture_on_session_raw` (used by
/// `commands::doctor::doctor_apply`'s step (c), on the session step (b) just
/// wrote to).
pub(crate) fn capture_on_session(
    s: &mut Session,
    force_bypass: &[(String, String, bool)],
    stimulus: &[f32],
    ref_level: Option<f32>,
    tail_ms: u64,
) -> Result<audio::Capture, LevelError> {
    // Force-bypass isolation AFTER the caller's scene recall (`arm_measurement`'s rule: a
    // recall re-asserts that scene's own bypass state, so isolation written before it is
    // reverted) and before the engage. The presetLevel set below is a global multiplier, not
    // a scene-scoped write, so it can't revert these either way.
    for (g, n, byp) in force_bypass {
        s.change_parameter_bool(g, n, "bypass", *byp)?;
    }
    // `None` = capture at the preset's OWN stored level (Doctor's apply A/B),
    // leaving the edit buffer's presetLevel untouched.
    if let Some(ref_level) = ref_level {
        set_knob(s, &LevelKnob::PresetLevel, ref_level.clamp(0.05, 1.0), None)?;
    }
    // NAKED-SHAPE idle breaker: with no isolation write and no reference level, this
    // seam has sent nothing itself before the engage, which would otherwise land on a
    // ~600 ms idle gap after the caller's `load_scene` recall and read the device's
    // stationary output floor instead of the stimulus. `Session::heartbeat` is the
    // designed keep-alive and writes nothing: recall → 300 → hb → 300 → hb → 300 →
    // engage keeps every idle gap ≤300 ms and lands the engage ~900 ms post-recall.
    // HW evidence, the two candidate mechanisms, and why the timing is two-sided:
    // gotchas.md "An engage after a naked scene recall latches silence".
    //
    // PRODUCTION fix, not harness-only: the naked shape is reached by
    // `capture_asis_full` (probe --measure-scene / --measure-current), by
    // `measure_sound_asis_strict`'s scene rows (empty bypass, no reference level),
    // and by Doctor's apply A/B whenever the diagnosed sound needs no isolation. Two
    // shapes over-fire benignly toward the proven-green timing: caller-side
    // `fs_params` writes (invisible here — the line is already warm) and the
    // no-recall captures (no mute window to outlive) — both cost one extra 300 ms
    // settle on one-shot paths, never in a hot loop.
    //
    // Write-bearing paths (any `force_bypass` entry, or a `ref_level`) deliberately
    // skip this: their transact round-trips already break the idle and land the
    // engage ≥~850 ms post-recall — HW-validated green, do not perturb.
    //
    if force_bypass.is_empty() && ref_level.is_none() {
        s.heartbeat()?;
        settle_or_cancel(SETTLE_AFTER_SET_MS)?;
        s.heartbeat()?;
    }
    // Settle UNCONDITIONALLY before the engage: the caller (or the loop above) may
    // have just issued bypass/param writes even when `ref_level` is `None` — the
    // Doctor apply A/B path — and `measure_fs_state` always settles after its
    // writes for the same reason (the engage latches whatever the DSP holds).
    settle_or_cancel(SETTLE_AFTER_SET_MS)?;
    let _ = s.set_reamp_mode(true)?;
    // Past the engage there is NO early return: re-amp is on, and leaving it on strands the
    // unit input-muted. So this settle only wakes early — `reamp_capture` then bails on its
    // own up-front abort check, and the OFF below still goes out on this open session.
    let _ = settle_abortable(SETTLE_AFTER_REAMP_MS);
    let cap = audio::reamp_capture(stimulus, RATE, tail_ms);
    let _ = s.set_reamp_mode(false);
    cap
}

/// MEASURE seam returning the FULL multi-channel capture: load `slot`, re-amp
/// `stimulus` at `ref_level`, return every captured channel. Validated own-conn
/// load → fresh-connect set → engage re-amp once → capture → off. `capture_samples`
/// and the per-channel N1 diagnostic (`probe --channels`) share this.
pub fn capture_full(
    slot: ListIndex,
    stimulus: &[f32],
    ref_level: f32,
) -> Result<audio::Capture, LevelError> {
    capture_full_at(
        slot,
        None,
        &[],
        stimulus,
        Some(ref_level),
        CAPTURE_TAIL_MS,
        false,
    )
}

/// Repro instrumentation: the FULL multi-channel as-is capture, run through
/// `capture_full_at`'s production choreography, so a probe can report EVERY channel's loudness and
/// make the loudest-channel argmax observable instead of inferred.
pub fn capture_asis_full(
    slot: Option<ListIndex>,
    scene: Option<u32>,
    stimulus: &[f32],
) -> Result<audio::Capture, LevelError> {
    capture_full_at(
        slot.unwrap_or_default(), // unused when skip_load
        scene,
        &[],
        stimulus,
        None,
        CAPTURE_TAIL_MS,
        slot.is_none(),
    )
}

/// MEASURE seam for analysis (spectrum / audit): load `slot`, re-amp the
/// `stimulus` at `ref_level`, and return the loudest captured channel's raw samples +
/// rate (for FFT / band analysis). Mirrors the validated `measure_c` + `measure_knob_at`
/// sequence (own-connection load → fresh-connect set → engage re-amp once → capture).
pub fn capture_samples(
    slot: ListIndex,
    stimulus: &[f32],
    ref_level: f32,
) -> Result<(Vec<f32>, u32), LevelError> {
    let cap = capture_full(slot, stimulus, ref_level)?;
    let (ch, _) = cap.loudest_channel();
    Ok((cap.channel(ch), cap.sample_rate))
}

/// [`capture_samples`] with a caller-supplied force-bypass list, so a probe can
/// measure the re-amp PATH rather than a preset's tone.
///
/// Why this is needed: a bandwidth test through a normal preset measures the
/// AMP's HF rolloff, not the transport's. Measured on fw 1.8.45, a
/// TubeScreamer→Plexi chain puts the capture 49 dB down by 16 kHz and at the
/// float floor by 22 kHz — which mimics a band-limit cliff that may not be there.
/// Bypassing every block makes the chain approximately a wire, so whatever
/// reaches 20–24 kHz is a property of the path, not of the tone.
///
/// Uses the same `capture_full_at` ordering as the leveller's own force-bypass
/// callers: the bypasses land after the load, before the engage.
pub fn capture_samples_bypassed(
    slot: ListIndex,
    stimulus: &[f32],
    ref_level: f32,
    force_bypass: &[(String, String, bool)],
) -> Result<(Vec<f32>, u32), LevelError> {
    let cap = capture_full_at(
        slot,
        None,
        force_bypass,
        stimulus,
        Some(ref_level),
        CAPTURE_TAIL_MS,
        false,
    )?;
    let (ch, _) = cap.loudest_channel();
    Ok((cap.channel(ch), cap.sample_rate))
}

/// STRICT-HARNESS measure (the online e2e's post-leveling audio gate,
/// `level.online.spec.ts`): re-measure one leveled sound of `slot` AS-IS on the
/// production capture path and the LEVELING metric (2-ch BS.1770 over the
/// processed pair, floor-guarded), so the spec can assert the SAVED state actually renders at the
/// leveling target — not merely that the run reported success. Context mirrors
/// the leveling lanes exactly:
/// * scene sound — pure as-is (`loadScene` applies the scene's own `ftswStates`);
///   `force_bypass` empty;
/// * base sound — fresh load + base recall + the base isolation (every
///   block-acting switch's block forced off) in `force_bypass`;
/// * footswitch sound — fresh load + base recall + siblings-off + own engaged
///   flip in `force_bypass`, plus `fs_value` re-playing an ASSIGN switch's saved
///   `valueA` onto the leveled param (a BAKED switch needs no write — its engaged
///   sound IS the base value).
///
/// `validate` is the P5 EXTERNAL-VALIDATION add-on (`crate::validate_log`): when it is
/// `Some` AND `TMP_E2E_VALIDATE_LOG` is set, the capture behind the returned loudness
/// is ALSO written to a WAV and one expectation row is appended to the log, so an
/// ffmpeg `ebur128` read this repo did not write can judge the same audio. Pure add-on
/// — `None` (every production call) is byte-identical to the previous behaviour, and
/// the env check happens before any extra work. Deliberately emitted HERE rather than
/// at the solve: the solve captures at its REFERENCE level, so its PCM is not the saved
/// preset's output.
pub fn measure_sound_asis_strict(
    slot: ListIndex,
    scene: Option<u32>,
    force_bypass: &[(String, String, bool)],
    fs_value: Option<((String, String, String), f32)>,
    stimulus: &[f32],
    validate: Option<&crate::validate_log::ValidationRow>,
) -> Result<lufs::Loudness, LevelError> {
    // The trace names these captures by the validation row's identity when there is one.
    let _trace_ctx = match validate {
        Some(r) => crate::trace::capture_ctx(&r.label, Some(r.slot), r.scene_slot, r.switch),
        None => crate::trace::capture_ctx("strict", Some(slot.get()), scene, None),
    };
    // Resolved ONCE, up front: an unarmed run must not clone a ~2.7 MB capture per
    // floor-guard attempt just to throw it away.
    let keep = validate.is_some() && crate::validate_log::log_path().is_some();
    // The capture the returned loudness was measured from — the LAST attempt the floor
    // guard made, so the dumped WAV and the reported number always describe one capture.
    let mut kept: Option<audio::Capture> = None;
    let measured = if let Some(((g, n, p), v)) = fs_value {
        load_preset_isolated(slot, CancelMode::Cleanup)?;
        require_live(
            || {
                // NO intended-level assert, deliberately: this seam's whole contract is
                // "re-measure the SAVED state as-is", so it must render at the level the
                // preset stores, not at any level a run wanted.
                let cap = capture_fs_at((&g, &n, &p), force_bypass, stimulus, v, None)?;
                let loud = processed_loudness_of(&cap)?;
                if keep {
                    kept = Some(cap);
                }
                Ok(loud)
            },
            stimulus,
        )
    } else {
        require_live(
            || {
                let cap = capture_full_at(
                    slot,
                    scene,
                    force_bypass,
                    stimulus,
                    None,
                    CAPTURE_TAIL_MS,
                    false,
                )?;
                let loud = processed_loudness_of(&cap)?;
                if keep {
                    kept = Some(cap);
                }
                Ok(loud)
            },
            stimulus,
        )
    }?;
    if let (Some(row), Some(cap)) = (validate, kept.as_ref()) {
        crate::validate_log::emit(row, cap, &measured);
    }
    Ok(measured)
}

/// MEASURE seam for scene leveling: load `slot`, then for each scene in
/// `0..scene_count` activate it (`loadScene`) and capture its ceiling loudness at
/// `presetLevel = 1.0`. Returns per-scene loudness (LUFS) — feed to
/// `scenes::normalize_scene_targets` for the per-scene gain offsets. The scene is
/// re-asserted on the CAPTURE connection immediately before `set_knob` (same
/// connect→load_scene→set→engage ordering as `measure_scene_asis`) — a scene
/// loaded on the earlier preset-load connection does not reliably survive the
/// reconnect.
pub fn capture_scene_ceilings(
    slot: ListIndex,
    scene_count: u32,
    stimulus: &[f32],
) -> Result<Vec<f64>, LevelError> {
    let mut cs = Vec::with_capacity(scene_count as usize);
    // Scenes are 0-based `scenes[]` indices on the wire (base is the constant slot 8)
    // — HW-proven by the `--loadscene 1` → scenes[1] activegraph diff. Slot 0 IS
    // addressable because `proto::load_scene` now emits the field explicitly even
    // for 0 (the device ignores an empty LoadScene{} — HW-found).
    for scene in 0..scene_count {
        load_preset_isolated(slot, CancelMode::Cleanup)?;
        let mut s = Session::connect_lean()?;
        s.load_scene(scene)?;
        crate::settle(Duration::from_millis(SETTLE_AFTER_SET_MS));
        set_knob(&mut s, &LevelKnob::PresetLevel, 1.0, None)?;
        crate::settle(Duration::from_millis(SETTLE_AFTER_SET_MS));
        cs.push(engage_measure_disengage(&mut s, stimulus)?.integrated_lufs);
    }
    Ok(cs)
}

/// Fresh-connect, load `scene_slot`, engage re-amp, capture — measure the scene's
/// loudness AS-IS without writing ANY parameter (no `set_knob`, no Scene Edit). Lets
/// the one-shot runner decide whether a scene already sits at target before touching
/// it. The preset must already be current (loaded in a prior connection).
/// The integrated LUFS of a re-amp capture's PROCESSED pair, erroring on silence.
/// The shared tail of every isolated measurement (load/engage/capture/disengage
/// differ per caller; this `capture → stereo-or-mono measure → finite-check` is
/// identical). `pub(crate)` so the `lib.rs` probe measure paths share it too.
/// Renamed from `loudest_lufs` (PR2): the metric is a 2-ch BS.1770 sum over
/// USB-Out 1/2, not an argmax-picked single channel — `loudest_channel` is still
/// used elsewhere (advisory VU pick, per-channel probe diagnostics) but no longer
/// describes this hub.
pub(crate) fn processed_lufs(cap: Result<audio::Capture, LevelError>) -> Result<f64, LevelError> {
    processed_loudness(cap).map(|l| l.integrated_lufs)
}

/// Like [`processed_lufs`] but keeps the full meter reading (integrated + short-term
/// max), for paths that report the capture's dynamics spread alongside the level.
pub(super) fn processed_loudness(
    cap: Result<audio::Capture, LevelError>,
) -> Result<lufs::Loudness, LevelError> {
    processed_loudness_of(&cap?)
}

/// [`processed_loudness`] on a BORROWED capture — for the one caller that must keep the
/// PCM alive past the measurement (the strict re-measure's external-validation dump,
/// `measure_sound_asis_strict`). Same verdict, same sentinel error.
pub(super) fn processed_loudness_of(cap: &audio::Capture) -> Result<lufs::Loudness, LevelError> {
    loudness_verdict(measure_processed(cap)?)
}

/// The hub's verdict on one processed-pair reading: silence → [`LevelError::NoSignal`], a
/// flat-topped capture → [`LevelError::Clipped`] (its integrated reading is LOW, so no lane
/// may solve from it), else the reading.
pub(super) fn loudness_verdict(m: lufs::Loudness) -> Result<lufs::Loudness, LevelError> {
    if !m.integrated_lufs.is_finite() {
        return Err(LevelError::NoSignal);
    }
    if m.clipped {
        log::warn!(
            "capture clipped at full scale ({:.2} LUFS, {:.2} dBTP) — refusing to solve from it",
            m.integrated_lufs,
            m.true_peak_dbtp
        );
        return Err(LevelError::Clipped);
    }
    Ok(m)
}

/// The ONE engaged/floor criterion this crate has: finite chain audio meaningfully
/// above the stationary floor, with real dynamics. A floor read is near-flat in the
/// 400 ms momentary loudness (≈0.1 LU), while a plucked stimulus keeps its note-to-note
/// envelope through any chain, so the momentary spread must clear `FLOOR_TRIP_LU`. NaN
/// comparisons are false, so a failed measure reads "not engaged".
///
/// `danger.md`: a silent/failed re-amp inject reads as the device's stationary OUTPUT
/// FLOOR — a real number for the wrong signal. This is what `probe --measure-current`'s
/// FLOOR/SILENT headline and `validate_log`'s `engaged` verdict both stamp from, so a
/// consumer never has to re-derive it (`probe_api::level::is_engaged` delegates here).
pub(crate) fn is_engaged(l: &lufs::Loudness) -> bool {
    l.integrated_lufs.is_finite()
        && l.integrated_lufs > -50.0
        && l.momentary_spread_lu > FLOOR_TRIP_LU
}

/// The metering convention shared by every OUTPUT-side measurement hub in this
/// module: 2-ch BS.1770 over the processed USB pair as the user's rig plays it
/// (`Capture::metered_pair` under the Settings playback-channels choice,
/// `audio::meter_channels` — stereo by default), falling back to
/// [`lufs::measure_mono`] on channel 0 for a genuinely 1-channel capture (never
/// duplicating it into fake dual-mono — see `processed_stereo`'s doc). The
/// INPUT/stimulus side (`stimulus_spread_lu`, calibration, etc.) never calls this —
/// it stays on `measure_mono` directly. `pub(crate)` so a probe diagnostic headline
/// can share the exact production convention instead of a parallel per-channel
/// re-check (`probe_api::level::probe_measure_current_lufs`).
pub(crate) fn measure_processed(cap: &audio::Capture) -> Result<lufs::Loudness, LevelError> {
    measure_processed_as(cap, audio::meter_channels())
}

/// [`measure_processed`] under an explicit playback-channels mode — for the one
/// caller that must NOT follow the setting (the Doctor's analysis capture pins
/// `Stereo`) and for tests.
pub(crate) fn measure_processed_as(
    cap: &audio::Capture,
    mode: crate::profiles::PlaybackChannels,
) -> Result<lufs::Loudness, LevelError> {
    let sample_rate = cap.sample_rate;
    Ok(match cap.metered_pair(mode) {
        Some(pair) => lufs::measure_stereo(&pair, sample_rate),
        None => lufs::measure_mono(&cap.channel(0), sample_rate),
    }?)
}

#[cfg(test)]
mod playback_channels_meter_tests {
    use super::*;
    use crate::profiles::PlaybackChannels::{self, LeftOnly, MonoSum, Stereo};

    const SR: u32 = 48_000;

    /// A 4-channel TMP-shaped capture (USB-Out 1/2 processed, 3 = dry DI, 4 = idle)
    /// of `secs` of `l(i)`/`r(i)`, with a loud dry channel the meter must ignore.
    fn capture(secs: f32, l: impl Fn(f32) -> f32, r: impl Fn(f32) -> f32) -> audio::Capture {
        let n = (secs * SR as f32) as usize;
        let mut interleaved = Vec::with_capacity(n * 4);
        for i in 0..n {
            let t = i as f32 / SR as f32;
            interleaved.extend_from_slice(&[l(t), r(t), 0.9, 0.0]);
        }
        audio::Capture {
            interleaved,
            channels: 4,
            sample_rate: SR,
        }
    }

    /// A plucked-ish 220 Hz note train: decaying bursts, so the gates and the momentary
    /// spread see real dynamics, not a stationary tone.
    fn plucks(t: f32) -> f32 {
        let phase = t % 0.5;
        0.3 * (-6.0 * phase).exp() * (std::f32::consts::TAU * 220.0 * t).sin()
    }

    fn lufs_of(cap: &audio::Capture, mode: PlaybackChannels) -> f64 {
        measure_processed_as(cap, mode).unwrap().integrated_lufs
    }

    #[test]
    fn dual_mono_reads_identically_in_every_mode() {
        let cap = capture(4.0, plucks, plucks);
        let stereo = measure_processed_as(&cap, Stereo).unwrap();
        for mode in [MonoSum, LeftOnly] {
            let m = measure_processed_as(&cap, mode).unwrap();
            assert_eq!(m.integrated_lufs, stereo.integrated_lufs, "{mode:?}");
            assert_eq!(
                m.momentary_spread_lu, stereo.momentary_spread_lu,
                "{mode:?}"
            );
        }
        // …and each mono mode is exactly "L metered as mono, +3.0103 dB".
        let mono = lufs::measure_mono(&cap.channel(0), SR)
            .unwrap()
            .integrated_lufs;
        assert!((stereo.integrated_lufs - (mono + 3.0103)).abs() < 1e-3);
    }

    #[test]
    fn decorrelated_stereo_shows_the_mono_sum_and_left_only_differences() {
        // L and R in QUADRATURE at one frequency: orthogonal (zero correlation), same
        // K-weighting gain, steady — so the expected offsets are closed-form.
        // stereo ∝ a² + b²; mono sum ∝ 2·((a² + b²)/4); left only ∝ 2·a².
        let (a, b) = (0.2_f32, 0.1_f32);
        let w = std::f32::consts::TAU * 1000.0;
        let cap = capture(4.0, |t| a * (w * t).sin(), |t| b * (w * t).cos());
        let stereo = lufs_of(&cap, Stereo);
        let mono_sum = lufs_of(&cap, MonoSum);
        let left = lufs_of(&cap, LeftOnly);
        let (a2, b2) = (f64::from(a * a), f64::from(b * b));
        assert!(
            (mono_sum - (stereo - 3.0103)).abs() < 0.02,
            "uncorrelated L/R lose 3 dB when summed: stereo {stereo:.3}, mono sum {mono_sum:.3}"
        );
        let want_left = stereo + 10.0 * (2.0 * a2 / (a2 + b2)).log10(); // +2.04 dB
        assert!(
            (left - want_left).abs() < 0.02,
            "left only {left:.3}, want {want_left:.3} (stereo {stereo:.3})"
        );
        // A right-heavy preset reads QUIETER left-only than in stereo.
        let swapped = capture(4.0, |t| b * (w * t).cos(), |t| a * (w * t).sin());
        assert!(lufs_of(&swapped, LeftOnly) < lufs_of(&swapped, Stereo) - 3.0);
    }

    #[test]
    fn the_stored_setting_reaches_both_output_side_hubs() {
        // Ping-pong-ish: the note on the left, a quieter copy delayed on the right.
        let cap = capture(4.0, plucks, |t| 0.4 * plucks((t - 0.25).max(0.0)));
        let mut store = crate::profiles::Store::default();
        assert_eq!(store.playback_channels, Stereo, "default is stereo");
        for mode in [Stereo, MonoSum, LeftOnly] {
            store.playback_channels = mode;
            crate::profiles::apply_meter_channels(&store);
            assert_eq!(audio::meter_channels(), mode);
            let want = lufs_of(&cap, mode);
            // Hub 1: the full-capture leveling meter.
            assert_eq!(
                measure_processed(&cap).unwrap().integrated_lufs,
                want,
                "{mode:?}"
            );
            // Hub 2: the streaming meter behind the live-LUFS readout (and the adaptive
            // measure), fed the RAW 4-channel frames in hops as the capture loop does.
            let mut live = audio::PairMeter::start(4, SR).unwrap();
            for hop in cap.interleaved.chunks(4 * SR as usize / 5) {
                live.add(hop).unwrap();
            }
            let got = live.integrated().unwrap();
            assert!(
                (got - want).abs() < 0.01,
                "{mode:?}: live {got:.4} vs full {want:.4}"
            );
        }
        // The three modes really differ on this capture, so the checks above can fail.
        assert!((lufs_of(&cap, Stereo) - lufs_of(&cap, MonoSum)).abs() > 0.3);
        assert!((lufs_of(&cap, Stereo) - lufs_of(&cap, LeftOnly)).abs() > 0.3);
        crate::profiles::apply_meter_channels(&crate::profiles::Store::default());
        assert_eq!(audio::meter_channels(), Stereo);
    }
}

/// Engage re-amp on `s` (latching the already-set knob/scene), settle, capture the
/// FULL stimulus + decay tail, measure the processed pair's integrated LUFS, then
/// disengage. The shared tail of every isolated leveling measurement (the
/// connect/load/set prefix differs per caller).
///
/// This deliberately uses the FULL capture, not the adaptive `audio::reamp_measure`.
/// The offline harness proved that trimming the window — early-exit, dropping the
/// 0.8 s tail, OR skipping a pre-roll — shifts the measured loudness up to ~0.4 LU on
/// time-effect/reverb presets (quiet delay buildup + decay tail that production
/// integrates). Adopting the adaptive capture is a measurement RE-BASELINE; until
/// that's signed off, leveling keeps the validated full-capture metric. The adaptive
/// path is HW-A/B-able via `probe --measure-adaptive`.
pub(crate) fn engage_measure_disengage(
    s: &mut Session,
    stimulus: &[f32],
) -> Result<lufs::Loudness, LevelError> {
    processed_loudness(engage_capture_disengage(s, stimulus))
}

/// [`engage_measure_disengage`] stopping one step short — returns the raw capture
/// instead of its loudness, for the caller that must also write the PCM to disk
/// (`measure_sound_asis_strict`'s external-validation dump). The engage/settle/capture/
/// disengage sequence is IDENTICAL: this is an extraction, not a second choreography.
pub(crate) fn engage_capture_disengage(
    s: &mut Session,
    stimulus: &[f32],
) -> Result<audio::Capture, LevelError> {
    let _ = s.set_reamp_mode(true)?;
    // Same no-early-return rule as `capture_full_at`: re-amp is engaged, the OFF must fire.
    let _ = settle_abortable(SETTLE_AFTER_REAMP_MS);
    let cap = audio::reamp_capture(stimulus, RATE, CAPTURE_TAIL_MS);
    let _ = s.set_reamp_mode(false);
    cap
}

/// One as-is scene reading: fresh connection, recall, engage, measure, disengage —
/// writing NOTHING. Every scene ceiling in the prepass and every jointk/verify as-is
/// reading comes through here.
///
/// The recall→engage sequence is shaped like the NAKED shape `capture_on_session` breaks
/// with heartbeats, and this seam does NOT route through that function — so it carries its
/// own copy of the breaker: recall → 300 → hb → 300 → hb → 300 → engage, every idle gap
/// ≤300 ms and the engage ~900 ms post-recall.
///
/// HARDENING, NOT A FIX FOR AN OBSERVED FAILURE — be precise about this, because the
/// evidence cuts the other way. gotchas.md "An engage after a naked scene recall latches
/// silence" records THIS seam's old `load_scene → 300 → engage` shape measuring the same
/// heavy amp-flip scenes LOUD, and cites that as the wrinkle favouring the idle-gap
/// mechanism over the DSP-mute one; the shapes that died were `probe --measure-scene`,
/// which reaches the engage through `capture_on_session`. So this is defence in depth:
/// the seam sat one un-analysed 300 ms gap away from the failing shape while the
/// mechanism is only partly understood, and `require_live` turns a floor read here into a
/// hard "couldn't read this sound" error. It costs 600 ms per as-is reading and moves the
/// timing onto the HW-proven-green cadence. Do NOT cite it as the cause of a scene-read
/// failure without new evidence.
///
/// `intended_preset_level` is the run's OWN `presetLevel` — the value it solved or is
/// holding UNSAVED in the working copy — re-asserted right after the recall. The recall runs
/// the device's own level-apply (`recall_reassert_save`'s doc carries the HW evidence), so
/// without this the capture renders at the level the DEVICE HAS SAVED, which is stale by the
/// whole raise while a headroom trade holds an unsaved `presetLevel`. `None` = assert nothing (capture at the preset's own
/// stored level) — the reading every caller that has no run-owned value still wants.
///
/// The assert is INSERTED into the breaker, never spliced over it: recall → 300 → set → 300 →
/// hb → 300 → hb → 300 → engage. Every idle gap stays ≤300 ms and the cadence above survives
/// intact; the engage simply lands ~1200 ms post-recall instead of ~900 ms.
// A4/F11: this seam does NOT route through `arm_measurement` — it recalls a SCENE, not a
// single `LevelKnob`, so there is no knob to hand that seam's `LevelKnob`-shaped API. The same
// ordering rule applies by hand instead: SCENE CONTEXT FIRST (the `load_scene` recall), THE
// INTENDED `presetLevel` next (between the recall and isolation — a `presetLevel` set before
// the recall would be reverted by it), ISOLATION LAST (`load_scene` re-asserts the scene's own
// bypass state, so bypasses written before it are silently undone — `capture_on_session`'s
// rule) — immediately before the engage.
pub(super) fn measure_scene_asis(
    scene_slot: u32,
    stimulus: &[f32],
    intended_preset_level: Option<f32>,
    force_bypass: &[(String, String, bool)],
) -> Result<lufs::Loudness, LevelError> {
    let mut s = Session::connect_lean()?;
    s.load_scene(scene_slot)?;
    settle_or_cancel(SETTLE_AFTER_SET_MS)?;
    if let Some(pl) = intended_preset_level {
        set_knob(&mut s, &LevelKnob::PresetLevel, pl, None)?;
        settle_or_cancel(SETTLE_AFTER_SET_MS)?;
    }
    for (g, n, byp) in force_bypass {
        s.change_parameter_bool(g, n, "bypass", *byp)?;
    }
    s.heartbeat()?;
    settle_or_cancel(SETTLE_AFTER_SET_MS)?;
    s.heartbeat()?;
    settle_or_cancel(SETTLE_AFTER_SET_MS)?;
    engage_measure_disengage(&mut s, stimulus)
}

/// Everything an isolated measurement writes on its connection BEFORE the re-amp engage,
/// in the ONE order the device's rules allow — the shared arming seam (`measure_knob_at`,
/// `measure_fs_at`), unit-tested against `SimDevice` since the engage/capture tail can't be.
///
/// 1. SCENE CONTEXT FIRST. A preset loads into its SAVED `lastLoadedScene`, never base
///    (HW), so a measurement that recalls nothing measures whatever scene the connection
///    happened to hold — the reported bug where a base/footswitch job was measured in
///    scene 3. `set_knobs` recalls the knob's own context (the scene for a scene knob,
///    base for a base block); `PresetLevel` is a global multiplier it deliberately does
///    NOT recall for, so the base recall is issued here.
/// 2. ISOLATION LAST. `load_scene` re-asserts that scene's own bypass state
///    (`capture_on_session`'s rule), so forced bypasses written before the recall are
///    silently reverted. Every capture that recalls therefore re-sends the FULL list.
///
/// 1b. THE INTENDED `presetLevel`, BETWEEN 1 and 2 — never before `set_knob`. A BLOCK knob's
///    own scene recall happens INSIDE `set_knobs` (its `has_base_block` / scene branch), and
///    that recall runs the device's own level-apply, so a `presetLevel` written above the
///    `set_knob` call is silently reverted by it and the capture renders at whatever level
///    the device HAS SAVED — stale by the whole raise while a headroom trade holds an unsaved
///    `presetLevel`
///    (`recall_reassert_save`'s doc carries the HW evidence). Written as a plain
///    `setPresetLevel` (no recall of its own — `set_knobs`' `PresetLevel`-only branch), so it
///    cannot revert the knob write that precedes it, and no settle is added: the isolation
///    writes and the single settle below already bracket it exactly as they bracket the knob
///    write. `None` = assert nothing (today's behaviour) — and a `PresetLevel` CALLER must
///    pass `None`, since `value` is already the level under measurement.
pub(super) fn arm_measurement(
    s: &mut Session,
    knob: &LevelKnob,
    value: f32,
    force_bypass: &[(String, String, bool)],
    saved: Option<&serde_json::Value>,
    intended_preset_level: Option<f32>,
) -> Result<(), LevelError> {
    if matches!(knob, LevelKnob::PresetLevel) {
        recall_base(s)?;
    }
    set_knob(s, knob, value, saved)?;
    if let Some(pl) = intended_preset_level {
        set_knob(s, &LevelKnob::PresetLevel, pl, None)?;
    }
    for (g, n, byp) in force_bypass {
        s.change_parameter_bool(g, n, "bypass", *byp)?;
    }
    // Cancellable: nothing is engaged yet and the connection is throwaway, so a Stop
    // here bails cleanly (the same settle #128 made interruptible pre-refactor).
    settle_or_cancel(SETTLE_AFTER_SET_MS)?;
    Ok(())
}

/// Everything `measure_pair_at` writes BEFORE the re-amp engage — the shared arming seam
/// verbatim (`arm_measurement` has the write order), recalling `scene` when there is one.
///
/// BASE ARM ONLY: with `writes` empty, interleave `heartbeat → settle → heartbeat` before the
/// final settle, or the trailing settles leave a ~600 ms idle gap that latches the floor.
/// [→ evidence](../../notes/gotchas.md#an-engage-after-a-naked-scene-recall-latches-silence--break-the-idle-with-heartbeats)
pub(super) fn arm_pair_measurement(
    s: &mut Session,
    scene: Option<u32>,
    preset_level: f32,
    writes: &[(String, String, String, f32)],
) -> Result<(), LevelError> {
    match scene {
        None => arm_measurement(s, &LevelKnob::PresetLevel, preset_level, &[], None, None)?,
        Some(sc) => {
            s.load_scene(sc)?;
            settle_or_cancel(SETTLE_AFTER_SET_MS)?;
            set_knob(s, &LevelKnob::PresetLevel, preset_level, None)?;
        }
    }
    for (g, n, p, v) in writes {
        s.change_parameter(g, n, p, *v)?;
    }
    // NAKED-GAP breaker — base arm, zero writes only. See this fn's doc for the bisect
    // that pins the shape and why it must not spread to the scene arm or a write-bearing
    // base run.
    if scene.is_none() && writes.is_empty() {
        s.heartbeat()?;
        settle_or_cancel(SETTLE_AFTER_SET_MS)?;
        s.heartbeat()?;
    }
    settle_or_cancel(SETTLE_AFTER_SET_MS)?;
    Ok(())
}

/// One fresh-connection measurement at an explicit (`presetLevel` × block-param) POINT:
/// base recall + presetLevel via `arm_measurement`, then each block write live on the same
/// armed connection, one engage, measure. P0 instrumentation seam for the headroom-trade
/// physics (presetLevel↑ / outputLevel↓ product invariance) — writes stay on the throwaway
/// working copy; the caller reloads to discard. See [`arm_pair_measurement`] for the
/// pre-engage choreography (and its zero-write naked-gap fix).
pub(crate) fn measure_pair_at(
    scene: Option<u32>,
    preset_level: f32,
    writes: &[(String, String, String, f32)],
    stimulus: &[f32],
) -> Result<lufs::Loudness, LevelError> {
    let mut s = Session::connect_lean()?;
    arm_pair_measurement(&mut s, scene, preset_level, writes)?;
    engage_measure_disengage(&mut s, stimulus)
}

/// Fresh-connect, arm the measurement (`arm_measurement`: scene context → knob → intended
/// `presetLevel` → isolation), engage re-amp once, measure the processed pair on the full
/// capture. Restores re-amp OFF. `intended_preset_level` is `arm_measurement`'s — the run's
/// own solved/held level, `None` to assert nothing (and always `None` when `knob` is
/// `PresetLevel`, which IS the level under measurement).
pub(super) fn measure_knob_at(
    stimulus: &[f32],
    knob: &LevelKnob,
    value: f32,
    force_bypass: &[(String, String, bool)],
    saved: Option<&serde_json::Value>,
    intended_preset_level: Option<f32>,
) -> Result<lufs::Loudness, LevelError> {
    let mut s = Session::connect_lean()?;
    arm_measurement(
        &mut s,
        knob,
        value,
        force_bypass,
        saved,
        intended_preset_level,
    )?;
    engage_measure_disengage(&mut s, stimulus)
}

#[cfg(test)]
mod floor_guard_tests {
    use super::*;

    /// A reading with short-term spread `st` and momentary spread `mom`.
    fn loud(integrated: f64, st: f64, mom: f64) -> lufs::Loudness {
        lufs::Loudness {
            integrated_lufs: integrated,
            short_term_max_lufs: integrated + st,
            true_peak_dbtp: integrated + 12.0,
            momentary_spread_lu: mom,
            clipped: false,
        }
    }

    /// A lively capture (compressed relative to [`STIM`]) and a stationary floor read.
    fn lively() -> lufs::Loudness {
        loud(-30.0, 0.2, 5.0)
    }
    fn flat() -> lufs::Loudness {
        loud(-30.18, 0.01, 0.08)
    }
    /// The shipped stimuli's momentary spread (~6 LU).
    const STIM: f64 = 6.0;

    fn shipped_stimuli() -> Vec<(String, Vec<f32>)> {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/resources/samples");
        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .expect("samples dir")
            .map(|e| e.expect("entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "wav"))
            .collect();
        paths.sort();
        assert!(
            paths.len() >= 8,
            "expected the 8 shipped stimuli, got {paths:?}"
        );
        paths
            .into_iter()
            .map(|p| {
                let s = crate::stimulus::read_stimulus_48k(p.to_str().expect("utf8"))
                    .expect("read stimulus");
                (p.display().to_string(), s)
            })
            .collect()
    }

    /// xorshift white noise in [-amp, amp] — deterministic, so the test is too.
    fn noise(secs: f32, amp: f32, mut x: u32) -> Vec<f32> {
        (0..(secs * RATE as f32) as usize)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x as f32 / u32::MAX as f32 * 2.0 - 1.0) * amp
            })
            .collect()
    }

    /// Scale `samples` so they measure `lufs` integrated (mono meter).
    fn at_lufs(samples: &[f32], lufs: f64) -> Vec<f32> {
        let now = lufs::measure_mono(samples, RATE)
            .expect("measure")
            .integrated_lufs;
        let g = 10f64.powf((lufs - now) / 20.0) as f32;
        samples.iter().map(|x| x * g).collect()
    }

    // BUG→GATE (#233): the guard used to read the 3 s short-term spread, which the shipped
    // stimuli (10 plucks 0.6 s apart) only move by 0.26–0.32 LU — under the 0.30 arming gate
    // for 7 of 8 of them, and ≈ 0 for all 8 on the Doctor's 3 s slice. Every shipped WAV must
    // ARM the guard, on both the leveling stimulus and the Doctor slice.
    #[test]
    fn every_shipped_stimulus_arms_the_floor_guard() {
        for (name, stim) in shipped_stimuli() {
            let full = stimulus_momentary_spread(&stim);
            assert!(
                floor_suspect(0.0, full),
                "{name}: leveling stimulus momentary spread {full:.2} LU does not arm the guard"
            );
            let doctor = stimulus_momentary_spread(&doctor_stim_slice(stim.clone()));
            assert!(
                floor_suspect(0.0, doctor),
                "{name}: Doctor slice momentary spread {doctor:.2} LU does not arm the guard"
            );
        }
    }

    // A synthetic stationary floor at ordinary leveling loudness trips the guard; the same
    // stimuli through 40 dB of hard saturation — harsher than any library preset — and
    // through plain gain do not. The trip gate sits between the two with margin both ways.
    #[test]
    fn a_stationary_floor_trips_and_a_saturated_stimulus_clears() {
        let floor = at_lufs(&noise(6.8, 0.1, 0x1234_5678), -27.0);
        let f = lufs::measure_mono(&floor, RATE).unwrap();
        assert!(
            floor_suspect(f.momentary_spread_lu, STIM),
            "floor momentary spread {:.3} LU",
            f.momentary_spread_lu
        );
        assert!(
            !is_engaged(&f),
            "a floor read at -27 LUFS must not read as engaged"
        );
        for (name, stim) in shipped_stimuli() {
            let s = stimulus_momentary_spread(&stim);
            let saturated: Vec<f32> = stim.iter().map(|x| (x * 100.0).tanh() * 0.5).collect();
            let l = lufs::measure_mono(&saturated, RATE).unwrap();
            assert!(
                !floor_suspect(l.momentary_spread_lu, s),
                "{name}: a saturated live capture ({:.2} LU) must not trip the guard",
                l.momentary_spread_lu
            );
            assert!(
                is_engaged(&l),
                "{name}: a saturated live capture reads engaged"
            );
        }
    }

    #[test]
    fn floor_suspect_trips_only_below_gate_with_lively_stimulus() {
        assert!(floor_suspect(0.08, 6.0)); // classic floor read
        assert!(floor_suspect(FLOOR_TRIP_LU, 6.0)); // boundary inclusive
        assert!(!floor_suspect(0.7, 6.0)); // a saturated live capture stays clear
        assert!(!floor_suspect(0.08, STATIONARY_STIM_LU)); // stationary stimulus disarms
    }

    // BUG→GATE (#234): the floor is post-DSP, so `presetLevel` scales it by the same 20·log10
    // a real signal follows (`notes/gotchas.md`). The old level-shift confirm accepted such a
    // floor as "ultra-compressed but real"; now a floor that stays flat through the retry is
    // the honest error, whatever level it was read at.
    #[test]
    fn a_floor_that_scales_with_preset_level_is_rejected() {
        let mut calls = 0;
        let r = measure_floor_guarded(
            || {
                calls += 1;
                // Each read 6.02 dB down, exactly as a level-tracking floor would.
                Ok(loud(-30.0 - 6.02 * calls as f64, 0.01, 0.08))
            },
            STIM,
            Duration::ZERO,
        )
        .unwrap();
        assert!(matches!(r, GuardOutcome::StillFlat(_)));
        assert_eq!(calls, 2);
    }

    // Predicted true peak scales with the same 20·log10(ratio) as presetLevel itself.
    #[test]
    fn predicted_true_peak_scales_with_level_ratio() {
        assert!((predicted_true_peak_dbtp(-3.0, 0.5, 0.5) - -3.0).abs() < 1e-6);
        let halved = predicted_true_peak_dbtp(-3.0, 0.5, 0.25);
        assert!((halved - -9.02).abs() < 0.01, "got {halved}");
        let doubled = predicted_true_peak_dbtp(-9.0, 0.25, 0.5);
        assert!((doubled - -2.98).abs() < 0.01, "got {doubled}");
    }

    // The guarded wrapper: one same-settings retry heals a transient inject failure;
    // a persistent flat read is reported as StillFlat.
    #[test]
    fn guarded_measure_retries_once_then_reports_still_flat() {
        let mut calls = 0;
        let out = measure_floor_guarded(
            || {
                calls += 1;
                Ok(lively())
            },
            STIM,
            Duration::ZERO,
        )
        .unwrap();
        assert!(matches!(out, GuardOutcome::Live(_)));
        assert_eq!(calls, 1);

        let mut calls = 0;
        let out = measure_floor_guarded(
            || {
                calls += 1;
                Ok(if calls == 1 { flat() } else { lively() })
            },
            STIM,
            Duration::ZERO,
        )
        .unwrap();
        assert!(matches!(out, GuardOutcome::Live(_)));
        assert_eq!(calls, 2);

        let mut calls = 0;
        let out = measure_floor_guarded(
            || {
                calls += 1;
                Ok(flat())
            },
            STIM,
            Duration::ZERO,
        )
        .unwrap();
        assert!(matches!(out, GuardOutcome::StillFlat(_)));
        assert_eq!(calls, 2);

        // A stimulus with no momentary dynamics disarms the guard: a flat capture passes.
        let mut calls = 0;
        let out = measure_floor_guarded(
            || {
                calls += 1;
                Ok(flat())
            },
            1.0,
            Duration::ZERO,
        )
        .unwrap();
        assert!(matches!(out, GuardOutcome::Live(_)));
        assert_eq!(calls, 1);
    }

    // BUG→GATE (#241): a gate-like capture with WIDER momentary dynamics than the
    // stimulus is a live read, measured once. The old "a chain cannot expand dynamics"
    // retry is gone (gates, expanders, swells and tremolo all expand).
    #[test]
    fn momentary_expansion_is_a_live_read_with_no_retry() {
        let mut calls = 0;
        let out = measure_floor_guarded(
            || {
                calls += 1;
                Ok(loud(-24.0, 0.9, 12.0))
            },
            STIM,
            Duration::ZERO,
        )
        .unwrap();
        assert!(matches!(out, GuardOutcome::Live(_)));
        assert_eq!(calls, 1);
    }

    // BUG→GATE (#175): `is_engaged` used the short-term spread (> 0.5 LU), which a healthy
    // capture of the stationary leveling stimulus never reaches, so the external judge
    // stamped `floor_suspect` on every row. A healthy capture of each shipped stimulus at
    // leveling loudness must read engaged.
    #[test]
    fn a_healthy_capture_of_every_shipped_stimulus_reads_engaged() {
        for (name, stim) in shipped_stimuli() {
            let l = lufs::measure_mono(&at_lufs(&stim, -23.0), RATE).unwrap();
            assert!(
                l.spread_lu() < 0.5,
                "{name}: the old arm would have passed this one ({:.2} LU) — fixture drifted",
                l.spread_lu()
            );
            assert!(is_engaged(&l), "{name}: healthy capture not engaged: {l:?}");
        }
    }

    // BUG→GATE (#239): a flat-topped capture reads LOW, so no lane may solve from it.
    #[test]
    fn a_clipped_capture_is_refused_and_a_hot_transient_is_not() {
        let mut clipped = loud(-12.0, 0.2, 5.0);
        clipped.clipped = true;
        assert_eq!(loudness_verdict(clipped).unwrap_err(), LevelError::Clipped);
        assert_eq!(
            LevelError::Clipped.to_string(),
            crate::error::CLIPPED_CAPTURE_ERR
        );
        assert!(loudness_verdict(loud(-12.0, 0.2, 5.0)).is_ok());
        assert_eq!(
            loudness_verdict(loud(f64::NEG_INFINITY, 0.0, 0.0)).unwrap_err(),
            LevelError::NoSignal
        );
        // The detector itself, on a real stereo capture shape: a 3-sample apex on one channel
        // is a transient; a 4-sample run pinned at full scale is clipping.
        let mut pair = vec![0.1f32; 2_000];
        for i in 0..3 {
            pair[100 + 2 * i] = 1.0;
        }
        assert!(!lufs::is_clipped(&pair, 2));
        pair[106] = 1.0;
        assert!(lufs::is_clipped(&pair, 2));
        // Interleaving must not join two channels' samples into one run.
        // (Four consecutive interleaved samples at full scale = two per channel.)
        let mut alt = vec![0.1f32; 2_000];
        alt[200..204].fill(1.0);
        assert!(!lufs::is_clipped(&alt, 2));
    }

    #[test]
    fn cross_check_level_moves_three_db_and_stays_in_range() {
        assert!((cross_check_level(0.5) - 0.35).abs() < 1e-6);
        assert!((cross_check_level(1.0) - 0.7).abs() < 1e-6);
        assert!((cross_check_level(0.06) - 0.084).abs() < 1e-6);
    }

    #[test]
    fn median3_ignores_the_one_outlier() {
        assert_eq!(median3(-20.0, -22.9, -20.1), -20.1);
        assert_eq!(median3(-22.9, -20.0, -20.1), -20.1);
        assert_eq!(median3(-20.1, -20.0, -22.9), -20.1);
    }

    // #243: the base lane measures the preset at its SAVED level, clamped away from the
    // floor; with no saved level it keeps the caller's reference.
    #[test]
    fn base_ref_level_uses_the_saved_level() {
        assert_eq!(base_ref_level(0.5, Some(0.42)), 0.42);
        assert_eq!(base_ref_level(0.5, Some(0.02)), SAVED_REF_MIN);
        assert_eq!(base_ref_level(0.5, Some(1.7)), 1.0);
        assert_eq!(base_ref_level(0.5, None), 0.5);
        assert_eq!(base_ref_level(0.5, Some(0.0)), 0.5);
        assert_eq!(base_ref_level(0.5, Some(f32::NAN)), 0.5);
    }

    #[test]
    fn verification_never_reports_a_failed_verify_as_measured() {
        assert_eq!(Verification::of(true, Some(-20.0)), Verification::Verified);
        assert_eq!(Verification::of(true, None), Verification::VerifyFailed);
        assert_eq!(Verification::of(false, None), Verification::Predicted);
    }
}
