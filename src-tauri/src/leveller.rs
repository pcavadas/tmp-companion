//! Preset leveling — derived from the live de-risk against real hardware.
//!
//! Findings that shape this (all confirmed on-device):
//!   1. The re-amp USB-Out tap reflects `presetLevel`, but ONLY the value that
//!      was set BEFORE re-amp engaged — re-amp latches preset state at engage.
//!   2. Re-amp engages reliably only ONCE per connection (fresh connect →
//!      single toggle). Repeated toggling within a session is unreliable.
//!   3. `set_preset_level` is IGNORED when it immediately follows `load_preset`
//!      in the SAME connection — the load's own level-apply overrides our set.
//!      A no-load `set_preset_level` (on the already-current preset) sticks.
//!      → load the preset in its own connection, then measure/set on FRESH
//!      connections. The device keeps the loaded preset "current" across USB
//!      reconnects, so the no-load set targets the right preset.
//!   4. `presetLevel` is a LINEAR amplitude control:
//!      `captured_LUFS = 20·log10(presetLevel) + C`,
//!      where `C` folds the preset's inherent processed loudness + stimulus
//!      level + the fixed re-amp tap gain. Verified to ~0.2 LU across 0.1–0.9.
//!
//! So leveling is one-shot/open-loop: measure once at a reference level, solve
//! for `C`, compute the exact `presetLevel` that hits the target, set it, save.

use std::time::Duration;

use serde::Serialize;

use crate::audio;
use crate::lufs;
use crate::session::Session;

// Post-load DSP settle before a capture. Was a conservative 1200; HW-bisected to 400
// on fw 1.8.45 (dry slot 11 + wet delay slot 5): measured C, presetLevel, and verify
// error are byte-identical to 1200, and the verify captures confirm writes also land
// at 400 (`notes/perf.md`). TMP_SETTLE_AFTER_LOAD_MS is the diagnostic env override
// for future bisects.
const SETTLE_AFTER_LOAD_MS: u64 = 400;
pub(crate) fn settle_after_load_ms() -> u64 {
    std::env::var("TMP_SETTLE_AFTER_LOAD_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(SETTLE_AFTER_LOAD_MS)
}
// Settle after a reload when the next step is a PURE WRITE (no verify capture).
// DELIBERATELY kept above SETTLE_AFTER_LOAD_MS: on this branch a settle-caused
// dropped write is maximally silent (no verify capture follows; the save persists
// the old value while the result reports success), so the 200 ms it could save
// isn't worth the risk class — see the scene-write-cliff history.
const SETTLE_BEFORE_WRITE_MS: u64 = 600;
pub(crate) const SETTLE_AFTER_SET_MS: u64 = 300;
const SETTLE_AFTER_REAMP_MS: u64 = 500;
/// Inter-session HID gap: let the IOKit seize release before the next open. The
/// HW-proven safe open-after-close gap within the lockout window (`lib.rs`'s scene
/// prepass→one-shot handoff reuses it). `pub(crate)` so that single shared value /
/// rationale isn't duplicated as a magic number elsewhere.
pub(crate) const RECONNECT_GAP_MS: u64 = 400;
const CAPTURE_TAIL_MS: u64 = 800;
/// Doctor-only capture tail: Doctor diagnostic captures (reverb/delay wash analysis)
/// keep a longer post-stimulus tail than the leveling capture, whose 800 ms tail is
/// HW-baselined and load-bearing (see `CAPTURE_TAIL_MS`) and must NOT change.
/// 1.5 s (down from the original 2.5 s) was HW-A/B'd against the 6 s + 2.5 s
/// full-capture oracle (`probe --doctor-window-ab`, 2026-07-16, 6 diverse presets
/// incl. two wet ones): 0 verdict flips, Δtilt ≤ 0.08 dB/oct, band deltas within
/// wash-preset run variance, `washed` still fires on both wet presets.
pub const DOCTOR_TAIL_MS: u32 = 1500;
/// Doctor stimulus window: diagnosis captures re-amp only the first 3 s of the
/// stimulus ([`doctor_stim_slice`]) — the leveling window (full stimulus + its
/// 800 ms tail) is UNTOUCHED per the capture-window lesson (a window change is a
/// re-baseline, validated only against the full-capture oracle). Same
/// `--doctor-window-ab` evidence as [`DOCTOR_TAIL_MS`]; the 4 s fallback showed
/// no better fidelity. Spectral balance (not absolute loudness) is the Doctor's
/// measurement, so the delay/reverb-buildup LUFS shift that forbids trimming the
/// LEVELING window does not apply here.
pub const DOCTOR_STIM_MS: usize = 3000;

/// [`DOCTOR_STIM_MS`] at the device clock ([`RATE`]), in samples.
pub fn doctor_stim_samples() -> usize {
    (RATE as usize / 1000) * DOCTOR_STIM_MS
}

/// Silent preamble prepended to the Doctor stimulus: the true inject latency is
/// only ~32 ms (HW, `audio::estimate_onset` across 15 captures), which leaves a
/// pre-onset noise-floor window too short for a full Welch segment
/// (`psd::SEG` = 8192 ≈ 171 ms) — the output-SNR coverage gate needs a stable
/// floor estimate. 200 ms of played silence stretches the floor window to
/// ~230 ms at the cost of 200 ms per capture. Spectrally neutral: LUFS gating
/// drops silence, and the body PSD starts at [`doctor_signal_start`].
pub const DOCTOR_PAD_MS: usize = 200;

/// [`DOCTOR_PAD_MS`] at the device clock, in samples.
pub fn doctor_pad_samples() -> usize {
    (RATE as usize / 1000) * DOCTOR_PAD_MS
}

/// The Doctor's stimulus window: the first [`DOCTOR_STIM_MS`] of the source,
/// behind [`DOCTOR_PAD_MS`] of leading silence — one home so capture, onset
/// alignment, floor-guard spread, and `stimulus_samples` all agree on the same
/// window. Takes the freshly-read buffer by value and edits in place (every
/// caller owns a throwaway full read; a borrow form just forced a second
/// allocation).
pub fn doctor_stim_slice(mut stim: Vec<f32>) -> Vec<f32> {
    stim.truncate(doctor_stim_samples());
    stim.splice(0..0, std::iter::repeat_n(0.0, doctor_pad_samples()));
    stim
}

/// Where the SIGNAL actually starts in a Doctor capture: the estimated onset is
/// where the PADDED stimulus aligns (= the inject latency), so the played
/// silence sits at `[onset, onset + pad)` and real signal begins after it. The
/// body PSD and the coverage gate's floor/body split use THIS; the tail split
/// keeps the raw `onset` (it adds the padded `stimulus_samples`, which already
/// contains the pad). An unconfident onset keeps the legacy whole-buffer 0.
pub fn doctor_signal_start(onset: usize, confident: bool) -> usize {
    if confident {
        onset + doctor_pad_samples()
    } else {
        0
    }
}

/// Doctor capture tail for a chain WITHOUT a time effect (no reverb/delay node,
/// [`crate::doctor::has_time_effect`]): a bare settle guard, not a wash window —
/// `washed` cannot fire without a time-based block in the chain, so the full tail
/// buys nothing there. Shrinking the tail also shrinks `tail_ratio_db`'s window
/// (an empty/near-empty tail floors it at −80, `doctor::tail_energy_ratio`) and
/// marginally shifts `spread_lu` on these dry captures — expected and harmless
/// since `washed` is inapplicable by construction; the R4/R5 hardware sweeps
/// re-baseline the OTHER thresholds against this shorter recipe.
pub const DOCTOR_TAIL_DRY_MS: u32 = 300;
// Scene mode (`SetNodeSceneEdit`) MUST be enabled before the value write, or the
// write hits the BASE/global value and leaks across scenes (HW). But the settle must
// stay SHORT: the device accepts the scene write only within ~700–750 ms of the
// `loadScene` recall (HW-bisected, `probe --bisect-scene` on fw 1.8.45 —
// load_scene→300→edit→400→write lands; …→450→write is SILENTLY DROPPED, no
// presetError, nothing persists). The old "generous" 600 ms put every production
// scene write past that cliff — 100% dropped — which surfaced as false "clamped at
// (as-is)" rows and zero persistence while every 300 ms probe path worked. The rare
// leak-to-base race a longer settle targeted is covered by the verify + correction
// pass instead. Keep load_scene→edit→write gaps ≤300 ms.
const SETTLE_AFTER_SCENE_EDIT_MS: u64 = 300;
// Gap between the `loadScene` recall and `SetNodeSceneEdit` in a scene write. 150 (not
// the general 300 `SETTLE_AFTER_SET_MS`) CENTERS the value write ~450 ms after
// `loadScene` — the old 300+300 put it at ~600–650 ms nominal, riding the ~700–750 ms
// silent-drop cliff above, so command-latency jitter occasionally pushed a production
// write over it (the user-reported non-deterministic false clamp; the write itself is
// fire-and-forget, so nothing surfaced). HW-bisected lower edge (`probe
// --bisect-scene`, fw 1.8.45): scene_settle 150, 100, and even 50 all land ON the
// scene overlay (never leak to base) and persist — 150 keeps ~2× margin on both sides.
const SETTLE_AFTER_SCENE_RECALL_MS: u64 = 150;
const RATE: u32 = 48_000;
const LEVEL_MIN: f32 = 0.0;
const LEVEL_MAX: f32 = 1.0;
/// `loudest_loudness`'s sentinel error text for a capture with no measurable signal — shared
/// so producer and consumers can't drift.
const NO_SIGNAL_CAPTURED: &str = "no signal captured";

#[derive(Debug, Clone, Serialize)]
pub struct LevelResult {
    pub slot: u32,
    pub ref_level: f32,
    /// Captured integrated LUFS measured at `ref_level`.
    pub measured_lufs: f64,
    /// Solved constant `C` in `LUFS = 20·log10(level) + C` (= max reachable LUFS).
    pub constant_c: f64,
    /// presetLevel computed to hit the target (clamped 0..1).
    pub final_level: f32,
    pub target_lufs: f64,
    /// Predicted captured LUFS at `final_level` (== target unless clamped).
    pub predicted_lufs: f64,
    /// True if the target needed level outside [0,1] (unreachable — clamped).
    pub clamped: bool,
    /// Whether `final_level` was persisted to the preset (SaveCurrentPreset).
    pub saved: bool,
    /// Independent re-measure at `final_level` on a fresh capture (None if skipped).
    pub verify_lufs: Option<f64>,
    /// Number of capture iterations the solve used (1 = one-shot presetLevel
    /// path; 2..=N for the closed-loop block-knob path).
    pub iterations: u32,
    /// Short-term-max − integrated of the measure capture (LU), gain-invariant.
    /// Large (≳6 LU) = a dynamic preset whose gated-integrated reading understates
    /// its peaks vs a compressed one — the UI flags it "verify by ear". `None`
    /// when the measuring path has no full-capture meter (live windows).
    pub dynamic_spread_lu: Option<f64>,
    /// When clamped for a SPECIFIC reason (currently "no authority" — the amp's
    /// `outputLevel` doesn't reach the USB 1/2 capture), the UI shows this verbatim
    /// instead of a generic "clamped". `None` for the preset-level path / plain clamp.
    pub clamp_reason: Option<String>,
    /// Best-effort rebalance "verify by ear" flag (lane-mute bleed may have skewed the
    /// equal-solo balance). Distinct from `dynamic_spread_lu`; the UI ORs both.
    pub verify_by_ear: bool,
    /// The preset's saved `presetLevel` BEFORE this run wrote it — the revert
    /// anchor for the Summary's "Restore original". Stamped by the `level_preset`
    /// command (from its base-isolation preset read); `None` when the read failed
    /// or the path doesn't write `presetLevel` (block-knob / scene paths).
    pub previous_level: Option<f32>,
    /// PREDICTED true peak (dBTP) at `final_level`, extrapolated from the reference
    /// capture's measured true peak (see `predicted_true_peak_dbtp`) — an ESTIMATE,
    /// never a re-measurement. Only the one-shot `presetLevel` path (`level_preset`)
    /// sets this; `None` for scene/block/footswitch paths this cycle.
    pub true_peak_dbtp: Option<f64>,
}

#[derive(Clone, Copy)]
pub struct LevelOptions {
    /// Persist `final_level` to the preset after computing it.
    pub save: bool,
    /// Re-measure at `final_level` on a fresh capture to confirm the result.
    pub verify: bool,
    /// Reference level to measure at (the model is solved from this point).
    pub ref_level: f32,
    /// Leave the written values UNSAVED in the device working copy: no save AND no
    /// restore-reload. The scene runners' accumulate-then-single-save mode — unsaved
    /// scene-edit writes survive scene recalls and reconnects, and ONE final
    /// `saveCurrentPreset` persists every accumulated overlay (HW,
    /// `probe --defer-scenes`). Meaningless with `save: true`.
    pub defer: bool,
}

impl Default for LevelOptions {
    fn default() -> Self {
        LevelOptions {
            save: false,
            verify: false,
            ref_level: 0.5,
            defer: false,
        }
    }
}

/// Fresh-connect, set `level` (NO load — the current preset is already the one
/// we want), engage re-amp once, capture, and return the loudest channel's
/// loudness. The one-shot `presetLevel` case of `measure_knob_at`.
fn measure_at_level(
    stimulus: &[f32],
    level: f32,
    force_bypass: &[(String, String, bool)],
) -> Result<lufs::Loudness, String> {
    measure_knob_at(stimulus, &LevelKnob::PresetLevel, level, force_bypass)
}

/// Sentinel error returned when a cooperative cancel flag is observed at a leveling
/// checkpoint. Compared by `restore_after_unsaved_error` (a cancel must restore the
/// stored preset even on the `save=true` path) and treated as a skip by the frontend.
pub const CANCELLED: &str = "cancelled";

/// Reload the stored preset to discard temporary level edits made while
/// measuring. `save=false` is a preview/read-only contract for callers: the TMP
/// edit buffer may be mutated during capture, but it must not remain dirty.
pub(crate) fn restore_saved_preset(slot: u32) -> Result<(), String> {
    std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
    let mut s = Session::connect_lean()?;
    s.load_preset(slot)?;
    std::thread::sleep(Duration::from_millis(settle_after_load_ms()));
    log::info!("restored stored preset slot={slot} after unsaved measurement");
    Ok(())
}

/// If an unsaved operation fails after touching a level control, try to discard
/// the temporary edit before returning the original error.
fn restore_after_unsaved_error<T>(
    slot: u32,
    save: bool,
    result: Result<T, String>,
) -> Result<T, String> {
    let err = match result {
        Ok(value) => return Ok(value),
        Err(err) => err,
    };
    // Restore the stored preset to discard a dirty edit buffer when nothing was
    // persisted: an unsaved op (save=false), OR a cancel — even with save=true a cancel
    // bails before `apply_level`, leaving `presetLevel` at the measurement reference
    // (`measure_knob_at` sets it and never restores). A non-cancel save error keeps the
    // prior pass-through behavior (no reload).
    if save && err != CANCELLED {
        return Err(err);
    }
    Err(append_restore_err(err, restore_saved_preset(slot)))
}

/// Fold a restore failure into a primary error — the ONE wording for the
/// "primary error + edit-buffer restore failure" merge every restore-after-
/// failure path shares (here, `probe_api::doctor_inject`/`doctor_defects`).
pub(crate) fn append_restore_err(primary: String, restore: Result<(), String>) -> String {
    match restore {
        Ok(()) => primary,
        Err(r) => format!("{primary}; also failed to restore stored preset: {r}"),
    }
}

// ───────────────────────── Floor-read guard ─────────────────────────
//
// A silent/failed re-amp inject captures the device's STATIONARY OUTPUT FLOOR, which
// is finite LUFS and solves into a plausible-looking level (HW: 19/20 floor reads in
// one sweep; a full solve+verify landed 0.00 LU error on pure floor). The tell is the
// dynamics spread: a plucked stimulus through ANY chain measures spread ≫ 0 (real
// library minimum 0.12 LU), the floor ≈ 0.01. The guard is stimulus-aware — an
// EBow-heavy calibration capture is near-stationary by design, so the spread trip is
// DISARMED when the stimulus itself is flat; discrimination then rests on the
// level-shift confirm (`presetLevel` is linear post-chain gain: real signal tracks
// 20·log10, the floor doesn't).

/// Trip gate: capture spread at or below this suspects a floor read. Set BELOW the
/// measured real-preset minimum (0.12 LU; floor reads ≈ 0.01).
pub(crate) const FLOOR_TRIP_LU: f64 = 0.08;
/// A stimulus with spread at or below this can't discriminate by spread — skip the trip.
pub(crate) const STATIONARY_STIM_LU: f64 = 0.30;
/// |Δmeasured − Δexpected| tolerance for the level-shift confirm (absorbs the ~0.12 LU
/// run-to-run noise with wide margin on a 6.02 LU expected shift).
pub(crate) const FLOOR_CONFIRM_TOL_LU: f64 = 2.0;
/// Quiet gap before the guard's retry — 5 s recovered 9/9 flagged rows on HW
/// (`probe --stim-ab`); revisit against `RECONNECT_GAP_MS` pacing if lockouts appear.
pub(crate) const FLOOR_RETRY_GAP_MS: u64 = 5_000;
/// The honest per-item error when a floor read persists through retry + confirm.
pub(crate) const FLOOR_READ_ERR: &str = "no stimulus reached the device (captured only \
    the output floor) — check the USB audio connection and try again";

/// Should this capture be suspected as a floor read?
pub(crate) fn floor_suspect(capture_spread_lu: f64, stimulus_spread_lu: f64) -> bool {
    stimulus_spread_lu > STATIONARY_STIM_LU && capture_spread_lu <= FLOOR_TRIP_LU
}

/// Did the capture track a `presetLevel` shift by 20·log10 (real signal), or stay
/// put (floor)?
pub(crate) fn tracks_level_shift(
    measured_ref_lufs: f64,
    measured_confirm_lufs: f64,
    ref_level: f32,
    confirm_level: f32,
) -> bool {
    let expected = 20.0 * (confirm_level as f64 / ref_level as f64).log10();
    ((measured_confirm_lufs - measured_ref_lufs) - expected).abs() <= FLOOR_CONFIRM_TOL_LU
}

/// The confirm probe's level: halve the reference, unless halving would hit the 0.05
/// clamp — then double (the shift must stay distinguishable from noise either way).
pub(crate) fn confirm_ref_level(ref_level: f32) -> f32 {
    if ref_level / 2.0 >= 0.05 {
        ref_level / 2.0
    } else {
        (ref_level * 2.0).min(1.0)
    }
}

/// A floor-guarded measurement's outcome. `StillFlat` carries the retry's loudness —
/// callers decide the escalation (scene paths error with [`FLOOR_READ_ERR`];
/// `measure_c` escalates to the level-shift confirm to clear ultra-compressed presets).
pub(crate) enum GuardOutcome {
    Live(lufs::Loudness),
    StillFlat(lufs::Loudness),
}

/// Run `measure`; if the capture is floor-suspect, wait `gap` and retry ONCE with the
/// same settings (heals a transient inject failure). A persistently flat capture is
/// reported, not swallowed. The measurement's own spread stays advisory elsewhere.
pub(crate) fn measure_floor_guarded(
    mut measure: impl FnMut() -> Result<lufs::Loudness, String>,
    stimulus_spread_lu: f64,
    gap: Duration,
) -> Result<GuardOutcome, String> {
    let first = measure()?;
    if !floor_suspect(first.spread_lu(), stimulus_spread_lu) {
        return Ok(GuardOutcome::Live(first));
    }
    log::warn!(
        "floor guard: capture spread {:.2} LU ≤ {FLOOR_TRIP_LU} — suspected silent inject, retrying once",
        first.spread_lu()
    );
    std::thread::sleep(gap);
    let second = measure()?;
    if floor_suspect(second.spread_lu(), stimulus_spread_lu) {
        Ok(GuardOutcome::StillFlat(second))
    } else {
        Ok(GuardOutcome::Live(second))
    }
}

/// The common call-site shape: guard `measure`, collapse a persistent flat read to
/// the honest [`FLOOR_READ_ERR`]. For paths with no better escalation than an error
/// (scene/knob/solo measurements); `measure_c` keeps its own match — it escalates
/// `StillFlat` to the level-shift confirm instead (rescuing ultra-compressed presets).
pub(crate) fn require_live(
    measure: impl FnMut() -> Result<lufs::Loudness, String>,
    stimulus: &[f32],
) -> Result<lufs::Loudness, String> {
    match measure_floor_guarded(
        measure,
        stimulus_spread_lu(stimulus),
        Duration::from_millis(FLOOR_RETRY_GAP_MS),
    )? {
        GuardOutcome::Live(l) => Ok(l),
        GuardOutcome::StillFlat(_) => Err(FLOOR_READ_ERR.to_string()),
    }
}

/// The stimulus's own dynamics spread (arms the floor guard). A measurement failure
/// DISARMS the guard (returns 0.0) — never turn a metering hiccup into false floor
/// errors; floor reads then pass exactly as they did before the guard existed.
pub(crate) fn stimulus_spread_lu(stimulus: &[f32]) -> f64 {
    match lufs::measure_mono(stimulus, RATE) {
        Ok(l) => l.spread_lu(),
        Err(e) => {
            log::warn!("floor guard disarmed: stimulus spread unmeasurable ({e})");
            0.0
        }
    }
}

/// What one reference capture yields: the loudness reading, the solved model
/// constant, and the capture's dynamics spread (see `LevelResult::dynamic_spread_lu`).
#[derive(Debug, Clone, Copy)]
pub struct MeasuredC {
    /// Captured integrated LUFS at the reference level.
    pub measured_lufs: f64,
    /// Solved `C` in `LUFS = 20·log10(level) + C` (= max reachable LUFS).
    pub c: f64,
    /// Short-term-max − integrated of the same capture (LU).
    pub dynamic_spread_lu: f64,
    /// True peak (dBTP) of the reference capture — the basis for the one-shot
    /// path's PREDICTED true peak at the solved level (see `predicted_true_peak_dbtp`).
    pub true_peak_dbtp: f64,
}

/// Conn 1+2 seam: load `slot` (own connection, since set-after-load is overridden
/// in-connection), then measure its captured loudness at `ref_level` on a fresh
/// connection, and solve `C` in `LUFS = 20·log10(level) + C`. `C` is the preset's
/// max reachable captured loudness.
pub fn measure_c(
    slot: u32,
    stimulus: &[f32],
    ref_level: f32,
    force_bypass: &[(String, String, bool)],
) -> Result<MeasuredC, String> {
    let ref_level = ref_level.clamp(0.05, 1.0);
    {
        let mut s = Session::connect_lean()?;
        s.load_preset(slot)?;
        std::thread::sleep(Duration::from_millis(settle_after_load_ms()));
    }
    std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
    let gap = Duration::from_millis(FLOOR_RETRY_GAP_MS);
    // No load → the set inside measure_at_level sticks on the now-current preset.
    let outcome = measure_floor_guarded(
        || measure_at_level(stimulus, ref_level, force_bypass),
        stimulus_spread_lu(stimulus),
        gap,
    )?;
    let loudness = match outcome {
        GuardOutcome::Live(l) => l,
        // Persistently flat: ultra-compressed real signal vs floor — the level-shift
        // confirm decides (real output tracks 20·log10(presetLevel); the floor doesn't).
        GuardOutcome::StillFlat(l) => {
            let confirm_level = confirm_ref_level(ref_level);
            std::thread::sleep(gap);
            let confirm = measure_at_level(stimulus, confirm_level, force_bypass)?;
            if tracks_level_shift(
                l.integrated_lufs,
                confirm.integrated_lufs,
                ref_level,
                confirm_level,
            ) {
                log::info!(
                    "floor guard: slot={slot} tracked the level shift — ultra-compressed but real"
                );
                l
            } else {
                return Err(FLOOR_READ_ERR.to_string());
            }
        }
    };
    let c = loudness.integrated_lufs - 20.0 * (ref_level as f64).log10();
    Ok(MeasuredC {
        measured_lufs: loudness.integrated_lufs,
        c,
        dynamic_spread_lu: loudness.spread_lu(),
        true_peak_dbtp: loudness.true_peak_dbtp,
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
/// its own connection, settle, drop; fresh-connect → (when `scene` is `Some`)
/// re-activate that 0-based `scenes[]` wire index ON THE CAPTURE CONNECTION → set
/// the reference level → engage re-amp once → `audio::reamp_capture(.., tail_ms)`
/// → guaranteed re-amp off.
///
/// The scene MUST be loaded on the capture connection, not the load connection:
/// the preset survives the load→capture reconnect but **the active scene does
/// not** (see `set_knob`'s "scene + scene-edit don't survive the leveller's
/// reconnects" — HW). Loading it only in the dropped load connection measured
/// whatever scene the unit was already on, so every scene read the same signal.
/// `scene` is `None` and `tail_ms` is `CAPTURE_TAIL_MS` for every existing
/// `capture_full` call, so that path is byte-identical to before this extraction.
/// `skip_load`: omit the load connection entirely — ONLY when the caller knows the
/// preset is already current AND unpolluted (same preset, previous capture made no
/// working-copy writes; the Doctor's consecutive-scene chain). The preset working
/// copy survives reconnects (HW-proven); the scene recall below re-materializes the
/// scene state on the capture connection either way.
fn capture_full_at(
    slot: u32,
    scene: Option<u32>,
    force_bypass: &[(String, String, bool)],
    stimulus: &[f32],
    ref_level: Option<f32>,
    tail_ms: u64,
    skip_load: bool,
) -> Result<audio::Capture, String> {
    if !skip_load {
        let mut s = Session::connect_lean()?;
        s.load_preset(slot)?;
        std::thread::sleep(Duration::from_millis(settle_after_load_ms()));
        drop(s);
        std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
    }
    let mut s = Session::connect_lean()?;
    // Re-assert the scene on THIS (capture) connection before setting the level —
    // load_scene recalls the scene's own state, so it must precede the reference-
    // level write, not follow it.
    if let Some(scene) = scene {
        s.load_scene(scene)?;
        std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SET_MS));
    }
    // Force-bypass isolation AFTER the scene recall, BEFORE the presetLevel set +
    // engage (the `measure_knob_at` ordering): a scene load would re-assert the
    // scene's own bypass state, so isolation must land after it.
    for (g, n, byp) in force_bypass {
        s.change_parameter_bool(g, n, "bypass", *byp)?;
    }
    // `None` = capture at the preset's OWN stored level (Doctor's apply A/B),
    // leaving the edit buffer's presetLevel untouched.
    if let Some(ref_level) = ref_level {
        set_knob(&mut s, &LevelKnob::PresetLevel, ref_level.clamp(0.05, 1.0))?;
        std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SET_MS));
    }
    let _ = s.set_reamp_mode(true)?;
    std::thread::sleep(Duration::from_millis(SETTLE_AFTER_REAMP_MS));
    let cap = audio::reamp_capture(stimulus, RATE, tail_ms);
    let _ = s.set_reamp_mode(false);
    cap
}

