//! Doctor capture seams: the Doctor's stimulus window and onset/body split
//! ([`doctor_onset`]), and its re-amp captures (fresh-connection, current-preset and
//! held-session forms). The leveling capture window is untouched by any of them.

use super::*;

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

/// Doctor capture tail for a chain WITHOUT a time effect (no reverb/delay node,
/// [`crate::doctor::has_time_effect`]): a bare settle guard, not a wash window —
/// `washed` cannot fire without a time-based block in the chain, so the full tail
/// buys nothing there. Shrinking the tail also shrinks `tail_ratio_db`'s window
/// (an empty/near-empty tail floors it at −80, `doctor::tail_energy_ratio`) and
/// marginally shifts `spread_lu` on these dry captures — expected and harmless
/// since `washed` is inapplicable by construction; the R4/R5 hardware sweeps
/// re-baseline the OTHER thresholds against this shorter recipe.
pub const DOCTOR_TAIL_DRY_MS: u32 = 300;

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
pub(super) const DOCTOR_ONSET_MAX_LATENCY_MS: i64 = audio::ONSET_MAX_PLAUSIBLE_LAG_MS as i64;
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
pub(super) const DOCTOR_ONSET_MIN_LATENCY_MS: i64 =
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
    slot: ListIndex,
    scene: Option<u32>,
    force_bypass: &[(String, String, bool)],
    fs_params: &[(String, String, String, f32)],
    stimulus: &[f32],
    ref_level: Option<f32>,
    tail_ms: u64,
    skip_load: bool,
) -> Result<(Vec<f32>, u32), LevelError> {
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
    slot: ListIndex,
    scene: Option<u32>,
    force_bypass: &[(String, String, bool)],
    fs_params: &[(String, String, String, f32)],
    stimulus: &[f32],
    ref_level: Option<f32>,
    tail_ms: u64,
    skip_load: bool,
    validate: Option<&crate::validate_log::ValidationRow>,
) -> Result<(Vec<f32>, u32, lufs::Loudness), LevelError> {
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
    // The Doctor diagnoses the preset's own stereo output: pinned to the stereo meter,
    // whatever the Settings playback-channels choice (that choice is a LEVELING meter).
    let loudness = measure_processed_as(&cap, crate::profiles::PlaybackChannels::Stereo)?;
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
pub(super) fn to_stereo(cap: audio::Capture) -> (Vec<f32>, u32) {
    let sr = cap.sample_rate;
    (cap.stereo_mix(), sr)
}

/// Doctor A/B AFTER-clip seam: capture the CURRENT live edit-buffer state WITHOUT
/// loading — a load would discard the unsaved `doctor_apply` prescription edit.
/// Delegates to `capture_full_at` with `skip_load: true` (its non-load branch is
/// byte-for-byte this: fresh-connect → (when `scene` is `Some`) re-activate that
/// 0-based `scenes[]` wire index on THIS connection → write `force_bypass`
/// isolation → optionally set the reference level BEFORE engaging → engage
/// re-amp once → capture with the Doctor tail → guaranteed re-amp off).
/// Deterministic stereo mixdown (`Capture::stereo_mix`, not
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
) -> Result<(Vec<f32>, u32), LevelError> {
    Ok(to_stereo(capture_full_at(
        ListIndex::default(), // slot unused: skip_load
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
) -> Result<(Vec<f32>, u32), LevelError> {
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
