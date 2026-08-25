//! Shared capture→Doctor-read pipeline: onset → body PSD → `SoundProfile` →
//! `band_db` → `deviations` → `tilt_split` → `diagnose_kind`, off ONE capture.
//! Extracted from `doctor_window_ab::capture_variant` and
//! `doctor_inject::measure`, which ran this same sequence independently —
//! kept as one seam so a metric-pipeline change can't drift between the two
//! probe arms. `doctor_calib.rs` runs its own (pre-existing, out of scope)
//! copy of a similar pipeline.
//!
//! The `pad_aware: true` arm's onset now goes through `leveller::doctor_onset`
//! (floor-relative energy step, primary — the correlator's confidence gate
//! can't detect a peakless correlation curve, HW-measured flat on a 65%-wet
//! reverb chain, fw 1.8.45); `pad_aware: false` keeps the raw `audio::estimate_onset`
//! correlator unchanged (it captures on the RAW unpadded stimulus, so the
//! padded stimulus the energy step needs isn't available there).

use crate::audio;
use crate::doctor;
use crate::leveller;

/// One capture's derived Doctor measurements — everything either probe arm
/// reads out of a capture.
pub(crate) struct DoctorRead {
    pub band_db: Vec<f64>,
    pub deviations: Vec<f64>,
    pub tilt_slope: Option<f64>,
    pub locals: Vec<f64>,
    pub tail_ratio_db: f64,
    pub spread_lu: f64,
    pub verdicts: Vec<&'static str>,
    /// Whether the onset split is trustworthy — `pad_aware: true` reads this
    /// off `leveller::doctor_onset` (energy step OR confident correlator);
    /// `pad_aware: false` reads it off the raw `audio::estimate_onset`
    /// correlator. Callers that care (e.g. to warn) read this instead of
    /// re-deriving it.
    pub onset_confident: bool,
    /// The capture's localized spectral peaks (`SoundProfile::peaks`,
    /// height-sorted) — printed by the inject arm so a resonant/boxy gate
    /// decision can be made from the measured height/Q, not just the verdict.
    pub peaks: Vec<crate::psd::SpectralPeak>,
}

/// Run the shared band/diagnosis pipeline over one capture. `stim` is
/// whatever stimulus slice the caller re-amped with — used both for onset
/// estimation and as the `stim_len` `SoundProfile::from_capture_with_psd`
/// needs; pass the SAME slice the capture was taken against. `tail_ms` is the
/// tail the caller actually captured with — threaded straight into
/// `doctor::tail_energy_ratio`'s pinned window.
///
/// `pad_aware` preserves each existing caller's onset handling exactly (the
/// two arms measured different stimulus shapes and diverged here before this
/// extraction): `doctor_inject` captures on the PADDED production stim
/// (`leveller::doctor_stim_slice`) and derives the body PSD's onset via
/// `leveller::doctor_onset` (skips the pre-roll silence) — pass
/// `pad_aware: true`. `doctor_window_ab` captures on the RAW, unpadded
/// calibrated stimulus and feeds the body PSD the estimated onset directly —
/// pass `pad_aware: false`. Both callers pass the raw `onset` (not the
/// pad-adjusted one) into `from_capture_with_psd`; only the PSD's own onset
/// differs.
pub(crate) fn analyze_capture(
    stim: &[f32],
    samples: &[f32],
    rate: u32,
    family: doctor::Family,
    pad_aware: bool,
    tail_ms: u32,
) -> Result<DoctorRead, String> {
    let (confident, psd_onset, body_len, body_start) = if pad_aware {
        let onset = leveller::doctor_onset(stim, samples, rate);
        (
            onset.confident(),
            onset.signal_start,
            onset.body_len,
            onset.body_start,
        )
    } else {
        let (onset, confident) = audio::estimate_onset(stim, samples, rate);
        (confident, onset, stim.len(), onset)
    };
    let body_psd = doctor::body_psd(samples, rate, psd_onset);
    let stim_psd = crate::psd::welch_psd(stim, rate as f32);
    let profile = doctor::SoundProfile::from_capture_with_psd(
        samples,
        rate,
        body_len,
        body_start,
        tail_ms,
        family,
        &body_psd,
        Some(&stim_psd),
    )?;
    let band_db = doctor::band_db(&profile.bands);
    let deviations = doctor::deviations(&band_db, family);
    // The CAPTURED OUTPUT's own coverage (mirrors `commands/doctor.rs`'s production
    // gate) — without it, low-energy bands skip the 30 dB confidence gate and can
    // false-fire. Computed before `tilt_split` so the REPORTED tilt/locals match
    // what `diagnose_kind` actually fires verdicts from, not an uncovered fit.
    let coverage = doctor::output_coverage_with_body(samples, rate, psd_onset, family, &body_psd);
    let (tilt_slope, locals) = doctor::tilt_split(&deviations, family, Some(&coverage));
    let verdicts: Vec<&'static str> = doctor::diagnose_kind(
        &profile,
        None,
        family,
        doctor::StimulusKind::Synthetic,
        Some(&coverage),
        doctor::PlaybackOffsets::NONE,
    )
    .into_iter()
    .map(|d| d.key)
    .collect();
    Ok(DoctorRead {
        band_db,
        deviations,
        tilt_slope,
        locals,
        tail_ratio_db: profile.tail_ratio_db,
        spread_lu: profile.spread_lu,
        verdicts,
        onset_confident: confident,
        peaks: profile.peaks,
    })
}