/// MEASURE seam returning the FULL multi-channel capture: load `slot`, re-amp
/// `stimulus` at `ref_level`, return every captured channel. Validated own-conn
/// load → fresh-connect set → engage re-amp once → capture → off. `capture_samples`
/// and the per-channel N1 diagnostic (`probe --channels`) share this.
pub fn capture_full(slot: u32, stimulus: &[f32], ref_level: f32) -> Result<audio::Capture, String> {
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

/// MEASURE seam for analysis (spectrum / audit): load `slot`, re-amp the
/// `stimulus` at `ref_level`, and return the loudest captured channel's raw samples +
/// rate (for FFT / band analysis). Mirrors the validated `measure_c` + `measure_knob_at`
/// sequence (own-connection load → fresh-connect set → engage re-amp once → capture).
pub fn capture_samples(
    slot: u32,
    stimulus: &[f32],
    ref_level: f32,
) -> Result<(Vec<f32>, u32), String> {
    let cap = capture_full(slot, stimulus, ref_level)?;
    let (ch, _) = cap.loudest_channel();
    Ok((cap.channel(ch), cap.sample_rate))
}

/// Doctor-only MEASURE seam: like `capture_samples`, but optionally activates a scene
/// first (0-based `scenes[]` wire index, `None` = base) and captures with a
/// caller-chosen tail (`tail_ms` below). Shares `capture_full_at`
/// with the leveling capture path — the leveling window/timings are untouched.
/// `ref_level`: `Some(0.5)` for the diagnosis run (measurement SNR); `None` for
/// the apply A/B (capture at the preset's own level — never writes presetLevel,
/// so a later `doctor_save` can't persist a reference level).
/// `skip_load`: see `capture_full_at` — the Doctor's consecutive-scene chain skips
/// the redundant per-sound preset reload (same preset, previous sound clean + Ok).
/// `tail_ms`: the caller picks — `DOCTOR_TAIL_MS` for a chain that may wash, else
/// the shorter `DOCTOR_TAIL_DRY_MS` (`doctor::has_time_effect` decides).
/// Mixes down via `Capture::stereo_mix` (deterministic average of USB-Out 1/2),
/// not the leveling path's argmax `loudest_channel` — on a stereo preset (ping-
/// pong delay, hard-panned dual amps) the argmax can flip L/R across runs and
/// flip spectral verdicts with it.
pub fn doctor_capture(
    slot: u32,
    scene: Option<u32>,
    force_bypass: &[(String, String, bool)],
    stimulus: &[f32],
    ref_level: Option<f32>,
    tail_ms: u64,
    skip_load: bool,
) -> Result<(Vec<f32>, u32), String> {
    let cap = capture_full_at(
        slot,
        scene,
        force_bypass,
        stimulus,
        ref_level,
        tail_ms,
        skip_load,
    )?;
    let sr = cap.sample_rate;
    Ok((cap.stereo_mix(), sr))
}

/// Doctor A/B AFTER-clip seam: capture the CURRENT live edit-buffer state WITHOUT
/// loading — a load would discard the unsaved `doctor_apply` prescription edit.
/// Delegates to `capture_full_at` with `skip_load: true` (its non-load branch is
/// byte-for-byte this: fresh-connect → (when `scene` is `Some`) re-activate that
/// 0-based `scenes[]` wire index on THIS connection → write `force_bypass`
/// isolation → optionally set the reference level BEFORE engaging → engage
/// re-amp once → capture with the Doctor tail → guaranteed re-amp off), plus
/// the leading `RECONNECT_GAP_MS` gap `capture_full_at`'s own load branch would
/// otherwise supply. Deterministic stereo mixdown (`Capture::stereo_mix`, not
/// the leveling path's argmax `loudest_channel` — see `doctor_capture`'s doc
/// for why). The scene recall + force-bypass writes land on the UNSAVED edit
/// buffer ON PURPOSE: `doctor_save` never persists this live buffer (it
/// rebuilds SAVED+ops from scratch, see `commands/doctor.rs::doctor_save`), so
/// a forced bypass or scene recall made here can never leak into a save —
/// `doctor_discard`'s reload clears them either way. `ref_level` MUST match
/// the before-capture's so the A/B is level-fair (`doctor_apply` passes `None`
/// to both: the preset's own level, never a presetLevel write). `scene`/
/// `force_bypass`/`tail_ms`: see `doctor_capture` — the AFTER capture must be
/// taken under the SAME diagnosed context (scene + isolation) as the BEFORE.
pub fn doctor_capture_current(
    stimulus: &[f32],
    scene: Option<u32>,
    force_bypass: &[(String, String, bool)],
    ref_level: Option<f32>,
    tail_ms: u64,
) -> Result<(Vec<f32>, u32), String> {
    std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
    let cap = capture_full_at(
        0, // slot unused: skip_load
        scene,
        force_bypass,
        stimulus,
        ref_level,
        tail_ms,
        true,
    )?;
    let sr = cap.sample_rate;
    Ok((cap.stereo_mix(), sr))
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
    slot: u32,
    scene_count: u32,
    stimulus: &[f32],
) -> Result<Vec<f64>, String> {
    let mut cs = Vec::with_capacity(scene_count as usize);
    // Scenes are 0-based `scenes[]` indices on the wire (base is the constant slot 8)
    // — HW-proven by the `--loadscene 1` → scenes[1] activegraph diff. Slot 0 IS
    // addressable because `proto::load_scene` now emits the field explicitly even
    // for 0 (the device ignores an empty LoadScene{} — HW-found).
    for scene in 0..scene_count {
        {
            let mut s = Session::connect_lean()?;
            s.load_preset(slot)?;
            std::thread::sleep(Duration::from_millis(settle_after_load_ms()));
        }
        std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
        let mut s = Session::connect_lean()?;
        s.load_scene(scene)?;
        std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SET_MS));
        set_knob(&mut s, &LevelKnob::PresetLevel, 1.0)?;
        std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SET_MS));
        cs.push(engage_measure_disengage(&mut s, stimulus)?.integrated_lufs);
    }
    Ok(cs)
}

/// The common leveling target for a set of preset ceilings `cs`: the
/// loudest level *every* preset can still reach, with `headroom_lu` of margin —
/// `min(C) − headroom`. `None` if `cs` is empty. Leveling all presets to this target
/// means no loudness jump when switching presets/instruments on stage.
pub fn common_target(cs: &[f64], headroom_lu: f64) -> Option<f64> {
    // Ignore non-finite ceilings (a failed/silent capture yields NaN); an all-NaN
    // slice returns None so the caller errors out rather than solving against NaN and
    // writing a garbage presetLevel.
    cs.iter()
        .copied()
        .filter(|c| c.is_finite())
        .reduce(f64::min)
        .map(|min_c| min_c - headroom_lu)
}

/// Amp `outputLevel` a redistribution-compensated knob must stay above — never write a
/// compensating value toward deep digital silence (`outputLevel = 0` reads as silence).
pub const REDIST_MIN_KNOB: f32 = 0.05;

/// A hair of extra headroom (dB) added to the redistribution delta beyond the worst clamped
/// scene's deficit, so that scene reaches target with its `outputLevel` a touch BELOW max
/// (genuinely re-solvable → "done") instead of pinned exactly at 1.0 (an edge clamp any
/// measurement jitter tips back over). Capped by the presetLevel headroom / down-room like
/// the deficit itself, so it never over-raises past what the budget allows.
pub const REDIST_HEADROOM_MARGIN_DB: f64 = 1.0;

/// The gain-budget redistribution delta (dB, ≥ 0): raise `presetLevel` by this and
/// compensate the base amp + non-clamped scene overlays DOWN by this, so clamped scenes
/// gain headroom while non-clamped sounds stay on target (net-neutral). = the min of:
///  - `worst_clamped_deficit_db` — the loudest-short clamped scene's `target − achieved`
///    (enough to rescue the worst; lesser-clamped scenes were shorter, so all are rescued);
///  - `presetLevel` headroom `−20·log10(pl)` — can't push `pl` past 1.0;
///  - the down-room before the LOWEST compensated knob would hit [`REDIST_MIN_KNOB`]
///    (`20·log10(min_knob / REDIST_MIN_KNOB)`), so no compensation writes toward silence.
///
/// Returns 0 (⇒ don't offer / no-op) when there's no clamp, no `pl` headroom, or a
/// compensated knob already sits at/below the floor.
pub fn redistribute_delta_db(
    preset_level: f32,
    worst_clamped_deficit_db: f64,
    min_compensated_knob: f32,
) -> f64 {
    if worst_clamped_deficit_db <= 0.0 {
        return 0.0; // no clamp → nothing to redistribute
    }
    let pl_headroom = -20.0 * (preset_level.clamp(1e-6, 1.0) as f64).log10();
    let down_room = if min_compensated_knob > REDIST_MIN_KNOB {
        20.0 * (min_compensated_knob as f64 / REDIST_MIN_KNOB as f64).log10()
    } else {
        0.0
    };
    (worst_clamped_deficit_db + REDIST_HEADROOM_MARGIN_DB)
        .min(pl_headroom)
        .min(down_room)
        .max(0.0)
}

/// Solve the `presetLevel` that hits `target_lufs` given `C`. Returns
/// `(final_level clamped 0..1, clamped, predicted_lufs)`.
pub fn solve_level(c: f64, target_lufs: f64) -> (f32, bool, f64) {
    let ideal = 10f64.powf((target_lufs - c) / 20.0);
    let clamped = ideal > LEVEL_MAX as f64 || ideal < LEVEL_MIN as f64;
    let final_level = (ideal as f32).clamp(LEVEL_MIN, LEVEL_MAX);
    let predicted = 20.0 * (final_level.max(1e-6) as f64).log10() + c;
    (final_level, clamped, predicted)
}

/// Conn 3 seam: set `knob`=`final_level` on a fresh connection, optionally verify
/// (fresh re-amp capture) and save. With `save=false`, reloads the stored preset
/// after verification so the TMP edit buffer does not remain dirty. Returns
/// `(saved, verify_lufs)`.
///
/// `reload_preset` controls whether the preset is re-loaded first: the
/// single-preset and block paths leave it `false` (the preset is still current
/// from the prior load — exactly the validated 3-connection sequence); the setlist
/// path sets it `true` because measuring other presets has since changed which
/// preset is current. The scene runners (`jointk_one_scene`/rebalance) also leave
/// it `false` — their runner loads the preset once up front and nothing between
/// applies changes it; a reload per apply was pure churn the user SAW (the unit
/// flashing back to the preset between every scene write).
pub fn apply_level(
    slot: u32,
    stimulus: &[f32],
    knob: &LevelKnob,
    final_level: f32,
    opts: LevelOptions,
    reload_preset: bool,
) -> Result<(bool, Option<f64>), String> {
    apply_levels(slot, stimulus, &[(knob, final_level)], opts, reload_preset)
}

/// Multi-knob Conn-3 seam: set every `(knob, value)` in `targets` (all belonging to
/// the same scene) on a fresh connection — the joint-k apply for a parallel-merged
/// scene's lane amps — optionally verify (one fresh re-amp capture, latching the
/// whole set) and save. `apply_level` is the one-element case. See `apply_level`'s
/// notes on `reload_preset`.
pub fn apply_levels(
    slot: u32,
    stimulus: &[f32],
    targets: &[(&LevelKnob, f32)],
    opts: LevelOptions,
    reload_preset: bool,
) -> Result<(bool, Option<f64>), String> {
    if reload_preset {
        let mut s = Session::connect()?;
        s.load_preset(slot)?;
        // A verify capture needs the DSP audio fully settled; a pure write does not.
        let settle = if opts.verify {
            settle_after_load_ms()
        } else {
            SETTLE_BEFORE_WRITE_MS
        };
        std::thread::sleep(Duration::from_millis(settle));
    }
    std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));

    let mut verify_lufs = None;
    let mut s = Session::connect()?;
    set_knobs(&mut s, targets)?; // set before any re-amp engage (latched)
    std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SET_MS));

    if opts.verify {
        verify_lufs = engage_measure_disengage(&mut s, stimulus)
            .ok()
            .map(|l| l.integrated_lufs);
        // Re-assert after the re-amp toggle ONLY for PresetLevel targets (the
        // historical motivation). For scene knobs the re-assert is actively harmful:
        // its `load_scene` REVERTS the just-verified unsaved write, and the re-write
        // runs on a post-re-amp session that observably answers nothing (HW,
        // `probe --jointk-scenes` forensics: zero echoed fields) — a dropped re-write
        // would SAVE the reverted value. The verify capture already measured the
        // written state; save persists exactly that.
        if targets
            .iter()
            .all(|(k, _)| matches!(k, LevelKnob::PresetLevel))
        {
            let _ = set_knobs(&mut s, targets);
            std::thread::sleep(Duration::from_millis(150));
        }
    }

    if opts.save {
        if opts.verify {
            // A session that has toggled re-amp silently DROPS the save (HW: after the
            // verify engage/disengage, `saveCurrentPreset` on the same session persists
            // nothing — `probe --bisect-scene … save` with TMP_BISECT_SAVE_MODE=same vs
            // fresh). The written values survive in the device's working copy across
            // reconnects, so save on a FRESH connection.
            drop(s);
            std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
            Session::connect()?.save_current_preset(slot)?;
        } else {
            s.save_current_preset(slot)?;
        }
    } else if opts.defer {
        // Deferred mode: leave the write UNSAVED in the working copy — the scene
        // runner persists every accumulated overlay with ONE save at batch end
        // (`save_deferred_scene_writes`). No restore: a reload would discard it.
        drop(s);
    } else {
        drop(s);
        restore_saved_preset(slot)?;
    }
    Ok((opts.save, verify_lufs))
}

/// Identity check for the Restore write: the preset-list row at `slot` must still
/// carry the display name recorded when the run leveled it. A slot is a position,
/// not an identity — if the list drifted (a move/clear/save-over between the run
/// and the Restore click), writing by slot alone would save the old level onto a
/// DIFFERENT preset. Pure (unit-tested); the caller supplies a fresh list read.
fn verify_slot_name(
    list: &[crate::session::PresetEntry],
    slot: u32,
    expected_name: &str,
) -> Result<(), String> {
    let now = list
        .iter()
        .find(|p| p.slot == slot)
        .map(|p| p.name.as_str())
        .ok_or_else(|| format!("slot {slot} is no longer in the preset list — not restoring"))?;
    if now != expected_name {
        return Err(format!(
            "preset at slot {slot} is now \"{now}\" (expected \"{expected_name}\") — not restoring"
        ));
    }
    Ok(())
}

/// Restore a preset's `presetLevel` to a pre-leveling snapshot value and SAVE —
/// the Summary "Restore original" write. A pure write (no verify capture), so the
/// stimulus is irrelevant; reuses the validated `apply_level` seam (reload → set →
/// save) with an empty stimulus. Slot-keyed destructive write ⇒ the mapping is
/// confirmed with a non-destructive read first ([`verify_slot_name`], the
/// write-safety lesson) so a drifted preset list fails loudly instead of saving
/// the old level onto a different preset.
pub fn restore_preset_level(slot: u32, level: f32, expected_name: &str) -> Result<(), String> {
    {
        let mut s = Session::connect()?;
        let list = s.list_my_presets()?;
        verify_slot_name(&list, slot, expected_name)?;
    }
    std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
    let opts = LevelOptions {
        save: true,
        verify: false,
        ..Default::default()
    };
    apply_level(slot, &[], &LevelKnob::PresetLevel, level, opts, true).map(|_| ())
}

/// One recorded pre-redistribution knob to write back on Restore. `scene_slot` `None` = the
/// base amp (plain `changeParameter`); `Some(i)` = the i-th FS scene overlay (scene-edit).
pub struct PrevKnobWrite {
    pub group_id: String,
    pub node_id: String,
    pub scene_slot: Option<u32>,
    pub value: f32,
}

