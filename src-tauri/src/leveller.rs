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
use crate::probe_api::scene_jobs::{overlay_param, SceneParamRead};
use crate::session::Session;
use crate::{
    read_saved_preset, read_saved_preset_complete, scene_write_verdict_for_param, settle_abortable,
    settle_or_cancel, SceneWriteVerdict,
};

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

/// [`DOCTOR_STIM_MS`] at the required host Core Audio rate ([`RATE`]), in samples.
pub fn doctor_stim_samples() -> usize {
    (RATE as usize / 1000) * DOCTOR_STIM_MS
}

/// Silent preamble prepended to the Doctor stimulus: the true inject latency is
/// only ~32 ms (HW, `audio::estimate_onset` across 15 captures), which leaves a
/// pre-onset noise-floor window too short for a full Welch segment
/// (`psd::SEG` = 8192 ≈ 171 ms) — the output-SNR coverage gate needs a stable
/// floor estimate. 200 ms of played silence stretches the floor window to
/// ~230 ms at the cost of 200 ms per capture. Spectrally neutral: LUFS gating
/// drops silence, and the body PSD starts at [`doctor_onset`]'s `signal_start`.
pub const DOCTOR_PAD_MS: usize = 200;

/// [`DOCTOR_PAD_MS`] at the required host Core Audio rate, in samples.
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

/// Which detector produced a [`DoctorOnset`] — carried for logging (every
/// fallback WARNs naming it) and tests; never branched on by a diagnosis rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnsetSource {
    /// The padded stimulus's played silence produced a floor-relative energy
    /// step (`audio::estimate_signal_start`) — the primary, deterministic path.
    Energy,
    /// Envelope cross-correlation (`audio::estimate_onset`) — the energy step
    /// didn't fire, or its implied latency was outside the plausible band.
    Correlator,
    /// Neither found a trustworthy split — legacy whole-buffer/un-aligned.
    UnAligned,
}

/// One Doctor capture's onset/body split, as [`doctor_onset`] derives it.
#[derive(Debug, Clone, Copy)]
pub struct DoctorOnset {
    /// Where REAL signal begins (after the played pad) — the body PSD and the
    /// coverage gate's floor window start here.
    pub signal_start: usize,
    /// Where the PADDED stimulus aligns — the pad stays IN the body (matches
    /// the historical aligned split, so the R4/R5 tail-window budget holds).
    pub body_start: usize,
    /// Body length in samples: `body_start + body_len` is the tail split point
    /// ([`crate::doctor::tail_energy_ratio`]'s `body_start`/`body_len`).
    pub body_len: usize,
    pub source: OnsetSource,
}

impl DoctorOnset {
    /// Whether this split is trustworthy — energy and confident-correlator
    /// splits are both confident; only the un-aligned fallback is not.
    pub fn confident(&self) -> bool {
        self.source != OnsetSource::UnAligned
    }

    /// The tail split point: `body_start + body_len`
    /// ([`crate::doctor::tail_energy_ratio`]'s `body_start`/`body_len` summed).
    pub fn body_end(&self) -> usize {
        self.body_start + self.body_len
    }
}

/// Mirrors `audio::ONSET_MAX_PLAUSIBLE_LAG_MS` — a wash-artifact lag beyond
/// this is not real latency regardless of which detector reports it. Derived
/// (not re-pinned) so the two thresholds can't drift apart.
const DOCTOR_ONSET_MAX_LATENCY_MS: i64 = audio::ONSET_MAX_PLAUSIBLE_LAG_MS as i64;
/// Negative-latency floor: a capture stream that starts a little late (signal
/// lands before the pad "would" end) is real and unrepresentable by the
/// correlator's forward-only lag search — but not unbounded, or an unrelated
/// early energy step (e.g. an engage pop the floor step still let through)
/// would be accepted as if it were the true onset. This is NOT a plausibility
/// choice — it's `audio::estimate_signal_start`'s physical reach: that
/// detector only ever searches AFTER its own [`audio::ONSET_ENERGY_FLOOR_WINDOW_MS`]
/// of the capture, so a step inside that window is never found (reads as
/// hot-from-zero, `None`) rather than found-but-early. Relative to
/// [`DOCTOR_PAD_MS`], the detector can NEVER report an onset earlier than
/// `DOCTOR_PAD_MS - ONSET_ENERGY_FLOOR_WINDOW_MS` before the pad "ends" (an
/// envelope, not a tightly-attained bound — the floor window's own hop
/// rounding makes the PRACTICAL reach a few ms tighter still, see the
/// `negative_latency_at_the_reachable_bound_is_found` test) — so an energy
/// step implying anything more negative than that cannot be this preset's
/// true onset; it's an unrelated early artifact (e.g. an engage pop) and must
/// fall through to the correlator instead.
const DOCTOR_ONSET_MIN_LATENCY_MS: i64 =
    -((DOCTOR_PAD_MS - audio::ONSET_ENERGY_FLOOR_WINDOW_MS) as i64);
const _: () = assert!(DOCTOR_ONSET_MIN_LATENCY_MS < 0);

/// Doctor's onset seam: a floor-relative energy step (primary — deterministic
/// on every wet/dry chain because the Doctor stimulus always carries a played
/// silent pad, see [`DOCTOR_PAD_MS`]), falling back to the envelope correlator
/// (`audio::estimate_onset`) when the step doesn't fire or its implied latency
/// is implausible, falling back to the legacy un-aligned split when neither
/// does. `stim_padded` is the padded Doctor stimulus ([`doctor_stim_slice`]);
/// `samples` is the raw capture. WARNs with the source on every fallback, so a
/// wash-cohort verdict flip is traceable to which path fired.
pub fn doctor_onset(stim_padded: &[f32], samples: &[f32], rate: u32) -> DoctorOnset {
    let pad = doctor_pad_samples();
    let body_len_full = stim_padded.len().saturating_sub(pad);
    if let Some(signal_start) = audio::estimate_signal_start(samples, rate) {
        let latency_ms = (signal_start as i64 - pad as i64) * 1000 / i64::from(rate.max(1));
        if (DOCTOR_ONSET_MIN_LATENCY_MS..=DOCTOR_ONSET_MAX_LATENCY_MS).contains(&latency_ms) {
            let body_start = signal_start.saturating_sub(pad);
            let body_end = signal_start.saturating_add(body_len_full);
            return DoctorOnset {
                signal_start,
                body_start,
                body_len: body_end.saturating_sub(body_start),
                source: OnsetSource::Energy,
            };
        }
        log::warn!(
            "doctor_onset: energy step implies {latency_ms} ms latency (outside {DOCTOR_ONSET_MIN_LATENCY_MS}..={DOCTOR_ONSET_MAX_LATENCY_MS} ms) — falling back to the correlator"
        );
    }
    let (onset, confident) = audio::estimate_onset(stim_padded, samples, rate);
    if confident {
        return DoctorOnset {
            signal_start: onset + pad,
            body_start: onset,
            body_len: stim_padded.len(),
            source: OnsetSource::Correlator,
        };
    }
    log::warn!("doctor_onset: no confident onset (energy step or correlator) — un-aligned split");
    DoctorOnset {
        signal_start: 0,
        body_start: 0,
        body_len: stim_padded.len(),
        source: OnsetSource::UnAligned,
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
// The device silently DROPS a scene-context write when the session sat IDLE too long
// immediately before it — a PER-COMMAND idle-gap cliff at ~400–450 ms, NOT a
// "time since loadScene" window (the earlier reading of the same bisect data). The
// discriminating HW run (Hiwatt slot 31, fw 1.8.45): the enable-dropped branch's
// back-to-back 150+300 ms sleeps formed ONE 450 ms idle gap and 6/6 bare scene
// writes vanished (no presetError, nothing in the working copy, nothing persisted)
// even though only ~450 ms had passed since `loadScene` — while the bisect's
// load_scene→300→edit→400→write (max single gap 400 ms) lands and …→450→write
// drops. Same cliff family as the idle re-amp-OFF drop (`reamp_off_guaranteed`).
// So: keep EVERY pre-write idle gap ≤~300 ms, and never sleep a settle for a
// command that wasn't sent (the conditional in `set_knobs`).
const SETTLE_AFTER_SCENE_EDIT_MS: u64 = 300;
// Gap between the `loadScene` recall and the next command (`SetNodeSceneEdit`, or the
// value write itself when the enable is dropped). 150 (not the general 300
// `SETTLE_AFTER_SET_MS`) keeps the gap far under the ~400–450 ms idle cliff above.
// HW-bisected lower edge (`probe --bisect-scene`, fw 1.8.45): scene_settle 150, 100,
// and even 50 all land ON the scene overlay (never leak to base) and persist.
pub(crate) const SETTLE_AFTER_SCENE_RECALL_MS: u64 = 150;
const RATE: u32 = 48_000;
const LEVEL_MIN: f32 = 0.0;
/// THE amplitude ceiling every `presetLevel` / amp-`outputLevel` lane clamps to.
pub(crate) const LEVEL_MAX: f32 = 1.0;
/// `processed_loudness`'s sentinel error text for a capture with no measurable signal — shared
/// so producer and consumers can't drift.
const NO_SIGNAL_CAPTURED: &str = "no signal captured";

#[derive(Debug, Clone, Serialize)]
pub struct LevelResult {
    pub slot: u32,
    /// WHICH sound this row describes, when the row is a SCENE row: the 0-based
    /// `scenes[]` wire index. `None` on every base / block / footswitch row.
    ///
    /// Load-bearing for identity, not decoration. `level_scenes_apply_batched` FILTERS
    /// failed scenes out of the vec it returns (`commands/level_scenes.rs`), so the
    /// result vec can be SHORTER than the request's `jobs` array — and a consumer that
    /// re-derives "which scene is row i?" by position mislabels every row after the
    /// first failure. The batched runner's outcome carries the slot
    /// ([`BatchedSceneOutcome::scene_slot`], whose own doc says the same thing about
    /// positional zips); this forwards it onto the wire so no consumer has to guess.
    /// Mirrored in `src/lib/types.ts`.
    pub scene_slot: Option<u32>,
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
    /// The clamp's CAUSE from the shared taxonomy ([`crate::headroom_trade::ClampKind`]) —
    /// `None` when the row is not clamped. ADDITIVE alongside `clamp_reason`, whose contract
    /// ("the leveled signal isn't reaching USB 1/2", `.claude/rules/leveling-dsp.md`) is
    /// unchanged: this is the machine-readable cause, that one stays the verbatim prose the
    /// UI maps to `offbranch`. Mirrored in `src/lib/types.ts`.
    pub clamp_kind: Option<crate::headroom_trade::ClampKind>,
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
    /// Post-save param-level verify, forwarded from the batched scene runner's
    /// [`BatchedSceneOutcome::persist_mismatch`]: `Some(true)` = the saved preset does
    /// NOT hold the value this result reports (do not trust the number); `Some(false)`
    /// = re-read and confirmed; `None` = not checked (no save, nothing written, the
    /// re-read failed, or a path without the verify).
    pub persist_mismatch: Option<bool>,
    /// THE HEADROOM TRADE this run made (or, on a preview, WOULD make) — see
    /// [`crate::headroom_trade::TradeSummary`] (disclosure rationale: its own doc). Stamped on
    /// EVERY row of a batch that traded, because the trade moved the whole preset's gain
    /// structure, not one row's. `None` on every untraded run and on every lane that has no
    /// trade (base, block, footswitch). Mirrored in `src/lib/types.ts`.
    pub trade: Option<crate::headroom_trade::TradeSummary>,
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
    /// Scene to recall right before a `save: true` save, so the save re-stamps the
    /// preset's ORIGINAL `lastLoadedScene` (0-based scene index, or
    /// `session::BASE_SCENE_SLOT`). The base-context measurement recalls base (issue:
    /// base must measure base, not the saved scene), which leaves base active at save
    /// time — without this the save silently rewrites `lastLoadedScene` to 8 (HW,
    /// Hiwatt slot 31: pre 3 → post 8), changing which scene the preset loads into on
    /// the pedalboard. `None` = save whatever context is active (old behavior).
    pub restore_scene: Option<u32>,
    /// The run's OWN `presetLevel` — its solved value, or the UNSAVED raise a headroom
    /// trade is holding — re-asserted after `set_knobs` and before the verify engage.
    ///
    /// `set_knobs` recalls the knob's scene, and a recall runs the device's own
    /// level-apply (`recall_reassert_save`'s doc carries the HW evidence), so without
    /// this the VERIFY capture renders at the level the device HAS SAVED even when the
    /// as-is capture was correctly asserted. That asymmetry is silent and it INVERTS the
    /// correction: the verify reads quiet, so the bounded secant walks the fader the wrong
    /// way (offline gate `a_batched_scene_run_persists_both_halves_of_a_landed_headroom_trade`:
    /// the compensating fader rose 0.69 → 0.726 instead of falling).
    ///
    /// Ignored when a `PresetLevel` target is in the same batch — there `set_knobs` is
    /// already writing the level under measurement and must win. `None` = assert nothing,
    /// which is every caller that has no run-owned value.
    pub intended_preset_level: Option<f32>,
}

impl Default for LevelOptions {
    fn default() -> Self {
        LevelOptions {
            save: false,
            verify: false,
            ref_level: 0.5,
            defer: false,
            restore_scene: None,
            intended_preset_level: None,
        }
    }
}

/// Fresh-connect, recall BASE, set `level` (NO preset load — the current preset is already
/// the one we want), engage re-amp once, capture, and return the processed pair's loudness.
/// The one-shot `presetLevel` case of `measure_knob_at` (which owns the base recall — see
/// `arm_measurement`: without it the capture measures the preset's saved `lastLoadedScene`).
fn measure_at_level(
    stimulus: &[f32],
    level: f32,
    force_bypass: &[(String, String, bool)],
) -> Result<lufs::Loudness, String> {
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

/// Sentinel error returned when a cooperative cancel flag is observed at a leveling
/// checkpoint. Compared by `restore_after_unsaved_error` (a cancel must restore the
/// stored preset even on the `save=true` path) and treated as a skip by the frontend.
pub const CANCELLED: &str = "cancelled";

/// The freshness barrier's player-facing caption — shared verbatim by `commands::level_scenes`
/// and `commands::level_footswitch`, whose independent barrier call sites (both gate on
/// `slot_save_pending_commit`) must read identically to the player regardless of which lane hit
/// the wait.
pub(crate) const WAITING_FOR_COMMIT_MSG: &str =
    "waiting for the device to commit the previous save…";

/// Reload the stored preset to discard temporary level edits made while
/// measuring. `save=false` is a preview/read-only contract for callers: the TMP
/// edit buffer may be mutated during capture, but it must not remain dirty.
pub(crate) fn restore_saved_preset(slot: u32) -> Result<(), String> {
    // NOT `sleep_or_cancel`: this runs AFTER a cancel to clean up. Bailing here would leave
    // the edit buffer dirty at the measurement level — the whole point of the restore.
    crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
    let mut s = Session::connect_lean()?;
    s.load_preset(slot)?;
    crate::settle(Duration::from_millis(settle_after_load_ms()));
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

/// Slack over the stimulus spread before a capture counts as aberrant. A driven chain
/// only ever COMPRESSES dynamics, so out-spread ≤ in-spread is the physical bound and
/// this margin covers metering noise around it (the two spreads are measured on
/// different buffers by different meter runs), not any real excess.
pub(crate) const SPREAD_ABERRANT_MARGIN_LU: f64 = 0.3;

/// Is this capture MORE dynamic than the stimulus that drove it? Physically impossible
/// for a genuinely driven amp state (a chain compresses, it does not expand), so it is
/// the tell for a fire-and-forget preset/scene recall that only PARTIALLY landed — the
/// capture then reads plausible but belongs to a sound we did not ask for. Armed by the
/// same stationary-stimulus gate as [`floor_suspect`]: a stimulus with no dynamics of
/// its own (or an unmeasurable one, which reports 0.0) can't discriminate, so it
/// disarms both checks rather than trip this one on every capture.
pub(crate) fn spread_aberrant(capture_spread_lu: f64, stimulus_spread_lu: f64) -> bool {
    stimulus_spread_lu > STATIONARY_STIM_LU
        && capture_spread_lu > stimulus_spread_lu + SPREAD_ABERRANT_MARGIN_LU
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

/// Run `measure`; if the capture is aberrant, wait `gap` and retry ONCE with the same
/// settings (heals a transient inject failure or a half-landed recall). TWO tells, one
/// retry budget between them:
///
/// * FLOOR ([`floor_suspect`]) — no dynamics at all. A persistently flat capture is
///   reported as [`GuardOutcome::StillFlat`], not swallowed; callers escalate.
/// * SPREAD ([`spread_aberrant`]) — MORE dynamics out than in. Always resolves to
///   `Live`: the reading is plausible (just possibly of the wrong sound), so erroring a
///   row on this heuristic would be the worse failure. It must NOT resolve to
///   `StillFlat` either — `measure_c`'s level-shift confirm would PASS a wrong-scene
///   capture (`presetLevel` is a linear post-chain multiplier whatever scene landed)
///   and launder it as verified.
pub(crate) fn measure_floor_guarded(
    mut measure: impl FnMut() -> Result<lufs::Loudness, String>,
    stimulus_spread_lu: f64,
    gap: Duration,
) -> Result<GuardOutcome, String> {
    let first = measure()?;
    if floor_suspect(first.spread_lu(), stimulus_spread_lu) {
        log::warn!(
            "floor guard: capture spread {:.2} LU ≤ {FLOOR_TRIP_LU} — suspected silent inject, retrying once",
            first.spread_lu()
        );
    } else if spread_aberrant(first.spread_lu(), stimulus_spread_lu) {
        log::warn!(
            "floor guard: capture spread {:.2} LU exceeds the stimulus's {stimulus_spread_lu:.2} LU \
             — a chain cannot expand dynamics, so the recall likely half-landed; retrying once",
            first.spread_lu()
        );
    } else {
        return Ok(GuardOutcome::Live(first));
    }
    settle_or_cancel(gap.as_millis() as u64)?;
    let second = measure()?;
    if floor_suspect(second.spread_lu(), stimulus_spread_lu) {
        return Ok(GuardOutcome::StillFlat(second));
    }
    if spread_aberrant(second.spread_lu(), stimulus_spread_lu) {
        log::warn!(
            "floor guard: capture spread {:.2} LU still exceeds the stimulus's \
             {stimulus_spread_lu:.2} LU after the retry — reporting it, verify this sound by ear",
            second.spread_lu()
        );
    }
    Ok(GuardOutcome::Live(second))
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

// ───────────────────────── Stale-load freshness barrier (per-slot save registry) ─────────────────────────
//
// `saveCurrentPreset` commits LAZILY on the real TMP (fw 1.8.45, HW-reproduced 2026-08-02):
// the commit materializes T+45–100 s after the request, and a same-slot `loadPreset` issued
// inside that window can still return the PRE-save bytes — read-your-writes only holds for
// the field-8 slot-addressed read, not for `loadPreset`. Incident: a footswitch batch loaded
// its slot ~2 s after the run's own base save and materialized the presetLevel from BEFORE
// that save (0.4377 saved, ≈0.798 read back), sweeping all 4 switches ~5.2 LU hot.
//
// Fix: an in-process per-slot SAVE REGISTRY. Every leveling save records what it wrote (a
// `SaveWitness`) and when; a load site that might race a still-committing save calls
// `ensure_fresh_load` first — it re-loads on a rich harvest session and waits for the
// harvested doc to show the registered witness before the caller's own (unchanged)
// load/connect proceeds. No registry entry, or the commit window has elapsed, is a
// zero-cost no-op — the overwhelming majority of loads, which never race a same-slot save.

/// How long a `saveCurrentPreset` commit stays racy (HW: 45–100 s observed; 150 s gives
/// margin). Past this, `ensure_fresh_load` stops waiting and proceeds — the commit is
/// time-bounded, so camping on an unharvestable witness forever would brick a run.
/// Mirrors: `probe_api::seed_scenario` derives its landed-import verify window from this
/// constant, and `scripts/validate-hbe.sh` carries the same 150 as a shell literal —
/// change one, change all three.
pub(crate) const COMMIT_WINDOW_SECS: u64 = 150;
/// Agreement band for a harvested witness vs its registered value — matches `PERSIST_TOL`'s
/// float-formatting slack, far below any real leveling step.
const WITNESS_EPS: f64 = 1e-4;
/// Wait between re-issued loads while a save is still racing — heartbeat-interleaved (see the
/// cadence loop in `ensure_fresh_load`), never a passive sleep.
const STALE_RETRY_WAIT_MS: u64 = 10_000;
/// Heartbeat cadence during a stale-load wait — the idle-gap family's ≤300 ms ceiling.
const STALE_HEARTBEAT_MS: u64 = 250;

/// One field a leveling save actually changed — the freshness barrier's comparison anchor.
/// `Param` covers a footswitch Bake/Assign write (`scene: None` — unaffected, same base
/// `dspUnitParameters`/`ftsw` candidate logic as always) AND a scene deferred `outputLevel`
/// write (`scene: Some(s)`, the 0-based `scenes[]` wire index the write landed in — closes
/// the scene-discriminator gap this comment used to record as accepted). For a scene
/// witness, `witness_value_in_doc` consults ONLY that scene's overlay
/// (`probe_api::scene_jobs::scene_overlay`) and accepts on an exact match; every other
/// answer — no overlay, a truncated/unknown read, or the param simply missing — reads as
/// still-stale, with NO fallback to the base candidates (base can never legitimately hold a
/// scene overlay's value, so a fallback match there would be a coincidence-accept of a
/// possibly pre-save doc). An unmatched witness (the barrier's own bare load never
/// re-activated that scene, say) still bounds out via the time-gate below, so the worst
/// case stays a `COMMIT_WINDOW_SECS`-long wait, never a hang.
#[derive(Debug, Clone)]
pub(crate) enum SaveWitness {
    PresetLevel(f32),
    Param {
        node: String,
        param: String,
        value: f32,
        /// 0-based `scenes[]` wire index the write landed in; `None` = base/footswitch.
        scene: Option<u32>,
    },
}

/// One slot's most recent leveling save: when it fired and what it should have changed.
struct SlotSave {
    at: std::time::Instant,
    witness: SaveWitness,
}

/// Per-slot save registry: `slot` → the last leveling save's witness + timestamp. Keyed on
/// the 0-BASED LIST INDEX — the same `slot` every `Session` method takes (they add the +1
/// for the wire `userSlot` themselves), so registry keys and load/save call sites never
/// convert. One process, one device, and (`device_gate::OP_ABORT`'s own reasoning) exactly
/// one leveling op ever in flight — a global map is the right shape, not a per-run registry
/// with extra ceremony.
static SLOT_SAVE_REGISTRY: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<u32, SlotSave>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Record what a leveling save just wrote to `slot`, and when. Call at EVERY leveling save
/// site — an unregistered save site silently reopens the stale-load hole for whatever loads
/// next, since `ensure_fresh_load` treats a missing entry as "nothing to wait for".
pub(crate) fn register_slot_save(slot: u32, witness: SaveWitness) {
    if let Ok(mut reg) = SLOT_SAVE_REGISTRY.lock() {
        reg.insert(
            slot,
            SlotSave {
                at: std::time::Instant::now(),
                witness,
            },
        );
    }
}

/// The slot's currently registered `PresetLevel` witness, if that's what the last leveling
/// save wrote. The FS batch command snapshots this BEFORE `write_footswitch_values` — whose
/// own save registers the batch's `Param` witness over the same slot key, after which the
/// base save's expectation is unrecoverable from the registry (it holds ONE entry per slot).
pub(crate) fn registered_preset_level(slot: u32) -> Option<f32> {
    SLOT_SAVE_REGISTRY.lock().ok().and_then(|reg| {
        reg.get(&slot).and_then(|e| match e.witness {
            SaveWitness::PresetLevel(pl) => Some(pl),
            SaveWitness::Param { .. } => None,
        })
    })
}

/// Wipe the registry — e2e-only: `/sim/reset` installs a FRESH sim device between specs, and
/// a witness left over from a previous spec's save would make the next spec's first leveling
/// load wait out the whole commit window against a doc that can never match it.
#[cfg(feature = "e2e")]
pub(crate) fn clear_slot_save_registry() {
    if let Ok(mut reg) = SLOT_SAVE_REGISTRY.lock() {
        reg.clear();
    }
}

/// Test seam: register with an explicit timestamp, so the time-gate path is coverable
/// without a real 2.5-minute wait (`e2e_server_tests`' sim-routed barrier tests).
#[cfg(all(test, feature = "e2e"))]
pub(crate) fn register_slot_save_at(slot: u32, witness: SaveWitness, at: std::time::Instant) {
    if let Ok(mut reg) = SLOT_SAVE_REGISTRY.lock() {
        reg.insert(slot, SlotSave { at, witness });
    }
}

/// True if `slot` has a registry entry `ensure_fresh_load` would actually wait on right now
/// (an entry inside the commit window). Used ONLY to decide whether to surface the caller's
/// "waiting for the device to commit…" progress line BEFORE calling `ensure_fresh_load`
/// (which re-derives the identical condition internally); never mutates the registry.
pub(crate) fn slot_save_pending_commit(slot: u32) -> bool {
    SLOT_SAVE_REGISTRY.lock().is_ok_and(|reg| {
        reg.get(&slot)
            .is_some_and(|e| e.at.elapsed().as_secs() <= COMMIT_WINDOW_SECS)
    })
}

/// The witness's own expected value, as `f64` for the comparison.
fn witness_expected(w: &SaveWitness) -> f64 {
    match w {
        SaveWitness::PresetLevel(v) => *v as f64,
        SaveWitness::Param { value, .. } => *value as f64,
    }
}

/// Scan `ftsw` for a `param` function targeting `(node, param)` on ANY switch and return its
/// `valueA` — the Assign write shape, which lives in the footswitch table, never in
/// `dspUnitParameters`. No switch index needed: a leveled block param is targeted by at most
/// one footswitch function in practice, and the caller (a `SaveWitness::Param`) doesn't carry
/// one either — so this walks every switch through the footswitch module's own accessor.
fn ftsw_value_a(ftsw: &serde_json::Value, node: &str, param: &str) -> Option<f64> {
    (0..ftsw.as_array()?.len() as u32)
        .find_map(|sw| crate::footswitch::existing_param_fn_value_a(ftsw, sw, node, param))
}

/// The scene overlay's own value for `Param { scene: Some(s), node, param, .. }` — MATCH
/// ONLY, no fallback (post-review amendment 2): a [`SceneParamRead::Value`] is the only accept
/// path; `Absent`/`Unknown` (and a `Value` that isn't itself numeric) read `None`. The caller
/// (`witness_value_in_doc`) must never fall through to the base `dspUnitParameters`/`ftsw`
/// candidates for a scene witness on a `None` here — base can never legitimately hold a scene
/// overlay's value, so a fallback match there would be a coincidence-accept of a possibly
/// pre-save doc. Thin wrapper over [`overlay_param`], the shared read authority
/// (`probe_api::scene_jobs`) also behind `persisted_value`'s scene arm.
fn scene_overlay_witness_value(
    doc: &serde_json::Value,
    scene: u32,
    node: &str,
    param: &str,
) -> Option<f64> {
    match overlay_param(doc, scene, node, param) {
        SceneParamRead::Value(v) => v.as_f64(),
        SceneParamRead::Absent | SceneParamRead::Unknown => None,
    }
}

/// Read the witness's field out of a harvested (or re-read) preset doc. `PresetLevel` reads
/// `audioGraph.presetLevel`. `Param { scene: Some(s), .. }` (Fix 3) consults ONLY that
/// scene's overlay via [`scene_overlay_witness_value`] — no fallback, see that function's
/// doc. `Param { scene: None, .. }` (a footswitch Bake/Assign write, unchanged) checks BOTH
/// places a leveling save can put the value — the block's own `dspUnitParameters` (the Bake
/// shape) and `ftsw`'s `valueA` (the Assign shape, where `dspUnitParameters` keeps holding
/// the switch-OFF value) — preferring whichever one matches the witness, else the first
/// present. The witness doesn't record which shape its save used, and for an Assign the
/// `dspUnitParameters` value EXISTS but can never match, so a fixed try-order would starve
/// that case into the time-gate.
fn witness_value_in_doc(doc: &serde_json::Value, w: &SaveWitness) -> Option<f64> {
    match w {
        SaveWitness::PresetLevel(_) => crate::audiograph::preset_level(doc),
        SaveWitness::Param {
            scene: Some(s),
            node,
            param,
            ..
        } => scene_overlay_witness_value(doc, *s, node, param),
        SaveWitness::Param {
            scene: None,
            node,
            param,
            value,
            ..
        } => {
            let expected = *value as f64;
            let candidates = [
                crate::commands::level_footswitch::node_param_f64(doc, node, param),
                doc.get("ftsw").and_then(|f| ftsw_value_a(f, node, param)),
            ];
            candidates
                .iter()
                .flatten()
                .copied()
                .find(|got| (got - expected).abs() <= WITNESS_EPS)
                .or_else(|| candidates.iter().flatten().copied().next())
        }
    }
}

/// Freshness barrier for a same-slot load that may race a still-committing save (see this
/// section's module doc). No registry entry for `slot`, or the registered save is older than
/// `COMMIT_WINDOW_SECS`, is the overwhelming common case and costs nothing — no session is
/// opened.
///
/// Otherwise: a RICH harvest session (heartbeat warmup, `send_and_collect(LoadPreset)` —
/// never `Session::load_preset`, whose `transact_eager` discards the field-3 push the harvest
/// reads; mirrors `probe_api::slot_write::discover_blocks_rich`) loads the slot and compares
/// the harvested doc's witness field against the registered value (`WITNESS_EPS`). A match
/// means fresh: done. An empty/truncated harvest counts as STILL-STALE, same as a mismatch —
/// never a free pass. `s.raw` is cleared before every harvest, including every retry: this
/// session must never carry a field-9 (`presetDataChanged`) stream, because
/// `best_json_payload` prefers the LONGEST of its three carriers and (9,3) is the field-8
/// reply's own carrier — a polluted session would compare the oracle against itself. This
/// barrier NEVER issues a field-8 read on its own session, by construction.
///
/// A mismatch retries on the SAME held session (no reconnects — the HID open-lockout window,
/// `danger.md`), heartbeat-interleaved so the wait is cancellable and the device sees a live
/// controller, re-issuing the load roughly every `STALE_RETRY_WAIT_MS`. Once
/// elapsed-since-save exceeds `COMMIT_WINDOW_SECS`, one final load re-issue runs and this
/// returns `Ok` regardless of the harvest (INFO-logged) — the commit is time-bounded, so this
/// must never hang or hard-error the caller.
///
/// After a barrier pass, the CALLER's own existing load/connect proceeds completely
/// unchanged — never fold the barrier's rich session into a measurement/engage flow (a
/// two-full-handshake connect wedges re-amp; `notes/leveling.md`'s "no signal captured").
pub(crate) fn ensure_fresh_load(
    slot: u32,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), String> {
    ensure_fresh_load_paced(slot, cancelled, STALE_RETRY_WAIT_MS)
}

/// [`ensure_fresh_load`] with an explicit retry cadence — the sim-routed barrier tests
/// (`e2e_server_tests`) shrink it so a stale→retry→pass cycle runs in seconds; production
/// always enters through the `STALE_RETRY_WAIT_MS` wrapper above.
pub(crate) fn ensure_fresh_load_paced(
    slot: u32,
    cancelled: &mut dyn FnMut() -> bool,
    retry_wait_ms: u64,
) -> Result<(), String> {
    let Some((saved_at, witness)) = SLOT_SAVE_REGISTRY
        .lock()
        .ok()
        .and_then(|reg| reg.get(&slot).map(|e| (e.at, e.witness.clone())))
    else {
        return Ok(());
    };
    if saved_at.elapsed().as_secs() > COMMIT_WINDOW_SECS {
        return Ok(());
    }
    let expected = witness_expected(&witness);
    let mut s = Session::connect()?;
    s.rich_warmup()?;
    let out = 'harvest: loop {
        if cancelled() {
            break Err(CANCELLED.to_string());
        }
        if let Err(e) = s.rich_load_collect(slot) {
            break Err(e);
        }
        let harvested = s
            .current_preset_value()
            .ok()
            .and_then(|doc| witness_value_in_doc(&doc, &witness));
        if let Some(got) = harvested {
            if (got - expected).abs() <= WITNESS_EPS {
                // Observability hook for the online lane (post-review amendment 3): a scene
                // witness accepting is the interesting case to see in real HW logs, since it
                // proves the early exit actually fired instead of silently degrading to the
                // (also-passing) time-gate below.
                if let SaveWitness::Param { scene: Some(s), .. } = &witness {
                    log::info!(
                        "ensure_fresh_load: slot {slot} scene {s} witness matched on a \
                         harvestable load — exiting early instead of blind-waiting"
                    );
                }
                break Ok(());
            }
        }
        if saved_at.elapsed().as_secs() > COMMIT_WINDOW_SECS {
            log::info!(
                "ensure_fresh_load: slot {slot} commit window elapsed — proceeding on the \
                 latest load (commit is time-bounded; the witness may be unharvestable, e.g. a \
                 scene overlay the barrier's bare load never re-activated)"
            );
            break Ok(());
        }
        log::warn!(
            "ensure_fresh_load: slot {slot} stale load — device has not committed the previous \
             save; waiting"
        );
        // Wall-clock-paced (post-review amendment 6: `cancelled()` checked BEFORE each
        // slice's sleep, preserving the cancel test's timing contract) — measuring elapsed
        // time rather than counting nominal pump slices, exactly because `HidTransport::pump`
        // may return before `pump_ms` elapses (its trait doc). On real HW a slice already
        // blocks ~its own duration, so this sleep is usually near-zero there; it only bites
        // where a pump returns early, and only ever LENGTHENS the wait toward the intended
        // `retry_wait_ms`, never shortens it.
        let wait_start = std::time::Instant::now();
        while wait_start.elapsed() < Duration::from_millis(retry_wait_ms) {
            if cancelled() {
                break 'harvest Err(CANCELLED.to_string());
            }
            let slice_start = std::time::Instant::now();
            let _ = s.heartbeat();
            let _ = s.pump_collect(STALE_HEARTBEAT_MS);
            let slice_target = Duration::from_millis(STALE_HEARTBEAT_MS);
            let slice_elapsed = slice_start.elapsed();
            if slice_elapsed < slice_target {
                std::thread::sleep(slice_target - slice_elapsed);
            }
        }
    };
    drop(s);
    // The barrier just held a live rich session; give the HID stack the same settle gap
    // every other session-close → connect seam pays before the caller's own connect
    // (callers only gap around their OWN sessions — they can't see whether the barrier's
    // fast path skipped the session entirely). Err aborts the flow, so no gap needed.
    if out.is_ok() {
        crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
    }
    out
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
/// max reachable captured loudness — of its BASE sound: every capture here recalls base
/// first (`arm_measurement`), since a load activates the preset's saved `lastLoadedScene`
/// and an unrecalled capture would solve `C` for that scene instead.
pub fn measure_c(
    slot: u32,
    stimulus: &[f32],
    ref_level: f32,
    force_bypass: &[(String, String, bool)],
) -> Result<MeasuredC, String> {
    let ref_level = ref_level.clamp(0.05, 1.0);
    ensure_fresh_load(slot, &mut || crate::op_aborted())?;
    {
        let mut s = Session::connect_lean()?;
        s.load_preset(slot)?;
        settle_or_cancel(settle_after_load_ms())?;
    }
    settle_or_cancel(RECONNECT_GAP_MS)?;
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
            // The 5 s confirm gap is the single longest non-capture wait in a run — a Stop
            // here used to cost the gap PLUS a second full capture.
            settle_or_cancel(gap.as_millis() as u64)?;
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
fn capture_full_at(
    slot: u32,
    scene: Option<u32>,
    force_bypass: &[(String, String, bool)],
    stimulus: &[f32],
    ref_level: Option<f32>,
    tail_ms: u64,
    skip_load: bool,
) -> Result<audio::Capture, String> {
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
fn capture_full_at_params(
    slot: u32,
    scene: Option<u32>,
    force_bypass: &[(String, String, bool)],
    fs_params: &[(String, String, String, f32)],
    stimulus: &[f32],
    ref_level: Option<f32>,
    tail_ms: u64,
    skip_load: bool,
) -> Result<audio::Capture, String> {
    // The settles here are `sleep_abortable`: a Stop pressed anywhere in the ~1.9 s of
    // settling that brackets a capture bails immediately instead of being noticed only at
    // the next step seam. Safe to leave from any of these points — nothing is engaged or
    // written yet before the re-amp ON below.
    if !skip_load {
        let mut s = Session::connect_lean()?;
        s.load_preset(slot)?;
        settle_or_cancel(settle_after_load_ms())?;
        drop(s);
        settle_or_cancel(RECONNECT_GAP_MS)?;
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
/// then calls this) and `doctor_capture_on_session` (used by
/// `commands::doctor::doctor_apply`'s step (c), on the session step (b) just
/// wrote to).
pub(crate) fn capture_on_session(
    s: &mut Session,
    force_bypass: &[(String, String, bool)],
    stimulus: &[f32],
    ref_level: Option<f32>,
    tail_ms: u64,
) -> Result<audio::Capture, String> {
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

/// HW-probe MEASURE seam: the captured loudness of `slot` (or of the CURRENT
/// preset when `slot` is `None`) at its OWN stored level, optionally after
/// recalling the 0-based `scenes[]` wire index `scene`.
///
/// Delegates to `capture_full_at` so a diagnostic arm exercises the EXACT
/// production choreography (lean handshakes + the HW-validated settles) rather
/// than a parallel copy of it. `probe --measure-scene` used to hand-roll its own
/// — two back-to-back FULL `Session::connect()` handshakes with an early engage —
/// which drops `set_reamp_mode(true)` and yields silent captures that read as
/// device flakiness (the "no signal captured" class in `notes/leveling.md`).
pub fn capture_loudness_asis(
    slot: Option<u32>,
    scene: Option<u32>,
    stimulus: &[f32],
) -> Result<lufs::Loudness, String> {
    processed_loudness(capture_asis_full(slot, scene, stimulus))
}

/// Repro instrumentation: the FULL multi-channel as-is capture behind
/// [`capture_loudness_asis`], so a probe can report EVERY channel's loudness and
/// make the loudest-channel argmax observable instead of inferred.
pub fn capture_asis_full(
    slot: Option<u32>,
    scene: Option<u32>,
    stimulus: &[f32],
) -> Result<audio::Capture, String> {
    capture_full_at(
        slot.unwrap_or(0), // unused when skip_load
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
    slot: u32,
    stimulus: &[f32],
    ref_level: f32,
) -> Result<(Vec<f32>, u32), String> {
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
    slot: u32,
    stimulus: &[f32],
    ref_level: f32,
    force_bypass: &[(String, String, bool)],
) -> Result<(Vec<f32>, u32), String> {
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
/// Mixes down via `Capture::stereo_mix` (deterministic AVERAGE of USB-Out 1/2)
/// for the returned `samples` — band/PSD analysis needs one deterministic
/// channel, and on a stereo preset (ping-pong delay, hard-panned dual amps) an
/// argmax pick (`Capture::loudest_channel`) can flip L/R across runs and flip
/// spectral verdicts with it. This is UNRELATED to the leveling LUFS metric,
/// which is a 2-ch BS.1770 SUM over the un-mixed pair (`processed_lufs`) — see
/// [`doctor_capture_with_loudness`] for the seam that gets both.
/// `fs_params`: the measured footswitch's `param`-function `(group, node, param,
/// valueA)` writes — empty for base/scene sounds. See `capture_full_at_params`.
#[allow(clippy::too_many_arguments)]
pub fn doctor_capture(
    slot: u32,
    scene: Option<u32>,
    force_bypass: &[(String, String, bool)],
    fs_params: &[(String, String, String, f32)],
    stimulus: &[f32],
    ref_level: Option<f32>,
    tail_ms: u64,
    skip_load: bool,
) -> Result<(Vec<f32>, u32), String> {
    Ok(to_stereo(capture_full_at_params(
        slot,
        scene,
        force_bypass,
        fs_params,
        stimulus,
        ref_level,
        tail_ms,
        skip_load,
    )?))
}

/// Like [`doctor_capture`], but ALSO returns the capture's stereo-measured
/// `Loudness` (D4, PR2): bands/PSD keep the `stereo_mix` AVERAGE mixdown
/// unchanged (`samples`/`rate` below are byte-identical to `doctor_capture`'s),
/// but the REPORTED `integrated_lufs` a Doctor sound displays must come from the
/// same 2-ch BS.1770 sum every other output-side measurement uses now —
/// `stereo_mix`'s average can't be un-mixed back into a correct 2-ch measure, so
/// this measures the loudness from the UN-MIXED capture before mixing it down.
/// The one production Doctor capture site (`commands/doctor.rs`) uses this; every
/// dev probe harness keeps the plain `doctor_capture` seam (and its legacy
/// mono-on-mixdown `SoundProfile::integrated_lufs`) unchanged — see
/// `SoundProfile::from_capture_with_psd_loudness`'s doc.
/// `fs_params`: the measured footswitch's `param`-function `(group, node, param, valueA)`
/// writes — empty for base/scene sounds. See `capture_full_at_params`.
///
/// `validate` is the DOCTOR EXTERNAL-VALIDATION add-on — see
/// `crate::validate_log::emit_doctor`'s doc for the premise-check rationale. Emitted HERE
/// rather than beside the verdict because this is the only place that holds the un-mixed
/// capture.
#[allow(clippy::too_many_arguments)]
pub fn doctor_capture_with_loudness(
    slot: u32,
    scene: Option<u32>,
    force_bypass: &[(String, String, bool)],
    fs_params: &[(String, String, String, f32)],
    stimulus: &[f32],
    ref_level: Option<f32>,
    tail_ms: u64,
    skip_load: bool,
    validate: Option<&crate::validate_log::ValidationRow>,
) -> Result<(Vec<f32>, u32, lufs::Loudness), String> {
    let cap = capture_full_at_params(
        slot,
        scene,
        force_bypass,
        fs_params,
        stimulus,
        ref_level,
        tail_ms,
        skip_load,
    )?;
    let samples = cap.stereo_mix();
    let sample_rate = cap.sample_rate;
    let loudness = measure_processed(&cap)?;
    if let Some(row) = validate {
        crate::validate_log::emit_doctor(row, &cap, &loudness);
    }
    Ok((samples, sample_rate, loudness))
}

/// Deterministic stereo mixdown (`Capture::stereo_mix` — average of USB-Out 1/2;
/// an argmax pick, `Capture::loudest_channel`, can flip L/R across runs on a
/// stereo preset and flip spectral verdicts with it) — the shared tail of every
/// Doctor capture seam that needs band/PSD `samples`, not the leveling LUFS
/// metric (a 2-ch BS.1770 SUM over the un-mixed pair — see
/// [`doctor_capture_with_loudness`]).
fn to_stereo(cap: audio::Capture) -> (Vec<f32>, u32) {
    let sr = cap.sample_rate;
    (cap.stereo_mix(), sr)
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
/// an argmax `loudest_channel` pick — see `doctor_capture`'s doc for why). The
/// scene recall + force-bypass writes land on the UNSAVED edit
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
    crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
    Ok(to_stereo(capture_full_at(
        0, // slot unused: skip_load
        scene,
        force_bypass,
        stimulus,
        ref_level,
        tail_ms,
        true,
    )?))
}

/// Doctor A/B AFTER-clip seam on an ALREADY-OPEN session — no load, no scene
/// recall. Used ONLY by `doctor_apply`'s step (c), which must capture on the
/// SAME session `ops_session` just applied the prescription ops to: a fresh
/// reconnect (what `doctor_capture_current` does) would recall the scene again,
/// reverting those unsaved ops before this capture ever ran and silently
/// rendering an identical AFTER clip. `fs_params` re-plays the measured
/// footswitch's `param`-function `valueA` writes on THIS session (the BEFORE
/// capture wrote them on its own throwaway connection, so without the re-play
/// the A/B would audition a different param state than the diagnosis) — written
/// before the isolation bypasses, `measure_fs_state`'s order. Stereo mixdown:
/// see `to_stereo`.
pub fn doctor_capture_on_session(
    s: &mut Session,
    force_bypass: &[(String, String, bool)],
    fs_params: &[(String, String, String, f32)],
    stimulus: &[f32],
    ref_level: Option<f32>,
    tail_ms: u64,
) -> Result<(Vec<f32>, u32), String> {
    for (g, n, p, v) in fs_params {
        s.change_parameter(g, n, p, *v)?;
    }
    Ok(to_stereo(capture_on_session(
        s,
        force_bypass,
        stimulus,
        ref_level,
        tail_ms,
    )?))
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
    slot: u32,
    scene: Option<u32>,
    force_bypass: &[(String, String, bool)],
    fs_value: Option<((String, String, String), f32)>,
    stimulus: &[f32],
    validate: Option<&crate::validate_log::ValidationRow>,
) -> Result<lufs::Loudness, String> {
    // Resolved ONCE, up front: an unarmed run must not clone a ~2.7 MB capture per
    // floor-guard attempt just to throw it away.
    let keep = validate.is_some() && crate::validate_log::log_path().is_some();
    // The capture the returned loudness was measured from — the LAST attempt the floor
    // guard made, so the dumped WAV and the reported number always describe one capture.
    let mut kept: Option<audio::Capture> = None;
    let measured = if let Some(((g, n, p), v)) = fs_value {
        {
            let mut s = Session::connect_lean()?;
            s.load_preset(slot)?;
            crate::settle(Duration::from_millis(settle_after_load_ms()));
        }
        crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
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
            crate::settle(Duration::from_millis(settle_after_load_ms()));
        }
        crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
        let mut s = Session::connect_lean()?;
        s.load_scene(scene)?;
        crate::settle(Duration::from_millis(SETTLE_AFTER_SET_MS));
        set_knob(&mut s, &LevelKnob::PresetLevel, 1.0, None)?;
        crate::settle(Duration::from_millis(SETTLE_AFTER_SET_MS));
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

/// The reachable common target for a run whose ceilings were ALREADY measured — the same
/// `min(C − offset) − headroom` math as [`level_setlist`]'s pass-1→target step, but reusing
/// the run's measured `C` values (zero re-capture). Each `ceilings` entry is
/// `(c_lufs, offset_lu)`: the raw measured ceiling and that sound's per-instrument
/// Fletcher–Munson playback offset. The returned target is in PRE-offset space (the runner
/// adds `offset` back), so an entry leveled `offset` hotter still fits under its `C` — exactly
/// [`common_target`] on the offset-adjusted ceilings. `None` when no ceiling is finite.
pub fn common_reachable_target(ceilings: &[(f64, f64)], headroom_lu: f64) -> Option<f64> {
    let cs: Vec<f64> = ceilings.iter().map(|(c, offset)| c - offset).collect();
    common_target(&cs, headroom_lu)
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
    apply_levels(
        slot,
        stimulus,
        &[(knob, final_level)],
        opts,
        reload_preset,
        None,
        &[],
    )
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
    saved: Option<&serde_json::Value>,
    // A4: a BASE job's isolation list — "base means base" (a scene job's is always empty; it
    // rides its own overlay instead). Written AFTER `set_knobs`/the intended-`presetLevel`
    // assert and BEFORE the settle/engage, mirroring `arm_measurement`'s "SCENE CONTEXT FIRST,
    // ISOLATION LAST" ordering (that seam is for a single `LevelKnob`; this one already has its
    // own recall via `set_knobs`, so the ordering is reproduced here rather than routed through
    // it).
    force_bypass: &[(String, String, bool)],
) -> Result<(bool, Option<f64>), String> {
    if reload_preset {
        ensure_fresh_load(slot, &mut || crate::op_aborted())?;
        let mut s = Session::connect()?;
        s.load_preset(slot)?;
        // A verify capture needs the DSP audio fully settled; a pure write does not.
        let settle = if opts.verify {
            settle_after_load_ms()
        } else {
            SETTLE_BEFORE_WRITE_MS
        };
        crate::settle(Duration::from_millis(settle));
    }
    crate::settle(Duration::from_millis(RECONNECT_GAP_MS));

    let mut verify_lufs = None;
    let mut s = Session::connect()?;
    set_knobs(&mut s, targets, saved)?; // set before any re-amp engage (latched)
                                        // Re-assert the run's own `presetLevel` AFTER `set_knobs` — its scene recall runs the
                                        // device's level-apply and would otherwise leave the verify capture rendering at the
                                        // SAVED level (see `LevelOptions::intended_preset_level`). Skipped when a PresetLevel
                                        // target is in this batch: that value is the one under measurement and must win.
    if let Some(pl) = opts.intended_preset_level {
        if !targets
            .iter()
            .any(|(k, _)| matches!(k, LevelKnob::PresetLevel))
        {
            set_knob(&mut s, &LevelKnob::PresetLevel, pl, None)?;
        }
    }
    // ISOLATION LAST — after every knob/level write above, before the settle/engage below
    // (`capture_on_session`'s rule: a scene recall inside `set_knobs` would otherwise revert
    // bypasses written earlier on this connection).
    for (g, n, byp) in force_bypass {
        s.change_parameter_bool(g, n, "bypass", *byp)?;
    }
    crate::settle(Duration::from_millis(SETTLE_AFTER_SET_MS));

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
            let _ = set_knobs(&mut s, targets, saved);
            crate::settle(Duration::from_millis(150));
        }
    }

    // Any unsaved `presetLevel` must ride through the pre-save recall via
    // `recall_reassert_save` (see its doc for the load-level-apply clobber this
    // guards). The re-assert value is the PresetLevel target when one is in the
    // batch; Block-knob targets need none (overlay writes persist through the
    // recall — the footswitch `switch_at_target` re-run spec proves it).
    let reassert_pl = targets
        .iter()
        .find(|(k, _)| matches!(k, LevelKnob::PresetLevel))
        .map(|(_, v)| *v);
    if opts.save {
        if opts.verify {
            // A session that has toggled re-amp silently DROPS the save (HW: after the
            // verify engage/disengage, `saveCurrentPreset` on the same session persists
            // nothing — `probe --bisect-scene … save` with TMP_BISECT_SAVE_MODE=same vs
            // fresh). The written values survive in the device's working copy across
            // reconnects, so save on a FRESH connection.
            drop(s);
            crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
            let mut s2 = Session::connect()?;
            recall_reassert_save(&mut s2, slot, opts.restore_scene, reassert_pl)?;
        } else {
            recall_reassert_save(&mut s, slot, opts.restore_scene, reassert_pl)?;
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
    crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
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
/// pure writes, NO measurement. Slot-keyed destructive write ⇒ a non-destructive name read
/// guards it first, so a drifted list fails loudly instead of restoring onto a different preset.
///
/// No `recall_reassert_save` here, on purpose: that seam's pre-save revert is a LEAN-session
/// observation, and this is the other shape. This session `begin_live_edit`s and
/// `load_preset`s the slot itself before the `set_preset_level`, and
/// `probe_redistribute_persist_check` (`probe_api/slot_write.rs`) HW-validated exactly this
/// write→recall→recall→save shape carrying an unsaved `presetLevel` through the save
/// (read-back 0.42 against a saved 0.53) — it is the go/no-go gate the redistribution
/// feature was built on. See `recall_reassert_save`'s doc for the two shapes side by side.
///
/// `knobs` is written GROUPED by `scene_slot` (one `set_knobs` call per distinct scene,
/// base included), not one `set_knob` call per knob: a parallel-merged preset's base or a
/// single scene can carry ≥2 restored knobs, and calling `set_knob` per knob would
/// re-`load_scene` the SAME target between them, reverting the earlier knob's just-written
/// value before this function ever saves (`set_knobs`'s own doc: "calling `set_knob` per
/// knob re-`load_scene`s between writes, which reverts the prior knob's unsaved value").
pub fn restore_redistribution(
    slot: u32,
    preset_level: f32,
    knobs: &[PrevKnobWrite],
    expected_name: &str,
) -> Result<(), String> {
    // The saved doc `set_knobs` needs for a per-scene restore write, read before the
    // name-guard session (`read_saved_preset` sleeps after itself).
    let saved = saved_for_scene_knobs(
        slot,
        &knobs
            .iter()
            .map(|k| LevelKnob::Block {
                group_id: k.group_id.clone(),
                node_id: k.node_id.clone(),
                parameter_id: "outputLevel".to_string(),
                scene_slot: k.scene_slot,
            })
            .collect::<Vec<_>>(),
    );
    {
        let mut s = Session::connect()?;
        let list = s.list_my_presets()?;
        verify_slot_name(&list, slot, expected_name)?;
    }
    ensure_fresh_load(slot, &mut || crate::op_aborted())?;
    crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    s.load_preset(slot)?;
    for _ in 0..8 {
        let _ = s.heartbeat();
        let _ = s.pump_collect(150);
    }
    s.set_preset_level(preset_level)?;
    crate::settle(Duration::from_millis(SETTLE_AFTER_SET_MS));
    write_grouped_knobs(&mut s, knobs, saved.as_ref())?;
    recall_base(&mut s)?;
    s.save_current_preset(slot)?;
    register_slot_save(slot, SaveWitness::PresetLevel(preset_level));
    Ok(())
}

/// Write `knobs` GROUPED by `scene_slot` (one `set_knobs` call per distinct scene,
/// base included) on an already-open session — split out of `restore_redistribution`
/// so the grouping is unit-testable against `SimDevice` without a real HID
/// connection (`restore_redistribution` itself needs `Session::connect()` +
/// `list_my_presets`' "My Presets" echo, which the fake doesn't model). See
/// `restore_redistribution`'s doc for why grouping (not one `set_knob` call per
/// knob) matters.
fn write_grouped_knobs(
    s: &mut Session,
    knobs: &[PrevKnobWrite],
    saved: Option<&serde_json::Value>,
) -> Result<(), String> {
    let level_knobs: Vec<LevelKnob> = knobs
        .iter()
        .map(|k| LevelKnob::Block {
            group_id: k.group_id.clone(),
            node_id: k.node_id.clone(),
            parameter_id: "outputLevel".to_string(),
            scene_slot: k.scene_slot,
        })
        .collect();
    let mut scenes_seen: Vec<Option<u32>> = Vec::new();
    for k in knobs {
        if !scenes_seen.contains(&k.scene_slot) {
            scenes_seen.push(k.scene_slot);
        }
    }
    for scene in scenes_seen {
        let group: Vec<(&LevelKnob, f32)> = level_knobs
            .iter()
            .zip(knobs)
            .filter(|(_, k)| k.scene_slot == scene)
            .map(|(lk, k)| (lk, k.value))
            .collect();
        set_knobs(s, &group, saved)?;
        let _ = s.heartbeat();
    }
    Ok(())
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
                if let Err(e) = restore_saved_preset(slot) {
                    log::warn!(
                        "restore_saved_preset failed after no-signal measure (slot {slot}): {e}"
                    );
                }
                return Ok(LevelResult {
                    slot,
                    scene_slot: None,
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
                    clamp_kind: Some(crate::headroom_trade::ClampKind::NoAuthority),
                    clamp_reason: Some("no signal on USB 1/2".into()),
                    verify_by_ear: false,
                    previous_level: None,
                    true_peak_dbtp: None,
                    persist_mismatch: None,
                    trade: None,
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
                    scene_slot: None,
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
                    // Idempotency skip: nothing was solved, so nothing clamped.
                    clamp_kind: None,
                    clamp_reason: None,
                    verify_by_ear: false,
                    previous_level: None,
                    true_peak_dbtp: Some(predicted_true_peak_dbtp(
                        m.true_peak_dbtp,
                        ref_level,
                        final_level,
                    )),
                    persist_mismatch: None,
                    trade: None,
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
            scene_slot: None,
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
            // The one-shot `presetLevel` lane has no wet floor and no routing clamp of its
            // own (that early-returns above), so a clamp here is the plain headroom case.
            clamp_kind: crate::headroom_trade::ClampKind::from_flags(clamped, false, None),
            clamp_reason: None,
            verify_by_ear: false,
            previous_level: None,
            true_peak_dbtp: Some(predicted_true_peak_dbtp(
                m.true_peak_dbtp,
                ref_level,
                final_level,
            )),
            persist_mismatch: None,
            trade: None,
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
            scene_slot: None,
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
            clamp_kind: crate::headroom_trade::ClampKind::from_flags(clamped, false, None),
            clamp_reason: None,
            verify_by_ear: false,
            previous_level: None,
            true_peak_dbtp: None,
            persist_mismatch: None,
            trade: None,
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
        /// level the base value — `set_knob`/`set_knobs` still recall base
        /// explicitly (via `BASE_SCENE_SLOT`) before writing: a preset load
        /// activates its SAVED `lastLoadedScene`, not necessarily base (HW), so
        /// omitting the recall would silently write whatever scene the connection
        /// defaulted to.
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
/// device round-trips; ≈0.3 LU is well within audible-match for leveling. SCENE
/// band only — the footswitch lane's acceptance is the tighter [`FS_TOL_LU`]; see
/// its doc for which.
pub(crate) const KNOB_TOL_LU: f64 = 0.3;
/// Footswitch-lane acceptance band — user-decided TRUE ±0.1 vs target, tighter than the
/// scene lane's `KNOB_TOL_LU` (0.3). Applies ONLY to solve_footswitch's at-target checks
/// (the `err(...)` distance-to-target gates), `classify_fs_outcome`, and `switch_at_target`
/// — the FLATNESS/no-authority checks in the same functions stay on `KNOB_TOL_LU` (a
/// noise-floor threshold, not an acceptance band; HW measured ~0.12 LU run-to-run noise).
/// `FS_CORRECT_MAX` was doubled alongside this to compensate — a tighter band needs more
/// bracket-aware iterates to converge. HW noise caveat: 0.1 LU sits close to the measured
/// capture noise floor, so a well-converged FS solve may occasionally read `unconverged` on
/// noise alone; a re-run absorbs it (idempotency skip on the next in-tolerance pass).
const FS_TOL_LU: f64 = 0.1;
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

/// The field-8 saved doc `set_knobs` needs for its Scene Edit decision, for the ENTRY
/// POINTS that can't be handed one: the probe/legacy/restore seams whose callers live
/// outside the leveling run that already read it (`level_preset_block`,
/// `mute_floor_report`, `restore_redistribution`, the bench runner). Reads ONLY when a
/// scene target is actually present, so every base/`presetLevel` path pays nothing, and
/// ONCE per run — the batched scene runners take the command's single read instead
/// (`read_saved_preset`'s read-once contract). `None` (read failed) makes `set_knobs`
/// refuse the scene write rather than guess.
fn saved_for_scene_knobs<'a>(
    slot: u32,
    knobs: impl IntoIterator<Item = &'a LevelKnob>,
) -> Option<serde_json::Value> {
    knobs
        .into_iter()
        .any(|k| {
            matches!(
                k,
                LevelKnob::Block {
                    scene_slot: Some(_),
                    ..
                }
            )
        })
        .then(|| read_saved_preset(slot))
        .flatten()
}

/// Recall base explicitly (`BASE_SCENE_SLOT`) + the standard post-recall settle —
/// shared by every base-context write/save seam that isn't already inside a
/// per-scene Scene Edit dance (which pays its own `SETTLE_AFTER_SCENE_RECALL_MS`
/// / `SETTLE_AFTER_SCENE_EDIT_MS` pair instead, in `set_knobs`).
fn recall_base(s: &mut Session) -> Result<(), String> {
    s.load_scene(crate::session::BASE_SCENE_SLOT)?;
    crate::settle(Duration::from_millis(SETTLE_AFTER_SET_MS));
    Ok(())
}

/// Recall the preset's ORIGINAL `lastLoadedScene` right before a save, so the save
/// re-stamps it instead of whatever scene the run left active (HW: a base-context
/// leveling save silently rewrote the preset's on-load scene, 3 → 8). Unsaved writes
/// survive the recall (HW, `probe --defer-scenes`). `None` = no recall (old behavior).
/// The ONE pre-save recall shared by `apply_levels`, `save_deferred_scene_writes`, and
/// `write_fs_values_on_session` — a settle-timing fix here fixes all three.
fn recall_original_scene(s: &mut Session, restore_scene: Option<u32>) -> Result<(), String> {
    if let Some(scene) = restore_scene {
        s.load_scene(scene)?;
        crate::settle(Duration::from_millis(SETTLE_AFTER_SET_MS));
    }
    Ok(())
}

/// The ONE pre-save sequence every batch/deferred save shares: recall the preset's
/// original scene, re-assert any unsaved `presetLevel` the recall just reverted,
/// then save. The recall's `loadScene` — base included — runs the device's own
/// level-apply (the same mechanism as the "`load_preset` + `set_preset_level` in
/// one connection → the set is overridden" gotcha), so an unsaved working-copy
/// `presetLevel` is silently reverted to the SAVED value right before the save
/// persists it (HW: `probe --levelpreset 400 -24 save` solved 0.3096 and the saved
/// doc still read the prior 0.32; caught by the online `level.online.spec.ts` base
/// idempotency test). The revert is a LEAN-session behavior — every caller of this
/// seam saves on a connection that did NOT `load_preset` in-session (the apply
/// path's fresh connect, the post-verify fresh `s2`), and that is the shape both HW
/// observations above come from. A live-edit session that loaded the preset ITSELF
/// carries an unsaved `presetLevel` through scene recalls — base included — and the
/// save (HW: `probe_redistribute_persist_check`'s `write_three_and_save`, fw
/// 1.8.45-era: pl 0.42 read back against a saved 0.53 after `loadScene(1)` +
/// `loadScene(BASE)` + save), which is why `restore_redistribution` does not route
/// through this seam. Node/overlay writes are immune in either shape (the footswitch
/// `switch_at_target` re-run spec proves `valueA` persists through the recall), so
/// only `reassert_pl` — the unsaved level a caller solved (`apply_levels`) or
/// raised (`redistribute_clamped_headroom`) — needs re-writing, and only when a
/// recall actually ran. Timing stays under the idle-gap cliff: recall +
/// `SETTLE_AFTER_SET_MS` → set + `SETTLE_AFTER_SET_MS` → save. The re-assert
/// deliberately does NOT defeat the restore: `setPresetLevel` emits no
/// `loadScene`, so the scene the save stamps is still the recalled one (pinned by
/// `recall_reassert_save_replays_the_unsaved_level_after_the_recall`).
fn recall_reassert_save(
    s: &mut Session,
    slot: u32,
    restore_scene: Option<u32>,
    reassert_pl: Option<f32>,
) -> Result<(), String> {
    recall_original_scene(s, restore_scene)?;
    if let (Some(pl), Some(_)) = (reassert_pl, restore_scene) {
        s.set_preset_level(pl)?;
        crate::settle(Duration::from_millis(SETTLE_AFTER_SET_MS));
    }
    s.save_current_preset(slot)?;
    // Whenever this save is carrying a presetLevel (the base/restore/redistribution path —
    // `reassert_pl` is `Some` regardless of whether a recall made the re-set necessary), it IS
    // the witness: register it so a same-slot load inside the lazy-commit window waits for it.
    if let Some(pl) = reassert_pl {
        register_slot_save(slot, SaveWitness::PresetLevel(pl));
    }
    Ok(())
}

/// Set the chosen knob to `value` on an open session (before re-amp engage). The
/// single-knob case of `set_knobs` — see its doc for the recall/write ordering
/// rules and what `saved` is for. Must be the FIRST write on the connection when
/// `knob` is a `Block`: its own scene recall, base or otherwise, reverts an
/// earlier unsaved `presetLevel` on this session (the lean-session level-apply —
/// callers here connect without an in-session `load_preset`; see
/// `recall_reassert_save`'s shape split) AND re-asserts that scene's own bypass
/// state (see `capture_on_session`'s doc), so a caller doing force-bypass
/// isolation must write the bypasses AFTER this call, never before
/// (`measure_knob_at`/`measure_fs_at` follow that order).
fn set_knob(
    s: &mut Session,
    knob: &LevelKnob,
    value: f32,
    saved: Option<&serde_json::Value>,
) -> Result<(), String> {
    set_knobs(s, &[(knob, value)], saved)
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
        // `bypass` is a distinct wire message (`ChangeParameter.boolVal`, field 7) from
        // every other block param (a plain float field) — see `Session::change_parameter`
        // vs `change_parameter_bool`. A bypass value rides the same `(LevelKnob, f32)` shape
        // as every other knob (0.0/1.0 encoding), so the dispatch lives here rather than
        // widening `LevelKnob`. NOT the same shape as
        // `capture_on_session`/`measure_knob_at`/`measure_fs_at`'s typed
        // `&[(String, String, bool)]` force-bypass side channels — those write BEFORE a
        // separate connect/engage sequence, outside `set_knobs` entirely.
        LevelKnob::Block {
            group_id,
            node_id,
            parameter_id,
            ..
        } if parameter_id == "bypass" => {
            s.change_parameter_bool(group_id, node_id, parameter_id, value != 0.0)
        }
        LevelKnob::Block {
            group_id,
            node_id,
            parameter_id,
            ..
        } => s.change_parameter(group_id, node_id, parameter_id, value),
    }
}

/// Write a SET of block knobs that all belong to the SAME scene (or all to base),
/// doing the scene recall ONCE up front (NOT per knob — calling this per knob
/// individually re-`load_scene`s between writes, which reverts the prior knob's
/// unsaved value; `restore_redistribution` groups its knobs by scene for exactly
/// this reason). Ordering: load scene → (per-scene only) enable Scene Edit on
/// every per-scene block → ONE settle → write every value.
///
/// * A target set with a `Block { scene_slot: Some(n), .. }` recalls scene `n`,
///   then enables Scene Edit on every per-scene block whose node has NO overlay in
///   that scene — and ONLY those. The enable RESEEDS the node's whole scene overlay
///   from base (HW 3-cell matrix, `probe_api/slot_write.rs`), so it is required
///   where the overlay is absent (it is what materialises it; without it the write
///   LEAKS TO BASE) and actively destructive where the overlay exists (the write
///   lands on the overlay anyway, and the enable wipes the scene's other stored
///   params). `saved` — the slot's field-8 preset JSON, read ONCE per run and
///   threaded (`probe_api::scene_jobs::read_saved_preset`) — is the only source
///   that can tell the two apart: after `overlay_scene_onto_graph` a base value is
///   indistinguishable from an overlaid one, so the live docs can't answer it.
///   The decision itself is NOT taken here: it is
///   `scene_jobs::scene_write_verdict_for_param`, the ONE write-landing policy this
///   lane shares with the Doctor's prescription apply, so a `BypassOnly` node can't
///   be refused in one lane and written in the other.
///   `SceneOverlay::Unknown` (truncated read) or no `saved` at all ⇒ REFUSE the
///   batch before any device write; both write shapes corrupt from there. Mostly so
///   does `SceneOverlay::BypassOnly` — an overlay carrying only the bypass family
///   means that block's Scene Edit flag is DISABLED and the scene SHARES its knobs
///   with base, so the enable-dropped write lands on BASE and changes every sharing
///   scene (HW-verified fw 1.8.45) — UNLESS `scene_jobs::shared_write_is_scene_local`
///   confirms the leak is audible ONLY in this scene (every other sharing scene stays
///   bypassed, or pins the param with its own overlay), in which case the verdict
///   allows the base-landing write through as `WriteDirect` too. The refusal is a
///   per-scene skip, never a batch abort: `run_scene_jobs` turns a solve `Err` into a
///   `failed_scene_outcome` and continues, exactly like a `build_scene_jobs`
///   "no active guitar amp" skip.
///   Re-asserted on EVERY connection (scene + scene-edit don't survive the
///   leveller's reconnects) — including the enable, since `saved` is a pre-run
///   snapshot: an overlay this run just materialised still reads Absent, so the
///   enable repeats and reseeds again. Harmless, because the leveled write always
///   follows it inside the same batch — do NOT "optimise" it into a cached
///   per-node enabled set, which would drop the enable on a later connection where
///   the device no longer has scene edit armed.
///   The settles must stay SHORT: the device silently drops a write after a
///   ~400–450 ms pre-write IDLE gap (see `SETTLE_AFTER_SCENE_EDIT_MS` — a
///   per-command cliff, so a settle is slept ONLY when its command was sent);
///   the rare too-fast leak-to-base race is covered by the verify + correction
///   pass.
/// * A base-only target set (`Block { scene_slot: None, .. }`, no `Some`) gets
///   an explicit base recall — a bare write with no preceding `loadScene` lands
///   in whatever scene the connection defaults to (the preset's saved
///   `lastLoadedScene`), not necessarily base (HW). No Scene Edit: base params
///   are never scene-specific.
/// * A `PresetLevel`-only target set gets NO recall at all: `setPresetLevel` is
///   a different wire message and a global multiplier, not a scene-scoped
///   `changeParameter`, so a recall there would only risk reverting an unsaved
///   presetLevel write for no benefit.
fn set_knobs(
    s: &mut Session,
    targets: &[(&LevelKnob, f32)],
    saved: Option<&serde_json::Value>,
) -> Result<(), String> {
    // ONE scene per batch, ENFORCED not just documented: only the first scene found is
    // recalled below, so a batch mixing two scenes would land every write in that first
    // scene's overlay — silently, with each write confirming normally. Refuse instead.
    let mut scenes: Vec<u32> = targets
        .iter()
        .filter_map(|(k, _)| match k {
            LevelKnob::Block {
                scene_slot: Some(slot),
                ..
            } => Some(*slot),
            _ => None,
        })
        .collect();
    scenes.sort_unstable();
    scenes.dedup();
    if scenes.len() > 1 {
        return Err(format!(
            "set_knobs: batch mixes scenes {scenes:?}; only one scene may be recalled per batch, \
             so the others would silently write into scene {}",
            scenes[0]
        ));
    }
    let scene = scenes.first().copied();
    let has_base_block = targets.iter().any(|(k, _)| {
        matches!(
            k,
            LevelKnob::Block {
                scene_slot: None,
                ..
            }
        )
    });
    // ...and no BASE block may ride along with a scene block, for the same reason one step
    // removed: the branch below recalls the scene whenever `scene` is Some, so a base-scoped
    // knob (`scene_slot: None`) in that batch would be written under the scene overlay
    // instead of base — silently, confirming normally, exactly like the two-scene case.
    // One connection can hold one scene context, so base and scene targets cannot share a
    // batch; split them into two calls.
    if scene.is_some() && has_base_block {
        return Err(format!(
            "set_knobs: batch mixes base and scene {} targets; the scene recall would capture \
             the base write too — split them into separate calls",
            scene.unwrap_or_default()
        ));
    }
    // Which nodes need the Scene Edit enable — decided BEFORE any device write, so an
    // unanswerable overlay state refuses with the preset untouched. The verdict itself is
    // checked per (node, PARAM) — `scene_write_verdict_for_param`'s BypassOnly arm can answer
    // two params on the same node differently (one audibility-guarded, one not) — but the
    // enable send stays deduped by (group_id, node_id): two targets on one node share one
    // enable, and re-enabling an already-enabled node re-triggers the reseed (HW 3-cell
    // matrix) — not cosmetic. Neither `WriteDirect` arm (Full overlay, or an audibility-
    // guarded BypassOnly leak) ever needs the enable, so this dedup can't hide a missed one.
    let mut needs_enable: Vec<(&str, &str)> = Vec::new();
    if let Some(scene) = scene {
        let mut checked: Vec<(&str, &str, &str)> = Vec::new();
        for (k, _) in targets {
            if let LevelKnob::Block {
                group_id,
                node_id,
                parameter_id,
                scene_slot: Some(_),
            } = k
            {
                let key = (group_id.as_str(), node_id.as_str(), parameter_id.as_str());
                if checked.contains(&key) {
                    continue;
                }
                checked.push(key);
                // ONE write-landing policy for every scene-writing lane
                // (`scene_jobs::scene_write_verdict_for_param`, shared with the Doctor's
                // apply) — the four overlay states can never be answered two ways. The
                // no-saved-read case stays local: the verdict is a function OF a saved
                // document, so having none is this caller's own gap, and it keeps its own
                // wording.
                let Some(sv) = saved else {
                    return Err(format!(
                        "set_knobs: refusing to write {group_id}/{node_id} in scene {scene} — \
                         the saved preset does not say whether that node already has a scene \
                         overlay (no saved-preset read), and both write shapes corrupt it \
                         (enable reseeds an existing overlay from base; omitting it leaks the \
                         write to base)"
                    ));
                };
                match scene_write_verdict_for_param(sv, scene, node_id, parameter_id) {
                    SceneWriteVerdict::WriteDirect { .. } => {}
                    SceneWriteVerdict::NeedsEnable => {
                        let node_key = (group_id.as_str(), node_id.as_str());
                        if !needs_enable.contains(&node_key) {
                            needs_enable.push(node_key);
                        }
                    }
                    SceneWriteVerdict::Refuse { reason, .. } => {
                        return Err(format!("set_knobs: {reason}"))
                    }
                }
            }
        }
    }
    // Repro instrumentation: wall-clock every step relative to the loadScene recall,
    // to observe idle gaps crossing the ~400-450 ms silent-drop cliff (see
    // `SETTLE_AFTER_SCENE_EDIT_MS`).
    let t0 = std::time::Instant::now();
    if let Some(scene) = scene {
        s.load_scene(scene)?;
        log::info!(
            "set_knobs[t] scene {scene}: loadScene returned at {} ms",
            t0.elapsed().as_millis()
        );
        crate::settle(Duration::from_millis(SETTLE_AFTER_SCENE_RECALL_MS));
        for (group_id, node_id) in &needs_enable {
            s.set_node_scene_edit(group_id, node_id, true)?;
            log::info!(
                "set_knobs[t]: SceneEdit({node_id}) returned at {} ms",
                t0.elapsed().as_millis()
            );
        }
        // Settle ONLY when an enable was actually sent — a settle for a command that
        // wasn't is pure idle and rides the ~400-450 ms silent-drop cliff (see
        // `SETTLE_AFTER_SCENE_EDIT_MS`'s doc for the HW evidence).
        if !needs_enable.is_empty() {
            crate::settle(Duration::from_millis(SETTLE_AFTER_SCENE_EDIT_MS));
        }
    } else if has_base_block {
        recall_base(s)?;
    }
    for (k, v) in targets {
        set_knob_value_only(s, k, *v)?;
        log::info!(
            "set_knobs[t]: write {}={v:.4} returned at {} ms",
            k.label(),
            t0.elapsed().as_millis()
        );
    }
    Ok(())
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
pub(crate) fn processed_lufs(cap: Result<audio::Capture, String>) -> Result<f64, String> {
    processed_loudness(cap).map(|l| l.integrated_lufs)
}

/// Like [`processed_lufs`] but keeps the full meter reading (integrated + short-term
/// max), for paths that report the capture's dynamics spread alongside the level.
fn processed_loudness(cap: Result<audio::Capture, String>) -> Result<lufs::Loudness, String> {
    processed_loudness_of(&cap?)
}

/// [`processed_loudness`] on a BORROWED capture — for the one caller that must keep the
/// PCM alive past the measurement (the strict re-measure's external-validation dump,
/// `measure_sound_asis_strict`). Same verdict, same sentinel error.
fn processed_loudness_of(cap: &audio::Capture) -> Result<lufs::Loudness, String> {
    let m = measure_processed(cap)?;
    m.integrated_lufs
        .is_finite()
        .then_some(m)
        .ok_or_else(|| NO_SIGNAL_CAPTURED.to_string())
}

/// The ONE engaged/floor criterion this crate has: finite chain audio meaningfully
/// above the stationary floor, with real dynamics (a floor read is near-flat). NaN
/// comparisons are false, so a failed measure reads "not engaged".
///
/// `danger.md`: a silent/failed re-amp inject reads as the device's stationary OUTPUT
/// FLOOR — a real number for the wrong signal. This is what `probe --measure-current`'s
/// FLOOR/SILENT headline and `validate_log`'s `engaged` verdict both stamp from, so a
/// consumer never has to re-derive it (`probe_api::level::is_engaged` delegates here).
pub(crate) fn is_engaged(l: &lufs::Loudness) -> bool {
    l.integrated_lufs.is_finite() && l.integrated_lufs > -50.0 && l.spread_lu() > 0.5
}

/// The metering convention shared by every OUTPUT-side measurement hub in this
/// module: 2-ch BS.1770 over the processed USB pair (`Capture::processed_stereo`),
/// falling back to [`lufs::measure_mono`] on channel 0 for a genuinely 1-channel
/// capture (never duplicating it into fake dual-mono — see `processed_stereo`'s
/// doc). The INPUT/stimulus side (`stimulus_spread_lu`, calibration, etc.) never
/// calls this — it stays on `measure_mono` directly. `pub(crate)` so a probe
/// diagnostic headline can share the exact production convention instead of a
/// parallel per-channel re-check (`probe_api::level::probe_measure_current_lufs`).
pub(crate) fn measure_processed(cap: &audio::Capture) -> Result<lufs::Loudness, String> {
    let sample_rate = cap.sample_rate;
    match cap.processed_stereo() {
        Some(stereo) => lufs::measure_stereo(&stereo, sample_rate),
        None => lufs::measure_mono(&cap.channel(0), sample_rate),
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
) -> Result<lufs::Loudness, String> {
    processed_loudness(engage_capture_disengage(s, stimulus))
}

/// [`engage_measure_disengage`] stopping one step short — returns the raw capture
/// instead of its loudness, for the caller that must also write the PCM to disk
/// (`measure_sound_asis_strict`'s external-validation dump). The engage/settle/capture/
/// disengage sequence is IDENTICAL: this is an extraction, not a second choreography.
pub(crate) fn engage_capture_disengage(
    s: &mut Session,
    stimulus: &[f32],
) -> Result<audio::Capture, String> {
    let _ = s.set_reamp_mode(true)?;
    // Same no-early-return rule as `capture_full_at`: re-amp is engaged, the OFF must fire.
    let _ = settle_abortable(SETTLE_AFTER_REAMP_MS);
    let cap = audio::reamp_capture(stimulus, RATE, CAPTURE_TAIL_MS);
    let _ = s.set_reamp_mode(false);
    cap
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
/// without this the capture renders at the level the DEVICE HAS SAVED: stale while a save is
/// still inside its lazy-commit window, and stale by the whole raise while a headroom trade
/// holds an unsaved `presetLevel`. `None` = assert nothing (capture at the preset's own
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
fn measure_scene_asis(
    scene_slot: u32,
    stimulus: &[f32],
    intended_preset_level: Option<f32>,
    force_bypass: &[(String, String, bool)],
) -> Result<lufs::Loudness, String> {
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
///    the device HAS SAVED — stale inside a save's lazy-commit window, and stale by the whole
///    raise while a headroom trade holds an unsaved `presetLevel`
///    (`recall_reassert_save`'s doc carries the HW evidence). Written as a plain
///    `setPresetLevel` (no recall of its own — `set_knobs`' `PresetLevel`-only branch), so it
///    cannot revert the knob write that precedes it, and no settle is added: the isolation
///    writes and the single settle below already bracket it exactly as they bracket the knob
///    write. `None` = assert nothing (today's behaviour) — and a `PresetLevel` CALLER must
///    pass `None`, since `value` is already the level under measurement.
fn arm_measurement(
    s: &mut Session,
    knob: &LevelKnob,
    value: f32,
    force_bypass: &[(String, String, bool)],
    saved: Option<&serde_json::Value>,
    intended_preset_level: Option<f32>,
) -> Result<(), String> {
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

/// One fresh-connection measurement at an explicit (`presetLevel` × block-param) POINT:
/// base recall + presetLevel via `arm_measurement`, then each block write live on the same
/// armed connection, one engage, measure. P0 instrumentation seam for the headroom-trade
/// physics (presetLevel↑ / outputLevel↓ product invariance) — writes stay on the throwaway
/// working copy; the caller reloads to discard.
pub(crate) fn measure_pair_at(
    scene: Option<u32>,
    preset_level: f32,
    writes: &[(String, String, String, f32)],
    stimulus: &[f32],
) -> Result<lufs::Loudness, String> {
    let mut s = Session::connect_lean()?;
    match scene {
        // Base case: the shared arming seam verbatim (base recall → presetLevel →
        // settle, the ONE tested write order — see `arm_measurement`'s doc).
        None => arm_measurement(
            &mut s,
            &LevelKnob::PresetLevel,
            preset_level,
            &[],
            None,
            None,
        )?,
        // Scene case: the recall targets the scene instead of base; everything after
        // mirrors the seam (recall FIRST — it reverts earlier unsaved writes).
        Some(sc) => {
            s.load_scene(sc)?;
            settle_or_cancel(SETTLE_AFTER_SET_MS)?;
            set_knob(&mut s, &LevelKnob::PresetLevel, preset_level, None)?;
        }
    }
    for (g, n, p, v) in writes {
        s.change_parameter(g, n, p, *v)?;
    }
    settle_or_cancel(SETTLE_AFTER_SET_MS)?;
    engage_measure_disengage(&mut s, stimulus)
}

/// Fresh-connect, arm the measurement (`arm_measurement`: scene context → knob → intended
/// `presetLevel` → isolation), engage re-amp once, measure the processed pair on the full
/// capture. Restores re-amp OFF. `intended_preset_level` is `arm_measurement`'s — the run's
/// own solved/held level, `None` to assert nothing (and always `None` when `knob` is
/// `PresetLevel`, which IS the level under measurement).
fn measure_knob_at(
    stimulus: &[f32],
    knob: &LevelKnob,
    value: f32,
    force_bypass: &[(String, String, bool)],
    saved: Option<&serde_json::Value>,
    intended_preset_level: Option<f32>,
) -> Result<lufs::Loudness, String> {
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
    /// The knob RAN OUT: the solved value sits at a `[0,1]` bound and target lies beyond it,
    /// so this sound cannot reach target at all. Distinct from `unconverged` below — the UI
    /// must not tell a user to retry something physically unreachable.
    pub clamped: bool,
    /// Off target but the knob still had room: the bounded secant spent its
    /// `MEASURE_CORRECT_MAX` captures mid-range (a compressed / noisy response), so the best
    /// point found is reported and a RE-RUN can improve it. Previously collapsed into
    /// `clamped` with no reason — three states behind one flag.
    pub unconverged: bool,
    pub clamp_reason: Option<String>,
    /// The clamp's pinned bound IS the wet/mix floor ([`WET_FLOOR_FRACTION`] × the anchor):
    /// target lies below what the effect can give without being gutted, so the write stops
    /// at the floor and the row wants a "verify by ear" advisory — the UI owns that prose.
    /// Deliberately NOT a `clamp_reason` string: that field's contract is "the leveled
    /// signal isn't reaching USB 1/2" (`.claude/rules/leveling-dsp.md`), and the UI maps ANY
    /// non-null reason to the `offbranch` outcome — a wet-floored row would be labelled a
    /// routing failure. Rides only with `clamped: true`.
    pub wet_floor: bool,
    /// The clamp's CAUSE from the shared taxonomy ([`crate::headroom_trade::ClampKind`]) —
    /// `None` when the row is not clamped. ADDITIVE alongside `clamp_reason`/`wet_floor`,
    /// which keep their documented contracts verbatim: this field exists so a consumer can
    /// TELL THE CAUSES APART without pattern-matching free text, which is exactly what the
    /// `clamp_reason`-means-off-branch rule forbids. Derived in one place
    /// ([`crate::headroom_trade::ClampKind::from_flags`]) so the footswitch, scene and
    /// handle lanes can never disagree.
    pub clamp_kind: Option<crate::headroom_trade::ClampKind>,
    pub saved: bool,
    pub verify_lufs: Option<f64>,
    pub iterations: u32,
    pub dynamic_spread_lu: Option<f64>,
    /// `"baked"` (value written straight onto the block) or `"assigned"` (param-change
    /// footswitch function written) — which simplification the leveler chose.
    pub method: String,
    /// Post-save param-level verify (see `verify_fs_persisted_writes`): `Some(true)` = the
    /// saved preset does NOT hold the value this result reports (do not trust the number);
    /// `Some(false)` = re-read and confirmed; `None` = not checked (no save, nothing written,
    /// the re-read failed, or a path without the verify). HONEST CONTRACT: detects ONLY
    /// "this run's own write didn't persist" — it does NOT detect staleness or the
    /// pre-save-revert shape (field-8 is read-your-writes and would echo stale-saved bytes
    /// right back); that coverage is the registry barrier (`ensure_fresh_load`) alone.
    pub persist_mismatch: Option<bool>,
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
    pub switch_type: u32,
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
    Bake {
        clear_stale: Option<u32>,
        /// Scenes whose overlay restates the base value of the leveled param — the solved
        /// value is also written into each (scene recall + bare write; the overlay exists,
        /// so no Scene Edit enable), else the full-param overlay masks the bake whenever
        /// a scene is active. See `footswitch::FsLevelPlan::Bake::mirror_scenes`.
        mirror_scenes: Vec<u32>,
    },
}

/// The fraction of a WET/MIX parameter's AUTHORED value the footswitch solve may never go
/// below. A wet/mix control is not a volume control: driving it toward 0 to hit a loudness
/// target does not make the effect quieter, it makes the effect DISAPPEAR — the player loses
/// the sound they wrote. So the solve is floored at a quarter of the authored mix and the row
/// is reported clamped, which is an honest "this can't reach target without gutting the
/// effect" rather than a silent tone change.
pub(crate) const WET_FLOOR_FRACTION: f32 = 0.25;

/// The CLASSIFIED footswitch-solve target: which block parameter is being swept, what
/// [`crate::param_class`] says it is, and the authored (pre-solve) value the wet floor is
/// measured against. Three things ride on the classification, all decided before any device
/// work:
///
/// * [`crate::param_class::ParamClass::Other`] ⇒ `solve_footswitch` REFUSES — sweeping a
///   non-level control changes the effect, not the volume.
/// * `range` replaces the old hard-coded `[0, 1]` everywhere the FS solve reasons about
///   knob bounds (seeds, secant clamp, bracket extremes, the pinned-at-a-bound verdict).
///   Params are not all `[0,1]`: `ACD_Boost.gain` is raw dB over `[0, 12]` (HW-verified fw
///   1.8.45). For a `[0,1]` param every derived number is byte-identical to before.
/// * [`crate::param_class::ParamClass::WetMix`] ⇒ the solved value is floored at
///   [`WET_FLOOR_FRACTION`] × `authored`.
///
/// Shared by every USER-CHOSEN-PARAM lane, not just footswitches (the name is historical):
/// the preset block-knob lane (`commands/level_preset.rs`) and the scene HANDLE lane
/// ([`SceneJob::handle`], `commands/level_scenes.rs`) build one too, so the refusal wording,
/// the bounds and the wet floor are identical wherever a user names a control.
///
/// The amp `outputLevel` JOINT-K path is untouched by any of this — it is the closed-form
/// amplitude solve (`solve_joint_k_at`), not a search, and keeps `scene_bench::knob_bounds`
/// and `LEVEL_MIN`/`LEVEL_MAX`.
#[derive(Debug, Clone)]
pub struct FsParamTarget {
    /// The block's FenderId — the classifier's override key, and named in the refusal.
    pub block: String,
    pub param: String,
    pub info: crate::param_class::ParamInfo,
    /// The param's authored value before this run — the block's base value / the assign's
    /// `valueB`. Anchors the wet floor; ignored for every other class. `solve_footswitch`
    /// raises it to an existing assign's stored `valueA` itself ([`Self::anchored`]).
    pub authored: f32,
}

/// The knob value [`FsParamTarget::to_coord`]/[`FsParamTarget::coord_to_value`] floor a
/// `LevelLinear` param's log-knob coordinate at, so `coord = 20*log10(v)` stays finite at
/// `v = 0` and the inverse map never produces a real, paid capture at a knob value that's
/// audibly indistinguishable from the quiet extreme. Named once so the several doc sites
/// that used to re-explain the bare literal `1e-3` now point here instead. The legacy
/// bounds-shape-discriminated coordinate maps (`knob_to_coord`/`coord_to_knob`,
/// `level_preset_block`'s local `to_c`/`from_c`) keep their own `f32` `eps` literals —
/// swapping this `f64` const in there is not a pure no-op, so they're left alone.
const KNOB_LOG_FLOOR: f64 = 1e-3;

impl FsParamTarget {
    /// Classify `param` on the block `fender_id`, anchoring the wet floor at `authored`.
    pub fn new(fender_id: &str, param: &str, authored: f32) -> Self {
        Self {
            block: fender_id.to_string(),
            param: param.to_string(),
            info: crate::param_class::classify(fender_id, param),
            authored,
        }
    }

    /// This target with the wet-floor anchor raised to `max(authored, engaged)` when
    /// `engaged` (the switch's currently-configured engaged value — an existing assign's
    /// stored `valueA`) is known and finite. The solve targets the ENGAGED state, but
    /// `authored` starts from the BASE graph value — for an existing param assign that's the
    /// switch-OFF `valueB`, not what the player actually dialed in while engaged. A
    /// hand-authored assign engaging mix 0.9 over a near-dry base 0.05 would otherwise floor
    /// at 0.0125 and could be gutted to near-silence — the exact incident (chorus mix→0) the
    /// floor exists to prevent. Base-anchoring stays as the FLOOR of the max: it's what
    /// keeps a re-run from ratcheting (max(base, previous solve), base intact) stable across
    /// runs. Applied by `solve_footswitch` itself from the `current_value` it already
    /// receives, so no call site can forget it.
    fn anchored(&self, engaged: Option<f32>) -> Self {
        let mut t = self.clone();
        if let Some(v) = engaged {
            if v.is_finite() && v > t.authored {
                t.authored = v;
            }
        }
        t
    }

    /// The classifier's override key for `node_id`: the base graph's FenderId, falling back
    /// to the node id when the graph carries none. One resolution rule for every constructor
    /// below — `ACD_Boost.gain` (raw dB) and `ACD_TMRumbleV3.level` (barred) are keyed on it,
    /// so a lane that resolved it differently would classify the same control two ways.
    fn fender_id_of(preset: &serde_json::Value, node_id: &str) -> String {
        crate::audiograph::roster_entry(preset, node_id)
            .map(|(_, _, fid)| fid)
            .unwrap_or_else(|| node_id.to_string())
    }

    /// Resolve straight off the SAVED (field-8) preset: the node's FenderId (falling back to
    /// the node id when the graph carries none) and its authored value for `param`. The one
    /// constructor every production call site uses — each already holds the preset that the
    /// batch's single field-8 read produced.
    pub fn from_preset(preset: &serde_json::Value, node_id: &str, param: &str) -> Self {
        let authored = crate::commands::level_footswitch::node_param_f64(preset, node_id, param)
            .unwrap_or(0.0) as f32;
        Self::new(&Self::fender_id_of(preset, node_id), param, authored)
    }

    /// [`Self::from_preset`] with an explicit wet-floor anchor and the CLASS GATE folded in:
    /// resolve the FenderId off `preset`, classify, and return the shared refusal as an `Err`
    /// instead of an admissible target. The one entry point for a lane that takes a
    /// USER-NAMED control — the preset block-knob lane and the scene HANDLE lane both hold
    /// their own `authored` value (the picker's displayed value / the scene's own overlaid
    /// one, never base's), and both must refuse BEFORE any device work, so the
    /// resolve→classify→refuse sequence lives here once rather than at each call site.
    pub fn classified(
        preset: &serde_json::Value,
        node_id: &str,
        param: &str,
        authored: f32,
    ) -> Result<Self, String> {
        let target = Self::new(&Self::fender_id_of(preset, node_id), param, authored);
        match target.refuse_if_not_a_level_control() {
            Some(refusal) => Err(refusal),
            None => Ok(target),
        }
    }

    /// The refusal EVERY lane shares when the classifier doesn't recognise this target as a
    /// level or wet/mix control — `None` when it is admissible. One wording so the
    /// footswitch solve, the preset block-knob lane and the scene handle lane can't drift
    /// (and so a user who hits it in two places reads the same sentence). Always checked
    /// BEFORE any device work: sweeping a non-level control changes the sound the player
    /// wrote, not its loudness.
    pub fn refuse_if_not_a_level_control(&self) -> Option<String> {
        (self.info.class == crate::param_class::ParamClass::Other).then(|| {
            format!(
                "{} on {} is not a level control — leveling it would change the effect, not \
                 the volume",
                self.param, self.block
            )
        })
    }

    /// The param's usable `(lo, hi)`. For a wet/mix param the LOW bound IS the wet floor
    /// (`max(range lo, `[`WET_FLOOR_FRACTION`]` × authored)`), so the whole solve — seeds,
    /// bracket extremes, secant clamp, the pinned-at-a-bound verdict — can never even PROBE
    /// a value that would gut the effect, and every reported loudness is a real reading of a
    /// writable value.
    pub fn bounds(&self) -> (f32, f32) {
        let (lo, hi) = self.info.range;
        (self.wet_floor().map_or(lo, |f| lo.max(f)), hi)
    }

    /// `frac` of the way across the param's range — how the two secant seeds and the
    /// bracket extremes are placed. `frac` 0.25/0.75 on a `[0,1]` param reproduces the
    /// validated 0.25/0.75 seeds exactly.
    fn at_fraction(&self, frac: f32) -> f32 {
        let (lo, hi) = self.bounds();
        lo + frac * (hi - lo)
    }

    /// Seed-2 prediction from the linear law `L = coord + C` (`coord = Self::to_coord(v)`)
    /// fixed by seed 1's own `(v_a, l_a_lufs)` — exact in solve-coord space for
    /// `LevelLinear` (log-knob coord) and `LevelDb` (already ~1:1 dB→LU, identity coord);
    /// `None` for `WetMix`/`Other`, which have no known closed-form law and always take
    /// the fixed-fraction fallback. Returns the predicted raw VALUE (already mapped back
    /// via [`Self::coord_to_value`]), unclamped and un-gated for plausibility — see
    /// [`Self::seed2_plausible`] for whether it's worth spending a real capture on.
    fn law_predicted(&self, v_a: f32, l_a_lufs: f64, target_lufs: f64) -> Option<f64> {
        matches!(
            self.info.class,
            crate::param_class::ParamClass::LevelLinear | crate::param_class::ParamClass::LevelDb
        )
        .then(|| self.coord_to_value(self.to_coord(f64::from(v_a)) + (target_lufs - l_a_lufs)))
    }

    /// Is a [`Self::law_predicted`] candidate `p` worth spending a real capture on, or
    /// should the solver fall back to the fixed complement fraction? Two independent
    /// gates: `p` must land in the central 5%–95% of the param's own range (a prediction
    /// at the very edge usually means a wrong/nonlinear law, not a genuine target), AND
    /// the expected LUFS separation `target_lufs − l_a_lufs` must clear
    /// [`FS_MIN_SEED_GAP_LU`] (see its doc for why gating in loudness space rather than
    /// knob-value space is both the correct check and still safe against a false
    /// no-authority verdict). No separate finiteness check on `p`: a non-finite prediction
    /// (NaN or ±inf from a degenerate law) fails the frac-range comparison outright —
    /// `Range::contains` is `false` for NaN against either bound — so the frac gate alone
    /// already rejects it.
    fn seed2_plausible(&self, p: f64, l_a_lufs: f64, target_lufs: f64) -> bool {
        let (bound_lo, bound_hi) = self.bounds();
        let frac = (p - f64::from(bound_lo)) / f64::from(bound_hi - bound_lo);
        (0.05..=0.95).contains(&frac) && (target_lufs - l_a_lufs).abs() >= FS_MIN_SEED_GAP_LU
    }

    /// Solve-COORD-space value for `v`. [`ParamClass::LevelLinear`] interpolates in
    /// LOG-KNOB space (`coord = 20*log10(v)`, floored at `v = `[`KNOB_LOG_FLOOR`]` so the 0
    /// bound stays finite) where the documented law `L = 20*log10(v) + C` is a straight
    /// line, making the secant EXACT and ending the ~1 LU/capture knob-space creep (HW,
    /// MythicDrive FS: −30 → −22 → −24 → −25 → −26 over 5 captures). `LevelDb` is already
    /// ~1:1 dB→LU (raw space IS log space) and `WetMix` has no known law — both keep the
    /// identity map, so their solve behavior is byte-identical to the validated one.
    /// (`Other` never reaches the solve.)
    ///
    /// Two other sites in this file build a coordinate map with the SAME log-vs-identity
    /// shape but a DIFFERENT discriminator, deliberately not merged with this one: the
    /// free functions `knob_to_coord`/`coord_to_knob` (paired with `knob_search_space`)
    /// pick log-space by BOUNDS SHAPE (`lo >= 0.0 && hi <= 1.0`) because their callers have
    /// no `ParamClass` to consult; `level_preset_block`'s local `to_c`/`from_c` closures do
    /// the same bounds-shape inference inline. Here the discriminator is the param's own
    /// classification — semantically correct where it's available — so a `LevelDb` param
    /// with a `[0, 1]`-shaped range (identity map) and a `LevelLinear` param with a wider
    /// range (log map) both solve correctly, which bounds-shape inference alone cannot
    /// distinguish.
    fn to_coord(&self, v: f64) -> f64 {
        match self.info.class {
            crate::param_class::ParamClass::LevelLinear => 20.0 * v.max(KNOB_LOG_FLOOR).log10(),
            _ => v,
        }
    }

    /// Inverse of [`Self::to_coord`] — caller clamps to [`Self::bounds`]. Floors its
    /// `LevelLinear` output at [`KNOB_LOG_FLOOR`] (not just `to_coord`'s input): without
    /// this floor, a correction-loop coordinate below `to_coord`'s own `-60` floor
    /// (`20*log10(KNOB_LOG_FLOOR)`) would still invert to a REAL, DISTINCT-per-coordinate
    /// value below `KNOB_LOG_FLOOR` (e.g. coord `-70` → `3.16e-4`, coord `-80` → `1e-4`) —
    /// paid captures at knob values that are audibly indistinguishable from the quiet
    /// extreme, silently degenerating the secant pair instead of collapsing cleanly onto
    /// the floor `to_coord` already treats as the bottom. With the floor here, round-trip
    /// `to_coord(coord_to_value(u)) == u` holds exactly for every `u >= -60`. Never emits
    /// exactly `0.0` for `LevelLinear`; the exact-0 bound stays reachable only through
    /// [`fs_bracket_expansion`]'s v-space extreme (see its doc — one-line pointer back
    /// here for the full explanation).
    fn coord_to_value(&self, coord: f64) -> f64 {
        match self.info.class {
            crate::param_class::ParamClass::LevelLinear => {
                10f64.powf(coord / 20.0).max(KNOB_LOG_FLOOR)
            }
            _ => coord,
        }
    }

    /// The lowest value a wet/mix solve may write, or `None` for every other class. An
    /// authored value of `0.0` (the effect is already fully dry) floors at `0.0`, i.e. no
    /// constraint — the floor is RELATIVE to what the player wrote, never an absolute 0.25.
    /// Enforced structurally: [`Self::bounds`] folds it into the low bound.
    fn wet_floor(&self) -> Option<f32> {
        (self.info.class == crate::param_class::ParamClass::WetMix)
            .then_some(self.authored * WET_FLOOR_FRACTION)
    }

    /// Did this (clamped) solve pin at the wet floor specifically — i.e. the low bound was
    /// RAISED by the floor and `v` sits on it? Distinguishes the "verify by ear" advisory
    /// from an ordinary range-edge clamp (e.g. a dB ceiling), which needs none.
    fn pinned_at_wet_floor(&self, v: f32) -> bool {
        self.wet_floor()
            .is_some_and(|f| f > self.info.range.0 && v <= f + 1e-6)
    }
}

/// Adopt `(v, l)` as the new best-so-far when it beats `*best_lufs`'s distance to
/// `target_lufs` — shared by `measure_footswitch`'s bracket-expansion probe and its secant
/// loop, both of which do exactly this after every extra capture.
fn improve_best(
    target_lufs: f64,
    v: f32,
    l: &lufs::Loudness,
    best_v: &mut f32,
    best_lufs: &mut f64,
    best_spread: &mut f64,
) {
    if (l.integrated_lufs - target_lufs).abs() < (*best_lufs - target_lufs).abs() {
        *best_v = v;
        *best_lufs = l.integrated_lufs;
        *best_spread = l.spread_lu();
    }
}

/// Pure secant step in solve-COORD-space coordinates: two `(coordinate, loudness)`
/// points → the next coordinate that should hit `target`. `None` when the local slope is
/// ~flat (the param doesn't move loudness). UNCLAMPED — caller maps back via
/// [`FsParamTarget::coord_to_value`] and clamps to [`FsParamTarget::bounds`].
///
/// For a `LevelLinear` param the coordinate is log-knob (`|Δcoord| ≤ ~60 dB` on
/// `[`[`KNOB_LOG_FLOOR`]`, 1]`, so any pair with ≥ 0.3 LU separation has
/// `|slope| ≥ 0.005 > 1e-3`, the guard below); for `LevelDb`/`WetMix` (identity map)
/// nothing changes from before. Two probed values that both fall below
/// [`KNOB_LOG_FLOOR`] collapse to the SAME coordinate `-60` — a degenerate pair with a
/// non-finite slope, which this guard turns into a graceful `unconverged` rather than a
/// divide-by-zero.
fn fs_secant_next(p0: (f64, f64), p1: (f64, f64), target: f64) -> Option<f64> {
    let slope = (p1.1 - p0.1) / (p1.0 - p0.0);
    if !slope.is_finite() || slope.abs() < 1e-3 {
        return None;
    }
    Some(p1.0 + (target - p1.1) / slope)
}

/// The extreme knob value worth ONE extra probe before giving up on a FLAT `(v_a, l_a)`/
/// `(v_b, l_b)` secant seed pair — `None` when the pair already has slope (the plain
/// secant can extrapolate from it as-is, bracketed or not — unchanged from before this
/// fix) or already includes the relevant extreme (nothing left to try). The pair may
/// arrive in EITHER order (seed 1 need not be the smaller value, and a law-predicted
/// seed 2 can land on either side of it) — every check below is order-agnostic: the
/// flatness test is a symmetric `abs()` difference, `(lo_l, hi_l)` re-sorts via
/// `min`/`max` rather than trusting the argument order, and both bound checks require
/// BOTH points to clear the same side, so swapping which argument is `_a` vs `_b`
/// changes nothing about the result. A knob whose useful range is a small slice of
/// `[0, 1]` (e.g. a compressor already saturated by 0.75) can seed a pair that reads flat
/// even though a reachable, non-flat point exists further out — the minimum-viable fix
/// for THAT specific pathology (full false-position/Illinois-damping bracketing
/// deferred) is one more sample at 1.0 (target needs MORE loudness than either seed) or
/// 0.0 (target needs LESS), so the existing plain secant gets a genuine slope instead of
/// an honest-but-avoidable "no authority" clamp. Gated on flatness specifically (not
/// merely "unbracketed") so an ordinary out-of-bracket-but-sloped pair — which the plain
/// secant already extrapolates from correctly — doesn't pay for an extra real device
/// capture it doesn't need.
///
/// `(lo, hi)` are the PARAM's own bounds (`FsParamTarget::bounds`), not a hard-coded
/// `[0, 1]` — the extremes worth probing are the ends of the range the param actually has.
///
/// Deliberately stays in REAL v-space rather than the solve's coord space: it is
/// choosing between the actual bounds, and `FsParamTarget::coord_to_value` never emits
/// exactly `0.0` for `LevelLinear` (see its doc for why), so the exact-0 bound is
/// reachable ONLY through this v-space extreme.
fn fs_bracket_expansion(
    v_a: f32,
    l_a: f64,
    v_b: f32,
    l_b: f64,
    target: f64,
    (lo, hi): (f32, f32),
) -> Option<f32> {
    if (l_b - l_a).abs() >= KNOB_TOL_LU {
        return None;
    }
    let (lo_l, hi_l) = (l_a.min(l_b), l_a.max(l_b));
    if target > hi_l {
        (v_a < hi && v_b < hi).then_some(hi)
    } else if target < lo_l {
        (v_a > lo && v_b > lo).then_some(lo)
    } else {
        None
    }
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

/// One fresh-connection engaged-state measurement point for a footswitch job: recall BASE,
/// set the swept param there, force the engaged bypass list, engage re-amp once, measure —
/// the `arm_measurement` order. The recall is mandatory: a preset loads into its saved
/// `lastLoadedScene` (HW), so without it the sweep measured (and the solved value described)
/// whatever scene the preset happened to save, not the footswitch's base sound. Because the
/// recall re-asserts that scene's bypass state, the FULL isolation list is re-sent here on
/// EVERY capture — never once per batch. The forced state lives only on this throwaway
/// connection's working-copy edits; the batch write session's reload discards ALL accumulated
/// sweep pollution at once.
///
/// `scene` is the SCENE CONTEXT the switch's sound is measured in (D3): `None` = base — the
/// historical path, byte-identical. `Some(i)` routes through [`measure_fs_state`]'s
/// HW-validated `loadScene → write → engage` composition instead, because a scene-context
/// measurement must write on the LIVE RENDERED LAYER: the persistent, overlay-aware `set_knobs`
/// path `arm_measurement` uses would REFUSE outright on a `BypassOnly` scene, and this is a
/// throwaway measurement, not a write the preset should keep.
///
/// `intended_preset_level` is the run's OWN `presetLevel`, asserted on every capture (see
/// [`arm_measurement`] step 1b and [`measure_fs_state`]): without it the capture renders at
/// the level the DEVICE HAS SAVED, because this path recalls a scene (base included) and
/// then writes a BLOCK param — it never writes `presetLevel` the way the base lane
/// incidentally does. `None` = assert nothing.
pub(crate) fn measure_fs_at(
    scene: Option<u32>,
    lev: (&str, &str, &str),
    engaged_bypass: &[(String, String, bool)],
    stimulus: &[f32],
    v: f32,
    intended_preset_level: Option<f32>,
) -> Result<lufs::Loudness, String> {
    match scene {
        None => processed_loudness(capture_fs_at(
            lev,
            engaged_bypass,
            stimulus,
            v,
            intended_preset_level,
        )),
        Some(_) => measure_fs_state(
            scene,
            engaged_bypass,
            &[(lev.0.to_string(), lev.1.to_string(), lev.2.to_string(), v)],
            stimulus,
            intended_preset_level,
        ),
    }
}

/// [`measure_fs_at`] stopping at the raw capture — the PCM-keeping twin, same
/// connect/arm/engage sequence, for `measure_sound_asis_strict`'s validation dump.
pub(crate) fn capture_fs_at(
    lev: (&str, &str, &str),
    engaged_bypass: &[(String, String, bool)],
    stimulus: &[f32],
    v: f32,
    intended_preset_level: Option<f32>,
) -> Result<audio::Capture, String> {
    let mut s = Session::connect_lean()?;
    let knob = LevelKnob::Block {
        group_id: lev.0.to_string(),
        node_id: lev.1.to_string(),
        parameter_id: lev.2.to_string(),
        scene_slot: None,
    };
    arm_measurement(
        &mut s,
        &knob,
        v,
        engaged_bypass,
        None,
        intended_preset_level,
    )?;
    engage_capture_disengage(&mut s, stimulus)
}

/// ONE fresh-connection, WRITE-NOTHING-PERMANENT capture of ONE footswitch STATE — the
/// measurement shape the reordered run's FS prepass is built on
/// ([`measure_fs_ceiling`]). Same `arm_measurement` order and for the same reasons:
/// scene recall FIRST (a preset loads into its saved `lastLoadedScene`, HW — without the
/// recall the reading would describe whatever scene the preset happens to save), then the
/// state's param writes, then the isolation bypasses LAST (the recall re-asserts that
/// scene's own bypass state, so the full list is re-sent on EVERY capture), then ONE engage.
/// `params` is `(group, node, param, value)` — the switch's `param` functions at this
/// state's `valueA`/`valueB`, plus (for a ceiling read) the leveling handle at its top
/// bound. Every write lands on the throwaway connection's working copy; the command's
/// reload discards them.
///
/// `scene` is the SCENE CONTEXT the sound is measured in: `None` = base (the historical
/// behaviour, `BASE_SCENE_SLOT`), `Some(i)` = that 0-based `scenes[]` wire index. The bare
/// `change_parameter` writes below act on the LIVE RENDERED LAYER under a scene recall
/// (HW, fw 1.8.45: writing a param that the scene carries a Full overlay for measured a
/// −11 LU drop — the overlay does NOT mask the write), which is what makes a per-scene FS
/// ceiling readable at all. These are measurement-only writes on a throwaway connection;
/// PERSISTENT scene-scoped writes still go through the overlay-aware `set_knobs`.
///
/// `intended_preset_level` is the run's OWN `presetLevel`, written straight after the recall
/// — which is what reverts it (the recall runs the device's own level-apply, see
/// `recall_reassert_save`) — and before the param writes. `None` = assert nothing, i.e. the
/// capture renders at the level the device has saved. No settle is added: `setPresetLevel` is
/// one more command in the run of writes the trailing `SETTLE_AFTER_SET_MS` already covers,
/// so no idle gap grows.
fn measure_fs_state(
    scene: Option<u32>,
    bypass: &[(String, String, bool)],
    params: &[(String, String, String, f32)],
    stimulus: &[f32],
    intended_preset_level: Option<f32>,
) -> Result<lufs::Loudness, String> {
    let mut s = Session::connect_lean()?;
    match scene {
        None => recall_base(&mut s)?,
        Some(sc) => {
            s.load_scene(sc)?;
            crate::settle(Duration::from_millis(SETTLE_AFTER_SCENE_RECALL_MS));
        }
    }
    if let Some(pl) = intended_preset_level {
        set_knob(&mut s, &LevelKnob::PresetLevel, pl, None)?;
    }
    for (g, n, p, v) in params {
        s.change_parameter(g, n, p, *v)?;
    }
    for (g, n, byp) in bypass {
        s.change_parameter_bool(g, n, "bypass", *byp)?;
    }
    settle_or_cancel(SETTLE_AFTER_SET_MS)?;
    engage_measure_disengage(&mut s, stimulus)
}

/// The measurement inputs for ONE footswitch sound's PREPASS CEILING read. Built by the
/// caller that owns the preset document, so this module never re-derives switch state.
pub struct FsCeilingProbe<'a> {
    /// THE SCENE CONTEXT this sound is measured in (D3): a 0-based `scenes[]` wire slot, or
    /// `None` = the preset's BASE sound. The capture recalls it before engaging, so the ceiling
    /// describes the switch AS IT SOUNDS THERE — the same context its solve measures in, or the
    /// two describe different sounds.
    pub scene: Option<u32>,
    /// The switch's engaged/disengaged isolation + `param`-function state, straight from
    /// [`crate::footswitch::switch_states`] — the same derivation the solve uses, so a
    /// ceiling can never describe a different sound than the row it belongs to.
    pub states: &'a crate::footswitch::SwitchStates,
    /// The row's leveling handle — `(group, node)` plus its classified target. The ceiling is
    /// read with this param PINNED AT ITS TOP BOUND.
    pub handle: (String, String, FsParamTarget),
}

impl FsCeilingProbe<'_> {
    /// The `(group, node, param, value)` writes the ceiling capture sends: the switch's own
    /// `param` functions at their ENGAGED (`valueA`) values, with the LEVELING HANDLE pinned
    /// at the top of its own (classified, wet-floor-aware) range appended LAST so it wins
    /// over any function addressing the same control — the ceiling is the handle at its top,
    /// by definition. Pure, so the composition is unit-testable without a device.
    pub fn ceiling_params(&self) -> Vec<(String, String, String, f32)> {
        let (group, node, target) = &self.handle;
        let (_, hi) = target.bounds();
        let mut params: Vec<(String, String, String, f32)> = self
            .states
            .params
            .iter()
            .map(|(g, n, p, a, _b)| (g.clone(), n.clone(), p.clone(), *a))
            .collect();
        params.retain(|(_, n, p, _)| !(n == node && p == &target.param));
        params.push((group.clone(), node.clone(), target.param.clone(), hi));
        params
    }
}

/// THE FS HALF OF THE REORDERED RUN'S PREPASS: read one footswitch sound's CEILING with ONE
/// engage, by measuring its engaged state with the leveling handle pinned at the top of its
/// own (classified, wet-floor-aware) range.
///
/// A MEASUREMENT, NEVER AN EXTRAPOLATION. An arbitrary block param has no algebraically
/// predictable response (`headroom_trade` module header), so the only honest way to know what
/// a footswitch sound can reach is to put its handle at the top and listen. One engage yields
/// the ceiling — never the solve, which still budgets its own captures after the trade.
///
/// TRUE-PEAK CAVEAT: a ceiling read at handle-max can CLIP on a hot chain, and the
/// true-peak caveat machinery is base-only. The returned loudness is the reading as taken;
/// the caller decides how much to trust a hot one.
///
/// `intended_preset_level` is the run's OWN `presetLevel` (see [`measure_fs_state`]). A
/// ceiling read at the device's STALE saved level is the whole reading off by the difference
/// — HW: 10.2 dB, which made `fs_target_beyond_ceiling` fire on every row of the batch and
/// clamp sounds that were comfortably in reach. `None` = assert nothing.
pub fn measure_fs_ceiling(
    probe: &FsCeilingProbe<'_>,
    stimulus: &[f32],
    intended_preset_level: Option<f32>,
) -> Result<lufs::Loudness, String> {
    let params = probe.ceiling_params();
    require_live(
        || {
            measure_fs_state(
                probe.scene,
                &probe.states.engaged_bypass,
                &params,
                stimulus,
                intended_preset_level,
            )
        },
        stimulus,
    )
}

/// How far a footswitch row's target must sit ABOVE its measured ceiling before the prepass
/// declares it unreachable and skips the solve.
///
/// Deliberately much wider than the lane's `FS_TOL_LU` acceptance band. The ceiling is read
/// with the handle PINNED AT ITS TOP, which is the hottest this sound ever gets — on a hot
/// chain that capture can CLIP, and the true-peak caveat machinery is base-only, so the
/// reading carries more uncertainty than an ordinary solve point. Skipping a row that could
/// actually have reached target is a silent product bug; paying a full secant budget to
/// rediscover a clamp is only slow. So the margin is sized to make FALSE CLAMPS implausible
/// and lets marginal rows fall through to the solve, which reports honestly either way.
pub(crate) const FS_CEILING_SKIP_MARGIN_LU: f64 = 1.5;

/// Is this footswitch row's target out of reach even with its handle at the top?
/// `ceiling` is [`measure_fs_ceiling`]'s reading. Pure — the decision is unit-testable.
pub(crate) fn fs_target_beyond_ceiling(ceiling: f64, target: f64) -> bool {
    ceiling.is_finite() && target - ceiling > FS_CEILING_SKIP_MARGIN_LU
}

/// The prepass verdict for ONE footswitch row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FsCeiling {
    /// Measured loudness with the handle at the top of its range (LUFS).
    pub ceiling_lufs: f64,
    /// Dynamics spread (LU) of that capture.
    pub spread_lu: f64,
    /// The target is beyond the ceiling by more than [`FS_CEILING_SKIP_MARGIN_LU`] — the row
    /// cannot reach it and the solve is skipped in favour of an honest clamp.
    pub unreachable: bool,
}

/// A CLAMPED footswitch result built from the prepass alone — no solve ran. Used when the
/// ceiling read already proves the target is out of reach: the row reports the loudest it can
/// actually be, at the handle value that produced it, with the shared
/// [`crate::headroom_trade::ClampKind::SceneCeiling`] cause. Writing nothing is correct — the
/// authored value is left exactly as the player wrote it.
pub fn fs_result_from_ceiling(
    switch: u32,
    target_lufs: f64,
    handle: &FsParamTarget,
    ceiling: &FsCeiling,
    method: &str,
) -> FootswitchLevelResult {
    let (_, hi) = handle.bounds();
    FootswitchLevelResult {
        switch,
        measured_lufs: ceiling.ceiling_lufs,
        final_value: hi,
        target_lufs,
        predicted_lufs: ceiling.ceiling_lufs,
        clamped: true,
        unconverged: false,
        clamp_kind: Some(crate::headroom_trade::ClampKind::SceneCeiling),
        clamp_reason: None,
        wet_floor: false,
        saved: false,
        verify_lufs: None,
        // The ONE prepass capture this verdict rests on.
        iterations: 1,
        dynamic_spread_lu: Some(ceiling.spread_lu),
        method: method.to_string(),
        persist_mismatch: None,
    }
}

/// Verdict on a solved footswitch value: `(clamped, unconverged)` — the footswitch mirror of
/// the scene path's "report from the FINAL point" rule (`jointk_one_scene`), which likewise
/// drops the open-loop's initial want. Three outcomes, not one flag: at target → neither;
/// knob pinned at the bound that blocks the needed direction → CLAMPED (unreachable, a
/// re-run cannot help); off target with knob room left → UNCONVERGED (the secant ran out of
/// captures on a compressed/noisy response; the best point is written and a re-run can
/// improve it). The third state, no-authority (`clamp_reason: Some(..)` from the seed's
/// routing probe), is decided before this and rides `clamped` as it always did.
///
/// `(lo, hi)` are the PARAM's own bounds (`FsParamTarget::bounds`) — "pinned at a bound"
/// means pinned at the end of THAT param's range, which is `[0, 1]` for most controls but
/// `[0, 12]` for a raw-dB one. `LEVEL_MIN`/`LEVEL_MAX` stay reserved for the preset/scene
/// lanes' amplitude knobs.
fn classify_fs_outcome(
    best_v: f32,
    best_lufs: f64,
    target_lufs: f64,
    (lo, hi): (f32, f32),
) -> (bool, bool) {
    if (best_lufs - target_lufs).abs() <= FS_TOL_LU {
        return (false, false);
    }
    // Unreachable only when the knob is pinned at the bound that blocks the direction target
    // needs: maxed and target still LOUDER, or zeroed and target still QUIETER. A bound hit
    // whose miss points back INTO the knob's range means the search stopped early, not that
    // the sound can't get there.
    let pinned_loud = best_v >= hi - 1e-3 && target_lufs > best_lufs;
    let pinned_quiet = best_v <= lo + 1e-3 && target_lufs < best_lufs;
    let clamped = pinned_loud || pinned_quiet;
    (clamped, !clamped)
}

/// Is the switch's engaged loudness already at target (within `FS_TOL_LU`, not clamped)? The
/// footswitch mirror of `scene_at_target` / `level_unchanged`, but on the FS lane's tighter
/// band (NOT delegated to `scene_at_target`, which is pinned to `KNOB_TOL_LU`) — a re-run
/// leaves an in-tolerance switch untouched instead of re-solving and re-randomizing it (the
/// idempotency gap PR #74 deferred). `clamped` is always `false` here (the probe measures a
/// real value at `cur`), but the param matches `scene_at_target` for parity and testability.
fn switch_at_target(measured: f64, target: f64, clamped: bool) -> bool {
    !clamped && (measured - target).abs() <= FS_TOL_LU
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
/// `level_unchanged` / scene `scene_at_target` skips). A Bake plan passes the block's own
/// stored param value here too (baking writes straight to the block, so that value IS the
/// engaged value) — `None` remains for fresh assigns and probe seams, which have no prior
/// value to anchor on.
///
/// `param` is the CLASSIFIED target ([`FsParamTarget`]) — it supplies the solve's bounds,
/// the not-a-level-control refusal and the wet floor. Build it with
/// [`FsParamTarget::from_preset`] off the batch's single field-8 read.
#[allow(clippy::too_many_arguments)]
///
/// `intended_preset_level` is the run's OWN `presetLevel`, re-asserted on EVERY capture of
/// the solve for the same reason the isolation list is re-sent on every capture: each one
/// recalls a scene, and the recall runs the device's own level-apply. Solving against a
/// stale level solves the wrong sound. `None` = assert nothing.
pub fn measure_footswitch(
    switch: u32,
    scene: Option<u32>,
    lev: (&str, &str, &str),
    engaged_bypass: &[(String, String, bool)],
    stimulus: &[f32],
    target_lufs: f64,
    method: &str,
    current_value: Option<f32>,
    param: &FsParamTarget,
    intended_preset_level: Option<f32>,
) -> Result<FootswitchLevelResult, String> {
    solve_footswitch(
        switch,
        engaged_bypass,
        stimulus,
        target_lufs,
        method,
        current_value,
        param,
        // EVERY capture of this row recalls `scene` before engaging — the isolation list and
        // the intended `presetLevel` are both re-sent per capture for the same reason (a
        // recall re-asserts its own bypass state AND runs the device's level-apply).
        |bypass, v| measure_fs_at(scene, lev, bypass, stimulus, v, intended_preset_level),
    )
}

/// The device-independent solve loop behind [`measure_footswitch`]: seed → bracket → bounded
/// secant, with `measure(isolation, v)` doing the actual capture. The isolation list is an
/// ARGUMENT of every call, not state captured once: each capture recalls a scene (base
/// included) and a recall RE-ASSERTS that scene's own bypass state, so a once-only forced
/// write is silently reverted for captures 2..N and every later point measures a
/// NON-isolated sound. (It used to be sent only on the first successful capture, on the
/// since-corrected assumption that working-copy edits alone survive the reconnects — they
/// do, but the recall undoes them.) Injectable so the loop + its outcome verdict are unit
/// tested without a device.
#[allow(clippy::too_many_arguments)]
fn solve_footswitch(
    switch: u32,
    engaged_bypass: &[(String, String, bool)],
    stimulus: &[f32],
    target_lufs: f64,
    method: &str,
    current_value: Option<f32>,
    param: &FsParamTarget,
    mut measure: impl FnMut(&[(String, String, bool)], f32) -> Result<lufs::Loudness, String>,
) -> Result<FootswitchLevelResult, String> {
    let solved = solve_param_secant(
        stimulus,
        target_lufs,
        current_value,
        param,
        FS_CORRECT_MAX,
        |v| measure(engaged_bypass, v),
    )?;
    Ok(solved.into_fs_result(switch, target_lufs, method))
}

/// What the param-space secant ACTUALLY found — the solve's own vocabulary, with no wire
/// shape attached. [`solve_param_secant`] used to return a [`FootswitchLevelResult`], which
/// forced its non-footswitch caller (the scene HANDLE lane) to hand it a fake `switch` and an
/// empty engaged-bypass list and then throw six report-only fields away. Each caller now maps
/// this into its own shape: [`solve_footswitch`] via [`ParamSolve::into_fs_result`], the scene
/// lane into a [`SceneSolve`].
struct ParamSolve {
    /// The value to write — the best point the search reached, always inside the param's
    /// [`FsParamTarget::bounds`] (the wet floor included), so it is always writable. ONE
    /// exception: the idempotency skip's `final_value == current_value` verbatim can sit
    /// BELOW the wet floor (an existing assign whose stored `valueA` predates the floor,
    /// or predates a base value change) — safe only because that equality is the
    /// caller's no-op signal, so this field is never clamped there; clamping would break
    /// the no-op detect and make an unchanged switch look like a solved write.
    final_value: f32,
    /// Loudness at the first REFERENCE point: the low seed's capture, the idempotency
    /// probe's, or the routing-clamp sentinel. Context for the report, never the achieved
    /// value.
    measured_lufs: f64,
    /// Achieved loudness AT `final_value` — a real capture, not a model prediction.
    predicted_lufs: f64,
    /// The knob RAN OUT (pinned at the bound blocking the needed direction) or has no
    /// authority over loudness at all. A re-run cannot help.
    clamped: bool,
    /// Off target with knob room left — the bounded secant spent its budget on a
    /// compressed/noisy response. A re-run CAN improve it.
    unconverged: bool,
    /// `Some` ONLY for the first seed's routing probe ("no signal on USB 1/2"); every later
    /// silence is the knob's own quiet extreme, which is data.
    clamp_reason: Option<String>,
    /// The clamp pinned at the WET FLOOR specifically — the row wants a "verify by ear"
    /// advisory. Rides only with `clamped`.
    wet_floor: bool,
    /// Dynamics spread (LU) of the capture behind `predicted_lufs`; `None` when no real
    /// capture backs it (the routing clamp).
    spread_lu: Option<f64>,
    /// Real device captures this solve paid for.
    iterations: u32,
}

impl ParamSolve {
    /// Map onto the FOOTSWITCH wire shape. `switch` and `method` are report-only tags the
    /// solve has no opinion about, and the persistence fields (`saved`, `verify_lufs`,
    /// `persist_mismatch`) belong to the write path that runs after this.
    fn into_fs_result(self, switch: u32, target_lufs: f64, method: &str) -> FootswitchLevelResult {
        FootswitchLevelResult {
            switch,
            measured_lufs: self.measured_lufs,
            final_value: self.final_value,
            target_lufs,
            predicted_lufs: self.predicted_lufs,
            clamped: self.clamped,
            unconverged: self.unconverged,
            clamp_kind: crate::headroom_trade::ClampKind::from_flags(
                self.clamped,
                self.wet_floor,
                self.clamp_reason.as_deref(),
            ),
            clamp_reason: self.clamp_reason,
            wet_floor: self.wet_floor,
            saved: false,
            verify_lufs: None,
            iterations: self.iterations,
            dynamic_spread_lu: self.spread_lu,
            method: method.into(),
            persist_mismatch: None,
        }
    }
}

/// [`solve_footswitch`]'s loop with the CAPTURE BUDGET as an argument — the seam the scene
/// handle lane reuses. Every correction iterate is a fresh-connect re-amp capture (~10 s),
/// and the two lanes' cost profiles differ: a footswitch batch pays it per switch
/// ([`FS_CORRECT_MAX`]), a scene batch per scene and up to 9 of them
/// ([`SCENE_HANDLE_CORRECT_MAX`]). Nothing else about the solve differs, so the budget is
/// the ONLY thing threaded — inheriting the footswitch cap silently would have re-inflated
/// per-scene cost toward the legacy 80–93 s regime the scene lane's own
/// `MEASURE_CORRECT_MAX` was set to avoid.
///
/// `measure(v)` captures at knob value `v`; whatever isolation that needs (a footswitch's
/// engaged-bypass list, a scene recall) is the CALLER's closure to carry — the solve itself
/// has no footswitch concepts left in it.
fn solve_param_secant(
    stimulus: &[f32],
    target_lufs: f64,
    current_value: Option<f32>,
    param: &FsParamTarget,
    correct_max: u32,
    mut measure_at: impl FnMut(f32) -> Result<lufs::Loudness, String>,
) -> Result<ParamSolve, String> {
    // ENTRY GUARD, before any device work: a param the classifier doesn't recognise as a
    // level or wet/mix control is not a volume control. Sweeping it would change the sound
    // the player wrote, not its loudness — refuse instead of "levelling" it. Surfaces as a
    // clean per-switch `status: "error"` item (the batched command's `Err` arm).
    if let Some(refusal) = param.refuse_if_not_a_level_control() {
        return Err(refusal);
    }
    // The wet floor anchors on the ENGAGED value when the switch already has one (see
    // `anchored`'s doc); applied here, not at call sites, so it cannot be forgotten.
    let param = &param.anchored(current_value);
    let (bound_lo, bound_hi) = param.bounds();

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
    // A successful-but-off-target probe is a PAID capture: it is kept as `probe_seed` and
    // reused as the solve's seed 1 below instead of being discarded and re-bought at a fresh
    // `at_fraction(0.25)`.
    let mut probe_seed: Option<(f32, lufs::Loudness)> = None;
    if let Some(cur) = current_value {
        match require_live(|| measure_at(cur), stimulus) {
            Ok(l) if switch_at_target(l.integrated_lufs, target_lufs, false) => {
                reamp_off();
                // Skip signal: `final_value` == the caller's current value verbatim, so the
                // caller detects the no-op by `final_value == current` and writes nothing
                // (the footswitch mirror of the scene lane's off-wire `writes: 0`). See
                // `ParamSolve::final_value`'s doc: this is the one path that can leave the
                // returned value below the wet floor, safely, because it is never written.
                return Ok(ParamSolve {
                    final_value: cur,
                    measured_lufs: l.integrated_lufs,
                    predicted_lufs: l.integrated_lufs,
                    clamped: false,
                    unconverged: false,
                    clamp_reason: None,
                    wet_floor: false,
                    spread_lu: Some(l.spread_lu()),
                    iterations: 1,
                });
            }
            // In-tolerance-but-not-a-skip carries into the seed pass as seed 1; a probe
            // error must disengage re-amp first (the seed pass re-engages on a fresh
            // connection) and falls through to the fixed-seed path below.
            Ok(l) => probe_seed = Some((cur, l)),
            Err(_) => reamp_off(),
        }
    }

    // Seed 1: reuse a successful idempotency probe when there is one (no new capture — it
    // already proved signal, so the routing-clamp verdict below belongs to the fixed-seed
    // path alone); else the fixed `at_fraction(0.25)` position exactly as before.
    let (v_a, l_a) = if let Some((cur, l)) = probe_seed {
        (cur, l)
    } else {
        let v = param.at_fraction(0.25);
        // This seed doubles as the routing probe: a genuinely silent capture (device output
        // not on USB 1/2) makes `processed_loudness` error "no signal captured" — convert
        // THAT one to the honest "not on USB 1/2" clamp (mirrors the scene mute-floor idiom
        // below). Signal-present but flat/short-of-target is a headroom/authority clamp with
        // NO reason, not a routing error. ONLY this seed can mean broken routing (silent from
        // the very first bought capture); every LATER silence — including a silent PROBE seed
        // taking this seed's place above — is the knob's own quiet extreme (a pedal
        // `level`/`volume` at 0 IS deep digital silence on the real unit — DATA the secant
        // needs, not a fatal abort). See `FS_SILENT_GEOMETRY_LUFS`'s doc: seed 2, the
        // bracket-expansion probe, and the correction loop below each convert a later
        // silence into a pseudo point instead of propagating the error (HW-reproduced, fw
        // 1.8.45, preset "TR+BD2+BMP": Plumes `level` knob, seeds 0.25/0.75 read the
        // compressed plateau, a −26 target extrapolated a negative knob value, clamped to
        // 0.0, and 0.0 is silent).
        // Floor-guarded (the flat-but-finite silent-inject case); the NO_SIGNAL arm below
        // stays separate — genuine silence is the routing clamp, not a floor read.
        let l = match require_live(|| measure_at(v), stimulus) {
            Ok(l) => l,
            Err(e) if e.contains(NO_SIGNAL_CAPTURED) => {
                reamp_off();
                return Ok(ParamSolve {
                    final_value: v,
                    measured_lufs: MUTE_FLOOR_SILENT_LUFS,
                    predicted_lufs: MUTE_FLOOR_SILENT_LUFS,
                    clamped: true,
                    unconverged: false,
                    clamp_reason: Some("no signal on USB 1/2".into()),
                    wet_floor: false,
                    spread_lu: None,
                    iterations: 1,
                });
            }
            Err(e) => return Err(e),
        };
        (v, l)
    };

    // Seed 2: probe → LAW → fixed-fraction ladder. `law_predicted` returns the linear-law
    // candidate (or `None` for a `WetMix`/no-known-law param, which always takes the fixed
    // fallback); `seed2_plausible` is the acceptance gate — see both methods' docs for the
    // law itself, the accuracy band it's validated against (HW, MythicDrive FS: −30 → −22
    // → −24 → −25 → −26 over 5 captures on the OLD knob-space secant this replaces), and
    // why a rejected/wrong prediction degrades to the old fixed complement fraction (0.75
    // if seed 1 sits in the lower half of the range, else 0.25) rather than ever doing
    // worse than the pre-existing behavior.
    let fixed_fallback = || {
        if v_a <= param.at_fraction(0.5) {
            param.at_fraction(0.75)
        } else {
            param.at_fraction(0.25)
        }
    };
    let v_b = param
        .law_predicted(v_a, l_a.integrated_lufs, target_lufs)
        .filter(|&p| param.seed2_plausible(p, l_a.integrated_lufs, target_lufs))
        .map(|p| p as f32)
        .unwrap_or_else(fixed_fallback);
    // Seed 2 can land silent (a knob whose useful range is a narrow slice of its own
    // range) — past seed 1, that's data, never a routing error (see seed 1's doc above).
    // `seed2_silent` feeds the initial best-seed pick below so a synthesized floor point
    // can never win it (mirrors `improve_best`'s exclusion of synthetic points).
    let (l_b_lufs, l_b_spread, seed2_silent) = match require_live(|| measure_at(v_b), stimulus) {
        Ok(l) => (l.integrated_lufs, l.spread_lu(), false),
        Err(e) if e.contains(NO_SIGNAL_CAPTURED) => {
            (fs_silent_geometry(l_a.integrated_lufs), 0.0, true)
        }
        Err(e) => return Err(e),
    };
    // The quietest REAL capture seen so far — the anchor `fs_silent_geometry` floors every
    // silent pseudo point under.
    let mut min_real = if seed2_silent {
        l_a.integrated_lufs
    } else {
        l_a.integrated_lufs.min(l_b_lufs)
    };
    // The probe capture (when present) counts as capture 1, so after seed 2 this is 2 real
    // device round-trips on BOTH seeded paths — probe-as-seed-1+seed2, or fixed-seed1+
    // seed2. It is NOT the count on the probe-ERROR path (probe fails, falls through to a
    // fresh fixed seed 1, then seed 2 — 3 round-trips: the failed probe, seed 1, seed 2);
    // that extra failed round-trip is deliberately left uncounted here, pre-existing
    // accounting this change doesn't touch.
    let mut iterations = 2u32;
    let err = |l: f64| (l - target_lufs).abs();
    let (mut best_v, mut best_lufs, mut best_spread) =
        if seed2_silent || err(l_a.integrated_lufs) <= err(l_b_lufs) {
            (v_a, l_a.integrated_lufs, l_a.spread_lu())
        } else {
            (v_b, l_b_lufs, l_b_spread)
        };
    // A seed pair that cannot move loudness is a physical dead end, not an exhausted
    // search — it must never advertise a re-run. Set ONLY where no-authority is proven
    // (the seed/expansion pair below); a mid-loop stall keeps `unconverged`.
    let mut flat_response = false;
    if err(best_lufs) > FS_TOL_LU {
        // The correction loop below interpolates in solve-COORD space
        // (`param.to_coord`), not raw parameter value — see `to_coord`'s doc for why
        // that makes the secant exact for a `LevelLinear`/`LevelDb` param.
        let mut p0 = (param.to_coord(v_a as f64), l_a.integrated_lufs);
        let mut p1 = (param.to_coord(v_b as f64), l_b_lufs);
        // Bracket before falling to the correction loop — see `fs_bracket_expansion`'s doc.
        if let Some(v_extreme) = fs_bracket_expansion(
            v_a,
            l_a.integrated_lufs,
            v_b,
            l_b_lufs,
            target_lufs,
            (bound_lo, bound_hi),
        ) {
            // Silence at the probe is data (see `FS_SILENT_GEOMETRY_LUFS`); any other
            // error just forfeits the extra probe (the plain secant still runs).
            let extreme_point = match require_live(|| measure_at(v_extreme), stimulus) {
                Ok(l_extreme) => {
                    iterations += 1;
                    min_real = min_real.min(l_extreme.integrated_lufs);
                    improve_best(
                        target_lufs,
                        v_extreme,
                        &l_extreme,
                        &mut best_v,
                        &mut best_lufs,
                        &mut best_spread,
                    );
                    Some((param.to_coord(v_extreme as f64), l_extreme.integrated_lufs))
                }
                Err(e) if e.contains(NO_SIGNAL_CAPTURED) => {
                    iterations += 1;
                    Some((
                        param.to_coord(v_extreme as f64),
                        fs_silent_geometry(min_real),
                    ))
                }
                Err(_) => None,
            };
            if let Some(extreme_point) = extreme_point {
                if err(p0.1) <= err(p1.1) {
                    p1 = extreme_point;
                } else {
                    p0 = extreme_point;
                }
            }
        }
        // Bracket-aware secant, gated on the pair having authority (still flat → the
        // knob truly can't move loudness here — an honest, reason-less clamp). Each
        // iterate interpolates the SAME `fs_secant_next` step; only the endpoint-update
        // rule differs by pair shape:
        //   * pair STRADDLES the target → keep the bracket (replace the same-side
        //     endpoint) with Illinois damping — a repeat on one side halves the retained
        //     endpoint's offset so a stale far end can't pin the interpolation. This is
        //     what converges on a steep-taper cliff (HW, Hiwatt fs12: the UniVibe
        //     `volume` measures −45.6 LUFS at 0.25 but −16.9 at 0.61, a >60 LU/knob-unit
        //     cliff around the −20 target; the plain sliding secant zig-zagged across it
        //     and stopped 3.1 LU hot — caught by the strict e2e re-measure).
        //   * pair does NOT straddle → the plain slide (drop the older point), unchanged
        //     from the validated behavior; the first crossing iterate forms a straddling
        //     consecutive pair, which flips the loop into bracket mode by itself.
        if err(best_lufs) > FS_TOL_LU {
            if (p1.1 - p0.1).abs() < KNOB_TOL_LU {
                // The seed pair itself is flat (and the expansion probe didn't land
                // it): the knob demonstrably can't move loudness, the correction loop
                // has nothing to interpolate on, and best_v sits mid-range. This is
                // the ONE place no-authority is proven — the noise-robust KNOB_TOL_LU
                // band over the widest pair the solve ever holds.
                flat_response = true;
            } else {
                // −1 / +1: which side of the target the previous bracket-mode iterate
                // landed on (0 until the pair straddles).
                let mut last_side = 0i8;
                for _ in 0..correct_max {
                    let straddles = (p0.1 - target_lufs) * (p1.1 - target_lufs) < 0.0;
                    let Some(raw) = fs_secant_next(p0, p1, target_lufs) else {
                        // NOT a no-authority verdict: this loop is only entered after
                        // the seeds proved ≥ KNOB_TOL_LU of authority, so a mid-loop
                        // flat/degenerate pair (e.g. both endpoints clamped onto the
                        // same bound on a non-monotone response) means the SEARCH is
                        // stuck, not that the knob is dead — keep the honest
                        // `unconverged` from classify_fs_outcome below.
                        break;
                    };
                    let v2 = param
                        .coord_to_value(raw)
                        .clamp(bound_lo as f64, bound_hi as f64)
                        as f32;
                    // Silence is data (see `FS_SILENT_GEOMETRY_LUFS`); `l2_real` gates the
                    // at-target break so only a REAL capture may declare victory.
                    let (l2_lufs, l2_real) = match require_live(|| measure_at(v2), stimulus) {
                        Ok(l2) => {
                            iterations += 1;
                            min_real = min_real.min(l2.integrated_lufs);
                            improve_best(
                                target_lufs,
                                v2,
                                &l2,
                                &mut best_v,
                                &mut best_lufs,
                                &mut best_spread,
                            );
                            (l2.integrated_lufs, true)
                        }
                        Err(e) if e.contains(NO_SIGNAL_CAPTURED) => {
                            iterations += 1;
                            (fs_silent_geometry(min_real), false)
                        }
                        Err(e) => return Err(e),
                    };
                    if l2_real && err(l2_lufs) <= FS_TOL_LU {
                        break;
                    }
                    let p2 = (param.to_coord(v2 as f64), l2_lufs);
                    if straddles {
                        if (p2.1 - target_lufs) * (p0.1 - target_lufs) > 0.0 {
                            if last_side == -1 {
                                p1.1 = target_lufs + (p1.1 - target_lufs) / 2.0;
                            }
                            p0 = p2;
                            last_side = -1;
                        } else {
                            if last_side == 1 {
                                p0.1 = target_lufs + (p0.1 - target_lufs) / 2.0;
                            }
                            p1 = p2;
                            last_side = 1;
                        }
                    } else {
                        p0 = p1;
                        p1 = p2;
                        last_side = 0;
                    }
                }
            }
        }
    }
    // Signal is present past the seed probe, so a miss is a headroom or convergence limit,
    // never a routing error → `clamp_reason` stays None (that reason belongs to the seed's
    // routing probe alone). A FLAT response is the exception: the knob has no authority
    // over loudness, so a re-run repeats the same miss — report it as the reason-less
    // clamp (the scene lane's precedent), never as `unconverged`.
    let (clamped, unconverged) = if flat_response {
        (true, false)
    } else {
        classify_fs_outcome(best_v, best_lufs, target_lufs, (bound_lo, bound_hi))
    };
    Ok(ParamSolve {
        // The wet floor needs no epilogue: `bounds()` folds it into `bound_lo`, so the
        // secant never probed below it, `best_v` is always writable, and `predicted_lufs`
        // is a real reading OF the written value. A wet solve pinned at the floor arrives
        // here as an ordinary at-a-bound clamp; only the advisory flag below names it.
        final_value: best_v,
        measured_lufs: l_a.integrated_lufs,
        predicted_lufs: best_lufs,
        clamped,
        unconverged,
        clamp_reason: None,
        wet_floor: clamped && param.pinned_at_wet_floor(best_v),
        spread_lu: Some(best_spread),
        iterations,
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
    restore_scene: Option<u32>,
    param: &FsParamTarget,
) -> Result<FootswitchLevelResult, String> {
    let body = || -> Result<FootswitchLevelResult, String> {
        // Freshness barrier first: this is the single-switch probe seam, called with no
        // caller-supplied witness — registry-driven only (a no-op when the slot has no
        // pending save).
        ensure_fresh_load(slot, &mut || crate::op_aborted())?;
        // Load the preset in its own connection (re-amp latch workaround), then measure on
        // fresh connections (the preset stays current across reconnects).
        {
            let mut s = Session::connect_lean()?;
            s.load_preset(slot)?;
            crate::settle(Duration::from_millis(settle_after_load_ms()));
        }
        crate::settle(Duration::from_millis(RECONNECT_GAP_MS));

        let method = match write {
            FsWrite::Bake { .. } => "baked",
            FsWrite::Assign { .. } => "assigned",
        };
        // Single-switch probe seam: always solve fresh (no idempotency probe) — the batched
        // command owns the re-run skip. No intended `presetLevel` either: this seam levels
        // ONE switch of an already-saved preset and holds no unsaved level of its own, so the
        // preset's own stored level is the right one to render at.
        let result = measure_footswitch(
            switch,
            None,
            lev,
            engaged_bypass,
            stimulus,
            target_lufs,
            method,
            None,
            param,
            None,
        )?;
        if result.clamp_reason.is_some() {
            // No-signal routing clamp: nothing to write — discard the sweep pollution.
            crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
            if let Ok(mut s) = Session::connect_lean() {
                if let Err(e) = s.load_preset(slot) {
                    log::warn!("footswitch no-signal reload failed (slot {slot}): {e}");
                }
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
            write_footswitch_values(slot, &pending, restore_scene)?;
            result.saved = true;
            if verify {
                crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
                // The verify runs AFTER the save, so the preset's own stored level IS the
                // intended one — nothing to re-assert.
                result.verify_lufs = measure_fs_at(
                    None,
                    lev,
                    engaged_bypass,
                    stimulus,
                    result.final_value,
                    None,
                )
                .ok()
                .map(|l| l.integrated_lufs);
                // Cleanup reload only — a failure must not fail the already-saved result.
                match Session::connect_lean() {
                    Ok(mut s) => {
                        if let Err(e) = s.load_preset(slot) {
                            log::warn!("footswitch verify reload failed (slot {slot}): {e}");
                        }
                    }
                    Err(e) => log::warn!("footswitch verify reload skipped (slot {slot}): {e}"),
                }
            }
        } else {
            // Dry: discard the measurement pollution (cleanup only, same rule as above).
            match Session::connect_lean() {
                Ok(mut s) => {
                    if let Err(e) = s.load_preset(slot) {
                        log::warn!("footswitch dry-run reload failed (slot {slot}): {e}");
                    }
                }
                Err(e) => log::warn!("footswitch dry-run reload skipped (slot {slot}): {e}"),
            }
        }
        Ok(result)
    };
    let out = body();
    // Final guarantee on EVERY exit — measure errors and the no-signal clamp return
    // included (verify re-amps; never leave the unit input-muted).
    reamp_off_guaranteed("level_footswitch");
    out
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
///
/// A `FsWrite::Bake` persists the solved value with `changeParameter`, which lands in
/// whatever scene the connection currently holds — and the load below activates the preset's
/// SAVED `lastLoadedScene`, not base (HW). Hence the mandatory base recall: without it a
/// scened preset's bake landed in that scene's overlay while BASE — the state the leveler
/// measured, and the one the switch's off position renders — kept its old value.
pub fn write_footswitch_values(
    slot: u32,
    pending: &[FsPendingWrite],
    restore_scene: Option<u32>,
) -> Result<(), String> {
    if pending.is_empty() {
        return Ok(());
    }
    // Guaranteed re-amp OFF first — the measurement's last disengage can be dropped.
    let _ = Session::connect_lean().map(|mut s| s.set_reamp_mode(false));
    crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
    // Freshness barrier on its OWN separate session, before this function's write session
    // opens — NEVER inside `write_fs_values_on_session`: a live-edit lapse there drops
    // chunked writes. Its own internal `load_preset` (below) stays exactly as-is.
    ensure_fresh_load(slot, &mut || crate::op_aborted())?;
    crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
    let mut s = Session::connect()?;
    write_fs_values_on_session(&mut s, slot, pending, restore_scene)
}

/// [`write_footswitch_values`]' body on an ALREADY-OPEN session — split out so the write
/// order (load → warmup → BASE recall → confirm-gated writes → ONE save) is unit-tested
/// against `SimDevice` with no real HID open.
///
/// The recall is issued ONCE, before the loop, never per item: `load_scene` reverts prior
/// UNSAVED writes to the recalled context, so a per-item recall would undo the earlier
/// bakes. The ~700 ms post-`loadScene` acceptance cliff that once argued against recalling
/// at all here is a SCENE-OVERLAY phenomenon (it gates writes needing the per-node Scene
/// Edit arming); these are base-context writes with no arming. RE-VERIFY ON HW for a
/// multi-switch batch: if a later item's bake stops landing, that cliff applies to base
/// writes too and the fix is to split the batch, not to drop the recall.
fn write_fs_values_on_session(
    s: &mut Session,
    slot: u32,
    pending: &[FsPendingWrite],
    restore_scene: Option<u32>,
) -> Result<(), String> {
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
    // Base is a RECALL (wire slot 8), never an omission — see this function's doc.
    recall_base(s)?;
    for p in pending {
        let (group, node, param) = (&p.lev.0, &p.lev.1, &p.lev.2);
        match &p.write {
            FsWrite::Assign { value_b, spec } => {
                let json = serde_json::json!({
                    "func": "param", "groupId": group, "nodeId": node, "parameterId": param,
                    "valueA": p.value, "valueB": value_b, "valueType": 2,
                    "colorA": spec.color_a, "colorB": spec.color_b,
                    "customLabel": spec.custom_label, "switchType": spec.switch_type,
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
                        crate::settle(Duration::from_millis(200));
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
            FsWrite::Bake { clear_stale, .. } => {
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
                        crate::settle(Duration::from_millis(200));
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
    // Mirror each bake into the scenes whose overlay restated the base value (grouped by
    // scene: one recall, then that scene's writes back-to-back — the write must follow its
    // recall inside the ~400 ms idle-gap acceptance window; the overlay exists by
    // construction, so no Scene Edit enable). Unsaved writes survive the recalls (HW,
    // `probe --defer-scenes`); the single save below persists base + every overlay.
    let mut by_scene: std::collections::BTreeMap<u32, Vec<&FsPendingWrite>> =
        std::collections::BTreeMap::new();
    for p in pending {
        if let FsWrite::Bake { mirror_scenes, .. } = &p.write {
            for &scene in mirror_scenes {
                by_scene.entry(scene).or_default().push(p);
            }
        }
    }
    for (scene, writes) in &by_scene {
        s.load_scene(*scene)?;
        crate::settle(Duration::from_millis(SETTLE_AFTER_SCENE_RECALL_MS));
        for p in writes {
            s.change_parameter(&p.lev.0, &p.lev.1, &p.lev.2, p.value)?;
        }
        let _ = s.heartbeat();
    }
    recall_original_scene(s, restore_scene)?;
    s.save_current_preset(slot)?;
    // Witness: the first Bake's baked param, else the first Assign's valueA — whichever this
    // batch actually wrote first. `write_footswitch_values` (this function's only caller)
    // already ran `ensure_fresh_load` before this session opened, so this registration is
    // exactly what a NEXT same-slot load should wait to see.
    let first_bake = pending
        .iter()
        .find(|p| matches!(p.write, FsWrite::Bake { .. }));
    let witness_write = first_bake.or_else(|| {
        pending
            .iter()
            .find(|p| matches!(p.write, FsWrite::Assign { .. }))
    });
    if let Some(p) = witness_write {
        register_slot_save(
            slot,
            SaveWitness::Param {
                node: p.lev.1.clone(),
                param: p.lev.2.clone(),
                value: p.value,
                scene: None, // footswitch batch — base/ftsw witness, never scene-scoped
            },
        );
    }
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

/// `(log_space, c_lo, c_hi)` for a knob-search coordinate map — the SAME log-vs-identity
/// shape as [`FsParamTarget::to_coord`]/[`FsParamTarget::coord_to_value`], but a DIFFERENT
/// discriminator: this seam has no `ParamClass` to consult, so it infers log-space from the
/// BOUNDS SHAPE (`[0, 1]`-like) instead of the param's own classification. Deliberately not
/// merged with `FsParamTarget`'s class-based map — see that type's doc for why.
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

/// See [`knob_search_space`]'s doc for the discriminator this pairs with.
fn knob_to_coord(value: f32, log_space: bool) -> f32 {
    if log_space {
        20.0 * value.max(1e-3).log10()
    } else {
        value
    }
}

/// See [`knob_search_space`]'s doc for the discriminator this pairs with.
fn coord_to_knob(coord: f32, log_space: bool, lo: f32, hi: f32) -> f32 {
    if log_space {
        10f32.powf(coord / 20.0).clamp(lo, hi)
    } else {
        coord.clamp(lo, hi)
    }
}

fn live_window_lufs(live: &audio::LiveReamp, window_ms: u64) -> Result<f64, String> {
    let cap = live.recent_capture(window_ms)?;
    let lufs = measure_processed(&cap)?.integrated_lufs;
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
    /// The clamp's CAUSE from the shared taxonomy ([`crate::headroom_trade::ClampKind`]);
    /// `None` when the scene is not clamped. Additive next to `clamp_reason` — see
    /// [`LevelResult::clamp_kind`].
    pub clamp_kind: Option<crate::headroom_trade::ClampKind>,
    /// Best-effort "verify by ear" flag from the rebalance flow: the lane-mute floor was
    /// close enough to a solo lane that bleed may have skewed the equal-solo balance (the
    /// overall target is still hit). `false` outside rebalance.
    pub verify_by_ear: bool,
    /// Post-save param-level verify (`verify_persisted_writes`): `Some(true)` = the saved
    /// preset does NOT hold the value this outcome reports, so the number above is
    /// pre-wipe and must not be trusted; `Some(false)` = re-read and confirmed. `None` =
    /// not checked (the scene wrote nothing, the run didn't save, or the re-read failed).
    pub persist_mismatch: Option<bool>,
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
    /// The USER'S OWN leveling handle for this scene, when they chose one: the CLASSIFIED
    /// block param `knobs[0]` addresses. `None` = the amp-`outputLevel` joint-k path (every
    /// existing caller), which keeps joint-k, the amplitude-bounds requirement, and the
    /// rebalance flow. `Some` switches the scene onto the generic param-space secant
    /// ([`solve_footswitch`]'s loop) with THIS param's class, range and wet floor — and
    /// `knobs` then holds exactly one [`KnobTarget`] for it, so the persist-verify and
    /// deferred-save machinery need no special case.
    pub handle: Option<FsParamTarget>,
    /// This scene's PREPASS reading, when the run measured every sound's ceiling BEFORE any
    /// write (the reordered run — see [`prepass_scene_ceilings`]). `Some` makes
    /// [`scene_prologue`] consume the reading instead of taking its own capture, so the
    /// per-scene solve costs exactly what it always did and the batch simply pays its
    /// as-is captures up front. `None` (every legacy caller: the rebalance flow, the
    /// redistribution runner, the probe benches) keeps the measure-inside-the-solve order
    /// byte-for-byte.
    pub prepass: Option<ScenePrepass>,
    /// Isolation bypass writes `(group_id, node_id, forced_bypass)` this job's every capture
    /// and write must re-assert — "base means base": preset 28's TubeScreamer is saved ON, so
    /// an un-isolated base capture would measure it and silently under-isolate. Empty for every
    /// scene job (a scene rides its own overlay). Non-empty only for a BASE job
    /// (`scene_slot == session::BASE_SCENE_SLOT`), derived once by the app command
    /// (`doctor_force_bypass`) and threaded through `measure_scene_asis`/`apply_levels`/
    /// `apply_first_verified`/`correct_iter`'s trailing `force_bypass` parameter at every site
    /// this job reaches one. The device working copy the isolation writes land in must be
    /// clean again before the batch's terminal save — see `danger.md`'s PHASE-1/trade-hold
    /// cleanup choreography; this field only carries WHAT to force, not when to undo it.
    pub force_bypass: Vec<(String, String, bool)>,
}

/// One scene's PREPASS measurement: the reading every solve used to take as its own first
/// step, hoisted OUT of the solve so a batch can know every sound's ceiling before it writes
/// anything.
///
/// WHY THE ORDER MATTERS. The benefit-aware headroom trade
/// ([`crate::headroom_trade::plan_headroom_trade`]) has to compare EVERY sound's ceiling
/// against EVERY sound's target before it decides whether to move the base
/// `presetLevel`/`outputLevel` pair — a decision that changes what every later sound must be
/// solved to. Measuring inside each solve makes that decision impossible without either
/// re-measuring (double the captures) or guessing.
///
/// It carries no more than the old inline capture produced (`asis` + `spread`), so nothing
/// downstream had to change shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScenePrepass {
    /// AS-IS loudness at the scene's authored knob values (LUFS).
    pub asis: f64,
    /// Dynamics spread (LU) of that same capture.
    pub spread: f64,
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
    saved: Option<&serde_json::Value>,
    mut on_scene: impl FnMut(u32, Option<&BatchedSceneOutcome>),
    mut cancelled: impl FnMut() -> bool,
) -> Result<Vec<BatchedSceneOutcome>, String> {
    let result = (|| {
        // Load in its own connection (set-after-load override + engage latch).
        {
            let mut s = Session::connect_lean()?;
            s.load_preset(slot)?;
            crate::settle(Duration::from_millis(settle_after_load_ms()));
        }
        crate::settle(Duration::from_millis(RECONNECT_GAP_MS));

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

            crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
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
                    clamp_kind: None,
                    clamp_reason: None,
                    verify_by_ear: false,
                    persist_mismatch: None,
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

/// THE REORDERED RUN'S PREPASS: measure every scene job's as-is reading BEFORE the batch
/// writes anything, stamping each one on its job ([`SceneJob::prepass`]) so the solve that
/// follows consumes it instead of taking its own capture. Total captures are UNCHANGED — the
/// same one-engage-per-scene as-is reading each solve always opened with, simply paid up
/// front.
///
/// WHY: [`crate::headroom_trade::plan_headroom_trade`] must see every sound's ceiling next to
/// every sound's target before it decides whether to move the base
/// `presetLevel`/`outputLevel` pair, because that decision changes what every LATER sound is
/// solved to. Measuring inside each solve makes the decision unavailable at the only moment
/// it can be acted on.
///
/// CALLER CONTRACT: the preset is already current (the caller ran `prepass_scene_docs`), and
/// this writes NOTHING — every reading is a pure `measure_scene_asis` (scene recall, engage,
/// capture, disengage), so there is no measurement dirt for the batch's deferred save to pick
/// up and no reload is owed before the write phase.
///
/// A row whose measurement FAILS is left with `prepass: None` and logged: its solve then
/// takes the capture itself and reports the failure through the batch's one reporting path
/// (`run_scene_jobs`), rather than inventing a second one here. A skip job is never measured.
///
/// Ends on a guaranteed fresh re-amp OFF — each capture already disengages, but an
/// interrupted one can strand the unit input-muted (`danger.md`).
///
/// `intended_preset_level` is the run's OWN `presetLevel` re-asserted on every reading (see
/// [`measure_scene_asis`]). The batched scene command passes the preset's own SAVED level —
/// not the headroom trade's raise, which is decided AFTER this prepass. It must match what
/// the solve captures render at: `correct_iter` takes a prepass reading as its `measured0`
/// and compares it to a post-apply capture, so two different renderings make the first
/// "response" include the level difference and can defeat the `no_authority` verdict outright.
/// The progress `message` a lane sends while a CEILING PREPASS capture is running.
///
/// A progress message on an ACTIVE row is that row's caption, and the wizard renders it two
/// ways depending on whether a capture is streaming (`RunBody`'s `rowStatus`):
///
/// - a capture IS streaming -> the caption is the VERB before the live number, `measuring · -18.9`
/// - nothing is streaming -> the caption is a NOTE, rendered verbatim, e.g. the freshness
///   barrier's "waiting for the device to commit the previous save…"
///
/// So send THIS one only from inside a measurement loop, wrapped around a `measure_*` call.
/// A message sent outside a capture is a note by construction and must read as a sentence.
pub const PREPASS_ACTIVE_MSG: &str = "measuring";

pub fn prepass_scene_ceilings(
    jobs: &mut [SceneJob],
    stimulus: &[f32],
    intended_preset_level: Option<f32>,
    mut on_scene: impl FnMut(u32),
    mut cancelled: impl FnMut() -> bool,
) -> Result<(), String> {
    let mut stopped = false;
    for job in jobs.iter_mut() {
        if cancelled() {
            stopped = true;
            break;
        }
        if job.skip.is_some() {
            continue;
        }
        on_scene(job.scene_slot);
        match require_live(
            || {
                measure_scene_asis(
                    job.scene_slot,
                    stimulus,
                    intended_preset_level,
                    &job.force_bypass,
                )
            },
            stimulus,
        ) {
            Ok(l) => {
                job.prepass = Some(ScenePrepass {
                    asis: l.integrated_lufs,
                    spread: l.spread_lu(),
                })
            }
            Err(e) if e == CANCELLED => {
                stopped = true;
                break;
            }
            Err(e) => log::warn!(
                "prepass ceiling for scene {} failed ({e}); its solve will re-measure",
                job.scene_slot
            ),
        }
        crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
    }
    reamp_off_guaranteed("prepass_scene_ceilings");
    if stopped {
        return Err(CANCELLED.to_string());
    }
    Ok(())
}

/// The maximum loudness this scene can reach AT THE CURRENT base `presetLevel` — the ceiling
/// the headroom trade compares against the row's target.
///
/// ONLY the amp-`outputLevel` (joint-k) lane can answer. That control is LINEAR IN dB with
/// full authority (HW, ~25 LU range), so the top of its range is an exact extrapolation from
/// the as-is reading: `asis + 20·log10(LEVEL_MAX / max_i current_i)` — the same ratio-
/// preserving `k_cap` [`solve_joint_k_at`] clamps to, so the two can never disagree about
/// where the top is.
///
/// A USER-HANDLE row answers `None` ON PURPOSE. An arbitrary block param (a taper `volume`, a
/// raw-dB `gain`, a wet `mix`) has no algebraically predictable response (`headroom_trade`
/// module header), so extrapolating one would be exactly the taper model this codebase refuses
/// to build. Such
/// a row's clamp is discovered by its own bounded secant and reported honestly; it simply
/// does not get a vote in the trade.
///
/// `None` also for a skip job, a knob-less job, and a job with no prepass reading.
pub fn scene_ceiling_lufs(job: &SceneJob) -> Option<f64> {
    if job.skip.is_some() || job.handle.is_some() {
        return None;
    }
    let asis = job.prepass?.asis;
    let max_cur = job
        .knobs
        .iter()
        .map(|kt| kt.current as f64)
        .fold(0.0f64, f64::max);
    if max_cur <= 0.0 {
        return None;
    }
    Some(asis + 20.0 * (LEVEL_MAX as f64 / max_cur).log10())
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
///
/// ONE per-row variation rides this runner, defaulted OFF so an existing caller's batch is
/// byte-identical: [`SceneJob::handle`] — the user named their OWN control for that scene, so
/// it is solved by [`handle_one_scene`] (the generic param secant) instead of the amp
/// joint-k. The amp `outputLevel` remains the ONLY control this lane touches when no handle
/// is given; a handle is an explicit, per-row user choice.
/// THE LEVEL EVERY CAPTURE IN A SCENE BATCH MUST RENDER AT. Each per-scene capture recalls
/// its scene first, and the recall runs the device's own level-apply — so a capture renders
/// at whatever level that apply serves, not at the one this run means.
///
/// Two ways that diverges, and BOTH need the same re-assert:
///  · a landed headroom trade holds a RAISED `presetLevel` UNSAVED in the working copy until
///    this batch's one save — without the re-assert every scene is solved against the
///    pre-raise sound;
///  · with no trade, the recall serves the COMMITTED level, and the load store commits
///    LAZILY — so shortly after this preset's base row saved a new level, every capture still
///    renders at the OLD one. This arm used to be `None`, i.e. exactly that bug. HW, fw
///    1.8.45, 2026-08-19, slot 26: the footswitch lane's twin of this measured a whole batch
///    5.53 dB quiet, that being 20·log10(0.51009/0.2699) — the just-saved level over the
///    pre-run one (see `commands/level_footswitch.rs`'s `intended_pl`).
///
/// The trade's unsaved raise wins when there is one; otherwise the preset's own SAVED level,
/// read from the complete field-8 doc the caller already holds (the fresher of the device's
/// two stores). Rendering is then independent of commit timing.
///
/// COUPLED WITH THE PREPASS. `prepass_scene_ceilings` must be given the SAME level, or the
/// as-is reading it produces and the post-apply captures compared against it render
/// differently and the difference is read as knob response — see `commands/level_scenes.rs`.
/// Split out as a named function so this decision is unit-gated: it is not otherwise
/// observable offline, because SimDevice only reverts a recall's level for a slot this run
/// has already saved.
pub(crate) fn scene_capture_level(
    hold: Option<&TradeHold>,
    saved: Option<&serde_json::Value>,
) -> Option<f32> {
    hold.map(|h| h.preset_level).or_else(|| {
        saved
            .and_then(crate::audiograph::preset_level)
            .map(|v| v as f32)
    })
}

#[allow(clippy::too_many_arguments)]
pub fn level_scenes_oneshot(
    slot: u32,
    jobs: &[SceneJob],
    stimulus: &[f32],
    save: bool,
    restore_scene: Option<u32>,
    saved: Option<&serde_json::Value>,
    hold: Option<&TradeHold>,
    // A5/F2: `(group, node, ORIGINAL saved bypass)` for a base-requested job's own isolation
    // (empty on every run that never isolated anything) — see `run_scene_jobs`'s param doc.
    isolation_restore: &[(String, String, bool)],
    on_scene: impl FnMut(u32, Option<&BatchedSceneOutcome>),
    // B6: forwarded to `run_scene_jobs` verbatim — see its own doc.
    on_tail: impl FnMut(&str),
    cancelled: impl FnMut() -> bool,
) -> Result<Vec<BatchedSceneOutcome>, String> {
    let intended_preset_level = scene_capture_level(hold, saved);
    run_scene_jobs(
        slot,
        jobs,
        save,
        restore_scene,
        hold,
        isolation_restore,
        on_scene,
        on_tail,
        cancelled,
        move |job: &SceneJob| match &job.handle {
            Some(handle) => handle_one_scene(
                slot,
                job,
                handle,
                stimulus,
                job.target_lufs,
                save,
                saved,
                intended_preset_level,
            ),
            None => jointk_one_scene(
                slot,
                job,
                stimulus,
                job.target_lufs,
                save,
                true,
                saved,
                // A scene amp may legitimately be solved all the way to silence; only
                // the trade's BASE hold carries the fader floor.
                LEVEL_MIN,
                intended_preset_level,
            ),
        },
    )
}

/// What [`apply_headroom_trade`] actually did to the base pair — the caller needs all of it:
/// the raised (UNSAVED) `presetLevel` has to be re-asserted at the batch's one save (the
/// pre-save scene recall reverts it otherwise, see `recall_reassert_save`), and the raise in
/// dB is what re-targets every benefiting sound's prepass reading.
#[derive(Debug, Clone, PartialEq)]
pub struct TradeApplied {
    /// dB the base `presetLevel` went UP by. Exact (module header).
    pub raise_db: f64,
    /// The raised `presetLevel`, sitting UNSAVED in the working copy. Pass it to the batch
    /// runner as `reassert_pl`.
    pub preset_level: f32,
    /// What the base `presetLevel` was before the trade — the value the back-out restores
    /// and the anchor the Summary's "Restore original" needs.
    pub previous_preset_level: f32,
    /// The solved base amp `outputLevel`(s) holding base at its target. On a parallel merge
    /// BOTH lanes are here, scaled by the same joint factor.
    pub base_levels: Vec<f32>,
}

/// Why [`apply_headroom_trade`] backed the pair out. Carries the ONE measurement the caller
/// cannot re-derive: how far base overshot its target when the hold pinned at the fader floor,
/// which is what the single bounded re-plan
/// ([`crate::headroom_trade::replan_after_floor_pin`]) is computed from.
#[derive(Debug, Clone, PartialEq)]
pub struct TradeFailure {
    pub kind: crate::headroom_trade::ClampKind,
    pub why: String,
    /// LU base ended ABOVE its target with the base fader on [`crate::headroom_trade::
    /// BASE_FADER_FLOOR`]. `None` for every other failure — a cancel, a dropped write, a
    /// no-authority amp, or a mid-range unconverged hold affords no smaller retry.
    pub base_overshoot_lu: Option<f64>,
}

/// The UNSAVED base pair a landed headroom trade left in the working copy, handed to the
/// batch runner so its ONE save persists BOTH halves and the post-save re-read covers them.
///
/// The compensating base-amp `outputLevel`(s) are just as unsaved as the raise and just as
/// much part of the trade: leaving them out of the run's `written` list would mean
/// `verify_persisted_writes` confirmed the scenes' values while saying nothing about the pair
/// that moved every one of them.
#[derive(Debug, Clone)]
pub struct TradeHold {
    /// The raised `presetLevel` to re-assert at the save (the pre-save scene recall runs the
    /// device's own level-apply and would otherwise revert it — see `recall_reassert_save`).
    pub preset_level: f32,
    /// The solved base amp `outputLevel` writes, in `PersistedWrite` form (scene slot
    /// `BASE_SCENE_SLOT` — `persisted_value` reads those straight off the base graph).
    pub writes: Vec<PersistedWrite>,
    /// A5/F2 DETECTION: `(group_id, node_id, expected_bypass)` for every node the hold's own
    /// base job isolated — `expected_bypass` is what the SAVED document held for that node
    /// BEFORE isolation (`footswitch::block_bypassed_in_base`), i.e. what `undo_base_isolation`
    /// already wrote back. Empty when the base job carried no isolation. Threaded to
    /// `verify_persisted_writes` so a silently dropped inverse write is visible at the
    /// post-save re-read — that function checks only `node_param_f64` numeric values otherwise
    /// and a bypass bool is invisible to it.
    pub force_bypass_restore: Vec<(String, String, bool)>,
}

/// EXECUTE the benefit-aware headroom trade: raise base `presetLevel` by exactly the planned
/// dB, then SOLVE the base amp `outputLevel` back down so the base sound stays on its target.
///
/// WHY THE SECOND HALF IS A SOLVE AND NOT ARITHMETIC. The raise is exact (`headroom_trade`
/// module header); the amp fader response is not (module header: "WHAT THIS MODULE
/// DELIBERATELY DOES NOT DO"). So the hold reuses [`jointk_one_scene`]'s measure →
/// closed-form → verify → bounded-secant correction, which is the only thing in this codebase
/// allowed to decide a fader value.
///
/// PARALLEL MERGE: nothing special is needed here — `base_job` carries BOTH lane amps as
/// knobs and joint-k scales them by ONE factor, which is exactly the required "scale both
/// amps by the same dB". A single-amp trade on a merge would shift the lane blend, i.e.
/// change the tone; the joint-k machinery already forbids that.
///
/// ATOMICITY (the `PartialTrade` state). The raised `presetLevel` and the compensating
/// `outputLevel` are ONE inseparable edit — half of it persisted leaves the preset either
/// uniformly loud or uniformly quiet. So any failure of the hold backs the WHOLE pair out by
/// reloading the stored preset (which discards every unsaved working-copy write, the raise
/// included) and returns a clamp kind, never a partial success. Nothing has been saved at
/// this point, so the reload cannot destroy a persisted value; it also runs BEFORE the
/// batch's write phase, so it is not a load in front of `save_deferred_scene_writes`.
///
/// CALLER CONTRACT: the preset is already current, `plan.raise_db > 0`, and `base_job` is the
/// BASE row's job with its amp knobs at their PRE-RAISE values. `base_job.prepass` is a reading
/// taken at the OLD `presetLevel`, so it is SHIFTED by `raise_db` rather than used as-is — see
/// the seed comment on `hold_job` below for why that shift is exact and what it trades.
pub fn apply_headroom_trade(
    slot: u32,
    plan: &crate::headroom_trade::TradePlan,
    preset_level: f32,
    base_job: &SceneJob,
    stimulus: &[f32],
    saved: Option<&serde_json::Value>,
    mut cancelled: impl FnMut() -> bool,
) -> Result<TradeApplied, TradeFailure> {
    use crate::headroom_trade::ClampKind;
    let bail = |kind: ClampKind, why: String| -> TradeFailure {
        // Back out the WHOLE pair: the reload discards the unsaved raise and every unsaved
        // fader write together, so no half-trade can ever be persisted.
        if let Err(e) = restore_saved_preset(slot) {
            log::warn!(
                "restore_saved_preset failed backing out a headroom trade (slot {slot}): {e}"
            );
        }
        reamp_off_guaranteed("headroom_trade_backout");
        TradeFailure {
            kind,
            why,
            base_overshoot_lu: None,
        }
    };
    if cancelled() {
        return Err(TradeFailure {
            kind: ClampKind::PartialTrade,
            why: CANCELLED.to_string(),
            base_overshoot_lu: None,
        });
    }
    let raised = crate::headroom_trade::raised_preset_level(preset_level, plan.raise_db);
    // Raise FIRST and UNSAVED. Every measure below connects LEAN (no `load_preset`), so the
    // working-copy value survives each fresh re-amp connection (HW: unsaved writes persist
    // across reconnects).
    {
        crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
        let mut s = Session::connect().map_err(|e| TradeFailure {
            kind: ClampKind::PartialTrade,
            why: e,
            base_overshoot_lu: None,
        })?;
        // A failed `set_preset_level` is NOT proof the set never landed — the write may have
        // reached the device and only its ack failed. Back out rather than leave a raise with
        // no compensating fader.
        s.set_preset_level(raised)
            .map_err(|e| bail(ClampKind::PartialTrade, e))?;
        crate::settle(Duration::from_millis(SETTLE_AFTER_SET_MS));
    }
    // SEED THE HOLD FROM BASE'S OWN PREPASS, SHIFTED BY THE RAISE. `presetLevel` is exact
    // (module header) — the same physics `retarget_prepass_after_trade` already applies to
    // every benefiting scene — so base's as-is reading at the new level is its old one plus
    // `raise_db`, exactly. Taking
    // a fresh as-is capture instead cost one engage per hold attempt and another on the re-plan
    // retry, to re-derive a number the arithmetic gives for free.
    //
    // WHAT THIS TRADES: if the `set_preset_level` above were silently DROPPED (its ack is not
    // proof, which is why the failure path backs out), the seeded reading would be `raise_db`
    // too loud and the open-loop solve would start that far off. The hold still runs
    // `verify: true` plus the bounded-secant correction below, so the miss surfaces at the
    // POST-WRITE verify — one capture later than the prologue would have caught it — and the
    // pair is backed out either way. A base row with NO prepass falls back to `None` and the
    // prologue measures exactly as it did before.
    let hold_job = SceneJob {
        prepass: base_job.prepass.map(|p| ScenePrepass {
            asis: p.asis + plan.raise_db,
            ..p
        }),
        ..base_job.clone()
    };
    let solved = match jointk_one_scene(
        slot,
        &hold_job,
        stimulus,
        hold_job.target_lufs,
        true, // defer: the batch's ONE save persists this with everything else
        true, // verify + bounded-secant correction — the fader is not predictable
        saved,
        // ⟦4a⟧ THE HOLD'S FLOOR (danger.md: `outputLevel = 0` is deep digital silence) — the
        // solve that pays for the raise stops at `BASE_FADER_FLOOR`, reported as `TradeFloor`
        // below.
        crate::headroom_trade::BASE_FADER_FLOOR,
        // The raise above is UNSAVED, and every capture the hold takes recalls base first —
        // which runs the device's own level-apply and would revert it. Re-assert it on each
        // one, or the hold solves the base fader against the PRE-raise sound.
        Some(raised),
    ) {
        Ok(s) => s,
        // A CANCEL backs the pair out exactly like a failure does. The raise is already in
        // the working copy and the hold did not complete, so returning early would strand a
        // half-trade there for the NEXT thing that saves this preset (a save on the unit, a
        // later non-leveling flow) to persist. `restore_saved_preset` is cancel-safe by
        // design and danger.md requires the post-cancel restore to run to completion.
        Err(e) => return Err(bail(ClampKind::PartialTrade, e)),
    };
    if solved.clamped {
        // The hold FAILED: base cannot be held at its target with the raise in place. See
        // `trade_hold_failure_kind`'s own doc for the three-cause mapping.
        let kind = trade_hold_failure_kind(solved.clamp_kind, solved.pinned);
        let why = solved
            .clamp_reason
            .clone()
            .unwrap_or_else(|| kind.message().to_string());
        let mut failure = bail(kind, why);
        if kind == ClampKind::TradeFloor {
            // How far ABOVE its target base ended with the fader on the floor — the dB the
            // raise has to give back for the retry (presetLevel exact — module header).
            failure.base_overshoot_lu = Some(solved.lufs - hold_job.target_lufs);
        }
        return Err(failure);
    }
    // A5/F2: the hold landed with base's isolation still forced live in the device's working
    // copy (every capture above re-asserted it via `base_job.force_bypass`). A preset load is
    // FORBIDDEN from here on — the raise and the solved fader are UNSAVED deferred writes, and
    // "never load before `save_deferred_scene_writes`" applies literally: a reload would wipe
    // them just as surely as it undoes the isolation. So the isolation is undone with INVERSE
    // writes instead, on their own fresh connection, and a failure to do so backs the WHOLE
    // trade out rather than let the batch's terminal save persist every pedal forced off.
    //
    // This owner is LOAD-BEARING on the anchor-only path: when base arrived only as an anchor
    // (no wire job of its own) it is stripped from PHASE 3 before `run_scene_jobs` ever runs,
    // so that function's OWN pre-save isolation-restore list sees an empty batch and this call
    // is the only place left in the whole run that can clean base's isolation up. On a
    // `base_requested` run base survives into PHASE 3, where `run_scene_jobs`' own pre-save
    // guard restores it too — so this call is merely redundant-but-cheap there, not required.
    if !base_job.force_bypass.is_empty() {
        if let Err(e) = undo_base_isolation(&base_job.force_bypass, saved) {
            return Err(bail(
                ClampKind::PartialTrade,
                format!(
                    "the base isolation could not be undone after the trade landed ({e}) — \
                     backing the whole trade out rather than risk saving every isolated pedal \
                     forced off"
                ),
            ));
        }
    }
    Ok(TradeApplied {
        raise_db: plan.raise_db,
        preset_level: raised,
        previous_preset_level: preset_level,
        base_levels: solved.levels,
    })
}

/// A5/F2's SHARED WRITER: push a precomputed `(group, node, ORIGINAL saved bypass)` list back
/// onto the device, on its OWN fresh connection, without ever loading the preset (either
/// caller may be holding UNSAVED deferred writes — the trade's raise/fader, or the batch's own
/// solved scene values — that a load would discard).
///
/// `recall_base` FIRST is load-bearing, not decorative: the scene-context rule says a bare
/// write with no preceding recall lands in whatever scene the connection currently holds, and
/// this fresh connection holds none — an un-recalled inverse write would risk creating (or
/// polluting) a SCENE overlay for the forced node instead of restoring its BASE value.
fn write_isolation_restore(restore: &[(String, String, bool)]) -> Result<(), String> {
    if restore.is_empty() {
        return Ok(());
    }
    crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
    let mut s = Session::connect()?;
    recall_base(&mut s)?;
    for (g, n, original) in restore {
        s.change_parameter_bool(g, n, "bypass", *original)?;
    }
    Ok(())
}

/// A5/F2's SHARED DERIVATION: `(group, node, ORIGINAL saved bypass)` for a set of forced-bypass
/// nodes, read straight off the SAVED document (`footswitch::block_bypassed_in_base`) — the
/// value the batch found there before isolation ever touched it, so the pedal ends up exactly
/// where the user's preset already had it, never a guessed inverse of the forced flag. This is
/// the ONE derivation shared by `undo_base_isolation` (the trade hold's own cleanup),
/// `commands::level_scenes::isolation_restore_for_batch` (the base-requested job's PHASE-3
/// cleanup) and `trade_for_batch`'s landed-trade detection — previously three independent
/// copies that had drifted on the `saved == None` case.
///
/// `saved == None` returns EMPTY, never a guessed `false` for every node: there is nothing to
/// restore TO, and a blind `false` would risk actively un-bypassing a pedal the player had
/// deliberately engaged. Leaving the isolation in place is the conservative answer —
/// `verify_persisted_writes`' own forced-bypass check still catches a leak at the post-save
/// re-read. Every caller here only reaches `None` defensively (each planner refuses its trade
/// or batch without a saved doc), so this warns once rather than failing.
pub(crate) fn isolation_restore_list(
    force_bypass: &[(String, String, bool)],
    saved: Option<&serde_json::Value>,
) -> Vec<(String, String, bool)> {
    let Some(saved_doc) = saved else {
        if !force_bypass.is_empty() {
            log::warn!(
                "isolation_restore_list: no saved document to restore {} forced bypass(es) \
                 from — returning no restore rather than guess",
                force_bypass.len()
            );
        }
        return Vec::new();
    };
    force_bypass
        .iter()
        .map(|(g, n, _forced)| {
            (
                g.clone(),
                n.clone(),
                crate::footswitch::block_bypassed_in_base(saved_doc, n),
            )
        })
        .collect()
}

/// A5/F2: reverse a base job's isolation bypass writes after a landed headroom trade hold. Thin
/// wrapper over `isolation_restore_list` (the derivation) + `write_isolation_restore` (the
/// writer) — see the former's doc for the `saved == None` policy.
fn undo_base_isolation(
    force_bypass: &[(String, String, bool)],
    saved: Option<&serde_json::Value>,
) -> Result<(), String> {
    write_isolation_restore(&isolation_restore_list(force_bypass, saved))
}

/// The base hold FAILED — name the cause. Pure, so the three-way mapping is unit-testable
/// without a device.
///
/// It reads the solve's OWN pinned-bound report ([`SceneSolve::pinned`]) rather than
/// re-deriving "is any lane on the floor?" from the levels: an author-MUTED lane sits at 0.0
/// by the player's choice, is deliberately excluded from binding the joint factor
/// ([`joint_k_floor`]) and is preserved untouched by the solve — so a levels scan calls every
/// mid-range stall on such a preset a floor pin, stamps the wrong wire cause AND burns the one
/// bounded re-plan on a raise the fader never refused. [`joint_levels_pinned`] already answers
/// the direction-aware question correctly (it ignores a bound the solve never moved a lane
/// to), so the answer is carried up rather than guessed at again here.
pub(crate) fn trade_hold_failure_kind(
    solved_kind: Option<crate::headroom_trade::ClampKind>,
    pinned: Option<PinnedBound>,
) -> crate::headroom_trade::ClampKind {
    use crate::headroom_trade::ClampKind;
    match solved_kind {
        // The amp never reached the USB 1/2 capture at all.
        Some(ClampKind::NoAuthority) => ClampKind::NoAuthority,
        // The fader genuinely ran out — the measured overshoot is what the ONE bounded
        // re-plan is computed from.
        _ if pinned == Some(PinnedBound::Floor) => ClampKind::TradeFloor,
        // Clamped MID-RANGE (or pinned at the TOP, which no base hold can pay for): the
        // bounded secant ran out of captures, which is NOT a floor. The pair IS backed out
        // and nothing is persisted, which is exactly what `PartialTrade` says.
        _ => ClampKind::PartialTrade,
    }
}

/// Re-target every job's PREPASS reading after a trade landed, so the write phase solves
/// against truth instead of pre-raise numbers.
///
/// TWO ANSWERS, and the split is the physics (both facts: `headroom_trade` module header):
/// * a BENEFITING sound (its `outputLevel` is pinned by its own scene overlay, so the
///   compensating base-fader drop misses it) gained EXACTLY `raise_db` — presetLevel is
///   exact. Its reading is shifted, no capture needed.
/// * every OTHER sound's change routes through the base FADER, whose response is not
///   algebraically predictable. Guessing it is precisely the taper model this codebase
///   refuses to build, so the reading is DROPPED and that sound's solve re-measures. The base
///   row is always dropped for the same reason (and its own hold already verified it).
///
/// `benefits` answers per scene slot; a slot it does not know is treated as NOT benefiting,
/// the conservative side (an extra capture, never a wrong number).
pub fn retarget_prepass_after_trade(
    jobs: &mut [SceneJob],
    raise_db: f64,
    benefits: impl Fn(u32) -> bool,
) {
    for job in jobs.iter_mut() {
        let keeps = job.scene_slot < crate::session::BASE_SCENE_SLOT && benefits(job.scene_slot);
        match (&mut job.prepass, keeps) {
            (Some(p), true) => p.asis += raise_db,
            (slot @ Some(_), false) => *slot = None,
            (None, _) => {}
        }
    }
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
#[allow(clippy::too_many_arguments)]
pub fn redistribute_clamped_headroom(
    slot: u32,
    new_preset_level: f32,
    jobs: &[SceneJob],
    stimulus: &[f32],
    restore_scene: Option<u32>,
    saved: Option<&serde_json::Value>,
    mut on_scene: impl FnMut(u32, Option<&BatchedSceneOutcome>),
    // B6 (F10): "the redistribute runner shares the seam" — this runner doesn't go through
    // `run_scene_jobs` (its own hand-rolled save + audio spot-verify below), so it takes the
    // same tail-emitter shape directly rather than inheriting it.
    mut on_tail: impl FnMut(&str),
    mut cancelled: impl FnMut() -> bool,
) -> Result<Vec<BatchedSceneOutcome>, String> {
    if cancelled() {
        return Err(CANCELLED.to_string());
    }
    // Raise presetLevel FIRST, UNSAVED. `measure_scene_asis` (the jointk measure) connects
    // LEAN — no `load_preset` — so this working-copy value survives every scene's fresh
    // re-amp connect (HW: unsaved writes persist across reconnects).
    {
        crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
        let mut s = Session::connect()?;
        s.set_preset_level(new_preset_level)?;
        crate::settle(Duration::from_millis(SETTLE_AFTER_SET_MS));
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
        let result = jointk_one_scene(
            slot,
            job,
            stimulus,
            job.target_lufs,
            true,
            true,
            saved,
            LEVEL_MIN,
            // The raise above is UNSAVED until this run's one save; each scene's capture
            // recalls its scene and the recall reverts it, so re-assert it per capture.
            Some(new_preset_level),
        );
        crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
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
    reamp_off_guaranteed("redistribute");

    // ATOMICITY: a cancel or ANY failed/off-branch compensating write leaves a PARTIAL
    // redistribution — reload the stored preset to discard the unsaved pl + writes, persist
    // nothing. (A jointk-reported headroom clamp on a still-clamped scene is EXPECTED and
    // not a failure; an ERROR or a no-authority `clamp_reason` is.)
    let partial = outcomes
        .iter()
        .any(|o| o.failure.is_some() || o.clamp_reason.is_some());
    if stopped || partial {
        if let Err(e) = restore_saved_preset(slot) {
            log::warn!(
                "restore_saved_preset failed after aborted redistribution (slot {slot}): {e}"
            );
        }
        return Err(if stopped {
            CANCELLED.to_string()
        } else {
            "redistribution aborted: a compensating write did not land — nothing saved".to_string()
        });
    }

    // ONE save — new pl + every compensated outputLevel together, original scene
    // recalled and the UNSAVED raised pl re-asserted after it (the recall would
    // otherwise revert it to the saved value — see `recall_reassert_save`).
    // No separate scene witness here: `recall_reassert_save` (called inside
    // `save_deferred_scene_writes`) already registers the raised `new_preset_level` as this
    // save's `PresetLevel` witness — that single value identifies the whole save.
    on_tail("Saving preset…");
    save_deferred_scene_writes(slot, restore_scene, Some(new_preset_level), None)?;

    // Post-save AUDIO spot-verify at the PERSISTED pl (the wrong-pl-solve guard). Pick a
    // compensated sound that actually moved (writes > 0); re-measure it as-is. Advisory —
    // the save already landed, so a miss WARNS (the UI offers Restore), never re-writes.
    if let Some(check) = outcomes
        .iter()
        .find(|o| o.writes > 0 && o.final_lufs.is_some())
    {
        on_tail("Verifying…");
        crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
        // The save above persisted `new_preset_level`, so re-asserting it here is a
        // belt-and-braces no-op on a committed save and the CORRECT value while the
        // firmware's lazy commit is still in flight — either way the spot-verify reads the
        // level this run intended, never a stale one.
        match require_live(
            || measure_scene_asis(check.scene_slot, stimulus, Some(new_preset_level), &[]),
            stimulus,
        ) {
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

/// A5/F2 DETECTION: union two `(group, node, expected_bypass)` isolation-restore lists, deduped
/// by node — `base_restore` (already-derived, e.g. the trade hold's own expectations) wins ties,
/// since a base row carries exactly one isolation set and both sources describe the SAME nodes
/// when they overlap. Pure so the union rule is unit-testable without a device: `verify_
/// persisted_writes` must check every node EITHER source isolated, whether a trade landed, a
/// base-requested job's own solve isolated it, or (rare) both.
fn union_isolation_restore(
    base_restore: &[(String, String, bool)],
    other_restore: &[(String, String, bool)],
) -> Vec<(String, String, bool)> {
    let mut v = base_restore.to_vec();
    for (g, n, expected) in other_restore {
        if !v.iter().any(|(_, vn, _)| vn == n) {
            v.push((g.clone(), n.clone(), *expected));
        }
    }
    v
}

#[cfg(test)]
mod union_isolation_restore_tests {
    use super::*;

    fn entry(node: &str, expected: bool) -> (String, String, bool) {
        ("G1".to_string(), node.to_string(), expected)
    }

    #[test]
    fn both_empty_is_empty() {
        assert!(union_isolation_restore(&[], &[]).is_empty());
    }

    // NO-TRADE CASE: a base-requested run with no headroom trade has nothing in the hold's
    // own list, but its OWN isolation still needs to reach the verify — the union must not
    // require a non-empty first argument to carry the second through.
    #[test]
    fn an_empty_hold_list_still_carries_the_runs_own_isolation() {
        let other = vec![entry("pedal", false)];
        assert_eq!(union_isolation_restore(&[], &other), other);
    }

    #[test]
    fn an_empty_other_list_still_carries_the_holds_isolation() {
        let base = vec![entry("pedal", true)];
        assert_eq!(union_isolation_restore(&base, &[]), base);
    }

    // Both sources describe the SAME node (a trade landed AND the base row itself isolated
    // something) — the union must not double-count it, and the base list's own expectation
    // wins rather than being silently overwritten.
    #[test]
    fn a_shared_node_is_deduped_with_the_base_lists_value_winning() {
        let base = vec![entry("pedal", true)];
        let other = vec![entry("pedal", false)];
        assert_eq!(
            union_isolation_restore(&base, &other),
            vec![entry("pedal", true)]
        );
    }

    #[test]
    fn distinct_nodes_from_both_sources_are_all_kept() {
        let base = vec![entry("a", true)];
        let other = vec![entry("b", false)];
        let got = union_isolation_restore(&base, &other);
        assert_eq!(got.len(), 2);
        assert!(got.contains(&entry("a", true)));
        assert!(got.contains(&entry("b", false)));
    }
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
#[allow(clippy::too_many_arguments)]
fn run_scene_jobs(
    slot: u32,
    jobs: &[SceneJob],
    save: bool,
    restore_scene: Option<u32>,
    // `hold`: the UNSAVED base pair a headroom trade raised, if one ran. The batch's ONE save
    // recalls the preset's original scene first, and that recall runs the device's own
    // level-apply — silently reverting an unsaved `presetLevel` right before the save persists
    // it (HW). Re-asserting it is what makes the trade's two halves land TOGETHER; `None`
    // (every untraded run) is byte-identical to the previous behaviour.
    hold: Option<&TradeHold>,
    // A5/F2 (TRADE-PATH GAP): when the BASE job itself is one of `jobs` (base_requested — not
    // stripped as anchor-only upstream), its own solve routes through `jointk_one_scene` →
    // `apply_levels(defer: true)`, which re-asserts `job.force_bypass` on EVERY capture and
    // never undoes it — that seam exists to keep the isolation alive across a multi-capture
    // secant, not to clean it up, so a trade's own `undo_base_isolation` (which ran BEFORE this
    // job even solved) is left stale the moment base solves again. `(group, node, ORIGINAL
    // saved bypass)`, same derivation `undo_base_isolation` uses — empty on every run that
    // never isolated anything.
    isolation_restore: &[(String, String, bool)],
    mut on_scene: impl FnMut(u32, Option<&BatchedSceneOutcome>),
    // B6 (F10): the seam widened to carry a batch-wide caption for the two phases below that
    // have no single scene to report progress against — "Saving preset…" just before
    // `save_deferred_scene_writes`, "Verifying…" just before `verify_persisted_writes`. A
    // no-op closure (`|_| {}`) is the byte-identical-to-before default for any caller that
    // doesn't want the captions.
    mut on_tail: impl FnMut(&str),
    mut cancelled: impl FnMut() -> bool,
    mut solve: impl FnMut(&SceneJob) -> Result<SceneSolve, String>,
) -> Result<Vec<BatchedSceneOutcome>, String> {
    let reassert_pl = hold.map(|h| h.preset_level);
    let mut outcomes = Vec::with_capacity(jobs.len());
    let mut attempted = false;
    let mut stopped = false;
    // Every value this batch actually wrote, for the post-save re-read (`verify_persisted_writes`).
    // SEEDED with the trade's own base pair when one landed: those `outputLevel` writes are
    // just as unsaved and just as load-bearing as the scene overlays, so the post-save re-read
    // has to cover them too.
    let mut written: Vec<PersistedWrite> = hold.map(|h| h.writes.clone()).unwrap_or_default();
    // A5/F2 DETECTION: the union of both isolation-restore sources — the trade hold's own
    // expectations (see `TradeHold::force_bypass_restore`'s doc) AND this run's own
    // `isolation_restore` (the base-requested path, cleaned up below).
    let force_bypass_restore = union_isolation_restore(
        hold.map(|h| h.force_bypass_restore.as_slice())
            .unwrap_or(&[]),
        isolation_restore,
    );
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

        // A skip job (unclassifiable scene: mic/split lane/no active amp/…) is reported
        // as a failed outcome and the run continues — never aborts the whole pass.
        if let Some(reason) = &job.skip {
            let outcome = failed_scene_outcome(
                job.scene_slot,
                job.target_lufs,
                reason.clone(),
                t0.elapsed().as_millis(),
            );
            on_scene(job.scene_slot, Some(&outcome));
            outcomes.push(outcome);
            continue;
        }

        attempted = true;
        let result = solve(job);

        crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
        let outcome = match result {
            Ok(s) => {
                // Harvested BEFORE `solved_scene_outcome` consumes the solve: the outcome keeps
                // only the loudest lane's level, but every lane amp was written and each one has
                // to be re-read. `writes == 0` wrote nothing, so there is nothing to confirm.
                if s.writes > 0 {
                    written.extend(job.knobs.iter().zip(&s.levels).filter_map(|(k, &v)| {
                        match &k.knob {
                            LevelKnob::Block {
                                node_id,
                                parameter_id,
                                ..
                            } => Some(PersistedWrite {
                                scene_slot: job.scene_slot,
                                node_id: node_id.clone(),
                                parameter_id: parameter_id.clone(),
                                value: v,
                            }),
                            // Not written by a scene job, and not scene-scoped if it were.
                            LevelKnob::PresetLevel => None,
                        }
                    }));
                }
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
        on_scene(job.scene_slot, Some(&outcome));
        outcomes.push(outcome);
    }
    // Guaranteed fresh re-amp OFF (each `measure_knob_at`/`apply_level` already
    // disengages, but an interrupted capture can strand it).
    let _ = Session::connect_lean().and_then(|mut s| s.set_reamp_mode(false).map(|_| ()));
    // A5/F2 (TRADE-PATH GAP): undo the base-requested isolation HERE, straight after the last
    // capture and BEFORE the one save below — this is the only place left that can, since every
    // capture in the loop above re-asserted `force_bypass` and dropped its own session without
    // ever clearing it (see the param doc). FAILURE here is not survivable: the working copy
    // still carries every isolated pedal forced off, and this batch's OWN deferred writes are
    // just as unsaved as that isolation — so a failed cleanup abandons the whole batch (reload,
    // exactly like `apply_headroom_trade`'s own back-out) rather than let the save below persist
    // a pedal in a state the player never authored. A lost batch beats a corrupted save.
    if attempted && !isolation_restore.is_empty() {
        if let Err(e) = write_isolation_restore(isolation_restore) {
            if let Err(e2) = restore_saved_preset(slot) {
                log::warn!(
                    "restore_saved_preset failed backing the batch out after a failed base \
                     isolation cleanup (slot {slot}): {e2}"
                );
            }
            return Err(format!(
                "the base isolation could not be undone before the save ({e}) — abandoning \
                 this batch's unsaved writes rather than risk saving a pedal forced off"
            ));
        }
    }
    // Did the run PERSIST a headroom trade? Only then is there anything to disclose on a
    // cancel: with nothing attempted the save below never fires and the reload two blocks down
    // discards the pair whole, so reporting a landed trade there would be a lie.
    let trade_persisted = save && attempted && hold.is_some();
    // The batch's ONE persist — after the re-amp OFF, on its own clean connection.
    // Fired on the stopped path too, so already-reported scenes are never lost.
    if save && attempted {
        // Witness: one written scene `outputLevel` this batch actually persisted — the
        // freshness registry's anchor for a NEXT run's same-slot prepass load. `SaveWitness::
        // Param` carries no group id: node+param alone locate the value (the comparator,
        // `witness_value_in_doc`, never needed one).
        // `written.first()` — the LOWEST-index written scene — is deliberate, not arbitrary:
        // it maximizes the odds a later harvest's `scenes` tail (often truncated, HW) still
        // reaches this scene's overlay (post-review amendment 3). Base jobs also land in
        // `written` at `scene_slot == session::BASE_SCENE_SLOT`, so the `<` guard below is
        // required, not a `!=` (post-review amendment 5) — a base write is a `Param`
        // witness too, but not a SCENE one.
        let scene_witness = written.first().map(|w| SaveWitness::Param {
            node: w.node_id.clone(),
            param: w.parameter_id.clone(),
            value: w.value,
            scene: (w.scene_slot < crate::session::BASE_SCENE_SLOT).then_some(w.scene_slot),
        });
        if stopped {
            // The callee already warns internally on its own first failure; this
            // catches the case where its retry ALSO failed (cancelled path only —
            // the non-cancelled `?` below still surfaces a hard error to the caller).
            on_tail("Saving preset…");
            if let Err(e) =
                save_deferred_scene_writes(slot, restore_scene, reassert_pl, scene_witness)
            {
                log::warn!("save_deferred_scene_writes failed on cancel (slot {slot}): {e}");
            }
            // A cancelled run that LANDED A TRADE returns its outcomes (see below), so they
            // need the same persist verdict a completed run's get.
            if trade_persisted {
                on_tail("Verifying…");
                verify_persisted_writes(slot, &written, &force_bypass_restore, &mut outcomes);
            }
        } else {
            on_tail("Saving preset…");
            save_deferred_scene_writes(slot, restore_scene, reassert_pl, scene_witness)?;
            // Confirm the save kept what the run reports — no re-capture, one field-8 read,
            // after every audio step. A stopped run with no trade returns CANCELLED below and
            // its outcomes are discarded, so it is not worth a read.
            on_tail("Verifying…");
            verify_persisted_writes(slot, &written, &force_bypass_restore, &mut outcomes);
        }
    }
    // ⟦3b⟧ CANCELLED BEFORE THE FIRST SOLVE. Nothing was deferred, so `save && attempted` above
    // is false and NO `save_deferred_scene_writes` ran — which makes this reload safe under
    // danger.md's "never a load in front of the deferred save": there are no deferred writes
    // to wipe. What there CAN be is a landed headroom trade's raised `presetLevel` + solved
    // base fader sitting dirty in the working copy with nothing to pay for them; the reload
    // discards the pair whole, exactly as `apply_headroom_trade`'s own back-out does.
    if stopped && !attempted {
        if let Err(e) = restore_saved_preset(slot) {
            log::warn!("restore_saved_preset failed after a pre-solve cancel (slot {slot}): {e}");
        }
    }
    if stopped {
        // ⟦3a⟧ CANCEL AFTER A LANDED TRADE: PERSIST AND DISCLOSE, never silently.
        //
        // The save above has just persisted the raised `presetLevel` + the base fader that
        // holds base on target. Backing the trade out HERE was the alternative and it is
        // WRONG: every scene this run already solved was solved AT the raised level, so
        // lowering `presetLevel` again would leave those persisted values off target by
        // exactly `raise_db` while the run reported them on target — silently wrong numbers,
        // the one outcome this codebase refuses. Skipped benefiting scenes DO end up
        // `raise_db` louder than authored, which is a real cost — so the run returns its
        // outcomes (rather than the historical `CANCELLED`, whose results the command drops)
        // and the trade summary rides along, giving the UI both the disclosure and the
        // pre-trade values to restore from. danger.md's cancel contract is intact either way:
        // nothing early-returns past the re-amp engage and the cleanup above ran whole.
        if trade_persisted {
            log::warn!(
                "slot {slot}: cancelled AFTER a headroom trade landed — the raised base pair is \
                 persisted and reported, not silently backed out"
            );
            return Ok(outcomes);
        }
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
fn save_deferred_scene_writes(
    slot: u32,
    restore_scene: Option<u32>,
    reassert_pl: Option<f32>,
    scene_witness: Option<SaveWitness>,
) -> Result<(), String> {
    // NOT `sleep_or_cancel`: this is ALSO fired on cancel, to persist the scene overlays
    // already written. Bailing here would throw away the run's completed work.
    let attempt = || -> Result<(), String> {
        crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
        let mut s = Session::connect()?;
        recall_reassert_save(&mut s, slot, restore_scene, reassert_pl)
    };
    attempt().or_else(|e| {
        log::warn!("deferred scene save failed ({e}); retrying on a fresh connection");
        attempt()
    })?;
    // Registered ONLY when the save didn't already carry a `PresetLevel` witness
    // (`recall_reassert_save` registers that one itself) — a scene deferred save with no
    // raised presetLevel needs its OWN witness so a later same-slot load has something to
    // wait for.
    if reassert_pl.is_none() {
        if let Some(w) = scene_witness {
            register_slot_save(slot, w);
        }
    }
    Ok(())
}

/// One solved scene write, to be checked against what the batch-end save actually persisted.
#[derive(Debug, Clone)]
pub(crate) struct PersistedWrite {
    /// The scene the value was written into; `session::BASE_SCENE_SLOT` = the base graph.
    pub scene_slot: u32,
    pub node_id: String,
    pub parameter_id: String,
    /// The value the run SOLVED and reported.
    pub value: f32,
}

/// Agreement band between a solved `f32` and its round-tripped JSON value. Wide enough for
/// the float formatting, far below any real leveling step.
const PERSIST_TOL: f64 = 1e-3;

/// What the saved document holds for one solved write: the SCENE OVERLAY's value for an FS
/// scene, the base graph node's for base (`scene_overlay` answers `Unknown` at/above
/// `BASE_SCENE_SLOT`, so base must not go through it).
/// What a post-save re-read can say about ONE solved write. The third state is the whole
/// point: "the document does not carry this value" and "the document cannot speak to this
/// location at all" are different facts, and only the first is evidence of a lost write.
enum PersistedRead {
    /// The saved document answers with this value.
    Value(f64),
    /// The document COVERS the location and holds nothing there — a genuine miss.
    Absent,
    /// The document cannot answer: the section the value lives in never arrived.
    Unverifiable,
}

fn persisted_value(saved: &serde_json::Value, w: &PersistedWrite) -> PersistedRead {
    if w.scene_slot >= crate::session::BASE_SCENE_SLOT {
        // `audioGraph` heads the document, so a tail truncation never reaches it — but a
        // read that lost it lost everything, and must not be read as a wiped write.
        if !saved.get("audioGraph").is_some_and(|g| g.is_object()) {
            return PersistedRead::Unverifiable;
        }
        return match crate::commands::level_footswitch::node_param_f64(
            saved,
            &w.node_id,
            &w.parameter_id,
        ) {
            Some(v) => PersistedRead::Value(v),
            None => PersistedRead::Absent,
        };
    }
    // CALL-SITE DECISION (three-state split): a value LOOKUP, so `Full` and `BypassOnly`
    // share one arm — read whatever the overlay holds. A `BypassOnly` overlay carries no
    // knob keys, so the lookup misses and the write counts as a MISS, which is right: a
    // scene write there was refused up front (`set_knobs`), so a value reported as written
    // into one is by definition not persisted.
    //
    // `Unknown` is the arm that must NOT collapse into that miss (HW, 2026-08-19): it means
    // the `scenes` section never arrived, and `scenes` sits at the document tail, so it is
    // exactly what a truncated field-8 read loses. "Friedman HBE" truncates at 21044 B before
    // its scenes, and the run duly warned that scene 3's `ACD_TwinReverb65NoFx/outputLevel`
    // "solved 0.7814 but the saved preset holds no such value" — while that scene's own
    // re-measure of the SAVED state read -22.99 LUFS against its -23 target, i.e. the write
    // had persisted perfectly. A false "did not persist" is worse than no check: it teaches
    // the user to distrust correct results, and the external judge SKIPS a good row.
    match overlay_param(saved, w.scene_slot, &w.node_id, &w.parameter_id) {
        SceneParamRead::Value(v) => match v.as_f64() {
            Some(v) => PersistedRead::Value(v),
            None => PersistedRead::Absent,
        },
        SceneParamRead::Absent => PersistedRead::Absent,
        SceneParamRead::Unknown => PersistedRead::Unverifiable,
    }
}

/// What a post-save re-read found, split by what it can honestly claim.
#[derive(Debug, Default)]
pub(crate) struct PersistCheck {
    /// Writes the save did NOT keep, as `(scene_slot, detail)` — the report must not show
    /// these numbers as persisted.
    pub(crate) missed: Vec<(u32, String)>,
    /// Writes the re-read cannot speak to, as `(scene_slot, detail)`. NOT a miss: the caller
    /// leaves the verdict unknown rather than stamping a mismatch it cannot support.
    pub(crate) unverifiable: Vec<(u32, String)>,
}

/// Grade every solved write against the re-read saved document. A value that diverges (or is
/// absent from a section that DID arrive) is a miss — the gate exists so a report can never
/// show numbers the save wiped. A value whose section never arrived is `unverifiable`, which
/// is not the same thing and must never be reported as a lost write.
pub(crate) fn persist_mismatches(
    saved: &serde_json::Value,
    writes: &[PersistedWrite],
) -> PersistCheck {
    let mut out = PersistCheck::default();
    for w in writes {
        match persisted_value(saved, w) {
            PersistedRead::Value(got) if (got - w.value as f64).abs() <= PERSIST_TOL => {}
            PersistedRead::Value(got) => out.missed.push((
                w.scene_slot,
                format!(
                    "{}/{} solved {:.4} but the save holds {got:.4}",
                    w.node_id, w.parameter_id, w.value
                ),
            )),
            PersistedRead::Absent => out.missed.push((
                w.scene_slot,
                format!(
                    "{}/{} solved {:.4} but the saved preset holds no such value",
                    w.node_id, w.parameter_id, w.value
                ),
            )),
            PersistedRead::Unverifiable => out.unverifiable.push((
                w.scene_slot,
                format!(
                    "{}/{} solved {:.4} but the re-read carries no section holding it",
                    w.node_id, w.parameter_id, w.value
                ),
            )),
        }
    }
    out
}

/// Post-save param-level verify: RE-READ the preset and confirm every solved write survived
/// the batch-end save, stamping `persist_mismatch` on each outcome. Cheap and audio-free —
/// one field-8 read per preset per run, after all capture work — and the one thing that stops
/// a summary from reporting pre-wipe numbers as persisted. `writes` pairs a scene slot with
/// the values solved for it; scenes that wrote nothing are not checked.
///
/// `force_bypass_restore` is A5/F2's DETECTION half: `(group_id, node_id, expected_bypass)` for
/// every node a trade's base isolation forced — this function otherwise reads only
/// `node_param_f64` numeric values (`persist_mismatches`), so a silently DROPPED inverse write
/// (the pedal saved forced OFF) would be invisible without it. A mismatch here is loud: it
/// means the run's own isolation cleanup did not land, so the saved preset carries a pedal in a
/// state the player never authored.
fn verify_persisted_writes(
    slot: u32,
    writes: &[PersistedWrite],
    force_bypass_restore: &[(String, String, bool)],
    outcomes: &mut [BatchedSceneOutcome],
) {
    if writes.is_empty() && force_bypass_restore.is_empty() {
        return;
    }
    // `save_deferred_scene_writes` has just closed its session and `read_saved_preset` sleeps
    // only AFTER itself, so the opening gap is the caller's to provide.
    crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
    // COMPLETE-OR-FAIL, not the plain read: this verifier compares SCENE OVERLAYS, which sit
    // at the tail of the document, and a field-8 stream that truncates before `scenes` makes
    // every checked scene look unwritten. HW, 2026-08-19: "Friedman HBE" truncates at 21044 B
    // before its scenes section, and the run duly warned that scene 3's
    // `ACD_TwinReverb65NoFx/outputLevel` "solved 0.7814 but the saved preset holds no such
    // value" — while that scene's own re-measure of the SAVED state read -22.99 LUFS against
    // its -23 target, i.e. the write had persisted perfectly. A false "did not persist" is
    // worse than no check: it teaches the user to distrust correct results, and the external
    // judge SKIPS the row rather than passing it.
    let saved = match read_saved_preset_complete(slot) {
        Ok(p) => p,
        Err(e) => {
            log::warn!(
                "slot {slot}: post-save verify skipped — the saved preset could not be re-read \
                 completely ({e}); the reported values are unconfirmed"
            );
            return;
        }
    };
    let check = persist_mismatches(&saved, writes);
    for o in outcomes.iter_mut() {
        // Only a scene we actually checked gets a verdict; the rest stay `None` (unknown).
        if !writes.iter().any(|w| w.scene_slot == o.scene_slot) {
            continue;
        }
        // A scene the re-read cannot speak to stays `None` too — an unanswerable document is
        // not evidence of a lost write (see `PersistedRead::Unverifiable`). Second line of
        // defence behind the complete-or-fail read above, and the one that survives a
        // document which arrives whole but still carries no `scenes`.
        if check.unverifiable.iter().any(|(s, _)| *s == o.scene_slot) {
            continue;
        }
        let mismatch = check.missed.iter().any(|(scene, _)| *scene == o.scene_slot);
        o.persist_mismatch = Some(mismatch);
    }
    for (scene, detail) in &check.missed {
        log::warn!(
            "slot {slot} scene {scene}: the save did not persist the leveled value — {detail}"
        );
    }
    for (scene, detail) in &check.unverifiable {
        log::warn!(
            "slot {slot} scene {scene}: the leveled value is UNCONFIRMED, not lost — {detail}"
        );
    }
    // A5/F2 DETECTION: confirm every isolated node's bypass came back to what the pre-run
    // saved document held. A mismatch here means the trade's inverse-write cleanup silently
    // dropped — the saved preset now carries this pedal forced OFF — so it is stamped on the
    // BASE outcome (the only row a base isolation fact can attach to) and logged loudly.
    let mut isolation_leaked = false;
    for (_g, node_id, expected) in force_bypass_restore {
        let got = crate::footswitch::block_bypassed_in_base(&saved, node_id);
        if got != *expected {
            isolation_leaked = true;
            log::error!(
                "slot {slot}: base isolation on {node_id} did NOT persist as restored — saved \
                 bypass is {got}, expected {expected}; the trade's inverse write was dropped"
            );
        }
    }
    if isolation_leaked {
        if let Some(base_outcome) = outcomes
            .iter_mut()
            .find(|o| o.scene_slot == crate::session::BASE_SCENE_SLOT)
        {
            base_outcome.persist_mismatch = Some(true);
        }
    }
}

/// The FS-lane mirror of [`verify_persisted_writes`]: re-read the saved preset ONCE after the
/// FS batch save and confirm every solved+saved switch's write survived, stamping
/// `persist_mismatch` on the result at each `(idx, node_id, param, value, is_assign)` entry
/// (`is_assign` picks the read: `dspUnitParameters` for a Bake, the `ftsw` table's `valueA`
/// for an Assign — the two writes land in different places on the device).
///
/// HONEST CONTRACT (§A4, restated): this detects ONLY "my writes didn't persist" (a dropped
/// chunked edit / lapse / rejection). It does NOT detect staleness or the pre-save-revert
/// shape — field-8 is read-your-writes and would happily echo stale-saved bytes right back.
/// Revert coverage is the registry barrier (`ensure_fresh_load`) alone. `base_expect` is the
/// run's own earlier base-save `presetLevel` expectation, if any — snapshotted by the CALLER
/// via [`registered_preset_level`] BEFORE the batch's save overwrote the slot's registry
/// entry with its own `Param` witness (by the time this runs, the registry can no longer
/// answer). A mismatch there means THAT save reverted — which no per-switch param check
/// would ever catch on its own — so every switch in this batch is stamped mismatched too
/// (the whole preset's base sound, not just one switch, is now suspect).
pub(crate) fn verify_fs_persisted_writes(
    slot: u32,
    writes: &[(usize, String, String, f32, bool)],
    base_expect: Option<f32>,
    results: &mut [Option<FootswitchLevelResult>],
) {
    if writes.is_empty() {
        return;
    }
    crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
    // COMPLETE-OR-FAIL for the same reason as the scene twin above: a truncated document
    // reports a write that landed as a write that vanished.
    let saved = match read_saved_preset_complete(slot) {
        Ok(p) => p,
        Err(e) => {
            log::warn!(
                "slot {slot}: FS post-save verify skipped — the saved preset could not be \
                 re-read completely ({e}); the reported values are unconfirmed"
            );
            return;
        }
    };
    for (idx, verdict) in fs_persist_verdicts(&saved, writes, base_expect) {
        match verdict {
            Some(mismatch) => {
                if let Some(r) = results.get_mut(idx).and_then(|o| o.as_mut()) {
                    r.persist_mismatch = Some(mismatch);
                }
                if mismatch {
                    log::warn!(
                        "slot {slot}: FS result idx {idx} did not persist as solved (or the \
                         run's earlier base save appears reverted)"
                    );
                }
            }
            // Verdict left unknown on purpose — see `fs_persist_verdicts`.
            None => log::warn!(
                "slot {slot}: FS result idx {idx} is UNCONFIRMED, not lost — the re-read \
                 carries no section holding its value"
            ),
        }
    }
}

/// The pure grading behind [`verify_fs_persisted_writes`]: `(result index, verdict)` per
/// write, where `Some(true)` is a lost write, `Some(false)` a kept one, and `None` means the
/// re-read cannot speak to it.
///
/// That third state is the gate (HW, 2026-08-19). A field-8 read of a large preset is
/// tail-truncated, and the sections these writes live in — `ftsw` for an Assign, `audioGraph`
/// for a Bake — can simply be missing. Grading a missing SECTION as a missing VALUE reports a
/// write that landed as a write that vanished, which is worse than no check at all: it
/// teaches the user to distrust correct results. Second line of defence behind the
/// complete-or-fail read; it holds even for a document that arrives whole but short.
fn fs_persist_verdicts(
    saved: &serde_json::Value,
    writes: &[(usize, String, String, f32, bool)],
    base_expect: Option<f32>,
) -> Vec<(usize, Option<bool>)> {
    let ftsw = saved.get("ftsw").filter(|f| f.is_array());
    let has_graph = saved.get("audioGraph").is_some_and(|g| g.is_object());
    // The base-revert arm reads `audioGraph.presetLevel`, so without the graph its `None`
    // would read as "reverted" and condemn every switch in the batch on a read defect.
    if base_expect.is_some() && !has_graph {
        return writes.iter().map(|w| (w.0, None)).collect();
    }
    let base_reverted =
        base_expect.is_some_and(|pl| match crate::audiograph::preset_level(saved) {
            Some(got) => (got - pl as f64).abs() > PERSIST_TOL,
            None => true,
        });
    writes
        .iter()
        .map(|(idx, node, param, value, is_assign)| {
            if !if *is_assign {
                ftsw.is_some()
            } else {
                has_graph
            } {
                return (*idx, None);
            }
            let got = if *is_assign {
                ftsw.and_then(|f| ftsw_value_a(f, node, param))
            } else {
                crate::commands::level_footswitch::node_param_f64(saved, node, param)
            };
            let mismatch = base_reverted
                || match got {
                    Some(v) => (v - *value as f64).abs() > PERSIST_TOL,
                    None => true,
                };
            (*idx, Some(mismatch))
        })
        .collect()
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

/// The smallest joint factor a scale-DOWN may use before the QUIETEST AUDIBLE lane reaches
/// `floor`. The mirror image of `k_cap` (where the LOUDEST lane binds the scale-up): every
/// lane moves by ONE `k`, so on the way down the quietest one runs out first.
///
/// TWO RULES that must not be lost (danger.md: `outputLevel = 0` is deep digital silence):
/// * the bound is on **k**, never on the individual levels — clamping a level UP to the floor
///   would UN-MUTE an author-muted `0 · k` lane, i.e. change the preset's tone.
/// * a lane already at or below `floor` is author-parked, so it does NOT bind: it would
///   otherwise forbid every scale-down.
///
/// `floor = LEVEL_MIN` (0.0) — every caller but the headroom trade's base hold — yields 0 and
/// leaves the solve byte-identical to the pre-floor behaviour.
fn joint_k_floor(currents: impl Iterator<Item = f32>, floor: f32) -> f64 {
    if floor <= 0.0 {
        return 0.0;
    }
    // The same "quietest AUDIBLE lane" fold the trade planner's own fader-room estimate uses
    // — one helper, so the bound the solver enforces and the room the planner promises can
    // never be computed off two different lanes.
    match crate::headroom_trade::min_audible_above(currents, floor) {
        Some(min_audible) => (floor as f64) / f64::from(min_audible),
        None => 0.0,
    }
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
///
/// `floor` bounds the solve from BELOW as well ([`joint_k_floor`]). `LEVEL_MIN` for every
/// lane but the headroom trade's base hold, which may not solve the base amp into digital
/// silence just to pay for a `presetLevel` raise.
fn solve_joint_k_at(
    knobs: &[KnobTarget],
    target_lufs: f64,
    measured: f64,
    floor: f32,
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
    let k_floor = joint_k_floor(knobs.iter().map(|kt| kt.current), floor);
    let k_eff = k.min(k_cap).max(k_floor).max(0.0);
    // Clamped in EITHER direction now: out of boost at the top, out of fader at the bottom.
    let clamped = (k_eff - k).abs() > 1e-9;
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
    /// The clamp's cause from the shared taxonomy —
    /// [`crate::headroom_trade::ClampKind::from_flags`]'s answer, so the wet floor of a
    /// user-HANDLE row is reported as [`crate::headroom_trade::ClampKind::WetFloor`] rather
    /// than collapsing into a generic headroom clamp.
    clamp_kind: Option<crate::headroom_trade::ClampKind>,
    /// WHICH bound the joint-k solve pinned at, when one did — the solve's own direction-aware
    /// answer ([`joint_levels_pinned`]), carried so no consumer re-derives it from `levels`.
    /// `None` on every lane that cannot pin one: the already-at-target skip, the user-HANDLE
    /// lane (its bound is the param's own, reported through `clamp_kind`), and any solve no
    /// bound stopped. Read by the headroom trade's base hold ([`trade_hold_failure_kind`]).
    pinned: Option<PinnedBound>,
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

/// Capture budget for the footswitch solve's bracket-aware secant (see
/// `solve_footswitch`). Larger than the scene path's `MEASURE_CORRECT_MAX`
/// because arbitrary block params (a `volume` audio taper, a `drive` knob) can
/// put the target on a steep cliff that needs several Illinois-damped bracket
/// iterates (HW, Hiwatt fs12: ~4 from a 21-LU-wide bracket) — converged solves
/// still exit on the first in-tolerance iterate, so the extra headroom costs
/// well-behaved knobs nothing. Doubled from 8 to 16 alongside the `FS_TOL_LU`
/// tightening (0.3 → 0.1): a tighter acceptance band needs more bracket-aware
/// iterates to walk down onto, and a stalled/no-authority pair still exits early
/// via `flat_response`/the secant's own degenerate-pair break, so the extra
/// headroom again costs well-behaved knobs nothing.
const FS_CORRECT_MAX: u32 = 16;
/// Minimum acceptable `|target_lufs − seed1's measured LUFS|` the law-predicted second
/// seed's IMPLIED separation must clear before it's trusted over the fixed 0.75/0.25-
/// fraction fallback (see `FsParamTarget::seed2_plausible`). Gates in the SAME space the
/// downstream flatness proof (`KNOB_TOL_LU`) reads, not raw knob-value span: the
/// predecessor of this gate, `FS_MIN_SEED_SPAN_FRAC` (0.12 of the param's own v-space
/// range), wrongly REJECTED correct predictions at low knob values — at `v_a = 0.05` a
/// genuine 6 LU correction moves `v` by only `0.0498`, under a 12%-of-`[0,1]` span, so the
/// old gate silently fell back to the fixed seed exactly where the log-knob law matters
/// most (the quiet end of the range, where `v` compresses hardest per dB).
///
/// Why gating on the EXPECTED LU gap is still safe against a false no-authority verdict:
/// for an ACCEPTED prediction the coord-space law makes the pair's expected separation
/// exactly `target_lufs − l_a` (1:1 for `LevelDb`, exact for `LevelLinear`'s log-knob
/// coord), so an accepted pair that nonetheless MEASURES flat (`|l_b − l_a| < KNOB_TOL_LU`
/// despite ≥ 1 LU expected) is precisely `fs_bracket_expansion`'s entry condition
/// (`|l_b − l_a| < KNOB_TOL_LU`) — it fires and probes a bound extreme, so the pair either
/// widens with real slope or the knob is confirmed to have no authority even at its
/// extremes (the correct verdict either way). A FALSE no-authority read would require the
/// law to be off by `≥ 1.0 − KNOB_TOL_LU ≈ 0.7 LU` while the prediction still lands inside
/// the central 5–95% of the range — well outside the law's HW-validated accuracy band.
const FS_MIN_SEED_GAP_LU: f64 = 1.0;
/// An `outputLevel` change of at least this many dB that moves the captured loudness by
/// less than `KNOB_TOL_LU` means the amp has no authority over the USB 1/2 capture
/// (off-branch / off-USB output, or hard-limited downstream).
const NO_AUTHORITY_MIN_DB: f64 = 6.0;
/// Rebalance: if a solo lane is within this many dB of the both-muted floor, the muted
/// lane's bleed corrupts the equal-solo balance → flag the scene "verify by ear".
const REBALANCE_BLEED_MARGIN_DB: f64 = 28.0;
/// Sentinel loudness for a both-lanes-muted capture that reads as digital silence — the
/// IDEAL mute (no bleed). `processed_loudness` errors on silence; this stands in so the
/// solo-above-floor margin is huge (→ no verify-by-ear flag) instead of failing the scene.
const MUTE_FLOOR_SILENT_LUFS: f64 = -120.0;

/// A silent capture PAST THE FIRST SEED is DATA (the knob's quiet extreme), not a routing
/// failure — `solve_footswitch` converts it into a pseudo point at this LUFS for the
/// second-seed / bracket-expansion-probe / correction-loop's internal GEOMETRY only (never
/// handed to `improve_best`, and the correction loop's at-target break is additionally
/// gated on the point being REAL — see both call sites — so this sentinel can never become
/// the reported "achieved" loudness). Deliberately NOT the true digital-silence floor
/// (`MUTE_FLOOR_SILENT_LUFS`, −120 — still used for the FIRST seed's routing-clamp report):
/// pinning the geometry to the literal floor forced the bounded Illinois-damped correction
/// loop (`FS_CORRECT_MAX` = 8 iterates) to spend its ENTIRE budget walking the bracket back
/// down from a 90+ LU gap, landing ONE iterate short of converging on the reported repro
/// (HW, fw 1.8.45, preset "TR+BD2+BMP": Plumes `level` knob, target −26 LUFS, mono-era
/// convention — see the PR2 metering re-baseline in `notes/leveling.md`). −50 sits 21 LU
/// below the quietest user-settable target (`TargetRow` TMIN = −29) — well clear of any real
/// target, yet close enough that the damping converges with iterates to spare (a sentinel
/// sensitivity sweep found −62..−70 and −98..−100 to be non-converging "dead zones" for this
/// fixture's geometry — avoid those bands if this value ever needs to move).
const FS_SILENT_GEOMETRY_LUFS: f64 = -50.0;

/// Fixed margin `fs_silent_geometry` keeps the silent-capture pseudo point BELOW the
/// quietest real capture of the solve.
const FS_SILENT_MARGIN_LU: f64 = 10.0;

/// The pseudo-LUFS for a silent post-first-seed capture: `FS_SILENT_GEOMETRY_LUFS`, floored
/// `FS_SILENT_MARGIN_LU` below the quietest REAL capture the solve has seen — a fixed
/// sentinel sitting ABOVE a real point would invert the secant pair's slope and walk the
/// solve the wrong way (real chains do measure below −50: the Plumes fixture's own bottom
/// anchor is −66.9 LUFS, and a whole-curve-quiet chain puts EVERY capture under the fixed
/// value).
fn fs_silent_geometry(min_real_lufs: f64) -> f64 {
    FS_SILENT_GEOMETRY_LUFS.min(min_real_lufs - FS_SILENT_MARGIN_LU)
}

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

/// What every per-scene solve opens with: the scene's as-is reading.
struct Prologue {
    /// AS-IS loudness at the scene's authored knob values — the solve's starting point.
    asis: f64,
    /// Dynamics spread (LU) of that same capture.
    spread: f64,
}

/// The prologue BOTH per-scene lanes share (`jointk_one_scene` on the amp's `outputLevel`,
/// `handle_one_scene` on the user's own control): ONE isolated fresh re-amp capture of the
/// scene as-is — no write, no Scene Edit, nothing to undo.
///
/// What is deliberately NOT here is the already-at-target short-circuit, because the two
/// lanes' skip TESTS genuinely differ (same `KNOB_TOL_LU` band, different clamp input):
/// joint-k asks `scene_at_target(asis, target, clamped)` with the clamp flag from
/// `solve_joint_k_at` — which does not exist until after its own solver runs, and which
/// deliberately forces a clamped scene to fall through and be REPORTED even when the as-is
/// reading happens to sit on target. The handle lane has no closed-form solve and therefore
/// no clamp flag to pass (`false`). Each lane keeps its own one-line test and builds the
/// skip outcome through [`Prologue::already_there`], so only the test differs, not the shape.
///
/// REORDERED RUN: when the job carries a [`ScenePrepass`] the reading is CONSUMED rather than
/// taken — the batch already paid this capture up front so it could plan the headroom trade
/// against every ceiling at once. A job with no prepass measures here exactly as before, so
/// the rebalance/redistribution/bench callers are untouched.
///
/// `intended_preset_level` rides through to [`measure_scene_asis`] — the run's own solved or
/// UNSAVED-held `presetLevel`, re-asserted after the scene recall so the reading describes
/// the level the run is actually working at rather than the one the device has saved. It has
/// no effect on a CONSUMED prepass reading (that capture was already taken).
fn scene_prologue(
    job: &SceneJob,
    stimulus: &[f32],
    intended_preset_level: Option<f32>,
) -> Result<Prologue, String> {
    // Hard-error on a persistent flat read (after the retry). Trade-off, made
    // consciously: a real scene crushed by a limiter (the UA1176 case) with spread ≤ the
    // trip gate would false-error — but the library's Base minimum is 0.12 and without the
    // guard a floor read lands on the no-authority clamp path, which mislabels a USB
    // failure as an off-branch amp.
    let (asis, spread) = match job.prepass {
        Some(p) => (p.asis, p.spread),
        None => {
            let loudness = require_live(
                || {
                    measure_scene_asis(
                        job.scene_slot,
                        stimulus,
                        intended_preset_level,
                        &job.force_bypass,
                    )
                },
                stimulus,
            )?;
            (loudness.integrated_lufs, loudness.spread_lu())
        }
    };
    Ok(Prologue { asis, spread })
}

impl Prologue {
    /// The scene is already at its target: report the as-is reading and leave every knob
    /// untouched (`writes: 0`). `levels` is what the device already holds — each knob's
    /// authored `current`.
    fn already_there(&self, levels: Vec<f32>) -> SceneSolve {
        SceneSolve {
            lufs: self.asis,
            levels,
            clamped: false,
            clamp_kind: None,
            pinned: None,
            spread: self.spread,
            writes: 0,
            clamp_reason: None,
            verify_by_ear: false,
        }
    }
}

/// Per-scene joint-k: measure the scene AS-IS once, solve one factor `k`, apply it to every
/// lane amp (preserving their mix), VERIFY, then `correct_iter` (bounded secant) to converge
/// through a downstream compressor. The open-loop `20·log10(k)` model is exact for pure gain
/// (±0.07 LU) but UNDERSHOOTS through a compressor/limiter (preset 027's UA1176 → −22.93 vs
/// −22). On a linear chain the first verify is within tol and no correction runs. Shared by
/// `level_scenes_oneshot` and the rebalance flow's non-mergeable scenes. `verify=false` skips
/// both verify and correction.
#[allow(clippy::too_many_arguments)]
fn jointk_one_scene(
    slot: u32,
    job: &SceneJob,
    stimulus: &[f32],
    target_lufs: f64,
    defer: bool,
    verify: bool,
    saved: Option<&serde_json::Value>,
    // The knob-set's DOWNWARD bound (see [`joint_k_floor`]): `LEVEL_MIN` for every scene lane,
    // [`crate::headroom_trade::BASE_FADER_FLOOR`] for the headroom trade's base hold, which
    // may not solve the base amp into digital silence.
    floor: f32,
    // The run's own `presetLevel` — its solved value, or the UNSAVED raise a headroom trade is
    // holding — re-asserted on the as-is capture below (`measure_scene_asis`). `None` = assert
    // nothing. STALE CLAIM CORRECTED (2026-08-29): `apply_first_verified`/`correct_iter` do NOT
    // route through `capture_on_session` (that seam is Doctor's + the isolated-block-knob
    // paths) — they capture through `apply_levels`' own `engage_measure_disengage`, and DO
    // thread `intended_preset_level` through it (`LevelOptions::intended_preset_level`, set
    // below). What is NOT threaded into those two is `force_bypass` from a caller-supplied
    // literal here — they take it from `job.force_bypass` instead (A4: base's isolation lives
    // on the job, not a parallel parameter chain).
    intended_preset_level: Option<f32>,
) -> Result<SceneSolve, String> {
    let prologue = scene_prologue(job, stimulus, intended_preset_level)?;
    let (measured, spread) = (prologue.asis, prologue.spread);
    let JointK {
        levels,
        clamped,
        achieved,
        k_eff,
    } = solve_joint_k_at(&job.knobs, target_lufs, measured, floor)?;
    // Already at target (within the KNOB_TOL_LU acceptance band) and not clamped →
    // leave every knob untouched (a clamp must still be REPORTED even if nothing moves,
    // which is why this lane's test takes the joint-k clamp flag and the handle lane's
    // cannot).
    if scene_at_target(measured, target_lufs, clamped) {
        return Ok(prologue.already_there(job.knobs.iter().map(|kt| kt.current).collect()));
    }
    // Scene writes are NEVER saved per apply: `defer` accumulates them unsaved in the
    // working copy (the runner saves ONCE at batch end); `!defer` is the dry-run shape
    // (each apply restores). See `save_deferred_scene_writes`.
    let opts = LevelOptions {
        verify,
        defer,
        intended_preset_level,
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
        saved,
        &job.force_bypass,
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
                saved,
                floor,
                intended_preset_level,
                &job.force_bypass,
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
    let (pinned, clamp_kind) = joint_clamp_report(
        &best_levels,
        &base,
        floor,
        best_lufs,
        target_lufs,
        clamped,
        clamp_reason.as_deref(),
    );
    Ok(SceneSolve {
        lufs: best_lufs,
        levels: best_levels,
        clamped,
        clamp_kind,
        pinned,
        spread,
        writes,
        clamp_reason,
        verify_by_ear: false,
    })
}

/// Did the joint-k solve actually PIN at the bound that blocks the direction the target needs?
/// The scene lane's mirror of [`classify_fs_outcome`]'s direction-aware test, and the gate on
/// stamping [`crate::headroom_trade::ClampKind::SceneCeiling`].
///
/// DIRECTION MATTERS, both ends:
/// * a lane at `LEVEL_MAX` with the target still LOUDER → out of headroom, a real ceiling;
/// * a lane at `floor` with the target still QUIETER → out of fader (the base hold's
///   [`crate::headroom_trade::BASE_FADER_FLOOR`] case);
/// * anything else — including a lane sitting AT a bound whose miss points back INTO its range
///   — is the search stopping early, not the sound being unable to get there.
///
/// `base` is the pre-solve value set, so a lane the author already parked at a bound (and the
/// solve never moved) is not read as the solve pinning there.
fn joint_levels_pinned(
    levels: &[f32],
    base: &[f32],
    floor: f32,
    achieved: f64,
    target: f64,
) -> Option<PinnedBound> {
    let moved_or_not_authored = |i: usize, v: f32| base.get(i).is_none_or(|b| (b - v).abs() > 1e-4);
    levels.iter().enumerate().find_map(|(i, &v)| {
        if v >= LEVEL_MAX - 1e-3 && target > achieved {
            Some(PinnedBound::Max)
        } else if v <= floor + 1e-6 && target < achieved && moved_or_not_authored(i, v) {
            Some(PinnedBound::Floor)
        } else {
            None
        }
    })
}

/// WHICH bound a joint-k solve pinned at. The two are mutually exclusive by construction —
/// they gate on opposite signs of `target − achieved` — so one answer per solve.
///
/// Carried on [`SceneSolve`] rather than re-derived downstream: the headroom trade's base hold
/// is the one consumer that must tell "the base fader genuinely ran out" apart from "the
/// bounded secant stalled mid-range", and a levels scan cannot (an author-MUTED lane sits at
/// the floor in every solve of that preset). See [`trade_hold_failure_kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinnedBound {
    /// A lane is at [`LEVEL_MAX`] and the target is still LOUDER — out of headroom.
    Max,
    /// A lane the solve MOVED is at `floor` and the target is still QUIETER — out of fader.
    Floor,
}

/// The joint-k lanes' whole clamp REPORT in one place: which bound the solve pinned at, and
/// the taxonomy cause that follows. Both joint-k producers ([`jointk_one_scene`] and
/// [`rebalance_one_scene`]) need exactly this pair, and forking the four lines twice is how
/// the two lanes drift about when a cause may be named at all.
///
/// ⟦8⟧ Only name a cause when a bound ACTUALLY pinned the solve (or a routing failure fired).
/// An amp-`outputLevel` lane has no wet floor, so the only causes here are routing and the
/// ordinary headroom clamp — but a row that merely ran out of secant captures MID-RANGE has
/// neither. Telling the user "its level control is already maxed out" about a fader sitting
/// at 0.4 is a false cause: a re-run can improve that row. `clamp_reason` is deliberately NOT
/// filled in instead — `.claude/rules/leveling-dsp.md` pins that field to "no signal on
/// USB 1/2" and the UI maps ANY non-null reason to the off-branch outcome.
fn joint_clamp_report(
    levels: &[f32],
    base: &[f32],
    floor: f32,
    achieved: f64,
    target: f64,
    clamped: bool,
    clamp_reason: Option<&str>,
) -> (
    Option<PinnedBound>,
    Option<crate::headroom_trade::ClampKind>,
) {
    let pinned = joint_levels_pinned(levels, base, floor, achieved, target);
    let kind = if clamp_reason.is_some() || pinned.is_some() {
        crate::headroom_trade::ClampKind::from_flags(clamped, false, clamp_reason)
    } else {
        None
    };
    (pinned, kind)
}

/// Correction budget for the scene HANDLE lane's param secant — half [`FS_CORRECT_MAX`].
/// Each iterate is a fresh-connect re-amp capture (~10 s) and a scene batch pays it up to 9
/// times over, so the footswitch lane's 16 would put a whole-preset handle run past 20
/// minutes; 8 still covers the HW-observed worst case (the Hiwatt `volume` cliff converged
/// in ~4 Illinois-damped bracket iterates), and a well-behaved knob exits on its first
/// in-tolerance iterate either way. A row that runs out reports honestly — it lands off
/// target and `handle_one_scene` marks it clamped against the scene lane's own
/// [`KNOB_TOL_LU`] band, never silently as done.
const SCENE_HANDLE_CORRECT_MAX: u32 = 8;

/// One scene solved on the USER'S OWN handle instead of the amp's `outputLevel`.
///
/// SEAM CHOICE (`solve_param_secant`, i.e. the footswitch solve's loop, over the joint-k
/// one): joint-k is not a search at all — it is the closed-form `k = 10^((target−measured)/20)`
/// that only holds for an AMPLITUDE knob multiplying the whole summed output, and
/// `solve_joint_k_at` hard-errors on anything outside `0..1` for exactly that reason. A
/// user-chosen handle is an arbitrary control (a raw-dB `gain`, a wet `mix`, a pedal taper),
/// so the lane needs the generic param-space search that already carries the class gate,
/// the range-driven bounds, the wet floor, silence-as-data and the Illinois-damped bracket
/// — that is the footswitch solve, and duplicating it would fork every one of those HW
/// lessons. Only the capture budget differs ([`SCENE_HANDLE_CORRECT_MAX`]).
///
/// The measure closure is [`measure_knob_at`] with the knob's SCENE context, so every
/// probe writes through the Scene-Edit-aware `set_knobs`: its overlay gates apply unchanged
/// — a `BypassOnly` scene (knobs shared with base) refuses on the FIRST capture and the row
/// becomes a per-scene failed outcome, never a write that leaks to base.
///
/// AS-IS FIRST, exactly like `jointk_one_scene`: one `measure_scene_asis` capture (no write
/// at all) supplies the idempotency skip, and the solve then seeds from the param's own
/// range. That is why `current_value` is `None` below — the solve's own idempotency probe
/// would re-measure this very point.
///
/// The write is DEFERRED like every other scene write (`defer`), so the batch's single
/// `save_deferred_scene_writes` persists it: ONE save per preset, unchanged.
#[allow(clippy::too_many_arguments)]
fn handle_one_scene(
    slot: u32,
    job: &SceneJob,
    handle: &FsParamTarget,
    stimulus: &[f32],
    target_lufs: f64,
    defer: bool,
    saved: Option<&serde_json::Value>,
    // The run's own `presetLevel` (see `jointk_one_scene`'s parameter of the same name),
    // asserted on the as-is capture AND on every solve capture — each `measure_knob_at` here
    // recalls the knob's scene, which reverts the level exactly like the as-is recall does.
    intended_preset_level: Option<f32>,
) -> Result<SceneSolve, String> {
    // CLASS GATE, before any device work — the same refusal wording every lane shares.
    if let Some(refusal) = handle.refuse_if_not_a_level_control() {
        return Err(refusal);
    }
    let kt = match job.knobs.as_slice() {
        [one] => one,
        n => {
            return Err(format!(
                "a user-chosen scene handle addresses exactly one control, got {}",
                n.len()
            ))
        }
    };
    let prologue = scene_prologue(job, stimulus, intended_preset_level)?;
    let (asis, spread) = (prologue.asis, prologue.spread);
    // Already there → leave the handle untouched (the scene lane's `scene_at_target` rule,
    // on its own acceptance band). No closed-form solve runs here, so there is no clamp flag
    // to feed it — unlike `jointk_one_scene`, whose flag exists by this point.
    if scene_at_target(asis, target_lufs, false) {
        return Ok(prologue.already_there(vec![kt.current]));
    }
    let knob = &kt.knob;
    let solved = solve_param_secant(
        stimulus,
        target_lufs,
        None,
        handle,
        SCENE_HANDLE_CORRECT_MAX,
        |v| {
            measure_knob_at(
                stimulus,
                knob,
                v,
                &job.force_bypass,
                saved,
                intended_preset_level,
            )
        },
    )?;
    // The sweep left the LAST probed value in the working copy, not necessarily the best
    // one — write the solved value explicitly (unsaved under `defer`; the batch-end save
    // persists it).
    let opts = LevelOptions {
        verify: false,
        defer,
        ..Default::default()
    };
    apply_levels(
        slot,
        stimulus,
        &[(knob, solved.final_value)],
        opts,
        false,
        saved,
        // A4: "base means base" applies to a handle-driven base row too — the user's own
        // control is still measured/written with base's isolation asserted.
        &job.force_bypass,
    )?;
    // Reported on the SCENE lane's band (`KNOB_TOL_LU`), not the tighter footswitch one the
    // search accepts on: a handle row and an amp row in the same batch must mean the same
    // thing by "done". A routing clamp (`clamp_reason`) rides through verbatim.
    let clamped = solved.clamp_reason.is_some()
        || solved.clamped
        || (solved.predicted_lufs - target_lufs).abs() > KNOB_TOL_LU;
    Ok(SceneSolve {
        lufs: solved.predicted_lufs,
        levels: vec![solved.final_value],
        clamped,
        // The handle lane CAN wet-floor: the user's control may be a mix knob, and the floor
        // that stopped the solve is the wet-preservation one, not the chain's ceiling.
        clamp_kind: crate::headroom_trade::ClampKind::from_flags(
            clamped,
            solved.wet_floor,
            solved.clamp_reason.as_deref(),
        ),
        // No joint-k bound here — the handle's own bound is what stopped it, and the trade
        // never holds base on a user handle (`plan_trade_for_batch` refuses that batch).
        pinned: None,
        spread,
        // The solve's own captures plus this final apply write.
        writes: solved.iterations + 1,
        clamp_reason: solved.clamp_reason,
        verify_by_ear: false,
    })
}

/// A ≥ this intended `outputLevel` move (dB) that reads back ~unchanged (< `KNOB_TOL_LU`)
/// is a suspected DROPPED WRITE, not compression: even 6:1 compression passes ~0.33 LU of a
/// 2 dB move. Below it a flat response is ambiguous with noise, so no retry fires.
const SUSPECT_DROP_MIN_DB: f64 = 2.0;

/// One `set_knobs` batch's targets: each knob paired with its level. (Superseded
/// `merge_repair_targets`, which also appended per-scene writes re-asserting params the
/// Scene Edit reseed would wipe — dead, and HW-proven not to land (8/8 failed): `set_knobs`
/// now enables Scene Edit ONLY where the node has no overlay, the one case where the reseed
/// has nothing to lose, so there is nothing left to repair.)
fn zip_targets<'a>(knobs: &[&'a LevelKnob], levels: &[f32]) -> Vec<(&'a LevelKnob, f32)> {
    knobs
        .iter()
        .copied()
        .zip(levels)
        .map(|(k, &v)| (k, v))
        .collect()
}

/// First verified apply with a ONE-SHOT dropped-write retry (scene paths). The device can
/// silently drop a scene write (the ~700 ms post-`loadScene` acceptance window, HW
/// `probe --bisect-scene`); without the retry a single drop reads as a flat response →
/// `correct_iter` sees no slope → a false, non-deterministic "clamped at <as-is>" (HW: the
/// user's first-run Arpeges clamp that succeeded on re-run). One re-apply — a fresh scene
/// recall + write + verify — disambiguates: a drop lands on the retry; a genuine
/// no-authority amp stays flat and takes the honest clamp downstream. Returns
/// `(verify_lufs, retry_writes)`.
#[allow(clippy::too_many_arguments)]
fn apply_first_verified(
    slot: u32,
    stimulus: &[f32],
    knobs: &[&LevelKnob],
    levels: &[f32],
    opts: LevelOptions,
    expected_db: f64,
    baseline_lufs: f64,
    saved: Option<&serde_json::Value>,
    // A4: the job's isolation list — empty for every scene job, the base row's own list when
    // this apply is base's (both a plain base solve and the headroom trade's hold).
    force_bypass: &[(String, String, bool)],
) -> Result<(Option<f64>, u32), String> {
    let targets = zip_targets(knobs, levels);
    let v0 = apply_levels(slot, stimulus, &targets, opts, false, saved, force_bypass)?.1;
    match v0 {
        Some(v)
            if opts.verify
                && expected_db.abs() >= SUSPECT_DROP_MIN_DB
                && (v - baseline_lufs).abs() < KNOB_TOL_LU =>
        {
            crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
            Ok((
                apply_levels(slot, stimulus, &targets, opts, false, saved, force_bypass)?.1,
                1,
            ))
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
    saved: Option<&serde_json::Value>,
    // The same downward bound the open-loop solve took (`joint_k_floor`): a correction step
    // must not walk the quietest lane past the floor that solve refused to cross. `LEVEL_MIN`
    // for every caller but the headroom trade's base hold.
    floor: f32,
    // The run's own `presetLevel`, re-asserted on every corrective capture — see
    // [`LevelOptions::intended_preset_level`]. Each iterate re-applies through
    // `apply_levels`, whose `set_knobs` recalls the scene and reverts an unsaved level, so
    // omitting it here reverts the whole loop to measuring the SAVED level.
    intended_preset_level: Option<f32>,
    // A4: the job's isolation list, forwarded to every `apply` inside the loop below — same
    // contract as `apply_first_verified`'s.
    force_bypass: &[(String, String, bool)],
) -> Result<Correction, String> {
    let max_base = base
        .iter()
        .map(|&x| x.clamp(1e-3, 1.0) as f64)
        .fold(0.0_f64, f64::max);
    let k_cap = (LEVEL_MAX as f64) / max_base;
    let k_floor = joint_k_floor(base.iter().copied(), floor).max(1e-3);
    let levels_for = |applied_db: f64| -> Vec<f32> {
        let k = 10f64.powf(applied_db / 20.0).clamp(k_floor, k_cap);
        base.iter()
            .map(|&b| (b as f64 * k).clamp(LEVEL_MIN as f64, LEVEL_MAX as f64) as f32)
            .collect()
    };
    let apply = |levels: &[f32], verify: bool| -> Result<Option<f64>, String> {
        let opts = LevelOptions {
            verify,
            defer,
            intended_preset_level,
            ..Default::default()
        };
        let targets = zip_targets(knobs, levels);
        Ok(apply_levels(slot, stimulus, &targets, opts, false, saved, force_bypass)?.1)
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

    // CONFIRMATION PROBE — the verdict above needs a move of at least `NO_AUTHORITY_MIN_DB`
    // to be conclusive, but the first step is sized by the SOLVE, not by what a verdict
    // needs: a scene sitting 4 dB from its target gets a 4 dB step. A FLAT response to that
    // is already suspicious, and left unresolved it falls through the secant (slope ~0 →
    // stop) and reports a REASON-LESS headroom clamp on a knob that in fact has no authority
    // at all — the user is told "couldn't get there" instead of "this amp doesn't reach USB
    // 1/2", which is the one thing they could act on. So resolve it: step DOWN by a full
    // `NO_AUTHORITY_MIN_DB` and look again.
    //
    // Downward only, deliberately: a raise can clip, and a saturating limiter makes an upward
    // non-response ambiguous anyway (`no_authority_reason`). Costs ONE extra capture, and only
    // on a sound whose first real move produced nothing — a knob with authority moves ~1 LU
    // per dB, so this never fires on a healthy solve.
    let mut prev = (0.0_f64, measured0); // (applied_db, lufs) — the secant's first seed
    if (v0 - measured0).abs() < KNOB_TOL_LU && applied_db0 > -NO_AUTHORITY_MIN_DB {
        let probe_db = applied_db0 - NO_AUTHORITY_MIN_DB;
        let probe_levels = levels_for(probe_db);
        // Only meaningful if the floor actually lets the knobs travel that far; if it does
        // not, the reading stays inconclusive and the ordinary clamp is the honest answer.
        if probe_levels
            .iter()
            .zip(&levels0)
            .any(|(a, b)| (a - b).abs() > 1e-3)
        {
            let vp = apply(&probe_levels, true)?;
            writes += 1;
            match vp {
                // Conclusive: a full `NO_AUTHORITY_MIN_DB` drop moved nothing.
                Some(vp) if (vp - measured0).abs() < KNOB_TOL_LU => {
                    apply(base, false)?;
                    writes += 1;
                    return Ok(Correction {
                        lufs: measured0,
                        levels: base.to_vec(),
                        clamp_reason: Some(no_authority_reason(true)),
                        writes,
                    });
                }
                // It DID move, so the knob has authority and the flat first reading was the
                // unreliable one. The probe is a genuine second point — seed the secant with
                // it instead of `base@measured0`, and put the device back where the loop
                // below expects to find it.
                Some(vp) => {
                    prev = (probe_db, vp);
                    apply(&levels0, false)?;
                    writes += 1;
                }
                // Capture dropped — no verdict either way; fall through unchanged.
                None => {}
            }
        }
    }

    // Bounded secant. Seed points: `prev` (base, or the probe above) and levels0@v0.
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
/// processed pair on the full capture — the multi-knob `measure_knob_at` used by the
/// rebalance flow to read one lane SOLO (the other muted) and the balanced combination.
fn measure_knobs_at(
    stimulus: &[f32],
    targets: &[(&LevelKnob, f32)],
    saved: Option<&serde_json::Value>,
) -> Result<lufs::Loudness, String> {
    let mut s = Session::connect_lean()?;
    set_knobs(&mut s, targets, saved)?;
    settle_or_cancel(SETTLE_AFTER_SET_MS)?;
    engage_measure_disengage(&mut s, stimulus)
}

/// Measure the both-lanes-muted FLOOR. `outputLevel`=0 is often DEEP silence — the ideal
/// mute — which `processed_loudness` reports as "no signal captured"; treat that as a sentinel
/// deep floor (`MUTE_FLOOR_SILENT_LUFS`), the BEST case (no bleed), rather than failing.
/// Other capture errors still propagate.
fn measure_mute_floor(
    stimulus: &[f32],
    a: &LevelKnob,
    b: &LevelKnob,
    saved: Option<&serde_json::Value>,
) -> Result<f64, String> {
    match measure_knobs_at(stimulus, &[(a, 0.0), (b, 0.0)], saved) {
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
    // Before the load: the field-8 read this diagnostic's scene writes need (see
    // `saved_for_scene_knobs`) — `read_saved_preset` sleeps after itself, so the load
    // session that follows still gets a spaced boundary.
    let saved = saved_for_scene_knobs(slot, [a, b]);
    let saved = saved.as_ref();
    {
        let mut s = Session::connect_lean()?;
        s.load_preset(slot)?;
        crate::settle(Duration::from_millis(settle_after_load_ms()));
    }
    let combined = measure_knobs_at(stimulus, &[(a, cur_a), (b, cur_b)], saved)?;
    crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
    let floor_lufs = measure_mute_floor(stimulus, a, b, saved)?;
    crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
    let a_solo = measure_knobs_at(stimulus, &[(a, cur_a), (b, 0.0)], saved)?;
    crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
    let b_solo = measure_knobs_at(stimulus, &[(a, 0.0), (b, cur_b)], saved)?;
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
/// HW-validated (2026-07-20, preset "Bass Dual Amps", 2-amp merge): `probe --rebalance-scenes`
/// Base @ −26 achieved −25.79 (+0.21, level 0.91); @ −23 honest clamp at −25.16/level 1.0.
/// `probe --mute-floor` confirmed the muting assumption: both lanes muted = digital silence
/// (ideal mute, zero bleed → trustworthy equal-solo balance; lane solos −31.3/−24.6), and
/// `probe --mute-floor` correctly refuses a single-amp split-output preset ("needs a 2-amp
/// merged parallel scene") — the leveller itself falls back to `jointk_one_scene` there.
/// Restores via preset reload on no-save; ends with a guaranteed re-amp OFF.
#[allow(clippy::too_many_arguments)]
pub fn level_scenes_rebalance(
    slot: u32,
    jobs: &[SceneJob],
    stimulus: &[f32],
    save: bool,
    restore_scene: Option<u32>,
    saved: Option<&serde_json::Value>,
    hold: Option<&TradeHold>,
    // A5/F2: `(group, node, ORIGINAL saved bypass)` for a base-requested job's own isolation
    // (empty on every run that never isolated anything) — see `run_scene_jobs`'s param doc.
    isolation_restore: &[(String, String, bool)],
    on_scene: impl FnMut(u32, Option<&BatchedSceneOutcome>),
    // B6: forwarded to `run_scene_jobs` verbatim — see its own doc.
    on_tail: impl FnMut(&str),
    cancelled: impl FnMut() -> bool,
) -> Result<Vec<BatchedSceneOutcome>, String> {
    // Same rule as `level_scenes_oneshot`: a landed trade's raise is UNSAVED, and every
    // per-scene capture recalls its scene (which reverts it), so it is re-asserted per capture.
    let intended_preset_level = hold.map(|h| h.preset_level);
    let result = run_scene_jobs(
        slot,
        jobs,
        save,
        restore_scene,
        hold,
        isolation_restore,
        on_scene,
        on_tail,
        cancelled,
        |job| {
            // Non-mergeable scenes: plain joint-k (nothing to rebalance), self-correcting.
            if !job.rebalanceable || job.knobs.len() < 2 {
                jointk_one_scene(
                    slot,
                    job,
                    stimulus,
                    job.target_lufs,
                    save,
                    true,
                    saved,
                    LEVEL_MIN,
                    intended_preset_level,
                )
            } else {
                // Rebalanceable: 2-lane equalize → joint-k. (Only the first two knobs are the
                // rebalance pair; the classifier never produces >2 for a single split.)
                rebalance_one_scene(
                    slot,
                    job,
                    stimulus,
                    job.target_lufs,
                    save,
                    true,
                    saved,
                    intended_preset_level,
                )
            }
        },
    );
    restore_after_unsaved_error(slot, save, result)
}

/// The rebalance flow for ONE mergeable scene: equalize the two lanes' solo loudness, then
/// joint-k the balanced pair to target. Returns a [`SceneSolve`] like `jointk_one_scene`,
/// plus a `verify_by_ear` flag when the lane-mute floor is too shallow to trust the balance.
#[allow(clippy::too_many_arguments)]
fn rebalance_one_scene(
    slot: u32,
    job: &SceneJob,
    stimulus: &[f32],
    target_lufs: f64,
    defer: bool,
    verify: bool,
    saved: Option<&serde_json::Value>,
    // The run's own `presetLevel`, re-asserted on the corrective captures — see
    // [`LevelOptions::intended_preset_level`]. The solo/combined captures above route
    // through `measure_knobs_at`, which arms its own context; this covers the
    // `correct_iter` tail, which re-applies through `apply_levels`.
    intended_preset_level: Option<f32>,
) -> Result<SceneSolve, String> {
    let a = &job.knobs[0];
    let b = &job.knobs[1];
    let cur_a = a.current.clamp(1e-3, 1.0);
    let cur_b = b.current.clamp(1e-3, 1.0);

    // 1+2. Each lane SOLO (the other muted to 0) at its current level → solo ceiling C.
    // Solo captures feed the per-lane model constants (c_a/c_b) with no verify
    // backstop downstream — floor-guarded like the combined measurement below.
    let la_solo = require_live(
        || measure_knobs_at(stimulus, &[(&a.knob, cur_a), (&b.knob, 0.0)], saved),
        stimulus,
    )?;
    crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
    let lb_solo = require_live(
        || measure_knobs_at(stimulus, &[(&a.knob, 0.0), (&b.knob, cur_b)], saved),
        stimulus,
    )?;
    crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
    let c_a = la_solo.integrated_lufs - 20.0 * (cur_a as f64).log10();
    let c_b = lb_solo.integrated_lufs - 20.0 * (cur_b as f64).log10();

    // 2b. Mute FLOOR: BOTH lanes at 0. `outputLevel`=0 may floor near ~−40 dB (not −∞), so a
    // "solo" carries the muted lane's bleed when the lanes sit within ~`REBALANCE_BLEED_MARGIN_DB`.
    // If so, the equal-solo balance is only approximate (the combined joint-k still hits the
    // overall target) → flag the scene "verify by ear". One extra capture; rebalance is opt-in.
    // A SILENT floor (deep mute) is the best case → huge margin → no flag.
    let floor_lufs = measure_mute_floor(stimulus, &a.knob, &b.knob, saved)?;
    crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
    let min_solo = la_solo.integrated_lufs.min(lb_solo.integrated_lufs);
    let verify_by_ear = (min_solo - floor_lufs) < REBALANCE_BLEED_MARGIN_DB;

    // 3. Balanced levels for equal solo loudness.
    let (la_bal, lb_bal) = balanced_solo_levels(c_a, c_b);

    // 4. Measure the COMBINED output at the balanced levels (correlation-real sum).
    // Floor-guarded: both lanes live at balanced levels must produce a lively capture
    // (the DELIBERATE floor measurement above is measure_mute_floor — never guarded).
    let combined = require_live(
        || measure_knobs_at(stimulus, &[(&a.knob, la_bal), (&b.knob, lb_bal)], saved),
        stimulus,
    )?;
    crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
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
    } = solve_joint_k_at(
        &balanced_knobs,
        target_lufs,
        combined.integrated_lufs,
        LEVEL_MIN,
    )?;

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
        saved,
        &job.force_bypass,
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
                saved,
                LEVEL_MIN,
                intended_preset_level,
                &job.force_bypass,
            )?;
            (c.lufs, c.levels, c.clamp_reason, c.writes)
        }
        _ => (v0.unwrap_or(achieved), levels, None, 0),
    };
    // Clamped from the FINAL point, not the open-loop want (see `jointk_one_scene`): a
    // verify+correct that landed within tolerance means the lane reached target — "done".
    let clamped = clamp_reason.is_some() || (best_lufs - target_lufs).abs() > KNOB_TOL_LU;
    let (pinned, clamp_kind) = joint_clamp_report(
        &best_levels,
        &base,
        LEVEL_MIN,
        best_lufs,
        target_lufs,
        clamped,
        clamp_reason.as_deref(),
    );
    Ok(SceneSolve {
        lufs: best_lufs,
        levels: best_levels,
        clamped,
        clamp_kind,
        pinned,
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
        clamp_kind: s.clamp_kind,
        clamp_reason: s.clamp_reason,
        verify_by_ear: s.verify_by_ear,
        persist_mismatch: None,
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
        // A FAILED row is not a clamped row: it never produced a verdict to name.
        clamp_kind: None,
        clamp_reason: None,
        verify_by_ear: false,
        persist_mismatch: None,
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
        // ONE field-8 read up front for a SCENE knob — every measure and the final apply
        // write through `set_knobs`, whose Scene Edit decision needs it (see
        // `saved_for_scene_knobs`). Read before the load session, never per capture.
        let overlays = saved_for_scene_knobs(slot, [knob]);
        let overlays = overlays.as_ref();
        // Load the preset in its own connection (the set-after-load-in-same-conn
        // override applies to any setter, so isolate the load).
        {
            let mut s = Session::connect_lean()?;
            s.load_preset(slot)?;
            crate::settle(Duration::from_millis(settle_after_load_ms()));
        }
        crate::settle(Duration::from_millis(RECONNECT_GAP_MS));

        // Search in a coordinate where the knob is ~linear in LUFS so the secant
        // converges in 1–2 steps. Amplitude knobs (range within [0,1]) are linear in
        // dB-of-knob (`20·log10(x)` — the de-risk's proven `presetLevel`/`outputLevel`
        // model); dB-unit knobs (e.g. an IR `outputlevel`) are already ~linear, so
        // search them in raw units. Same log-vs-identity shape as `knob_search_space` and
        // `FsParamTarget::to_coord`/`coord_to_value`, but this seam infers log-space from
        // the BOUNDS SHAPE (no `ParamClass` available here) — see `FsParamTarget::to_coord`'s
        // doc for why the three maps stay separate rather than merged.
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
            // Standalone block-knob seam: no run-owned `presetLevel` to assert.
            || measure_knob_at(stimulus, knob, from_c(ca), &[], overlays, None),
            stimulus,
        )?;
        let dynamic_spread_lu = first.spread_lu();
        let mut ya = first.integrated_lufs;
        if cancelled() {
            return Err(CANCELLED.to_string());
        }
        crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
        let mut yb =
            measure_knob_at(stimulus, knob, from_c(cb), &[], overlays, None)?.integrated_lufs;
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
            crate::settle(Duration::from_millis(RECONNECT_GAP_MS));
            let ynext = measure_knob_at(stimulus, knob, from_c(cnext), &[], overlays, None)?
                .integrated_lufs;
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
        let (saved, verify_lufs) = apply_levels(
            slot,
            stimulus,
            &[(knob, final_level)],
            opts,
            false,
            overlays,
            &[],
        )?;

        Ok(LevelResult {
            slot,
            // A block-knob row is not a scene row on the wire — the shipped scene lane is
            // the BATCHED runner (`outcome_to_level_result`), which stamps the real slot.
            scene_slot: None,
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
            clamp_kind: crate::headroom_trade::ClampKind::from_flags(clamped, false, None),
            clamp_reason: None,
            verify_by_ear: false,
            previous_level: None,
            true_peak_dbtp: None,
            persist_mismatch: None,
            trade: None,
        })
    })();
    restore_after_unsaved_error(slot, opts.save, result)
}

#[cfg(test)]
mod persist_verify_tests {
    use super::*;

    // Base graph carrying one amp at outputLevel 0.40, plus a scene 0 overlay that holds
    // 0.72 for the same node — the shape a batch-end save leaves behind.
    fn saved_preset() -> serde_json::Value {
        serde_json::json!({
            "audioGraph": { "guitarNodes": {
                "G1": [ { "nodeId": "amp", "FenderId": "amp",
                          "dspUnitParameters": { "bypass": false, "outputLevel": 0.40 } } ]
            } },
            "scenes": [
                { "guitarNodes": { "G1": { "amp": { "dspUnitParameters": { "outputLevel": 0.72 } } } } },
                { "guitarNodes": { "G1": {} } }
            ]
        })
    }

    fn write(scene_slot: u32, value: f32) -> PersistedWrite {
        PersistedWrite {
            scene_slot,
            node_id: "amp".to_string(),
            parameter_id: "outputLevel".to_string(),
            value,
        }
    }

    // The gate this exists for: a report must never show numbers the save did not persist.
    #[test]
    fn persist_mismatches_flags_only_the_scenes_the_save_did_not_keep() {
        let saved = saved_preset();

        // Solved == persisted, in the scene overlay and at base: nothing to flag.
        assert!(persist_mismatches(&saved, &[write(0, 0.72)])
            .missed
            .is_empty());
        assert!(
            persist_mismatches(&saved, &[write(crate::session::BASE_SCENE_SLOT, 0.40)])
                .missed
                .is_empty()
        );

        // The wipe case: the overlay holds a DIFFERENT value than the run solved.
        let miss = persist_mismatches(&saved, &[write(0, 0.55)]).missed;
        assert_eq!(
            miss.len(),
            1,
            "the divergent scene must be flagged: {miss:?}"
        );
        assert_eq!(miss[0].0, 0);

        // A base write compared against the base graph, same divergence.
        assert_eq!(
            persist_mismatches(&saved, &[write(crate::session::BASE_SCENE_SLOT, 0.55)])
                .missed
                .len(),
            1
        );

        // Scene 1 has no overlay for the node at all — the write is simply not there, which
        // is a miss, not an "unknown" to be waved through.
        let miss = persist_mismatches(&saved, &[write(1, 0.61)]).missed;
        assert_eq!(miss.len(), 1, "an absent overlay is a miss: {miss:?}");
        assert_eq!(miss[0].0, 1);

        // Only the divergent write is reported when a batch mixes both.
        let mixed = persist_mismatches(&saved, &[write(0, 0.72), write(1, 0.61)]).missed;
        assert_eq!(mixed.len(), 1);
        assert_eq!(mixed[0].0, 1);
    }

    /// GATE for the false "did not persist" incident (HW, 2026-08-19, "Friedman HBE"): the
    /// field-8 read of a large preset truncates at ~21044 B, BEFORE `scenes`. The run then
    /// warned that scene 3's `ACD_TwinReverb65NoFx/outputLevel` "solved 0.7814 but the saved
    /// preset holds no such value" — while that same scene's re-measure of the SAVED device
    /// state read -22.99 LUFS against its -23 target, i.e. the write had persisted perfectly.
    /// The external judge duly SKIPped a row that should have passed.
    ///
    /// A document that cannot answer must be reported as UNCONFIRMED, never as a lost write.
    /// This is the second line of defence behind `read_saved_preset_complete`: it holds even
    /// for a document that arrives whole and still carries no `scenes`.
    #[test]
    fn a_truncated_reread_is_unconfirmed_not_a_lost_write() {
        // Exactly what the HBE re-read returned: everything up to the tail, `scenes` gone.
        let mut truncated = saved_preset();
        truncated.as_object_mut().expect("object").remove("scenes");

        let check = persist_mismatches(&truncated, &[write(0, 0.72)]);
        assert!(
            check.missed.is_empty(),
            "a scene the re-read cannot see must NOT be reported as a lost write: {:?}",
            check.missed
        );
        assert_eq!(
            check.unverifiable.len(),
            1,
            "it must be reported as unconfirmed instead: {check:?}"
        );
        assert_eq!(check.unverifiable[0].0, 0);

        // The base write in the SAME document is still fully checkable — `audioGraph` heads
        // the document, so a tail truncation never costs it. Grading it as unverifiable too
        // would throw away a real check.
        let base = persist_mismatches(&truncated, &[write(crate::session::BASE_SCENE_SLOT, 0.55)]);
        assert!(base.unverifiable.is_empty(), "{base:?}");
        assert_eq!(
            base.missed.len(),
            1,
            "base divergence still caught: {base:?}"
        );

        // A scenes array that ARRIVED but is cut short of the slot is the same class: the
        // HBE read returned 3 of 4 scenes, the third cut mid-record.
        let short = persist_mismatches(&saved_preset(), &[write(7, 0.61)]);
        assert!(
            short.missed.is_empty(),
            "a scene slot past the end of a truncated scenes array is unconfirmed: {short:?}"
        );
        assert_eq!(short.unverifiable.len(), 1, "{short:?}");

        // And the guarantee that keeps this from becoming a blanket amnesty: when `scenes`
        // IS present and covers the slot, a genuinely-absent overlay stays a MISS.
        let real = persist_mismatches(&saved_preset(), &[write(1, 0.61)]);
        assert_eq!(
            real.missed.len(),
            1,
            "a covered-but-empty overlay is still a lost write: {real:?}"
        );
        assert!(real.unverifiable.is_empty(), "{real:?}");
    }

    /// GATE for the scene half of the stale-`presetLevel` fix. Unlike the FS half — which the
    /// e2e harness can drive end-to-end against SimDevice's lazy-commit model — this decision
    /// is not observable offline: the sim only reverts a recall's level for a slot the run has
    /// already saved, and the scene specs level slots they never saved first. So the CHOICE is
    /// gated here directly; the mechanism it feeds is proven by the FS twin
    /// (`a_capture_renders_at_the_saved_preset_level_not_the_stale_committed_one`).
    ///
    /// Reverting this to `hold.map(|h| h.preset_level)` — its shape before 2026-08-19 — leaves
    /// the whole offline suite green, which is exactly why it needs a gate of its own.
    #[test]
    fn a_scene_batch_captures_at_the_saved_level_when_no_trade_holds_one() {
        let saved = serde_json::json!({ "audioGraph": { "presetLevel": 0.51009 } });

        // No trade: the preset's OWN saved level, never `None`. `None` is the bug — it lets
        // the recall's level-apply serve the lazily-committed (stale) value instead.
        assert_eq!(
            scene_capture_level(None, Some(&saved)),
            Some(0.51009),
            "with no trade the captures must render at the preset's saved level"
        );

        // A landed trade holds a RAISED level unsaved in the working copy until the batch's
        // one save, so it outranks the saved doc — which still shows the pre-raise value.
        let hold = TradeHold {
            preset_level: 0.72,
            writes: Vec::new(),
            force_bypass_restore: Vec::new(),
        };
        assert_eq!(
            scene_capture_level(Some(&hold), Some(&saved)),
            Some(0.72),
            "a trade's unsaved raise wins over the saved doc"
        );

        // Nothing to assert only when there is genuinely nothing to assert.
        assert_eq!(scene_capture_level(None, None), None);
        assert_eq!(
            scene_capture_level(None, Some(&serde_json::json!({"audioGraph": {}}))),
            None,
            "a doc with no presetLevel asserts nothing rather than inventing a level"
        );
    }

    /// The FS-lane half of the same gate. An Assign write is read out of `ftsw` and a Bake
    /// write out of `audioGraph`; either section can be missing from a truncated re-read, and
    /// a missing SECTION must never be graded as a missing VALUE.
    #[test]
    fn a_truncated_reread_leaves_fs_verdicts_unknown_rather_than_lost() {
        // Base graph + an ftsw table whose switch 0 assigns the amp's outputLevel to 0.72.
        let full = serde_json::json!({
            "audioGraph": { "presetLevel": 0.5, "guitarNodes": {
                "G1": [ { "nodeId": "amp", "FenderId": "amp",
                          "dspUnitParameters": { "bypass": false, "outputLevel": 0.40 } } ]
            } },
            "ftsw": [ [ { "func": "param", "groupId": "G1", "nodeId": "amp",
                          "parameterId": "outputLevel", "valueA": 0.72, "valueType": 1 } ] ]
        });
        let bake = (
            0usize,
            "amp".to_string(),
            "outputLevel".to_string(),
            0.40,
            false,
        );
        let assign = (
            1usize,
            "amp".to_string(),
            "outputLevel".to_string(),
            0.72,
            true,
        );
        let writes = [bake.clone(), assign.clone()];

        // Control: the whole document is there, so both writes are gradable and both kept.
        assert_eq!(
            fs_persist_verdicts(&full, &writes, Some(0.5)),
            vec![(0, Some(false)), (1, Some(false))],
        );

        // `ftsw` gone (the tail a truncated read loses): the ASSIGN becomes unknown, while
        // the bake — read from the head of the document — is still fully checked.
        let mut no_ftsw = full.clone();
        no_ftsw.as_object_mut().expect("object").remove("ftsw");
        assert_eq!(
            fs_persist_verdicts(&no_ftsw, &writes, Some(0.5)),
            vec![(0, Some(false)), (1, None)],
            "an unreadable ftsw must not condemn the assign, nor cost the bake its check"
        );

        // `audioGraph` gone: the base-revert arm cannot run either, so the whole batch is
        // unknown rather than every switch being condemned by a `presetLevel` that is merely
        // unreadable.
        let mut no_graph = full.clone();
        no_graph
            .as_object_mut()
            .expect("object")
            .remove("audioGraph");
        assert_eq!(
            fs_persist_verdicts(&no_graph, &writes, Some(0.5)),
            vec![(0, None), (1, None)],
        );

        // Not an amnesty: a document that DOES carry both sections still reports real losses.
        let lost = [
            (
                0usize,
                "amp".to_string(),
                "outputLevel".to_string(),
                0.90,
                false,
            ),
            (
                1usize,
                "amp".to_string(),
                "outputLevel".to_string(),
                0.90,
                true,
            ),
        ];
        assert_eq!(
            fs_persist_verdicts(&full, &lost, Some(0.5)),
            vec![(0, Some(true)), (1, Some(true))],
        );
        // …and a base save that reverted still condemns the batch.
        assert_eq!(
            fs_persist_verdicts(&full, &writes, Some(0.9)),
            vec![(0, Some(true)), (1, Some(true))],
        );
    }
}

#[cfg(test)]
mod fresh_load_registry_tests {
    use super::*;

    // No registry entry for the slot ⇒ zero-cost fast path: `ensure_fresh_load` must return
    // `Ok` WITHOUT ever attempting a real device connect. Proven indirectly: this test binary
    // has no real device, so `Session::connect()` inside the barrier would return `Err` (a
    // failed HID open) and propagate through `?` — an `Ok` here is only possible if that
    // branch was never reached.
    #[test]
    fn ensure_fresh_load_is_a_no_op_with_no_registry_entry() {
        let slot = 900_001;
        assert!(
            SLOT_SAVE_REGISTRY.lock().unwrap().get(&slot).is_none(),
            "test fixture invariant: slot must start unregistered"
        );
        let result = ensure_fresh_load(slot, &mut || false);
        assert!(
            result.is_ok(),
            "no registry entry must be a no-op, not attempt a real device connect: {result:?}"
        );
    }

    // A registered save older than `COMMIT_WINDOW_SECS` is the SAME zero-cost fast path — the
    // commit is assumed done, so `ensure_fresh_load` must not open a session either. Directly
    // pokes `SLOT_SAVE_REGISTRY` (same module) to backdate the entry without a real wait.
    #[test]
    fn ensure_fresh_load_is_a_no_op_once_the_commit_window_has_elapsed() {
        let slot = 900_002;
        {
            let mut reg = SLOT_SAVE_REGISTRY.lock().unwrap();
            reg.insert(
                slot,
                SlotSave {
                    at: std::time::Instant::now() - Duration::from_secs(COMMIT_WINDOW_SECS + 1),
                    witness: SaveWitness::PresetLevel(0.5),
                },
            );
        }
        let result = ensure_fresh_load(slot, &mut || false);
        assert!(
            result.is_ok(),
            "an elapsed commit window must proceed, not attempt a real device connect: {result:?}"
        );
    }

    // The harvest loop itself (witness match → first-pass Ok, stale → retry → pass,
    // cancellation mid-wait, time-gate) is covered end-to-end against the SimDevice
    // lazy-commit model in `e2e_server_tests`' `fresh_load_barrier_*` tests — they need
    // `Session::connect()` routed to the sim (the e2e transport factory), which only that
    // module's serial harness owns.

    // The incident's own numbers (base saved 0.4377, the stale pre-save materialization read
    // back ≈0.798): the comparator is a DUMB value compare with no staleness detection of its
    // own — given a stale/pre-save doc, it must report a MISMATCH against the freshly
    // registered witness, never a false match. The compare's only real protection is upstream
    // session hygiene (`ensure_fresh_load` clears `raw` before every harvest and never issues
    // a field-8 read); `session::best_json_payload_from_reports_can_prefer_a_stale_field9_reply`
    // proves that pollution hazard at the wire layer this reads from.
    #[test]
    fn witness_compare_can_fail_on_a_stale_pre_save_value() {
        let stale_doc = serde_json::json!({ "audioGraph": { "presetLevel": 0.798 } });
        let registered = SaveWitness::PresetLevel(0.4377);
        let got = witness_value_in_doc(&stale_doc, &registered).expect("presetLevel present");
        assert!(
            (got - witness_expected(&registered)).abs() > WITNESS_EPS,
            "a stale doc must NOT compare equal to the freshly-registered witness"
        );
    }

    #[test]
    fn witness_value_in_doc_reads_preset_level() {
        let doc = serde_json::json!({ "audioGraph": { "presetLevel": 0.6543 } });
        let got = witness_value_in_doc(&doc, &SaveWitness::PresetLevel(0.6543));
        assert_eq!(got, Some(0.6543));
    }

    #[test]
    fn witness_value_in_doc_reads_a_baked_dsp_param() {
        let doc = serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "nodeId": "amp", "FenderId": "amp",
                  "dspUnitParameters": { "drive": 0.42 } }
            ] } }
        });
        let w = SaveWitness::Param {
            node: "amp".into(),
            param: "drive".into(),
            value: 0.42,
            scene: None,
        };
        assert_eq!(witness_value_in_doc(&doc, &w), Some(0.42));
    }

    // The Assign shape: the witness value lives in `ftsw`'s `valueA`, NOT
    // `dspUnitParameters` — an Assign never touches the block's own live param value, so
    // comparing against `dspUnitParameters` there would compare against the switch-OFF
    // value and could never match.
    #[test]
    fn witness_value_in_doc_falls_back_to_ftsw_value_a_for_an_assign() {
        let doc = serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "nodeId": "amp", "FenderId": "amp",
                  "dspUnitParameters": { "drive": 0.10 } }
            ] } },
            "ftsw": [[
                { "func": "param", "nodeId": "amp", "parameterId": "drive", "valueA": 0.77 }
            ]]
        });
        let w = SaveWitness::Param {
            node: "amp".into(),
            param: "drive".into(),
            value: 0.77,
            scene: None,
        };
        assert_eq!(witness_value_in_doc(&doc, &w), Some(0.77));
    }

    // ─── Scene-indexed witness (Fix 3) — overlay-match ONLY, no fallback candidates ───
    //
    // Every fixture below carries the node in the BASE `audioGraph` too (with a distinct
    // `outputLevel`, deliberately equal to the SCENE witness's expected value in the
    // negative cases) so a bug that falls through to the base/ftsw candidates — forbidden
    // by post-review amendment 2 — would read as a false accept, not a false reject.

    fn scene_witness_doc(overlay_key: &str, overlay: serde_json::Value) -> serde_json::Value {
        let mut group = serde_json::Map::new();
        group.insert(overlay_key.to_string(), overlay);
        serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "nodeId": "n1", "FenderId": "ACD_Amp",
                  "dspUnitParameters": { "outputLevel": 0.81 } }
            ] } },
            "scenes": [
                { "guitarNodes": { "G1": serde_json::Value::Object(group) } }
            ]
        })
    }

    fn scene_witness(value: f32) -> SaveWitness {
        SaveWitness::Param {
            node: "n1".into(),
            param: "outputLevel".into(),
            value,
            scene: Some(0),
        }
    }

    #[test]
    fn witness_value_in_doc_matches_a_fender_id_keyed_scene_overlay() {
        let doc = scene_witness_doc(
            "ACD_Amp", // FenderId-keyed — `scene_overlay_for`'s first lookup order
            serde_json::json!({ "dspUnitParameters": { "outputLevel": 0.81 } }),
        );
        assert_eq!(witness_value_in_doc(&doc, &scene_witness(0.81)), Some(0.81));
    }

    #[test]
    fn witness_value_in_doc_matches_a_node_id_keyed_scene_overlay() {
        let doc = scene_witness_doc(
            "n1", // nodeId-keyed fallback
            serde_json::json!({ "dspUnitParameters": { "outputLevel": 0.81 } }),
        );
        assert_eq!(witness_value_in_doc(&doc, &scene_witness(0.81)), Some(0.81));
    }

    #[test]
    fn witness_value_in_doc_never_falls_back_to_base_when_scenes_is_absent() {
        // No `scenes` key at all — a truncated field-8 read (`scenes` sits at the doc
        // tail). Base's own `outputLevel` (0.81) equals the witness's expected value, so
        // a base-candidate fallback would wrongly accept; the scene arm must not take it.
        let doc = serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "nodeId": "n1", "FenderId": "ACD_Amp",
                  "dspUnitParameters": { "outputLevel": 0.81 } }
            ] } }
        });
        assert_eq!(witness_value_in_doc(&doc, &scene_witness(0.81)), None);
    }

    #[test]
    fn witness_value_in_doc_never_falls_back_to_base_when_the_overlay_misses_the_param() {
        // A Full-shaped overlay (a non-bypass key present) that simply doesn't carry
        // `outputLevel` — base again coincidentally holds the expected value.
        let doc = scene_witness_doc(
            "ACD_Amp",
            serde_json::json!({ "dspUnitParameters": { "gain": 0.5 } }),
        );
        assert_eq!(witness_value_in_doc(&doc, &scene_witness(0.81)), None);
    }
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
        // The stimulus spread must exceed the capture's: a chain only compresses, so a
        // livelier-out-than-in fixture is the aberrant case the spread tell retries.
        let stim = 6.0;

        // First capture lively → no retry.
        let mut calls = 0;
        let out = measure_floor_guarded(
            || {
                calls += 1;
                Ok(lively)
            },
            stim,
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
            stim,
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
            stim,
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

    // A capture cannot be MORE dynamic than the stimulus that drove it — a chain only
    // compresses. More spread out than in means the capture is not the sound we asked
    // for (a partially-landed recall). Armed by the same stationary-stimulus gate as
    // `floor_suspect`, so a disarmed stimulus disarms both.
    #[test]
    fn spread_aberrant_trips_only_above_the_stimulus_with_a_lively_stimulus() {
        assert!(spread_aberrant(9.0, 6.0)); // more dynamic out than in — impossible
        assert!(!spread_aberrant(6.0, 6.0)); // equal: the pass-through limit
        assert!(!spread_aberrant(5.0, 6.0)); // compressed: the normal case
        assert!(!spread_aberrant(6.0 + SPREAD_ABERRANT_MARGIN_LU, 6.0)); // margin is inclusive
        assert!(!spread_aberrant(9.0, 0.2)); // stationary stimulus disarms the trip
    }

    // The spread tell buys exactly ONE re-measure and then reports whatever it got:
    // unlike a floor read, an aberrant-spread capture is a plausible number, so it is
    // never escalated to an error (and never to StillFlat, whose level-shift confirm a
    // wrong-scene capture would pass — presetLevel is linear whatever scene landed).
    #[test]
    fn guarded_measure_retries_once_on_aberrant_spread() {
        let stim = 6.0;
        let aberrant = loud(-30.0, 9.0);
        let compressed = loud(-24.0, 5.0);

        // Aberrant then compressed → the SECOND reading is returned, two calls.
        let mut calls = 0;
        let out = measure_floor_guarded(
            || {
                calls += 1;
                Ok(if calls == 1 { aberrant } else { compressed })
            },
            stim,
            Duration::ZERO,
        )
        .unwrap();
        match out {
            GuardOutcome::Live(l) => assert!((l.integrated_lufs - -24.0).abs() < 1e-9),
            GuardOutcome::StillFlat(_) => panic!("an aberrant capture must never escalate"),
        }
        assert_eq!(calls, 2);

        // Persistently aberrant → still Live (best effort), still exactly one retry.
        let mut calls = 0;
        let out = measure_floor_guarded(
            || {
                calls += 1;
                Ok(aberrant)
            },
            stim,
            Duration::ZERO,
        )
        .unwrap();
        assert!(matches!(out, GuardOutcome::Live(_)));
        assert_eq!(calls, 2);

        // A compressed capture is the normal case — no retry at all.
        let mut calls = 0;
        let out = measure_floor_guarded(
            || {
                calls += 1;
                Ok(compressed)
            },
            stim,
            Duration::ZERO,
        )
        .unwrap();
        assert!(matches!(out, GuardOutcome::Live(_)));
        assert_eq!(calls, 1);

        // The retry can land on a FLOOR read — that must still be reported as flat,
        // not laundered into a Live reading by the spread lane.
        let mut calls = 0;
        let out = measure_floor_guarded(
            || {
                calls += 1;
                Ok(if calls == 1 {
                    aberrant
                } else {
                    loud(-30.18, 0.01)
                })
            },
            stim,
            Duration::ZERO,
        )
        .unwrap();
        assert!(matches!(out, GuardOutcome::StillFlat(_)));
        assert_eq!(calls, 2);
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

    // See `fs_bracket_expansion`'s doc for the bug this covers.
    #[test]
    fn fs_bracket_expansion_targets_the_extreme_that_can_bracket() {
        // Seed pair both near a saturated ceiling (flat) — target is well below both, so
        // the amp needs to go QUIETER: probe toward 0.0.
        assert_eq!(
            fs_bracket_expansion(0.25, -18.0, 0.75, -17.9, -25.0, UNIT_BOUNDS),
            Some(0.0)
        );
        // Symmetric case: target well ABOVE both seeds (needs MORE loudness) → probe 1.0.
        assert_eq!(
            fs_bracket_expansion(0.25, -30.0, 0.75, -29.9, -20.0, UNIT_BOUNDS),
            Some(1.0)
        );
        // Target already bracketed by the seed pair → no expansion needed, the plain
        // secant can converge as-is.
        assert_eq!(
            fs_bracket_expansion(0.25, -30.0, 0.75, -18.0, -23.0, UNIT_BOUNDS),
            None
        );
        // The relevant extreme is ALREADY one of the seeds — nothing left to try.
        assert_eq!(
            fs_bracket_expansion(0.0, -30.0, 0.75, -29.9, -35.0, UNIT_BOUNDS),
            None
        );
        assert_eq!(
            fs_bracket_expansion(0.25, -18.0, 1.0, -17.9, -10.0, UNIT_BOUNDS),
            None
        );
        // The extremes follow the PARAM's range, not a hard-coded [0, 1]: on a raw-dB
        // `[0, 12]` param the quiet/loud probes are 0.0 and 12.0, and a seed already AT
        // 12.0 leaves nothing to try.
        assert_eq!(
            fs_bracket_expansion(3.0, -30.0, 9.0, -29.9, -20.0, (0.0, 12.0)),
            Some(12.0)
        );
        assert_eq!(
            fs_bracket_expansion(3.0, -30.0, 12.0, -29.9, -20.0, (0.0, 12.0)),
            None
        );
    }

    // The full bracket-then-secant shape, mirroring `correct_iter_secant_converges_on_compressor`'s
    // convention (replicate the runner's loop against a synthetic response curve, no device).
    #[test]
    fn fs_bracket_expansion_lets_the_secant_reach_a_target_below_a_saturated_seed_pair() {
        let ceiling = -18.0_f64;
        let model = |v: f64| {
            // Saturates by v=0.2 — BOTH seeds (0.25 and 0.75) land in the flat ceiling,
            // reproducing the reported bug (a knob whose useful range is a small slice
            // of [0, 1]).
            if v > 0.2 {
                ceiling
            } else {
                // Rises linearly from -40 (silent) to the ceiling over 0..0.2.
                -40.0 + (ceiling - -40.0) * (v / 0.2)
            }
        };
        let target = -25.0_f64;
        let (v_lo, v_hi) = (0.25_f64, 0.75_f64);
        let (l_lo, l_hi) = (model(v_lo), model(v_hi));
        let err = |l: f64| (l - target).abs();

        // Old behavior check: the seed pair alone is flat (both on the saturated ceiling) —
        // confirms this fixture actually reproduces the reported bug, not a fixture error.
        assert!(
            (l_hi - l_lo).abs() < KNOB_TOL_LU,
            "fixture must reproduce a flat seed pair: l_lo={l_lo} l_hi={l_hi}"
        );
        assert!(
            err(l_lo.min(l_hi)) > KNOB_TOL_LU,
            "target must be unreached by either seed"
        );

        let mut best = if err(l_lo) <= err(l_hi) {
            (v_lo, l_lo)
        } else {
            (v_hi, l_hi)
        };
        let (mut p0, mut p1) = ((v_lo, l_lo), (v_hi, l_hi));
        if let Some(v_extreme) =
            fs_bracket_expansion(v_lo as f32, l_lo, v_hi as f32, l_hi, target, UNIT_BOUNDS)
        {
            let l_extreme = model(v_extreme as f64);
            if err(l_extreme) < err(best.1) {
                best = (v_extreme as f64, l_extreme);
            }
            if err(p0.1) <= err(p1.1) {
                p1 = (v_extreme as f64, l_extreme);
            } else {
                p0 = (v_extreme as f64, l_extreme);
            }
        }
        if (p1.1 - p0.1).abs() >= KNOB_TOL_LU {
            for _ in 0..MEASURE_CORRECT_MAX {
                if err(best.1) <= KNOB_TOL_LU {
                    break;
                }
                let Some(raw) = fs_secant_next(p0, p1, target) else {
                    break;
                };
                let v2 = raw.clamp(0.0, 1.0);
                let l2 = model(v2);
                if err(l2) < err(best.1) {
                    best = (v2, l2);
                }
                p0 = p1;
                p1 = (v2, l2);
            }
        }
        assert!(
            err(best.1) <= KNOB_TOL_LU,
            "expected convergence near {target}, got {} (v={})",
            best.1,
            best.0
        );
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

    #[test]
    fn common_reachable_target_is_min_of_offset_adjusted_ceilings() {
        use super::{common_reachable_target, common_target};
        // Guitar-only (offset 0): pure min − headroom, identical to `common_target`.
        let g = [(-28.0, 0.0), (-23.0, 0.0)];
        assert_eq!(
            common_reachable_target(&g, 1.0),
            common_target(&[-28.0, -23.0], 1.0),
        );
        assert_eq!(common_reachable_target(&g, 1.0), Some(-29.0)); // min(-28,-23) − 1

        // OFFSET ROUND-TRIP (the offset double-application guard — invisible on guitar):
        // a bass ceiling C=-24 with a +1.5 LU playback offset constrains at C−offset=-25.5.
        // A quieter guitar ceiling C=-28 (offset 0) still binds the min → target -29. The
        // runner ADDS the offset back, so the bass's EFFECTIVE target is -29 + 1.5 = -27.5,
        // which sits UNDER its raw ceiling -24 → reachable (offset applied EXACTLY once).
        let mixed = [(-28.0, 0.0), (-24.0, 1.5)];
        let t = common_reachable_target(&mixed, 1.0).expect("finite");
        assert!((t - -29.0).abs() < 1e-9, "t={t}");
        for &(c, offset) in &mixed {
            assert!(
                t + offset <= c + 1e-9,
                "effective target {} must fit under ceiling {c} (offset {offset})",
                t + offset,
            );
        }

        // When the BASS's offset-adjusted ceiling is the lowest, IT binds: C=-24 offset 3.0
        // → -27 constrains vs a guitar -26 offset 0 → min(-27,-26) − 1 = -28.
        assert_eq!(
            common_reachable_target(&[(-26.0, 0.0), (-24.0, 3.0)], 1.0),
            Some(-28.0)
        );

        // Non-finite ceilings (a silent capture) are ignored; all-non-finite → None.
        assert_eq!(
            common_reachable_target(&[(f64::NAN, 0.0), (-22.0, 0.0)], 1.0),
            Some(-23.0)
        );
        assert_eq!(common_reachable_target(&[(f64::NAN, 0.0)], 1.0), None);
        assert_eq!(common_reachable_target(&[], 1.0), None);
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
        let j = super::solve_joint_k_at(&[amp_knob(0.5)], -30.0, -26.0, super::LEVEL_MIN).unwrap();
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
        let j = super::solve_joint_k_at(
            &[amp_knob(0.5), amp_knob(0.5)],
            -26.0,
            -20.0,
            super::LEVEL_MIN,
        )
        .unwrap();
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
        let j = super::solve_joint_k_at(
            &[amp_knob(0.9), amp_knob(0.3)],
            -18.0,
            -30.0,
            super::LEVEL_MIN,
        )
        .unwrap();
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
        let j = super::solve_joint_k_at(
            &[amp_knob(0.5), amp_knob(0.2)],
            -30.0,
            -30.0,
            super::LEVEL_MIN,
        )
        .unwrap();
        assert!(!j.clamped);
        assert!((j.k_eff - 1.0).abs() < 1e-6, "k_eff={}", j.k_eff);
    }

    // ⟦4a⟧ THE HOLD'S FLOOR (danger.md: outputLevel = 0 is deep digital silence) — the trade's
    // base hold may not solve there. The floor bounds **k**, ratio-preserving, and the pin is
    // REPORTED.
    #[test]
    fn joint_k_honours_the_base_fader_floor_on_a_scale_down() {
        use crate::headroom_trade::BASE_FADER_FLOOR;
        // A 30 dB cut asked of a 0.02 lane: only ~6 dB of it fits above the 0.01 floor.
        let j = super::solve_joint_k_at(
            &[amp_knob(0.8), amp_knob(0.02)],
            -56.0,
            -26.0,
            BASE_FADER_FLOOR,
        )
        .unwrap();
        assert!(j.clamped, "a floor pin is a clamp, not a silent success");
        assert!(
            (j.levels[1] - BASE_FADER_FLOOR).abs() < 1e-6,
            "the QUIETEST lane pins at the floor, got {}",
            j.levels[1]
        );
        let ratio = j.levels[0] / j.levels[1];
        assert!(
            (ratio - 40.0).abs() < 1e-2,
            "0.8:0.02 ratio preserved: {ratio}"
        );
        assert!(
            j.achieved > -33.0 && j.achieved < -31.0,
            "achieved reports the ~6 dB it could actually pay: {}",
            j.achieved
        );

        // LEVEL_MIN (every other lane) is byte-identical to the pre-floor behaviour.
        let free = super::solve_joint_k_at(
            &[amp_knob(0.8), amp_knob(0.02)],
            -56.0,
            -26.0,
            super::LEVEL_MIN,
        )
        .unwrap();
        assert!(!free.clamped);
        assert!((free.achieved - (-56.0)).abs() < 1e-6, "{}", free.achieved);
    }

    // A lane the AUTHOR muted stays muted: the floor bounds `k`, never the individual levels,
    // so `0 · k` is never clamped UP to the floor (which would un-mute it — a tone change).
    #[test]
    fn the_fader_floor_never_unmutes_an_author_muted_lane() {
        use crate::headroom_trade::BASE_FADER_FLOOR;
        let j = super::solve_joint_k_at(
            &[amp_knob(0.8), amp_knob(0.0)],
            -32.0,
            -26.0,
            BASE_FADER_FLOOR,
        )
        .unwrap();
        assert_eq!(j.levels[1], 0.0, "the muted lane stays muted");
        assert!(!j.clamped, "and it does not veto the audible lane's cut");
        assert!((j.achieved - (-32.0)).abs() < 1e-6);
    }

    // ⟦8⟧ Only a DIRECTION-BLOCKING pin names a cause. A row that merely ran out of secant
    // captures mid-range is clamped with no false "already maxed out" claim.
    #[test]
    fn only_a_direction_blocking_pin_counts_as_a_ceiling() {
        use super::PinnedBound;
        // Maxed out and the target is still louder → a real ceiling.
        assert_eq!(
            super::joint_levels_pinned(&[1.0], &[0.5], super::LEVEL_MIN, -20.0, -15.0),
            Some(PinnedBound::Max)
        );
        // Maxed out but the target is BELOW what we achieved → the search stopped early.
        assert_eq!(
            super::joint_levels_pinned(&[1.0], &[0.5], super::LEVEL_MIN, -15.0, -20.0),
            None
        );
        // Mid-range and off target in either direction → not pinned at all.
        assert_eq!(
            super::joint_levels_pinned(&[0.4], &[0.5], super::LEVEL_MIN, -20.0, -15.0),
            None
        );
        // At the trade's fader floor with the target still quieter → out of fader.
        assert_eq!(
            super::joint_levels_pinned(
                &[crate::headroom_trade::BASE_FADER_FLOOR],
                &[0.5],
                crate::headroom_trade::BASE_FADER_FLOOR,
                -15.0,
                -20.0
            ),
            Some(PinnedBound::Floor)
        );
        // An author-muted lane the solve never moved is not the solve pinning there.
        assert_eq!(
            super::joint_levels_pinned(&[0.0], &[0.0], super::LEVEL_MIN, -15.0, -20.0),
            None
        );
    }

    // A dB-unit knob can't be scaled multiplicatively → error, never a garbage write.
    #[test]
    fn joint_k_rejects_db_knob() {
        let mut kt = amp_knob(0.5);
        kt.lo = -18.0;
        kt.hi = 6.0;
        assert!(super::solve_joint_k_at(&[kt], -30.0, -26.0, super::LEVEL_MIN).is_err());
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
    fn switch_at_target_accepts_within_fs_tol() {
        assert!(
            super::switch_at_target(-24.0, -24.09, false),
            "0.09 LU off, unclamped → skip the re-solve (FS_TOL_LU = 0.1, tighter than the scene lane's KNOB_TOL_LU)"
        );
    }

    #[test]
    fn switch_at_target_rejects_just_outside_fs_tol() {
        assert!(
            !super::switch_at_target(-24.0, -24.11, false),
            "0.11 LU off → must re-level"
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

    // (A3) A base block knob write must recall base explicitly — a preset loads
    // into its saved lastLoadedScene, not necessarily base (HW), so a bare write
    // with no recall would silently land wherever that saved scene left it. This
    // is the reported bug's exact shape: preset 28 (the e2e `E2E Hiwatt 3S` fixture)
    // has `lastLoadedScene = 3`, and scene 3 is literally named "Base Scene" —
    // the naming collision that made the symptom read as "leveling wrote into
    // the base preset" when it actually wrote into scene 3's overlay. The
    // engine only ever addresses scenes by numeric slot (no name field exists
    // at this layer), so the fix is immune to the name regardless.
    #[test]
    fn set_knob_base_block_recalls_base_explicitly() {
        let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(30).expect("load_preset"); // activates saved scene 3, not base
        let knob = LevelKnob::Block {
            group_id: "G1".into(),
            node_id: "amp".into(),
            parameter_id: "outputLevel".into(),
            scene_slot: None,
        };
        set_knob(&mut s, &knob, 0.6, None).expect("set_knob");
        let ev = sim.events();
        assert!(
            ev.iter().any(|e| matches!(
                e,
                crate::sim_device::SimEvent::ChangeParameter {
                    scene: crate::sim_device::SCENE_BASE,
                    param,
                    ..
                } if param == "outputLevel"
            )),
            "a base block knob must write scene_base, not the leftover saved scene 3: {ev:?}"
        );
    }

    // Same fix, batched path: a base-only `set_knobs` target set must ALSO recall
    // base explicitly, not rely on the connection's default scene.
    #[test]
    fn set_knobs_refuses_a_batch_that_mixes_scenes() {
        // Only the FIRST scene found is recalled, so a mixed batch would land every
        // write in that scene's overlay — silently, each write confirming normally.
        let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(30).expect("load_preset");
        let mk = |scene: u32| LevelKnob::Block {
            group_id: "G1".into(),
            node_id: "amp".into(),
            parameter_id: "outputLevel".into(),
            scene_slot: Some(scene),
        };
        let (a, b) = (mk(1), mk(2));
        let err = set_knobs(&mut s, &[(&a, 0.6), (&b, 0.4)], None)
            .expect_err("a batch mixing scenes 1 and 2 must be refused, not silently merged");
        assert!(
            err.contains("mixes scenes"),
            "error should name the mixed-scene cause: {err}"
        );
        assert!(
            !sim.events()
                .iter()
                .any(|e| matches!(e, crate::sim_device::SimEvent::ChangeParameter { .. })),
            "nothing may be written when the batch is refused: {:?}",
            sim.events()
        );
    }

    #[test]
    fn set_knobs_refuses_a_batch_mixing_base_and_scene() {
        // The scene branch recalls the scene whenever ANY scene target is present, so a
        // base-scoped knob riding along would be written under that overlay, not base.
        let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(30).expect("load_preset");
        let mk = |scene: Option<u32>| LevelKnob::Block {
            group_id: "G1".into(),
            node_id: "amp".into(),
            parameter_id: "outputLevel".into(),
            scene_slot: scene,
        };
        let (base, scened) = (mk(None), mk(Some(2)));
        let err = set_knobs(&mut s, &[(&base, 0.6), (&scened, 0.4)], None)
            .expect_err("a batch mixing a base target with a scene target must be refused");
        assert!(
            err.contains("mixes base and scene"),
            "error should name the base/scene mix: {err}"
        );
        assert!(
            !sim.events()
                .iter()
                .any(|e| matches!(e, crate::sim_device::SimEvent::ChangeParameter { .. })),
            "nothing may be written when the batch is refused: {:?}",
            sim.events()
        );
    }

    #[test]
    fn set_knobs_base_only_batch_recalls_base_explicitly() {
        let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(30).expect("load_preset");
        let knob = LevelKnob::Block {
            group_id: "G1".into(),
            node_id: "amp".into(),
            parameter_id: "outputLevel".into(),
            scene_slot: None,
        };
        set_knobs(&mut s, &[(&knob, 0.6)], None).expect("set_knobs");
        let ev = sim.events();
        assert!(
            ev.iter().any(|e| matches!(
                e,
                crate::sim_device::SimEvent::ChangeParameter {
                    scene: crate::sim_device::SCENE_BASE,
                    param,
                    ..
                } if param == "outputLevel"
            )),
            "a base-only set_knobs batch must write scene_base: {ev:?}"
        );
    }

    // A PresetLevel-only batch gets NO recall — setPresetLevel is a global
    // multiplier, not a scene-scoped changeParameter, so a recall would only risk
    // reverting an unsaved presetLevel write for no benefit.
    #[test]
    fn set_knobs_preset_level_only_gets_no_recall() {
        let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(30).expect("load_preset");
        set_knobs(&mut s, &[(&LevelKnob::PresetLevel, 0.5)], None).expect("set_knobs");
        let ev = sim.events();
        assert!(
            !ev.iter()
                .any(|e| matches!(e, crate::sim_device::SimEvent::LoadScene(_))),
            "a PresetLevel-only batch must not recall any scene: {ev:?}"
        );
    }

    // The pre-save recall reverts an UNSAVED `presetLevel` to the saved value (the
    // load-level-apply gotcha), so every recalling save must replay it between the
    // recall and the save — the exact op order is the invariant (HW: a solved
    // 0.3096 "[SAVED]" persisted the prior 0.32 until this re-assert existed;
    // caught live by the online level.online.spec.ts base idempotency test).
    #[test]
    fn recall_reassert_save_replays_the_unsaved_level_after_the_recall() {
        let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(30).expect("load_preset");
        recall_reassert_save(&mut s, 30, Some(3), Some(0.42)).expect("save");
        let tail: Vec<String> = sim
            .events()
            .iter()
            .filter_map(|e| match e {
                crate::sim_device::SimEvent::LoadScene(n) => Some(format!("scene{n}")),
                crate::sim_device::SimEvent::PresetLevel(v) => Some(format!("level{v}")),
                crate::sim_device::SimEvent::Saved(n) => Some(format!("save{n}")),
                _ => None,
            })
            .collect();
        assert_eq!(
            tail,
            vec!["scene3", "level0.42", "save30"],
            "order must be recall → re-assert → save"
        );
    }

    // Without a recall there is nothing to revert — the re-assert must NOT fire
    // (an unconditional extra write would be a behavior change for plain saves).
    #[test]
    fn recall_reassert_save_skips_the_reassert_without_a_recall() {
        let sim = crate::sim_device::SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(30).expect("load_preset");
        recall_reassert_save(&mut s, 30, None, Some(0.42)).expect("save");
        let ev = sim.events();
        assert!(
            !ev.iter()
                .any(|e| matches!(e, crate::sim_device::SimEvent::PresetLevel(_))),
            "no recall → no re-assert: {ev:?}"
        );
        assert!(
            ev.iter()
                .any(|e| matches!(e, crate::sim_device::SimEvent::Saved(30))),
            "the save itself must still land: {ev:?}"
        );
    }

    // A multi-lane redistribution restore (≥2 base knobs, e.g. a parallel-merged
    // preset's two amps) must recall base ONCE for the whole group, not once per
    // knob — a per-knob `set_knob` loop would re-`load_scene(BASE)` between
    // writes, reverting the earlier knob's just-written value before the batch
    // ever saves.
    #[test]
    fn write_grouped_knobs_recalls_base_once_for_multiple_base_knobs() {
        let sim = crate::sim_device::SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(30).expect("load_preset");
        let knobs = vec![
            PrevKnobWrite {
                group_id: "G1".into(),
                node_id: "amp1".into(),
                scene_slot: None,
                value: 0.6,
            },
            PrevKnobWrite {
                group_id: "G1".into(),
                node_id: "amp2".into(),
                scene_slot: None,
                value: 0.7,
            },
        ];
        write_grouped_knobs(&mut s, &knobs, None).expect("write_grouped_knobs");
        let ev = sim.events();
        let base_recalls = ev
            .iter()
            .filter(|e| matches!(e, crate::sim_device::SimEvent::LoadScene(scene) if *scene == crate::session::BASE_SCENE_SLOT))
            .count();
        assert_eq!(
            base_recalls, 1,
            "two base knobs must share ONE base recall, not one each: {ev:?}"
        );
        // Both knobs' values must have actually landed (not reverted by a
        // redundant recall).
        assert!(ev.iter().any(
            |e| matches!(e, crate::sim_device::SimEvent::ChangeParameter { node, .. } if node == "amp1")
        ));
        assert!(ev.iter().any(
            |e| matches!(e, crate::sim_device::SimEvent::ChangeParameter { node, .. } if node == "amp2")
        ));
    }

    /// Saved (field-8) preset for the Scene Edit tests: one amp node in G1, base
    /// `outputLevel`/`gain`, and — when `overlay` — a scene-0 overlay for that node.
    fn saved_with_overlay(overlay: bool) -> serde_json::Value {
        let mut p = serde_json::json!({
            "lastLoadedScene": 3,
            "audioGraph": { "template": "gtrSeries", "guitarNodes": { "G1": [
                { "nodeId": "amp", "FenderId": "ACD_Twin57",
                  "dspUnitParameters": { "bypass": false, "outputLevel": 0.5, "gain": 0.7 } }
            ] } },
            "scenes": [{ "sceneName": "Lead" }]
        });
        if overlay {
            p["scenes"][0]["guitarNodes"] = serde_json::json!({
                "G1": { "amp": { "dspUnitParameters": { "outputLevel": 0.4, "gain": 0.3 } } }
            });
        }
        p
    }

    fn scene_knob(param: &str) -> LevelKnob {
        LevelKnob::Block {
            group_id: "G1".into(),
            node_id: "amp".into(),
            parameter_id: param.into(),
            scene_slot: Some(0),
        }
    }

    // B1: `SetNodeSceneEdit(node, true)` RESEEDS the node's scene overlay from base (HW
    // 3-cell matrix, `probe_api/slot_write.rs`), so a scene that ALREADY has an overlay for
    // the node must be written with the enable DROPPED — the write lands on the overlay
    // regardless, and the enable would wipe the scene's other stored params (the reported
    // corruption: scene tone params reseeded to base while the leveled value survived).
    #[test]
    fn set_knobs_omits_the_scene_edit_enable_when_the_overlay_exists() {
        let sim = crate::sim_device::SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(0).expect("load_preset");
        // Base gain 0.7; scene 0 already carries its own gain 0.3 (what the reseed wipes).
        s.load_scene(crate::session::BASE_SCENE_SLOT).expect("base");
        s.change_parameter("G1", "amp", "gain", 0.7)
            .expect("base gain");
        s.load_scene(0).expect("scene 0");
        s.set_node_scene_edit("G1", "amp", true)
            .expect("seed overlay");
        s.change_parameter("G1", "amp", "gain", 0.3)
            .expect("scene gain");

        let knob = scene_knob("outputLevel");
        let saved = saved_with_overlay(true);
        set_knobs(&mut s, &[(&knob, 0.9)], Some(&saved)).expect("set_knobs");

        let enables = sim
            .events()
            .iter()
            .skip_while(|e| !matches!(e, crate::sim_device::SimEvent::ChangeParameter { param, value, .. } if param == "gain" && *value == 0.3))
            .filter(|e| matches!(e, crate::sim_device::SimEvent::SceneEdit { enable: true, .. }))
            .count();
        assert_eq!(
            enables,
            0,
            "an existing overlay must be written WITHOUT the reseeding enable: {:?}",
            sim.events()
        );
        assert_eq!(
            sim.param_write(0, "G1", "amp", "gain"),
            Some(0.3),
            "the scene's own gain must survive the leveling write, not be reseeded to base's 0.7"
        );
    }

    // The other HW branch: no overlay for the node in that scene → the enable is what
    // MATERIALISES the overlay, so without it the write LEAKS TO BASE. It must be sent.
    #[test]
    fn set_knobs_enables_scene_edit_when_the_overlay_is_absent() {
        let sim = crate::sim_device::SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(0).expect("load_preset");
        let knob = scene_knob("outputLevel");
        let saved = saved_with_overlay(false);
        set_knobs(&mut s, &[(&knob, 0.9)], Some(&saved)).expect("set_knobs");
        assert!(
            sim.events().iter().any(|e| matches!(
                e,
                crate::sim_device::SimEvent::SceneEdit { group, node, enable: true }
                    if group == "G1" && node == "amp"
            )),
            "an absent overlay needs the enable to materialise it: {:?}",
            sim.events()
        );
    }

    // Neither write shape is safe when overlay presence is UNKNOWN (truncated field-8 read)
    // or no saved doc was read at all — both mistakes corrupt the preset, so refuse before
    // touching the device rather than guess.
    #[test]
    fn set_knobs_refuses_a_scene_write_without_known_overlay_presence() {
        for (label, saved) in [
            ("no saved doc", None),
            // `scenes` truncated away → SceneOverlay::Unknown.
            (
                "truncated read",
                Some(serde_json::json!({ "audioGraph": { "guitarNodes": { "G1": [
                { "nodeId": "amp", "FenderId": "ACD_Twin57", "dspUnitParameters": {} }] } } })),
            ),
        ] {
            let sim = crate::sim_device::SimDevice::new();
            let mut s = Session::from_transport(Box::new(sim.clone()));
            s.load_preset(0).expect("load_preset");
            let knob = scene_knob("outputLevel");
            let err = set_knobs(&mut s, &[(&knob, 0.9)], saved.as_ref())
                .expect_err("a scene write with unknown overlay presence must be refused");
            assert!(
                err.contains("overlay"),
                "{label}: the error must name the overlay-presence cause: {err}"
            );
            assert!(
                !sim.events().iter().any(|e| matches!(
                    e,
                    crate::sim_device::SimEvent::ChangeParameter { .. }
                        | crate::sim_device::SimEvent::SceneEdit { .. }
                )),
                "{label}: nothing may be written when the batch is refused: {:?}",
                sim.events()
            );
        }
    }

    // The BYPASS-ONLY branch (HW-verified fw 1.8.45): the node's Scene Edit flag is OFF, so
    // the scene carries only the bypass family and SHARES the node's knobs with base. Both
    // write shapes are wrong — the enable reseeds, the enable-dropped write lands on BASE and
    // changes every sharing scene (measured: base gain 2.5 → 7.0, the bypass-only overlay
    // unchanged). Refuse, with the SHARING named so the user can act on it, and touch nothing.
    #[test]
    fn set_knobs_refuses_a_scene_write_on_a_bypass_only_overlay() {
        for (label, overlay) in [
            ("bypass only", serde_json::json!({ "bypass": false })),
            (
                "bypass + bypassType",
                serde_json::json!({ "bypass": false, "bypassType": "Post" }),
            ),
        ] {
            let sim = crate::sim_device::SimDevice::new();
            let mut s = Session::from_transport(Box::new(sim.clone()));
            s.load_preset(0).expect("load_preset");
            let mut saved = saved_with_overlay(true);
            saved["scenes"][0]["guitarNodes"]["G1"]["amp"] =
                serde_json::json!({ "dspUnitParameters": overlay });
            let knob = scene_knob("outputLevel");
            let err = set_knobs(&mut s, &[(&knob, 0.9)], Some(&saved))
                .expect_err("a bypass-only overlay must refuse the scene-scoped write");
            assert!(
                err.contains("shares") && err.contains("Base"),
                "{label}: the error must name the sharing and point at Base: {err}"
            );
            assert!(
                !sim.events().iter().any(|e| matches!(
                    e,
                    crate::sim_device::SimEvent::ChangeParameter { .. }
                        | crate::sim_device::SimEvent::SceneEdit { .. }
                )),
                "{label}: nothing may be written when the batch is refused: {:?}",
                sim.events()
            );
        }
    }

    /// The bounds of an ordinary `[0,1]` control — what the FS solve assumed unconditionally
    /// before the param-class split, so every pre-existing solve test keeps its exact
    /// arithmetic by passing it.
    const UNIT_BOUNDS: (f32, f32) = (0.0, 1.0);

    /// A plain `level_linear` `[0,1]` solve target — the shape every legacy FS solve test
    /// exercised implicitly. `authored` is irrelevant outside the WetMix floor, so 0.5.
    fn fs_unit_param() -> FsParamTarget {
        let p = FsParamTarget::new("ACD_SomeBlock", "level", 0.5);
        assert_eq!(p.info.class, crate::param_class::ParamClass::LevelLinear);
        assert_eq!(p.bounds(), UNIT_BOUNDS);
        p
    }

    /// A synthetic loudness reading for the injected-capture footswitch tests.
    fn fs_loud(integrated: f64) -> lufs::Loudness {
        lufs::Loudness {
            integrated_lufs: integrated,
            short_term_max_lufs: integrated + 2.0,
            true_peak_dbtp: -1.0,
        }
    }

    /// FS isolation must be re-sent on EVERY capture, not just the first: each capture
    /// recalls a scene (base included — `arm_measurement`) and a recall RE-ASSERTS that
    /// scene's own bypass state, so a once-only forced write leaves captures 2..N measuring
    /// a NON-isolated sound (the sibling block-acting switch audible again). Two switches:
    /// the sibling's forced bypass must appear in every call's list.
    ///
    /// The injected capture keeps this off the device paths that open a real session
    /// (`reamp_off`): a plain solve with `current_value: None` and live readings never
    /// reaches them.
    #[test]
    fn solve_footswitch_resends_the_isolation_list_on_every_capture() {
        let iso = vec![("G1".to_string(), "sibling".to_string(), true)];
        let seen: std::cell::RefCell<Vec<Vec<(String, String, bool)>>> =
            std::cell::RefCell::new(Vec::new());
        // Response with real slope so the secant keeps iterating (target never reached).
        let r = solve_footswitch(
            2,
            &iso,
            &[],
            -20.0,
            "baked",
            None,
            &fs_unit_param(),
            |byp, v| {
                seen.borrow_mut().push(byp.to_vec());
                Ok(fs_loud(-40.0 + 10.0 * f64::from(v)))
            },
        )
        .expect("solve");
        let calls = seen.borrow();
        assert!(
            calls.len() >= 3,
            "expected a multi-capture solve: {calls:?}"
        );
        assert!(
            calls.iter().all(|got| got == &iso),
            "every capture must carry the full isolation list, not just the first: {calls:?}"
        );
        assert_eq!(r.switch, 2);
    }

    /// Monotone piecewise-linear interpolant over measured `(knob, LUFS)` anchors — the
    /// shared body of every HW-captured response-curve fixture below.
    fn piecewise_lufs(anchors: &[(f64, f64)], v: f32) -> f64 {
        let v = f64::from(v).clamp(0.0, 1.0);
        for w in anchors.windows(2) {
            let ((v0, l0), (v1, l1)) = (w[0], w[1]);
            if v <= v1 {
                return l0 + (v - v0) / (v1 - v0) * (l1 - l0);
            }
        }
        anchors[anchors.len() - 1].1
    }

    /// The HW curve that beat the plain secant (Hiwatt fs12, UniVibe `volume` —
    /// measured points from the strict-harness post-mortem, 3/3 reproducible):
    /// a steep audio-taper cliff (~90 LU/knob-unit) under a shallow top (~10),
    /// modeled as the monotone piecewise-linear interpolant of the captures.
    fn univibe_volume_curve(v: f32) -> f64 {
        const ANCHORS: [(f64, f64); 8] = [
            (0.0, -80.0),
            (0.25, -45.6),
            (0.3207, -38.0),
            (0.45, -32.0),
            (0.6146, -16.94),
            (0.6758, -16.2),
            (0.75, -15.5),
            (1.0, -14.5),
        ];
        piecewise_lufs(&ANCHORS, v)
    }

    /// On the UniVibe cliff the plain sliding secant provably stops ~3 LU hot
    /// (it did on HW, and the strict e2e re-measure caught the miss); the
    /// bracket-aware loop must converge inside `KNOB_TOL_LU` — the seed pair
    /// already straddles the target, so Illinois-damped bracket retention owns
    /// the endgame instead of the slide.
    #[test]
    fn solve_footswitch_bracket_mode_converges_on_the_univibe_cliff() {
        let mut captures = 0u32;
        let r = solve_footswitch(
            12,
            &[],
            &[],
            -20.0,
            "baked",
            None,
            &fs_unit_param(),
            |_, v| {
                captures += 1;
                Ok(fs_loud(univibe_volume_curve(v)))
            },
        )
        .expect("solve");
        assert!(
            (r.predicted_lufs - -20.0).abs() <= KNOB_TOL_LU,
            "bracket mode must land within KNOB_TOL_LU: best {} LUFS at v={} ({} captures)",
            r.predicted_lufs,
            r.final_value,
            captures
        );
        assert!(!r.clamped && !r.unconverged, "converged solve: {r:?}");
        // Budget sanity: seeds + expansion + bounded correction, never unbounded.
        assert!(captures <= 3 + FS_CORRECT_MAX, "captures={captures}");
    }

    // A knob with NO authority over loudness (every capture measures the same
    // off-target LUFS) is a physical dead end: a re-run repeats the identical miss,
    // so the verdict must be the reason-less clamp — never "unconverged", whose UI
    // copy advertises a re-run that cannot help.
    #[test]
    fn solve_footswitch_flat_response_reports_clamped_not_unconverged() {
        let mut captures = 0u32;
        let r = solve_footswitch(
            12,
            &[],
            &[],
            -20.0,
            "baked",
            None,
            &fs_unit_param(),
            |_, _| {
                captures += 1;
                Ok(fs_loud(-27.0))
            },
        )
        .expect("solve");
        assert!(
            r.clamped && !r.unconverged,
            "flat response must clamp, not advertise a re-run: {r:?}"
        );
        assert!(captures <= 3 + FS_CORRECT_MAX, "captures={captures}");
    }

    // --- Regression coverage for the "silence past the first seed aborts the whole solve"
    // bug (HW-reproduced, fw 1.8.45, preset "TR+BD2+BMP" — 4 drive pedals into a '65 Twin).
    // A pedal `level`/`volume` knob at 0 reads genuinely SILENT on the real unit — that
    // silence is the quiet EXTREME of the response curve (DATA), not a routing error, for
    // every measurement AFTER the first seed. See `FS_SILENT_GEOMETRY_LUFS`'s doc for why
    // the synthesized pseudo point is pinned there rather than at the true digital-silence
    // floor `MUTE_FLOOR_SILENT_LUFS`. The first-seed routing-clamp arm is deliberately NOT
    // exercised here: it calls `reamp_off()` → a real `Session::connect_lean()`, which the
    // injected-capture suite keeps off-limits (see the isolation test's doc above) — that
    // arm is byte-unchanged by the silence-as-data fix.

    /// HW-measured Plumes-pedal `level` knob curve (`probe --fs-sweep`, preset
    /// "TR+BD2+BMP", fw 1.8.45): 0.11→−66.9, 0.20→−24.2, 0.30→−20.1, 0.50→−18.1,
    /// 0.75→−17.2, 1.00→−16.9 LUFS — the cliff shifted 0.01 up from the HW-measured
    /// 0.10 so the fixed seed pair (0.25/0.75) reads a genuinely SLOPED, non-flat pair
    /// (`fs_bracket_expansion` must NOT fire) while the correction loop's FIRST
    /// extrapolated point still lands on the wrong side of the cliff — the exact
    /// pathology reported (HW, preset "TR+BD2+BMP"): a knob whose useful range is a
    /// narrow slice of its own range extrapolates a knob value PAST the silent
    /// extreme mid-correction, not at either seed. `v ≤ 0.11` collapses to a
    /// genuinely SILENT capture (not merely quiet).
    fn plumes_level_curve(v: f32) -> Result<lufs::Loudness, String> {
        const ANCHORS: [(f64, f64); 6] = [
            (0.11, -66.9),
            (0.20, -24.2),
            (0.30, -20.1),
            (0.50, -18.1),
            (0.75, -17.2),
            (1.00, -16.9),
        ];
        if f64::from(v) <= 0.11 {
            return Err(NO_SIGNAL_CAPTURED.to_string());
        }
        Ok(fs_loud(piecewise_lufs(&ANCHORS, v)))
    }

    /// Re-pinned to the MID-LOOP silence arm (a `NO_SIGNAL_CAPTURED` on the bracket-aware
    /// secant's OWN extrapolated point, not on either seed) — the law-predicted seed 2
    /// reroutes this fixture off the arm it was originally built to cover, and the
    /// `FS_MIN_SEED_GAP_LU` fix (the law-predicted seed 2's acceptance gate is now the
    /// EXPECTED LUFS separation, not the raw v-space span) reroutes it AGAIN: at the old
    /// -26.0 target the prediction is now ACCEPTED (a real, ≈0.16 knob value — the gap-gate
    /// fix's whole point), so the mid-loop-silence arm needs a target far enough from seed
    /// 1's -22.15 LUFS reading to fail the CENTRAL-5%-95%-of-range frac gate instead
    /// (rather than the gap gate, which a target this far away trivially clears) — -40.0
    /// pushes the raw prediction below v=0.05, rejecting it and falling seed 2 back to the
    /// fixed 0.75 exactly as before. Seed 1 (0.25, real) and seed 2 (0.75, real) are both
    /// proven-real captures with real slope (`fs_bracket_expansion` never fires — the pair
    /// isn't flat), and the correction loop's own extrapolation from that (comparatively
    /// shallow, real-region-only) pair wildly undershoots into the silent zone before
    /// climbing back — several consecutive captures land silent before the secant recovers
    /// (mirroring the ORIGINAL reported pathology more faithfully than a single-dip case
    /// would: a shallow seed pair genuinely can take more than one correction to walk back
    /// off a cliff it undershot). Provably reaches the arm: `seen[2]` (the first
    /// correction-loop capture, neither seed) is asserted silent.
    #[test]
    fn solve_footswitch_treats_post_seed_silence_as_the_quiet_extreme_not_a_routing_error() {
        let seen: std::cell::RefCell<Vec<f32>> = std::cell::RefCell::new(Vec::new());
        let r = solve_footswitch(
            21,
            &[],
            &[],
            -40.0,
            "baked",
            None,
            &fs_unit_param(),
            |_, v| {
                seen.borrow_mut().push(v);
                plumes_level_curve(v)
            },
        )
        .expect("a silent capture mid-correction must be DATA, never a fatal abort");
        let seen = seen.borrow();
        assert_eq!(seen[0], 0.25, "seed 1 is the fixed quarter-range point");
        assert_eq!(
            seen[1], 0.75,
            "seed 2's law prediction is gated off (predicted value outside the central \
             5%-95% of the range), so it falls back to the fixed complement"
        );
        assert!(
            seen.len() >= 3 && seen[2] <= 0.11,
            "the MID-LOOP arm must actually fire — the third capture (the correction \
             loop's own extrapolation, not either seed) must probe below the cliff: \
             {seen:?}"
        );
        assert!(
            (r.predicted_lufs - -40.0).abs() <= KNOB_TOL_LU,
            "expected convergence near -40.0: got {} at v={} ({} captures)",
            r.predicted_lufs,
            r.final_value,
            seen.len()
        );
        assert!(
            !r.clamped,
            "the -40 target sits on the measured cliff, reachable: {r:?}"
        );
        assert!(
            !r.unconverged,
            "must actually converge, not run out of budget: {r:?}"
        );
        assert!(
            r.final_value > 0.11 && r.final_value < 0.25,
            "solved value should land on the measured cliff (0.11, 0.25): {}",
            r.final_value
        );
        assert!(
            seen.len() as u32 <= 3 + FS_CORRECT_MAX,
            "captures={}",
            seen.len()
        );
    }

    /// A knob whose useful range is a narrow slice near the TOP of `[0, 1]`: the fixed
    /// 0.25/0.75 seeds land on an identical flat −17.0 plateau (the ORIGINAL false
    /// "no authority" trigger — `fs_bracket_expansion` fires), but the expansion probe
    /// toward 0.0 is genuinely SILENT below 0.12 rather than merely quiet. Before the fix
    /// this silence was swallowed whole by the bracket-expansion probe's `if let Ok`,
    /// leaving the flat pair undisturbed → the false reason-less clamp. After the fix the
    /// probe's silence becomes a pseudo point, giving the pair real slope so the secant
    /// reaches the target sitting in the 0.12–0.25 ramp.
    fn flat_plateau_curve(v: f32) -> Result<lufs::Loudness, String> {
        let v = f64::from(v);
        if v < 0.12 {
            return Err(NO_SIGNAL_CAPTURED.to_string());
        }
        if v >= 0.25 {
            return Ok(fs_loud(-17.0));
        }
        let frac = (v - 0.12) / (0.25 - 0.12);
        Ok(fs_loud(-50.0 + frac * 33.0))
    }

    /// Re-pinned to the EXPANSION-PROBE arm specifically. The seed-1 reading here sits
    /// EXACTLY on the flat plateau (`v = 0.25` is the plateau's own boundary, `l_a =
    /// −17.0`), so under `FS_MIN_SEED_GAP_LU` the ONLY way to reject the law-predicted
    /// seed 2 is a target within 1 LU of `l_a` — a target far enough away (the original
    /// −26.0, or −22.0) instead gets ACCEPTED and lands the prediction for real inside the
    /// 0.12–0.25 ramp, never reaching the expansion probe at all. −17.6 (0.6 LU of −17.0,
    /// under the 1 LU gap floor) rejects the prediction on the gap gate — the SAME
    /// rejection that keeps seed 2 at the fixed 0.75 complement, also real, also flat vs
    /// seed 1 (`v = 0.75` is still on the plateau) — so the seed PAIR itself stays flat and
    /// it is `fs_bracket_expansion` alone that reaches for the 0.0 extreme and finds it
    /// silent. Provably reaches the arm: `seen[2] == 0.0` — the low bound the expansion
    /// probe (never a seed) targets.
    #[test]
    fn solve_footswitch_flat_seed_pair_with_silent_expansion_still_converges() {
        let seen: std::cell::RefCell<Vec<f32>> = std::cell::RefCell::new(Vec::new());
        let r = solve_footswitch(
            22,
            &[],
            &[],
            -17.6,
            "baked",
            None,
            &fs_unit_param(),
            |_, v| {
                seen.borrow_mut().push(v);
                flat_plateau_curve(v)
            },
        )
        .expect("a silent bracket-expansion probe must not abort the solve");
        let seen = seen.borrow();
        assert_eq!(seen[0], 0.25, "seed 1 is the fixed quarter-range point");
        assert_eq!(
            seen[1], 0.75,
            "seed 2's law prediction is gated off (the expected LUFS gap from seed 1 is \
             under FS_MIN_SEED_GAP_LU), keeping the pair flat so the expansion probe — not \
             seed 2 — owns the silence"
        );
        assert_eq!(
            seen.get(2),
            Some(&0.0),
            "the EXPANSION-PROBE arm must fire: the third capture is `fs_bracket_expansion`'s \
             own low-bound probe, not a seed: {seen:?}"
        );
        assert!(
            (r.predicted_lufs - -17.6).abs() <= KNOB_TOL_LU,
            "expected convergence near -17.6: got {} at v={} ({} captures)",
            r.predicted_lufs,
            r.final_value,
            seen.len()
        );
        assert!(
            !r.clamped && !r.unconverged,
            "the false 'no authority' clamp must not fire once the expansion probe's \
             silence is treated as data: {r:?}"
        );
        assert!(
            seen.len() as u32 <= 3 + FS_CORRECT_MAX,
            "captures={}",
            seen.len()
        );
    }

    /// A "gate"-style knob response — DEcreasing with `v` (a physically real shape: e.g. a
    /// noise-gate threshold, where turning it UP silences MORE) so seed 2 is the one that
    /// lands silent, not seed 1. Steepened from the original -26 LU/knob-unit slope to -44:
    /// with the shallower slope the law-predicted seed 2 landed INSIDE the real region
    /// (0.436, computed from seed 1's -32.33 LUFS reading), rerouting this fixture off the
    /// `seed2_silent` arm entirely. At -44 LU/knob-unit the same prediction (`0.25 +
    /// 20·log10⁻¹(...)` ≈ 1.03) overshoots past the range's 0.95 fraction ceiling (in fact
    /// past `1.0` outright), so the frac gate — UNCHANGED by the `FS_MIN_SEED_GAP_LU` fix,
    /// which only replaced the separate v-space-span component — rejects it and seed 2
    /// falls back to the fixed complement (0.75) — which this curve keeps silent
    /// (`v ≥ 0.60`). Exercises the `seed2_silent` arm directly: the initial best-seed pick
    /// must still choose the real (0.25) reading, never the synthesized one, and the pseudo
    /// point must still let the secant find the target. Provably reaches the arm: `seen[1]`
    /// (seed 2 itself) is asserted silent.
    fn second_seed_silent_curve(v: f32) -> Result<lufs::Loudness, String> {
        let v = f64::from(v);
        if v >= 0.60 {
            return Err(NO_SIGNAL_CAPTURED.to_string());
        }
        Ok(fs_loud(-14.0 - (v / 0.60) * 44.0))
    }

    #[test]
    fn solve_footswitch_second_seed_silence_becomes_data_not_a_hard_error() {
        let seen: std::cell::RefCell<Vec<f32>> = std::cell::RefCell::new(Vec::new());
        let r = solve_footswitch(
            23,
            &[],
            &[],
            -20.0,
            "baked",
            None,
            &fs_unit_param(),
            |_, v| {
                seen.borrow_mut().push(v);
                second_seed_silent_curve(v)
            },
        )
        .expect("a silent SECOND seed must not abort the solve either");
        let seen = seen.borrow();
        assert_eq!(seen[0], 0.25, "seed 1 is the fixed quarter-range point");
        assert!(
            seen.len() >= 2 && seen[1] >= 0.60,
            "the seed2_silent arm must fire: seed 2 itself must be the silent capture: {seen:?}"
        );
        assert!(
            (r.predicted_lufs - -20.0).abs() <= KNOB_TOL_LU,
            "expected convergence near -20.0: got {} at v={} ({} captures)",
            r.predicted_lufs,
            r.final_value,
            seen.len()
        );
        assert!(
            !r.clamped && !r.unconverged,
            "reachable target must converge: {r:?}"
        );
        assert!(
            seen.len() as u32 <= 3 + FS_CORRECT_MAX,
            "captures={}",
            seen.len()
        );
    }

    /// The Plumes shape shifted 40 LU down — a very quiet chain whose REAL captures sit
    /// BELOW the −50 sentinel. A fixed-LUFS pseudo point would be the LOUDEST point in any
    /// pair it enters (slope sign inverts — the solver would walk the wrong way), so the
    /// sentinel must ride a fixed margin below the quietest real capture instead.
    fn deep_quiet_pedal_curve(v: f32) -> Result<lufs::Loudness, String> {
        const ANCHORS: [(f64, f64); 7] = [
            (0.10, -106.9),
            (0.15, -69.8),
            (0.20, -64.2),
            (0.30, -60.1),
            (0.50, -58.1),
            (0.75, -57.2),
            (1.00, -56.9),
        ];
        if f64::from(v) <= 0.10 {
            return Err(NO_SIGNAL_CAPTURED.to_string());
        }
        Ok(fs_loud(piecewise_lufs(&ANCHORS, v)))
    }

    /// Retargeted from the original −68.0 to −72.0 LUFS: the law-predicted seed 2 is what
    /// now drives this fixture into the silent zone in the first place — at −68.0 the
    /// prediction (0.127, computed from seed 1's −62.15 LUFS reading) stayed just above the
    /// 0.10 cliff, so NO capture ever went silent and the fixture stopped exercising
    /// `fs_silent_geometry` at all. At −72.0 the same prediction formula lands at ≈0.080
    /// (silent), and the correction loop that follows keeps re-probing the silent zone at
    /// slightly different `v` for several iterates BEFORE crossing back into real territory
    /// — a direct exercise of `min_real`'s running-floor property (every real capture on
    /// this curve sits below −50, so a pseudo point anchored at the FIXED
    /// `FS_SILENT_GEOMETRY_LUFS` sentinel rather than `min_real - FS_SILENT_MARGIN_LU` would
    /// sit ABOVE the real points and invert the secant's slope). UNCHANGED by the
    /// `FS_MIN_SEED_GAP_LU` fix: both gates agree here (the 9.85 LU expected gap clears
    /// `FS_MIN_SEED_GAP_LU`, and 0.080 sits inside the central 5%-95% frac window), so this
    /// fixture's target and behavior needed no retune. Provably reaches the arm: at least
    /// two DISTINCT probed values sit in the silent zone.
    #[test]
    fn solve_footswitch_silence_sentinel_stays_below_real_captures_on_quiet_chains() {
        let seen: std::cell::RefCell<Vec<f32>> = std::cell::RefCell::new(Vec::new());
        let r = solve_footswitch(
            26,
            &[],
            &[],
            -72.0,
            "baked",
            None,
            &fs_unit_param(),
            |_, v| {
                seen.borrow_mut().push(v);
                deep_quiet_pedal_curve(v)
            },
        )
        .expect("a silent probe on a quiet chain must be data, never an abort");
        let seen = seen.borrow();
        let silent_probes: Vec<f32> = seen.iter().copied().filter(|&v| v <= 0.10).collect();
        assert!(
            silent_probes.len() >= 2,
            "fs_silent_geometry's min_real floor needs REPEATED silent probes (not a \
             one-off) to prove it: {seen:?}"
        );
        assert!(
            silent_probes
                .iter()
                .any(|&a| silent_probes.iter().any(|&b| (a - b).abs() > 1e-6)),
            "the repeated silent probes must land at DISTINCT knob values, not the same \
             point re-measured: {seen:?}"
        );
        assert!(
            (r.predicted_lufs - -72.0).abs() <= KNOB_TOL_LU,
            "expected convergence near -72.0: got {} at v={} ({} captures) — a \
             sentinel ABOVE the real captures inverts the secant's slope",
            r.predicted_lufs,
            r.final_value,
            seen.len()
        );
        assert!(
            !r.clamped && !r.unconverged,
            "the -72 target sits on the curve's cliff, reachable: {r:?}"
        );
        assert!(
            seen.len() as u32 <= 3 + FS_CORRECT_MAX,
            "captures={}",
            seen.len()
        );
    }

    // Regression: an HONESTLY flat response (never silent anywhere) must keep the
    // reason-less headroom/no-authority clamp — the fix must not turn a genuine flat
    // response into a spurious silence-driven convergence.
    #[test]
    fn solve_footswitch_flat_response_without_silence_keeps_honest_clamp() {
        let mut captures = 0u32;
        let r = solve_footswitch(
            25,
            &[],
            &[],
            -26.0,
            "baked",
            None,
            &fs_unit_param(),
            |_, _v| {
                captures += 1;
                Ok(fs_loud(-17.0))
            },
        )
        .expect("solve");
        assert!(
            r.clamped && !r.unconverged,
            "an honestly flat (never silent) response must clamp, not advertise a re-run: {r:?}"
        );
        assert_eq!(
            r.clamp_reason, None,
            "a headroom/no-authority clamp carries NO reason (that belongs to the seed's \
             routing probe alone): {r:?}"
        );
        assert!(captures <= 3 + FS_CORRECT_MAX, "captures={captures}");
    }

    // ── log-space coordinate map + law-predicted seed 2 ─────────────────────────────────

    /// The linear-amplitude law `L = 20·log10(v) + C` — proven exact for `presetLevel`,
    /// amp `outputLevel` (scene lane), and the VolumePedal DSP (`out = in × g`) — is
    /// solvable from ONE live point: seed 1 fixes `C`, seed 2 lands on target. The old
    /// knob-space secant instead crept ~1 LU per capture (HW, MythicDrive FS:
    /// −30 → −22 → −24 → −25 → −26 over 5 captures). Each capture is a real ~15–20 s
    /// re-amp measurement, so the budget IS the feature: seed 1 + the exact law-predicted
    /// seed 2 = 2 captures, budget 3 gives noise headroom.
    #[test]
    fn solve_footswitch_log_law_curve_solves_in_three_captures() {
        let mut captures = 0u32;
        let r = solve_footswitch(
            12,
            &[],
            &[],
            -20.0,
            "baked",
            None,
            &fs_unit_param(),
            |_, v| {
                captures += 1;
                Ok(fs_loud(20.0 * f64::from(v.max(1e-4)).log10() - 14.0))
            },
        )
        .expect("solve");
        assert!(
            (r.predicted_lufs - -20.0).abs() <= FS_TOL_LU,
            "must land within FS_TOL_LU: best {} LUFS at v={} ({captures} captures)",
            r.predicted_lufs,
            r.final_value,
        );
        assert!(!r.clamped && !r.unconverged, "converged solve: {r:?}");
        assert!(captures <= 3, "captures={captures}");
    }

    /// The idempotency probe's capture is a PAID measurement: when it misses target it
    /// must join the solve as seed 1 (it proved signal, so it owns no routing verdict —
    /// that stays with the fixed seed on the probe-less path), not be discarded and
    /// re-bought. Probe + law-placed seed 2 = 2 captures on an exact-law curve — the SAME
    /// count as the no-probe path (`solve_footswitch_log_law_curve_solves_in_three_captures`);
    /// the probe replaces seed 1 rather than adding to it, so it is never MORE captures,
    /// only ever fewer than main's PRE-this-change discard-and-rebuy behavior (which paid
    /// 3: the wasted probe, seed 1, seed 2).
    #[test]
    fn solve_footswitch_reuses_the_idempotency_probe_as_a_seed() {
        let mut captures = 0u32;
        let r = solve_footswitch(
            12,
            &[],
            &[],
            -20.0,
            "baked",
            Some(0.3),
            &fs_unit_param(),
            |_, v| {
                captures += 1;
                Ok(fs_loud(20.0 * f64::from(v.max(1e-4)).log10() - 14.0))
            },
        )
        .expect("solve");
        assert!(
            (r.predicted_lufs - -20.0).abs() <= FS_TOL_LU,
            "must land within FS_TOL_LU: best {} LUFS at v={} ({captures} captures)",
            r.predicted_lufs,
            r.final_value,
        );
        assert!(!r.clamped && !r.unconverged, "converged solve: {r:?}");
        assert_eq!(captures, 2, "probe replaces seed 1: exactly 2 captures");
        assert_eq!(
            r.iterations, 2,
            "the probe counts as capture 1, so `iterations` is 2 after seed 2 — the same \
             accounting as the no-probe path"
        );
    }

    /// A param whose loudness DECREASES with knob value — names imply neither direction
    /// nor monotonicity, so the log-space transform must stay a pure monotone
    /// reparameterization (a misprediction falls back to the fixed seed pair, or a
    /// straddling bracket recovers) and never a direction assumption. Must converge inside
    /// the standard fallback-seed budget.
    #[test]
    fn solve_footswitch_inverted_response_still_converges() {
        let mut captures = 0u32;
        let r = solve_footswitch(
            12,
            &[],
            &[],
            -20.0,
            "baked",
            None,
            &fs_unit_param(),
            |_, v| {
                captures += 1;
                Ok(fs_loud(-26.0 + 12.0 * (1.0 - f64::from(v))))
            },
        )
        .expect("solve");
        assert!(
            (r.predicted_lufs - -20.0).abs() <= FS_TOL_LU,
            "must land within FS_TOL_LU: best {} LUFS at v={} ({captures} captures)",
            r.predicted_lufs,
            r.final_value,
        );
        assert!(!r.clamped && !r.unconverged, "converged solve: {r:?}");
        assert!(captures <= 2 + 1 + FS_CORRECT_MAX, "captures={captures}");
    }

    /// Flat response with a `current_value` probe: seeding from the probe + a law-placed
    /// (or fixed-fallback) second seed must not shrink the pair below the span the
    /// no-authority proof needs — the verdict stays the reason-less clamp (never
    /// "unconverged"), exactly as on the probe-less path.
    #[test]
    fn solve_footswitch_flat_response_with_probe_still_reports_clamped() {
        let mut captures = 0u32;
        let r = solve_footswitch(
            12,
            &[],
            &[],
            -20.0,
            "baked",
            Some(0.5),
            &fs_unit_param(),
            |_, _| {
                captures += 1;
                Ok(fs_loud(-27.0))
            },
        )
        .expect("solve");
        assert!(
            r.clamped && !r.unconverged,
            "flat response must clamp, not advertise a re-run: {r:?}"
        );
        assert!(captures <= 2 + 1 + FS_CORRECT_MAX, "captures={captures}");
    }

    /// `FS_MIN_SEED_GAP_LU`'s whole reason to exist: the OLD `FS_MIN_SEED_SPAN_FRAC` gate
    /// (raw v-space span ≥ 12% of the range) wrongly rejected a correct law prediction at a
    /// LOW knob value, because the log-knob map compresses hardest exactly there — seed 1
    /// at `v ≈ 0.05` needing a genuine ~6 LU correction moves `v` by only ~0.05, well under
    /// a 12%-of-`[0,1]` span, so the feature silently no-op'd in the exact regime it was
    /// built for. The new LU-gap gate accepts it instead. Uses a probe seed (`current_value:
    /// Some(0.05)`) so seed 1 sits at 0.05 directly rather than the fixed 0.25 fraction, on
    /// an EXACT log-amplitude law (`L = 20·log10(v)`) so the law-predicted seed 2 lands
    /// exactly on target — solved in 2 captures (the probe + the accepted, non-fallback
    /// seed 2), never reaching the correction loop at all.
    #[test]
    fn solve_footswitch_accepts_a_quiet_knob_law_prediction_the_old_span_gate_rejected() {
        let seen: std::cell::RefCell<Vec<f32>> = std::cell::RefCell::new(Vec::new());
        let r = solve_footswitch(
            27,
            &[],
            &[],
            -20.0,
            "baked",
            Some(0.05),
            &fs_unit_param(),
            |_, v| {
                seen.borrow_mut().push(v);
                Ok(fs_loud(20.0 * f64::from(v).log10()))
            },
        )
        .expect("solve");
        let seen = seen.borrow();
        assert_eq!(
            seen[0], 0.05,
            "the probe supplies seed 1 directly, at the quiet value"
        );
        assert!(
            seen.contains(&0.1),
            "the law-predicted (non-fallback) seed 2 must actually be probed — the fixed \
             fallback for a seed 1 this low in the range would be 0.75, never 0.1: {seen:?}"
        );
        assert!(
            (r.predicted_lufs - -20.0).abs() <= FS_TOL_LU,
            "must land within FS_TOL_LU: best {} LUFS at v={} ({} captures)",
            r.predicted_lufs,
            r.final_value,
            seen.len()
        );
        assert!(!r.clamped && !r.unconverged, "converged solve: {r:?}");
        assert!(seen.len() as u32 <= 3, "captures={}", seen.len());
    }

    /// Round-trip identity that makes [`FsParamTarget::coord_to_value`]'s floor safe: for
    /// every coordinate at or above `to_coord`'s own `-60` floor, mapping back through
    /// `coord_to_value` and forward again through `to_coord` must reproduce it exactly —
    /// otherwise a correction-loop coordinate near the floor could drift to a DIFFERENT
    /// real, paid knob value on every iterate instead of collapsing cleanly onto the floor.
    #[test]
    fn coord_round_trips_exactly_at_and_above_the_log_floor() {
        let param = fs_unit_param();
        for u in [-60.0, -59.999, -40.0, -20.0, -12.041, -0.5, 0.0] {
            let v = param.coord_to_value(u);
            assert!(
                (param.to_coord(v) - u).abs() < 1e-9,
                "to_coord(coord_to_value({u})) should reproduce {u} exactly, got {} via v={v}",
                param.to_coord(v)
            );
        }
        // Below the floor, `coord_to_value` clamps to `KNOB_LOG_FLOOR` rather than emitting
        // a real, distinct value per coordinate — two different sub-floor coordinates must
        // collapse to the identical `v`.
        assert_eq!(param.coord_to_value(-70.0), param.coord_to_value(-90.0));
        assert_eq!(param.coord_to_value(-70.0), KNOB_LOG_FLOOR);
    }

    /// The RESCUE half of `FS_MIN_SEED_GAP_LU`'s safety guarantor, which every other
    /// accepted-prediction fixture leaves unexercised (they all end in a clamp): seed 1
    /// (0.25, −24.0) and the law-predicted, ACCEPTED seed 2 (≈0.284 — gap 1.1 LU ≥
    /// `FS_MIN_SEED_GAP_LU`, frac ≈0.28 inside 5%–95% — land in the −23.9 flat plateau
    /// [0.28, 0.6], so the pair measures ~0.1 LU flat (< `KNOB_TOL_LU`) even though this
    /// prediction was accepted, not rejected. `fs_bracket_expansion`'s entry condition
    /// fires; the target (−22.9) sits ABOVE the flat plateau's loudness, so it probes the
    /// HI extreme (1.0) and finds REAL slope there (−18.0), rescuing the pair into a
    /// genuine converged solve rather than a false no-authority clamp. Provably reaches
    /// the arm: `seen[1]` is the accepted prediction (not the 0.75 fixed fallback),
    /// `seen[2] == 1.0` (the expansion probe, not a seed).
    #[test]
    fn solve_footswitch_accepted_flat_pair_is_rescued_by_bracket_expansion() {
        fn accepted_flat_pair_curve(v: f32) -> Result<lufs::Loudness, String> {
            let v = f64::from(v);
            let l = if v <= 0.25 {
                -24.0
            } else if v < 0.28 {
                let frac = (v - 0.25) / (0.28 - 0.25);
                -24.0 + frac * (-23.9 - -24.0)
            } else if v <= 0.6 {
                -23.9
            } else {
                let frac = (v - 0.6) / (1.0 - 0.6);
                -23.9 + frac * (-18.0 - -23.9)
            };
            Ok(fs_loud(l))
        }
        let seen: std::cell::RefCell<Vec<f32>> = std::cell::RefCell::new(Vec::new());
        let r = solve_footswitch(
            40,
            &[],
            &[],
            -22.9,
            "baked",
            None,
            &fs_unit_param(),
            |_, v| {
                seen.borrow_mut().push(v);
                accepted_flat_pair_curve(v)
            },
        )
        .expect("solve");
        let seen = seen.borrow();
        assert_eq!(seen[0], 0.25, "seed 1 is the fixed quarter-range point");
        assert!(
            (0.26..0.6).contains(&seen[1]),
            "seed 2 must be the ACCEPTED law prediction landing in the flat plateau, not \
             the 0.75 fixed fallback: {seen:?}"
        );
        assert_eq!(
            seen.get(2),
            Some(&1.0),
            "the accepted-but-flat-measured pair must trigger `fs_bracket_expansion`'s hi \
             extreme probe, not a mid-loop secant step: {seen:?}"
        );
        assert!(
            (r.predicted_lufs - -22.9).abs() <= KNOB_TOL_LU,
            "the extreme probe's real slope must let the secant converge: best {} LUFS at \
             v={} ({} captures)",
            r.predicted_lufs,
            r.final_value,
            seen.len()
        );
        assert!(
            !r.clamped && !r.unconverged,
            "a REAL slope at the extreme must rescue the solve, never a false no-authority \
             clamp: {r:?}"
        );
        assert!(
            seen.len() as u32 <= 3 + FS_CORRECT_MAX,
            "captures={}",
            seen.len()
        );
    }

    /// The original Plumes incident (HW-reproduced, fw 1.8.45, preset "TR+BD2+BMP") AT ITS
    /// REAL in-UI-range target: the re-pinned mid-loop-silence test above had to move to
    /// −40.0 (outside any real target range) to keep exercising that specific arm once the
    /// law-predicted seed 2 started landing for real — but that leaves the ORIGINAL −26.0
    /// shape uncovered. There, the accepted prediction (gap 3.85 LU, frac ≈0.16 — both
    /// gates pass) lands at v≈0.16, where `plumes_level_curve`'s REAL response reads
    /// ~16 LU off the idealized log-amplitude law it was predicted from (a piecewise HW
    /// curve, not the exact law `solve_footswitch_log_law_curve_solves_in_three_captures`
    /// uses) — this is the only convergence coverage for an ACCEPTED-but-badly-wrong
    /// prediction on a real, non-ideal curve; every other accepted-prediction test uses an
    /// exact law curve where the prediction is exact by construction. Provably reaches the
    /// arm: `seen[1]` is the accepted prediction (~0.16), not the 0.75 fixed fallback.
    #[test]
    fn solve_footswitch_plumes_accepted_prediction_still_converges_from_a_bad_first_guess() {
        let seen: std::cell::RefCell<Vec<f32>> = std::cell::RefCell::new(Vec::new());
        let r = solve_footswitch(
            41,
            &[],
            &[],
            -26.0,
            "baked",
            None,
            &fs_unit_param(),
            |_, v| {
                seen.borrow_mut().push(v);
                plumes_level_curve(v)
            },
        )
        .expect("solve");
        let seen = seen.borrow();
        assert_eq!(seen[0], 0.25, "seed 1 is the fixed quarter-range point");
        assert!(
            (0.10..0.25).contains(&seen[1]),
            "seed 2 must be the ACCEPTED law prediction (~0.16), not the 0.75 fixed \
             fallback: {seen:?}"
        );
        assert!(
            (r.predicted_lufs - -26.0).abs() <= KNOB_TOL_LU,
            "the correction loop must recover from a badly-wrong-but-accepted first guess: \
             best {} LUFS at v={} ({} captures)",
            r.predicted_lufs,
            r.final_value,
            seen.len()
        );
        assert!(
            !r.clamped && !r.unconverged,
            "a reachable target must converge, not clamp or run out of budget: {r:?}"
        );
        assert!(
            seen.len() as u32 <= 3 + FS_CORRECT_MAX,
            "captures={}",
            seen.len()
        );
    }

    // ── param-class-driven solve: refusal, bounds, wet floor ────────────────────────────

    // ENTRY GUARD: a param the classifier answers `Other` for is not a level control.
    // Sweeping it would change the sound the player wrote, so the solve must refuse BEFORE
    // any device work — no capture may be requested at all.
    #[test]
    fn solve_footswitch_refuses_a_param_that_is_not_a_level_control() {
        let mut captures = 0u32;
        // `intensity` is in neither the defaults nor any block override ⇒ Other.
        let param = FsParamTarget::new("ACD_TremoloBias", "intensity", 0.5);
        assert_eq!(param.info.class, crate::param_class::ParamClass::Other);
        let err = solve_footswitch(3, &[], &[], -20.0, "baked", None, &param, |_, _| {
            captures += 1;
            Ok(fs_loud(-20.0))
        })
        .expect_err("an Other-classified param must refuse");
        assert_eq!(captures, 0, "the refusal must precede every device capture");
        assert!(
            err.contains("intensity")
                && err.contains("ACD_TremoloBias")
                && err.contains("not a level control"),
            "the refusal must name the param, the block and the cause: {err}"
        );
        // The block-scoped override trap rides the same guard: `level` is a level_linear
        // DEFAULT everywhere, but on the TM Rumble it is an amp knob that must never be swept.
        let trapped = FsParamTarget::new("ACD_TMRumbleV3", "level", 0.5);
        assert!(
            solve_footswitch(3, &[], &[], -20.0, "baked", None, &trapped, |_, _| Ok(
                fs_loud(-20.0)
            ))
            .is_err()
        );
    }

    // BOUNDS: params are no longer all `[0,1]`. On a raw-dB `[0, 12]` control the seeds land
    // at a quarter/three quarters of THAT range (3.0 / 9.0), the secant clamps to it, and a
    // solved value above 1.0 survives instead of being silently pinned.
    #[test]
    fn solve_footswitch_solves_in_the_params_own_db_range() {
        let param = FsParamTarget::new("ACD_Boost", "gain", 2.5);
        assert_eq!(param.info.class, crate::param_class::ParamClass::LevelDb);
        assert_eq!(param.bounds(), (0.0, 12.0));
        let seen: std::cell::RefCell<Vec<f32>> = std::cell::RefCell::new(Vec::new());
        // ~1:1 dB→LUFS, HW-verified for this block: -30 LUFS at 0 dB.
        let r = solve_footswitch(0, &[], &[], -22.0, "baked", None, &param, |_, v| {
            seen.borrow_mut().push(v);
            Ok(fs_loud(-30.0 + f64::from(v)))
        })
        .expect("solve");
        // Seed 1 is still the fixed quarter across the range (3.0). Seed 2 is now the
        // LAW-PREDICTED point, not the fixed three-quarters mark: `LevelDb` is ~1:1
        // dB→LUFS (identity `to_coord`/`coord_to_value`), so predicting from seed 1's -27.0 dB
        // reading toward the -22.0 target lands EXACTLY on 3.0 + (-22.0 - (-27.0)) =
        // 8.0 — a deliberate behavior change (the prediction is exact here), not a
        // weakened gate.
        assert_eq!(
            seen.borrow()[..2],
            [3.0, 8.0],
            "seed 1 stays the fixed quarter-range point; seed 2 is now law-predicted"
        );
        assert!(
            (r.final_value - 8.0).abs() < 0.05,
            "target -22 needs +8 dB, a value the old [0,1] clamp could never return: {r:?}"
        );
        assert!(!r.clamped && !r.unconverged, "8 dB is well inside [0,12]");
    }

    // ...and the bound-hit verdict follows the range too: pinned at 12.0 with the target
    // still louder is UNREACHABLE, not an exhausted search.
    #[test]
    fn solve_footswitch_clamps_at_the_db_range_ceiling() {
        let param = FsParamTarget::new("ACD_Boost", "gain", 2.5);
        let r = solve_footswitch(0, &[], &[], -5.0, "baked", None, &param, |_, v| {
            Ok(fs_loud(-30.0 + f64::from(v)))
        })
        .expect("solve");
        assert!(
            (r.final_value - 12.0).abs() < 1e-3,
            "the solve must pin at the param's own ceiling: {r:?}"
        );
        assert!(
            r.clamped && !r.unconverged,
            "maxed with the target still louder is unreachable, not re-runnable: {r:?}"
        );
    }

    // WET FLOOR: a wet/mix control driven toward 0 REMOVES the effect rather than making it
    // quieter, so `bounds()` raises the low bound to `WET_FLOOR_FRACTION` × authored — the
    // solve can't even probe below it. The floored row is reported clamped with the
    // `wet_floor` flag — and deliberately WITHOUT `clamp_reason`, whose contract is "not
    // reaching USB 1/2" (the UI renders any reason as `offbranch`).
    #[test]
    fn solve_footswitch_floors_a_wet_mix_at_a_quarter_of_the_authored_value() {
        // Authored mix 0.80 ⇒ floor 0.20. The response wants far less to hit target.
        let param = FsParamTarget::new("ACD_Chorus", "mix", 0.8);
        assert_eq!(param.info.class, crate::param_class::ParamClass::WetMix);
        let r = solve_footswitch(0, &[], &[], -40.0, "baked", None, &param, |_, v| {
            Ok(fs_loud(-20.0 + 10.0 * f64::from(v)))
        })
        .expect("solve");
        assert!(
            (r.final_value - 0.2).abs() < 1e-6,
            "the solve must be floored at 25% of the authored 0.8: {r:?}"
        );
        assert!(
            r.clamped && !r.unconverged,
            "a floored row is an honest clamp, never a re-runnable miss: {r:?}"
        );
        assert_eq!(
            r.clamp_reason, None,
            "clamp_reason means 'not on USB 1/2' ONLY — a wet floor must not be rendered as \
             a routing failure: {r:?}"
        );
        assert!(
            r.wet_floor,
            "the floor's cause rides the wet_floor flag: {r:?}"
        );
        // With the floor folded into `bounds()`, the reported loudness is a REAL reading
        // of the written value — never an estimate at an unwritable point below the floor.
        assert!(
            (r.predicted_lufs - (-20.0 + 10.0 * f64::from(r.final_value))).abs() < 1e-9,
            "predicted_lufs must be the capture AT final_value: {r:?}"
        );
    }

    // The floor is RELATIVE, never an absolute 0.25: a solve that lands ABOVE the floor is
    // untouched and unflagged, and an authored 0.0 (already fully dry) constrains nothing.
    #[test]
    fn solve_footswitch_wet_floor_is_relative_and_only_binds_when_crossed() {
        let unfloored = solve_footswitch(
            0,
            &[],
            &[],
            -14.0,
            "baked",
            None,
            &FsParamTarget::new("ACD_Chorus", "mix", 0.8),
            |_, v| Ok(fs_loud(-20.0 + 10.0 * f64::from(v))),
        )
        .expect("solve");
        assert!(
            (unfloored.final_value - 0.6).abs() < 0.01 && !unfloored.clamped,
            "0.6 is above the 0.2 floor, so nothing is clamped or flagged: {unfloored:?}"
        );
        assert!(!unfloored.wet_floor);

        // Authored 0.0 ⇒ floor 0.0 ⇒ no constraint at all (never a hard 0.25).
        let dry = solve_footswitch(
            0,
            &[],
            &[],
            -40.0,
            "baked",
            None,
            &FsParamTarget::new("ACD_Chorus", "mix", 0.0),
            |_, v| Ok(fs_loud(-20.0 + 10.0 * f64::from(v))),
        )
        .expect("solve");
        assert!(
            dry.final_value <= 1e-6,
            "an authored-0.0 mix floors at 0.0, not at an absolute 0.25: {dry:?}"
        );
        assert!(
            !dry.wet_floor,
            "a floor that never RAISED the low bound is an ordinary range edge, not the \
             verify-by-ear advisory: {dry:?}"
        );
    }

    // The wet floor must anchor on the ENGAGED value, not the switch-OFF base the target was
    // constructed from — an existing assign's stored valueA is what the player actually dialed
    // in while engaged, and it reaches the solve as `current_value` (the re-run anchor), which
    // `solve_footswitch` folds into the target itself (`anchored`) so no call site can forget.
    // Base 0.05 (near-dry, switch-OFF `valueB`) with an existing engaged valueA of 0.9 must
    // floor at 0.9 × 25% = 0.225, not 0.05 × 25% = 0.0125 — the exact incident (chorus mix→0)
    // the anchor exists to prevent.
    #[test]
    fn wet_floor_anchors_on_the_existing_assigns_engaged_value_not_the_base() {
        let param = FsParamTarget::new("ACD_Chorus", "mix", 0.05);
        let r = solve_footswitch(0, &[], &[], -40.0, "assigned", Some(0.9), &param, |_, v| {
            Ok(fs_loud(-20.0 + 10.0 * f64::from(v)))
        })
        .expect("solve");
        assert!(
            (r.final_value - 0.225).abs() < 1e-6,
            "must floor at 25% of the ENGAGED 0.9, not the base 0.05: {r:?}"
        );
        assert!(r.clamped && !r.unconverged && r.wet_floor);
    }

    // Base-anchoring stays intact when there is no existing assign (`engaged: None`) — a fresh
    // assign floors on the base value exactly as before the anchor existed.
    #[test]
    fn wet_floor_anchor_is_a_noop_with_no_existing_assign() {
        let param = FsParamTarget::new("ACD_Chorus", "mix", 0.05);
        assert_eq!(param.anchored(None).authored, 0.05);
    }

    // The WRITE path must carry a raw-dB solved value VERBATIM: with params no longer all
    // `[0,1]`, a stray clamp anywhere between the solve and the wire would silently pin a
    // `+8 dB` boost at `1.0`. Pinned on the BAKE write, which goes out as `changeParameter`
    // (`proto::change_parameter`'s `field_f32`, no clamp — the only `clamp(0.0, 1.0)` in
    // `proto.rs` belongs to `setPresetLevel`, a different message). The ASSIGN write's
    // `valueA` is a plain `serde_json` float in the same function and is likewise unclamped.
    #[test]
    fn write_fs_values_carries_a_raw_db_value_past_one_unclamped() {
        let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
        let mut s = Session::from_transport(Box::new(sim.clone()));
        let pending = vec![FsPendingWrite {
            switch: 1,
            lev: ("G1".to_string(), "boost".to_string(), "gain".to_string()),
            write: FsWrite::Bake {
                clear_stale: None,
                mirror_scenes: vec![],
            },
            value: 8.0,
        }];
        write_fs_values_on_session(&mut s, 30, &pending, None).expect("write");
        assert_eq!(
            sim.param_write(crate::sim_device::SCENE_BASE, "G1", "boost", "gain"),
            Some(8.0),
            "a raw-dB +8 must reach the device as 8.0, never pinned to 1.0: {:?}",
            sim.events()
        );
    }

    // The three-state split (was ONE `clamped` flag with `clamp_reason: None` for both the
    // unreachable and the merely-unconverged case — the UI could only say "clamped", telling
    // a user to accept a number a re-run would have improved, and telling another to retry
    // something physically unreachable). Each row must produce a DISTINCT tuple.
    #[test]
    fn classify_fs_outcome_splits_a_bound_hit_from_an_exhausted_search() {
        let target = -20.0;
        for (label, best_v, best_lufs, want) in [
            // Knob maxed and target still louder → unreachable.
            ("maxed, target louder", 1.0, -26.0, (true, false)),
            // Knob at zero and target still quieter → unreachable.
            ("zeroed, target quieter", 0.0, -14.0, (true, false)),
            // Mid-range miss: captures ran out, the knob still has room → re-runnable.
            ("mid-range miss", 0.55, -26.0, (false, true)),
            ("mid-range overshoot", 0.55, -14.0, (false, true)),
            // On target (within FS_TOL_LU) → neither, even sitting on a bound. 0.05, not
            // 0.1 exactly — the exact boundary is float-rounding-sensitive (0.1's f64
            // representation makes `abs(-20.1 - -20.0)` land a hair ABOVE 0.1).
            ("on target", 1.0, -20.05, (false, false)),
            // At a bound but the miss is in the direction the knob CAN still move → the
            // search simply stopped early, so it is unconverged, not unreachable.
            ("maxed, target quieter", 1.0, -14.0, (false, true)),
        ] {
            assert_eq!(
                classify_fs_outcome(best_v, best_lufs, target, UNIT_BOUNDS),
                want,
                "{label}: v={best_v} lufs={best_lufs}"
            );
        }
    }

    // A `FsWrite::Bake` persists with `changeParameter`, which lands in the connection's
    // CURRENT scene — and the write session's own `load_preset` activates the preset's saved
    // `lastLoadedScene`. Without a base recall the solved value went into that scene's
    // overlay while base (what the leveler measured, and what the switch's off position
    // renders) kept its old value.
    #[test]
    fn write_fs_values_bakes_into_base_not_the_saved_scene() {
        let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
        let mut s = Session::from_transport(Box::new(sim.clone()));
        let pending = vec![FsPendingWrite {
            switch: 1,
            lev: (
                "G1".to_string(),
                "amp".to_string(),
                "outputLevel".to_string(),
            ),
            write: FsWrite::Bake {
                clear_stale: None,
                mirror_scenes: vec![],
            },
            value: 0.42,
        }];
        write_fs_values_on_session(&mut s, 30, &pending, None).expect("write");
        let ev = sim.events();
        let baked: Vec<i64> = ev
            .iter()
            .filter_map(|e| match e {
                crate::sim_device::SimEvent::ChangeParameter { scene, param, .. }
                    if param == "outputLevel" =>
                {
                    Some(*scene)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            baked,
            vec![crate::sim_device::SCENE_BASE],
            "the bake must land in BASE, not the saved scene 3: {ev:?}"
        );
    }

    // THE WRITER side of the param-func-without-valueType HW finding (fw 1.8.45 silently
    // discards a WHOLE imported preset at its lazy commit when any `func: "param"` ftsw
    // entry lacks `valueType` — see `notes/gotchas.md`'s entry of the same name). Pins the
    // ASSIGN branch's composed functionJson (the literal wire string a test can parse, per
    // `SimEvent::SetFootswitchAssignment`'s own doc) directly, so a refactor that threads
    // `FootswitchWriteSpec` "faithfully" but drops the field fails HERE instead of only
    // re-arming the discard on a later export/import cycle.
    #[test]
    fn write_fs_values_assign_composes_a_numeric_value_type() {
        let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
        let mut s = Session::from_transport(Box::new(sim.clone()));
        let pending = vec![FsPendingWrite {
            switch: 1,
            lev: ("G1".to_string(), "n1".to_string(), "level".to_string()),
            write: FsWrite::Assign {
                value_b: 0.2,
                spec: FootswitchWriteSpec {
                    function_index: 0,
                    color_a: 5,
                    color_b: 0,
                    custom_label: "TEST".to_string(),
                    link_group: 0,
                    is_active: false,
                    switch_type: 0,
                },
            },
            value: 0.7,
        }];
        write_fs_values_on_session(&mut s, 30, &pending, None).expect("write");
        let ev = sim.events();
        let sent = ev
            .iter()
            .find_map(|e| match e {
                crate::sim_device::SimEvent::SetFootswitchAssignment { function_json, .. } => {
                    Some(function_json.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("the assign must send a setFootswitchAssignment: {ev:?}"));
        let json: serde_json::Value = serde_json::from_str(&sent).expect("valid functionJson");
        assert!(
            json["valueType"].is_number(),
            "the composed param functionJson must carry a NUMERIC valueType, not omit it \
             or carry a string — its absence makes fw 1.8.45 silently discard the whole \
             preset on a later import: {sent}"
        );
    }

    // A bake on a device-authored scened preset is MASKED by every full-param scene overlay
    // (HW, Hiwatt slot 31: the overlays governed the DSP while base stayed untouched), so the
    // solved value must ALSO be written into each overlay that restated the base value —
    // after the base write, one recall per mirror scene, then the ORIGINAL `lastLoadedScene`
    // recalled so the save re-stamps it (HW: the FS save stamped 8 over the user's scene 3).
    #[test]
    fn write_fs_values_mirrors_the_bake_and_restores_the_saved_scene() {
        let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
        let mut s = Session::from_transport(Box::new(sim.clone()));
        let pending = vec![FsPendingWrite {
            switch: 1,
            lev: (
                "G1".to_string(),
                "amp".to_string(),
                "outputLevel".to_string(),
            ),
            write: FsWrite::Bake {
                clear_stale: None,
                mirror_scenes: vec![0, 2],
            },
            value: 0.42,
        }];
        write_fs_values_on_session(&mut s, 30, &pending, Some(3)).expect("write");
        let ev = sim.events();
        let writes: Vec<i64> = ev
            .iter()
            .filter_map(|e| match e {
                crate::sim_device::SimEvent::ChangeParameter { scene, param, .. }
                    if param == "outputLevel" =>
                {
                    Some(*scene)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            writes,
            vec![crate::sim_device::SCENE_BASE, 0, 2],
            "bake writes base first, then each mirror scene's overlay: {ev:?}"
        );
        // The LAST scene recall before the save must be the preset's original scene 3.
        let last_recall = ev
            .iter()
            .rev()
            .find_map(|e| match e {
                crate::sim_device::SimEvent::LoadScene(scene) => Some(*scene),
                _ => None,
            })
            .expect("a scene recall");
        assert_eq!(
            last_recall, 3,
            "the save must re-stamp the original lastLoadedScene: {ev:?}"
        );
    }

    // (A3, presetLevel lane) The reported measurement bug: `measure_c`'s captures set
    // `presetLevel` with NO scene recall, so on a preset whose saved `lastLoadedScene` is a
    // FS scene (HW: preset 28, scene 3) the "base" ceiling `C` was measured in that scene —
    // a different sound than the one being leveled. Base is a recall (wire slot 8), never an
    // omission.
    #[test]
    fn arm_measurement_recalls_base_before_a_preset_level_capture() {
        let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(30).expect("load_preset"); // activates saved scene 3, not base
        arm_measurement(&mut s, &LevelKnob::PresetLevel, 0.5, &[], None, None).expect("arm");
        let ev = sim.events();
        let recall = ev.iter().position(|e| {
            matches!(e, crate::sim_device::SimEvent::LoadScene(sc) if *sc == crate::session::BASE_SCENE_SLOT)
        });
        let set = ev
            .iter()
            .position(|e| matches!(e, crate::sim_device::SimEvent::PresetLevel(_)));
        assert!(
            recall.is_some() && recall < set,
            "the base recall must precede the presetLevel write: {ev:?}"
        );
    }

    // Isolation LAST: `load_scene` re-asserts the scene's own bypass state, so a forced
    // bypass written BEFORE the recall is silently reverted and the capture measures a
    // non-isolated sound. Every recall-bearing capture re-sends the full list after it.
    #[test]
    fn arm_measurement_writes_isolation_bypasses_after_the_scene_recall() {
        let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(30).expect("load_preset");
        let knob = LevelKnob::Block {
            group_id: "G1".into(),
            node_id: "amp".into(),
            parameter_id: "outputLevel".into(),
            scene_slot: None,
        };
        arm_measurement(
            &mut s,
            &knob,
            0.6,
            &[("G1".to_string(), "fx".to_string(), true)],
            None,
            None,
        )
        .expect("arm");
        let ev = sim.events();
        let recall = ev
            .iter()
            .rposition(|e| matches!(e, crate::sim_device::SimEvent::LoadScene(_)));
        let bypass = ev.iter().position(
            |e| matches!(e, crate::sim_device::SimEvent::Bypass { node, on } if node == "fx" && *on),
        );
        assert!(
            bypass.is_some() && recall < bypass,
            "the isolation bypass must be written AFTER the last scene recall: {ev:?}"
        );
    }

    // The intended `presetLevel` must land AFTER the knob write (whose own recall — base or
    // scene, inside `set_knobs` — runs the device's level-apply and would revert an earlier
    // one) and BEFORE the engage. Written above the `set_knob` call it is silently reverted
    // and the capture renders at the level the DEVICE HAS SAVED: the footswitch/scene lanes'
    // measured 10.2 dB error. `None` must stay byte-identical to the old behaviour.
    #[test]
    fn arm_measurement_asserts_the_intended_preset_level_after_the_knobs_recall() {
        let knob = LevelKnob::Block {
            group_id: "G1".into(),
            node_id: "amp".into(),
            parameter_id: "outputLevel".into(),
            scene_slot: None,
        };
        let arm = |intended: Option<f32>| {
            let sim = crate::sim_device::SimDevice::new().with_saved_scene(30, Some(3));
            let mut s = Session::from_transport(Box::new(sim.clone()));
            s.load_preset(30).expect("load_preset");
            arm_measurement(
                &mut s,
                &knob,
                0.6,
                &[("G1".to_string(), "fx".to_string(), true)],
                None,
                intended,
            )
            .expect("arm");
            sim.events()
        };

        let ev = arm(Some(0.42));
        let recall = ev
            .iter()
            .rposition(|e| matches!(e, crate::sim_device::SimEvent::LoadScene(_)));
        let level = ev
            .iter()
            .position(|e| matches!(e, crate::sim_device::SimEvent::PresetLevel(_)));
        let bypass = ev.iter().position(
            |e| matches!(e, crate::sim_device::SimEvent::Bypass { node, on } if node == "fx" && *on),
        );
        assert!(
            level.is_some() && recall < level && level < bypass,
            "the intended presetLevel must be written after the knob's recall and before the \
             isolation bypasses: {ev:?}"
        );
        assert!(
            matches!(
                ev.iter().find_map(|e| match e {
                    crate::sim_device::SimEvent::PresetLevel(v) => Some(*v),
                    _ => None,
                }),
                Some(v) if (v - 0.42).abs() < 1e-6
            ),
            "the asserted level must be the intended one: {ev:?}"
        );

        assert!(
            !arm(None)
                .iter()
                .any(|e| matches!(e, crate::sim_device::SimEvent::PresetLevel(_))),
            "`None` must write no presetLevel at all"
        );
    }

    // A `"bypass"` knob value (0.0/1.0-encoded) must route through `change_parameter_bool`
    // — the WIRE message is different (`ChangeParameter.boolVal`, field 7) from every other
    // block param, so smuggling it through the float `change_parameter` call would set the
    // wrong field and silently fail to write bypass at all.
    #[test]
    fn set_knob_value_only_routes_bypass_through_the_bool_wire_call() {
        let sim = crate::sim_device::SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(0).expect("load_preset");
        let bypass_knob = LevelKnob::Block {
            group_id: "G1".into(),
            node_id: "amp".into(),
            parameter_id: "bypass".into(),
            scene_slot: None,
        };
        set_knob(&mut s, &bypass_knob, 1.0, None).expect("set_knob");
        assert_eq!(
            sim.bypass_write("amp"),
            Some(true),
            "a bypass knob value must land as a BOOL write, not a float dspUnitParameters one"
        );
    }
}

#[cfg(test)]
mod reordered_run_tests {
    use super::*;
    use crate::headroom_trade::ClampKind;

    fn knob(node: &str, scene: Option<u32>, current: f32) -> KnobTarget {
        KnobTarget {
            knob: LevelKnob::Block {
                group_id: "G1".into(),
                node_id: node.into(),
                parameter_id: "outputLevel".into(),
                scene_slot: scene,
            },
            lo: 0.0,
            hi: 1.0,
            current,
        }
    }

    fn job(scene_slot: u32, target: f64, knobs: Vec<KnobTarget>) -> SceneJob {
        SceneJob {
            scene_slot,
            target_lufs: target,
            knobs,
            skip: None,
            rebalanceable: false,
            handle: None,
            prepass: None,
            force_bypass: Vec::new(),
        }
    }

    fn measured(job: SceneJob, asis: f64, spread: f64) -> SceneJob {
        SceneJob {
            prepass: Some(ScenePrepass { asis, spread }),
            ..job
        }
    }

    // THE REORDER'S LOAD-BEARING PROPERTY: a job that already carries its prepass reading
    // must resolve its prologue from that reading and NOT touch the device. There is no
    // hardware in a unit test, so a prologue that still measured would fail to connect —
    // this passing IS the proof the capture was skipped, and the values pass through
    // verbatim.
    #[test]
    fn a_job_carrying_a_prepass_reading_resolves_without_measuring() {
        let j = measured(job(2, -18.0, vec![knob("amp", Some(2), 0.5)]), -21.25, 4.5);
        let p = scene_prologue(&j, &[], None).expect("no device work");
        assert!((p.asis - -21.25).abs() < 1e-9, "{}", p.asis);
        assert!((p.spread - 4.5).abs() < 1e-9);
    }

    // The amp-fader ceiling IS an exact extrapolation: `outputLevel` is linear in dB with
    // full authority (HW), so the top of the range is `asis + 20·log10(LEVEL_MAX / max_cur)`
    // — the same `k_cap` the joint-k solver clamps to, so the two cannot disagree.
    #[test]
    fn the_scene_ceiling_extrapolates_the_fader_to_its_top_bound() {
        let j = measured(job(0, -20.0, vec![knob("amp", Some(0), 0.5)]), -26.0, 2.0);
        let c = scene_ceiling_lufs(&j).expect("an amp row has a ceiling");
        assert!((c - (-26.0 + 6.0206)).abs() < 1e-3, "{c}");
        // A fader already at the top has no headroom left: the ceiling IS the as-is reading.
        let maxed = measured(job(0, -20.0, vec![knob("amp", Some(0), 1.0)]), -26.0, 2.0);
        assert!((scene_ceiling_lufs(&maxed).expect("ceiling") - -26.0).abs() < 1e-9);
        // A parallel merge is bounded by the LOUDEST lane (the first to hit the cap).
        let merge = measured(
            job(
                0,
                -20.0,
                vec![knob("a", Some(0), 0.5), knob("b", Some(0), 0.25)],
            ),
            -26.0,
            2.0,
        );
        assert!((scene_ceiling_lufs(&merge).expect("ceiling") - (-26.0 + 6.0206)).abs() < 1e-3);
    }

    // A USER-HANDLE row answers `None` ON PURPOSE: an arbitrary block param has no
    // algebraically predictable response, so extrapolating one would be exactly the taper
    // model this codebase refuses to build. Same for a skip job and an unmeasured job.
    #[test]
    fn a_handle_row_a_skip_row_and_an_unmeasured_row_report_no_ceiling() {
        let mut handle_row = measured(job(1, -20.0, vec![knob("pedal", Some(1), 0.5)]), -26.0, 2.0);
        handle_row.handle = Some(FsParamTarget::new("ACD_Plumes", "level", 0.5));
        assert_eq!(scene_ceiling_lufs(&handle_row), None);

        let mut skip = measured(job(1, -20.0, Vec::new()), -26.0, 2.0);
        skip.skip = Some("no active amp".into());
        assert_eq!(scene_ceiling_lufs(&skip), None);

        assert_eq!(
            scene_ceiling_lufs(&job(1, -20.0, vec![knob("amp", Some(1), 0.5)])),
            None,
            "no prepass reading ⇒ no ceiling to report"
        );
    }

    // AFTER A TRADE, the two halves of the preset move differently and the re-target must
    // reflect exactly that: a benefiting sound gained the raise EXACTLY (module header), while
    // every other reading routed through the base FADER — not algebraically predictable
    // (module header) — and is therefore DROPPED so its own solve re-measures rather than
    // trusting a guess.
    #[test]
    fn a_landed_trade_shifts_benefiting_readings_and_drops_the_rest() {
        let mut jobs = vec![
            measured(job(0, -18.0, vec![knob("amp", Some(0), 1.0)]), -24.0, 2.0),
            measured(job(1, -18.0, vec![knob("amp", Some(1), 0.5)]), -20.0, 2.0),
            measured(
                job(
                    crate::session::BASE_SCENE_SLOT,
                    -18.0,
                    vec![knob("amp", None, 0.8)],
                ),
                -18.0,
                2.0,
            ),
        ];
        retarget_prepass_after_trade(&mut jobs, 4.0, |sc| sc == 0);
        assert_eq!(
            jobs[0].prepass.map(|p| p.asis),
            Some(-20.0),
            "the benefiting scene gained EXACTLY the raise"
        );
        assert_eq!(
            jobs[0].prepass.map(|p| p.spread),
            Some(2.0),
            "a pure gain shift leaves the dynamics spread alone"
        );
        assert_eq!(
            jobs[1].prepass, None,
            "a net-zero scene's reading routed through the fader — it must be re-measured"
        );
        assert_eq!(
            jobs[2].prepass, None,
            "base is never kept: its own hold already verified it at the new level"
        );
    }

    // THE CLAMP TAXONOMY, in one table. Order is load-bearing — a routing failure outranks a
    // wet floor, which outranks an ordinary headroom clamp — and an unclamped row never
    // names a cause at all.
    #[test]
    fn the_clamp_taxonomy_names_one_cause_per_shape() {
        assert_eq!(ClampKind::from_flags(false, false, None), None);
        assert_eq!(
            ClampKind::from_flags(false, true, Some("no signal on USB 1/2")),
            None,
            "not clamped ⇒ no cause, whatever the other flags say"
        );
        assert_eq!(
            ClampKind::from_flags(true, false, None),
            Some(ClampKind::SceneCeiling)
        );
        assert_eq!(
            ClampKind::from_flags(true, true, None),
            Some(ClampKind::WetFloor)
        );
        assert_eq!(
            ClampKind::from_flags(true, true, Some("no signal on USB 1/2")),
            Some(ClampKind::NoAuthority),
            "a sound that never reached USB 1/2 did not run out of wet floor"
        );
    }

    // ⟦A1⟧ AN AUTHOR-MUTED BASE LANE IS NOT A FLOOR PIN. A preset whose base carries a muted
    // amp (`outputLevel = 0`, the player's own choice) has a lane sitting at/below
    // `BASE_FADER_FLOOR` in EVERY solve — `joint_k_floor` deliberately lets it ride and the
    // joint factor never moves it. A hold that then stalls MID-RANGE (the bounded secant out
    // of captures, every audible lane still well inside its range) must report
    // `PartialTrade`: calling it `TradeFloor` both words the wrong cause on the wire and
    // spends the one bounded re-plan chasing fader room that was never the problem.
    #[test]
    fn a_muted_base_lane_does_not_turn_a_mid_range_stall_into_a_floor_pin() {
        use crate::headroom_trade::BASE_FADER_FLOOR;
        // The solve's own direction-aware answer: the muted lane was never MOVED to the floor,
        // and the audible lane sits mid-range — so no bound pinned this solve.
        let base = [0.0f32, 0.6];
        let pinned = joint_levels_pinned(
            &[0.0, 0.4],
            &base,
            BASE_FADER_FLOOR,
            -20.0, // achieved
            -23.0, // target: QUIETER than achieved, i.e. the direction the fader blocks
        );
        assert_eq!(
            pinned, None,
            "an author-muted lane is not a solved floor pin"
        );
        assert_eq!(
            trade_hold_failure_kind(Some(ClampKind::SceneCeiling), pinned),
            ClampKind::PartialTrade,
            "a mid-range stall backs the pair out; it did not run out of base fader"
        );
        // The GENUINE floor pin still reports itself — the fix must not blind the one retry.
        let floored = joint_levels_pinned(
            &[0.0, BASE_FADER_FLOOR],
            &base,
            BASE_FADER_FLOOR,
            -20.0,
            -23.0,
        );
        assert_eq!(floored, Some(PinnedBound::Floor));
        assert_eq!(
            trade_hold_failure_kind(Some(ClampKind::SceneCeiling), floored),
            ClampKind::TradeFloor
        );
        // Routing outranks everything: a sound that never reached USB 1/2 is no floor case.
        assert_eq!(
            trade_hold_failure_kind(Some(ClampKind::NoAuthority), floored),
            ClampKind::NoAuthority
        );
        // A TOP pin is not something a base hold can pay for either — it backs out.
        let maxed = joint_levels_pinned(&[1.0], &[0.6], BASE_FADER_FLOOR, -30.0, -23.0);
        assert_eq!(maxed, Some(PinnedBound::Max));
        assert_eq!(
            trade_hold_failure_kind(Some(ClampKind::SceneCeiling), maxed),
            ClampKind::PartialTrade
        );
    }

    // A prepass-decided clamp reports the LOUDEST the sound can actually be, at the handle
    // value that produced it, with the shared cause — and writes nothing.
    #[test]
    fn a_ceiling_clamp_reports_the_measured_ceiling_and_writes_nothing() {
        let handle = FsParamTarget::new("ACD_Plumes", "level", 0.5);
        let ceiling = FsCeiling {
            ceiling_lufs: -25.4,
            spread_lu: 3.1,
            unreachable: true,
        };
        let r = fs_result_from_ceiling(7, -18.0, &handle, &ceiling, "baked");
        assert!(r.clamped);
        assert_eq!(r.clamp_kind, Some(ClampKind::SceneCeiling));
        assert_eq!(r.clamp_reason, None, "a headroom clamp is reason-less");
        assert!(!r.wet_floor);
        assert!(!r.unconverged, "nothing was searched, so nothing stalled");
        assert!(!r.saved);
        assert_eq!(r.predicted_lufs, -25.4);
        assert_eq!(r.final_value, handle.bounds().1, "the handle's top bound");
        assert_eq!(r.iterations, 1, "the ONE prepass capture it rests on");
    }

    // THE CEILING PROBE'S PURE HALF. The capture puts the LEVELING HANDLE at the top of its
    // own (classified, wet-floor-aware) range and writes it LAST, so it wins over any `param`
    // function of the same switch addressing the same control — the ceiling is the handle at
    // its top, by definition. Every other function still rides at its ENGAGED (`valueA`) value,
    // because the ceiling has to describe the sound the switch actually makes.
    #[test]
    fn the_ceiling_probe_pins_the_handle_at_its_top_bound_and_writes_it_last() {
        let handle = FsParamTarget::new("ACD_Plumes", "level", 0.5);
        let states = crate::footswitch::SwitchStates {
            engaged_bypass: vec![("G1".into(), "ACD_Plumes".into(), false)],
            disengaged_bypass: vec![("G1".into(), "ACD_Plumes".into(), true)],
            params: vec![
                // A function on ANOTHER control rides at its engaged value…
                ("G1".into(), "ACD_Boost".into(), "gain".into(), 2.5, 0.0),
                // …and one on the HANDLE ITSELF is dropped: the top bound replaces it.
                ("G1".into(), "ACD_Plumes".into(), "level".into(), 0.4, 0.2),
            ],
        };
        let probe = FsCeilingProbe {
            scene: Some(2),
            states: &states,
            handle: ("G1".into(), "ACD_Plumes".into(), handle.clone()),
        };
        let params = probe.ceiling_params();
        let (_, hi) = handle.bounds();
        assert_eq!(
            params.last().map(|p| (p.1.as_str(), p.2.as_str(), p.3)),
            Some(("ACD_Plumes", "level", hi)),
            "the handle write is appended LAST, at the top of its range: {params:?}"
        );
        assert_eq!(
            params
                .iter()
                .filter(|(_, n, p, _)| n == "ACD_Plumes" && p == "level")
                .count(),
            1,
            "the switch's own function on the handle is replaced, not duplicated: {params:?}"
        );
        assert!(
            params
                .iter()
                .any(|(_, n, p, v)| n == "ACD_Boost" && p == "gain" && *v == 2.5),
            "every OTHER function keeps its ENGAGED value: {params:?}"
        );
    }

    // The skip margin is deliberately much wider than the lane's acceptance band: a ceiling
    // read at handle-max can clip, and a FALSE clamp is a silent product bug while a
    // needlessly-solved row is only slow.
    #[test]
    fn the_ceiling_skip_margin_leaves_marginal_rows_to_the_solve() {
        // The margin is wider than the FS lane's acceptance band by construction, so a row
        // that merely misses `FS_TOL_LU` still gets its solve.
        assert!(!fs_target_beyond_ceiling(-25.0, -25.0 + FS_TOL_LU));
        assert!(!fs_target_beyond_ceiling(
            -25.0,
            -25.0 + FS_CEILING_SKIP_MARGIN_LU
        ));
        assert!(fs_target_beyond_ceiling(
            -25.0,
            -25.0 + FS_CEILING_SKIP_MARGIN_LU + 0.01
        ));
        // A target BELOW the ceiling is always reachable, and a non-finite read never
        // decides anything (the solve takes over).
        assert!(!fs_target_beyond_ceiling(-25.0, -40.0));
        assert!(!fs_target_beyond_ceiling(f64::NEG_INFINITY, -18.0));
    }
}

/// The Doctor onset seam — see `doctor_onset`'s doc and the fixture's header
/// for the HW evidence (`fs13` `ACD_TMLargePlate` 65%-wet, `probe --doctor-fs
/// 407 13`, fw 1.8.45, 2026-08-24). `G0` composes whatever the PRODUCTION
/// pipeline actually calls, so it's rewritten (not just re-run) the moment the
/// production seam changes from the raw correlator to `doctor_onset` — see its
/// own doc for why that's intentional here.
#[cfg(test)]
mod onset_gate {
    use super::*;
    use crate::audio;
    use crate::doctor;
    use crate::test_support::{fs13_capture, plucky};

    const SR: u32 = 48_000;
    /// 2 ms hop at 48 kHz — matches the fixture's envelope resolution.
    const HOP: usize = 96;

    /// A synthetic stand-in for the real guitar-humbucker Doctor stimulus
    /// (not a bundled test asset) — padded exactly as production pads it.
    fn synthetic_padded_stim() -> Vec<f32> {
        doctor_stim_slice(plucky(6.0))
    }

    /// G0 — behavioral: the production composition (`doctor_onset` ->
    /// `tail_energy_ratio`) must land on the pinned-tail aligned ground truth
    /// the fixture's header records (`fixtures/fs13_wash_envelope_2ms.txt`),
    /// not the un-aligned or unpinned numbers the same header documents as
    /// the failure modes this fix replaces — this test's stimulus is a
    /// synthetic stand-in (see `synthetic_padded_stim`), not the real
    /// guitar-humbucker capture the fixture itself came from, so it targets
    /// the pinned-tail row, not the fixture's own correlator-run numbers.
    #[test]
    fn wash_capture_pinned_tail_matches_ground_truth_through_the_production_seam() {
        let capture = fs13_capture();
        let stim = synthetic_padded_stim();
        let onset = doctor_onset(&stim, &capture, SR);
        let ratio = doctor::tail_energy_ratio(
            &capture,
            SR,
            onset.body_len,
            onset.body_start,
            DOCTOR_TAIL_MS,
        );
        assert!(
            (ratio - (-9.54)).abs() < 0.2,
            "doctor_onset -> tail_energy_ratio should land within 0.2 dB of the \
             pinned-tail aligned ground truth (-9.54 dB); got {ratio:.2} \
             (source={:?} confident={} signal_start={} body_start={} body_len={})",
            onset.source,
            onset.confident(),
            onset.signal_start,
            onset.body_start,
            onset.body_len
        );
    }

    /// G1-equivalent: the low-level energy step alone finds the fixture's true
    /// onset (hop 115 = 230 ms = 11,040 samples) directly, independent of the
    /// `doctor_onset` composition above.
    #[test]
    fn energy_step_lands_at_230ms_on_the_fixture() {
        let capture = fs13_capture();
        let found = audio::estimate_signal_start(&capture, SR)
            .expect("a clear step should be found on the fixture");
        let expected = 230 * SR as usize / 1000; // 11,040 samples
        let err = (found as i64 - expected as i64).unsigned_abs() as usize;
        assert!(
            err <= HOP,
            "energy step at {found} samples vs expected {expected} (±1 hop)"
        );
    }

    /// Documents the flat-curve fact: the envelope correlator does NOT reliably
    /// align this wash capture — either it isn't confident, or it lands more
    /// than 5 ms from the true 30 ms lag (1,440 samples).
    #[test]
    fn correlator_on_the_wash_capture_is_not_the_aligning_source() {
        let capture = fs13_capture();
        let stim = synthetic_padded_stim();
        let (onset, confident) = audio::estimate_onset(&stim, &capture, SR);
        let expected = 30 * SR as usize / 1000; // 1,440 samples
        let err_ms = (onset as i64 - expected as i64).unsigned_abs() as f64 / SR as f64 * 1000.0;
        assert!(
            !confident || err_ms > 5.0,
            "correlator unexpectedly aligned the wash capture confidently \
             (onset={onset} vs expected {expected}, err {err_ms:.1} ms) — the flat-curve \
             premise this fix relies on no longer holds for this fixture"
        );
    }

    /// G3: a −40 dB white-noise floor ahead of the true onset must not be
    /// mistaken for the signal — the step is relative to the floor, not an
    /// absolute level.
    #[test]
    fn hiss_floor_before_the_onset_does_not_fool_the_energy_step() {
        let floor_amp = 0.01f32; // ~-40 dBFS peak
        let floor_len = SR as usize * 400 / 1000; // 400 ms of hiss
        let mut capture: Vec<f32> = (0..floor_len)
            .map(|i| ((i * 7919) % 1000) as f32 / 1000.0 * 2.0 * floor_amp - floor_amp)
            .collect();
        let step_at = capture.len();
        capture.extend(std::iter::repeat_n(0.5f32, SR as usize / 2)); // loud, 500 ms
        let found = audio::estimate_signal_start(&capture, SR)
            .expect("a clear step over a hiss floor should still be found");
        let err = (found as i64 - step_at as i64).unsigned_abs() as usize;
        assert!(
            err <= HOP,
            "step at {found} samples vs expected {step_at} (±1 hop) over a -40 dB floor"
        );
    }

    /// G4: a capture whose signal starts 20 ms BEFORE the pad "would" end (a
    /// negative-latency stream start) still gets a body split ending at the
    /// true `signal_start + body_len_full` — the truncated-pad case in
    /// `doctor_onset`'s doc.
    #[test]
    fn negative_latency_still_splits_at_the_true_body_end() {
        let pad = doctor_pad_samples(); // 200 ms
        let stim_padded = doctor_stim_slice(vec![0.0f32; doctor_stim_samples()]);
        let body_len_full = stim_padded.len() - pad;
        let signal_start_samples = pad - SR as usize * 20 / 1000; // 20 ms early
        let mut capture: Vec<f32> = (0..signal_start_samples)
            .map(|i| ((i * 7919) % 1000) as f32 / 1000.0 * 0.002 - 0.001) // tiny floor
            .collect();
        capture.extend(std::iter::repeat_n(0.5f32, SR as usize / 5)); // 200 ms loud
        let onset = doctor_onset(&stim_padded, &capture, SR);
        assert_eq!(onset.source, OnsetSource::Energy);
        assert!(onset.confident());
        let err = (onset.signal_start as i64 - signal_start_samples as i64).unsigned_abs() as usize;
        assert!(
            err <= HOP,
            "signal_start {} vs expected {signal_start_samples}",
            onset.signal_start
        );
        // The pad is truncated (clipped to 0), so body_start clips to 0 rather
        // than going negative.
        assert_eq!(onset.body_start, 0);
        assert_eq!(
            onset.body_end(),
            onset.signal_start + body_len_full,
            "body_end must be the true signal_start + the full body length"
        );
    }

    /// Pins `DOCTOR_ONSET_MIN_LATENCY_MS` at the derived `-50` and confirms a
    /// step at the detector's practical reach is still FOUND. `-50` is an
    /// envelope, not an attained bound: the floor window rounds UP to whole
    /// `ENERGY_FLOOR_HOP_MS` sub-hops (150 -> 160 ms), so a step inside that
    /// last sub-hop contaminates the floor reference and reads as None (sweep:
    /// found at <=41 ms early, None from 42 ms). The test uses `pad - 40 ms`
    /// (exactly the internal cutoff, zero contamination) as the robust point.
    #[test]
    fn negative_latency_at_the_reachable_bound_is_found() {
        assert_eq!(
            DOCTOR_ONSET_MIN_LATENCY_MS,
            -50,
            "the derivation must still land on -50 ms (pad {} - floor window {})",
            DOCTOR_PAD_MS,
            audio::ONSET_ENERGY_FLOOR_WINDOW_MS
        );
        let pad = doctor_pad_samples(); // 200 ms
        let stim_padded = doctor_stim_slice(vec![0.0f32; doctor_stim_samples()]);
        // 40 ms early = 160 ms into the capture — the internal floor-window
        // cutoff (see the doc comment above for why not the nominal 50 ms).
        let signal_start_samples = pad - SR as usize * 40 / 1000;
        let mut capture: Vec<f32> = (0..signal_start_samples)
            .map(|i| ((i * 7919) % 1000) as f32 / 1000.0 * 0.002 - 0.001) // tiny floor
            .collect();
        capture.extend(std::iter::repeat_n(0.5f32, SR as usize / 5)); // 200 ms loud
        let found = audio::estimate_signal_start(&capture, SR)
            .expect("a step at the detector's practical reach should be found");
        let err = (found as i64 - signal_start_samples as i64).unsigned_abs() as usize;
        assert!(
            err <= HOP,
            "signal_start {found} vs expected {signal_start_samples} (±1 hop)"
        );
        let onset = doctor_onset(&stim_padded, &capture, SR);
        assert_eq!(onset.source, OnsetSource::Energy);
    }

    /// A step 75 ms early (125 ms into the capture — inside the floor window,
    /// beyond the reachable bound) must NOT be reported as an energy onset:
    /// `estimate_signal_start` cannot see it (its search never runs there),
    /// and `doctor_onset` must not claim `OnsetSource::Energy` for it either.
    #[test]
    fn negative_latency_beyond_the_reachable_bound_falls_back() {
        let pad = doctor_pad_samples(); // 200 ms
        let stim_padded = doctor_stim_slice(vec![0.0f32; doctor_stim_samples()]);
        // 75 ms early = 125 ms into the capture — inside the floor window.
        let signal_start_samples = pad - SR as usize * 75 / 1000;
        let mut capture: Vec<f32> = (0..signal_start_samples)
            .map(|i| ((i * 7919) % 1000) as f32 / 1000.0 * 0.002 - 0.001) // tiny floor
            .collect();
        capture.extend(std::iter::repeat_n(0.5f32, SR as usize / 5)); // 200 ms loud
        assert!(
            audio::estimate_signal_start(&capture, SR).is_none(),
            "a step inside the floor window must not be found by the energy detector"
        );
        let onset = doctor_onset(&stim_padded, &capture, SR);
        assert_ne!(
            onset.source,
            OnsetSource::Energy,
            "doctor_onset must not claim an energy onset beyond the reachable bound"
        );
    }

    /// G5: a fully silent capture, and a capture that's loud from sample 0
    /// (no quiet pre-roll to step away from), must both fall through to the
    /// correlator/un-aligned path rather than reporting a bogus energy step.
    #[test]
    fn silent_or_hot_from_sample_zero_capture_falls_back() {
        let stim_padded = synthetic_padded_stim();

        let silent = vec![0.0f32; SR as usize];
        assert!(audio::estimate_signal_start(&silent, SR).is_none());
        let onset = doctor_onset(&stim_padded, &silent, SR);
        assert_ne!(onset.source, OnsetSource::Energy);

        let hot: Vec<f32> = (0..SR as usize)
            .map(|i| (2.0 * std::f32::consts::PI * 220.0 * i as f32 / SR as f32).sin() * 0.5)
            .collect();
        assert!(audio::estimate_signal_start(&hot, SR).is_none());
        let onset = doctor_onset(&stim_padded, &hot, SR);
        assert_ne!(onset.source, OnsetSource::Energy);
    }

    /// An energy step that fires WAY outside the plausible latency band (here,
    /// an uncorrelated loud burst over a second after the true, correlatable
    /// lag) must be ignored — `doctor_onset` falls through to the confident
    /// correlator result instead of trusting the out-of-band step.
    #[test]
    fn out_of_band_energy_onset_falls_through_to_the_correlator() {
        let stim = plucky(2.0); // 96,000 samples
        let true_lag = SR as usize * 32 / 1000; // 32 ms — in-band, correlatable
        let floor_amp = 0.02f32;
        let total_len = true_lag + stim.len() + SR as usize / 2;
        let mut capture: Vec<f32> = (0..total_len)
            .map(|i| ((i * 104_729) % 1009) as f32 / 1009.0 * floor_amp - floor_amp / 2.0)
            .collect();
        // A quiet (5%) correlatable copy of the stimulus at the true lag — its
        // RMS stays well under the energy step's threshold (verified below).
        for (i, &s) in stim.iter().enumerate() {
            capture[true_lag + i] += s * 0.05;
        }
        // A loud, UNCORRELATED burst ~1.5 s after the true lag (outside the
        // correlator's ~1.5 s head window) — the only thing that trips the
        // energy step, and well outside the plausible ±120 ms latency band.
        let burst_at = true_lag + SR as usize * 3 / 2;
        for v in capture.iter_mut().skip(burst_at).take(SR as usize / 50) {
            *v = 0.3;
        }
        let stim_padded = plucky(2.0); // stand-in "padded" stim (len only matters)
        let onset = doctor_onset(&stim_padded, &capture, SR);
        assert_ne!(
            onset.source,
            OnsetSource::Energy,
            "an out-of-band energy step must not be trusted"
        );
        assert_eq!(
            onset.source,
            OnsetSource::Correlator,
            "expected the confident correlator to win the fallthrough (got {:?})",
            onset.source
        );
        let (corr_onset, corr_confident) = audio::estimate_onset(&stim_padded, &capture, SR);
        assert!(corr_confident);
        assert_eq!(onset.body_start, corr_onset);
    }

    /// Offline e2e passthrough (`sim_device::e2e_capture`): a scaled copy of
    /// the padded stimulus with zero latency and no extra tail. `body_start`
    /// must land at 0 and the tail (nothing past the body) must floor at -80,
    /// exactly as before this change.
    #[test]
    fn sim_passthrough_zero_latency_no_tail_keeps_body_start_zero_and_floors_the_tail() {
        let stim_padded = doctor_stim_slice(plucky(3.0));
        let capture: Vec<f32> = stim_padded.iter().map(|&x| x * 0.7).collect();
        let onset = doctor_onset(&stim_padded, &capture, SR);
        assert_eq!(onset.body_start, 0);
        let ratio = doctor::tail_energy_ratio(
            &capture,
            SR,
            onset.body_len,
            onset.body_start,
            DOCTOR_TAIL_MS,
        );
        assert_eq!(ratio, -80.0);
    }
}
