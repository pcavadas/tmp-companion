//! Shared capture→Doctor-read pipeline: `estimate_onset` → body PSD →
//! `SoundProfile` → `band_db` → `deviations` → `tilt_split` → `diagnose_kind`,
//! off ONE capture. Extracted from `doctor_window_ab::capture_variant` and
//! `doctor_inject::measure`, which ran this same sequence independently —
//! kept as one seam so a metric-pipeline change can't drift between the two
//! probe arms. `doctor_calib.rs` runs its own (pre-existing, out of scope)
//! copy of a similar pipeline and is untouched.

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
    /// Whether `audio::estimate_onset` found the onset confidently — callers
    /// that care (e.g. to warn) read this instead of re-deriving it.
    pub onset_confident: bool,
}

/// Run the shared band/diagnosis pipeline over one capture. `stim` is
/// whatever stimulus slice the caller re-amped with — used both for onset
/// estimation and as the `stim_len` `SoundProfile::from_capture_with_psd`
/// needs; pass the SAME slice the capture was taken against.
///
/// `pad_aware` preserves each existing caller's onset handling exactly (the
/// two arms measured different stimulus shapes and diverged here before this
/// extraction): `doctor_inject` captures on the PADDED production stim
/// (`leveller::doctor_stim_slice`) and derives the body PSD's onset via
/// `leveller::doctor_signal_start` (skips the pre-roll silence) — pass
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
) -> Result<DoctorRead, String> {
    let (onset, confident) = audio::estimate_onset(stim, samples, rate);
    let psd_onset = if pad_aware {
        leveller::doctor_signal_start(onset, confident)
    } else {
        onset
    };
    let body_psd = doctor::body_psd(samples, rate, psd_onset);
    let profile = doctor::SoundProfile::from_capture_with_psd(
        samples,
        rate,
        stim.len(),
        onset,
        family,
        &body_psd,
    )?;
    let band_db = doctor::band_db(&profile.bands);
    let deviations = doctor::deviations(&band_db, family);
    let (tilt_slope, locals) = doctor::tilt_split(&deviations, family, None);
    let verdicts: Vec<&'static str> = doctor::diagnose_kind(
        &profile,
        None,
        family,
        doctor::StimulusKind::Synthetic,
        None,
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
    })
}