/// Restore a redistribution: write `preset_level` + every recorded amp `outputLevel` back on
/// ONE live-edit session (base scene recalled before the save — the empty-graph-corruption
/// guard), name-guarded. The reverse of `redistribute_clamped_headroom`'s persisted write —
/// pure writes, NO measurement. `set_knob` does the scene-edit for an overlay knob and a plain
/// write for the base. Slot-keyed destructive write ⇒ a non-destructive name read guards it
/// first, so a drifted list fails loudly instead of restoring onto a different preset.
pub fn restore_redistribution(
    slot: u32,
    preset_level: f32,
    knobs: &[PrevKnobWrite],
    expected_name: &str,
) -> Result<(), String> {
    {
        let mut s = Session::connect()?;
        let list = s.list_my_presets()?;
        verify_slot_name(&list, slot, expected_name)?;
    }
    std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    s.load_preset(slot)?;
    for _ in 0..8 {
        let _ = s.heartbeat();
        let _ = s.pump_collect(150);
    }
    s.set_preset_level(preset_level)?;
    std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SET_MS));
    for k in knobs {
        let knob = LevelKnob::Block {
            group_id: k.group_id.clone(),
            node_id: k.node_id.clone(),
            parameter_id: "outputLevel".to_string(),
            scene_slot: k.scene_slot,
        };
        set_knob(&mut s, &knob, k.value)?;
        let _ = s.heartbeat();
    }
    s.load_scene(crate::session::BASE_SCENE_SLOT)?;
    std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SET_MS));
    s.save_current_preset(slot)
}

/// Is the solved `final_level` the same as the preset's already-saved `previous`
/// level, within the LU-space `KNOB_TOL_LU` band? Deliberately matches
/// `KNOB_TOL_LU` rather than a tighter ratio — a band under the ~0.12 LU measured
/// run-to-run noise would make the skip a coin flip. `previous <= 0.0` (unread or
/// nonsensical) never counts as unchanged.
fn level_unchanged(final_level: f32, previous: f32) -> bool {
    previous > 0.0 && (20.0 * (final_level as f64 / previous as f64).log10()).abs() <= KNOB_TOL_LU
}

/// Level one preset to `target_lufs`. Self-contained: opens its own fresh
/// connections (load → measure → set), so the caller must NOT hold a competing
/// device seize while this runs. Composes the `measure_c` → `solve_level` →
/// `apply_level` seams. `previous_level` (the preset's currently-saved
/// `presetLevel`, when the caller already read it) enables the idempotency skip:
/// a re-run that solves the SAME level as last time reloads the stored preset and
/// returns without writing (see the `level_unchanged` check below), so repeat runs
/// don't re-randomize an already-on-target preset. `None` (the probe/benchmark
/// call sites, and the setlist common-target pass) keeps the always-write behavior.
pub fn level_preset(
    slot: u32,
    stimulus: &[f32],
    target_lufs: f64,
    opts: LevelOptions,
    force_bypass: &[(String, String, bool)],
    previous_level: Option<f32>,
    mut cancelled: impl FnMut() -> bool,
) -> Result<LevelResult, String> {
    // Pre-measure cancel: nothing has touched the device yet, so return WITHOUT the
    // restore wrapper (no needless reload).
    if cancelled() {
        return Err(CANCELLED.to_string());
    }
    let result = (|| {
        let ref_level = opts.ref_level.clamp(0.05, 1.0);
        let m = match measure_c(slot, stimulus, ref_level, force_bypass) {
            Ok(m) => m,
            // Silence == output not routed to USB 1/2 (a routing state, can happen on ANY preset):
            // report the honest "not on USB 1/2" clamp instead of a generic read failure.
            Err(e) if e.contains(NO_SIGNAL_CAPTURED) => {
                // `measure_c` already set `presetLevel`/forced bypasses on the live device;
                // discard that before returning (this is an Ok result, so
                // `restore_after_unsaved_error` below never runs for it).
                let _ = restore_saved_preset(slot);
                return Ok(LevelResult {
                    slot,
                    ref_level,
                    measured_lufs: MUTE_FLOOR_SILENT_LUFS,
                    constant_c: MUTE_FLOOR_SILENT_LUFS,
                    final_level: ref_level,
                    target_lufs,
                    predicted_lufs: MUTE_FLOOR_SILENT_LUFS,
                    clamped: true,
                    saved: false,
                    verify_lufs: None,
                    iterations: 1,
                    dynamic_spread_lu: None,
                    clamp_reason: Some("no signal on USB 1/2".into()),
                    verify_by_ear: false,
                    previous_level: None,
                    true_peak_dbtp: None,
                });
            }
            Err(e) => return Err(e),
        };
        // Post-measure cancel: `measure_c` left `presetLevel` at `ref_level`; bail before
        // the apply+save. The restore wrapper reloads the stored preset (see CANCELLED).
        if cancelled() {
            return Err(CANCELLED.to_string());
        }
        let (final_level, clamped, predicted) = solve_level(m.c, target_lufs);
        // Idempotency skip: the solved level matches what's already saved — reload to
        // discard the measurement's ref-level edit (same recovery as the NO_SIGNAL
        // branch above) and return without writing. `previous_level: None` on the
        // result (not `previous_level`/`Some(p)`) is CRITICAL: the UI's Summary
        // "Restore original" button gates on it, and there is nothing to restore when
        // this run touched nothing.
        if let Some(p) = previous_level {
            if !clamped && level_unchanged(final_level, p) {
                log::info!(
                    "level_preset slot={slot}: solved level within tolerance of saved ({final_level:.4} vs {p:.4}) — skipping write"
                );
                restore_saved_preset(slot)?;
                return Ok(LevelResult {
                    slot,
                    ref_level,
                    measured_lufs: m.measured_lufs,
                    constant_c: m.c,
                    final_level: p,
                    target_lufs,
                    predicted_lufs: predicted,
                    clamped: false,
                    saved: false,
                    verify_lufs: None,
                    iterations: 1,
                    dynamic_spread_lu: Some(m.dynamic_spread_lu),
                    clamp_reason: None,
                    verify_by_ear: false,
                    previous_level: None,
                    true_peak_dbtp: Some(predicted_true_peak_dbtp(
                        m.true_peak_dbtp,
                        ref_level,
                        final_level,
                    )),
                });
            }
        }
        // With forced footswitch bypasses, the device edit buffer is dirty (bypasses persist
        // across HID reconnects), so `apply_level` must reload FIRST to reset it before setting
        // only `presetLevel` and saving. And skip verify: its capture runs AFTER that reload, so
        // it would measure the un-isolated (Base + all FS blocks) state — a misleading number, and
        // re-forcing there would risk persisting the bypasses. The solve already used the
        // correctly-isolated measure_c, and the UI falls back to `predicted_lufs`.
        let mut apply_opts = opts;
        if !force_bypass.is_empty() {
            apply_opts.verify = false;
        }
        let (saved, verify_lufs) = apply_level(
            slot,
            stimulus,
            &LevelKnob::PresetLevel,
            final_level,
            apply_opts,
            !force_bypass.is_empty(),
        )?;

        Ok(LevelResult {
            slot,
            ref_level,
            measured_lufs: m.measured_lufs,
            constant_c: m.c,
            final_level,
            target_lufs,
            predicted_lufs: predicted,
            clamped,
            saved,
            verify_lufs,
            iterations: 1,
            dynamic_spread_lu: Some(m.dynamic_spread_lu),
            clamp_reason: None,
            verify_by_ear: false,
            previous_level: None,
            true_peak_dbtp: Some(predicted_true_peak_dbtp(
                m.true_peak_dbtp,
                ref_level,
                final_level,
            )),
        })
    })();
    restore_after_unsaved_error(slot, opts.save, result)
}

/// One entry in a setlist leveling pass: the preset slot + its already-loaded
/// instrument stimulus.
pub struct SetlistEntry<'a> {
    pub slot: u32,
    pub stimulus: &'a [f32],
    /// Fletcher–Munson playback compensation (LU) added to this entry's target
    /// (see `profiles::playback_offset_lu`). 0 = level at the common target as-is.
    pub offset_lu: f64,
}

/// The result of leveling a whole setlist to one common target.
#[derive(Debug, Clone, Serialize)]
pub struct SetlistResult {
    /// The common target chosen = min(C across entries) − `headroom_lu`.
    pub target_lufs: f64,
    pub results: Vec<LevelResult>,
}

/// Level a whole setlist so every (preset, instrument) pair lands at one common
/// loudness — the goal being no on-stage jump when switching presets/guitars.
///
/// Two passes (each entry's stimulus is its instrument's): pass 1 measures `C`
/// for every entry; the common target `T = min(C − offset) − headroom_lu` is the
/// loudest level every preset can still reach AT ITS OWN effective target
/// `T + offset_lu` (the per-instrument Fletcher–Munson compensation —
/// presetLevel only attenuates, so an effective target above any preset's `C`
/// would clamp → a residual jump, surfaced per row). Pass 2 applies each entry's
/// effective target (reloading the preset, since measuring others moved the
/// "current" preset). Verify is forced off for speed across many presets.
pub fn level_setlist(
    entries: &[SetlistEntry<'_>],
    headroom_lu: f64,
    ref_level: f32,
    save: bool,
) -> Result<SetlistResult, String> {
    if entries.is_empty() {
        return Err("no presets to level".to_string());
    }
    let ref_level = ref_level.clamp(0.05, 1.0);

    // Pass 1 — measure C for every entry (C is intrinsic, independent of target).
    let mut measured: Vec<MeasuredC> = Vec::with_capacity(entries.len());
    for e in entries {
        measured.push(measure_c(e.slot, e.stimulus, ref_level, &[])?);
    }

    // Common target: just below the quietest-capable preset's ceiling, in
    // OFFSET-ADJUSTED space (an entry leveled `offset_lu` hotter eats into its
    // own ceiling by exactly that much, so its constraint is `C − offset`).
    let cs: Vec<f64> = measured
        .iter()
        .zip(entries)
        .map(|(m, e)| m.c - e.offset_lu)
        .collect();
    let target_lufs = common_target(&cs, headroom_lu).ok_or("no presets to level")?;

    // Pass 2 — apply each entry's effective target (reload: measuring moved current).
    let opts = LevelOptions {
        save,
        verify: false,
        ref_level,
        ..Default::default()
    };
    let mut results = Vec::with_capacity(entries.len());
    for (e, m) in entries.iter().zip(measured.iter()) {
        let entry_target = target_lufs + e.offset_lu;
        let (final_level, clamped, predicted) = solve_level(m.c, entry_target);
        let (saved, verify_lufs) = apply_level(
            e.slot,
            e.stimulus,
            &LevelKnob::PresetLevel,
            final_level,
            opts,
            true,
        )?;
        results.push(LevelResult {
            slot: e.slot,
            ref_level,
            measured_lufs: m.measured_lufs,
            constant_c: m.c,
            final_level,
            target_lufs: entry_target,
            predicted_lufs: predicted,
            clamped,
            saved,
            verify_lufs,
            iterations: 1,
            dynamic_spread_lu: Some(m.dynamic_spread_lu),
            clamp_reason: None,
            verify_by_ear: false,
            previous_level: None,
            true_peak_dbtp: None,
        });
    }

    Ok(SetlistResult {
        target_lufs,
        results,
    })
}

// ─── Closed-loop block-control leveling ──────────────────────────────────────

/// Which control to drive when leveling. `PresetLevel` is the validated one-shot
/// master path; `Block` drives a chosen block parameter via `ChangeParameter`
/// (e.g. an amp's `outputLevel`) and is solved with a closed loop because an
/// arbitrary level knob's response isn't guaranteed linear-in-dB.
#[derive(Debug, Clone)]
pub enum LevelKnob {
    PresetLevel,
    Block {
        group_id: String,
        node_id: String,
        parameter_id: String,
        /// When `Some(scene_slot)` (0-based `scenes[]` wire index), each connection
        /// loads that scene and enables per-block Scene Edit before driving the knob,
        /// so the write lands on the SCENE overlay (per-scene leveling). `None` =
        /// level the base/preset value (a normal block knob; base needs no recall —
        /// the preset load activates it).
        scene_slot: Option<u32>,
    },
}

impl LevelKnob {
    pub fn label(&self) -> String {
        match self {
            LevelKnob::PresetLevel => "presetLevel".to_string(),
            LevelKnob::Block {
                group_id,
                node_id,
                parameter_id,
                scene_slot,
            } => match scene_slot {
                Some(s) => format!("{group_id}/{node_id}/{parameter_id}@scene{s}"),
                None => format!("{group_id}/{node_id}/{parameter_id}"),
            },
        }
    }
}

/// Closed-loop convergence tolerance and iteration cap. Each iteration is one
/// fresh connection (re-amp engages once per connection), so the cap bounds the
/// device round-trips; ≈0.3 LU is well within audible-match for leveling.
const KNOB_TOL_LU: f64 = 0.3;
const KNOB_MAX_ITERS: u32 = 6;
const LIVE_SETTLE_MS: u64 = 350;
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
pub enum SceneLevelStrategy {
    LiveSecant,
    LiveProportional,
    LiveHybrid,
    FractalStyle,
    /// One preset load + one stream pair per preset; one re-amp engage per
    /// scene; trust-region slope jumps (`level_scenes_live_batched`).
    BatchedLive,
}

#[derive(Debug, Clone, Serialize)]
pub struct SceneLevelBenchmarkRow {
    pub preset_slot: u32,
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

/// Set the chosen knob to `value` on an open session (before re-amp engage).
fn set_knob(s: &mut Session, knob: &LevelKnob, value: f32) -> Result<(), String> {
    match knob {
        LevelKnob::PresetLevel => {
            s.set_preset_level(value)?;
            Ok(())
        }
        LevelKnob::Block {
            group_id,
            node_id,
            parameter_id,
            scene_slot,
        } => {
            if let Some(scene) = scene_slot {
                // Per-scene leveling: activate the scene, then enable scene mode on this
                // block so its params become scene-specific. Scene mode MUST be enabled
                // before the value write — otherwise the write hits the BASE/global
                // value and leaks across scenes (HW). It's re-asserted on EVERY
                // connection (scene + scene-edit don't survive the leveller's
                // reconnects). The settles must stay SHORT: the device silently drops
                // the write past ~700 ms after `loadScene` (see
                // `SETTLE_AFTER_SCENE_EDIT_MS`); the rare too-fast leak-to-base race
                // is covered by the verify + correction pass.
                s.load_scene(*scene)?;
                std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SCENE_RECALL_MS));
                s.set_node_scene_edit(group_id, node_id, true)?;
                std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SCENE_EDIT_MS));
            }
            s.change_parameter(group_id, node_id, parameter_id, value)
        }
    }
}

/// Write ONLY the knob value (no scene re-activation) — the live loop's
/// mid-stream adjustment. The scene + per-block Scene Edit were already
/// activated by the initial `set_knob` on the same connection; re-loading the
/// scene while re-amp is engaged and audio is streaming is slow and risks
/// disturbing the engaged state.
fn set_knob_value_only(s: &mut Session, knob: &LevelKnob, value: f32) -> Result<(), String> {
    match knob {
        LevelKnob::PresetLevel => {
            s.set_preset_level(value)?;
            Ok(())
        }
        LevelKnob::Block {
            group_id,
            node_id,
            parameter_id,
            ..
        } => s.change_parameter(group_id, node_id, parameter_id, value),
    }
}

/// Write a SET of block knobs that all belong to the SAME scene, doing the scene
/// recall + per-block Scene Edit ONCE up front (NOT per knob — calling `set_knob`
/// per knob re-`load_scene`s between writes, which reverts the prior knob's unsaved
/// value). Ordering mirrors `set_knob` but batched, so the scene-edit-before-write
/// rule holds for every block at once: load scene → enable Scene Edit on every
/// per-scene block → ONE settle → write every value. Base/`PresetLevel` knobs
/// (no `scene_slot`) write directly. The settle-race that leaks to the base scene
/// (see `SETTLE_AFTER_SCENE_EDIT_MS`) is paid once, covering all knobs.
fn set_knobs(s: &mut Session, targets: &[(&LevelKnob, f32)]) -> Result<(), String> {
    let scene = targets.iter().find_map(|(k, _)| match k {
        LevelKnob::Block {
            scene_slot: Some(slot),
            ..
        } => Some(*slot),
        _ => None,
    });
    if let Some(scene) = scene {
        s.load_scene(scene)?;
        std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SCENE_RECALL_MS));
        for (k, _) in targets {
            if let LevelKnob::Block {
                group_id,
                node_id,
                scene_slot: Some(_),
                ..
            } = k
            {
                s.set_node_scene_edit(group_id, node_id, true)?;
            }
        }
        std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SCENE_EDIT_MS));
    }
    for (k, v) in targets {
        set_knob_value_only(s, k, *v)?;
    }
    Ok(())
}

/// Fresh-connect, load `scene_slot`, engage re-amp, capture — measure the scene's
/// loudness AS-IS without writing ANY parameter (no `set_knob`, no Scene Edit). Lets
/// the one-shot runner decide whether a scene already sits at target before touching
/// it. The preset must already be current (loaded in a prior connection).
/// The integrated LUFS of a re-amp capture's loudest channel, erroring on silence.
/// The shared tail of every isolated measurement (load/engage/capture/disengage
/// differ per caller; this `capture → loudest channel → measure → finite-check` is
/// identical). `pub(crate)` so the `lib.rs` probe measure paths share it too.
pub(crate) fn loudest_lufs(cap: Result<audio::Capture, String>) -> Result<f64, String> {
    loudest_loudness(cap).map(|l| l.integrated_lufs)
}

/// Like [`loudest_lufs`] but keeps the full meter reading (integrated + short-term
/// max), for paths that report the capture's dynamics spread alongside the level.
fn loudest_loudness(cap: Result<audio::Capture, String>) -> Result<lufs::Loudness, String> {
    let cap = cap?;
    let (ch, _) = cap.loudest_channel();
    let m = lufs::measure_mono(&cap.channel(ch), cap.sample_rate)?;
    m.integrated_lufs
        .is_finite()
        .then_some(m)
        .ok_or_else(|| NO_SIGNAL_CAPTURED.to_string())
}

/// Engage re-amp on `s` (latching the already-set knob/scene), settle, capture the
/// FULL stimulus + decay tail, measure the loudest channel's integrated LUFS, then
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
) -> Result<lufs::Loudness, String> {
    let _ = s.set_reamp_mode(true)?;
    std::thread::sleep(Duration::from_millis(SETTLE_AFTER_REAMP_MS));
    let cap = audio::reamp_capture(stimulus, RATE, CAPTURE_TAIL_MS);
    let _ = s.set_reamp_mode(false);
    loudest_loudness(cap)
}

/// GUARANTEED re-amp OFF on a fresh connection — the run-end backstop every
/// command that engages re-amp must call, success or failure. The device
/// silently DROPS a `set_reamp_mode(false)` sent on a session that has sat
/// idle >~1 s (HW-bisected: 300 ms lands, 1 s+ drops; heartbeats through the
/// idle rescue it — the same session-lapse cliff as the ~700 ms scene-write
/// drop), and every ~7 s leveling capture idles that long, so the in-session
/// disengage after each capture cannot be trusted. A dropped final OFF strands
/// the unit input-muted until a power-cycle (HW-observed; recovery:
/// `probe --reamp-off`). `tag` names the calling lane in the log lines.
pub(crate) fn reamp_off_guaranteed(tag: &str) {
    match Session::connect_lean().and_then(|mut s| s.set_reamp_mode(false)) {
        Ok(_) => log::info!("{tag}: final re-amp OFF sent"),
        Err(e) => log::warn!("{tag}: final re-amp OFF failed ({e})"),
    }
}

fn measure_scene_asis(scene_slot: u32, stimulus: &[f32]) -> Result<lufs::Loudness, String> {
    let mut s = Session::connect_lean()?;
    s.load_scene(scene_slot)?;
    std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SET_MS));
    engage_measure_disengage(&mut s, stimulus)
}

/// Fresh-connect, set `knob`=`value` (before engage), engage re-amp once,
/// measure the loudest channel on the full capture. Restores re-amp OFF.
fn measure_knob_at(
    stimulus: &[f32],
    knob: &LevelKnob,
    value: f32,
    force_bypass: &[(String, String, bool)],
) -> Result<lufs::Loudness, String> {
    let mut s = Session::connect_lean()?;
    // Force footswitch-block bypasses BEFORE the knob set + single re-amp engage so they latch —
    // the same connect → bypasses → set → engage ordering the footswitch `measure_at` proves.
    for (g, n, byp) in force_bypass {
        s.change_parameter_bool(g, n, "bypass", *byp)?;
    }
    set_knob(&mut s, knob, value)?;
    std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SET_MS));
    engage_measure_disengage(&mut s, stimulus)
}

// ───────────────────────── Footswitch (engaged-state) leveling ─────────────────────────
//
// Levels a footswitch's engaged state by solving the switch-ON value (`valueA`) of a
// `param` function on that switch. The leveler creates/edits ONLY a parameter-change
// assignment for the user's chosen block+param — it does NOT touch on/off. Measurement
// sweeps the chosen param (via `changeParameter`) in the preset's natural state and finds
// the value that hits target; when the footswitch is later engaged, that param jumps to the
// solved `valueA`. The param is user-chosen (any continuous `[0,1]` control), so this is a
// generic param-space secant (not the amplitude one-shot) with an honest clamp — a param on
// a bypassed/inert block measures as no-authority and clamps.

/// One footswitch-leveling outcome (mirrors the `LevelResult` subset the UI shows).
#[derive(Debug, Clone, Serialize)]
pub struct FootswitchLevelResult {
    pub switch: u32,
    /// Engaged loudness at the low reference seed (context).
    pub measured_lufs: f64,
    /// Solved switch-ON value written as the `param` function's `valueA`.
    pub final_value: f32,
    pub target_lufs: f64,
    /// Achieved engaged loudness at `final_value`.
    pub predicted_lufs: f64,
    pub clamped: bool,
    pub clamp_reason: Option<String>,
    pub saved: bool,
    pub verify_lufs: Option<f64>,
    pub iterations: u32,
    pub dynamic_spread_lu: Option<f64>,
    /// `"baked"` (value written straight onto the block) or `"assigned"` (param-change
    /// footswitch function written) — which simplification the leveler chose.
    pub method: String,
}

/// How to write the leveling `param` function — resolved by the caller (edit an existing
/// matching function, or add at the next free index), preserving its display fields.
#[derive(Debug, Clone)]
pub struct FootswitchWriteSpec {
    pub function_index: u32,
    pub color_a: u32,
    pub color_b: u32,
    pub custom_label: String,
    pub link_group: u32,
    pub is_active: bool,
}

/// How `level_footswitch` persists the solved value.
#[derive(Debug, Clone)]
pub enum FsWrite {
    /// Write a `param`-change footswitch function (`valueA`=solved, `valueB`=`value_b`).
    Assign {
        value_b: f32,
        spec: FootswitchWriteSpec,
    },
    /// Bake the solved value straight onto the block (`change_parameter`), and clear a
    /// now-redundant `param` function at `clear_stale` so the bake is the single source.
    Bake { clear_stale: Option<u32> },
}

/// Pure secant step in PARAMETER space: two `(value, loudness)` points → the next value
/// that should hit `target`. `None` when the local slope is ~flat (the param doesn't move
/// loudness). UNCLAMPED — caller clamps to the param's `[0,1]` range.
fn fs_secant_next(p0: (f64, f64), p1: (f64, f64), target: f64) -> Option<f64> {
    let slope = (p1.1 - p0.1) / (p1.0 - p0.0);
    if !slope.is_finite() || slope.abs() < 1e-3 {
        return None;
    }
    Some(p1.0 + (target - p1.1) / slope)
}

/// True if `ftsw[switch][index]` is a `param` function targeting `param` — the post-write
/// read-back confirmation (the schema has no dedicated `…Changed` echo).
fn param_fn_present(ftsw: &serde_json::Value, switch: u32, index: u32, param: &str) -> bool {
    ftsw.as_array()
        .and_then(|a| a.get(switch as usize))
        .and_then(|s| s.as_array())
        .and_then(|fns| fns.get(index as usize))
        .map(|f| {
            f.get("func").and_then(|v| v.as_str()) == Some("param")
                && f.get("parameterId").and_then(|v| v.as_str()) == Some(param)
        })
        .unwrap_or(false)
}

/// One fresh-connection engaged-state measurement point for a footswitch job: force the
/// engaged bypass list, set the swept param, engage re-amp once, measure. The forced
/// state lives only on this throwaway connection's working-copy edits; the batch write
/// session's reload discards ALL accumulated sweep pollution at once.
pub(crate) fn measure_fs_at(
    lev: (&str, &str, &str),
    engaged_bypass: &[(String, String, bool)],
    stimulus: &[f32],
    v: f32,
) -> Result<lufs::Loudness, String> {
    let mut s = Session::connect_lean()?;
    for (g, n, byp) in engaged_bypass {
        s.change_parameter_bool(g, n, "bypass", *byp)?;
    }
    s.change_parameter(lev.0, lev.1, lev.2, v)?;
    std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SET_MS));
    engage_measure_disengage(&mut s, stimulus)
}

/// Is the switch's engaged loudness already at target (within `KNOB_TOL_LU`, not clamped)?
/// The footswitch mirror of `scene_at_target` / `level_unchanged` — same acceptance band,
/// so a re-run leaves an in-tolerance switch untouched instead of re-solving and
/// re-randomizing it (the idempotency gap PR #74 deferred). `clamped` is always `false`
/// here (the probe measures a real value at `cur`), but the param matches `scene_at_target`
/// for parity and testability.
fn switch_at_target(measured: f64, target: f64, clamped: bool) -> bool {
    scene_at_target(measured, target, clamped)
}

/// Measurement/solve phase of ONE footswitch job — no write, no save, no reload.
/// CALLER CONTRACT: the preset is already current (load it once per batch), and the
/// caller discards the sweep pollution afterwards (the batch write session's reload,
/// or a plain reload on the dry/no-signal paths). Returns the un-persisted result
/// (`saved:false`, `verify_lufs:None`); `final_value` is the solved value.
///
/// `current_value` = the switch's currently-configured engaged value (a live-read prior
/// `valueA` on the Assign re-run path). When `Some`, the leveler probes it FIRST: if the
/// engaged loudness there is already at target it returns `final_value == current_value`
/// verbatim so the caller writes nothing — the re-run idempotency skip (mirrors the base
/// `level_unchanged` / scene `scene_at_target` skips). `None` (fresh assign, Bake, probe
/// seams) always solves.
pub fn measure_footswitch(
    switch: u32,
    lev: (&str, &str, &str),
    engaged_bypass: &[(String, String, bool)],
    stimulus: &[f32],
    target_lufs: f64,
    method: &str,
    current_value: Option<f32>,
) -> Result<FootswitchLevelResult, String> {
    // Send the forced engaged-bypass list only on the FIRST successful capture:
    // working-copy edits persist across HID disconnect/reconnect (HW-proven, fw
    // 1.8.45 — a fresh zero-write connection measured the forced state exactly),
    // and no reload happens inside this function's scope (caller contract above).
    // `forced` is set only on Ok, so an errored capture re-sends the full list.
    // TMP_FS_ISOLATION_EVERY restores the old per-capture re-send (kill-switch).
    let isolation_every = std::env::var("TMP_FS_ISOLATION_EVERY").is_ok();
    let forced = std::cell::Cell::new(false);
    let measure_at = |v: f32| {
        let bypass: &[(String, String, bool)] = if !isolation_every && forced.get() {
            &[]
        } else {
            engaged_bypass
        };
        let r = measure_fs_at(lev, bypass, stimulus, v);
        if r.is_ok() {
            forced.set(true);
        }
        r
    };

    // Guaranteed re-amp OFF on a fresh connection — the measurement's last disengage can be
    // dropped, stranding the unit input-muted. (Not the write-confirm fix; just hygiene.)
    let reamp_off = || {
        let _ = Session::connect_lean().map(|mut s| s.set_reamp_mode(false));
    };

    // Idempotency probe: if the switch's currently-configured engaged value already hits
    // target, leave it untouched (a re-run must not re-solve + re-randomize an in-tolerance
    // switch). Reuses `measure_at` (so a success arms the isolation-once optimization for the
    // seeds); a NO_SIGNAL / floor / transient error falls through to the seed pass, which
    // owns the routing-clamp verdict. `current_value` is None for fresh assigns / probe seams.
    if let Some(cur) = current_value {
        match require_live(|| measure_at(cur), stimulus) {
            Ok(l) if switch_at_target(l.integrated_lufs, target_lufs, false) => {
                reamp_off();
                // Skip signal: `final_value` == the caller's current value verbatim, so the
                // caller detects the no-op by `final_value == current` and writes nothing
                // (the footswitch mirror of the scene lane's off-wire `writes: 0`).
                return Ok(FootswitchLevelResult {
                    switch,
                    measured_lufs: l.integrated_lufs,
                    final_value: cur,
                    target_lufs,
                    predicted_lufs: l.integrated_lufs,
                    clamped: false,
                    clamp_reason: None,
                    saved: false,
                    verify_lufs: None,
                    iterations: 1,
                    dynamic_spread_lu: Some(l.spread_lu()),
                    method: method.into(),
                });
            }
            // In-tolerance-but-not-a-skip falls through to the seed pass; a probe error
            // must disengage re-amp first (the seed pass re-engages on a fresh connection).
            Ok(_) => {}
            Err(_) => reamp_off(),
        }
    }

    // Seed two real points and run a bounded generic secant.
    let (v_lo, v_hi) = (0.25f32, 0.75f32);
    // The FIRST seed doubles as the routing probe: a genuinely silent capture (device output not
    // on USB 1/2) makes `loudest_loudness` error "no signal captured" — convert THAT one to the
    // honest "not on USB 1/2" clamp (mirrors the scene mute-floor idiom below). Signal-present but
    // flat/short-of-target is a headroom/authority clamp with NO reason, not a routing error. Only
    // the first seed catches: broken routing is silent from capture #1; later silences stay errors.
    // Floor-guarded (the flat-but-finite silent-inject case); the NO_SIGNAL arm below
    // stays separate — genuine silence is the routing clamp, not a floor read.
    let l_lo = match require_live(|| measure_at(v_lo), stimulus) {
        Ok(l) => l,
        Err(e) if e.contains(NO_SIGNAL_CAPTURED) => {
            reamp_off();
            return Ok(FootswitchLevelResult {
                switch,
                measured_lufs: MUTE_FLOOR_SILENT_LUFS,
                final_value: v_lo,
                target_lufs,
                predicted_lufs: MUTE_FLOOR_SILENT_LUFS,
                clamped: true,
                clamp_reason: Some("no signal on USB 1/2".into()),
                saved: false,
                verify_lufs: None,
                iterations: 1,
                dynamic_spread_lu: None,
                method: method.into(),
            });
        }
        Err(e) => return Err(e),
    };
    let l_hi = measure_at(v_hi)?;
    let mut iterations = 2u32;
    let err = |l: f64| (l - target_lufs).abs();
    let (mut best_v, mut best_lufs, mut best_spread) =
        if err(l_lo.integrated_lufs) <= err(l_hi.integrated_lufs) {
            (v_lo, l_lo.integrated_lufs, l_lo.spread_lu())
        } else {
            (v_hi, l_hi.integrated_lufs, l_hi.spread_lu())
        };
    // Run the secant only when not already converged AND the knob has authority (a flat seed pair
    // can't be solved — leave it as an honest, reason-less clamp).
    if err(best_lufs) > KNOB_TOL_LU
        && (l_hi.integrated_lufs - l_lo.integrated_lufs).abs() >= KNOB_TOL_LU
    {
        let mut p0 = (v_lo as f64, l_lo.integrated_lufs);
        let mut p1 = (v_hi as f64, l_hi.integrated_lufs);
        for _ in 0..MEASURE_CORRECT_MAX {
            let Some(raw) = fs_secant_next(p0, p1, target_lufs) else {
                break; // flat response — the knob can't move loudness here
            };
            let v2 = raw.clamp(0.0, 1.0) as f32;
            let l2 = measure_at(v2)?;
            iterations += 1;
            if err(l2.integrated_lufs) < err(best_lufs) {
                best_v = v2;
                best_lufs = l2.integrated_lufs;
                best_spread = l2.spread_lu();
            }
            if err(l2.integrated_lufs) <= KNOB_TOL_LU {
                break;
            }
            p0 = p1;
            p1 = (v2 as f64, l2.integrated_lufs);
        }
    }
    // Signal is present past the seed probe, so a miss is a headroom/authority clamp, never a
    // routing error → `clamp_reason` stays None (the UI shows "clamped at X LUFS").
    let clamped = err(best_lufs) > KNOB_TOL_LU;
    Ok(FootswitchLevelResult {
        switch,
        measured_lufs: l_lo.integrated_lufs,
        final_value: best_v,
        target_lufs,
        predicted_lufs: best_lufs,
        clamped,
        clamp_reason: None,
        saved: false,
        verify_lufs: None,
        iterations,
        dynamic_spread_lu: Some(best_spread),
        method: method.into(),
    })
}

/// One footswitch's solved write, pending the batch's single write+save session.
pub struct FsPendingWrite {
    pub switch: u32,
    /// The leveled `(group, node, param)`.
    pub lev: (String, String, String),
    pub write: FsWrite,
    /// The solved value (`valueA` for Assign; the baked block value for Bake).
    pub value: f32,
}

/// Level a footswitch's engaged state by solving a parameter-change assignment.
/// `lev` = the `(group, node, param)` solved; `value_b` = the switch-OFF value written. On
/// `save`, writes the `param` function (gated on the field-54 echo / read-back, never on
/// `presetError`) and persists; otherwise reverts the working copy. The measurement param
/// sweep is NEVER saved — the write path reloads the preset first.
///
/// SINGLE-SWITCH seam, used by the `probe` HW-verify arms: composes the same
/// `measure_footswitch` + `write_footswitch_values` the app's batched command
/// (`level_footswitches_apply`) assembles itself — keep the three in lockstep.
#[allow(clippy::too_many_arguments)]
pub fn level_footswitch(
    slot: u32,
    switch: u32,
    lev: (&str, &str, &str),
    engaged_bypass: &[(String, String, bool)],
    write: &FsWrite,
    stimulus: &[f32],
    target_lufs: f64,
    save: bool,
    verify: bool,
) -> Result<FootswitchLevelResult, String> {
    // Load the preset in its own connection (re-amp latch workaround), then measure on
    // fresh connections (the preset stays current across reconnects).
    {
        let mut s = Session::connect_lean()?;
        s.load_preset(slot)?;
        std::thread::sleep(Duration::from_millis(settle_after_load_ms()));
    }
    std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));

    let method = match write {
        FsWrite::Bake { .. } => "baked",
        FsWrite::Assign { .. } => "assigned",
    };
    // Single-switch probe seam: always solve fresh (no idempotency probe) — the batched
    // command owns the re-run skip.
    let result = measure_footswitch(
        switch,
        lev,
        engaged_bypass,
        stimulus,
        target_lufs,
        method,
        None,
    )?;
    if result.clamp_reason.is_some() {
        // No-signal routing clamp: nothing to write — discard the sweep pollution.
        std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
        if let Ok(mut s) = Session::connect_lean() {
            let _ = s.load_preset(slot);
        }
        return Ok(result);
    }
    let mut result = result;

    // ── Write (save only): the batch writer reloads (discarding the sweep pollution),
    //    writes, and persists with ONE save; the dry path just reloads ──
    if save {
        let pending = [FsPendingWrite {
            switch,
            lev: (lev.0.into(), lev.1.into(), lev.2.into()),
            write: write.clone(),
            value: result.final_value,
        }];
        write_footswitch_values(slot, &pending)?;
        result.saved = true;
        if verify {
            std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
            result.verify_lufs = measure_fs_at(lev, engaged_bypass, stimulus, result.final_value)
                .ok()
                .map(|l| l.integrated_lufs);
            let mut s = Session::connect_lean()?; // discard the verify pollution
            let _ = s.load_preset(slot);
        }
    } else {
        let mut s = Session::connect_lean()?; // dry: discard the measurement pollution
        let _ = s.load_preset(slot);
    }
    // Final guarantee (verify re-amps; never leave the unit input-muted).
    let _ = Session::connect_lean().map(|mut s| s.set_reamp_mode(false));
    Ok(result)
}

/// Write every pending footswitch value on ONE live-edit session and persist with ONE
/// `saveCurrentPreset` — the per-preset single save (a batch of switches used to reload
/// and save once EACH: N base flashes + N saves of user-visible churn). Session shape
/// per the chunked-edit rules: establish a LIVE CONTROLLER (`begin_live_edit` warmup),
/// then load (discarding ALL the measurement pollution the batch's sweeps accumulated),
/// then keep the session live with heartbeat bursts right up to each chunked `ftsw`
/// edit (chunked edits are silently DROPPED if the session lapses — a passive sleep
/// lets it lapse; HW `probe --repro-chunked`). Each write keeps its confirm gate
/// (field-54 echo / read-back, retry-once, never save on `presetError`); ANY
/// unconfirmed write aborts BEFORE the save, so nothing half-applied persists.
pub fn write_footswitch_values(slot: u32, pending: &[FsPendingWrite]) -> Result<(), String> {
    if pending.is_empty() {
        return Ok(());
    }
    // Guaranteed re-amp OFF first — the measurement's last disengage can be dropped.
    let _ = Session::connect_lean().map(|mut s| s.set_reamp_mode(false));
    std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    s.load_preset(slot)?;
    let name = s.active_preset_name().unwrap_or_default();
    if !name.is_empty() && !s.await_active_preset(&name, 20) {
        return Err("after reload, active preset changed — aborting before write".into());
    }
    for _ in 0..8 {
        let _ = s.heartbeat();
        let _ = s.pump_collect(150);
    }
    for p in pending {
        let (group, node, param) = (&p.lev.0, &p.lev.1, &p.lev.2);
        match &p.write {
            FsWrite::Assign { value_b, spec } => {
                let json = serde_json::json!({
                    "func": "param", "groupId": group, "nodeId": node, "parameterId": param,
                    "valueA": p.value, "valueB": value_b, "valueType": 2,
                    "colorA": spec.color_a, "colorB": spec.color_b,
                    "customLabel": spec.custom_label, "switchType": 0,
                    "isActive": spec.is_active, "linkGroup": spec.link_group
                })
                .to_string();
                // Confirm the set landed: the device ECHOES field 54 on success (checked first,
                // before the read-back clears the buffer); the working-copy read-back corroborates
                // (it can lag a heartbeat on a post-measurement flooded line). The first edit after
                // a fresh load can be silently dropped, so retry the whole set+confirm once.
                let mut confirmed = false;
                let mut last_seen = Vec::new();
                for _ in 0..2 {
                    s.set_footswitch_assignment(p.switch, spec.function_index, &json, false, None)?;
                    if s.saw_preset_error() {
                        return Err(
                            "device rejected the footswitch assignment (presetError) — not saved"
                                .into(),
                        );
                    }
                    last_seen = s.seen_preset_fields();
                    if last_seen.contains(&54) {
                        confirmed = true;
                        break;
                    }
                    for _ in 0..3 {
                        if s.live_ftsw().is_some_and(|f| {
                            param_fn_present(&f, p.switch, spec.function_index, param)
                        }) {
                            confirmed = true;
                            break;
                        }
                        let _ = s.heartbeat();
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    if confirmed {
                        break;
                    }
                }
                if !confirmed {
                    return Err(format!(
                        "footswitch assignment not confirmed (no field-54 echo / read-back, \
                         retried; device replied with PresetMessage fields {last_seen:?}) — not saved"
                    ));
                }
            }
            FsWrite::Bake { clear_stale } => {
                // Clear a now-redundant param fn FIRST (a chunked `ftsw` edit — done while the
                // session is freshest), confirming it's gone (else its valueA would override the
                // baked value when engaged). Then bake the value onto the block. Abort before
                // save if the clear can't be confirmed (nothing is persisted on the reload).
                if let Some(idx) = clear_stale {
                    s.clear_footswitch_assignment(p.switch, *idx)?;
                    if s.saw_preset_error() {
                        return Err(
                            "device rejected the footswitch clear (presetError) — not saved".into(),
                        );
                    }
                    let mut cleared = false;
                    for _ in 0..4 {
                        if s.live_ftsw().is_some_and(|f| {
                            crate::footswitch::existing_param_fn_index(&f, p.switch, node, param)
                                .is_none()
                        }) {
                            cleared = true;
                            break;
                        }
                        let _ = s.heartbeat();
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    if !cleared {
                        return Err(
                            "redundant footswitch param fn not confirmed cleared — not saved"
                                .into(),
                        );
                    }
                }
                s.change_parameter(group, node, param, p.value)?;
            }
        }
        // Keep the live controller warm between chunked writes.
        let _ = s.heartbeat();
        let _ = s.pump_collect(150);
    }
    s.save_current_preset(slot)?;
    Ok(())
}

/// Pure secant step for the closed loop: given two measured points
/// `(xa, ya)`/`(xb, yb)` of knob-value → captured LUFS and a `target`, return the
/// next knob value that should hit it (UNCLAMPED — caller clamps to bounds).
/// `None` if the local response is flat (slope ≈ 0 → the knob doesn't move
/// loudness here, so the caller should stop).
fn secant_next(xa: f32, ya: f64, xb: f32, yb: f64, target: f64) -> Option<f32> {
    let dx = (xb - xa) as f64;
    if dx.abs() < 1e-9 {
        return None;
    }
    let slope = (yb - ya) / dx; // LUFS per knob unit
    if !slope.is_finite() || slope.abs() < 1e-4 {
        return None;
    }
    let next = xb as f64 + (target - yb) / slope;
    if next.is_finite() {
        Some(next as f32)
    } else {
        None
    }
}

fn knob_search_space(lo: f32, hi: f32) -> (bool, f32, f32) {
    let log_space = lo >= 0.0 && hi <= 1.0 + 1e-6;
    let eps = 1e-3f32;
    let to_c = |x: f32| {
        if log_space {
            20.0 * x.max(eps).log10()
        } else {
            x
        }
    };
    let c_lo = to_c(if log_space { lo.max(eps) } else { lo });
    let c_hi = to_c(hi);
    (log_space, c_lo, c_hi)
}

fn knob_to_coord(value: f32, log_space: bool) -> f32 {
    if log_space {
        20.0 * value.max(1e-3).log10()
    } else {
        value
    }
}

fn coord_to_knob(coord: f32, log_space: bool, lo: f32, hi: f32) -> f32 {
    if log_space {
        10f32.powf(coord / 20.0).clamp(lo, hi)
    } else {
        coord.clamp(lo, hi)
    }
}

fn live_window_lufs(live: &audio::LiveReamp, window_ms: u64) -> Result<f64, String> {
    let cap = live.recent_capture(window_ms)?;
    let (ch, _) = cap.loudest_channel();
    let lufs = lufs::measure_mono(&cap.channel(ch), cap.sample_rate)?.integrated_lufs;
    if lufs.is_finite() {
        Ok(lufs)
    } else {
        Err("no finite live LUFS measurement".to_string())
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
                secant_next(pa, py, coord, measured, target).unwrap_or(full)
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
/// Trust region for the slope-jump controller (max dB the knob moves per trim):
/// full computed jumps overshot steep nonlinear knobs by ~6 LU on HW.
const BATCH_TRUST_DB: f32 = 6.0;

/// Per-scene outcome of [`level_scenes_live_batched`].
#[derive(Debug, Clone, Serialize)]
pub struct BatchedSceneOutcome {
    pub scene_slot: u32,
    /// The effective (offset-adjusted) loudness target this scene was leveled to.
    /// Per-scene because one batch can carry a mix of targets; `outcome_to_level_result`
    /// reads it here rather than zipping outcomes against jobs by index (the failure
    /// filter misaligns positional zips).
    pub target_lufs: f64,
    pub final_lufs: Option<f64>,
    pub final_level: Option<f32>,
    pub clamped: bool,
    pub windows: u32,
    pub writes: u32,
    pub elapsed_ms: u128,
    pub failure: Option<String>,
    /// Dynamics spread of the scene's measure capture (LU); `None` where the
    /// measuring path has no full-capture meter (the live-window runner) or the
    /// scene failed. See `LevelResult::dynamic_spread_lu`.
    pub dynamic_spread_lu: Option<f64>,
    /// Set with `clamped` when the scene clamped for a SPECIFIC reason the UI should
    /// show verbatim — currently "no authority": a big `outputLevel` change moved the
    /// USB 1/2 capture by ~nothing, so the amp is off-branch / off-USB (or hard-limited).
    /// `None` for an ordinary headroom clamp.
    pub clamp_reason: Option<String>,
    /// Best-effort "verify by ear" flag from the rebalance flow: the lane-mute floor was
    /// close enough to a solo lane that bleed may have skewed the equal-solo balance (the
    /// overall target is still hit). `false` outside rebalance.
    pub verify_by_ear: bool,
}

/// One amp knob to drive within a scene: the control, its bounds, and its current
/// value in THAT scene (from the pre-pass doc, so the first jump starts from truth).
#[derive(Debug, Clone)]
pub struct KnobTarget {
    pub knob: LevelKnob,
    pub lo: f32,
    pub hi: f32,
    pub current: f32,
}

/// A pre-resolved per-scene leveling job. A scene carries a **set** of amp knobs:
/// one for a series chain (the last active amp) and the split-output single-lane
/// case, but TWO+ for a parallel-merged scene where each lane has its own amp — those
/// are driven together by one factor `k` (joint-k), since scaling every amp in a sum
/// by the same `k` shifts the captured loudness by exactly `20·log10(k)` regardless of
/// inter-lane correlation. The probe-only bench runner (`level_scenes_live_batched`)
/// requires `knobs.len() == 1` (via `solo()`) and errors otherwise.
#[derive(Debug, Clone)]
pub struct SceneJob {
    pub scene_slot: u32,
    /// This scene's own (offset-adjusted) loudness target — the SINGLE source of truth.
    /// `build_scene_jobs` stamps it on every job; the app command overrides it per wire
    /// job so a mixed-target preset levels in ONE batch. The runners read it directly.
    pub target_lufs: f64,
    pub knobs: Vec<KnobTarget>,
    /// When `Some`, this scene can't be safely leveled (mic/split/no-active-amp/etc.);
    /// the runner reports it as a skipped (failed) outcome and moves on, never aborting
    /// the whole run. `knobs` is empty in that case.
    pub skip: Option<String>,
    /// True only for a parallel scene whose lanes RE-MERGE (≥2 knobs feeding one summed
    /// output) — the rebalance flow may adjust the lanes' mix. False for series, single
    /// amp, and split-OUTPUT scenes (separate physical outs have no shared mix).
    pub rebalanceable: bool,
}

impl SceneJob {
    /// The single knob for the single-knob paths; errors if this is a multi-knob
    /// (parallel) job or a skip job, which those probe-only runners can't solve.
    fn solo(&self) -> Result<&KnobTarget, String> {
        if let Some(reason) = &self.skip {
            return Err(reason.clone());
        }
        match self.knobs.as_slice() {
            [one] => Ok(one),
            n => Err(format!(
                "this leveling path supports a single amp knob per scene, got {} \
                 (a parallel-merged scene needs the joint-k runner)",
                n.len()
            )),
        }
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
pub fn level_scenes_live_batched(
    slot: u32,
    jobs: &[SceneJob],
    stimulus: &[f32],
    save: bool,
    mut on_scene: impl FnMut(u32, Option<&BatchedSceneOutcome>),
    mut cancelled: impl FnMut() -> bool,
) -> Result<Vec<BatchedSceneOutcome>, String> {
    let result = (|| {
        // Load in its own connection (set-after-load override + engage latch).
        {
            let mut s = Session::connect_lean()?;
            s.load_preset(slot)?;
            std::thread::sleep(Duration::from_millis(settle_after_load_ms()));
        }
        std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));

        // ONE pair of CoreAudio streams for the whole preset (between engages
        // they just carry silence). Rebuilding streams per scene both wasted
        // ~0.5 s/scene and churned coreaudiod.
        let live = audio::LiveReamp::start(stimulus, RATE)?;

        let mut outcomes = Vec::with_capacity(jobs.len());
        for job in jobs {
            if cancelled() {
                return Err(CANCELLED.to_string());
            }
            on_scene(job.scene_slot, None);
            let t0 = std::time::Instant::now();
            let mut windows = 0u32;
            let mut writes = 0u32;

            let scene_result = (|windows: &mut u32,
                                 writes: &mut u32|
             -> Result<(f64, f32, bool), String> {
                // This closed-loop runner is single-knob only (probe benchmark path);
                // a parallel-merged scene must use the joint-k `level_scenes_oneshot`.
                let kt = job.solo()?;
                // Fresh engage connection per scene: scene recall + Scene Edit
                // + start value ride `set_knob` BEFORE the engage (latch rule).
                let mut s = Session::connect()?;
                set_knob(&mut s, &kt.knob, kt.current.clamp(kt.lo, kt.hi))?;
                *writes += 1;
                std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SET_MS));
                let _ = s.set_reamp_mode(true)?;
                std::thread::sleep(Duration::from_millis(SETTLE_AFTER_REAMP_MS));

                let (log_space, c_lo, c_hi) = knob_search_space(kt.lo, kt.hi);
                let mut coord =
                    knob_to_coord(kt.current.clamp(kt.lo, kt.hi), log_space).clamp(c_lo, c_hi);
                std::thread::sleep(Duration::from_millis(LIVE_SETTLE_MS + BATCH_WINDOW_MS));
                let mut measured = live_window_lufs(&live, BATCH_WINDOW_MS)?;
                *windows += 1;
                let mut best = (coord, measured);
                let mut prev: Option<(f32, f64)> = None;

                for iter in 0..BATCH_MAX_TRIMS {
                    if cancelled() {
                        return Err(CANCELLED.to_string());
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
                    std::thread::sleep(Duration::from_millis(LIVE_SETTLE_MS + BATCH_WINDOW_MS));
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
                    std::thread::sleep(Duration::from_millis(LIVE_SETTLE_MS + BATCH_WINDOW_MS));
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

            std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
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
                },
                Err(e) if e == CANCELLED => return Err(e),
                Err(e) => BatchedSceneOutcome {
                    scene_slot: job.scene_slot,
                    target_lufs: job.target_lufs,
                    final_lufs: None,
                    final_level: None,
                    clamped: false,
                    windows,
                    writes,
                    elapsed_ms: t0.elapsed().as_millis(),
                    failure: Some(e),
                    dynamic_spread_lu: None,
                    clamp_reason: None,
                    verify_by_ear: false,
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
    let _ = Session::connect_lean().and_then(|mut s| s.set_reamp_mode(false).map(|_| ()));
    restore_after_unsaved_error(slot, save, result)
}

/// ONE-SHOT open-loop per-scene leveling — the validated replacement for
/// [`level_scenes_live_batched`] (HW). The active amp's `outputLevel` is
/// LINEAR in dB (`captured_LUFS = 20·log10(outputLevel) + C`, ~25 LU authority), so —
/// exactly like `presetLevel` — there is no need for a closed loop: measure ONCE at a
/// reference level via an ISOLATED fresh re-amp capture, solve `C`, set the exact
/// level. The BatchedLive runner's shared continuous stream MIS-MEASURED scenes
/// (returning impossible loudness, e.g. -6.96 LUFS on a knob whose true range is
/// -40..-14), which made the trust-region loop clamp on garbage; the isolated
/// measurement (`measure_knob_at`) reads correctly. Same signature + outcome shape as
/// `level_scenes_live_batched` so the command path is a drop-in swap. Per-scene
/// isolation rides `set_knob`'s Scene Edit; `presetLevel` (the Base) must be leveled
/// FIRST (it is a global multiplier over every scene). With `save`, every scene's
/// write accumulates UNSAVED in the working copy and ONE `saveCurrentPreset` at
/// batch end persists them all (`save_deferred_scene_writes` — also fired on
/// cancel, so already-reported scenes are never silently lost); `restore_scene`
/// is recalled first so the save stamps the preset's original active scene. A
/// per-scene failure becomes a failed outcome, never aborting the run.
pub fn level_scenes_oneshot(
    slot: u32,
    jobs: &[SceneJob],
    stimulus: &[f32],
    save: bool,
    restore_scene: Option<u32>,
    on_scene: impl FnMut(u32, Option<&BatchedSceneOutcome>),
    cancelled: impl FnMut() -> bool,
) -> Result<Vec<BatchedSceneOutcome>, String> {
    run_scene_jobs(
        slot,
        jobs,
        save,
        restore_scene,
        on_scene,
        cancelled,
        |job| jointk_one_scene(slot, job, stimulus, job.target_lufs, save, true),
    )
}

/// Post-save spot-verify tolerance (LU): a compensated sound re-measured at the PERSISTED
/// new presetLevel that lands more than this off target is the wrong-pl-solve tell (a
/// per-scene jointk solved against a stale pl — self-consistent at solve time, wrong after
/// the save re-establishes the real pl). Advisory: the save already happened, so this
/// warns + flags for the UI's Restore, it doesn't undo.
pub(crate) const REDIST_POST_VERIFY_TOL_LU: f64 = 1.5;

/// Gain-budget redistribution runner (loud-preset clamp class, single-amp v1). Raises
/// `presetLevel` to `new_preset_level` (UNSAVED) FIRST — a pure linear multiplier, so every
/// clamped scene inherits the rise as headroom — then re-levels EVERY sound in `jobs`
/// (base at slot 8 + all FS scenes) back to its target at the new pl via `jointk_one_scene`
/// (defer). A still-clamped scene stays at `outputLevel = 1.0` (jointk reports it clamped, no
/// write); every other sound drops its `outputLevel` to hold target (no overshoot — re-leveling
/// is uniform, so a lesser-clamped scene that now overshoots is compensated too). ATOMICITY:
/// if any sound's compensating write FAILS (error / no-authority off-branch), the redistribution
/// is partial → reload to discard, save NOTHING. Otherwise ONE `saveCurrentPreset` (base recall)
/// persists the new pl + every compensated `outputLevel` together, then a post-save AUDIO
/// spot-verify re-measures one compensated sound at the persisted pl (the only check at the real
/// pl — jointk's own verify is self-consistent at solve-time and misses a wrong-pl solve).
///
/// CALLER CONTRACT (mirrors `level_scenes_oneshot`): the preset is already current (the caller
/// ran `prepass_scene_docs`), and `jobs`' knobs carry each sound's pre-raise `current` value.
pub fn redistribute_clamped_headroom(
    slot: u32,
    new_preset_level: f32,
    jobs: &[SceneJob],
    stimulus: &[f32],
    restore_scene: Option<u32>,
    mut on_scene: impl FnMut(u32, Option<&BatchedSceneOutcome>),
    mut cancelled: impl FnMut() -> bool,
) -> Result<Vec<BatchedSceneOutcome>, String> {
    if cancelled() {
        return Err(CANCELLED.to_string());
    }
    // Raise presetLevel FIRST, UNSAVED. `measure_scene_asis` (the jointk measure) connects
    // LEAN — no `load_preset` — so this working-copy value survives every scene's fresh
    // re-amp connect (HW: unsaved writes persist across reconnects).
    {
        std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
        let mut s = Session::connect()?;
        s.set_preset_level(new_preset_level)?;
        std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SET_MS));
    }

    let mut outcomes = Vec::with_capacity(jobs.len());
    let mut stopped = false;
    for job in jobs {
        if cancelled() {
            stopped = true;
            break;
        }
        on_scene(job.scene_slot, None);
        let t0 = std::time::Instant::now();
        if let Some(reason) = &job.skip {
            let o = failed_scene_outcome(
                job.scene_slot,
                job.target_lufs,
                reason.clone(),
                t0.elapsed().as_millis(),
            );
            on_scene(job.scene_slot, Some(&o));
            outcomes.push(o);
            continue;
        }
        let result = jointk_one_scene(slot, job, stimulus, job.target_lufs, true, true);
        std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
        let o = match result {
            Ok(s) => {
                solved_scene_outcome(job.scene_slot, job.target_lufs, s, t0.elapsed().as_millis())
            }
            Err(e) if e == CANCELLED => {
                stopped = true;
                break;
            }
            Err(e) => {
                failed_scene_outcome(job.scene_slot, job.target_lufs, e, t0.elapsed().as_millis())
            }
        };
        on_scene(job.scene_slot, Some(&o));
        outcomes.push(o);
    }
    // Guaranteed fresh re-amp OFF (an interrupted capture can strand it engaged).
    let _ = Session::connect_lean().and_then(|mut s| s.set_reamp_mode(false).map(|_| ()));

    // ATOMICITY: a cancel or ANY failed/off-branch compensating write leaves a PARTIAL
    // redistribution — reload the stored preset to discard the unsaved pl + writes, persist
    // nothing. (A jointk-reported headroom clamp on a still-clamped scene is EXPECTED and
    // not a failure; an ERROR or a no-authority `clamp_reason` is.)
    let partial = outcomes
        .iter()
        .any(|o| o.failure.is_some() || o.clamp_reason.is_some());
    if stopped || partial {
        let _ = restore_saved_preset(slot);
        return Err(if stopped {
            CANCELLED.to_string()
        } else {
            "redistribution aborted: a compensating write did not land — nothing saved".to_string()
        });
    }

    // ONE save — new pl + every compensated outputLevel together, base scene recalled.
    save_deferred_scene_writes(slot, restore_scene)?;

    // Post-save AUDIO spot-verify at the PERSISTED pl (the wrong-pl-solve guard). Pick a
    // compensated sound that actually moved (writes > 0); re-measure it as-is. Advisory —
    // the save already landed, so a miss WARNS (the UI offers Restore), never re-writes.
    if let Some(check) = outcomes
        .iter()
        .find(|o| o.writes > 0 && o.final_lufs.is_some())
    {
        std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
        match require_live(|| measure_scene_asis(check.scene_slot, stimulus), stimulus) {
            Ok(l) => {
                let err = (l.integrated_lufs - check.target_lufs).abs();
                if err > REDIST_POST_VERIFY_TOL_LU {
                    log::warn!(
                        "redistribute slot={slot}: post-save spot-verify scene {} read {:.2} LUFS \
                         vs target {:.2} (Δ{:.2} > {REDIST_POST_VERIFY_TOL_LU}) — possible wrong-pl \
                         solve; Restore is available",
                        check.scene_slot, l.integrated_lufs, check.target_lufs, err
                    );
                } else {
                    log::info!(
                        "redistribute slot={slot}: post-save spot-verify scene {} on target ({:.2} LUFS)",
                        check.scene_slot, l.integrated_lufs
                    );
                }
            }
            Err(e) => log::warn!("redistribute slot={slot}: post-save spot-verify skipped ({e})"),
        }
        // The post-verify capture disengages re-amp itself; the command's run-end
        // `reamp_off_guaranteed` is the fresh-connection backstop, so no extra OFF here.
    }
    Ok(outcomes)
}

/// The ONE scene-batch scaffold shared by [`level_scenes_oneshot`] and
/// [`level_scenes_rebalance`] — only the per-job `solve` differs. Owning the loop in
/// one place matters beyond dedup: the loop has a SINGLE EXIT so the deferred-save
/// guard (persist accumulated unsaved writes on EVERY exit, incl. cancel — the
/// silent-data-loss class this design exists to prevent) exists exactly once.
///
/// CALLER CONTRACT: the preset must already be current — every caller runs
/// `prepass_scene_docs` (which loads it) right before this. Re-loading here was
/// pure churn the user SAW: the unit flashing back to the preset (base scene)
/// between the prepass and the first scene measure, once per dispatched scene.
fn run_scene_jobs(
    slot: u32,
    jobs: &[SceneJob],
    save: bool,
    restore_scene: Option<u32>,
    mut on_scene: impl FnMut(u32, Option<&BatchedSceneOutcome>),
    mut cancelled: impl FnMut() -> bool,
    mut solve: impl FnMut(&SceneJob) -> Result<SceneSolve, String>,
) -> Result<Vec<BatchedSceneOutcome>, String> {
    let mut outcomes = Vec::with_capacity(jobs.len());
    let mut attempted = false;
    let mut stopped = false;
    // Every writing scene verifies + self-corrects (see `jointk_one_scene`): a downstream
    // compressor undershoots the open-loop solve per scene, so the canary-only model isn't
    // enough. Cost is one verify capture per off-target scene (none when already at target).
    for job in jobs {
        if cancelled() {
            stopped = true;
            break;
        }
        on_scene(job.scene_slot, None);
        let t0 = std::time::Instant::now();
        let eff_target = job.target_lufs;

        // A skip job (unclassifiable scene: mic/split lane/no active amp/…) is reported
        // as a failed outcome and the run continues — never aborts the whole pass.
        if let Some(reason) = &job.skip {
            let outcome = failed_scene_outcome(
                job.scene_slot,
                eff_target,
                reason.clone(),
                t0.elapsed().as_millis(),
            );
            on_scene(job.scene_slot, Some(&outcome));
            outcomes.push(outcome);
            continue;
        }

        attempted = true;
        let result = solve(job);

        std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
        let outcome = match result {
            Ok(s) => solved_scene_outcome(job.scene_slot, eff_target, s, t0.elapsed().as_millis()),
            Err(e) if e == CANCELLED => {
                stopped = true;
                break;
            }
            Err(e) => failed_scene_outcome(job.scene_slot, eff_target, e, t0.elapsed().as_millis()),
        };
        on_scene(job.scene_slot, Some(&outcome));
        outcomes.push(outcome);
    }
    // Guaranteed fresh re-amp OFF (each `measure_knob_at`/`apply_level` already
    // disengages, but an interrupted capture can strand it).
    let _ = Session::connect_lean().and_then(|mut s| s.set_reamp_mode(false).map(|_| ()));
    // The batch's ONE persist — after the re-amp OFF, on its own clean connection.
    // Fired on the stopped path too, so already-reported scenes are never lost.
    if save && attempted {
        if stopped {
            let _ = save_deferred_scene_writes(slot, restore_scene);
        } else {
            save_deferred_scene_writes(slot, restore_scene)?;
        }
    }
    if stopped {
        return Err(CANCELLED.to_string());
    }
    Ok(outcomes)
}

/// The scene batch's ONE persist: recall the preset's original active scene (so the
/// save stamps the same base/scene/footswitch state the preset had before the run —
/// a save stamps `lastLoadedScene` + switch states from the working state), then ONE
/// `saveCurrentPreset` persisting every accumulated unsaved scene overlay. HW
/// (`probe --defer-scenes`, fw 1.8.45): unsaved scene-edit writes survive scene
/// recalls and reconnects; re-recalling a written scene does NOT revert it; base
/// recall = wire slot 8; the single save persists ALL accumulated overlays. One
/// retry on a fresh connection (the realistic failure is the HID open lockout, not
/// the save itself). The connection never toggles re-amp, so the post-re-amp
/// save-drop cannot bite.
fn save_deferred_scene_writes(slot: u32, restore_scene: Option<u32>) -> Result<(), String> {
    let attempt = || -> Result<(), String> {
        std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
        let mut s = Session::connect()?;
        if let Some(scene) = restore_scene {
            s.load_scene(scene)?;
            std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SET_MS));
        }
        s.save_current_preset(slot)
    };
    attempt().or_else(|e| {
        log::warn!("deferred scene save failed ({e}); retrying on a fresh connection");
        attempt()
    })
}

/// Result of the joint-k solve for a scene's amp-knob set.
struct JointK {
    /// Per-knob final levels (aligned with the input `knobs`), each `current_i · k_eff`.
    levels: Vec<f32>,
    /// The target needed more boost than the hottest amp's headroom allowed.
    clamped: bool,
    /// Predicted captured loudness at the applied levels (= target unless clamped).
    achieved: f64,
    /// The applied scale factor (< the ideal `k` when clamped).
    k_eff: f64,
}

/// Solve joint-k for a scene's amp-knob set, given the as-is `measured` loudness at the
/// knobs' current values. Scaling every amp's `outputLevel` by one `k` shifts the
/// summed output by exactly `20·log10(k)` (correlation-invariant), so:
///   `k = 10^((target − measured)/20)`, clamped to keep the hottest amp ≤ `LEVEL_MAX`
///   (`k_eff = min(k, LEVEL_MAX / max_i current_i)` — ratio-preserving), then
///   `level_i = current_i · k_eff`.
/// Requires amplitude (0..1) knobs — a dB-unit knob can't be scaled multiplicatively
/// (`−12 dB · 1.5` is nonsense) and errors. The per-knob `current` is floored at 1e-3
/// so an author-muted lane (current 0) doesn't divide-by-zero (it stays muted: `0·k`).
fn solve_joint_k_at(
    knobs: &[KnobTarget],
    target_lufs: f64,
    measured: f64,
) -> Result<JointK, String> {
    if knobs.is_empty() {
        return Err("joint-k: no amp knobs in scene".to_string());
    }
    for kt in knobs {
        if kt.lo < -1e-6 || kt.hi > 1.0 + 1e-6 {
            return Err(format!(
                "joint-k requires amplitude (0..1) knobs; got bounds [{}, {}] for {}",
                kt.lo,
                kt.hi,
                kt.knob.label()
            ));
        }
    }
    let max_cur = knobs
        .iter()
        .map(|kt| kt.current.clamp(1e-3, 1.0) as f64)
        .fold(0.0_f64, f64::max);
    let k = 10f64.powf((target_lufs - measured) / 20.0);
    let k_cap = (LEVEL_MAX as f64) / max_cur;
    let k_eff = k.min(k_cap).max(0.0);
    let clamped = k_eff < k - 1e-9;
    let levels = knobs
        .iter()
        .map(|kt| (kt.current as f64 * k_eff).clamp(LEVEL_MIN as f64, LEVEL_MAX as f64) as f32)
        .collect();
    let achieved = measured + 20.0 * k_eff.max(1e-9).log10();
    Ok(JointK {
        levels,
        clamped,
        achieved,
        k_eff,
    })
}

/// One scene's solve outcome.
struct SceneSolve {
    /// Achieved (verified, or predicted when unverified) loudness.
    lufs: f64,
    /// Final per-knob levels, aligned with the job's knobs.
    levels: Vec<f32>,
    /// Target unreachable — ran out of headroom, OR a specific `clamp_reason` applies.
    clamped: bool,
    /// Dynamics spread (LU) of the as-is measure capture.
    spread: f64,
    /// Device writes this scene took (0 = already at target, nothing written).
    writes: u32,
    /// Set with `clamped` for the "no authority" case (off-branch / off-USB amp).
    clamp_reason: Option<String>,
    /// Rebalance "verify by ear" flag; `false` for the plain joint-k path.
    verify_by_ear: bool,
}

/// Max secant CORRECTIONS after the first apply, shared by every re-amp-measured solve
/// (scene `outputLevel` AND footswitch `param` valueA). Each correction is a fresh-connect
/// re-amp capture (~10 s), so this is kept small — 2–3 slope-corrected steps converge any
/// chain with slope ≥ ~0.15 (below that it's the no-authority case), and a large cap would
/// re-inflate per-scene cost toward the legacy 80–93 s regime. (NOT `KNOB_MAX_ITERS`, which
/// counts 2 seed measurements in its budget.)
const MEASURE_CORRECT_MAX: u32 = 3;
/// An `outputLevel` change of at least this many dB that moves the captured loudness by
/// less than `KNOB_TOL_LU` means the amp has no authority over the USB 1/2 capture
/// (off-branch / off-USB output, or hard-limited downstream).
const NO_AUTHORITY_MIN_DB: f64 = 6.0;
/// Rebalance: if a solo lane is within this many dB of the both-muted floor, the muted
/// lane's bleed corrupts the equal-solo balance → flag the scene "verify by ear".
const REBALANCE_BLEED_MARGIN_DB: f64 = 28.0;
/// Sentinel loudness for a both-lanes-muted capture that reads as digital silence — the
/// IDEAL mute (no bleed). `loudest_loudness` errors on silence; this stands in so the
/// solo-above-floor margin is huge (→ no verify-by-ear flag) instead of failing the scene.
const MUTE_FLOOR_SILENT_LUFS: f64 = -120.0;

/// Is this scene already at target? Matches the correction loop's `KNOB_TOL_LU`
/// acceptance band (rather than the tighter ~0.1 dB knob-ratio check this replaces)
/// so a re-run doesn't rewrite an already-in-tolerance scene and re-randomize it.
/// Deliberately skips the corrective pass (`correct_iter`) for a within-tolerance
/// COMPRESSED scene (the UA1176 case below, see `jointk_one_scene`'s doc) — within
/// tolerance is good enough, and a `clamped` solve must still fall through and
/// report clamped even when the measured value happens to sit on target.
fn scene_at_target(measured: f64, target: f64, clamped: bool) -> bool {
    !clamped && (measured - target).abs() <= KNOB_TOL_LU
}

/// Per-scene joint-k: measure the scene AS-IS once, solve one factor `k`, apply it to every
/// lane amp (preserving their mix), VERIFY, then `correct_iter` (bounded secant) to converge
/// through a downstream compressor. The open-loop `20·log10(k)` model is exact for pure gain
/// (±0.07 LU) but UNDERSHOOTS through a compressor/limiter (preset 027's UA1176 → −22.93 vs
/// −22). On a linear chain the first verify is within tol and no correction runs. Shared by
/// `level_scenes_oneshot` and the rebalance flow's non-mergeable scenes. `verify=false` skips
/// both verify and correction.
fn jointk_one_scene(
    slot: u32,
    job: &SceneJob,
    stimulus: &[f32],
    target_lufs: f64,
    defer: bool,
    verify: bool,
) -> Result<SceneSolve, String> {
    // Hard-error on a persistent flat read (after the retry). Trade-off, made
    // consciously: a real scene crushed by a limiter (the UA1176 case below) with
    // spread ≤ the trip gate would false-error — but the library's Base minimum is
    // 0.12 and without the guard a floor read lands on the no-authority clamp path,
    // which mislabels a USB failure as an off-branch amp.
    let loudness = require_live(|| measure_scene_asis(job.scene_slot, stimulus), stimulus)?;
    let (measured, spread) = (loudness.integrated_lufs, loudness.spread_lu());
    let JointK {
        levels,
        clamped,
        achieved,
        k_eff,
    } = solve_joint_k_at(&job.knobs, target_lufs, measured)?;
    // Already at target (within the KNOB_TOL_LU acceptance band) and not clamped →
    // leave every knob untouched (a clamp must still be REPORTED even if nothing moves).
    if scene_at_target(measured, target_lufs, clamped) {
        let currents = job.knobs.iter().map(|kt| kt.current).collect();
        return Ok(SceneSolve {
            lufs: measured,
            levels: currents,
            clamped: false,
            spread,
            writes: 0,
            clamp_reason: None,
            verify_by_ear: false,
        });
    }
    // Scene writes are NEVER saved per apply: `defer` accumulates them unsaved in the
    // working copy (the runner saves ONCE at batch end); `!defer` is the dry-run shape
    // (each apply restores). See `save_deferred_scene_writes`.
    let opts = LevelOptions {
        verify,
        defer,
        ..Default::default()
    };
    let base: Vec<f32> = job.knobs.iter().map(|kt| kt.current).collect();
    let knob_refs: Vec<&LevelKnob> = job.knobs.iter().map(|kt| &kt.knob).collect();
    let expected_db = 20.0 * k_eff.max(1e-9).log10();
    let (v0, retry_writes) = apply_first_verified(
        slot,
        stimulus,
        &knob_refs,
        &levels,
        opts,
        expected_db,
        measured,
    )?;
    let (best_lufs, best_levels, clamp_reason, writes) = match v0 {
        Some(v0) if verify => {
            let c = correct_iter(
                slot,
                stimulus,
                &knob_refs,
                &base,
                levels,
                measured,
                v0,
                target_lufs,
                defer,
            )?;
            (
                c.lufs,
                c.levels,
                c.clamp_reason,
                1 + retry_writes + c.writes,
            )
        }
        _ => (v0.unwrap_or(achieved), levels, None, 1 + retry_writes),
    };
    // Report clamped from the FINAL point, NOT the open-loop's initial want: a specific reason
    // fired, or the verified best still can't reach target (knob out of headroom / chain limits
    // below target). The open-loop `clamped` flag is deliberately DROPPED here — a scene whose
    // first solve wanted `outputLevel > 1.0` but whose verify+correct then landed a valid point
    // within `KNOB_TOL_LU` HAS reached target (it just started far below), so it is "done", not
    // clamped. Keying the flag on `clamped ||` over-reported those as clamped (a stale edge flag,
    // exactly the redistribution's once-clamped-now-rescued scenes).
    let clamped = clamp_reason.is_some() || (best_lufs - target_lufs).abs() > KNOB_TOL_LU;
    Ok(SceneSolve {
        lufs: best_lufs,
        levels: best_levels,
        clamped,
        spread,
        writes,
        clamp_reason,
        verify_by_ear: false,
    })
}

/// A ≥ this intended `outputLevel` move (dB) that reads back ~unchanged (< `KNOB_TOL_LU`)
/// is a suspected DROPPED WRITE, not compression: even 6:1 compression passes ~0.33 LU of a
/// 2 dB move. Below it a flat response is ambiguous with noise, so no retry fires.
const SUSPECT_DROP_MIN_DB: f64 = 2.0;

/// First verified apply with a ONE-SHOT dropped-write retry (scene paths). The device can
/// silently drop a scene write (the ~700 ms post-`loadScene` acceptance window, HW
/// `probe --bisect-scene`); without the retry a single drop reads as a flat response →
/// `correct_iter` sees no slope → a false, non-deterministic "clamped at <as-is>" (HW: the
/// user's first-run Arpeges clamp that succeeded on re-run). One re-apply — a fresh scene
/// recall + write + verify — disambiguates: a drop lands on the retry; a genuine
/// no-authority amp stays flat and takes the honest clamp downstream. Returns
/// `(verify_lufs, retry_writes)`.
fn apply_first_verified(
    slot: u32,
    stimulus: &[f32],
    knobs: &[&LevelKnob],
    levels: &[f32],
    opts: LevelOptions,
    expected_db: f64,
    baseline_lufs: f64,
) -> Result<(Option<f64>, u32), String> {
    let targets: Vec<(&LevelKnob, f32)> = knobs
        .iter()
        .copied()
        .zip(levels)
        .map(|(k, &v)| (k, v))
        .collect();
    let v0 = apply_levels(slot, stimulus, &targets, opts, false)?.1;
    match v0 {
        Some(v)
            if opts.verify
                && expected_db.abs() >= SUSPECT_DROP_MIN_DB
                && (v - baseline_lufs).abs() < KNOB_TOL_LU =>
        {
            std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
            Ok((apply_levels(slot, stimulus, &targets, opts, false)?.1, 1))
        }
        other => Ok((other, 0)),
    }
}

/// Result of the bounded correction loop.
struct Correction {
    lufs: f64,
    levels: Vec<f32>,
    /// `Some` for the no-authority case (the amp doesn't reach the USB 1/2 capture).
    clamp_reason: Option<String>,
    /// Device writes the correction itself performed (iterations + any land-on re-apply).
    writes: u32,
}

/// Bounded secant correction after a first verified apply (shared by joint-k + rebalance).
/// The open-loop `20·log10(k)` solve undershoots through a downstream compressor; this
/// iterates a trust-region-clamped (±`BATCH_TRUST_DB`) secant from the real points until
/// within `KNOB_TOL_LU`, capped at `MEASURE_CORRECT_MAX`, and ALWAYS lands the device on the
/// best point seen — re-applying it if the last write wasn't the best. (Critical: the device
/// working copy holds whatever was LAST written and the batch-end save persists exactly that,
/// so a worse final step would otherwise persist while a better number is reported.) When a
/// large applied gain produced ~no response, it reports
/// NO-AUTHORITY (off-branch / off-USB) and restores `base` rather than leaving the amp slammed.
/// `levels0`/`v0` = the levels the caller already applied (the device holds them) and their
/// verified loudness; `measured0` = loudness at `base`.
#[allow(clippy::too_many_arguments)]
fn correct_iter(
    slot: u32,
    stimulus: &[f32],
    knobs: &[&LevelKnob],
    base: &[f32],
    levels0: Vec<f32>,
    measured0: f64,
    v0: f64,
    target: f64,
    defer: bool,
) -> Result<Correction, String> {
    let max_base = base
        .iter()
        .map(|&x| x.clamp(1e-3, 1.0) as f64)
        .fold(0.0_f64, f64::max);
    let k_cap = (LEVEL_MAX as f64) / max_base;
    let levels_for = |applied_db: f64| -> Vec<f32> {
        let k = 10f64.powf(applied_db / 20.0).clamp(1e-3, k_cap);
        base.iter()
            .map(|&b| (b as f64 * k).clamp(LEVEL_MIN as f64, LEVEL_MAX as f64) as f32)
            .collect()
    };
    let apply = |levels: &[f32], verify: bool| -> Result<Option<f64>, String> {
        let opts = LevelOptions {
            verify,
            defer,
            ..Default::default()
        };
        let targets: Vec<(&LevelKnob, f32)> = knobs
            .iter()
            .copied()
            .zip(levels)
            .map(|(k, &v)| (k, v))
            .collect();
        Ok(apply_levels(slot, stimulus, &targets, opts, false)?.1)
    };

    let k0 = levels0[0] as f64 / (base[0].max(1e-3)) as f64; // shared factor (uniform across lanes)
    let applied_db0 = 20.0 * k0.max(1e-9).log10();
    let mut writes = 0u32; // the device currently holds levels0 (applied by the caller)

    // No-authority: a big applied gain barely moved the capture → the amp isn't on the USB
    // 1/2 path. Restore `base` (don't leave it slammed) and report the distinct reason.
    if no_authority(applied_db0, v0 - measured0) {
        let reason = no_authority_reason(applied_db0 < 0.0);
        apply(base, false)?;
        writes += 1;
        return Ok(Correction {
            lufs: measured0,
            levels: base.to_vec(),
            clamp_reason: Some(reason),
            writes,
        });
    }

    // Already at target, or no applied gain to read a slope from → keep levels0.
    if applied_db0.abs() <= 1e-3 || (v0 - target).abs() <= KNOB_TOL_LU {
        return Ok(Correction {
            lufs: v0,
            levels: levels0,
            clamp_reason: None,
            writes,
        });
    }

    // Bounded secant. Seed points: base@measured0 and levels0@v0 (device at levels0).
    let mut prev = (0.0_f64, measured0); // (applied_db, lufs)
    let mut last = (applied_db0, v0);
    let mut best = (levels0.clone(), v0); // best MEASURED point
    let mut device = levels0; // what the device currently holds
    for _ in 0..MEASURE_CORRECT_MAX {
        if (last.1 - target).abs() <= KNOB_TOL_LU {
            break;
        }
        // Trust-region-clamped secant step (None ⇒ slope too flat / non-finite → stop).
        let Some(next_db) = secant_next_db(prev, last, target) else {
            break;
        };
        let next_levels = levels_for(next_db);
        if next_levels
            .iter()
            .zip(&device)
            .all(|(a, b)| (a - b).abs() <= 1e-3)
        {
            break; // pinned — stepping changes nothing
        }
        let vn = apply(&next_levels, true)?;
        writes += 1;
        device = next_levels.clone();
        let Some(vn) = vn else { break }; // capture failed — land on best below
        if (vn - target).abs() < (best.1 - target).abs() {
            best = (next_levels, vn);
        }
        prev = last;
        last = (next_db, vn);
    }

    // Land on best: persist the best point if the device isn't already there (the
    // apply_levels-saves-the-last-write fix). No verify needed — best.1 is known.
    if device
        .iter()
        .zip(&best.0)
        .any(|(a, b)| (a - b).abs() > 1e-3)
    {
        apply(&best.0, false)?;
        writes += 1;
    }
    Ok(Correction {
        lufs: best.1,
        levels: best.0,
        clamp_reason: None,
        writes,
    })
}

/// Hedged message for a no-authority scene. A DOWNWARD move that gets no response is
/// near-conclusive off-branch (attenuating below any limiter still passes ~1:1); an UPWARD
/// one is ambiguous (a hard limiter saturates identically to an absent path).
fn no_authority_reason(downward: bool) -> String {
    let cause = if downward {
        "it is routed to a different output"
    } else {
        "it is likely routed to a different output (or hard-limited downstream)"
    };
    format!(
        "changing this amp's outputLevel did not move the USB 1/2 capture — {cause}; \
         route it to USB 1/2 or level it manually"
    )
}

/// Pure: trust-region-clamped secant step toward `target` from two real points
/// `prev`/`last` = `(applied_db, lufs)`. Returns the next `applied_db`, clamped to
/// `last.applied_db ± BATCH_TRUST_DB` so a noisy near-zero slope can't explode the jump.
/// `None` when the local slope is non-finite or ≤ 0.05 (no usable response → stop).
fn secant_next_db(prev: (f64, f64), last: (f64, f64), target: f64) -> Option<f64> {
    let slope = (last.1 - prev.1) / (last.0 - prev.0);
    if !slope.is_finite() || slope <= 0.05 {
        return None;
    }
    let raw = last.0 + (target - last.1) / slope;
    Some(raw.clamp(
        last.0 - BATCH_TRUST_DB as f64,
        last.0 + BATCH_TRUST_DB as f64,
    ))
}

/// Pure: a no-authority verdict — a large applied gain (`|applied_db| ≥ NO_AUTHORITY_MIN_DB`)
/// produced almost no loudness `response` (`< KNOB_TOL_LU`), so the knob doesn't reach the
/// captured output. A small applied gain is inconclusive (a headroom clamp), so it's `false`.
fn no_authority(applied_db: f64, response: f64) -> bool {
    applied_db.abs() >= NO_AUTHORITY_MIN_DB && response.abs() < KNOB_TOL_LU
}

/// Fresh-connect, set a SET of knobs (before engage), engage re-amp once, measure the
/// loudest channel on the full capture — the multi-knob `measure_knob_at` used by the
/// rebalance flow to read one lane SOLO (the other muted) and the balanced combination.
fn measure_knobs_at(
    stimulus: &[f32],
    targets: &[(&LevelKnob, f32)],
) -> Result<lufs::Loudness, String> {
    let mut s = Session::connect_lean()?;
    set_knobs(&mut s, targets)?;
    std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SET_MS));
    engage_measure_disengage(&mut s, stimulus)
}

/// Measure the both-lanes-muted FLOOR. `outputLevel`=0 is often DEEP silence — the ideal
/// mute — which `loudest_loudness` reports as "no signal captured"; treat that as a sentinel
/// deep floor (`MUTE_FLOOR_SILENT_LUFS`), the BEST case (no bleed), rather than failing.
/// Other capture errors still propagate.
fn measure_mute_floor(stimulus: &[f32], a: &LevelKnob, b: &LevelKnob) -> Result<f64, String> {
    match measure_knobs_at(stimulus, &[(a, 0.0), (b, 0.0)]) {
        Ok(l) => Ok(l.integrated_lufs),
        Err(e) if e.contains(NO_SIGNAL_CAPTURED) => Ok(MUTE_FLOOR_SILENT_LUFS),
        Err(e) => Err(e),
    }
}

/// READ-ONLY mute-isolation diagnostic for `probe --mute-floor` (rebalance validation).
/// For a 2-amp scene, measures the combined output, the both-lanes-muted FLOOR
/// (`outputLevel`=0 on both), and each lane SOLO (other muted), and reports the
/// solo-above-floor margins. A small margin means `outputLevel`=0 isn't deep silence — the
/// muted lane bleeds into the solo, so the equal-solo rebalance balance is only approximate
/// (the combined joint-k still hits the overall target). NO SAVE (each measure reloads).
pub fn mute_floor_report(
    slot: u32,
    a: &LevelKnob,
    cur_a: f32,
    b: &LevelKnob,
    cur_b: f32,
    stimulus: &[f32],
) -> Result<String, String> {
    {
        let mut s = Session::connect_lean()?;
        s.load_preset(slot)?;
        std::thread::sleep(Duration::from_millis(settle_after_load_ms()));
    }
    let combined = measure_knobs_at(stimulus, &[(a, cur_a), (b, cur_b)])?;
    std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
    let floor_lufs = measure_mute_floor(stimulus, a, b)?;
    std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
    let a_solo = measure_knobs_at(stimulus, &[(a, cur_a), (b, 0.0)])?;
    std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
    let b_solo = measure_knobs_at(stimulus, &[(a, 0.0), (b, cur_b)])?;
    let _ = Session::connect_lean().and_then(|mut s| s.set_reamp_mode(false).map(|_| ()));

    let silent = floor_lufs <= MUTE_FLOOR_SILENT_LUFS + 1e-6;
    let margin_a = a_solo.integrated_lufs - floor_lufs;
    let margin_b = b_solo.integrated_lufs - floor_lufs;
    let min_margin = margin_a.min(margin_b);
    let flag = !silent && min_margin < REBALANCE_BLEED_MARGIN_DB;
    let floor_disp = if silent {
        "silent (no signal — ideal mute)".to_string()
    } else {
        format!("{floor_lufs:.2} LUFS")
    };
    Ok(format!(
        "mute-floor (list index {slot})\n\
         combined (both at current): {:.2} LUFS\n\
         both muted (outputLevel=0):  {floor_disp}  ← floor\n\
         lane A solo: {:.2} LUFS\n\
         lane B solo: {:.2} LUFS\n\
         {}verify_by_ear = {flag}\n",
        combined.integrated_lufs,
        a_solo.integrated_lufs,
        b_solo.integrated_lufs,
        if silent {
            "floor is digital silence → no lane bleed → ".to_string()
        } else {
            format!(
                "min solo-above-floor margin {min_margin:.1} dB {} threshold {REBALANCE_BLEED_MARGIN_DB:.0} dB → ",
                if flag { "<" } else { ">=" }
            )
        },
    ))
}

/// Balanced per-lane levels for EQUAL solo loudness given each lane's solo ceiling `C`
/// (its captured loudness at level 1.0): the quieter-ceiling lane is pinned at 1.0 and
/// the louder lane is attenuated to match, so both stay ≤ 1.0. The absolute level is then
/// set by the joint-k pass over the combined capture, so the choice of equal point
/// (= the quieter ceiling) is just the max-headroom anchor. Pure → unit-testable.
fn balanced_solo_levels(c_a: f64, c_b: f64) -> (f32, f32) {
    let equal_point = c_a.min(c_b);
    let la = 10f64.powf((equal_point - c_a) / 20.0).clamp(0.0, 1.0) as f32;
    let lb = 10f64.powf((equal_point - c_b) / 20.0).clamp(0.0, 1.0) as f32;
    (la, lb)
}

/// OPT-IN rebalance leveling (only on a path MERGE, never on separate outputs).
/// For each `rebalanceable` scene (≥2 lane amps that re-merge), it first equalizes the
/// two lanes' SOLO loudness (mute one, measure the other — 2 isolated captures), then
/// joint-ks the balanced pair to the target (1 combined measure + apply). Non-rebalanceable
/// scenes (series / single / split-output) fall through to the plain joint-k (`jointk_one_scene`).
/// Same signature + outcome shape as `level_scenes_oneshot`, so the command path swaps in.
///
/// NOTE (HW-UNVALIDATED): muting a lane via `outputLevel`=0 assumes 0 is true silence and
/// that the write lands on the per-scene overlay (not the base). Validate `probe --rebalance`
/// before trusting the equal-solo balance; the final combined joint-k still hits the target
/// either way. Restores via preset reload on no-save; ends with a guaranteed re-amp OFF.
pub fn level_scenes_rebalance(
    slot: u32,
    jobs: &[SceneJob],
    stimulus: &[f32],
    save: bool,
    restore_scene: Option<u32>,
    on_scene: impl FnMut(u32, Option<&BatchedSceneOutcome>),
    cancelled: impl FnMut() -> bool,
) -> Result<Vec<BatchedSceneOutcome>, String> {
    let result = run_scene_jobs(
        slot,
        jobs,
        save,
        restore_scene,
        on_scene,
        cancelled,
        |job| {
            let eff_target = job.target_lufs;
            // Non-mergeable scenes: plain joint-k (nothing to rebalance), self-correcting.
            if !job.rebalanceable || job.knobs.len() < 2 {
                jointk_one_scene(slot, job, stimulus, eff_target, save, true)
            } else {
                // Rebalanceable: 2-lane equalize → joint-k. (Only the first two knobs are the
                // rebalance pair; the classifier never produces >2 for a single split.)
                rebalance_one_scene(slot, job, stimulus, eff_target, save, true)
            }
        },
    );
    restore_after_unsaved_error(slot, save, result)
}

/// The rebalance flow for ONE mergeable scene: equalize the two lanes' solo loudness, then
/// joint-k the balanced pair to target. Returns a [`SceneSolve`] like `jointk_one_scene`,
/// plus a `verify_by_ear` flag when the lane-mute floor is too shallow to trust the balance.
fn rebalance_one_scene(
    slot: u32,
    job: &SceneJob,
    stimulus: &[f32],
    target_lufs: f64,
    defer: bool,
    verify: bool,
) -> Result<SceneSolve, String> {
    let a = &job.knobs[0];
    let b = &job.knobs[1];
    let cur_a = a.current.clamp(1e-3, 1.0);
    let cur_b = b.current.clamp(1e-3, 1.0);

    // 1+2. Each lane SOLO (the other muted to 0) at its current level → solo ceiling C.
    // Solo captures feed the per-lane model constants (c_a/c_b) with no verify
    // backstop downstream — floor-guarded like the combined measurement below.
    let la_solo = require_live(
        || measure_knobs_at(stimulus, &[(&a.knob, cur_a), (&b.knob, 0.0)]),
        stimulus,
    )?;
    std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
    let lb_solo = require_live(
        || measure_knobs_at(stimulus, &[(&a.knob, 0.0), (&b.knob, cur_b)]),
        stimulus,
    )?;
    std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
    let c_a = la_solo.integrated_lufs - 20.0 * (cur_a as f64).log10();
    let c_b = lb_solo.integrated_lufs - 20.0 * (cur_b as f64).log10();

    // 2b. Mute FLOOR: BOTH lanes at 0. `outputLevel`=0 may floor near ~−40 dB (not −∞), so a
    // "solo" carries the muted lane's bleed when the lanes sit within ~`REBALANCE_BLEED_MARGIN_DB`.
    // If so, the equal-solo balance is only approximate (the combined joint-k still hits the
    // overall target) → flag the scene "verify by ear". One extra capture; rebalance is opt-in.
    // A SILENT floor (deep mute) is the best case → huge margin → no flag.
    let floor_lufs = measure_mute_floor(stimulus, &a.knob, &b.knob)?;
    std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
    let min_solo = la_solo.integrated_lufs.min(lb_solo.integrated_lufs);
    let verify_by_ear = (min_solo - floor_lufs) < REBALANCE_BLEED_MARGIN_DB;

    // 3. Balanced levels for equal solo loudness.
    let (la_bal, lb_bal) = balanced_solo_levels(c_a, c_b);

    // 4. Measure the COMBINED output at the balanced levels (correlation-real sum).
    // Floor-guarded: both lanes live at balanced levels must produce a lively capture
    // (the DELIBERATE floor measurement above is measure_mute_floor — never guarded).
    let combined = require_live(
        || measure_knobs_at(stimulus, &[(&a.knob, la_bal), (&b.knob, lb_bal)]),
        stimulus,
    )?;
    std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
    let spread = combined.spread_lu();

    // 5. Joint-k the balanced pair to target (scale both by one k from the combined point).
    let balanced_knobs = vec![
        KnobTarget {
            knob: a.knob.clone(),
            lo: a.lo,
            hi: a.hi,
            current: la_bal,
        },
        KnobTarget {
            knob: b.knob.clone(),
            lo: b.lo,
            hi: b.hi,
            current: lb_bal,
        },
    ];
    let JointK {
        levels,
        clamped: _, // the FINAL point decides clamped (see below), not the open-loop want
        achieved,
        k_eff,
    } = solve_joint_k_at(&balanced_knobs, target_lufs, combined.integrated_lufs)?;

    // 6. Apply the final balanced+scaled levels (reload discards the temporary mutes), then the
    // bounded secant correction — the balanced pair feeds the same downstream chain (e.g. a
    // post-merge compressor), so the open-loop solve undershoots there exactly like joint-k.
    let opts = LevelOptions {
        verify,
        defer,
        ..Default::default()
    };
    let knob_refs = [&a.knob, &b.knob];
    let base = [la_bal, lb_bal];
    let expected_db = 20.0 * k_eff.max(1e-9).log10();
    let (v0, retry_writes) = apply_first_verified(
        slot,
        stimulus,
        &knob_refs,
        &levels,
        opts,
        expected_db,
        combined.integrated_lufs,
    )?;
    let (best_lufs, best_levels, clamp_reason, corr_writes) = match v0 {
        Some(v0) if verify => {
            let c = correct_iter(
                slot,
                stimulus,
                &knob_refs,
                &base,
                levels,
                combined.integrated_lufs,
                v0,
                target_lufs,
                defer,
            )?;
            (c.lufs, c.levels, c.clamp_reason, c.writes)
        }
        _ => (v0.unwrap_or(achieved), levels, None, 0),
    };
    // Clamped from the FINAL point, not the open-loop want (see `jointk_one_scene`): a
    // verify+correct that landed within tolerance means the lane reached target — "done".
    let clamped = clamp_reason.is_some() || (best_lufs - target_lufs).abs() > KNOB_TOL_LU;
    Ok(SceneSolve {
        lufs: best_lufs,
        levels: best_levels,
        clamped,
        spread,
        writes: 1 + retry_writes + corr_writes,
        clamp_reason,
        verify_by_ear,
    })
}

/// Build a successful per-scene outcome from a [`SceneSolve`] (joint-k / rebalance share this).
fn solved_scene_outcome(
    scene_slot: u32,
    target_lufs: f64,
    s: SceneSolve,
    elapsed_ms: u128,
) -> BatchedSceneOutcome {
    BatchedSceneOutcome {
        scene_slot,
        target_lufs,
        final_lufs: Some(s.lufs),
        // The loudest lane amp's solved value (representative for the single-knob case; the
        // meaningful number for a multi-knob scene is `final_lufs`). All lanes share `k_eff`.
        final_level: s
            .levels
            .iter()
            .copied()
            .fold(None, |m, v| Some(m.map_or(v, |mx: f32| mx.max(v)))),
        clamped: s.clamped,
        windows: 1,
        writes: s.writes,
        elapsed_ms,
        failure: None,
        dynamic_spread_lu: Some(s.spread),
        clamp_reason: s.clamp_reason,
        verify_by_ear: s.verify_by_ear,
    }
}

/// Build a failed/skipped per-scene outcome.
fn failed_scene_outcome(
    scene_slot: u32,
    target_lufs: f64,
    failure: String,
    elapsed_ms: u128,
) -> BatchedSceneOutcome {
    BatchedSceneOutcome {
        scene_slot,
        target_lufs,
        final_lufs: None,
        final_level: None,
        clamped: false,
        windows: 0,
        writes: 0,
        elapsed_ms,
        failure: Some(failure),
        dynamic_spread_lu: None,
        clamp_reason: None,
        verify_by_ear: false,
    }
}

/// Level `slot` to `target_lufs` by driving `knob` in a closed loop within
/// `[lo, hi]`. Loads the preset once (own connection), seeds two measurements,
/// then secant-iterates (each a fresh re-amp capture) until within `KNOB_TOL_LU`
/// or `KNOB_MAX_ITERS`. Self-contained: opens its own connections, so the caller
/// must NOT hold a device seize. `clamped` = the target needed a knob value
/// outside `[lo, hi]` (unreachable). Optionally verifies and saves.
#[allow(clippy::too_many_arguments)]
pub fn level_preset_block(
    slot: u32,
    stimulus: &[f32],
    knob: &LevelKnob,
    lo: f32,
    hi: f32,
    target_lufs: f64,
    opts: LevelOptions,
    mut cancelled: impl FnMut() -> bool,
) -> Result<LevelResult, String> {
    // Pre-measure cancel: no device touch yet → return without the restore wrapper.
    if cancelled() {
        return Err(CANCELLED.to_string());
    }
    let result = (|| {
        if hi <= lo {
            return Err(format!("invalid knob bounds [{lo}, {hi}]"));
        }
        // Load the preset in its own connection (the set-after-load-in-same-conn
        // override applies to any setter, so isolate the load).
        {
            let mut s = Session::connect_lean()?;
            s.load_preset(slot)?;
            std::thread::sleep(Duration::from_millis(settle_after_load_ms()));
        }
        std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));

        // Search in a coordinate where the knob is ~linear in LUFS so the secant
        // converges in 1–2 steps. Amplitude knobs (range within [0,1]) are linear in
        // dB-of-knob (`20·log10(x)` — the de-risk's proven `presetLevel`/`outputLevel`
        // model); dB-unit knobs (e.g. an IR `outputlevel`) are already ~linear, so
        // search them in raw units.
        let log_space = lo >= 0.0 && hi <= 1.0 + 1e-6;
        let eps = 1e-3f32;
        let to_c = |x: f32| {
            if log_space {
                20.0 * x.max(eps).log10()
            } else {
                x
            }
        };
        let from_c = |c: f32| {
            if log_space {
                10f32.powf(c / 20.0).clamp(lo, hi)
            } else {
                c.clamp(lo, hi)
            }
        };
        let c_lo = to_c(if log_space { lo.max(eps) } else { lo });
        let c_hi = to_c(hi);
        let cspan = c_hi - c_lo;

        // Seed two points inside the range (avoid the extremes), in c-space.
        let mut ca = c_lo + 0.4 * cspan;
        let mut cb = c_lo + 0.75 * cspan;
        if cancelled() {
            return Err(CANCELLED.to_string());
        }
        // Floor-guarded: the first capture characterizes the preset (spread is
        // gain-invariant), so a floor read here poisons the whole secant loop.
        // Mid-loop captures (`yb`, `ynext`) stay UNGUARDED on purpose: a preset that
        // legitimately measures near the trip gate would pay the retry on EVERY
        // iteration, and a persistent mid-loop floor lands on the secant's
        // flat-response / no-authority backstops instead of a wrong write.
        let first = require_live(
            || measure_knob_at(stimulus, knob, from_c(ca), &[]),
            stimulus,
        )?;
        let dynamic_spread_lu = first.spread_lu();
        let mut ya = first.integrated_lufs;
        if cancelled() {
            return Err(CANCELLED.to_string());
        }
        std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
        let mut yb = measure_knob_at(stimulus, knob, from_c(cb), &[])?.integrated_lufs;
        let mut iterations = 2u32;

        // Track the best (closest-to-target) measured point as the result.
        let mut best = if (ya - target_lufs).abs() <= (yb - target_lufs).abs() {
            (ca, ya)
        } else {
            (cb, yb)
        };

        while iterations < KNOB_MAX_ITERS && (best.1 - target_lufs).abs() > KNOB_TOL_LU {
            let Some(raw_next) = secant_next(ca, ya, cb, yb, target_lufs) else {
                break; // flat response — knob can't move loudness here
            };
            let cnext = raw_next.clamp(c_lo, c_hi);
            // If the secant keeps pinning us to the same bound, we've converged to
            // the reachable extreme — stop.
            if (cnext - cb).abs() < 1e-4 {
                break;
            }
            if cancelled() {
                return Err(CANCELLED.to_string());
            }
            std::thread::sleep(Duration::from_millis(RECONNECT_GAP_MS));
            let ynext = measure_knob_at(stimulus, knob, from_c(cnext), &[])?.integrated_lufs;
            iterations += 1;
            if (ynext - target_lufs).abs() < (best.1 - target_lufs).abs() {
                best = (cnext, ynext);
            }
            (ca, ya, cb, yb) = (cb, yb, cnext, ynext);
        }

        let (final_level, measured_at_final) = (from_c(best.0), best.1);
        // "Clamped" = target not achieved within tolerance — the knob couldn't reach
        // it, whether because it hit a bound or its response went flat (e.g. a
        // normalvolume whose channel can't go quiet enough). This is the
        // user-meaningful "target unreachable with this knob" signal.
        let clamped = (measured_at_final - target_lufs).abs() > KNOB_TOL_LU;

        // Apply the solved value + optional verify/save. The preset is still current
        // from the initial load, so no reload — the same Conn-3 seam the one-shot path
        // uses, just with a block knob.
        if cancelled() {
            return Err(CANCELLED.to_string());
        }
        let (saved, verify_lufs) = apply_level(slot, stimulus, knob, final_level, opts, false)?;

        Ok(LevelResult {
            slot,
            ref_level: final_level, // for a block knob, "ref" carries the solved value
            measured_lufs: measured_at_final,
            constant_c: f64::NAN, // no single-constant model for an arbitrary knob
            final_level,
            target_lufs,
            predicted_lufs: measured_at_final,
            clamped,
            saved,
            verify_lufs,
            iterations,
            dynamic_spread_lu: Some(dynamic_spread_lu),
            clamp_reason: None,
            verify_by_ear: false,
            previous_level: None,
            true_peak_dbtp: None,
        })
    })();
    restore_after_unsaved_error(slot, opts.save, result)
}

#[cfg(test)]
mod floor_guard_tests {
    use super::*;

    fn loud(integrated: f64, spread: f64) -> lufs::Loudness {
        lufs::Loudness {
            integrated_lufs: integrated,
            short_term_max_lufs: integrated + spread,
            true_peak_dbtp: integrated + 12.0,
        }
    }

    // The trip gate sits BELOW the measured real-preset minimum (0.12 LU) and is
    // DISARMED for a near-stationary stimulus (EBow-heavy captures legitimately
    // produce near-zero output spread — the level-shift confirm discriminates there).
    #[test]
    fn floor_suspect_trips_only_below_gate_with_lively_stimulus() {
        assert!(floor_suspect(0.01, 1.5)); // classic floor read
        assert!(floor_suspect(FLOOR_TRIP_LU, 1.5)); // boundary inclusive
        assert!(!floor_suspect(0.12, 1.5)); // real library minimum stays clear
        assert!(!floor_suspect(0.01, 0.2)); // stationary stimulus disarms the trip
    }

    // Real signal tracks a presetLevel shift by 20·log10 (linear post-chain gain);
    // the output floor doesn't move. Tolerance absorbs run-to-run noise.
    #[test]
    fn level_shift_tracking_discriminates_floor_from_compressed() {
        // ref 0.5 → confirm 0.25: expected Δ = −6.02 LU.
        assert!(tracks_level_shift(-30.0, -36.0, 0.5, 0.25)); // tracks
        assert!(!tracks_level_shift(-30.18, -30.20, 0.5, 0.25)); // floor: flat
        assert!(tracks_level_shift(-30.0, -34.1, 0.5, 0.25)); // inside ±2 LU
        assert!(!tracks_level_shift(-30.0, -33.9, 0.5, 0.25)); // outside ±2 LU
    }

    // The confirm level must stay distinguishable from the reference: halve, unless
    // halving hits the 0.05 clamp — then double instead.
    #[test]
    fn confirm_level_is_distinguishable() {
        assert!((confirm_ref_level(0.5) - 0.25).abs() < 1e-6);
        assert!((confirm_ref_level(1.0) - 0.5).abs() < 1e-6);
        assert!((confirm_ref_level(0.08) - 0.16).abs() < 1e-6);
    }

    // Predicted true peak scales with the same 20·log10(ratio) as presetLevel itself
    // (linear post-chain gain) — halving the level should drop the predicted peak
    // ~6.02 dB, matching a level unchanged from ref keeps the ref peak verbatim.
    #[test]
    fn predicted_true_peak_scales_with_level_ratio() {
        assert!((predicted_true_peak_dbtp(-3.0, 0.5, 0.5) - -3.0).abs() < 1e-6);
        let halved = predicted_true_peak_dbtp(-3.0, 0.5, 0.25);
        assert!((halved - -9.02).abs() < 0.01, "got {halved}");
        let doubled = predicted_true_peak_dbtp(-9.0, 0.25, 0.5);
        assert!((doubled - -2.98).abs() < 0.01, "got {doubled}");
    }

    // The guarded wrapper: one same-settings retry heals a transient inject failure;
    // a persistent flat read is reported as StillFlat (callers decide: scenes error,
    // measure_c escalates to the level-shift confirm).
    #[test]
    fn guarded_measure_retries_once_then_reports_still_flat() {
        let lively = loud(-30.0, 5.0);
        let flat = loud(-30.18, 0.01);

        // First capture lively → no retry.
        let mut calls = 0;
        let out = measure_floor_guarded(
            || {
                calls += 1;
                Ok(lively)
            },
            1.5,
            Duration::ZERO,
        )
        .unwrap();
        assert!(matches!(out, GuardOutcome::Live(_)));
        assert_eq!(calls, 1);

        // Transient failure: flat then lively → Live, two calls.
        let mut calls = 0;
        let out = measure_floor_guarded(
            || {
                calls += 1;
                Ok(if calls == 1 { flat } else { lively })
            },
            1.5,
            Duration::ZERO,
        )
        .unwrap();
        assert!(matches!(out, GuardOutcome::Live(_)));
        assert_eq!(calls, 2);

        // Persistent flat → StillFlat after exactly one retry.
        let mut calls = 0;
        let out = measure_floor_guarded(
            || {
                calls += 1;
                Ok(flat)
            },
            1.5,
            Duration::ZERO,
        )
        .unwrap();
        assert!(matches!(out, GuardOutcome::StillFlat(_)));
        assert_eq!(calls, 2);

        // Stationary stimulus disarms the guard entirely: flat first capture passes.
        let mut calls = 0;
        let out = measure_floor_guarded(
            || {
                calls += 1;
                Ok(flat)
            },
            0.2,
            Duration::ZERO,
        )
        .unwrap();
        assert!(matches!(out, GuardOutcome::Live(_)));
        assert_eq!(calls, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_stim_slice_truncates_then_pads_with_leading_silence() {
        let src = vec![0.7f32; doctor_stim_samples() * 2];
        let prepared = doctor_stim_slice(src);
        assert_eq!(prepared.len(), doctor_pad_samples() + doctor_stim_samples());
        assert!(prepared[..doctor_pad_samples()].iter().all(|&s| s == 0.0));
        assert!(prepared[doctor_pad_samples()..].iter().all(|&s| s == 0.7));
        // Signal start accounting: confident onset = latency + the pad; an
        // unconfident onset keeps the legacy whole-buffer 0.
        assert_eq!(doctor_signal_start(1536, true), 1536 + doctor_pad_samples());
        assert_eq!(doctor_signal_start(1536, false), 0);
    }

    // A cancel flag already set at entry must bail at the PRE-MEASURE checkpoint —
    // before any `Session::connect`/device touch — so this runs with no hardware and
    // returns the CANCELLED sentinel. Guards both the master-level and block-knob paths.
    #[test]
    fn cancel_before_measure_short_circuits_without_device() {
        let stim = [0.0f32; 16];
        let opts = LevelOptions::default();
        assert_eq!(
            level_preset(0, &stim, -30.0, opts, &[], None, || true).unwrap_err(),
            CANCELLED
        );
        let knob = LevelKnob::PresetLevel;
        assert_eq!(
            level_preset_block(0, &stim, &knob, 0.05, 1.0, -30.0, opts, || true).unwrap_err(),
            CANCELLED
        );
    }

    // Restore identity guard: passes on the recorded name, fails loudly on a
    // renamed/moved slot or a slot that left the list (slot ≠ identity).
    #[test]
    fn restore_verify_slot_name_guards_drift() {
        let entry = |slot: u32, name: &str| crate::session::PresetEntry {
            slot,
            name: name.to_string(),
        };
        let list = [entry(0, "Clean Twin"), entry(1, "Cello")];
        assert!(verify_slot_name(&list, 1, "Cello").is_ok());
        let e = verify_slot_name(&list, 1, "Synth").unwrap_err();
        assert!(e.contains("not restoring") && e.contains("Cello"), "{e}");
        let e = verify_slot_name(&list, 7, "Cello").unwrap_err();
        assert!(e.contains("no longer in the preset list"), "{e}");
    }

    // Footswitch generic param-space secant: hits a linear response, gives up on a flat one.
    #[test]
    fn fs_secant_converges_and_detects_flat() {
        // loudness(v) = 10·v − 30 → target −23 ⇒ v = 0.7.
        let f = |v: f64| 10.0 * v - 30.0;
        let next = fs_secant_next((0.25, f(0.25)), (0.75, f(0.75)), -23.0).unwrap();
        assert!((next - 0.70).abs() < 1e-9, "got {next}");
        // Flat response → None (no authority).
        assert!(fs_secant_next((0.25, -9.0), (0.75, -9.0), -23.0).is_none());
    }

    #[test]
    fn param_fn_present_matches_switch_index_and_param() {
        let ftsw = serde_json::json!([
            [],
            [{ "func": "on-off" }, { "func": "param", "parameterId": "gain" }],
        ]);
        assert!(param_fn_present(&ftsw, 1, 1, "gain"));
        assert!(!param_fn_present(&ftsw, 1, 1, "level")); // wrong param
        assert!(!param_fn_present(&ftsw, 1, 0, "gain")); // index 0 is on-off
        assert!(!param_fn_present(&ftsw, 0, 0, "gain")); // empty switch
    }

    /// Solve for `C` from a real on-device data point: -26.70 LUFS at ref 0.5.
    /// C is the captured loudness at level=1.0 (20·log10(1)=0), i.e. this
    /// preset's MAX achievable captured loudness ≈ -20.68 LUFS.
    fn c_from_real_data() -> f64 {
        let (measured, ref_level) = (-26.70f64, 0.5f64);
        measured - 20.0 * ref_level.log10()
    }

    #[test]
    fn reachable_target_hits_exactly() {
        let c = c_from_real_data();
        assert!((c - (-20.68)).abs() < 0.1, "C={c}");
        // -30 is quieter than C → reachable. The model at the computed level
        // returns the target exactly; the level lands near the on-device point
        // (0.3225 measured -30.51, so -30 needs slightly more: ~0.342).
        let target = -30.0f64;
        let level = (10f64.powf((target - c) / 20.0)).clamp(0.0, 1.0);
        assert!((level - 0.342).abs() < 0.005, "level={level}");
        let back = 20.0 * level.log10() + c;
        assert!((back - target).abs() < 1e-6, "got {back}");
    }

    #[test]
    fn target_louder_than_max_clamps() {
        let c = c_from_real_data(); // ≈ -20.68 = max achievable
        let target = -16.0f64; // louder than the preset can reach
        let ideal = 10f64.powf((target - c) / 20.0);
        assert!(ideal > 1.0, "ideal={ideal}");
        assert_eq!((ideal as f32).clamp(0.0, 1.0), 1.0);
    }

    #[test]
    fn solve_level_reachable_and_clamped() {
        let c = c_from_real_data();
        // Reachable target → exact, not clamped.
        let (lvl, clamped, predicted) = super::solve_level(c, -30.0);
        assert!(!clamped);
        assert!((lvl - 0.342).abs() < 0.005, "lvl={lvl}");
        assert!((predicted - (-30.0)).abs() < 1e-4, "predicted={predicted}");
        // Target louder than C → clamps at 1.0 and predicts C (the ceiling).
        let (lvl2, clamped2, predicted2) = super::solve_level(c, -16.0);
        assert!(clamped2);
        assert_eq!(lvl2, 1.0);
        assert!((predicted2 - c).abs() < 1e-9, "predicted2={predicted2}");
    }

    #[test]
    fn redistribute_delta_is_min_of_deficit_headroom_and_downroom() {
        use super::redistribute_delta_db;
        // pl=0.5 → 6.02 dB headroom; min knob 0.5 → 20 dB down-room; deficit 3 + 1 margin = 4
        // (deficit-bound, well under headroom/down-room).
        assert!((redistribute_delta_db(0.5, 3.0, 0.5) - 4.0).abs() < 1e-9);
        // pl=0.9 → 0.915 dB headroom binds (deficit+margin=4, but no room to raise past ceiling).
        assert!((redistribute_delta_db(0.9, 3.0, 0.5) - 0.9151).abs() < 1e-3);
        // min knob 0.06 → 20·log10(0.06/0.05)=1.584 dB down-room binds.
        assert!((redistribute_delta_db(0.5, 3.0, 0.06) - 1.5836).abs() < 1e-3);
        // A knob already at/below the floor → no room → 0 (don't offer).
        assert_eq!(redistribute_delta_db(0.5, 3.0, 0.05), 0.0);
        // No clamp deficit → 0 (the margin never fires without a real deficit).
        assert_eq!(redistribute_delta_db(0.5, 0.0, 0.5), 0.0);
        // pl at ceiling (1.0) → no headroom → 0 (the quiet class PR6 owns).
        assert_eq!(redistribute_delta_db(1.0, 3.0, 0.5), 0.0);
    }

    // ── joint-k (parallel-merged) solve ──────────────────────────────────────
    fn amp_knob(current: f32) -> super::KnobTarget {
        super::KnobTarget {
            knob: super::LevelKnob::Block {
                group_id: "G1".into(),
                node_id: "ACD_X".into(),
                parameter_id: "outputLevel".into(),
                scene_slot: Some(0),
            },
            lo: 0.0,
            hi: 1.0,
            current,
        }
    }

    // Single amp: joint-k degenerates to the validated one-amp solve.
    #[test]
    fn joint_k_single_amp_hits_target() {
        let j = super::solve_joint_k_at(&[amp_knob(0.5)], -30.0, -26.0).unwrap();
        assert!(!j.clamped);
        assert!(
            (j.achieved - (-30.0)).abs() < 1e-6,
            "achieved={}",
            j.achieved
        );
        assert!(
            (j.levels[0] - 0.3155).abs() < 0.002,
            "level={}",
            j.levels[0]
        );
    }

    // Two equal lanes summing to `measured`: each scaled by the same k → target hit,
    // balance (equal) preserved.
    #[test]
    fn joint_k_two_equal_amps_scale_together() {
        let j = super::solve_joint_k_at(&[amp_knob(0.5), amp_knob(0.5)], -26.0, -20.0).unwrap();
        assert!(!j.clamped);
        assert!(
            (j.achieved - (-26.0)).abs() < 1e-6,
            "achieved={}",
            j.achieved
        );
        assert_eq!(j.levels.len(), 2);
        assert!(
            (j.levels[0] - j.levels[1]).abs() < 1e-6,
            "balance preserved"
        );
        assert!(
            (j.levels[0] - 0.2505).abs() < 0.002,
            "level={}",
            j.levels[0]
        );
    }

    // Unequal lanes, boost beyond the hottest amp's headroom → ratio-preserving clamp:
    // hottest hits 1.0, the other scales by the SAME k_eff (mix intact), `clamped` set,
    // `achieved` reports the shortfall (NOT the target).
    #[test]
    fn joint_k_unequal_clamp_preserves_ratio() {
        let j = super::solve_joint_k_at(&[amp_knob(0.9), amp_knob(0.3)], -18.0, -30.0).unwrap();
        assert!(j.clamped);
        assert!(
            (j.levels[0] - 1.0).abs() < 1e-4,
            "hottest pinned at 1.0: {}",
            j.levels[0]
        );
        let ratio = j.levels[0] / j.levels[1];
        assert!(
            (ratio - 3.0).abs() < 1e-3,
            "0.9:0.3 ratio preserved, got {ratio}"
        );
        assert!(
            j.achieved < -28.0,
            "achieved reports the shortfall: {}",
            j.achieved
        );
        assert!(j.achieved > -30.0);
    }

    // At target already → k_eff ≈ 1, not clamped. `jointk_one_scene`'s caller-side
    // skip no longer keys off k_eff though — it compares `measured` vs `target_lufs`
    // directly via `scene_at_target` (the KNOB_TOL_LU band), which a unity k_eff implies.
    #[test]
    fn joint_k_at_target_is_unity_unclamped() {
        let j = super::solve_joint_k_at(&[amp_knob(0.5), amp_knob(0.2)], -30.0, -30.0).unwrap();
        assert!(!j.clamped);
        assert!((j.k_eff - 1.0).abs() < 1e-6, "k_eff={}", j.k_eff);
    }

    // A dB-unit knob can't be scaled multiplicatively → error, never a garbage write.
    #[test]
    fn joint_k_rejects_db_knob() {
        let mut kt = amp_knob(0.5);
        kt.lo = -18.0;
        kt.hi = 6.0;
        assert!(super::solve_joint_k_at(&[kt], -30.0, -26.0).is_err());
    }

    // Rebalance: equal-ceiling lanes both sit at 1.0; a louder lane is attenuated to match
    // the quieter (which pins at 1.0), and both stay ≤ 1.0 — equal SOLO loudness.
    #[test]
    fn balanced_solo_levels_equalizes_lanes() {
        let (la, lb) = super::balanced_solo_levels(-20.0, -20.0);
        assert!(
            (la - 1.0).abs() < 1e-6 && (lb - 1.0).abs() < 1e-6,
            "equal → both 1.0"
        );

        // A louder (C=-15) than B (C=-21): B pins at 1.0, A attenuates to 10^(-6/20)≈0.501.
        let (la, lb) = super::balanced_solo_levels(-15.0, -21.0);
        assert!((lb - 1.0).abs() < 1e-6, "quieter lane B at 1.0, got {lb}");
        assert!(
            (la - 0.501).abs() < 0.005,
            "louder lane A attenuated, got {la}"
        );
        // Equal solo loudness check: 20·log10(la)+C_a ≈ 20·log10(lb)+C_b.
        let solo_a = 20.0 * (la as f64).log10() + (-15.0);
        let solo_b = 20.0 * (lb as f64).log10() + (-21.0);
        assert!(
            (solo_a - solo_b).abs() < 0.05,
            "solo loudness equal: {solo_a} vs {solo_b}"
        );
    }

    // AC1 — the common target is min(C) − headroom (the loudest level every
    // preset can still reach), and empty input yields None.
    #[test]
    fn common_target_is_min_c_minus_headroom() {
        let cs = [-22.0, -25.5, -19.0]; // quietest ceiling is -25.5
        let t = super::common_target(&cs, 2.0).unwrap();
        assert!((t - (-27.5)).abs() < 1e-9, "t={t}");
        // A target equal to min(C) (headroom 0) is reachable by the quietest; a
        // louder target would clamp that preset (solve_level flags it).
        assert!(super::common_target(&[], 2.0).is_none());
    }

    #[test]
    fn secant_next_solves_linear_response() {
        // A perfectly linear knob: lufs = 10*x - 25. Target -20 ⇒ x = 0.5.
        let f = |x: f32| 10.0 * x as f64 - 25.0;
        let x = super::secant_next(0.2, f(0.2), 0.8, f(0.8), -20.0).unwrap();
        assert!((x - 0.5).abs() < 1e-4, "x={x}");
    }

    #[test]
    fn secant_next_none_on_flat_response() {
        // A knob that doesn't move loudness → no solution.
        assert!(super::secant_next(0.2, -20.0, 0.8, -20.0, -18.0).is_none());
    }

    // Item 3 — no-authority verdict: a LARGE applied gain that barely moves loudness means
    // the amp is off-branch; a small gain is inconclusive (headroom clamp), and a real
    // response means the amp has authority.
    #[test]
    fn no_authority_flags_dead_knob_only() {
        assert!(
            super::no_authority(12.0, 0.10),
            "big boost, no response → off-branch"
        );
        assert!(
            super::no_authority(-9.0, -0.05),
            "big cut, no response → off-branch"
        );
        assert!(
            !super::no_authority(12.0, 6.0),
            "big gain, real response → has authority"
        );
        assert!(
            !super::no_authority(2.0, 0.05),
            "small gain → inconclusive (headroom clamp)"
        );
        assert!(
            !super::no_authority(0.0, 0.0),
            "no gain applied → not no-authority"
        );
    }

    // Item 1 — the secant step is trust-region-clamped: a shallow (but > 0.05) slope with a
    // big residual must NOT explode the Newton jump; it caps at ±BATCH_TRUST_DB.
    #[test]
    fn secant_next_db_trust_region_caps_jump() {
        let prev = (0.0, -30.0);
        let last = (1.0, -29.7); // slope 0.3 over 1 dB; raw jump ≈ +32 dB
        let next = super::secant_next_db(prev, last, -20.0).unwrap();
        assert!(
            (next - (last.0 + super::BATCH_TRUST_DB as f64)).abs() < 1e-9,
            "step clamped to +{} dB, got {next}",
            super::BATCH_TRUST_DB
        );
    }

    #[test]
    fn secant_next_db_none_on_flat_slope() {
        // slope ≈ 0.0017 ≤ 0.05 → no usable response (→ the loop stops / no-authority path).
        assert!(super::secant_next_db((0.0, -30.0), (6.0, -29.99), -20.0).is_none());
    }

    // Item 1 — the bounded secant converges on a SATURATING (compressor-like) response where
    // the open-loop slope-1 first apply overshoots and one step would still miss, within
    // MEASURE_CORRECT_MAX steps, honoring the trust region. Mirrors `correct_iter`'s loop.
    #[test]
    fn correct_iter_secant_converges_on_compressor() {
        let l0 = -30.0_f64;
        let (g, tau) = (15.0_f64, 8.0_f64); // saturating: dB-out/dB-in slope < 1, decreasing
        let model = |db: f64| l0 + g * (1.0 - (-db / tau).exp());
        let target = -22.0_f64;

        // Seed exactly as correct_iter: base@0 and the open-loop first apply at db0=target-l0
        // (assumes slope 1 → overshoots through the compressor).
        let db0 = target - l0;
        let mut prev = (0.0_f64, model(0.0));
        let mut last = (db0, model(db0));
        let mut best = last;
        let mut steps = 0u32;
        let mut max_step = 0.0_f64;
        while steps < super::MEASURE_CORRECT_MAX && (last.1 - target).abs() > super::KNOB_TOL_LU {
            let Some(next_db) = super::secant_next_db(prev, last, target) else {
                break;
            };
            max_step = max_step.max((next_db - last.0).abs());
            let vn = model(next_db);
            steps += 1;
            if (vn - target).abs() < (best.1 - target).abs() {
                best = (next_db, vn);
            }
            prev = last;
            last = (next_db, vn);
        }
        assert!(
            (best.1 - target).abs() <= super::KNOB_TOL_LU,
            "converged to {} (target {target})",
            best.1
        );
        assert!(steps <= super::MEASURE_CORRECT_MAX, "steps={steps}");
        assert!(
            max_step <= super::BATCH_TRUST_DB as f64 + 1e-9,
            "trust region honored, max step {max_step} dB"
        );
    }

    // Drive the secant loop against a synthetic dB-of-amplitude knob, searching
    // in log-of-knob coordinate (c = 20·log10(x)) exactly as `level_preset_block`
    // does. In that space the knob is linear, so it converges in one secant step.
    #[test]
    fn secant_loop_converges_on_log_knob() {
        // captured_LUFS = 20*log10(x) + C, x in (0,1], C = -10 (amp outputLevel).
        let model = |x: f32| 20.0 * (x.max(1e-4) as f64).log10() - 10.0;
        let target = -24.0f64;
        let (lo, hi) = (0.0f32, 1.0f32);
        let eps = 1e-3f32;
        let to_c = |x: f32| 20.0 * x.max(eps).log10();
        let from_c = |c: f32| 10f32.powf(c / 20.0).clamp(lo, hi);
        let (c_lo, c_hi) = (to_c(lo.max(eps)), to_c(hi));
        let span = c_hi - c_lo;
        let (mut ca, mut cb) = (c_lo + 0.4 * span, c_lo + 0.75 * span);
        let (mut ya, mut yb) = (model(from_c(ca)), model(from_c(cb)));
        let mut best = if (ya - target).abs() <= (yb - target).abs() {
            (ca, ya)
        } else {
            (cb, yb)
        };
        let mut iters = 2;
        while iters < super::KNOB_MAX_ITERS && (best.1 - target).abs() > super::KNOB_TOL_LU {
            let Some(nc) = super::secant_next(ca, ya, cb, yb, target) else {
                break;
            };
            let nc = nc.clamp(c_lo, c_hi);
            if (nc - cb).abs() < 1e-4 {
                break;
            }
            let ny = model(from_c(nc));
            iters += 1;
            if (ny - target).abs() < (best.1 - target).abs() {
                best = (nc, ny);
            }
            (ca, ya, cb, yb) = (cb, yb, nc, ny);
        }
        assert!(
            (best.1 - target).abs() <= super::KNOB_TOL_LU,
            "converged to lufs {} for target {target}",
            best.1
        );
        let final_x = from_c(best.0);
        assert!((final_x - 0.1995).abs() < 0.02, "final knob {final_x}"); // 10^((-24+10)/20)
        assert!(
            iters <= 3,
            "should converge fast in log space, iters={iters}"
        );
    }

    /// The setlist common target is min(C) − headroom; presets whose C equals the
    /// floor land below the target (reachable), so none clamp.
    #[test]
    fn setlist_common_target_is_below_min_c() {
        // Three presets with different ceilings.
        let cs = [-20.68f64, -24.0, -22.5];
        let headroom = 1.0;
        let min_c = cs.iter().cloned().fold(f64::INFINITY, f64::min);
        let target = min_c - headroom;
        assert!((target - (-25.0)).abs() < 1e-9, "target={target}");
        // Every preset can reach the common target (level ≤ 1.0, not clamped).
        for &c in &cs {
            let (lvl, clamped, _) = super::solve_level(c, target);
            assert!(!clamped, "C={c} unexpectedly clamped at target {target}");
            assert!(lvl <= 1.0 && lvl > 0.0, "lvl={lvl}");
        }
    }

    // ---- live-controller (next_live_coord) against a fake loudness source ----

    use super::{next_live_coord, SceneLevelStrategy, KNOB_TOL_LU, LIVE_MAX_ITERS};

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

    // scene_at_target mirrors the correction loop's KNOB_TOL_LU acceptance band —
    // a re-run must not rewrite an already-in-tolerance scene.
    #[test]
    fn scene_at_target_accepts_within_knob_tol() {
        assert!(
            super::scene_at_target(-22.0, -22.29, false),
            "0.29 LU off, unclamped"
        );
    }

    #[test]
    fn scene_at_target_rejects_just_outside_knob_tol() {
        assert!(!super::scene_at_target(-22.0, -22.31, false), "0.31 LU off");
    }

    #[test]
    fn scene_at_target_rejects_when_clamped_even_at_zero_delta() {
        assert!(
            !super::scene_at_target(-22.0, -22.0, true),
            "clamped must still report"
        );
    }

    // switch_at_target is the footswitch mirror of scene_at_target — the re-run
    // idempotency band (the PR #74 follow-up gap). Same KNOB_TOL_LU acceptance.
    #[test]
    fn switch_at_target_accepts_within_knob_tol() {
        assert!(
            super::switch_at_target(-24.0, -24.29, false),
            "0.29 LU off, unclamped → skip the re-solve"
        );
    }

    #[test]
    fn switch_at_target_rejects_just_outside_knob_tol() {
        assert!(
            !super::switch_at_target(-24.0, -24.31, false),
            "0.31 LU off → must re-level"
        );
    }

    // level_unchanged: LU-space ratio tolerance matching KNOB_TOL_LU, guards the
    // Base-leveling idempotency skip against re-writing an in-tolerance presetLevel.
    #[test]
    fn level_unchanged_true_on_identical_levels() {
        assert!(super::level_unchanged(0.5160, 0.5160));
    }

    #[test]
    fn level_unchanged_true_within_knob_tol() {
        // 20*log10(0.5160/0.5300) ≈ -0.23 LU
        assert!(super::level_unchanged(0.5160, 0.5300));
    }

    #[test]
    fn level_unchanged_false_beyond_knob_tol() {
        // 20*log10(0.5160/0.55) ≈ -0.55 LU
        assert!(!super::level_unchanged(0.5160, 0.55));
    }

    #[test]
    fn level_unchanged_false_on_zero_previous() {
        assert!(!super::level_unchanged(0.5, 0.0));
    }

    #[test]
    fn level_unchanged_false_on_negative_previous() {
        assert!(!super::level_unchanged(0.5, -1.0));
    }
}
