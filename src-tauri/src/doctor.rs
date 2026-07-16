//! The Doctor diagnosis engine — PURE (no device I/O, no Tauri).
//!
//! Turns a re-amp capture's measurements (`SoundProfile`) into named tone
//! diagnoses (muddy / boomy / harsh / fizzy / washed / lost / buried / spiky /
//! dark / bright / thin) with concrete, graph-derived prescriptions (`Rx` →
//! `DoctorOp`s), plus the scene-loudness consistency check. The device work
//! (capture, apply) lives in `leveller::doctor_capture` and the `doctor_*`
//! commands; this module is the rules.
//!
//! ## Wire param casing (load-bearing)
//! Preset JSON serializes each parameter under the firmware schema's
//! `controlId`, NOT its display `name` (verified against the device-exported
//! `e2e/fixtures/scenario-presets.json`: `ACD_TMLargePlate.WetDryMix` →
//! `"wetdrymix"`, `ACD_TweedDeluxe.OutputLevel` → `"outputLevel"`). Every param
//! id below is the schema `controlId`, byte-verbatim.
//!
//! ## Block matching
//! Exact-FenderId membership only — never substring (`ACD_Freqout` is a
//! feedback pedal that substring-matches "eq"). The one documented exception
//! is [`has_time_effect`]'s conservative tail-length catch-all (see its doc
//! for why a false positive is harmless there). The `DIST_IDS` / `REVERB_MIX`
//! tables are extracted from the fw 1.8.45 embedded schemas
//! (`algoCategory == "dist" / "reverb"`); re-derive with the carve script if a
//! firmware rev adds blocks.
//!
//! ## Thresholds
//! Every rule constant lives in `Thresholds` (`GUITAR` / `BASS`) — calibrated
//! against real-library captures (probe --doctor); tune values there, never
//! inline numbers in rules.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::blockcaps;
use crate::profiles::PlaybackLevel;

/// The six player-named analysis bands (Hz) — the Guitar/Bass layout. Doctor-
/// specific; the legacy 4-band `spectrum::default_bands` stays untouched for the
/// older commands. 12 kHz top matches the practical capture ceiling `spectrum`
/// already uses.
const BANDS_6: [(f32, f32); 6] = [
    (60.0, 120.0),     // Lows
    (120.0, 400.0),    // Low-mids
    (400.0, 1000.0),   // Mids
    (1000.0, 3000.0),  // High-mids
    (3000.0, 6000.0),  // Highs
    (6000.0, 12000.0), // Air
];
const LABELS_6: [&str; 6] = ["Lows", "Low-mids", "Mids", "High-mids", "Highs", "Air"];

/// The Bass VI layout: a (30,60) "Sub" band under the standard six. Bass VI
/// fundamentals are E1 ≈ 41 Hz — invisible in the 60 Hz-floored 6-band layout —
/// so its lowest octave gets its own measured band. No rule keys on Sub yet
/// (PR-7 calibration decides); it is MEASURED and DISPLAYED only.
const BANDS_7: [(f32, f32); 7] = [
    (30.0, 60.0),      // Sub
    (60.0, 120.0),     // Lows
    (120.0, 400.0),    // Low-mids
    (400.0, 1000.0),   // Mids
    (1000.0, 3000.0),  // High-mids
    (3000.0, 6000.0),  // Highs
    (6000.0, 12000.0), // Air
];
const LABELS_7: [&str; 7] = [
    "Sub",
    "Lows",
    "Low-mids",
    "Mids",
    "High-mids",
    "Highs",
    "Air",
];

/// Rule constants — ALL of them, one place. dB values are deviations in
/// deviation-from-target space (a band's dB above the authored [`target_curve`],
/// after the Theil–Sen tilt/local split — see [`deviations`] / [`tilt_split`]).
///
/// ONE table per family (no more synthetic/capture split): the diagnosis metric
/// itself is deterministic per sound, and capture-space is discriminated by the
/// `fizzy` rule's [`FIZZY_MIN_FLATNESS`] gate, not a second threshold table. A
/// DI-specific recalibration of these values is an explicit follow-up (the R5
/// attended sweep). Any recalibration pass edits values here and nowhere else.
pub struct Thresholds {
    /// Low-mid excess ⇒ muddy.
    pub muddy_db: f64,
    /// Lows excess ⇒ boomy.
    pub boomy_db: f64,
    /// High-mid spike ⇒ harsh.
    pub harsh_db: f64,
    /// Air relative to the sound's OWN presence band (`bdb[Air] − bdb[Highs]`)
    /// ⇒ fizzy. A self-difference (not a tilt residual): real-library calibration
    /// showed the Air band is bimodal across a library (cab'd presets roll off
    /// 25–44 dB, open ones 10–20), so an absolute per-band deviation flags every
    /// open preset. Fizz is HF hash extending past 6 kHz — i.e. Air failing to
    /// roll off below the presence band, a property of the sound itself.
    pub fizzy_db: f64,
    /// Mids deficit (scoop) ⇒ lost in the mix.
    pub lost_db: f64,
    /// Post-stimulus tail RMS relative to the body (dB; closer to 0 = wetter)
    /// ⇒ washed out.
    pub wash_tail_db: f64,
    /// Lows deficit on a driven bass ⇒ buried clean tone (bass rule).
    pub buried_lows_db: f64,
    /// Dynamics spread (short-term-max − integrated LU) on a clean chain ⇒
    /// spiky. Baselined 2026-07-09 under the DOCTOR capture (then a 2.5 s tail;
    /// the R5 sweep re-derives every threshold under the 3 s + 1.5 s window):
    /// 0.12–0.81 LU across all 16 library sounds — the feared wet-preset tail
    /// inflation did not materialize, and `spiky` fires on zero library
    /// presets by design (see notes/doctor-calibration.md).
    pub spiky_spread_lu: f64,
    /// Scene-to-base loudness jump ⇒ scene-consistency flag.
    pub scene_delta_db: f64,
    /// Broadband Theil–Sen tilt magnitude (dB/oct) past which the whole tone
    /// reads dark (negative slope) or bright (positive slope).
    // ponytail: R5-calibrated for guitar (factory sweep); bass/bass-vi stay a
    // provisional +0.5 dB/oct extrapolation.
    pub tilt_db_per_oct: f64,
    /// Lows local deficit (dB, guitar only) past which a tone reads thin.
    // ponytail: R5-calibrated for guitar (factory sweep); thin is guitar-only
    // so there's no bass/bass-vi extrapolation to speak of.
    pub thin_db: f64,
    /// CONSENSUS gates (R5, HW defect-injection, `probe --doctor-inject`): a
    /// band rule fires only when its Theil–Sen tilt-split local (the `*_db`
    /// gates above) AND its [`centered_deviations`] value BOTH clear their own
    /// threshold — severity is `min(margin_tilt, margin_centered)`. The
    /// tilt-split local alone misattributes a skirted single-band defect (the
    /// Theil–Sen slope follows the smear into 2–3 neighboring bands, firing
    /// the WRONG rule); `centered_deviations` alone is contaminated by healthy
    /// broadband tilt at the endpoint bands. Neither space is individually
    /// trustworthy — the false-positive control lives in the INTERSECTION, so
    /// each space's gate is set independently at ~p75 of the healthy factory
    /// population (looser than a single combined max+margin gate would need,
    /// because consensus itself is the safety margin). See `notes/doctor.md`
    /// for the injection sweep this pair was derived from.
    pub muddy_centered_db: f64,
    pub boomy_centered_db: f64,
    pub harsh_centered_db: f64,
    pub lost_centered_db: f64,
    pub thin_centered_db: f64,
    pub buried_centered_db: f64,
}

/// Calibrated 2026-07-16 (R5, `probe --doctor-calib-factory`) against the same
/// 25-preset flagship factory-bank sweep that derived [`GUITAR_TARGET`]: every
/// band-rule gate is the healthy-population max plus the R4 repeatability
/// margin (3σ ≈ 1.2 dB for the band rules; tilt σ ≈ 0.05 dB/oct).
/// `wash_tail_db` sits in the NEW aligned-tail space (R4's onset fix moved dry
/// tails from ≈−19 to ≈−40 dB) — the factory wet cluster measures
/// −6.3…−7.8 dB and the wettest flagship preset −3.0 dB, which is meant to
/// fire. `spiky_spread_lu` is reachable only on Capture stimuli (the
/// synthetic-stimulus max observed across the sweep was 1.17 LU) — pending
/// the Tier-2 DI sweep.
///
/// R5 (2026-07-16, `probe --doctor-inject` HW defect injection): the tilt-space
/// band-rule gates below (`muddy_db`/`boomy_db`/`harsh_db`/`lost_db`/`thin_db`)
/// are LOWERED to their joint-calibration values now that they fire only in
/// CONSENSUS with the new `*_centered_db` gates (see [`Thresholds`]'s doc) —
/// on their own these looser values would over-fire; paired with the centered
/// gate, 3/3 injected single-band defects landed the right rule, the injected
/// false "thin" the old tilt-only metric produced on a skirted muddy defect is
/// vetoed, and the 25-preset factory bank fires on exactly 5 interpretable
/// presets. `buried_db` stays `INFINITY` (bass-only); its centered pair mirrors
/// that (see `BASS`/`GUITAR` split below — `buried_centered_db` is likewise
/// inert here).
pub const GUITAR: Thresholds = Thresholds {
    muddy_db: 2.0,
    boomy_db: 2.5,
    harsh_db: 2.5,
    fizzy_db: -9.0,
    lost_db: 3.5,
    wash_tail_db: -4.0,
    buried_lows_db: f64::INFINITY, // bass-only rule
    spiky_spread_lu: 4.0,
    scene_delta_db: 3.0,
    tilt_db_per_oct: 3.0,
    thin_db: 4.0,
    muddy_centered_db: 3.5,
    boomy_centered_db: 3.5,
    harsh_centered_db: 4.0,
    lost_centered_db: 3.5,
    thin_centered_db: 4.5,
    buried_centered_db: f64::INFINITY, // bass-only rule
};

/// PROVISIONAL — every band-rule gate +0.5 dB looser than [`GUITAR`] in BOTH
/// spaces (tilt and centered), pending a real bass factory sweep (mirrors
/// [`BASS_TARGET`]'s single-preset-anchor status): the honest starting point
/// until `probe --doctor-calib-factory` on a real bass library re-derives
/// these from their own population. `tilt_db_per_oct` (the whole-tone
/// dark/bright gate, not a band-rule gate) is untouched by the R5 consensus
/// retune.
pub const BASS: Thresholds = Thresholds {
    muddy_db: 2.5,
    boomy_db: 3.0,
    harsh_db: 3.0,
    fizzy_db: -9.0,
    lost_db: 4.0,
    wash_tail_db: -4.0,
    buried_lows_db: 3.0,
    spiky_spread_lu: 4.0,
    scene_delta_db: 3.0,
    tilt_db_per_oct: 3.5,
    thin_db: f64::INFINITY, // guitar-only rule
    muddy_centered_db: 4.0,
    boomy_centered_db: 4.0,
    harsh_centered_db: 4.5,
    lost_centered_db: 4.0,
    thin_centered_db: f64::INFINITY, // guitar-only rule
    buried_centered_db: 4.0,
};

/// PROVISIONAL — a straight copy of `BASS`, pending the attended Bass VI
/// calibration sweep (PR-7). Bass VI shares bass's low-fundamental character, so
/// bass thresholds are the honest starting point until `probe --doctor` on a
/// real Bass VI library re-derives them. (`thin_db` stays `INFINITY` here too,
/// inherited from `BASS` — thin is guitar-only.)
pub const BASS_VI: Thresholds = BASS;

/// Whether a sound was captured through the SYNTHETIC shaped-noise stimulus (the
/// Doctor default) or a real Tier-2 DI CAPTURE. Both diagnose against the SAME
/// per-family `Thresholds` table now — a real DI's HF hash reads as noise-like
/// broadband content rather than a systematic band-balance shift, so the one
/// place capture space needs different behavior is the `fizzy` rule: it gains an
/// extra [`FIZZY_MIN_FLATNESS`] gate on `Capture` (the synthetic shaped-noise
/// stimulus is noise-like everywhere, so the gate is inert there by
/// construction). The diagnosis metric itself ([`deviations`] / [`tilt_split`])
/// is deterministic per sound — this enum never pools measurements across
/// sounds. A DI-specific threshold recalibration is an explicit follow-up (see
/// `Thresholds` doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StimulusKind {
    Synthetic,
    Capture,
}

/// Per-rule threshold shifts (dB) added at COMPARISON time for the user's
/// playback level — a separate additive mechanism, never a mutation of the
/// pinned `Thresholds` consts. Fletcher–Munson: equal-loudness contours flatten
/// as SPL rises, so low-frequency (boomy/muddy) and mildly-HF (fizzy) content is
/// perceptually HOTTER at stage volume than at bedroom volume — Doctor should
/// flag it EARLIER at Stage (tighten → lower the effective threshold) and LATER
/// at Quiet (relax). Only these three rules shift; every other rule is SPL-blind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaybackOffsets {
    /// Added to `boomy_db` AND `muddy_db` before the `>` comparison.
    pub low_end_db: f64,
    /// Added to `fizzy_db` before the `>` comparison. NB `fizzy_db` is negative
    /// (−9.0) and the rule is `bal[air]−bal[highs] > fizzy_db`, so a NEGATIVE
    /// offset LOWERS the threshold and fires MORE easily (tighten).
    pub fizzy_db: f64,
}

impl PlaybackOffsets {
    /// No shift — the Rehearsal anchor and the `diagnose()` back-compat default.
    pub const NONE: PlaybackOffsets = PlaybackOffsets {
        low_end_db: 0.0,
        fizzy_db: 0.0,
    };
}

/// PROVISIONAL playback-level offsets, anchored at Rehearsal (= the assumed
/// calibration monitoring level, offset 0). Values are deliberately coarse and
/// PROVISIONAL — pending an SPL-anchored recalibration sweep that records monitor
/// SPL and re-derives them (see notes/doctor-calibration.md), the same convention
/// as the `*_CAPTURE` tables.
pub fn playback_offsets(level: PlaybackLevel) -> PlaybackOffsets {
    match level {
        // Stage is loudest → tighten: boomy/muddy −2.0, fizzy −1.0 (fires earlier).
        PlaybackLevel::Stage => PlaybackOffsets {
            low_end_db: -2.0,
            fizzy_db: -1.0,
        },
        PlaybackLevel::Rehearsal => PlaybackOffsets::NONE,
        // Quiet is softest → relax: boomy/muddy +2.0, fizzy +1.0 (fires later).
        PlaybackLevel::Quiet => PlaybackOffsets {
            low_end_db: 2.0,
            fizzy_db: 1.0,
        },
    }
}

/// The instrument family a sound is judged as. Drives both the threshold table
/// and the analysis-band LAYOUT: Guitar/Bass share the 6-band [`BANDS_6`], Bass
/// VI adds a 7th "Sub" band ([`BANDS_7`]) for its sub-60 Hz fundamentals. The
/// semantic band indices ([`Family::semantic_bands`]) hide the layout shift so
/// the rules read the same band by MEANING regardless of family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    Guitar,
    Bass,
    BassVi,
}

/// Migration alias — the old name for [`Family`] before the Bass VI split.
pub type Instrument = Family;

impl Family {
    /// The synthetic-stimulus thresholds (the HW-calibrated default). Kept for
    /// callers that don't distinguish stimulus kind (scene consistency, whose
    /// `scene_delta_db` is identical in both tables).
    pub fn thresholds(self) -> &'static Thresholds {
        self.thresholds_for(StimulusKind::Synthetic)
    }
    /// The threshold table for a stimulus kind: ONE table per family — `kind`
    /// no longer selects a different table (see the `StimulusKind` doc); it
    /// stays a parameter so call sites read the same regardless of which
    /// stimulus produced the sound.
    pub fn thresholds_for(self, _kind: StimulusKind) -> &'static Thresholds {
        match self {
            Family::Guitar => &GUITAR,
            Family::Bass => &BASS,
            Family::BassVi => &BASS_VI,
        }
    }
    /// Map a topology's `instrument` field ("guitar" | "bass" | "bass-vi").
    /// Anything unrecognized falls back to Guitar (the neutral 6-band layout).
    pub fn from_topology(instrument: &str) -> Family {
        if instrument.eq_ignore_ascii_case("bass-vi") {
            Family::BassVi
        } else if instrument.eq_ignore_ascii_case("bass") {
            Family::Bass
        } else {
            Family::Guitar
        }
    }
    /// The Hz band layout for this family (6 bands guitar/bass, 7 for Bass VI).
    pub fn bands(self) -> &'static [(f32, f32)] {
        match self {
            Family::BassVi => &BANDS_7,
            _ => &BANDS_6,
        }
    }
    /// The player-facing display labels, in lockstep with [`Family::bands`].
    pub fn labels(self) -> &'static [&'static str] {
        match self {
            Family::BassVi => &LABELS_7,
            _ => &LABELS_6,
        }
    }
    /// The six semantic band indices `(lows, low_mids, mids, high_mids, highs,
    /// air)`. Guitar/Bass start at 0; Bass VI's Sub band shifts the block +1 —
    /// rules address a band by MEANING, not raw index, regardless of layout.
    pub fn semantic_bands(self) -> (usize, usize, usize, usize, usize, usize) {
        let b = match self {
            Family::BassVi => 1,
            _ => 0,
        };
        (b, b + 1, b + 2, b + 3, b + 4, b + 5)
    }
    /// Owned display labels (the wire/serialization shape of [`Family::labels`]).
    pub fn labels_owned(self) -> Vec<String> {
        self.labels().iter().map(|s| (*s).to_string()).collect()
    }
    /// Geometric centre frequency `sqrt(lo·hi)` of each layout band (Hz). The x
    /// axis the Theil–Sen tilt line is fit over ([`tilt_split`]).
    pub fn band_centers(self) -> Vec<f64> {
        self.bands()
            .iter()
            .map(|&(lo, hi)| (f64::from(lo) * f64::from(hi)).sqrt())
            .collect()
    }
}

/// One captured sound's measurements — everything `diagnose` needs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundProfile {
    /// Welch-PSD band power per [`Family::bands`] band (raw linear power, not dB).
    /// Length follows the family's layout (6 guitar/bass, 7 Bass VI).
    pub bands: Vec<f64>,
    pub integrated_lufs: f64,
    /// Short-term-max − integrated LUFS (see `lufs::Loudness::spread_lu`):
    /// gain-invariant dynamics spread of the capture.
    pub spread_lu: f64,
    /// Post-stimulus tail RMS vs body RMS, in dB (see [`tail_energy_ratio`]).
    pub tail_ratio_db: f64,
    /// Spectral flatness (SFM, `psd::Psd::flatness`) of the top octave
    /// (6–12 kHz), `0..1`. Separates NOISE-like HF hash (fizz, high on a real DI
    /// capture) from a merely bright cab's harmonic top — gates the `fizzy` rule
    /// under [`StimulusKind::Capture`] (see [`FIZZY_MIN_FLATNESS`]).
    pub air_flatness: f64,
    /// Localized spectral peaks (200 Hz–8 kHz, see [`PEAK_DETECT_FLOOR_DB`])
    /// off the FINE body Welch PSD the capture measured — drives the
    /// `resonant`/`boxy` localized rules. Empty for profiles built without a
    /// PSD (curated fixtures, hand-built test vectors): both rules then
    /// silently no-op, same as every other Option-gated rule input. Skipped in
    /// serialization — a diagnosis input, not a report measurement.
    #[serde(skip)]
    pub peaks: Vec<crate::psd::SpectralPeak>,
}

impl SoundProfile {
    /// Build a profile from one Doctor capture of the PADDED stimulus
    /// ([`crate::leveller::doctor_stim_slice`]): resolves the pad-aware signal
    /// start ([`crate::leveller::doctor_signal_start`]), computes the body PSD
    /// ([`body_psd`]) internally, and delegates to
    /// [`SoundProfile::from_capture_with_psd`]. The convenience form for
    /// callers that don't reuse the body PSD (the probe sweeps);
    /// `doctor_check`'s closure keeps the manual split because it shares the
    /// PSD with the coverage gate.
    pub fn from_capture(
        samples: &[f32],
        rate: u32,
        stimulus_samples: usize,
        onset: usize,
        confident: bool,
        family: Family,
    ) -> Result<SoundProfile, String> {
        let signal_start = crate::leveller::doctor_signal_start(onset, confident);
        let psd = body_psd(samples, rate, signal_start);
        Self::from_capture_with_psd(samples, rate, stimulus_samples, onset, family, &psd)
    }

    /// Build a profile from one captured mono signal — the 6 Doctor-band
    /// energies, integrated loudness, and the reverb-wash tail ratio — with the
    /// body Welch PSD supplied by the caller instead of recomputed. The seam
    /// lets `doctor_check`'s capture closure compute ONE post-onset body PSD
    /// ([`body_psd`]) and share it between the profile's band
    /// powers/air-flatness and [`output_coverage_with_body`]'s SNR gate,
    /// instead of each independently computing (and disagreeing on) its own —
    /// the body PSD excludes the pre-signal preamble (device idle floor + the
    /// played pad) so band powers measure the SOUND, not the silence.
    /// LUFS/spread (whole-buffer, time-domain — leading silence dilutes them
    /// uniformly and the gated LUFS discards it anyway) and `tail_ratio_db`
    /// (already onset-aware, see [`tail_energy_ratio`]) are unchanged by which
    /// PSD is passed in.
    pub fn from_capture_with_psd(
        samples: &[f32],
        rate: u32,
        stimulus_samples: usize,
        onset: usize,
        family: Family,
        body_psd: &crate::psd::Psd,
    ) -> Result<SoundProfile, String> {
        let bands = body_psd.band_powers(family.bands());
        let loudness = crate::lufs::measure_mono(samples, rate)?;
        let integrated_lufs = loudness.integrated_lufs;
        // A silent capture measures −inf — route the sound to the errors lane
        // (the leveller's sentinel philosophy) instead of poisoning the scene
        // deltas with non-finite numbers.
        if !integrated_lufs.is_finite() {
            return Err("no signal on USB 1/2 — the sound is silent".to_string());
        }
        Ok(SoundProfile {
            bands,
            integrated_lufs,
            spread_lu: loudness.spread_lu(),
            tail_ratio_db: tail_energy_ratio(samples, rate, stimulus_samples, onset),
            air_flatness: body_psd.flatness(6000.0, 12000.0),
            peaks: body_psd.find_peaks(
                RESONANT_SCAN_LO_HZ,
                RESONANT_SCAN_HI_HZ,
                PEAK_DETECT_FLOOR_DB,
            ),
        })
    }
}

/// The shared post-onset BODY Welch PSD one Doctor capture measures ONCE, read
/// by both [`SoundProfile::from_capture_with_psd`] (band powers + air
/// flatness) and [`output_coverage_with_body`] (the SNR gate) — one
/// measurement space instead of two disagreeing ones (the profile used to span
/// the WHOLE buffer, preamble + tail included; the coverage gate's body
/// excluded them). Slices to `samples[onset..]` when `onset` is a confident
/// in-range value (`> 0` and `< samples.len()`); falls back to the legacy
/// whole-buffer PSD otherwise (an un-aligned/zero onset — every
/// synthetic-stimulus call site passes 0, `welch_psd` handles a short slice by
/// falling back to a smaller segment length, no extra guard needed here).
pub fn body_psd(samples: &[f32], rate: u32, onset: usize) -> crate::psd::Psd {
    // Low-variance Welch PSD (Stage 1) replaces the noisy single-bin Goertzel
    // probes — true integrated band power over each layout band, plus the
    // top-octave flatness the capture-space fizzy gate reads.
    if onset > 0 && onset < samples.len() {
        crate::psd::welch_psd(&samples[onset..], rate as f32)
    } else {
        crate::psd::welch_psd(samples, rate as f32)
    }
}

/// Curated Doctor `SoundProfile`s for the marketing-screenshot showcase
/// (`TMP_E2E_SHOWCASE=1`). The offline fake re-amp returns the raw stimulus for
/// every preset, so every sound would measure identically and the Results page
/// would read "All clear". Instead `doctor_check` injects these per showcase list
/// index (`commands/doctor.rs`), so the REAL `diagnose` engine renders genuine
/// cards.
///
/// The three mapped presets cover five of the six original guitar diagnoses
/// (two cards each), now judged under the deviation-from-target + Theil–Sen
/// tilt/local metric ([`deviations`] / [`tilt_split`]). Band values are
/// ABSOLUTE per-band dB; `fizzy` (Air − Highs) and `washed` (tail) ride
/// independently of the tilt/local split. NOTE: a tilt (dark/bright) and a local
/// bump (muddy/boomy/harsh/lost) now CAN co-fire — Theil–Sen's median fit
/// doesn't absorb a single-band bump into the trend the way the old OLS fit did
/// — so preset 11 is muddy+dark (a −5.0 dB/oct dark tilt plus a +7 dB low-mid
/// bump on top of the guitar target). The one co-firing limit that remains: a
/// SYMMETRIC 2-band low shelf (both Lows and Low-mids raised together) reads as
/// a tilt (dark), not a local bump — an accepted 6-point identifiability limit
/// of a single slope+intercept fit. Verified by the `showcase_profile_diagnoses`
/// test — keep those presets PLAIN and scene-less.
/// Band dB `[Lo, LoM, Mid, HiM, Hi, Air]` for the −5.0 dB/oct dark tilt PLUS a
/// +7 dB low-mid bump case (`deviations`/`tilt_split` oracle case O4): shared
/// by [`showcase_profile`]'s preset 11 (Tweed Warm) and the
/// `oracle_o4_dark_and_muddy_co_fire` test so the two stay byte-identical.
/// Derived as `GUITAR_TARGET[i] + (-5.0)·X[i] + (7.0 if i==1 else 0)` where
/// `X` is `log2` of the guitar band centers (see the oracle test block's
/// derivation comment) — python-verified to land `slope == -5.0` and
/// `locals == [0, 7, 0, 0, 0, 0]`.
#[cfg(any(test, feature = "e2e"))]
pub(crate) const SHOWCASE_DARK_MUDDY_BDB: [f64; 6] = [
    -37.034_453,
    -33.876_867,
    -51.524_101,
    -40.791_328,
    -46.753_734,
    -79.753_734,
];

#[cfg(any(test, feature = "e2e"))]
pub(crate) fn showcase_profile(list_index: u32) -> SoundProfile {
    // (band dB `[Lo, LoM, Mid, HiM, Hi, Air]`, tail_ratio_db). Scooped Verse (index 4)
    // carries `lost` (the mid scoop sorts first in the row), the tour expands it to
    // feature the add-a-compressor fix.
    // R5-retuned targets (2026-07-16): every band vector below is re-derived
    // as `GUITAR_TARGET[i] + delta[i]` (python-verified — see the oracle test
    // block's derivation comments for the shared method), so each showcase
    // preset lands its intended verdict pair under the new gates and every
    // other rule stays silent.
    let (db, tail): ([f64; 6], f64) = match list_index {
        // Scooped Verse → lost + washed: target + a −7 dB Mids scoop (a
        // single-band bump — both consensus spaces read back the full −7.0,
        // vs the 3.5 dB lost gate in each ⇒ margin ≈3.5) and a −2 dB tail
        // (wash gate −4.0 + 2 margin).
        4 => ([-5.0, -2.0, -12.0, 13.0, 13.5, -14.5], -2.0),
        // Tweed Warm → dark + muddy: target + a −5.0 dB/oct dark tilt + a +7 dB
        // low-mid bump (`deviations`/`tilt_split` oracle case O4).
        11 => (SHOWCASE_DARK_MUDDY_BDB, -80.0),
        // Direct Acoustic → harsh + fizzy: target + a +6 dB high-mid spike
        // (harsh gate 4.0 + 2 margin) and Air pulled to Highs−7 dB (past the
        // −9.0 fizzy gate by 2).
        167 => ([-5.0, -2.0, -5.0, 19.0, 13.5, 6.5], -80.0),
        // any other preset → all clear: sits exactly on the guitar target
        // curve (dev = 0 everywhere → silent by construction).
        _ => (GUITAR_TARGET, -80.0),
    };
    SoundProfile {
        bands: db.iter().map(|d| 10f64.powf(d / 10.0)).collect(),
        integrated_lufs: -18.0,
        // Steady by construction — the showcase never features the spiky card.
        spread_lu: 0.0,
        tail_ratio_db: tail,
        air_flatness: 0.5,
        peaks: Vec::new(),
    }
}

fn to_db(p: f64) -> f64 {
    10.0 * p.max(1e-12).log10()
}

/// A band counts as "covered" (actually excited) when its energy is within this
/// many dB of the loudest band in the capture — anything quieter reads as
/// unplayed or fully masked.
pub const BAND_COVERAGE_DB: f64 = 30.0;

/// Minimum top-octave spectral flatness ([`SoundProfile::air_flatness`]) for the
/// `fizzy` rule to fire under [`StimulusKind::Capture`]. Fizz is NOISE-like HF
/// hash; on a real-DI capture the top octave's flatness separates it from a
/// merely bright cab's harmonic top. On the SYNTHETIC shaped-noise stimulus
/// everything above the cab's rolloff is noise-like by construction, so the gate
/// is inert there by design (only `Capture` reads it).
pub const FIZZY_MIN_FLATNESS: f64 = 0.35;

// ─── localized peak (resonant/boxy) constants ────────────────────────────────

/// The frequency range [`RuleMetrics::peaks`] scans — every EQ-10-drivable
/// band the `resonant` rule can target sits inside it.
const RESONANT_SCAN_LO_HZ: f64 = 200.0;
const RESONANT_SCAN_HI_HZ: f64 = 8_000.0;

/// [`crate::psd::Psd::find_peaks`]'s detection floor: well below both rule
/// gates below so a below-threshold peak is still CANDIDATE data the rules can
/// compare against, rather than invisible to `find_peaks` entirely. ponytail:
/// an arbitrary "clearly a bump, not noise" floor — no HW calibration behind
/// this one (only the two rule gates below are pinned to the R8 sweep).
const PEAK_DETECT_FLOOR_DB: f64 = 3.0;

/// Minimum height (dB above the ~1/3-octave envelope, [`crate::psd::Psd::find_peaks`])
/// for the strongest peak in `RESONANT_SCAN_LO_HZ..RESONANT_SCAN_HI_HZ` to read
/// as `resonant` — PROVISIONAL, pending an R8 factory false-fire check (no HW
/// sweep behind this value yet, unlike the R5-calibrated `Thresholds` fields).
pub const RESONANT_MIN_HEIGHT_DB: f64 = 10.0;
/// Minimum quality factor (center / −3 dB width) for that same peak —
/// PROVISIONAL, see [`RESONANT_MIN_HEIGHT_DB`].
pub const RESONANT_MIN_Q: f64 = 4.0;
/// Minimum height for a peak centered in 300–500 Hz to read as `boxy` (a
/// narrower, more specific verdict than `muddy`'s whole-band buildup) — any Q
/// counts. PROVISIONAL, see [`RESONANT_MIN_HEIGHT_DB`].
pub const BOXY_MIN_HEIGHT_DB: f64 = 7.0;
const BOXY_LO_HZ: f64 = 300.0;
const BOXY_HI_HZ: f64 = 500.0;

/// Per-band coverage of a spectrum: `bands` are linear band powers (as returned
/// by [`crate::psd::welch_psd`] + `band_powers`); a band is "covered" when within [`BAND_COVERAGE_DB`]
/// of the loudest band. A sparse stimulus (an EBow drone, a couple of notes)
/// leaves bands uncovered, and the Doctor skips any rule keyed on a band the
/// stimulus never excited. Synthetic stimuli cover every band by construction, so
/// gating no-ops there. Shared by the Tier-2 calibration readout and the Doctor
/// band-confidence gate. Pure — no I/O, unit-tested.
pub fn coverage(bands: &[f64]) -> Vec<bool> {
    let loudest = bands.iter().copied().fold(0.0f64, f64::max).max(1e-12);
    let loudest_db = 10.0 * loudest.log10();
    bands
        .iter()
        .map(|&b| loudest_db - 10.0 * b.max(1e-12).log10() <= BAND_COVERAGE_DB)
        .collect()
}

/// A sound's spectral "balance": each band's dB offset from the sound's own
/// mean band level. Level-invariant — the UI's `balance_db` bar meter
/// (`commands/doctor.rs`). NOT the diagnosis metric anymore (that's
/// [`deviations`] / [`tilt_split`] below); kept only for the display bars.
pub fn balance(bands: &[f64]) -> Vec<f64> {
    let db: Vec<f64> = bands.iter().copied().map(to_db).collect();
    let n = db.len().max(1);
    let mean = db.iter().sum::<f64>() / n as f64;
    db.iter().map(|d| d - mean).collect()
}

/// Per-band level in dB: `10·log10(power)`, floored so a silent band is finite.
/// The dB space [`deviations`] and the [`tilt_split`] fit both work in.
pub fn band_db(bands: &[f64]) -> Vec<f64> {
    bands.iter().copied().map(to_db).collect()
}

/// Authored per-family target curves, in RAW band-power dB space (`band_db` —
/// note `band_power` is a width-integral, so wider high bands carry a bigger
/// term baked into these numbers).
///
/// Guitar: the R5 attended sweep's result — the MEDIAN spectral shape of a
/// 25-preset flagship factory-bank sweep through the shipped humbucker
/// stimulus at the production capture window (2026-07-16, `probe
/// --doctor-calib-factory`), rounded to 0.5 dB. An authored anchor for
/// "well-voiced Fender tone" whose provenance is the factory designers' own
/// consensus voicing, NOT a per-preset fit — a corpus-derived target and
/// threshold would be confounded (the target IS "what a typical preset
/// does", so comparing a preset against it can only ever measure how
/// atypical it is, never whether it sounds good; the flaw that killed the
/// old runtime-cohort metric). The original hand-authored curve imagined
/// per-Hz density and read every factory preset as +5 dB/oct "bright":
/// band powers are WIDTH-INTEGRATED, so the wide presence bands legitimately
/// dominate.
///
/// Bass: a single-preset anchor (user bank slot 21). Bass VI: a two-preset
/// anchor (slots 8–9). Both PROVISIONAL pending a proper bass factory sweep
/// — a starting point, not a calibrated consensus like the guitar curve.
// ponytail: bass/bass-vi voicings are single/two-preset anchors, provisional
// until a real bass factory sweep lands.
const GUITAR_TARGET: [f64; 6] = [-5.0, -2.0, -5.0, 13.0, 13.5, -14.5];
const BASS_TARGET: [f64; 6] = [1.5, 6.5, 9.5, 5.0, 1.5, -24.0];
const BASS_VI_TARGET: [f64; 7] = [-18.0, 1.5, 2.0, -5.0, 14.0, 15.0, -9.0];

/// The authored target curve for a family (length = its band count) — see
/// [`GUITAR_TARGET`].
pub fn target_curve(family: Family) -> &'static [f64] {
    match family {
        Family::Guitar => &GUITAR_TARGET,
        Family::Bass => &BASS_TARGET,
        Family::BassVi => &BASS_VI_TARGET,
    }
}

/// Per-band deviation from the family's authored target curve: `band_db[i] −
/// target_curve(family)[i]`, where `band_db` is the caller's already-computed
/// [`band_db`] output. NO mean removal anywhere — absolute level is absorbed
/// by the Theil–Sen fit's intercept in [`tilt_split`], so this stays
/// level-invariant by construction downstream, not by subtracting a mean here.
/// A length mismatch (should not happen — `band_db` always follows `family`'s
/// layout) truncates to the shorter of the two via `zip`.
pub fn deviations(band_db: &[f64], family: Family) -> Vec<f64> {
    let target = target_curve(family);
    band_db.iter().zip(target).map(|(&d, &t)| d - t).collect()
}

/// The one shared median: `xs` (sorted internally) — the middle value for an
/// odd count, the mean of the two middles for an even count. `f64::total_cmp`
/// keeps the sort NaN-safe (a failed loudness measure won't panic) and — so
/// the result — deterministic. `0.0` for an empty slice.
pub(crate) fn median(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(f64::total_cmp);
    let n = xs.len();
    if n == 0 {
        return 0.0;
    }
    if n % 2 == 1 {
        xs[n / 2]
    } else {
        (xs[n / 2 - 1] + xs[n / 2]) / 2.0
    }
}

/// Whether band `i` is covered by the stimulus: `true` when `coverage` is
/// `None` (gating disabled — all bands treated as covered) or the band's own
/// entry, defaulting to covered when the index is out of range. Shared by
/// [`tilt_split`]'s fit-band filter and [`diagnose_kind`]'s per-rule gate.
fn band_covered(coverage: Option<&[bool]>, i: usize) -> bool {
    coverage.is_none_or(|c| c.get(i).copied().unwrap_or(true))
}

/// Robust tilt/local decomposition of a sound's target deviations: a Theil–Sen
/// line fit over `(log2(band_center), dev[i])` for the semantic BODY bands
/// ([`Family::semantic_bands`]'s `lows..=highs` — Sub and Air are excluded from
/// the fit, further restricted to `covered` bands when `Some`), returning the
/// fitted slope (`None` when fewer than 3 fit points survive) and the per-band
/// LOCAL residual after removing that line from every band (including the
/// excluded Sub/Air bands, extrapolated — display/unused there).
///
/// Theil–Sen (median of all pairwise slopes), not OLS: a median fit doesn't
/// absorb a 1–2-band genuine bump into the trend line the way a least-squares
/// fit does (an OLS fit on 5 points reads a real +8 dB low bump as only +3.2 dB
/// local — see the doctor-metric oracle tests), while a true broadband tilt
/// still lands exactly in the slope either way. Median-of-medians ties broken
/// via `f64::total_cmp` ⇒ deterministic.
pub fn tilt_split(
    dev: &[f64],
    family: Family,
    covered: Option<&[bool]>,
) -> (Option<f64>, Vec<f64>) {
    let xs: Vec<f64> = family.band_centers().iter().map(|c| c.log2()).collect();
    let (lows, _low_mids, _mids, _high_mids, highs, _air) = family.semantic_bands();
    let fit: Vec<usize> = (lows..=highs)
        .filter(|&i| band_covered(covered, i))
        .collect();

    let (slope, intercept) = if fit.len() >= 3 {
        let mut slopes = Vec::with_capacity(fit.len() * (fit.len() - 1) / 2);
        for a in 0..fit.len() {
            for &j in &fit[a + 1..] {
                let i = fit[a];
                slopes.push((dev[j] - dev[i]) / (xs[j] - xs[i]));
            }
        }
        let slope = median(slopes);
        let intercept = median(fit.iter().map(|&i| dev[i] - slope * xs[i]).collect());
        (Some(slope), intercept)
    } else {
        let intercept = median(fit.iter().map(|&i| dev[i]).collect());
        (None, intercept)
    };

    let s = slope.unwrap_or(0.0);
    let locals = xs
        .iter()
        .zip(dev)
        .map(|(&x, &d)| d - (s * x + intercept))
        .collect();
    (slope, locals)
}

/// Median-centered deviations: `dev[i] − median(dev[body bands])`. Level-robust
/// (median of ≥5 body bands ignores ≤2 defect-inflated ones) and — unlike the
/// tilt-split locals — immune to a skirted bump dragging the slope estimate.
/// NOT tilt-invariant (healthy tilt leaks ± a few dB into the endpoint bands),
/// which is why band rules demand consensus WITH the tilt-split local instead
/// of replacing it (see [`Thresholds`]'s doc). Body bands = the semantic
/// `lows..=highs` range ([`Family::semantic_bands`]) — Guitar/Bass indices
/// 0..=4, Bass VI 0..=5 (Sub excluded, same convention `tilt_split`'s fit
/// uses). Returns one value per band in `dev` (Sub/Air included, centered
/// against the same body median — extrapolated/display there, like
/// `tilt_split`'s locals). `median` is the shared odd/even-safe helper.
pub fn centered_deviations(dev: &[f64], family: Family) -> Vec<f64> {
    let (lows, _, _, _, highs, _) = family.semantic_bands();
    let m = median(dev[lows..=highs].to_vec());
    dev.iter().map(|&d| d - m).collect()
}

/// RMS of a sample window, in f64 (0.0 for an empty window). The ONE copy of the
/// formula shared by [`tail_energy_ratio`] and the `--doctor-calib` noise-floor
/// metric — a precision/edge-case fix must not drift between them.
pub(crate) fn rms_f64(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples
        .iter()
        .map(|x| f64::from(*x) * f64::from(*x))
        .sum::<f64>()
        / samples.len() as f64)
        .sqrt()
}

/// A signal's own band coverage in a family's layout — [`coverage`] over its
/// Welch-PSD band powers ([`crate::psd::welch_psd`]) at the device's 48 kHz
/// clock. The one home for the (rate, layout) pairing shared by the Tier-2
/// calibration readout and the `--doctor-calib` sweep. NOT used by the live
/// `doctor_check` gate anymore — that reads the CAPTURED OUTPUT's own coverage
/// ([`output_coverage`]), since the input stimulus can't see amp-created
/// distortion HF.
pub fn band_coverage(samples: &[f32], family: Family) -> Vec<bool> {
    coverage(&crate::psd::welch_psd(samples, 48_000.0).band_powers(family.bands()))
}

/// SNR margin (dB) a captured-output band must clear its pre-onset noise floor
/// by to count as "covered" in [`output_coverage_with_body`].
pub const OUTPUT_SNR_MARGIN_DB: f64 = 8.0;

/// Per-band coverage measured on the CAPTURED OUTPUT rather than the input
/// stimulus ([`band_coverage`]): the pre-onset window (device idle floor,
/// re-amp engaged but the stimulus not yet arrived) is the noise-floor
/// reference, and a band counts as covered when the post-onset body's band
/// power exceeds the floor's by [`OUTPUT_SNR_MARGIN_DB`]. Gating on the output
/// — not the input — revives `fizzy`/`harsh` on the DI path, where amp
/// distortion CREATES high-frequency content the input stimulus never
/// carried; gating on the input would starve those rules of a band they
/// should be allowed to fire in.
///
/// Takes the body Welch PSD from the caller instead of recomputing it — see
/// [`SoundProfile::from_capture_with_psd`]'s doc for why sharing one body PSD
/// per capture matters (`doctor_check`'s capture path shares one `body_psd`
/// across the profile + this coverage gate, see `commands/doctor.rs`).
/// `signal_start` is the pad-shifted start of real signal
/// ([`crate::leveller::doctor_signal_start`]); a confident onset always lands
/// it well past MIN_FLOOR_SAMPLES thanks to the 200 ms stimulus preamble,
/// so the guard below fires only on the UNCONFIDENT-onset fallback
/// (`signal_start == 0`) — then every band reads covered WITHOUT touching
/// `body_psd`, the legacy permissive behavior.
pub fn output_coverage_with_body(
    samples: &[f32],
    rate: u32,
    signal_start: usize,
    family: Family,
    body_psd: &crate::psd::Psd,
) -> Vec<bool> {
    /// Minimum pre-signal window for a stable Welch floor estimate.
    const MIN_FLOOR_SAMPLES: usize = 2048;
    let bands = family.bands();
    if signal_start < MIN_FLOOR_SAMPLES || signal_start >= samples.len() {
        return vec![true; bands.len()];
    }
    let floor = crate::psd::welch_psd(&samples[..signal_start], rate as f32).band_powers(bands);
    let body = body_psd.band_powers(bands);
    let margin = 10f64.powf(OUTPUT_SNR_MARGIN_DB / 10.0);
    floor
        .iter()
        .zip(&body)
        .map(|(&f, &b)| b > f * margin)
        .collect()
}

/// Post-stimulus tail energy vs stimulus-body energy, in dB (≤ 0 in practice;
/// a dry sound decays fast → strongly negative; a drowning reverb/delay tail
/// keeps ringing → closer to 0). Returns −80 (a "silent tail" floor) when the
/// capture has no tail window.
///
/// `onset` is where the stimulus actually starts in the capture (the buffer
/// begins at stream start, BEFORE the audio propagated through cpal/USB/DSP —
/// see `audio::estimate_onset`). Splitting at `stimulus_samples` alone leaks the
/// last ~latency of body-level signal into the tail, inflating a bone-dry
/// preset's ratio toward the washed threshold (~−17 dB vs the −13 dB gate for a
/// 50 ms leak into a multi-second tail). Pass 0 to keep the un-aligned legacy split.
pub fn tail_energy_ratio(
    samples: &[f32],
    _rate: u32,
    stimulus_samples: usize,
    onset: usize,
) -> f64 {
    let body_end = onset.saturating_add(stimulus_samples);
    if samples.len() <= body_end || stimulus_samples == 0 {
        return -80.0;
    }
    let body = rms_f64(&samples[onset..body_end]);
    let tail = rms_f64(&samples[body_end..]);
    if body <= 0.0 {
        return -80.0;
    }
    (20.0 * (tail / body).max(1e-4).log10()).max(-80.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Sev {
    High,
    Med,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diag {
    pub key: &'static str,
    /// Card headline. `String` (not `&'static str`) because the localized
    /// resonant/boxy rules name the MEASURED frequency (e.g. "Rings at 2.8
    /// kHz") — every other rule's label is still a fixed string, just owned.
    pub label: String,
    /// Coarse tint (High/Med).
    pub sev: Sev,
    /// Magnitude PAST threshold in the rule's natural unit (dB for the
    /// band/fizzy/washed rules, LU for spiky) — `metric − threshold`, always ≥ 0
    /// for a fired card. Playback offsets shift the threshold, so they shift this.
    /// The frontend's `isPossible` (a near-threshold "possible" verdict, rendered
    /// muted) reads this directly — see `severity.ts::POSSIBLE_MAX_SEVERITY`. The
    /// UI renders a fired card as "possible" when `severity < 1.0` — a flat
    /// provisional cutoff applied uniformly across every rule's own unit, not a
    /// per-rule-calibrated one; the R5 calibration sweep is expected to refine it
    /// per rule.
    pub severity: f64,
    /// Indices into the sound's family band layout ([`Family::bands`]) that
    /// light up in the UI; empty = a time-domain finding (washed / buried).
    pub bands: Vec<usize>,
    /// The Hz/dB one-liner (progressive disclosure under the plain sentence).
    pub detail: String,
    pub explain: &'static str,
    pub rx: Vec<Rx>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RxKind {
    OneClick,
    Advisory,
    Chain,
}

/// A prescription. `ops` is empty for advisory cards (nothing to apply).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rx {
    pub kind: RxKind,
    pub title: String,
    pub detail: String,
    /// "no CPU change" | "+N.N% CPU" (real delta from the model-cpu table).
    pub cpu_note: String,
    pub ops: Vec<DoctorOp>,
    /// Chain-preview DTO for `kind == Chain`: `{ "template": …, "blocks":
    /// [{ "model": FenderId, "added"?: true }] }` — the UI resolves art by
    /// model id through its existing strip engine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain: Option<Value>,
}

/// One concrete device edit. Serde casing mirrors the `copy_apply` wire
/// convention (camelCase field names via rename). `Deserialize` is needed by
/// `doctor_apply` (the frontend sends these back verbatim to apply live).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DoctorOp {
    /// Live `changeParameter` on an existing node. `param` is the schema
    /// `controlId` (the preset-JSON key, see module docs).
    Param {
        #[serde(rename = "groupId")]
        group_id: String,
        #[serde(rename = "nodeId")]
        node_id: String,
        param: String,
        value: f64,
    },
    /// Live `insertNode` (+ param writes on the fresh node). `beforeFenderId`
    /// None = append to the group.
    InsertNode {
        #[serde(rename = "groupId")]
        group_id: String,
        #[serde(rename = "beforeFenderId")]
        before_fender_id: Option<String>,
        #[serde(rename = "fenderId")]
        fender_id: String,
        params: Vec<(String, f64)>,
    },
}

// ─── exact-id block tables (fw 1.8.45 schema extractions) ───────────────────

/// Drive/distortion pedals (`algoCategory == "dist"`).
const DIST_IDS: [&str; 46] = [
    "ACD_BigFuzz",
    "ACD_BigFuzzGT",
    "ACD_Blackbox",
    "ACD_BluesBreaker",
    "ACD_BluesDriver",
    "ACD_Blumes",
    "ACD_Boost",
    "ACD_DS1",
    "ACD_DistortionPlus",
    "ACD_EPBooster",
    "ACD_Fuzz",
    "ACD_GeFuzzFace",
    "ACD_GreenRussianBmp",
    "ACD_Greenbox",
    "ACD_Greenbox10",
    "ACD_JRockettDude",
    "ACD_KingOfTone",
    "ACD_KlonCentaur",
    "ACD_LargeOverdrive",
    "ACD_Lightspeed",
    "ACD_ModernBassOverdrive",
    "ACD_MythicDrive",
    "ACD_NobelsOdr1",
    "ACD_ObsessiveDrive",
    "ACD_Octavia",
    "ACD_Orangebox",
    "ACD_Palladium",
    "ACD_Plumes",
    "ACD_Pugilist",
    "ACD_RackPreamp",
    "ACD_RamsHeadBmp",
    "ACD_Rangemaster",
    "ACD_Ranger",
    "ACD_Rat",
    "ACD_Rockman",
    "ACD_RoundFuzz",
    "ACD_SD1",
    "ACD_SwordFuzz",
    "ACD_TCIntegratedPre",
    "ACD_TCIntegratedPreStatic",
    "ACD_TimmyV3",
    "ACD_TubeDriver",
    "ACD_TubeScreamer",
    "ACD_VariFuzz",
    "ACD_Yellowbox",
    "ACD_ZenDrive",
];

/// Reverbs (`algoCategory == "reverb"`) → their wet/dry mix `controlId`.
/// `None` = spring/dwell-style blocks with no true mix control (those get an
/// advisory, not a param write — dwell changes the drive, not the balance).
const REVERB_MIX: [(&str, Option<&str>); 36] = [
    ("ACD_Ambient", None),
    ("ACD_Arena", None),
    ("ACD_BloomfieldDriveConv", Some("mix")),
    ("ACD_Cirrostratus", Some("mix")),
    ("ACD_CirrostratusLite", Some("mix")),
    ("ACD_CloudReverb", Some("mix")),
    ("ACD_FenderLargeHall", None),
    ("ACD_FenderLargeModulatedHall", None),
    ("ACD_FenderSmallModulatedHall", None),
    ("ACD_FenderSmallRoom", None),
    ("ACD_Ga15Reverb", None),
    ("ACD_NebulaReverse", Some("mix")),
    ("ACD_NebulaTamed", Some("mix")),
    ("ACD_SlimmerShimmer", Some("mix")),
    ("ACD_SpectralReverb", Some("mix")),
    ("ACD_Spring65", None),
    ("ACD_Spring65New", None),
    ("ACD_TMAmbienceConv", Some("mix")),
    ("ACD_TMCathedralConv", Some("mix")),
    ("ACD_TMChamberConv", Some("mix")),
    ("ACD_TMEtherealHallConv", Some("mix")),
    ("ACD_TMHallOfDoomConv", Some("mix")),
    ("ACD_TMLargeHall", Some("mix")),
    ("ACD_TMLargePlate", Some("wetdrymix")),
    ("ACD_TMLargeRoom", Some("mix")),
    ("ACD_TMNewAgeHallConv", Some("mix")),
    ("ACD_TMRichPlateConv", Some("mix")),
    ("ACD_TMShimmer", Some("mix")),
    ("ACD_TMSmallHall", Some("mix")),
    ("ACD_TMSmallPlate", Some("wetdrymix")),
    ("ACD_TMSmallRoom", Some("mix")),
    ("ACD_TMSpring63", Some("mix")),
    ("ACD_TMSpring63Conv", None),
    ("ACD_TMSpring65", Some("mix")),
    ("ACD_TMSpring65Conv", None),
    ("ACD_TMWarmPlateConv", None),
];

/// Compressors — the `dyn` family's comp rows (`src/models/blockArtCatalog/dyn.ts`
/// is the reference list). Raw catalog FenderIds, exact-match like `DIST_IDS`
/// (the backend never suffix-normalizes).
const COMP_IDS: [&str; 5] = [
    "ACD_CS3",
    "ACD_CompressorSimple",
    "ACD_CompressorSimpleSoftKnee",
    "ACD_DynaComp",
    "ACD_Sustain",
];

/// Delays (`algoCategory == "delay"`) — `src/models/blockArtCatalog/delay.ts`
/// is the reference list. Raw catalog FenderIds, exact-match like `DIST_IDS`.
const DELAY_IDS: [&str; 30] = [
    "ACD_AutoSwellDelay",
    "ACD_BoilerPlateMono",
    "ACD_BoilerPlateStereo",
    "ACD_DM2",
    "ACD_DeepFreeze",
    "ACD_Doubler",
    "ACD_DynamicDelay",
    "ACD_EchoMachine",
    "ACD_EchoplexEP3",
    "ACD_EchoplexEP3Stereo",
    "ACD_Freeze",
    "ACD_Glooper",
    "ACD_HaloDelay",
    "ACD_HaloDelayStereo",
    "ACD_HoldDelay",
    "ACD_HoldDelayStereo",
    "ACD_MemoryMan",
    "ACD_MemoryManStereo",
    "ACD_ModDelay",
    "ACD_MultiplyDelay",
    "ACD_MultiplyDelayMono",
    "ACD_Polyhedron",
    "ACD_RackDelay",
    "ACD_RackDelayStereo",
    "ACD_SpaceEcho",
    "ACD_SpaceEchoStereo",
    "ACD_TMDelayFilter",
    "ACD_TMDelayFilterStereo",
    "ACD_TMPingPong",
    "ACD_TMReverse",
];

const CAB_STANDALONE: &str = "ACD_CabSimTMS";
const EQ10_STEREO: &str = "ACD_TenBandEQStereo"; // never the Mono variant (absent from the product profile)
/// Graphic/parametric EQ blocks a preset may ALREADY carry that the Doctor
/// cannot precisely drive: their band controlIds aren't in the graph param
/// allowlist, so a one-click would be a BLIND overwrite of the player's own
/// curve — the value-aware rule forbids it. When one is active, a muddy/harsh
/// fix advises using it rather than stacking a redundant EQ-10. Raw catalog
/// FenderIds, exact-match like `DIST_IDS` (the `algoCategory == "eq"` blocks,
/// minus the drivable `EQ10_STEREO`). Freqout is a feedback pedal, not an EQ
/// (see `freqout_is_not_an_eq`).
const OTHER_EQ_IDS: [&str; 4] = [
    "ACD_TenBandEQMono",
    "ACD_MustangSevenBandEq",
    "ACD_FiveBandParamEQ",
    "ACD_MustangPEQ",
];
const HIGH_LOW_PASS: &str = "ACD_HighLowPass";
const COMPRESSOR: &str = "ACD_DynaComp"; // classic 2-knob pedal comp, schema-verified
/// Soft-knee studio comp: transparent post-cab leveling (1.0% CPU vs the
/// DynaComp's 4.8; DynaComp stays the front-of-chain pedal-squish pick).
const COMPRESSOR_STUDIO: &str = "ACD_CompressorSimpleSoftKnee";

/// EQ-10 band gain range (dB). ponytail: ±12 is the graphic-EQ standard; the
/// band controlIds' fw schema is the source to re-derive from if a rev differs.
const EQ10_BAND_RANGE_DB: f64 = 12.0;

// ─── graph facts ─────────────────────────────────────────────────────────────

/// One chain node as the frontend holds it — the wire mirror of the serialized
/// `session::GraphNode` (the backup scan's per-preset `ActiveGraph.nodes`).
/// The frontend passes these per checked sound, so prescriptions target the
/// preset's REAL blocks with zero extra device reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorNode {
    pub group_id: String,
    pub node_id: String,
    pub model: String,
    #[serde(default)]
    pub bypassed: bool,
    /// Present on any node carrying a cab sim (`dspUnitParameters.cabsimid`):
    /// the standalone `ACD_CabSimTMS` AND the CabIR amps — the same
    /// device-driven discriminator the strip renderer uses.
    #[serde(default)]
    pub cab_sim_id: Option<String>,
    /// True only when a standalone CabSim runs two cabinets (counts as 2
    /// cabinet slots in the firmware caps).
    #[serde(default)]
    pub cab_sim2_enabled: Option<bool>,
    /// The node's allowlisted current param values (reverb mix names + cab
    /// hpf/lpf + EQ-10 band gains — see `session::GraphNode.params`), for
    /// value-aware prescriptions. `default` so pre-params payloads still deserialize.
    #[serde(default)]
    pub params: HashMap<String, f64>,
}

impl DoctorNode {
    /// Map the crate-internal graph node (`extract_active_graph` output) —
    /// used by the probe sweep; the app path deserializes from the frontend.
    pub fn from_graph_node(n: &crate::session::GraphNode) -> DoctorNode {
        DoctorNode {
            group_id: n.group_id.clone(),
            node_id: n.node_id.clone(),
            model: n.model.clone(),
            bypassed: n.bypassed,
            cab_sim_id: n.cab_sim_id.clone(),
            cab_sim2_enabled: n.cab_sim2_enabled,
            params: n.params.clone(),
        }
    }
}

/// What `generate_rx` needs to know about the preset's chain, gathered in one
/// walk. Bypassed blocks are never carriers (a param write to a bypassed
/// block is inaudible), so the hierarchy falls through to an insert instead.
#[derive(Debug, Default)]
struct GraphFacts {
    /// (group, node_id, current params) of the standalone cab or the amp carrying
    /// an embedded cab (`cab_sim_id` present). Both expose the same `hpf`/`lpf`
    /// controlIds (schema-verified); the params carry their current values (when
    /// the allowlist captured them) so [`cut_move`] can skip a filter one-click
    /// that would push the cut the WRONG way.
    cab: Option<(String, String, HashMap<String, f64>)>,
    /// Existing EQ-10 stereo node, if any: (group, node_id, current band gains).
    eq10: Option<(String, String, HashMap<String, f64>)>,
    /// True when an EQ the Doctor can't precisely drive (a 7-band GE, parametric,
    /// mono 10-band — [`OTHER_EQ_IDS`]) is already ACTIVE in the chain. Gates the
    /// EQ moves to an advisory (use the one you have) instead of a second EQ.
    eq_other: bool,
    /// First reverb with a real mix param: (group, node_id, mix_param, current
    /// mix value when the graph carried it).
    reverb_mix: Option<(String, String, String, Option<f64>)>,
    /// Any drive/dist pedal present (bypassed ones don't count).
    has_drive: bool,
    /// A compressor in the chain: `Some(bypassed)` — an ACTIVE comp (`Some(false)`)
    /// wins over a bypassed one. Unlike the carriers above, a BYPASSED comp is
    /// still a fact (the fix is "switch it back on").
    comp: Option<bool>,
    /// First node of the first guitar group (the "front of chain" insert anchor).
    front: Option<(String, String)>,
}

/// A reverb (any `REVERB_MIX` row, dwell-only ones included) or a delay —
/// the two time-based effect families whose wet tail a downstream compressor
/// would pump.
fn is_time_effect(model: &str) -> bool {
    REVERB_MIX.iter().any(|(id, _)| *id == model) || DELAY_IDS.contains(&model)
}

/// True when the chain carries anything the `washed` rule could plausibly fire
/// on: a curated reverb/delay id ([`is_time_effect`]) OR a substring match on
/// "Reverb"/"Delay"/"Echo"/"Rvb" — a catch-all for ids the curated lists miss
/// plus amp models with baked-in convolution reverb (e.g. an amp id ending
/// `…CabIRConvRvb`). Gates the Doctor capture's tail length via
/// [`doctor_tail_ms`] — a full tail exists only so `washed` has wash to
/// analyze.
///
/// The substring matcher is DELIBERATELY conservative (broad, not precise):
/// the two failure directions are asymmetric. A FALSE POSITIVE — an amp
/// NAMED Reverb with no live reverb block, e.g. `ACD_DeluxeReverb65…NoFx` —
/// merely keeps the full wash tail (wasted capture time, no verdict impact).
/// A false NEGATIVE would silently starve `washed` of its tail window on a
/// genuinely wet preset. So when in doubt, this says yes.
pub fn has_time_effect(nodes: &[DoctorNode]) -> bool {
    const SUBSTRINGS: [&str; 4] = ["Reverb", "Delay", "Echo", "Rvb"];
    nodes
        .iter()
        .any(|n| is_time_effect(&n.model) || SUBSTRINGS.iter().any(|s| n.model.contains(s)))
}

/// The Doctor capture tail (ms) for a chain: the full wash-analysis tail
/// (`leveller::DOCTOR_TAIL_MS`) when the graph is unknown (empty `nodes` —
/// conservative default) or carries a time-based block ([`has_time_effect`]);
/// otherwise the short settle-only tail (`leveller::DOCTOR_TAIL_DRY_MS` —
/// `washed` can't fire without a time-based block, so the full tail buys
/// nothing there). The ONE home of the tail-length policy.
pub fn doctor_tail_ms(nodes: &[DoctorNode]) -> u32 {
    if nodes.is_empty() || has_time_effect(nodes) {
        crate::leveller::DOCTOR_TAIL_MS
    } else {
        crate::leveller::DOCTOR_TAIL_DRY_MS
    }
}

fn graph_facts(nodes: &[DoctorNode]) -> GraphFacts {
    let mut f = GraphFacts::default();
    for n in nodes {
        if f.front.is_none() && n.group_id.starts_with('G') {
            f.front = Some((n.group_id.clone(), n.model.clone()));
        }
        if COMP_IDS.contains(&n.model.as_str()) && f.comp != Some(false) {
            f.comp = Some(n.bypassed);
        }
        if n.bypassed {
            continue;
        }
        if f.cab.is_none() && (n.model == CAB_STANDALONE || n.cab_sim_id.is_some()) {
            f.cab = Some((n.group_id.clone(), n.node_id.clone(), n.params.clone()));
        }
        if n.model == EQ10_STEREO {
            f.eq10 = Some((n.group_id.clone(), n.node_id.clone(), n.params.clone()));
        } else if OTHER_EQ_IDS.contains(&n.model.as_str()) {
            f.eq_other = true;
        }
        if f.reverb_mix.is_none() {
            if let Some((_, Some(p))) = REVERB_MIX.iter().find(|(id, _)| *id == n.model) {
                f.reverb_mix = Some((
                    n.group_id.clone(),
                    n.node_id.clone(),
                    (*p).to_string(),
                    n.params.get(*p).copied(),
                ));
            }
        }
        if DIST_IDS.contains(&n.model.as_str()) {
            f.has_drive = true;
        }
    }
    f
}

// ─── prescriptions ───────────────────────────────────────────────────────────

/// CPU-gate an insert: `Some(note)` when the block fits every firmware cap,
/// `None` when it doesn't (a fix that can't apply must not render a button).
fn insert_cpu_note(nodes: &[DoctorNode], candidate: &str) -> Option<String> {
    // Mirror `blockcaps::roster_from_preset`'s mapping: dual-cab weighting is
    // real only on the standalone CabSim (on an amp the same flag means a
    // dual MIC on one cab).
    let roster: Vec<blockcaps::RosterEntry> = nodes
        .iter()
        .map(|n| blockcaps::RosterEntry {
            group: n.group_id.clone(),
            node_id: n.node_id.clone(),
            fender_id: n.model.clone(),
            dual_cab: n.model == CAB_STANDALONE && n.cab_sim2_enabled == Some(true),
        })
        .collect();
    let counts = blockcaps::counts(&roster);
    blockcaps::check_op(&counts, candidate, None, false, false, false).ok()?;
    let mut next = counts.clone();
    next.add(candidate, false);
    let delta = next.cpu - counts.cpu;
    Some(if delta > 0.005 {
        format!("+{delta:.1}% CPU")
    } else {
        "no CPU change".to_string()
    })
}

fn advisory(title: &str, detail: &str) -> Rx {
    Rx {
        kind: RxKind::Advisory,
        title: title.to_string(),
        detail: detail.to_string(),
        cpu_note: String::new(),
        ops: Vec::new(),
        chain: None,
    }
}

/// Chain-preview DTO: the current roster's models plus the inserted one.
fn chain_preview(nodes: &[DoctorNode], template: &str, inserted: &str, at: usize) -> Value {
    let mut blocks: Vec<Value> = nodes
        .iter()
        .map(|n| serde_json::json!({ "model": n.model }))
        .collect();
    blocks.insert(at, serde_json::json!({ "model": inserted, "added": true }));
    serde_json::json!({ "template": template, "blocks": blocks })
}

/// Where to drop a block immediately AFTER the cab, in the cab's group.
/// Shared by the EQ and post-cab-compressor inserts.
struct CabAnchor {
    /// The cab's group id (the insert's group).
    group: String,
    /// The same-group `beforeFenderId` anchor: the next node's id, or `None`
    /// when the cab ends its group (append — same position). The wire's
    /// `beforeFenderId` is same-group only; a cross-group one is silently
    /// dropped. Bypassed neighbours still anchor (position is chain order, not
    /// audibility — skipping one would drift the block when it's re-enabled).
    before: Option<String>,
    /// Chain-preview insert index (cab index + 1).
    at: usize,
}

/// The [`CabAnchor`] for the chain's cab, or `None` when there's no cab.
fn after_cab_anchor(nodes: &[DoctorNode], facts: &GraphFacts) -> Option<CabAnchor> {
    let (cab_group, cab_node, _) = facts.cab.as_ref()?;
    let idx = nodes
        .iter()
        .position(|n| &n.group_id == cab_group && &n.node_id == cab_node)?;
    let before = nodes
        .get(idx + 1)
        .filter(|n| &n.group_id == cab_group)
        .map(|n| n.node_id.clone());
    Some(CabAnchor {
        group: cab_group.clone(),
        before,
        at: idx + 1,
    })
}

/// Player-facing frequency label: `90.0` → "90 Hz", `8000.0` → "8 kHz".
fn freq_label(hz: f64) -> String {
    if hz >= 1000.0 {
        format!("{:.0} kHz", hz / 1000.0)
    } else {
        format!("{hz:.0} Hz")
    }
}

/// Player-facing band label from an EQ-10 gain controlId:
/// `gain250hz` → "250 Hz", `gain2khz` → "2 kHz".
fn eq_band_label(param: &str) -> String {
    let core = param.trim_start_matches("gain").trim_end_matches("hz");
    match core.strip_suffix('k') {
        Some(k) => format!("{k} kHz"),
        None => format!("{core} Hz"),
    }
}

/// Player-facing copy for an [`eq_move`], varying per diagnosis.
struct EqCopy<'a> {
    /// One-click title when a drivable EQ-10 is reused (known band values).
    reuse_title: &'a str,
    reuse_detail: &'a str,
    /// Chain title/detail when no EQ exists and one is inserted.
    insert_title: &'a str,
    insert_detail: &'a str,
    /// Advisory detail when a non-drivable EQ is already present.
    advise_detail: &'a str,
}

/// The muddy/harsh EQ move, in priority order:
/// 1. A drivable EQ-10 stereo already in the chain → a value-aware one-click on
///    it. `gains` are RELATIVE moves (e.g. −3 = cut 3 dB): known band values get
///    `current + move` clamped to the band range under `copy.reuse_title`; an
///    unknown value writes the absolute and the title says so ("Set the 250 Hz
///    band to −3 dB"). A fresh insert starts at 0 dB, so absolute == relative.
/// 2. A DIFFERENT EQ (7-band GE, parametric, mono 10-band) already present →
///    `advisory(copy.reuse_title, copy.advise_detail)`: its bands aren't drivable
///    value-aware, so we point the player at it rather than stacking a second EQ.
/// 3. Otherwise insert an EQ-10 anchored right AFTER the cab (not the chain tail).
///
/// Returns None only when an insert is needed but fails the caps / has no anchor.
fn eq_move(
    nodes: &[DoctorNode],
    facts: &GraphFacts,
    copy: &EqCopy,
    gains: &[(&str, f64)],
) -> Option<Rx> {
    if let Some((group, node, current)) = &facts.eq10 {
        let known = gains.iter().all(|(p, _)| current.contains_key(*p));
        let ops = gains
            .iter()
            .map(|(p, v)| DoctorOp::Param {
                group_id: group.clone(),
                node_id: node.clone(),
                param: (*p).to_string(),
                value: match current.get(*p) {
                    Some(cur) => (cur + v).clamp(-EQ10_BAND_RANGE_DB, EQ10_BAND_RANGE_DB),
                    None => *v,
                },
            })
            .collect();
        let title = if known {
            copy.reuse_title.to_string()
        } else {
            // Unknown current values → the write is absolute, say so honestly.
            let bands: Vec<String> = gains.iter().map(|(p, _)| eq_band_label(p)).collect();
            format!(
                "Set the {} band{} to {:.0} dB",
                bands.join(" and "),
                if bands.len() > 1 { "s" } else { "" },
                gains[0].1
            )
        };
        return Some(Rx {
            kind: RxKind::OneClick,
            title,
            detail: copy.reuse_detail.to_string(),
            cpu_note: "no CPU change".to_string(),
            ops,
            chain: None,
        });
    }
    // A different EQ is already in the chain — advise using it (title reuses the
    // cut description), never stack a second one we can't drive value-aware.
    if facts.eq_other {
        return Some(advisory(copy.reuse_title, copy.advise_detail));
    }
    // No EQ to reuse → insert one, anchored right AFTER the cab so it shapes the
    // post-cab tone before any time-effects (not dumped at the chain's tail). No
    // cab (rare) falls back to the front group's tail.
    let anchor = match after_cab_anchor(nodes, facts) {
        Some(a) => a,
        None => CabAnchor {
            group: facts.front.as_ref().map(|(g, _)| g.clone())?,
            before: None,
            at: nodes.len(),
        },
    };
    let cpu_note = insert_cpu_note(nodes, EQ10_STEREO)?;
    Some(Rx {
        kind: RxKind::Chain,
        title: copy.insert_title.to_string(),
        detail: copy.insert_detail.to_string(),
        cpu_note,
        ops: vec![DoctorOp::InsertNode {
            group_id: anchor.group,
            before_fender_id: anchor.before,
            fender_id: EQ10_STEREO.to_string(),
            params: gains.iter().map(|(p, v)| ((*p).to_string(), *v)).collect(),
        }],
        chain: Some(chain_preview(nodes, "after · +EQ", EQ10_STEREO, anchor.at)),
    })
}

/// Player-facing label for a MEASURED frequency (a `resonant`/`boxy` peak
/// center, NOT an EQ-10 controlId — see [`eq_band_label`] for that): Hz below
/// 1 kHz (no decimal), kHz above with ONE decimal — `380.0` → "380 Hz",
/// `2_800.0` → "2.8 kHz". Distinct from [`freq_label`] (whole-number cab cut
/// frequencies) because a measured peak center is rarely a round number.
fn measured_freq_label(hz: f64) -> String {
    if hz >= 1000.0 {
        format!("{:.1} kHz", hz / 1000.0)
    } else {
        format!("{hz:.0} Hz")
    }
}

/// The 10 standard EQ-10 graphic-EQ bands (Hz, gain controlId), ascending.
/// HW-verified via `probe --doctor-inject` (a wrong id silently no-ops on the
/// device, visible as the defect not appearing in the after-capture):
/// 62/125/250/500/1k/2k/4k/8k. Only `gain31hz`/`gain16khz` remain
/// unverified — both UNREACHABLE by the resonant/boxy Rx (the 200 Hz–8 kHz
/// scan's log-nearest bands span 250 Hz–8 kHz).
const EQ10_BANDS: [(f64, &str); 10] = [
    (31.0, "gain31hz"),
    (62.0, "gain62hz"),
    (125.0, "gain125hz"),
    (250.0, "gain250hz"),
    (500.0, "gain500hz"),
    (1_000.0, "gain1khz"),
    (2_000.0, "gain2khz"),
    (4_000.0, "gain4khz"),
    (8_000.0, "gain8khz"),
    (16_000.0, "gain16khz"),
];

/// The controlId of the LOG-FREQUENCY-nearest band in `bands` to `freq_hz`
/// (log distance, not linear — EQ bands are octave-spaced).
fn nearest_band(freq_hz: f64, bands: &[(f64, &'static str)]) -> &'static str {
    bands
        .iter()
        .min_by(|a, b| {
            (a.0.ln() - freq_hz.ln())
                .abs()
                .total_cmp(&(b.0.ln() - freq_hz.ln()).abs())
        })
        .map(|&(_, id)| id)
        .expect("bands is never empty ([`EQ10_BANDS`] or a fixed slice of it)")
}

/// Nearest of all 10 [`EQ10_BANDS`] to a `resonant` peak's measured center.
fn nearest_eq10_band(freq_hz: f64) -> &'static str {
    nearest_band(freq_hz, &EQ10_BANDS)
}

/// Nearest of the 250/500 Hz pair to a `boxy` peak's measured center — the
/// task's explicit "500 Hz (or 250 Hz if nearer)" band choice.
fn nearest_boxy_band(freq_hz: f64) -> &'static str {
    nearest_band(freq_hz, &EQ10_BANDS[3..=4])
}

/// Round to the nearest 0.5 (the resonant/boxy Rx's gain step).
fn round_to_half(x: f64) -> f64 {
    (x * 2.0).round() / 2.0
}

/// The family band index whose Hz range contains `freq_hz` — `None` only if
/// it falls outside every band, which shouldn't happen for the resonant/boxy
/// scan ranges (both sit inside `Family::bands`'s 60 Hz–12 kHz span) but is
/// handled safely (an unmapped peak is treated as "covered" by the caller,
/// same `None` = allow convention every other coverage gate in this file uses).
fn band_index_for_freq(instrument: Family, freq_hz: f64) -> Option<usize> {
    instrument
        .bands()
        .iter()
        .position(|&(lo, hi)| freq_hz >= f64::from(lo) && freq_hz <= f64::from(hi))
}

/// The resonant/boxy Rx: a cut of `−min(max_cut_db, height_db/2)` (rounded to
/// the nearest 0.5) on `band_id`, via the SAME value-aware reuse/insert/
/// advisory machinery [`eq_move`] gives every other EQ move. `nodes`/`facts`
/// absent (no graph) → no Rx, same as every other graph-dependent
/// prescription in this file.
fn localized_cut_rx(
    nodes: Option<&[DoctorNode]>,
    facts: &Option<GraphFacts>,
    band_id: &'static str,
    max_cut_db: f64,
    height_db: f64,
    copy: &EqCopy,
) -> Vec<Rx> {
    let (Some(nodes), Some(facts)) = (nodes, facts.as_ref()) else {
        return Vec::new();
    };
    let gain = round_to_half(-(height_db / 2.0).min(max_cut_db));
    eq_move(nodes, facts, copy, &[(band_id, gain)])
        .into_iter()
        .collect()
}

/// A cut-filter move on the existing cab (standalone only — the amp-embedded
/// cab's `hpf`/`lpf` share the schema but ride the amp node; supported the
/// same way) or an `ACD_HighLowPass` insert when the chain has no cab.
fn cut_move(
    nodes: &[DoctorNode],
    facts: &GraphFacts,
    is_low_cut: bool, // true = HPF (boomy), false = LPF (fizzy)
    freq: f64,
    cab_title: &str,
    insert_title: &str,
    detail: &str,
) -> Option<Rx> {
    if let Some((group, node, params)) = &facts.cab {
        // Both the standalone cab and CabIR amps expose `hpf` (20–500) /
        // `lpf` (1000–20000) — schema-verified, same controlIds.
        let param = if is_low_cut { "hpf" } else { "lpf" };
        // Value-aware, like `eq_move` / the washed reverb-mix rule: a blind write
        // can push the cut the WRONG way. Low cut (hpf) TIGHTENS by RAISING toward
        // 90 Hz; high cut (lpf) tames fizz by LOWERING toward 8 kHz.
        let title = match params.get(param).copied() {
            // Known AND already at/past the target → the one-click would move it
            // backwards (loosen a cut that's already there), worsening the problem.
            // Bail so the caller's advisory fallback fires instead.
            Some(cur) if (is_low_cut && cur >= freq) || (!is_low_cut && cur <= freq) => {
                return None;
            }
            // Known and on the WRONG side → the directional one-click (the caller's
            // "Raise…" / "Lower…" title is honest here).
            Some(_) => cab_title.to_string(),
            // Unknown current value → keep today's blind write, but retitle
            // honestly (it may raise OR lower): "Set the cab's …" like `eq_move`.
            None => format!(
                "Set the cab's {} to {}",
                if is_low_cut { "low cut" } else { "high cut" },
                freq_label(freq),
            ),
        };
        return Some(Rx {
            kind: RxKind::OneClick,
            title,
            detail: detail.to_string(),
            cpu_note: "no CPU change".to_string(),
            ops: vec![DoctorOp::Param {
                group_id: group.clone(),
                node_id: node.clone(),
                param: param.to_string(),
                value: freq,
            }],
            chain: None,
        });
    }
    let group = facts.front.as_ref().map(|(g, _)| g.clone())?;
    let cpu_note = insert_cpu_note(nodes, HIGH_LOW_PASS)?;
    let param = if is_low_cut { "hpffc" } else { "lpffc" };
    Some(Rx {
        kind: RxKind::OneClick,
        title: insert_title.to_string(),
        detail: detail.to_string(),
        cpu_note,
        ops: vec![DoctorOp::InsertNode {
            group_id: group,
            before_fender_id: None,
            fender_id: HIGH_LOW_PASS.to_string(),
            params: vec![(param.to_string(), freq)],
        }],
        chain: None,
    })
}

/// The comp-aware early-out shared by the compressor moves: an ACTIVE comp
/// already in the chain → advisory to work its knob (per-caller copy); a
/// BYPASSED one → advisory to switch it back on; none → None (the caller
/// inserts one).
fn comp_present_advisory(
    facts: &GraphFacts,
    active_title: &str,
    active_detail: &str,
    bypassed_detail: &str,
) -> Option<Rx> {
    match facts.comp {
        Some(false) => Some(advisory(active_title, active_detail)),
        Some(true) => Some(advisory("Switch your compressor back on", bypassed_detail)),
        None => None,
    }
}

/// The compressor move (lost / buried). Comp-aware: an ACTIVE comp already in
/// the chain → advisory to raise its sustain; a BYPASSED one → advisory to
/// switch it back on; none → insert one in front.
fn comp_front(nodes: &[DoctorNode], facts: &GraphFacts) -> Option<Rx> {
    if let Some(a) = comp_present_advisory(
        facts,
        "Bring up the sustain on the compressor you already have",
        "Your chain already runs a compressor — raising its sustain evens out your picking without adding another block.",
        "There's a compressor in the chain but it's switched off — turning it back on evens out your picking.",
    ) {
        return Some(a);
    }
    let (group, first_fid) = facts.front.clone()?;
    let cpu_note = insert_cpu_note(nodes, COMPRESSOR)?;
    Some(Rx {
        kind: RxKind::Chain,
        title: "Add a compressor in front".to_string(),
        detail: "Evens out your picking so the guitar holds a steady spot in the mix.".to_string(),
        cpu_note,
        ops: vec![DoctorOp::InsertNode {
            group_id: group,
            before_fender_id: Some(first_fid),
            fender_id: COMPRESSOR.to_string(),
            params: Vec::new(),
        }],
        chain: Some(chain_preview(nodes, "after · +COMP", COMPRESSOR, 0)),
    })
}

/// The post-cab compressor move (spiky). Comp-aware like `comp_front`, but
/// the insert lands immediately AFTER the cab — studio-style channel
/// compression taming output swings. No cab (and no comp) → None; the
/// caller's advisory covers it.
fn comp_after_cab(nodes: &[DoctorNode], facts: &GraphFacts) -> Option<Rx> {
    if let Some(a) = comp_present_advisory(
        facts,
        "Turn up the compression on the compressor you already have",
        "Your chain already runs a compressor — raising its compression (or sustain) knob reins in the level swings without adding another block.",
        "There's a compressor in the chain but it's switched off — turning it back on reins in the level swings.",
    ) {
        return Some(a);
    }
    let anchor = after_cab_anchor(nodes, facts)?;
    // A reverb/delay earlier in the cab's own group would then also sit before
    // the inserted comp — compressing its wet tail pumps, so bail to the
    // advisory-only path (the caller's None branch) instead of anchoring a
    // placement that contradicts the prescription's own detail text.
    let time_effect_before_cab = nodes[..anchor.at - 1]
        .iter()
        .any(|n| n.group_id == anchor.group && !n.bypassed && is_time_effect(&n.model));
    if time_effect_before_cab {
        return None;
    }
    let cpu_note = insert_cpu_note(nodes, COMPRESSOR_STUDIO)?;
    Some(Rx {
        kind: RxKind::Chain,
        title: "Add a studio compressor after the cab".to_string(),
        detail: "Evens out the level after the cab, transparently — the right fix when the swings come from your playing rather than an effect doing its job."
            .to_string(),
        cpu_note,
        ops: vec![DoctorOp::InsertNode {
            group_id: anchor.group.clone(),
            before_fender_id: anchor.before,
            fender_id: COMPRESSOR_STUDIO.to_string(),
            params: Vec::new(),
        }],
        chain: Some(chain_preview(nodes, "after · +COMP", COMPRESSOR_STUDIO, anchor.at)),
    })
}

/// Generate the prescriptions for one diagnosis against the ACTUAL preset
/// graph. Prescriptions whose insert fails the firmware caps are dropped.
pub fn generate_rx(diag_key: &str, nodes: &[DoctorNode], _instrument: Instrument) -> Vec<Rx> {
    let facts = graph_facts(nodes);
    match diag_key {
        "muddy" => {
            let mut rx = Vec::new();
            if let Some(m) = eq_move(
                nodes,
                &facts,
                &EqCopy {
                    reuse_title: "Cut 3 dB around 300 Hz on your EQ",
                    reuse_detail: "Dips the muddy low-mids on the EQ you already have — the note stays, the mud goes.",
                    insert_title: "Add a 10-band EQ and cut 3 dB around 300 Hz",
                    insert_detail: "Puts a graphic EQ after the cab and dips the muddy low-mids — the note stays, the mud goes.",
                    advise_detail: "Your chain already runs a graphic EQ — pull its band nearest 300 Hz down about 3 dB, rather than adding a second EQ.",
                },
                &[("gain250hz", -3.0)],
            ) {
                rx.push(m);
            }
            rx.push(advisory(
                "Or roll the amp's Bass back a notch",
                "If you'd rather not add a block, turning Bass down 1–2 on the amp does most of the same job.",
            ));
            rx
        }
        "boomy" => {
            let mut rx: Vec<Rx> = cut_move(
                nodes,
                &facts,
                true,
                90.0,
                "Raise the cab's low cut to 90 Hz",
                "Add a low cut at 90 Hz",
                "Rolls off the sub-lows the speaker can't use anyway, so the low end tightens up.",
            )
            .into_iter()
            .collect();
            if rx.is_empty() {
                rx.push(advisory(
                    "Roll the amp's Bass back a notch",
                    "If a low-cut block won't fit, turning Bass down 1–2 on the amp does most of the same job.",
                ));
            }
            rx
        }
        "harsh" => {
            let mut rx = vec![advisory(
                "Nudge Presence (and Treble) down a notch",
                "This peak lives on the amp's Presence and Treble — easing them off by one is the quickest fix.",
            )];
            // Detection is band 3 (1–3 kHz), so the cut targets the SAME band:
            // the 1 kHz + 2 kHz EQ-10 bands.
            if let Some(m) = eq_move(
                nodes,
                &facts,
                &EqCopy {
                    reuse_title: "Cut 2 dB around 1–3 kHz",
                    reuse_detail: "Dips the harsh high-mids right where the spike lives and leaves the rest of the tone alone.",
                    insert_title: "Cut 2 dB around 1–3 kHz",
                    insert_detail: "Dips the harsh high-mids right where the spike lives and leaves the rest of the tone alone.",
                    advise_detail: "Your chain already runs a graphic EQ — pull its 1–3 kHz bands down about 2 dB, rather than adding a second EQ.",
                },
                &[("gain1khz", -2.0), ("gain2khz", -2.0)],
            ) {
                rx.push(m);
            }
            rx
        }
        "fizzy" => {
            let mut rx: Vec<Rx> = cut_move(
                nodes,
                &facts,
                false,
                8000.0,
                "Lower the cab's high cut to tame the fizz",
                "Add a high cut at 8 kHz",
                "Pulls the cabinet's high cut down to about 8 kHz, which is where the fizz lives.",
            )
            .into_iter()
            .collect();
            if rx.is_empty() {
                rx.push(advisory(
                    "Ease the amp's Presence/Treble",
                    "If a high-cut block won't fit, backing Presence and Treble off a notch tames the fizz.",
                ));
            }
            rx
        }
        "washed" => match &facts.reverb_mix {
            // Value-aware: only set the mix when it's KNOWN to sit above the
            // 25% target — a blind write on an already-low mix would RAISE it
            // (the wash is delay-driven then, not this reverb's).
            Some((group, node, param, Some(cur))) if *cur > 0.25 => vec![Rx {
                kind: RxKind::OneClick,
                title: "Bring the reverb mix down to 25%".to_string(),
                detail: "Keeps the space but lets the dry note lead again.".to_string(),
                cpu_note: "no CPU change".to_string(),
                ops: vec![DoctorOp::Param {
                    group_id: group.clone(),
                    node_id: node.clone(),
                    param: param.clone(),
                    value: 0.25,
                }],
                chain: None,
            }],
            // Dwell-style springs (and delay wash) have no wet/dry mix to set;
            // an unknown or already-low mix means the wash lives elsewhere.
            _ => vec![advisory(
                "Turn the reverb (or delay) down a touch",
                "This one has no single mix knob Doctor can set — ease the reverb's dwell or the delay's level until the dry note leads again.",
            )],
        },
        "lost" => {
            let mut rx = vec![advisory(
                "Nudge Mids up a notch on the amp",
                "Mids are what cut through a band — bringing them up one or two is the honest fix.",
            )];
            if let Some(m) = comp_front(nodes, &facts) {
                rx.push(m);
            }
            rx
        }
        // The handoff's parallel clean/driven split is NOT wire-expressible:
        // insert/replace/remove operate inside existing groups, and no session
        // op creates a new split topology — so Doctor prescribes the honest
        // expressible fixes instead (comp in front + ease the drive).
        "buried" => {
            let mut rx = Vec::new();
            if let Some(m) = comp_front(nodes, &facts) {
                // Only the INSERT variant gets the buried-specific detail — the
                // comp-aware advisories keep their own copy.
                let m = if m.kind == RxKind::Chain {
                    Rx {
                        detail:
                            "Evens the picking out so the clean low end holds its spot under the drive."
                                .to_string(),
                        ..m
                    }
                } else {
                    m
                };
                rx.push(m);
            }
            rx.push(advisory(
                "Ease the drive's gain and bring its level up",
                "Less gain, more level keeps the grit but stops it swallowing the clean low end.",
            ));
            rx
        }
        "spiky" => {
            let mut rx = vec![advisory(
                "Tame the swings at the source",
                "If the swings come from a volume swell, tremolo, or a delay building up, easing that effect's depth or level is the honest fix — a compressor would flatten the effect.",
            )];
            if let Some(m) = comp_after_cab(nodes, &facts) {
                rx.push(m);
            }
            rx
        }
        // dark/bright/thin are broadband tilt/local findings, not a single
        // block's fault — advisory-only. ponytail deviation from the frozen
        // spec: dark's "reuse the EQ as a lift" branch is SKIPPED (advisory-only
        // for both directions) rather than reusing `eq_move`'s cut-only helper —
        // no verified EQ-10 band controlId exists in this codebase past
        // gain1khz/gain2khz (harsh) and gain250hz (muddy), and this codebase's
        // exact-id-matching discipline (see the module doc) makes guessing one
        // (e.g. a `gain4khz`/`gain8khz`) too risky to ship unverified.
        "dark" => vec![
            advisory(
                "Open the amp's treble and presence",
                "A small treble or presence lift on the amp brings the whole tone forward — try that before reaching for an EQ.",
            ),
            advisory(
                "Or tame the low end",
                "If the lows are what's heavy, easing bass on the amp gets the same balance back.",
            ),
        ],
        "bright" => vec![advisory(
            "Ease the treble/presence on the amp",
            "Backing Presence and Treble off a notch takes the brittle edge off without losing the amp's character.",
        )],
        "thin" => vec![advisory(
            "Bring up the amp's bass, or move toward the neck pickup",
            "There's little happening below the low-mids — riding up the amp's Bass, or picking closer to the neck, fills it back in.",
        )],
        // Generated inline in `apply_thresholds` ([`localized_cut_rx`]) — their
        // Rx needs the peak's MEASURED freq/height, which this key-only
        // signature can't carry. Explicit arms so the absence reads as a
        // decision, not a missing case in this otherwise-exhaustive map.
        "resonant" | "boxy" => Vec::new(),
        _ => Vec::new(),
    }
}

// ─── diagnosis ───────────────────────────────────────────────────────────────

/// Diagnose one sound. `nodes` (the preset's chain, from the backup scan's
/// graph) enriches detection (drive presence) and drives prescriptions;
/// diagnosis still works without it (graph-dependent rx are simply absent).
/// Deterministic — the verdict depends only on THIS sound (its deviation from a
/// fixed authored target curve), never on which other sounds ran.
pub fn diagnose(
    profile: &SoundProfile,
    nodes: Option<&[DoctorNode]>,
    instrument: Family,
) -> Vec<Diag> {
    // Back-compat shim: the synthetic table + no band gating + no playback offset
    // (Rehearsal anchor) == the pinned pre-capture behavior (synthetic stimuli
    // cover every band by construction).
    diagnose_kind(
        profile,
        nodes,
        instrument,
        StimulusKind::Synthetic,
        None,
        PlaybackOffsets::NONE,
    )
}

/// Level-invariant per-sound metrics `diagnose_kind` derives from `profile` /
/// `nodes` / `instrument` / `coverage` — everything the band rules read that
/// does NOT depend on `offsets` (offsets only shift per-rule thresholds).
/// Split out ([`compute_rule_metrics`]) so [`diagnose_levels`] computes this
/// ONCE per sound and re-applies thresholds three times ([`apply_thresholds`])
/// instead of re-running the whole deviation/tilt/graph pipeline per playback
/// level.
struct RuleMetrics {
    bdb: Vec<f64>,
    // Kept alongside `locals`/`centered` (both derived from it) though
    // `apply_thresholds` itself only reads `bdb` directly (the fizzy rule).
    #[allow(dead_code)]
    dev: Vec<f64>,
    slope: Option<f64>,
    locals: Vec<f64>,
    centered: Vec<f64>,
    facts: Option<GraphFacts>,
}

/// Deviation-from-target + Theil–Sen tilt/local split: each band's dB above
/// the authored target curve, then decomposed into a broadband TILT (the
/// whole-tone dark/bright lean) and per-band LOCAL bumps (muddy/boomy/harsh/
/// lost/buried/thin) — level-invariant by construction (no mean removal), and
/// robust to a single-band anomaly. The same space the `--doctor-calib` sweep
/// derives thresholds in. Also derives the median-centered space (see
/// [`centered_deviations`]'s doc) — the SECOND space every band rule must
/// ALSO clear (CONSENSUS: R5 HW defect injection showed the tilt-split local
/// alone misattributes a skirted single-band defect to a neighboring rule,
/// and the centered space alone is contaminated by healthy tilt at the
/// endpoint bands).
fn compute_rule_metrics(
    profile: &SoundProfile,
    nodes: Option<&[DoctorNode]>,
    instrument: Family,
    coverage: Option<&[bool]>,
) -> RuleMetrics {
    let bdb = band_db(&profile.bands);
    let dev = deviations(&bdb, instrument);
    let (slope, locals) = tilt_split(&dev, instrument, coverage);
    let centered = centered_deviations(&dev, instrument);
    let facts = nodes.map(graph_facts);
    RuleMetrics {
        bdb,
        dev,
        slope,
        locals,
        centered,
        facts,
    }
}

/// The two-space consensus gate for a band rule: the margin in each space,
/// combined as the SMALLER one (severity), firing only when BOTH clear —
/// the false-positive control lives in the intersection (see `Thresholds`).
fn consensus(tilt_val: f64, tilt_gate: f64, centered_val: f64, centered_gate: f64) -> (f64, bool) {
    let margin_tilt = tilt_val - tilt_gate;
    let margin_centered = centered_val - centered_gate;
    (
        margin_tilt.min(margin_centered),
        margin_tilt > 0.0 && margin_centered > 0.0,
    )
}

/// Diagnose one sound with an explicit stimulus `kind` (picks the threshold
/// table) and optional band `coverage` — per-band excitation of whatever
/// coverage source the caller supplies (the input stimulus via [`coverage`],
/// or the captured output via [`output_coverage`]): a band-keyed rule whose
/// primary band is UNCOVERED is skipped — a sparse capture must not produce
/// verdicts in bands it never excited. `coverage = None` disables gating
/// (all bands treated as covered). The localized `resonant`/`boxy` rules read
/// `profile.peaks` (populated by [`SoundProfile::from_capture_with_psd`];
/// empty on PSD-less profiles, silently skipping both).
pub fn diagnose_kind(
    profile: &SoundProfile,
    nodes: Option<&[DoctorNode]>,
    instrument: Family,
    kind: StimulusKind,
    coverage: Option<&[bool]>,
    offsets: PlaybackOffsets,
) -> Vec<Diag> {
    let metrics = compute_rule_metrics(profile, nodes, instrument, coverage);
    apply_thresholds(
        &metrics, profile, nodes, instrument, kind, coverage, offsets,
    )
}

/// Threshold-application half of [`diagnose_kind`]: takes the level-invariant
/// [`RuleMetrics`] plus everything that DOES vary with playback level
/// (`kind`/`coverage`/`offsets`) and produces the fired [`Diag`]s. An exact
/// behavioral split of the old single-function body — driven once directly by
/// `diagnose_kind`, and three times (over one shared `RuleMetrics`) by
/// `diagnose_levels`.
fn apply_thresholds(
    metrics: &RuleMetrics,
    profile: &SoundProfile,
    nodes: Option<&[DoctorNode]>,
    instrument: Family,
    kind: StimulusKind,
    coverage: Option<&[bool]>,
    offsets: PlaybackOffsets,
) -> Vec<Diag> {
    let t = instrument.thresholds_for(kind);
    // A rule keyed on band `i` fires only when the coverage source excited it.
    let covered = |i: usize| band_covered(coverage, i);
    let bdb = &metrics.bdb;
    let locals = &metrics.locals;
    let centered = &metrics.centered;
    let (lows, low_mids, mids, high_mids, highs, air) = instrument.semantic_bands();
    let facts = &metrics.facts;
    let mut out = Vec::new();
    // `margin` = metric − (offset-adjusted) threshold, ≥ 0 for a fired card. It
    // IS the severity (magnitude past threshold).
    let mut push = |key: &'static str,
                    label: String,
                    sev: Sev,
                    margin: f64,
                    bands: Vec<usize>,
                    detail: String,
                    explain: &'static str| {
        let rx = nodes
            .map(|n| generate_rx(key, n, instrument))
            .unwrap_or_default();
        out.push(Diag {
            key,
            label,
            sev,
            severity: margin,
            bands,
            detail,
            explain,
            rx,
        });
    };

    // Every rule fires on `margin > 0` (metric strictly past the offset-adjusted
    // threshold). Band rules (muddy/boomy/harsh/lost/thin/buried) additionally
    // need the CENTERED-space margin to also clear its own gate (`consensus`) —
    // severity is the smaller of the two, so a boundary consensus fire reads as
    // "Possible".
    // ponytail: guard-band widening (surfacing below-threshold cards) is a later,
    // HW-σ-calibrated step — keep the raw `> threshold` fire condition here.
    let (muddy_margin, muddy_fires) = consensus(
        locals[low_mids],
        t.muddy_db + offsets.low_end_db,
        centered[low_mids],
        t.muddy_centered_db + offsets.low_end_db,
    );
    if covered(low_mids) && muddy_fires {
        push(
            "muddy",
            "Muddy".to_string(),
            Sev::High,
            muddy_margin,
            vec![low_mids],
            format!("buildup around 250–350 Hz ({:+.1} dB)", locals[low_mids]),
            "There's a buildup in the low-mids that stacks up with the bass player.",
        );
    }
    let (boomy_margin, boomy_fires) = consensus(
        locals[lows],
        t.boomy_db + offsets.low_end_db,
        centered[lows],
        t.boomy_centered_db + offsets.low_end_db,
    );
    if covered(lows) && boomy_fires {
        push(
            "boomy",
            "Boomy".to_string(),
            Sev::Med,
            boomy_margin,
            vec![lows],
            format!("excess energy below 100 Hz ({:+.1} dB)", locals[lows]),
            "Too much deep low end — it booms and loses focus once you turn up.",
        );
    }
    let (harsh_margin, harsh_fires) = consensus(
        locals[high_mids],
        t.harsh_db,
        centered[high_mids],
        t.harsh_centered_db,
    );
    if covered(high_mids) && harsh_fires {
        push(
            "harsh",
            "Harsh".to_string(),
            Sev::High,
            harsh_margin,
            vec![high_mids],
            format!("spike around 1–3 kHz ({:+.1} dB)", locals[high_mids]),
            "A sharp peak in the high-mids makes it harsh and tiring to listen to.",
        );
    }
    // Fizz is the Air band failing to roll off below the presence band — a
    // difference of the two bands' own dB (self-mean cancels, so it's identical
    // to the old balance-space form). See `Thresholds::fizzy_db`. Own-spectrum,
    // so it needs BOTH the Air and Highs bands actually excited. On a real DI
    // capture, gate on the top octave's spectral flatness (noise-like HF hash vs
    // a merely bright cab) — inert on the synthetic stimulus (see
    // `FIZZY_MIN_FLATNESS`).
    let fizzy_margin = (bdb[air] - bdb[highs]) - (t.fizzy_db + offsets.fizzy_db);
    let fizzy_gated = kind != StimulusKind::Capture || profile.air_flatness >= FIZZY_MIN_FLATNESS;
    if covered(air) && covered(highs) && fizzy_margin > 0.0 && fizzy_gated {
        push(
            "fizzy",
            "Fizzy".to_string(),
            Sev::Med,
            fizzy_margin,
            vec![air],
            format!(
                "content above 6 kHz only {:.1} dB under the presence band",
                bdb[highs] - bdb[air]
            ),
            "Fizzy, buzzy top end — the kind that sounds like radio static on the note tails.",
        );
    }
    let (lost_margin, lost_fires) = consensus(
        -locals[mids],
        t.lost_db,
        -centered[mids],
        t.lost_centered_db,
    );
    if covered(mids) && lost_fires {
        push(
            "lost",
            "Gets lost in the mix".to_string(),
            Sev::High,
            lost_margin,
            vec![mids],
            format!("mids scooped {:.1} dB around 800 Hz", -locals[mids]),
            "The mids are scooped, so it sounds big alone but disappears with a full band.",
        );
    }
    // Broadband tilt (dark/bright) and the guitar-only thin local: all three
    // need a determined slope (≥3 fit points survived `covered` gating) — a
    // fit anchored on fewer points is too unreliable to support a whole-tone or
    // local verdict.
    if let Some(s) = metrics.slope {
        let tilt_bands = vec![highs, air];
        let tilt_margin = s.abs() - t.tilt_db_per_oct;
        if tilt_margin > 0.0 {
            let (key, label, dir, explain) = if s < 0.0 {
                (
                    "dark",
                    "Dark",
                    "darker",
                    "The whole tone leans dark — the top end is shy across the board, so it sounds dull and boxed-in.",
                )
            } else {
                (
                    "bright",
                    "Bright",
                    "brighter",
                    "The whole tone leans bright — it gets brittle and tiring, with little warmth underneath.",
                )
            };
            push(
                key,
                label.to_string(),
                Sev::Med,
                tilt_margin,
                tilt_bands,
                format!(
                    "tilted {:.1} dB/octave {dir} than the target voicing",
                    s.abs()
                ),
                explain,
            );
        }
        // Bass low-deficit is `buried`'s domain (needs a drive + graph); thin is
        // the graph-free, guitar-only counterpart.
        if instrument == Family::Guitar {
            let (thin_margin, thin_fires) = consensus(
                -locals[lows],
                t.thin_db,
                -centered[lows],
                t.thin_centered_db,
            );
            if covered(lows) && thin_fires {
                push(
                    "thin",
                    "Thin".to_string(),
                    Sev::Med,
                    thin_margin,
                    vec![lows],
                    format!("low end {:.1} dB under the target voicing", -locals[lows]),
                    "The low end is missing, so the tone sounds small and never fills the room.",
                );
            }
        }
    }
    let washed_margin = profile.tail_ratio_db - t.wash_tail_db;
    if washed_margin > 0.0 {
        let detail = format!(
            "decay tail only {:.0} dB under the note",
            -profile.tail_ratio_db
        );
        push(
            "washed",
            "Washed out".to_string(),
            Sev::Med,
            washed_margin,
            vec![],
            detail,
            "The reverb is drowning the note — it washes out instead of ringing clearly.",
        );
    }
    // Spread is gain-invariant and preset-intrinsic (lufs::spread_lu), so it's
    // judged absolutely, cohort-independent — like fizzy. Graph required: a
    // drive pedal compresses naturally, so spiky is a CLEAN-chain finding.
    let spiky_margin = profile.spread_lu - t.spiky_spread_lu;
    if spiky_margin > 0.0 && facts.as_ref().map(|f| f.has_drive) == Some(false) {
        push(
            "spiky",
            "Spiky".to_string(),
            Sev::Med,
            spiky_margin,
            vec![],
            format!("swings {:.1} LU between peaks and average", profile.spread_lu),
            "The level jumps between loud peaks and a much quieter average — it pokes out of the mix one moment and disappears the next.",
        );
    }
    let (buried_margin, buried_fires) = consensus(
        -locals[lows],
        t.buried_lows_db,
        -centered[lows],
        t.buried_centered_db,
    );
    if matches!(instrument, Family::Bass | Family::BassVi)
        && facts.as_ref().map(|f| f.has_drive) == Some(true)
        && covered(lows)
        && buried_fires
    {
        push(
            "buried",
            "Buried clean tone".to_string(),
            Sev::Med,
            buried_margin,
            vec![],
            "drive stacked in series".to_string(),
            "Your clean low end is buried under the drive — common on a driven bass sound.",
        );
    }

    // ── Localized resonance, off the FINE Welch PSD (`profile.peaks`, empty
    // on PSD-less profiles — both rules then silently no-op, same as every
    // other Option-gated rule input this function reads). Built into a
    // separate Vec (not the `push` closure above, shaped for the band rules)
    // and appended at the end.
    let mut localized: Vec<Diag> = Vec::new();
    // `boxy` is the more specific verdict when a peak's center sits in its
    // 300–500 Hz range (see its doc) — picked first so the resonant search
    // below can exclude the SAME peak from also firing.
    let boxy_pick = profile
        .peaks
        .iter()
        .filter(|p| {
            (BOXY_LO_HZ..=BOXY_HI_HZ).contains(&p.freq_hz) && p.height_db >= BOXY_MIN_HEIGHT_DB
        })
        .max_by(|a, b| a.height_db.total_cmp(&b.height_db));
    if let Some(peak) = boxy_pick {
        let band = band_index_for_freq(instrument, peak.freq_hz);
        if band.is_none_or(covered) {
            let freq = measured_freq_label(peak.freq_hz);
            let band_id = nearest_boxy_band(peak.freq_hz);
            let band_label = eq_band_label(band_id);
            localized.push(Diag {
                key: "boxy",
                label: format!("Boxy (a {freq} hump)"),
                sev: Sev::Med,
                severity: peak.height_db - BOXY_MIN_HEIGHT_DB,
                bands: band.into_iter().collect(),
                detail: format!(
                    "{freq} hump, {:.1} dB above the surrounding spectrum",
                    peak.height_db
                ),
                explain: "A narrow bump in the low-mids reads as boxy — a closed-in, cardboard coloration rather than a broad muddy buildup.",
                rx: {
                    let title = format!("Cut the {band_label} band");
                    localized_cut_rx(
                        nodes,
                        facts,
                        band_id,
                        4.0,
                        peak.height_db,
                        &EqCopy {
                            reuse_title: &title,
                            reuse_detail: "Dips the boxy hump right where it measures, on the EQ you already have.",
                            insert_title: &title,
                            insert_detail: "Puts a graphic EQ after the cab and dips the boxy hump right where it measures.",
                            advise_detail: "Your chain already runs a graphic EQ — pull its band nearest the hump down, rather than adding a second EQ.",
                        },
                    )
                },
            });
        }
    }
    let resonant_pick = profile
        .peaks
        .iter()
        .filter(|p| p.height_db >= RESONANT_MIN_HEIGHT_DB && p.q >= RESONANT_MIN_Q)
        .max_by(|a, b| a.height_db.total_cmp(&b.height_db));
    if let Some(peak) = resonant_pick {
        // Same peak `boxy` already claimed → boxy is the more specific verdict
        // (see its doc); don't also fire resonant.
        let claimed_by_boxy = (BOXY_LO_HZ..=BOXY_HI_HZ).contains(&peak.freq_hz);
        let band = band_index_for_freq(instrument, peak.freq_hz);
        if !claimed_by_boxy && band.is_none_or(covered) {
            let freq = measured_freq_label(peak.freq_hz);
            let band_id = nearest_eq10_band(peak.freq_hz);
            let band_label = eq_band_label(band_id);
            localized.push(Diag {
                key: "resonant",
                label: format!("Rings at {freq}"),
                sev: Sev::High,
                severity: peak.height_db - RESONANT_MIN_HEIGHT_DB,
                bands: band.into_iter().collect(),
                detail: format!(
                    "{freq}, {:.1} dB above the surrounding spectrum, Q {:.1}",
                    peak.height_db, peak.q
                ),
                explain: "A narrow spectral peak rings out and colors the tone at that one frequency — a precise EQ cut right on it is the fix.",
                rx: {
                    let title = format!("Rings at {freq} — cut the {band_label} band");
                    localized_cut_rx(
                        nodes,
                        facts,
                        band_id,
                        6.0,
                        peak.height_db,
                        &EqCopy {
                            reuse_title: &title,
                            reuse_detail: "Dips the ring right where it measures, on the EQ you already have.",
                            insert_title: &title,
                            insert_detail: "Puts a graphic EQ after the cab and dips the ring right where it measures.",
                            advise_detail: "Your chain already runs a graphic EQ — pull its band nearest the ring down, rather than adding a second EQ.",
                        },
                    )
                },
            });
        }
    }
    out.extend(localized);
    out
}

/// A diagnosis plus the playback levels at which it fires. `from_level` is the
/// QUIETEST level in that set (`quiet` < `rehearsal` < `stage`): `quiet` = the
/// finding is present at every volume (rendered untagged — a problem regardless
/// of how loud you play), `rehearsal`/`stage` = it only appears at that volume
/// and louder. The playback offsets are monotonic in loudness (louder ⇒ tighter
/// thresholds ⇒ strictly more firings, asserted by
/// `playback_offsets_are_monotonic`), so the firing set is always a louder-suffix
/// and `from_level` describes it completely.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeveledDiag {
    #[serde(flatten)]
    pub diag: Diag,
    pub from_level: PlaybackLevel,
}

/// Diagnose one sound at ALL THREE playback levels and return each finding ONCE,
/// tagged with the quietest level it fires at. The capture is level-independent
/// (the offset shifts the comparison THRESHOLD, not the measured deviation), so a
/// finding's content — label, detail, bands, rx — is identical across levels;
/// this computes the level-invariant [`RuleMetrics`] ONCE ([`compute_rule_metrics`])
/// and re-applies thresholds three times ([`apply_thresholds`]) over the one
/// profile, merging by diagnosis key, rather than re-running the whole
/// deviation/tilt/graph pipeline per level. Iterating quietest→loudest means the
/// FIRST time a key appears is at its quietest firing level, so `from_level` is
/// correct on insert and later (louder) passes only re-confirm. First-seen order
/// is preserved (quiet-firing findings first, then rehearsal-only, then
/// stage-only). Cheap: one metrics pass + three microsecond-scale threshold
/// passes vs the one ~11 s hardware capture upstream.
pub fn diagnose_levels(
    profile: &SoundProfile,
    nodes: Option<&[DoctorNode]>,
    instrument: Family,
    kind: StimulusKind,
    coverage: Option<&[bool]>,
) -> Vec<LeveledDiag> {
    // Quietest → loudest: louder tightens the offset-keyed thresholds, so each
    // level is a superset of the quieter one's firings. First-seen wins, which is
    // the quietest firing level; a linear key check keeps first-seen order (≤ 8
    // diagnoses per sound, so O(n²) is a non-issue).
    let levels = [
        PlaybackLevel::Quiet,
        PlaybackLevel::Rehearsal,
        PlaybackLevel::Stage,
    ];
    let metrics = compute_rule_metrics(profile, nodes, instrument, coverage);
    let mut out: Vec<LeveledDiag> = Vec::new();
    for level in levels {
        let diags = apply_thresholds(
            &metrics,
            profile,
            nodes,
            instrument,
            kind,
            coverage,
            playback_offsets(level),
        );
        for diag in diags {
            if !out.iter().any(|d| d.diag.key == diag.key) {
                out.push(LeveledDiag {
                    diag,
                    from_level: level,
                });
            }
        }
    }
    out
}

// ─── scene consistency ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneDeltaRow {
    pub name: String,
    /// FS tag ("FS1"…), None for the base row.
    pub tag: Option<String>,
    /// LUFS delta vs the base scene (0 for the base row).
    pub delta_db: f64,
    pub is_ref: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneConsistency {
    pub rows: Vec<SceneDeltaRow>,
    pub worst_name: String,
    pub worst_delta_db: f64,
    pub rx: Vec<Rx>,
}

/// Sound-loudness consistency: deltas vs the base sound, flagged when any
/// scene or footswitch sound jumps more than the threshold. `others` =
/// (name, fs-tag, integrated LUFS, wire scene index): the last element is
/// `Some(index)` for a scene sound and `None` for a footswitch sound. Returns
/// None when nothing jumps.
pub fn scene_consistency(
    base_name: &str,
    base_lufs: f64,
    others: &[(String, Option<String>, f64, Option<u32>)],
    instrument: Instrument,
) -> Option<SceneConsistency> {
    let t = instrument.thresholds();
    // A non-finite base (silent capture) can't anchor any delta; non-finite
    // rows are skipped below — belt-and-braces with the from_capture guard.
    if others.is_empty() || !base_lufs.is_finite() {
        return None;
    }
    let mut rows = vec![SceneDeltaRow {
        name: base_name.to_string(),
        tag: None,
        delta_db: 0.0,
        is_ref: true,
    }];
    let mut worst: Option<(&str, f64, Option<u32>)> = None;
    for (name, tag, lufs, scene) in others {
        if !lufs.is_finite() {
            continue;
        }
        let delta = lufs - base_lufs;
        rows.push(SceneDeltaRow {
            name: name.clone(),
            tag: tag.clone(),
            delta_db: delta,
            is_ref: false,
        });
        if worst.map(|(_, w, _)| delta.abs() > w.abs()) != Some(false) {
            worst = Some((name, delta, *scene));
        }
    }
    let (worst_name, worst_delta, worst_scene) = worst?;
    if worst_delta.abs() <= t.scene_delta_db {
        return None;
    }
    let rx = if worst_delta < 0.0 {
        // Quieter-than-base worst: trimming DOWN is wrong — leveling it UP is
        // the Level tab's job, so advise instead of prescribing a SceneTrim.
        vec![advisory(
            &format!("{worst_name} is much quieter than the others"),
            &format!(
                "{worst_name} sits {:.1} dB under the base sound — if that's not intentional, bring it up from the Level tab.",
                -worst_delta
            ),
        )]
    } else {
        match worst_scene {
            // A block-acting FOOTSWITCH sound louder than base: there's no
            // wire scene index to trim, so advise leveling it (the Level tab
            // levels footswitch sounds) or backing off the block's knob.
            None => vec![advisory(
                &format!("{worst_name} is much louder than the base sound"),
                &format!(
                    "{worst_name} jumps {worst_delta:+.1} dB when you step on it — pros keep it to +1–3 dB. Level it from the Level tab (it can level footswitch sounds), or back off the block's level knob."
                ),
            )],
            // The open loadScene(0) anomaly: USB scene-0 recall can
            // materialize a different amp state than the physical footswitch
            // tap, so its reading isn't trustworthy enough for a wire trim —
            // ask for ears instead.
            Some(0) => vec![advisory(
                &format!("Verify {worst_name} by ear"),
                &format!(
                    "{worst_name} measured {worst_delta:+.1} dB vs the base scene, but the first scene's USB reading can differ from the footswitch (a known device quirk) — check it by ear before leveling."
                ),
            )],
            // A louder scene: Doctor has no in-app scene trim (the wire can't
            // set scene loudness relative to base, and Level-tab leveling targets
            // an absolute LUFS), so ADVISE rather than promising a one-click.
            Some(_) => vec![advisory(
                &format!("{worst_name} is much louder than the base sound"),
                &format!(
                    "{worst_name} jumps {worst_delta:+.1} dB when you switch to it — pros keep lead sounds to +1–3 dB. Level it from the Level tab, or back off its amp level for that scene."
                ),
            )],
        }
    };
    Some(SceneConsistency {
        rows,
        worst_name: worst_name.to_string(),
        worst_delta_db: worst_delta,
        rx,
    })
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod onset_split_tests {
    use super::*;

    const SR: u32 = 48_000;

    /// The D3 regression: a bone-dry chain whose capture starts with 50 ms of
    /// I/O-latency silence. The un-aligned split leaks the last 50 ms of BODY
    /// into the tail window (reads ≈ −17 dB, hugging the −13 dB washed
    /// threshold); the onset-aligned split reads the true near-silent tail.
    #[test]
    fn onset_aligned_split_stops_body_leak_into_the_tail() {
        let stim_n = SR as usize * 6;
        let lag = SR as usize / 20; // 50 ms
        let tail_n = SR as usize * 5 / 2; // 2.5 s tail (illustrative; > today's 1.5 s DOCTOR_TAIL_MS)
        let mut cap = vec![0.0f32; lag];
        cap.extend(std::iter::repeat_n(0.5f32, stim_n)); // body
        cap.extend(std::iter::repeat_n(0.0005f32, tail_n)); // truly dry tail
        let unaligned = tail_energy_ratio(&cap, SR, stim_n, 0);
        let aligned = tail_energy_ratio(&cap, SR, stim_n, lag);
        assert!(
            unaligned > -20.0,
            "leak should inflate the unaligned tail (got {unaligned:.1})"
        );
        assert!(
            aligned <= -30.0,
            "aligned split must read the dry tail (got {aligned:.1})"
        );
    }

    /// A wet sound's ratio barely moves under alignment (the tail really rings).
    #[test]
    fn wet_tail_survives_alignment() {
        let stim_n = SR as usize * 6;
        let lag = SR as usize / 20;
        let tail_n = SR as usize * 5 / 2;
        let mut cap = vec![0.0f32; lag];
        cap.extend(std::iter::repeat_n(0.5f32, stim_n));
        cap.extend(std::iter::repeat_n(0.25f32, tail_n)); // ringing reverb
        let unaligned = tail_energy_ratio(&cap, SR, stim_n, 0);
        let aligned = tail_energy_ratio(&cap, SR, stim_n, lag);
        assert!((unaligned - aligned).abs() < 2.0);
        assert!(aligned > -13.0); // stays on the washed side
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `SoundProfile` sitting EXACTLY on `family`'s authored target curve,
    /// except band `band` bumped by `local` dB. Since only one of the 5 semantic
    /// BODY bands differs from the (otherwise flat-zero) target deviation, the
    /// Theil–Sen fit's slope and intercept both land EXACTLY at 0 regardless of
    /// which band is bumped or by how much — at least 6 of the 10 pairwise
    /// slopes are between two zero-deviation points, which pins the median at 0
    /// (verified against the real [`tilt_split`] in the oracle tests below) — so
    /// `tilt_split`'s `locals[band]` reads back EXACTLY `local`, no approximation.
    /// `band` must be one of the 5 body indices ([`Family::semantic_bands`]'s
    /// `lows..=highs`). `spread_lu`/`tail`/`air_flatness` are inert defaults (no
    /// spiky/washed/capture-gate effect) unless the caller overwrites them.
    fn resid_profile(family: Family, band: usize, local: f64) -> SoundProfile {
        let mut bdb: Vec<f64> = target_curve(family).to_vec();
        bdb[band] += local;
        SoundProfile {
            bands: bdb.iter().map(|d| 10f64.powf(d / 10.0)).collect(),
            integrated_lufs: -20.0,
            spread_lu: 0.0,
            tail_ratio_db: -40.0,
            air_flatness: 0.5,
            peaks: Vec::new(),
        }
    }

    fn keys(diags: &[Diag]) -> Vec<&'static str> {
        diags.iter().map(|d| d.key).collect()
    }

    // ── new-metric pure helpers ──

    #[test]
    fn band_db_is_10_log10_floored() {
        let got = band_db(&[1.0, 0.1, 0.0]);
        assert!((got[0] - 0.0).abs() < 1e-9);
        assert!((got[1] - (-10.0)).abs() < 1e-9);
        // A silent band floors at 1e-12 → −120 dB, never −inf.
        assert!((got[2] - (-120.0)).abs() < 1e-9);
    }

    #[test]
    fn target_curve_length_and_values_pinned() {
        assert_eq!(target_curve(Family::Guitar).len(), 6);
        assert_eq!(target_curve(Family::Bass).len(), 6);
        assert_eq!(target_curve(Family::BassVi).len(), 7);
        assert_eq!(target_curve(Family::Guitar)[0], -5.0);
        assert_eq!(target_curve(Family::Bass)[0], 1.5);
        assert_eq!(target_curve(Family::BassVi)[0], -18.0); // Sub
                                                            // Deliberately DIFFERENT authored opinions per family, not a shared flat
                                                            // baseline (the old provisional-zero target).
        assert_ne!(
            target_curve(Family::Guitar)[1],
            target_curve(Family::Bass)[1]
        );
    }

    #[test]
    fn deviations_is_band_db_minus_target() {
        let bands = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0]; // 0 dB in every band
        let dev = deviations(&band_db(&bands), Family::Guitar);
        for (d, t) in dev.iter().zip(GUITAR_TARGET) {
            assert!((d - (0.0 - t)).abs() < 1e-9);
        }
    }

    // ── deviation/tilt-split oracle tests (supervisor-derived constants,
    //    independent of the implementation) — build a band-dB vector, check
    //    `deviations` + `tilt_split` against precomputed slope/local values,
    //    then confirm the `diagnose()`-level firing set. Guitar/Bass x-axis
    //    (log2 of `Family::Guitar.band_centers()`): [6.406891, 7.775373,
    //    9.30482, 10.758266, 12.050747, 13.050747].
    //
    // R5-retune (2026-07-16): every vector below is re-derived against the new
    // GUITAR_TARGET/BASS_VI_TARGET and the new gates using this reference
    // implementation (independent of the Rust under test), run with python3:
    //
    //   X  = [6.406891, 7.775373, 9.30482, 10.758266, 12.050747, 13.050747]
    //   T  = [-5.0, -2.0, -5.0, 13.0, 13.5, -14.5]   # GUITAR_TARGET
    //   def theil_sen(dev, covered):   # covered = semantic body bands lows..highs
    //       pts = [(X[i], dev[i]) for i in covered]
    //       sl = sorted((y2-y1)/(x2-x1) for i,(x1,y1) in enumerate(pts)
    //                   for (x2,y2) in pts[i+1:] if x2 != x1)
    //       n = len(sl); s = sl[n//2] if n % 2 else (sl[n//2-1]+sl[n//2])/2
    //       it = sorted(y - s*x for x,y in pts)
    //       n = len(it); b = it[n//2] if n % 2 else (it[n//2-1]+it[n//2])/2
    //       return s, [dev[i]-(s*X[i]+b) for i in range(len(dev))]
    //
    // Construction convention: `bdb[i] = T[i] + L + s·X[i] + bump·e_i` (a pure
    // tilt picks any level `L`, here 0; a local-bump case adds `L` uniformly
    // then one band's `bump` on top — Theil–Sen's median then reads the tilt's
    // slope back exactly and the bump's local back exactly, see `resid_profile`
    // doc above for why). Defect magnitude is `gate + 2` unless a test wants a
    // specific co-fire/near-boundary case (noted per test).

    fn oracle_profile(bdb: &[f64]) -> SoundProfile {
        SoundProfile {
            bands: bdb.iter().map(|d| 10f64.powf(d / 10.0)).collect(),
            integrated_lufs: -20.0,
            spread_lu: 0.0,
            tail_ratio_db: -80.0,
            air_flatness: 0.5,
            peaks: Vec::new(),
        }
    }

    fn assert_locals_close(got: &[f64], want: &[f64], tol: f64) {
        assert_eq!(got.len(), want.len());
        for (i, (&g, &w)) in got.iter().zip(want).enumerate() {
            assert!((g - w).abs() <= tol, "local[{i}]: got {g}, want {w}");
        }
    }

    #[test]
    fn oracle_o1_pure_target_no_cards() {
        let bdb: Vec<f64> = target_curve(Family::Guitar)
            .iter()
            .map(|t| t + 12.0)
            .collect();
        let p = oracle_profile(&bdb);
        let dev = deviations(&bdb, Family::Guitar);
        let (slope, locals) = tilt_split(&dev, Family::Guitar, None);
        assert!((slope.unwrap() - 0.0).abs() <= 0.01);
        assert_locals_close(&locals, &[0.0; 6], 0.01);
        assert!(keys(&diagnose(&p, None, Family::Guitar)).is_empty());
    }

    #[test]
    fn oracle_o2_muddy_local_bump() {
        // python: L=8, bump=5.5 (muddy gate 3.5 + 2) on low-mids (index 1) —
        // bdb = [T[i]+8+(5.5 if i==1 else 0) for i in range(6)]
        //     = [3.0, 11.5, 3.0, 21.0, 21.5, -6.5]
        // -> slope=0.0, locals=[0,5.5,0,0,0,0], muddy margin = 5.5-3.5 = 2.0
        let bdb = [3.0, 11.5, 3.0, 21.0, 21.5, -6.5]; // target+8 uniform, +5.5 low-mids
        let p = oracle_profile(&bdb);
        let dev = deviations(&bdb, Family::Guitar);
        let (slope, locals) = tilt_split(&dev, Family::Guitar, None);
        assert!((slope.unwrap() - 0.0).abs() <= 0.01);
        assert_locals_close(&locals, &[0.0, 5.5, 0.0, 0.0, 0.0, 0.0], 0.01);
        let diags = diagnose(&p, None, Family::Guitar);
        let got = keys(&diags);
        assert!((find(&diags, "muddy").severity - 2.0).abs() < 0.01);
        for absent in ["boomy", "dark", "bright", "thin"] {
            assert!(!got.contains(&absent), "{absent} must not fire");
        }
    }

    #[test]
    fn oracle_o3_dark_tilt_only() {
        // python: L=0, s=-5.0 (tilt gate 3.0 + 2) —
        // bdb = [T[i] + s*X[i] for i in range(6)]
        //     = [-37.034453, -40.876867, -51.524101, -40.791328, -46.753734,
        //        -79.753734]
        // -> slope=-5.0, locals≈[0;6], dark margin = 5.0-3.0 = 2.0
        let bdb = [
            -37.034453, -40.876867, -51.524101, -40.791328, -46.753734, -79.753734,
        ];
        let p = oracle_profile(&bdb);
        let dev = deviations(&bdb, Family::Guitar);
        let (slope, locals) = tilt_split(&dev, Family::Guitar, None);
        assert!((slope.unwrap() - (-5.0)).abs() <= 0.01);
        assert_locals_close(&locals, &[0.0; 6], 0.01);
        let diags = diagnose(&p, None, Family::Guitar);
        let got = keys(&diags);
        assert!((find(&diags, "dark").severity - 2.0).abs() < 0.01);
        for absent in ["boomy", "muddy", "thin"] {
            assert!(!got.contains(&absent), "{absent} must not fire");
        }
    }

    #[test]
    fn oracle_o4_dark_and_muddy_co_fire() {
        // The multi-defect lock: a −5.0 dB/oct dark tilt PLUS a +7 dB low-mid
        // bump on top — under Theil–Sen (unlike the old OLS fit) both verdicts
        // fire together. Same vector as the marketing showcase's preset 11
        // (SHOWCASE_DARK_MUDDY_BDB's doc carries the python derivation).
        // -> slope=-5.0, locals=[0,7,0,0,0,0]: dark margin=2.0, muddy margin=3.5.
        let bdb = SHOWCASE_DARK_MUDDY_BDB;
        let p = oracle_profile(&bdb);
        let dev = deviations(&bdb, Family::Guitar);
        let (slope, locals) = tilt_split(&dev, Family::Guitar, None);
        assert!((slope.unwrap() - (-5.0)).abs() <= 0.01);
        assert_locals_close(&locals, &[0.0, 7.0, 0.0, 0.0, 0.0, 0.0], 0.01);
        let got = keys(&diagnose(&p, None, Family::Guitar));
        assert!(got.contains(&"dark"));
        assert!(got.contains(&"muddy"));
    }

    #[test]
    fn oracle_o5_no_false_fire() {
        // python: apply the OLD test's delta-from-target ([-1,-1,0,2,2,3],
        // chosen for a nonzero-but-small residual) to the NEW target —
        // bdb = [T[i]+delta[i] for i in range(6)] = [-6.0, -3.0, -5.0, 15.0,
        //        15.5, -11.5]
        // -> slope≈0.671634, locals≈[0.790607,-0.128513,-0.155742,0.868075,
        //    0,0.328366] — every |local| and |slope| stays well under every
        //    new gate (lowest is muddy 3.5 / tilt 3.0), so nothing fires.
        let bdb = [-6.0, -3.0, -5.0, 15.0, 15.5, -11.5]; // a different normal combo
        let p = oracle_profile(&bdb);
        let dev = deviations(&bdb, Family::Guitar);
        let (slope, locals) = tilt_split(&dev, Family::Guitar, None);
        assert!((slope.unwrap() - 0.671634).abs() <= 0.01);
        assert!(locals[..5].iter().all(|l| l.abs() <= 0.9), "{locals:?}");
        assert!(
            keys(&diagnose(&p, None, Family::Guitar)).is_empty(),
            "no-false-harsh lock"
        );
    }

    #[test]
    fn oracle_o7_bright_only() {
        // python: L=0, s=+5.0 (tilt gate 3.0 + 2) —
        // bdb = [T[i] + s*X[i] for i in range(6)]
        //     = [27.034453, 36.876867, 41.524101, 66.791328, 73.753734,
        //        50.753734]
        // -> slope=5.0, locals≈[0;6], bright margin = 5.0-3.0 = 2.0
        let bdb = [
            27.034453, 36.876867, 41.524101, 66.791328, 73.753734, 50.753734,
        ];
        let p = oracle_profile(&bdb);
        let dev = deviations(&bdb, Family::Guitar);
        let (slope, locals) = tilt_split(&dev, Family::Guitar, None);
        assert!((slope.unwrap() - 5.0).abs() <= 0.01);
        assert_locals_close(&locals, &[0.0; 6], 0.01);
        let got = keys(&diagnose(&p, None, Family::Guitar));
        assert!(got.contains(&"bright"));
        assert!(!got.contains(&"thin"));
    }

    #[test]
    fn oracle_o8_thin_is_guitar_only() {
        // R5 consensus: a single-band bump over an otherwise-flat target has 4
        // of 5 body bands sitting on the median, so `centered_deviations` reads
        // back the SAME exact bump as the tilt-split local (median = 0 either
        // way) — the binding gate is the LARGER of the two spaces:
        // thin tilt=4.0 / centered=4.5 -> binding 4.5. python: delta=[-6.5,0,0,0,0,0]
        // (binding 4.5 + 2 margin) on Lows (index 0)
        // -> slope=0.0, locals=[-6.5,0,0,0,0,0], thin margin = min(6.5-4.0, 6.5-4.5) = 2.0
        let bdb: Vec<f64> = target_curve(Family::Guitar)
            .iter()
            .zip([-6.5, 0.0, 0.0, 0.0, 0.0, 0.0])
            .map(|(t, d)| t + d)
            .collect();
        let p = oracle_profile(&bdb);
        let dev = deviations(&bdb, Family::Guitar);
        let (slope, locals) = tilt_split(&dev, Family::Guitar, None);
        assert!((slope.unwrap() - 0.0).abs() <= 0.01);
        assert_locals_close(&locals, &[-6.5, 0.0, 0.0, 0.0, 0.0, 0.0], 0.01);
        let diags = diagnose(&p, None, Family::Guitar);
        assert!((find(&diags, "thin").severity - 2.0).abs() < 0.01);
        // Same profile judged as Bass: no thin (guitar-only gate, INFINITY in
        // both spaces) — and no buried either (buried needs a graph-derived
        // drive; nodes=None here).
        let got_bass = keys(&diagnose(&p, None, Family::Bass));
        assert!(!got_bass.contains(&"thin"));
        assert!(!got_bass.contains(&"buried"));
    }

    #[test]
    fn oracle_o9_boomy_the_old_ols_miss() {
        // Same single-band-bump identity as O8 above: centered == tilt local
        // exactly. boomy tilt=2.5 / centered=3.5 -> binding 3.5. python:
        // delta=[5.5,0,0,0,0,0] (binding 3.5 + 2 margin) on Lows (index 0)
        // -> slope=0.0, locals=[5.5,0,0,0,0,0], boomy margin = min(5.5-2.5, 5.5-3.5) = 2.0.
        // An OLS fit on 5 points would absorb part of this single-band bump
        // into the trend line and under-read it; Theil–Sen's median reads the
        // full 5.5 dB local bump back exactly.
        let bdb: Vec<f64> = target_curve(Family::Guitar)
            .iter()
            .zip([5.5, 0.0, 0.0, 0.0, 0.0, 0.0])
            .map(|(t, d)| t + d)
            .collect();
        let p = oracle_profile(&bdb);
        let dev = deviations(&bdb, Family::Guitar);
        let (slope, locals) = tilt_split(&dev, Family::Guitar, None);
        assert!((slope.unwrap() - 0.0).abs() <= 0.01);
        assert_locals_close(&locals, &[5.5, 0.0, 0.0, 0.0, 0.0, 0.0], 0.01);
        let diags = diagnose(&p, None, Family::Guitar);
        assert!((find(&diags, "boomy").severity - 2.0).abs() < 0.01);
    }

    #[test]
    fn oracle_o10_bass_vi_semantic_addressing() {
        // python (BassVi x-axis [5.406891,6.406891,7.775373,9.30482,
        // 10.758266,12.050747,13.050747], target=BASS_VI_TARGET, fit over raw
        // indices 1..=5 = semantic lows..highs): uniform=9, extra=+6 (muddy
        // gate 4.0 + 2) on raw index 2 (LowMids) -> delta=[9,9,15,9,9,9,9]
        // -> slope=0.0, locals=[0,0,6,0,0,0,0], muddy margin = 6.0-4.0 = 2.0
        let bdb: Vec<f64> = target_curve(Family::BassVi)
            .iter()
            .zip([9.0, 9.0, 15.0, 9.0, 9.0, 9.0, 9.0])
            .map(|(t, d)| t + d)
            .collect();
        let p = oracle_profile(&bdb);
        let dev = deviations(&bdb, Family::BassVi);
        let (slope, locals) = tilt_split(&dev, Family::BassVi, None);
        assert!((slope.unwrap() - 0.0).abs() <= 0.01);
        assert_locals_close(&locals, &[0.0, 0.0, 6.0, 0.0, 0.0, 0.0, 0.0], 0.01);
        assert!(keys(&diagnose(&p, None, Family::BassVi)).contains(&"muddy"));
    }

    // ── R5 HW defect-injection regression fixtures (`probe --doctor-inject`,
    //    post-chain ±12 dB EQ-10 moves on a clean guitar preset, real device
    //    measurements — pinned VERBATIM, not python-derived like the oracle
    //    vectors above). These are the CONSENSUS design's motivating cases: the
    //    tilt-split-only metric misattributed the muddy injection as a false
    //    "thin" (skirted low-mid energy smeared the Theil–Sen slope down into
    //    Lows); the centered-only metric alone would be contaminated by healthy
    //    tilt. Only the intersection gets both right. Vectors are already
    //    target-subtracted (`dev[i]`, i.e. what `deviations()` returns) — built
    //    into a `band_db` via `GUITAR_TARGET[i] + dev[i]` so they exercise the
    //    real `deviations()` too, per `notes/doctor.md`.

    /// A dev-space fixture: `band_db[i] = GUITAR_TARGET[i] + dev[i]`, run
    /// through the real `deviations()`/`diagnose()` — asserts round-trip fidelity
    /// (deviations reconstructs `dev` exactly) before checking the diagnosis.
    fn injected_profile(dev: [f64; 6]) -> SoundProfile {
        let bdb: Vec<f64> = GUITAR_TARGET.iter().zip(dev).map(|(t, d)| t + d).collect();
        oracle_profile(&bdb)
    }

    #[test]
    fn inject_muddy_250hz_fires_exactly_muddy_not_the_old_false_thin() {
        // +12 dB @250 Hz post-chain boost, HW-measured. The tilt-split-only
        // metric fired a false "thin" here (locals[lows] read ≈ −7.3 under the
        // smeared slope) while muddy (the real defect) read only +2.2 local —
        // exactly backwards. Consensus fires ONLY muddy.
        let dev = [-51.3, -43.0, -45.4, -48.6, -49.2, -50.0];
        let bdb: Vec<f64> = GUITAR_TARGET.iter().zip(dev).map(|(t, d)| t + d).collect();
        let got_dev = deviations(&bdb, Family::Guitar);
        for (g, d) in got_dev.iter().zip(dev) {
            assert!((g - d).abs() < 1e-9);
        }
        let p = injected_profile(dev);
        let diags = diagnose(&p, None, Family::Guitar);
        assert_eq!(keys(&diags), vec!["muddy"], "{diags:?}");
        assert!(
            !keys(&diags).contains(&"thin"),
            "the old false-thin verdict"
        );
        // Boundary-adjacent: a "Possible" fire (severity.ts::POSSIBLE_MAX_SEVERITY
        // == 1.0, mirrored here since this crate has no frontend dependency).
        let muddy = find(&diags, "muddy");
        assert!(
            muddy.severity > 0.0 && muddy.severity < 1.0,
            "severity {} should read as Possible",
            muddy.severity
        );
    }

    #[test]
    fn inject_mid_cut_500_1k_fires_exactly_lost() {
        // −12 dB @500 Hz+1 kHz post-chain cut, HW-measured.
        let dev = [-52.6, -52.6, -57.2, -49.9, -49.8, -50.2];
        let p = injected_profile(dev);
        assert_eq!(keys(&diagnose(&p, None, Family::Guitar)), vec!["lost"]);
    }

    #[test]
    fn inject_harsh_2khz_fires_exactly_harsh() {
        // +12 dB @2 kHz post-chain boost, HW-measured.
        let dev = [-52.4, -50.5, -46.0, -41.4, -47.2, -49.7];
        let p = injected_profile(dev);
        assert_eq!(keys(&diagnose(&p, None, Family::Guitar)), vec!["harsh"]);
    }

    #[test]
    fn inject_low_shelf_62_125hz_fires_no_band_rule_documented_limit() {
        // +12 dB @62+125 Hz (a low SHELF, not a single band) — HW-measured.
        // Documented R1 identifiability limit: a shelf spanning 2+ bands reads as
        // a tilt (dark/bright territory), not a local bump, to a single
        // slope+intercept fit. This one's tilt (−1.2 dB/oct) sits under the
        // dark gate too, so it's silent among band rules by design, not a miss.
        let dev = [-42.7, -44.7, -46.4, -48.7, -49.3, -50.0];
        let p = injected_profile(dev);
        let got = keys(&diagnose(&p, None, Family::Guitar));
        for band_rule in ["muddy", "boomy", "harsh", "lost", "thin"] {
            assert!(
                !got.contains(&band_rule),
                "{band_rule} must not fire: {got:?}"
            );
        }
    }

    #[test]
    fn inject_clean_control_is_silent() {
        // The un-injected clean preset, HW-measured — the negative control.
        let dev = [-52.4, -50.6, -46.5, -48.7, -49.3, -50.0];
        let p = injected_profile(dev);
        assert!(keys(&diagnose(&p, None, Family::Guitar)).is_empty());
    }

    #[test]
    fn oracle_fizzy_gate_inert_on_synthetic_gated_on_capture() {
        // bdb[air]−bdb[highs] = −5, past the −9 dB threshold either way.
        let mut bdb: Vec<f64> = target_curve(Family::Guitar).to_vec();
        bdb[5] = bdb[4] - 5.0;
        let mk = |flatness: f64| {
            let mut p = oracle_profile(&bdb);
            p.air_flatness = flatness;
            p
        };
        let fires = |p: &SoundProfile, kind: StimulusKind| {
            keys(&diagnose_kind(
                p,
                None,
                Family::Guitar,
                kind,
                None,
                PlaybackOffsets::NONE,
            ))
            .contains(&"fizzy")
        };
        // Synthetic: fires regardless of flatness — the gate is inert there.
        assert!(fires(&mk(0.1), StimulusKind::Synthetic));
        assert!(fires(&mk(0.9), StimulusKind::Synthetic));
        // Capture: gated on air_flatness ≥ FIZZY_MIN_FLATNESS (0.35).
        assert!(!fires(&mk(0.1), StimulusKind::Capture));
        assert!(fires(&mk(0.5), StimulusKind::Capture));
    }

    #[test]
    fn oracle_coverage_excludes_uncovered_body_band_but_keeps_finite_locals() {
        let bdb = [3.0, 11.5, 3.0, 21.0, 21.5, -6.5]; // O2
        let dev = deviations(&bdb, Family::Guitar);
        let covered = [false, true, true, true, true, true]; // lows excluded
        let (slope, locals) = tilt_split(&dev, Family::Guitar, Some(&covered));
        assert!(
            slope.is_some(),
            "4 remaining fit points still determine a slope"
        );
        assert!(locals.iter().all(|l| l.is_finite()));
        assert_eq!(locals.len(), 6);
    }

    #[test]
    fn oracle_coverage_two_body_bands_gives_no_slope_and_no_tilt_cards() {
        // O3's pure −5.0 dB/oct tilt — with full coverage this fires `dark`.
        let bdb = [
            -37.034453, -40.876867, -51.524101, -40.791328, -46.753734, -79.753734,
        ];
        let p = oracle_profile(&bdb);
        let dev = deviations(&bdb, Family::Guitar);
        // Only lows + low-mids covered in the body → 2 fit points.
        let covered = [true, true, false, false, false, true];
        let (slope, locals) = tilt_split(&dev, Family::Guitar, Some(&covered));
        assert!(
            slope.is_none(),
            "only 2 fit points must not determine a slope"
        );
        assert!(locals.iter().all(|l| l.is_finite()));

        let got = keys(&diagnose_kind(
            &p,
            None,
            Family::Guitar,
            StimulusKind::Synthetic,
            Some(&covered),
            PlaybackOffsets::NONE,
        ));
        for absent in ["dark", "bright", "thin"] {
            assert!(!got.contains(&absent), "{absent} must not fire");
        }
    }

    // ── rules fire just-above, stay silent just-below ──

    #[test]
    fn muddy_fires_above_threshold_only() {
        // Consensus: the binding gate is the LARGER of the tilt/centered pair
        // (a single-band bump reads identically in both spaces — see O8/O9's
        // derivation note), so "just above threshold" means past the max.
        let bind = GUITAR.muddy_db.max(GUITAR.muddy_centered_db);
        let hot = resid_profile(Family::Guitar, 1, bind + 1.0);
        let cold = resid_profile(Family::Guitar, 1, bind - 1.0);
        assert!(keys(&diagnose(&hot, None, Instrument::Guitar)).contains(&"muddy"));
        assert!(!keys(&diagnose(&cold, None, Instrument::Guitar)).contains(&"muddy"));
    }

    #[test]
    fn boomy_and_harsh_fire_on_their_bands() {
        // `thr` is the binding (larger) of each rule's tilt/centered gate pair.
        for (band, key, thr) in [
            (
                0usize,
                "boomy",
                GUITAR.boomy_db.max(GUITAR.boomy_centered_db),
            ),
            (3, "harsh", GUITAR.harsh_db.max(GUITAR.harsh_centered_db)),
        ] {
            let hot = resid_profile(Family::Guitar, band, thr + 1.0);
            assert!(
                keys(&diagnose(&hot, None, Instrument::Guitar)).contains(&key),
                "{key} should fire"
            );
            let cold = resid_profile(Family::Guitar, band, thr - 1.0);
            assert!(
                !keys(&diagnose(&cold, None, Instrument::Guitar)).contains(&key),
                "{key} should stay silent below threshold"
            );
        }
    }

    #[test]
    fn fizzy_fires_on_missing_air_rolloff() {
        // Fizz = Air failing to roll off below the presence band (a self-difference
        // bdb[Air]−bdb[Highs], not a tilt residual — see Thresholds::fizzy_db).
        let mut hash = resid_profile(Family::Guitar, 2, 0.0);
        hash.bands[5] = hash.bands[4]; // no rolloff at all: air == highs → diff 0 > −9
        assert!(keys(&diagnose(&hash, None, Instrument::Guitar)).contains(&"fizzy"));
        // The bare −12 dB/oct baseline rolls Air 12 dB below Highs → never fizzes.
        let cabbed = resid_profile(Family::Guitar, 2, 0.0);
        assert!(!keys(&diagnose(&cabbed, None, Instrument::Guitar)).contains(&"fizzy"));
    }

    #[test]
    fn lost_fires_on_mid_scoop() {
        // lost = −dev(mids) over threshold, i.e. the Mids residual pushed DOWN.
        let scooped = resid_profile(Family::Guitar, 2, -(GUITAR.lost_db + 1.0));
        assert!(keys(&diagnose(&scooped, None, Instrument::Guitar)).contains(&"lost"));
        let shallow = resid_profile(Family::Guitar, 2, -(GUITAR.lost_db - 1.0));
        assert!(!keys(&diagnose(&shallow, None, Instrument::Guitar)).contains(&"lost"));
    }

    #[test]
    fn washed_fires_on_wet_tail() {
        let mut p = resid_profile(Family::Guitar, 2, 0.0);
        p.tail_ratio_db = GUITAR.wash_tail_db + 5.0;
        assert!(keys(&diagnose(&p, None, Instrument::Guitar)).contains(&"washed"));
        p.tail_ratio_db = GUITAR.wash_tail_db - 5.0;
        assert!(!keys(&diagnose(&p, None, Instrument::Guitar)).contains(&"washed"));
    }

    #[test]
    fn a_clean_line_profile_is_all_clear() {
        // A sound sitting exactly on the target curve (no local bump, see
        // `oracle_o1_pure_target_no_cards` for the tilted case) reads clean.
        let clean = resid_profile(Family::Guitar, 0, 0.0);
        assert!(keys(&diagnose(&clean, None, Instrument::Guitar)).is_empty());
    }

    #[test]
    fn diagnose_is_deterministic_across_calls() {
        // No runtime cohort → the SAME input yields the SAME key set every call,
        // independent of any other sound.
        let p = resid_profile(Family::Guitar, 1, GUITAR.muddy_db + 1.0);
        let a = keys(&diagnose(&p, None, Instrument::Guitar));
        let b = keys(&diagnose(&p, None, Instrument::Guitar));
        let c = keys(&diagnose(&p, None, Instrument::Guitar));
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    // ── severity (magnitude past threshold — no backend confidence anymore;
    //    "possible" is a pure frontend threshold on severity, see severity.ts) ──

    fn find(diags: &[Diag], key: &str) -> Diag {
        diags
            .iter()
            .find(|d| d.key == key)
            .expect("rule fired")
            .clone()
    }

    #[test]
    fn far_past_threshold_severity_equals_db_past() {
        // Severity = min(margin_tilt, margin_centered); with a single-band bump
        // (centered == tilt local exactly) that's `local - binding_gate`, so
        // anchor the defect on the BINDING (larger) gate to keep "4 dB past" true.
        let bind = GUITAR.muddy_db.max(GUITAR.muddy_centered_db);
        let hot = resid_profile(Family::Guitar, 1, bind + 4.0);
        let muddy = find(&diagnose(&hot, None, Instrument::Guitar), "muddy");
        assert!(
            (muddy.severity - 4.0).abs() < 0.05,
            "severity {}",
            muddy.severity
        );
    }

    #[test]
    fn playback_offset_shifts_severity() {
        // Stage lowers the boomy threshold by 2 dB (BOTH the tilt and centered
        // gates, per `offsets.low_end_db`), so the same sound reads 2 dB MORE
        // margin (higher severity) at Stage than at Rehearsal. Anchor on the
        // binding (centered) gate, same reasoning as far_past_threshold above.
        use crate::profiles::PlaybackLevel::{Rehearsal, Stage};
        let bind = GUITAR.boomy_db.max(GUITAR.boomy_centered_db);
        let p = resid_profile(Family::Guitar, 0, bind + 1.0); // rehearsal margin = 1.0
        let at = |lvl| {
            find(
                &diagnose_kind(
                    &p,
                    None,
                    Family::Guitar,
                    StimulusKind::Synthetic,
                    None,
                    playback_offsets(lvl),
                ),
                "boomy",
            )
        };
        let reh = at(Rehearsal);
        let stg = at(Stage);
        assert!((reh.severity - 1.0).abs() < 0.05);
        assert!(
            (stg.severity - 3.0).abs() < 0.05,
            "stage margin = 1.0 + 2.0"
        );
    }

    #[test]
    fn wire_shape_carries_severity_and_not_confidence() {
        let p = resid_profile(
            Family::Guitar,
            1,
            GUITAR.muddy_db.max(GUITAR.muddy_centered_db) + 1.0,
        );
        let v = serde_json::to_value(&diagnose(&p, None, Family::Guitar)[0]).unwrap();
        let obj = v.as_object().unwrap();
        assert!(obj.contains_key("severity"));
        assert!(
            !obj.contains_key("confidence"),
            "confidence was deleted — the wire shape must not carry it"
        );
    }

    // ── tail_energy_ratio ──

    #[test]
    fn tail_ratio_separates_wet_from_dry() {
        let rate = 48_000u32;
        let body: Vec<f32> = (0..rate)
            .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        let dry_tail = vec![0.001f32; (rate / 2) as usize];
        let wet_tail = vec![0.3f32; (rate / 2) as usize];
        let dry: Vec<f32> = body.iter().chain(dry_tail.iter()).copied().collect();
        let wet: Vec<f32> = body.iter().chain(wet_tail.iter()).copied().collect();
        let d = tail_energy_ratio(&dry, rate, body.len(), 0);
        let w = tail_energy_ratio(&wet, rate, body.len(), 0);
        assert!(w > d, "wet tail must read hotter ({w} vs {d})");
        assert!(w > -6.0 && d < -40.0);
    }

    #[test]
    fn tail_ratio_guards_short_capture() {
        assert_eq!(tail_energy_ratio(&[0.1; 100], 48_000, 100, 0), -80.0);
        assert_eq!(tail_energy_ratio(&[0.1; 50], 48_000, 100, 0), -80.0);
        assert_eq!(tail_energy_ratio(&[], 48_000, 0, 0), -80.0);
    }

    // ── graph-driven prescriptions ──

    /// A G1 series chain of models; `cabbed` marks models whose node carries a
    /// cab sim (`cab_sim_id` present — CabIR amps and the standalone CabSim).
    fn chain(models: &[&str], cabbed: &[&str]) -> Vec<DoctorNode> {
        models
            .iter()
            .map(|m| DoctorNode {
                group_id: "G1".into(),
                node_id: (*m).into(),
                model: (*m).into(),
                bypassed: false,
                cab_sim_id: (cabbed.contains(m) || *m == "ACD_CabSimTMS")
                    .then(|| "SomeCab".to_string()),
                cab_sim2_enabled: None,
                params: HashMap::new(),
            })
            .collect()
    }

    #[test]
    fn fizzy_targets_existing_cab_lpf() {
        let p = chain(&["ACD_TweedDeluxe", "ACD_CabSimTMS"], &[]);
        let rx = generate_rx("fizzy", &p, Instrument::Guitar);
        assert_eq!(rx.len(), 1);
        assert_eq!(rx[0].kind, RxKind::OneClick);
        assert_eq!(rx[0].cpu_note, "no CPU change");
        match &rx[0].ops[0] {
            DoctorOp::Param {
                node_id,
                param,
                value,
                ..
            } => {
                assert_eq!(node_id, "ACD_CabSimTMS");
                assert_eq!(param, "lpf");
                assert!((value - 8000.0).abs() < f64::EPSILON);
            }
            other => panic!("expected Param, got {other:?}"),
        }
    }

    #[test]
    fn fizzy_targets_amp_embedded_cab() {
        // A CabIR amp carries its cab on the amp node (`cab_sim_id` present).
        let p = chain(
            &["ACD_HiwattDR103CanModCabIR"],
            &["ACD_HiwattDR103CanModCabIR"],
        );
        let rx = generate_rx("fizzy", &p, Instrument::Guitar);
        assert_eq!(rx.len(), 1);
        match &rx[0].ops[0] {
            DoctorOp::Param { node_id, param, .. } => {
                assert_eq!(node_id, "ACD_HiwattDR103CanModCabIR");
                assert_eq!(param, "lpf");
            }
            other => panic!("expected Param, got {other:?}"),
        }
    }

    #[test]
    fn boomy_without_cab_inserts_highlowpass() {
        let p = chain(&["ACD_TweedDeluxe"], &[]);
        let rx = generate_rx("boomy", &p, Instrument::Guitar);
        assert_eq!(rx.len(), 1);
        assert_eq!(rx[0].title, "Add a low cut at 90 Hz");
        match &rx[0].ops[0] {
            DoctorOp::InsertNode {
                fender_id, params, ..
            } => {
                assert_eq!(fender_id, "ACD_HighLowPass");
                assert_eq!(params[0].0, "hpffc");
            }
            other => panic!("expected InsertNode, got {other:?}"),
        }
    }

    #[test]
    fn boomy_and_fizzy_fall_back_to_advisory_when_cut_cannot_fit() {
        // No cab and no front group (empty graph) → cut_move yields nothing, so
        // the flagged problem must still carry an advisory rather than zero cards.
        for key in ["boomy", "fizzy"] {
            let rx = generate_rx(key, &[], Instrument::Guitar);
            assert_eq!(rx.len(), 1, "{key} should fall back to one advisory");
            assert_eq!(rx[0].kind, RxKind::Advisory);
        }
    }

    #[test]
    fn muddy_reuses_existing_eq10_else_inserts() {
        let with_eq = chain(&["ACD_TweedDeluxe", "ACD_TenBandEQStereo"], &[]);
        let rx = generate_rx("muddy", &with_eq, Instrument::Guitar);
        assert_eq!(rx[0].kind, RxKind::OneClick, "existing EQ → param write");
        match &rx[0].ops[0] {
            DoctorOp::Param { param, value, .. } => {
                assert_eq!(param, "gain250hz");
                assert!((value + 3.0).abs() < f64::EPSILON);
            }
            other => panic!("expected Param, got {other:?}"),
        }

        let without = chain(&["ACD_TweedDeluxe"], &[]);
        let rx = generate_rx("muddy", &without, Instrument::Guitar);
        assert_eq!(rx[0].kind, RxKind::Chain, "no EQ → insert with preview");
        assert!(rx[0].chain.is_some());
        match &rx[0].ops[0] {
            DoctorOp::InsertNode { fender_id, .. } => {
                assert_eq!(fender_id, "ACD_TenBandEQStereo");
            }
            other => panic!("expected InsertNode, got {other:?}"),
        }
        // The advisory alternative always rides along.
        assert!(rx.iter().any(|r| r.kind == RxKind::Advisory));
    }

    #[test]
    fn muddy_advises_using_an_existing_non_eq10_eq() {
        // A 7-band GE already in the chain can't be driven value-aware → advise
        // using it, NEVER insert a second EQ on top.
        let p = chain(
            &["ACD_TweedDeluxe", "ACD_CabSimTMS", "ACD_MustangSevenBandEq"],
            &[],
        );
        let rx = generate_rx("muddy", &p, Instrument::Guitar);
        assert!(
            rx.iter().all(|r| r.kind != RxKind::Chain),
            "must not insert a second EQ"
        );
        assert!(
            rx.iter().all(|r| !r
                .ops
                .iter()
                .any(|o| matches!(o, DoctorOp::InsertNode { .. }))),
            "no InsertNode op"
        );
        assert!(
            rx.iter()
                .any(|r| r.kind == RxKind::Advisory
                    && r.title == "Cut 3 dB around 300 Hz on your EQ"),
            "advises using the EQ already present"
        );
    }

    #[test]
    fn eq_insert_lands_after_the_cab() {
        // Amp · cab · delay, no EQ → the inserted EQ-10 anchors right AFTER the
        // cab (before the delay), not at the chain's tail.
        let p = chain(&["ACD_TweedDeluxe", "ACD_CabSimTMS", "ACD_SpaceEcho"], &[]);
        let (fid, before) =
            eq_insert(&generate_rx("muddy", &p, Instrument::Guitar)).expect("insert rx");
        assert_eq!(fid, "ACD_TenBandEQStereo");
        assert_eq!(
            before.as_deref(),
            Some("ACD_SpaceEcho"),
            "anchored after the cab, before the delay"
        );
    }

    // ─── EQ prescription regression lock ─────────────────────────────────────
    // The "muddy/harsh stacks a redundant EQ / drops it at the chain tail" bug
    // class, pinned exhaustively at the public `generate_rx` boundary so the
    // guarantee survives any refactor of eq_move / graph_facts / after_cab_anchor.

    /// The (fender_id, before_fender_id) of the single inserted block, if any.
    fn eq_insert(rx: &[Rx]) -> Option<(String, Option<String>)> {
        rx.iter()
            .find(|r| r.kind == RxKind::Chain)
            .and_then(|r| match &r.ops[0] {
                DoctorOp::InsertNode {
                    fender_id,
                    before_fender_id,
                    ..
                } => Some((fender_id.clone(), before_fender_id.clone())),
                _ => None,
            })
    }

    /// True when NO prescription in the set inserts a block.
    fn inserts_nothing(rx: &[Rx]) -> bool {
        rx.iter().all(|r| {
            !r.ops
                .iter()
                .any(|o| matches!(o, DoctorOp::InsertNode { .. }))
        })
    }

    #[test]
    fn every_non_eq10_eq_family_is_reused_not_duplicated() {
        // Each graphic/parametric EQ the Doctor can't drive value-aware → an
        // advisory to use it, NEVER a second inserted EQ. Guards the whole
        // OTHER_EQ_IDS set: add a family and forget the detection, this fails.
        for eq in OTHER_EQ_IDS {
            let p = chain(&["ACD_TweedDeluxe", "ACD_CabSimTMS", eq], &[]);
            for key in ["muddy", "harsh"] {
                let rx = generate_rx(key, &p, Instrument::Guitar);
                assert!(
                    inserts_nothing(&rx),
                    "{key} must not insert a 2nd EQ when {eq} is present"
                );
                assert!(
                    rx.iter()
                        .any(|r| r.kind == RxKind::Advisory && r.title.starts_with("Cut")),
                    "{key} must advise using the {eq} already present"
                );
            }
        }
    }

    #[test]
    fn drivable_eq10_wins_over_a_second_eq() {
        // A GE-7 AND a drivable EQ-10 present → the value-aware one-click on the
        // EQ-10, never an advisory-only nor an insert — order-independent.
        for order in [
            ["ACD_MustangSevenBandEq", "ACD_TenBandEQStereo"],
            ["ACD_TenBandEQStereo", "ACD_MustangSevenBandEq"],
        ] {
            let p = chain(&["ACD_TweedDeluxe", order[0], order[1]], &[]);
            let rx = generate_rx("muddy", &p, Instrument::Guitar);
            assert!(rx.iter().any(|r| r.kind == RxKind::OneClick), "{order:?}");
            assert!(inserts_nothing(&rx), "{order:?}");
        }
    }

    #[test]
    fn a_bypassed_eq_does_not_block_the_insert() {
        // A bypassed EQ processes no signal → treated as absent → the fix
        // inserts a fresh EQ (current behaviour; a bypassed EQ is not yet a
        // "switch it back on" carrier the way a bypassed comp is).
        let mut p = chain(
            &["ACD_TweedDeluxe", "ACD_CabSimTMS", "ACD_MustangSevenBandEq"],
            &[],
        );
        p[2].bypassed = true;
        let (fid, _) = eq_insert(&generate_rx("muddy", &p, Instrument::Guitar))
            .expect("bypassed EQ ignored → insert");
        assert_eq!(fid, "ACD_TenBandEQStereo");
    }

    #[test]
    fn freqout_is_not_treated_as_an_existing_eq() {
        // ACD_Freqout substring-contains "eq" but is a feedback pedal — with a
        // cab present the muddy fix must INSERT an EQ, never mistake it for one.
        let p = chain(&["ACD_TweedDeluxe", "ACD_CabSimTMS", "ACD_Freqout"], &[]);
        let rx = generate_rx("muddy", &p, Instrument::Guitar);
        assert!(eq_insert(&rx).is_some(), "Freqout is not an EQ → insert");
    }

    #[test]
    fn eq_insert_appends_when_the_cab_ends_its_group() {
        // Amp · cab (cab last) → append after the cab (before None, same spot).
        let p = chain(&["ACD_TweedDeluxe", "ACD_CabSimTMS"], &[]);
        let (fid, before) =
            eq_insert(&generate_rx("muddy", &p, Instrument::Guitar)).expect("insert");
        assert_eq!(fid, "ACD_TenBandEQStereo");
        assert_eq!(before, None, "cab last in group → append");
    }

    #[test]
    fn eq_insert_after_a_cabir_amp() {
        // A CabIR amp carries the cab on the amp node (cab_sim_id present); the
        // EQ still anchors right after it, before the following block.
        let p = chain(&["ACD_TweedDeluxe", "ACD_SpaceEcho"], &["ACD_TweedDeluxe"]);
        let (_, before) = eq_insert(&generate_rx("muddy", &p, Instrument::Guitar)).expect("insert");
        assert_eq!(before.as_deref(), Some("ACD_SpaceEcho"));
    }

    #[test]
    fn eq_insert_without_a_cab_falls_back_to_the_front_group() {
        // No cab anywhere → insert in the front guitar group, appended.
        let p = chain(&["ACD_TubeScreamer", "ACD_TweedDeluxe"], &[]);
        let rx = generate_rx("muddy", &p, Instrument::Guitar);
        let chain_rx = rx.iter().find(|r| r.kind == RxKind::Chain).expect("insert");
        match &chain_rx.ops[0] {
            DoctorOp::InsertNode {
                group_id,
                before_fender_id,
                ..
            } => {
                assert_eq!(group_id, "G1");
                assert_eq!(before_fender_id.as_deref(), None);
            }
            other => panic!("expected InsertNode, got {other:?}"),
        }
    }

    #[test]
    fn eq_insert_anchor_never_crosses_into_another_group() {
        // Cab last in G1, a mic group M1 follows. beforeFenderId is a same-group
        // anchor (a cross-group one is silently dropped on the wire), so the
        // anchor must be None (append to G1), never the M1 node.
        let mut p = chain(&["ACD_TweedDeluxe", "ACD_CabSimTMS"], &[]);
        p.push(DoctorNode {
            group_id: "M1".into(),
            node_id: "mic-1".into(),
            model: "ACD_Sm57".into(),
            bypassed: false,
            cab_sim_id: None,
            cab_sim2_enabled: None,
            params: HashMap::new(),
        });
        let (_, before) = eq_insert(&generate_rx("muddy", &p, Instrument::Guitar)).expect("insert");
        assert_eq!(before, None, "cross-group anchor must not be used");
    }

    #[test]
    fn eq_insert_chain_preview_marks_the_added_block_after_the_cab() {
        // The preview the UI renders must place the +added EQ tile right after
        // the cab, not at the tail — the visible half of the placement fix.
        let p = chain(&["ACD_TweedDeluxe", "ACD_CabSimTMS", "ACD_SpaceEcho"], &[]);
        let rx = generate_rx("muddy", &p, Instrument::Guitar);
        let preview = rx
            .iter()
            .find(|r| r.kind == RxKind::Chain)
            .and_then(|r| r.chain.clone())
            .expect("chain preview");
        let blocks = preview["blocks"].as_array().expect("blocks array");
        // amp(0) · cab(1) · +EQ(2, added) · delay(3)
        assert_eq!(blocks[2]["model"], "ACD_TenBandEQStereo");
        assert_eq!(blocks[2]["added"], true);
        assert!(
            blocks[3].get("added").is_none(),
            "the delay stays downstream of the EQ"
        );
    }

    #[test]
    fn muddy_and_harsh_inserts_carry_their_band_cuts() {
        // muddy → the 250 Hz band at −3; harsh → the 1k + 2k bands at −2. Fresh
        // insert, so the values are absolute.
        let p = chain(&["ACD_TweedDeluxe", "ACD_CabSimTMS"], &[]);
        let muddy = generate_rx("muddy", &p, Instrument::Guitar);
        match &muddy.iter().find(|r| r.kind == RxKind::Chain).unwrap().ops[0] {
            DoctorOp::InsertNode { params, .. } => {
                assert_eq!(params, &vec![("gain250hz".to_string(), -3.0)]);
            }
            other => panic!("expected InsertNode, got {other:?}"),
        }
        let harsh = generate_rx("harsh", &p, Instrument::Guitar);
        match &harsh.iter().find(|r| r.kind == RxKind::Chain).unwrap().ops[0] {
            DoctorOp::InsertNode { params, .. } => {
                assert!(params.contains(&("gain1khz".to_string(), -2.0)));
                assert!(params.contains(&("gain2khz".to_string(), -2.0)));
            }
            other => panic!("expected InsertNode, got {other:?}"),
        }
    }

    #[test]
    fn other_eq_ids_excludes_the_drivable_stereo() {
        // The drivable one-click EQ must never sit in the advisory-only set,
        // else a preset that has it would advise instead of driving it.
        assert!(!OTHER_EQ_IDS.contains(&EQ10_STEREO));
    }

    #[test]
    fn real_world_ge7_preset_advises_rather_than_stacking() {
        // The reported shape: a CabIR amp · comp · delay · reverb · a GE-7
        // already present. muddy must point at the GE-7, never add a 2nd EQ.
        let p = chain(
            &[
                "ACD_TweedDeluxe",
                "ACD_DynaComp",
                "ACD_SpaceEcho",
                "ACD_TMSmallHall",
                "ACD_MustangSevenBandEq",
            ],
            &["ACD_TweedDeluxe"],
        );
        let rx = generate_rx("muddy", &p, Instrument::Guitar);
        assert!(inserts_nothing(&rx), "must not stack a 2nd EQ");
        assert!(rx
            .iter()
            .any(|r| r.kind == RxKind::Advisory && r.title == "Cut 3 dB around 300 Hz on your EQ"));
    }

    #[test]
    fn washed_targets_reverb_mix_param_per_model() {
        let mut plate = chain(&["ACD_TMLargePlate"], &[]);
        plate[0].params.insert("wetdrymix".into(), 0.6);
        let rx = generate_rx("washed", &plate, Instrument::Guitar);
        match &rx[0].ops[0] {
            DoctorOp::Param { param, value, .. } => {
                assert_eq!(param, "wetdrymix", "TMLargePlate mixes via wetdrymix");
                assert!((value - 0.25).abs() < f64::EPSILON);
            }
            other => panic!("expected Param, got {other:?}"),
        }
        // Dwell-style spring: no mix param → advisory, never a wrong write.
        let spring = chain(&["ACD_Spring65"], &[]);
        let rx = generate_rx("washed", &spring, Instrument::Guitar);
        assert_eq!(rx[0].kind, RxKind::Advisory);
        assert!(rx[0].ops.is_empty());
    }

    #[test]
    fn washed_mix_set_only_when_known_and_high() {
        // Unknown current mix → advisory (a blind 0.25 write could RAISE it).
        let unknown = chain(&["ACD_TMLargeRoom"], &[]);
        let rx = generate_rx("washed", &unknown, Instrument::Guitar);
        assert_eq!(rx[0].kind, RxKind::Advisory);
        assert!(rx[0].ops.is_empty());
        // Known but already ≤ 0.25 → the wash is delay-driven, keep the advisory.
        let mut low = chain(&["ACD_TMLargeRoom"], &[]);
        low[0].params.insert("mix".into(), 0.2);
        let rx = generate_rx("washed", &low, Instrument::Guitar);
        assert_eq!(rx[0].kind, RxKind::Advisory);
        // Known high mix → the one-click cut to 25%.
        let mut high = chain(&["ACD_TMLargeRoom"], &[]);
        high[0].params.insert("mix".into(), 0.6);
        let rx = generate_rx("washed", &high, Instrument::Guitar);
        assert_eq!(rx[0].kind, RxKind::OneClick);
    }

    #[test]
    fn lost_comp_inserts_at_front_of_chain() {
        let p = chain(&["ACD_TubeScreamer", "ACD_TweedDeluxe"], &[]);
        let rx = generate_rx("lost", &p, Instrument::Guitar);
        let chain = rx
            .iter()
            .find(|r| r.kind == RxKind::Chain)
            .expect("comp rx");
        match &chain.ops[0] {
            DoctorOp::InsertNode {
                fender_id,
                before_fender_id,
                ..
            } => {
                assert_eq!(fender_id, "ACD_DynaComp");
                assert_eq!(before_fender_id.as_deref(), Some("ACD_TubeScreamer"));
            }
            other => panic!("expected InsertNode, got {other:?}"),
        }
        assert!(chain.chain.is_some());
    }

    #[test]
    fn eq_move_is_value_aware_on_existing_eq10() {
        // Known band value → relative cut on top of it, relative title kept.
        let mut p = chain(&["ACD_TweedDeluxe", "ACD_TenBandEQStereo"], &[]);
        p[1].params.insert("gain250hz".into(), 2.0);
        let rx = generate_rx("muddy", &p, Instrument::Guitar);
        assert_eq!(rx[0].title, "Cut 3 dB around 300 Hz on your EQ");
        match &rx[0].ops[0] {
            DoctorOp::Param { value, .. } => assert!((value - (-1.0)).abs() < 1e-9),
            other => panic!("expected Param, got {other:?}"),
        }
        // Clamped to the band range: −11 current − 3 → floor −12.
        p[1].params.insert("gain250hz".into(), -11.0);
        let rx = generate_rx("muddy", &p, Instrument::Guitar);
        match &rx[0].ops[0] {
            DoctorOp::Param { value, .. } => {
                assert!((value - (-EQ10_BAND_RANGE_DB)).abs() < 1e-9);
            }
            other => panic!("expected Param, got {other:?}"),
        }
        // Unknown current value → absolute write + the absolute-truth title.
        let p = chain(&["ACD_TweedDeluxe", "ACD_TenBandEQStereo"], &[]);
        let rx = generate_rx("muddy", &p, Instrument::Guitar);
        assert_eq!(rx[0].title, "Set the 250 Hz band to -3 dB");
        match &rx[0].ops[0] {
            DoctorOp::Param { value, .. } => assert!((value + 3.0).abs() < f64::EPSILON),
            other => panic!("expected Param, got {other:?}"),
        }
    }

    #[test]
    fn harsh_cuts_the_detection_band() {
        // Detection is band 3 (1–3 kHz) → the cut rides gain1khz + gain2khz.
        let p = chain(&["ACD_TweedDeluxe", "ACD_TenBandEQStereo"], &[]);
        let rx = generate_rx("harsh", &p, Instrument::Guitar);
        let eq = rx
            .iter()
            .find(|r| r.kind == RxKind::OneClick)
            .expect("eq move");
        let params: Vec<&str> = eq
            .ops
            .iter()
            .map(|op| match op {
                DoctorOp::Param { param, .. } => param.as_str(),
                other => panic!("expected Param, got {other:?}"),
            })
            .collect();
        assert_eq!(params, ["gain1khz", "gain2khz"]);
    }

    #[test]
    fn comp_moves_are_graph_aware() {
        // Active comp already in the chain → sustain advisory, never an insert.
        let mut p = chain(&["ACD_DynaComp", "ACD_TweedDeluxe"], &[]);
        let rx = generate_rx("lost", &p, Instrument::Guitar);
        assert!(rx
            .iter()
            .any(|r| r.kind == RxKind::Advisory && r.title.contains("sustain")));
        assert!(!rx.iter().any(|r| r.kind == RxKind::Chain));
        // Bypassed comp → "switch it back on" advisory.
        p[0].bypassed = true;
        let rx = generate_rx("lost", &p, Instrument::Guitar);
        assert!(rx
            .iter()
            .any(|r| r.title == "Switch your compressor back on"));
        assert!(!rx.iter().any(|r| r.kind == RxKind::Chain));
        // Active wins over bypassed (order-independent).
        let mut p = chain(&["ACD_CS3", "ACD_Sustain", "ACD_TweedDeluxe"], &[]);
        p[0].bypassed = true;
        let rx = generate_rx("lost", &p, Instrument::Guitar);
        assert!(rx
            .iter()
            .any(|r| r.title.contains("sustain on the compressor")));
        // Buried takes the same comp-aware path.
        let p = chain(&["ACD_DynaComp", "ACD_ModernBassOverdrive"], &[]);
        let rx = generate_rx("buried", &p, Instrument::Bass);
        assert!(rx
            .iter()
            .any(|r| r.kind == RxKind::Advisory && r.title.contains("sustain")));
        assert!(!rx.iter().any(|r| r.kind == RxKind::Chain));
    }

    #[test]
    fn spiky_fires_on_clean_spread_only() {
        let clean = chain(&["ACD_TweedDeluxe", "ACD_CabSimTMS"], &[]);
        let driven = chain(
            &["ACD_TubeScreamer", "ACD_TweedDeluxe", "ACD_CabSimTMS"],
            &[],
        );

        let mut hot = resid_profile(Family::Guitar, 0, 0.0);
        hot.spread_lu = GUITAR.spiky_spread_lu + 1.0;
        assert!(keys(&diagnose(&hot, Some(&clean), Instrument::Guitar)).contains(&"spiky"));
        // A drive block in the chain means the amp is already compressing it —
        // spiky is a clean-chain-only finding.
        assert!(!keys(&diagnose(&hot, Some(&driven), Instrument::Guitar)).contains(&"spiky"));

        let mut cold = resid_profile(Family::Guitar, 0, 0.0);
        cold.spread_lu = GUITAR.spiky_spread_lu - 1.0;
        assert!(!keys(&diagnose(&cold, Some(&clean), Instrument::Guitar)).contains(&"spiky"));

        // Without a graph we can't assert "clean" — never fires.
        assert!(!keys(&diagnose(&hot, None, Instrument::Guitar)).contains(&"spiky"));
    }

    #[test]
    fn spiky_comp_inserts_right_after_cab() {
        let p = chain(
            &["ACD_TweedDeluxe", "ACD_TMSmallHall"],
            &["ACD_TweedDeluxe"],
        );
        let rx = generate_rx("spiky", &p, Instrument::Guitar);
        let chain_rx: Vec<&Rx> = rx.iter().filter(|r| r.kind == RxKind::Chain).collect();
        assert_eq!(chain_rx.len(), 1);
        assert_eq!(chain_rx[0].ops.len(), 1);
        match &chain_rx[0].ops[0] {
            DoctorOp::InsertNode {
                group_id,
                before_fender_id,
                fender_id,
                ..
            } => {
                assert_eq!(group_id, "G1");
                assert_eq!(before_fender_id.as_deref(), Some("ACD_TMSmallHall"));
                assert_eq!(fender_id, "ACD_CompressorSimpleSoftKnee");
            }
            other => panic!("expected InsertNode, got {other:?}"),
        }
        let blocks = &chain_rx[0].chain.as_ref().expect("chain preview")["blocks"];
        assert_eq!(
            blocks[1],
            serde_json::json!({"model": "ACD_CompressorSimpleSoftKnee", "added": true})
        );
    }

    #[test]
    fn spiky_comp_appends_when_cab_ends_group() {
        let assert_appends_in_g1 = |nodes: &[DoctorNode]| {
            let rx = generate_rx("spiky", nodes, Instrument::Guitar);
            let chain_rx = rx
                .iter()
                .find(|r| r.kind == RxKind::Chain)
                .expect("chain rx");
            match &chain_rx.ops[0] {
                DoctorOp::InsertNode {
                    group_id,
                    before_fender_id,
                    ..
                } => {
                    assert_eq!(group_id, "G1");
                    assert!(before_fender_id.is_none());
                }
                other => panic!("expected InsertNode, got {other:?}"),
            }
        };

        // Cab last in its group → append (before_fender_id: None).
        let mut p = chain(&["ACD_TweedDeluxe", "ACD_CabSimTMS"], &[]);
        assert_appends_in_g1(&p);

        // A node in a DIFFERENT group right after the cab must never anchor
        // cross-group (the firmware silently drops a cross-group anchor).
        p.push(DoctorNode {
            group_id: "M1".into(),
            node_id: "ACD_Beta57".into(),
            model: "ACD_Beta57".into(),
            bypassed: false,
            cab_sim_id: None,
            cab_sim2_enabled: None,
            params: HashMap::new(),
        });
        assert_appends_in_g1(&p);
    }

    #[test]
    fn spiky_comp_anchors_on_node_id_not_model() {
        // Two same-model reverb nodes after the cab: only the node_id
        // distinguishes which one the anchor must reference.
        let p = vec![
            DoctorNode {
                group_id: "G1".into(),
                node_id: "amp1".into(),
                model: "ACD_TweedDeluxe".into(),
                bypassed: false,
                cab_sim_id: Some("SomeCab".to_string()),
                cab_sim2_enabled: None,
                params: HashMap::new(),
            },
            DoctorNode {
                group_id: "G1".into(),
                node_id: "reverb-a".into(),
                model: "ACD_TMSmallHall".into(),
                bypassed: false,
                cab_sim_id: None,
                cab_sim2_enabled: None,
                params: HashMap::new(),
            },
        ];
        let rx = generate_rx("spiky", &p, Instrument::Guitar);
        let chain_rx = rx
            .iter()
            .find(|r| r.kind == RxKind::Chain)
            .expect("chain rx");
        match &chain_rx.ops[0] {
            DoctorOp::InsertNode {
                before_fender_id, ..
            } => {
                assert_eq!(before_fender_id.as_deref(), Some("reverb-a"));
            }
            other => panic!("expected InsertNode, got {other:?}"),
        }
    }

    #[test]
    fn spiky_comp_skips_insert_when_time_effect_precedes_cab() {
        // [delay, cab]: compressing after the cab would still sit after the
        // delay's wet tail — bail to advisory-only rather than pump it.
        let p = chain(&["ACD_SpaceEcho", "ACD_CabSimTMS"], &[]);
        let rx = generate_rx("spiky", &p, Instrument::Guitar);
        assert!(!rx.iter().any(|r| r.kind == RxKind::Chain));
        assert!(rx.iter().all(|r| r.kind == RxKind::Advisory));

        // A BYPASSED time effect before the cab doesn't process signal, so the
        // insert still fires normally.
        let mut p = chain(&["ACD_SpaceEcho", "ACD_CabSimTMS"], &[]);
        p[0].bypassed = true;
        let rx = generate_rx("spiky", &p, Instrument::Guitar);
        assert!(rx.iter().any(|r| r.kind == RxKind::Chain));
    }

    #[test]
    fn spiky_is_comp_aware_and_advisory_without_cab() {
        // Active comp in the chain → advisory about compression, no Chain rx.
        // Every case also carries the leading "Tame the swings at the source"
        // advisory, so assert Chain-rx absence rather than an exact count.
        let p = chain(&["ACD_DynaComp", "ACD_TweedDeluxe"], &[]);
        let rx = generate_rx("spiky", &p, Instrument::Guitar);
        assert!(rx
            .iter()
            .any(|r| r.kind == RxKind::Advisory && r.title.contains("compression")));
        assert!(!rx.iter().any(|r| r.kind == RxKind::Chain));

        // Bypassed comp → "switch it back on" advisory.
        let mut p = chain(&["ACD_DynaComp", "ACD_TweedDeluxe"], &[]);
        p[0].bypassed = true;
        let rx = generate_rx("spiky", &p, Instrument::Guitar);
        assert!(rx
            .iter()
            .any(|r| r.title == "Switch your compressor back on"));
        assert!(!rx.iter().any(|r| r.kind == RxKind::Chain));

        // No comp, no cab → advisory-only, empty ops.
        let p = chain(&["ACD_TweedDeluxe"], &[]);
        let rx = generate_rx("spiky", &p, Instrument::Guitar);
        assert!(!rx.is_empty());
        assert!(rx.iter().all(|r| r.kind == RxKind::Advisory));
        assert!(rx.iter().all(|r| r.ops.is_empty()));
    }

    #[test]
    fn freqout_is_not_an_eq() {
        // The substring trap: ACD_Freqout contains "eq" but is a feedback
        // pedal — exact-id matching must never treat it as an EQ carrier.
        let p = chain(&["ACD_Freqout"], &[]);
        let rx = generate_rx("muddy", &p, Instrument::Guitar);
        for r in &rx {
            for op in &r.ops {
                if let DoctorOp::Param { node_id, .. } = op {
                    assert_ne!(node_id, "ACD_Freqout");
                }
            }
        }
    }

    #[test]
    fn buried_is_bass_only_and_needs_a_drive() {
        // buried = the Lows local scooped past buried_lows_db (−locals[lows]),
        // consensus-gated — anchor on the binding (centered) gate.
        let bind = BASS.buried_lows_db.max(BASS.buried_centered_db);
        let scooped_lows = resid_profile(Family::Bass, 0, -(bind + 1.0));
        let driven = chain(&["ACD_ModernBassOverdrive", "ACD_TweedDeluxe"], &[]);
        let got = diagnose(&scooped_lows, Some(&driven), Instrument::Bass);
        assert!(keys(&got).contains(&"buried"));
        // Same profile on guitar, or without a drive → silent.
        let got = diagnose(&scooped_lows, Some(&driven), Instrument::Guitar);
        assert!(!keys(&got).contains(&"buried"));
        let clean = chain(&["ACD_TweedDeluxe"], &[]);
        let got = diagnose(&scooped_lows, Some(&clean), Instrument::Bass);
        assert!(!keys(&got).contains(&"buried"));
    }

    // ── scene consistency ──

    #[test]
    fn scene_consistency_flags_big_jump_only() {
        let scenes = vec![
            (
                "Crunch".to_string(),
                Some("FS1".to_string()),
                -14.0,
                Some(1u32),
            ),
            (
                "Lead".to_string(),
                Some("FS2".to_string()),
                -18.5,
                Some(2u32),
            ),
        ];
        let sc = scene_consistency("Rhythm", -20.0, &scenes, Instrument::Guitar)
            .expect("6 dB jump flags");
        assert_eq!(sc.worst_name, "Crunch");
        assert!((sc.worst_delta_db - 6.0).abs() < 1e-9);
        assert_eq!(sc.rows.len(), 3);
        assert!(sc.rows[0].is_ref);
        assert!(sc.rx[0].title.contains("Crunch"));
        // A louder scene is ADVISED (Doctor has no in-app scene trim), never a
        // one-click op — the guidance points at the Level tab.
        assert_eq!(sc.rx[0].kind, RxKind::Advisory);
        assert!(sc.rx[0].ops.is_empty());
        assert!(sc.rx[0].detail.contains("Level tab"));
        // All within 3 dB → no flag.
        let tame = vec![("Lead".to_string(), None, -18.0, Some(1u32))];
        assert!(scene_consistency("Rhythm", -20.0, &tame, Instrument::Guitar).is_none());
    }

    #[test]
    fn scene_quieter_worst_gets_advisory_not_trim() {
        // A much QUIETER worst scene: trimming down is wrong — point to the
        // Level tab instead of prescribing a SceneTrim.
        let scenes = vec![(
            "Ballad".to_string(),
            Some("FS1".to_string()),
            -27.0,
            Some(1u32),
        )];
        let sc = scene_consistency("Rhythm", -20.0, &scenes, Instrument::Guitar)
            .expect("7 dB quieter still flags");
        assert!((sc.worst_delta_db + 7.0).abs() < 1e-9);
        assert_eq!(sc.rx[0].kind, RxKind::Advisory);
        assert!(sc.rx[0].ops.is_empty());
        assert!(sc.rx[0].detail.contains("Level tab"));
    }

    #[test]
    fn scene_zero_worst_gets_verify_by_ear_advisory() {
        // The open loadScene(0) anomaly: a worst scene at wire index 0 gets a
        // verify-by-ear advisory (distinct copy from the plain louder advisory).
        let scenes = vec![(
            "Lead".to_string(),
            Some("FS1".to_string()),
            -14.0,
            Some(0u32),
        )];
        let sc = scene_consistency("Rhythm", -20.0, &scenes, Instrument::Guitar)
            .expect("6 dB jump flags");
        assert_eq!(sc.rx[0].kind, RxKind::Advisory);
        assert!(sc.rx[0].ops.is_empty());
        assert!(sc.rx[0].title.contains("by ear"));
        // The same jump on wire index 1 is the plain louder advisory — advisory
        // too (no in-app scene trim), but NOT the by-ear copy.
        let scenes = vec![(
            "Lead".to_string(),
            Some("FS1".to_string()),
            -14.0,
            Some(1u32),
        )];
        let sc = scene_consistency("Rhythm", -20.0, &scenes, Instrument::Guitar).unwrap();
        assert_eq!(sc.rx[0].kind, RxKind::Advisory);
        assert!(sc.rx[0].ops.is_empty());
        assert!(!sc.rx[0].title.contains("by ear"));
        assert!(sc.rx[0].title.contains("louder"));
    }

    #[test]
    fn scene_consistency_guards_non_finite() {
        let dead = vec![("Dead".to_string(), None, f64::NEG_INFINITY, Some(1u32))];
        assert!(scene_consistency("Rhythm", -20.0, &dead, Instrument::Guitar).is_none());
        let ok = vec![("Lead".to_string(), None, -14.0, Some(1u32))];
        assert!(scene_consistency("Rhythm", f64::NEG_INFINITY, &ok, Instrument::Guitar).is_none());
    }

    #[test]
    fn footswitch_worst_gets_advisory_not_trim() {
        // A footswitch sound (no wire scene index) as the worst jump: there is
        // nothing to SceneTrim — advise leveling / backing off the knob.
        let others = vec![
            ("Boost".to_string(), Some("FS3".to_string()), -13.0, None),
            (
                "Lead".to_string(),
                Some("FS1".to_string()),
                -18.0,
                Some(1u32),
            ),
        ];
        let sc = scene_consistency("Rhythm", -20.0, &others, Instrument::Guitar)
            .expect("7 dB FS jump flags");
        assert_eq!(sc.worst_name, "Boost");
        assert_eq!(sc.rx.len(), 1);
        assert_eq!(sc.rx[0].kind, RxKind::Advisory);
        assert!(sc.rx[0].ops.is_empty());
        assert!(sc.rx[0].title.contains("louder than the base sound"));
        assert!(sc.rx[0].detail.contains("Level tab"));
    }

    #[test]
    fn scene_worst_advises_with_footswitch_rows_present() {
        // An FS row present but a louder SCENE is the worst: the louder-scene
        // advisory fires and the FS row still appears in the delta table.
        let others = vec![
            ("Boost".to_string(), Some("FS3".to_string()), -19.0, None),
            (
                "Lead".to_string(),
                Some("FS1".to_string()),
                -14.0,
                Some(1u32),
            ),
        ];
        let sc = scene_consistency("Rhythm", -20.0, &others, Instrument::Guitar)
            .expect("6 dB scene jump flags");
        assert_eq!(sc.worst_name, "Lead");
        assert_eq!(sc.rx[0].kind, RxKind::Advisory);
        assert!(sc.rx[0].ops.is_empty());
        assert!(sc.rx[0].detail.contains("Level tab"));
        assert!(sc.rows.iter().any(|r| r.name == "Boost"));
    }

    // ── silent capture (#6) ──

    #[test]
    fn silent_capture_errors_instead_of_minus_inf() {
        let silence = vec![0.0f32; 96_000];
        let psd = body_psd(&silence, 48_000, 0);
        let err =
            SoundProfile::from_capture_with_psd(&silence, 48_000, 48_000, 0, Family::Guitar, &psd)
                .expect_err("silence must not produce a profile");
        assert!(err.contains("silent"), "{err}");
    }

    // ── instrument families + Bass VI band layout ──

    #[test]
    fn family_from_topology_maps_bass_vi_and_defaults_to_guitar() {
        assert_eq!(Family::from_topology("guitar"), Family::Guitar);
        assert_eq!(Family::from_topology("bass"), Family::Bass);
        assert_eq!(Family::from_topology("bass-vi"), Family::BassVi);
        assert_eq!(Family::from_topology("Bass-VI"), Family::BassVi); // case-insensitive
                                                                      // Anything unrecognized falls back to the neutral 6-band guitar layout.
        assert_eq!(Family::from_topology("sitar"), Family::Guitar);
        assert_eq!(Family::from_topology(""), Family::Guitar);
    }

    #[test]
    fn band_layouts_and_labels_stay_in_lockstep() {
        // Every family: one display label per Hz band.
        for fam in [Family::Guitar, Family::Bass, Family::BassVi] {
            assert_eq!(
                fam.bands().len(),
                fam.labels().len(),
                "{fam:?} labels must match its band count"
            );
        }
        // Guitar/Bass: the historical 6 bands, indices 0..=5.
        for fam in [Family::Guitar, Family::Bass] {
            assert_eq!(fam.bands().len(), 6);
            assert_eq!(fam.semantic_bands(), (0, 1, 2, 3, 4, 5));
        }
        // Bass VI: a 7th "Sub" band at index 0; the six semantic bands shift +1.
        assert_eq!(Family::BassVi.bands().len(), 7);
        assert_eq!(Family::BassVi.bands()[0], (30.0, 60.0));
        assert_eq!(Family::BassVi.labels()[0], "Sub");
        assert_eq!(Family::BassVi.semantic_bands(), (1, 2, 3, 4, 5, 6));
        // The shared six bands are byte-identical to the guitar/bass layout.
        assert_eq!(&Family::BassVi.bands()[1..], Family::Guitar.bands());
    }

    #[test]
    fn bass_vi_hot_sub_keys_no_diagnosis() {
        // A Bass VI sound with a hot Sub band (index 0) and an otherwise healthy
        // spectrum: nothing rules on Sub yet, so no diagnosis may highlight it.
        let mut bands = vec![1.0; 7];
        bands[6] = 10f64.powf(-20.0 / 10.0); // air rolled off (no fizz)
        bands[0] = 10f64.powf(20.0 / 10.0); // hot sub
        let p = SoundProfile {
            bands,
            integrated_lufs: -20.0,
            spread_lu: 0.0,
            tail_ratio_db: -40.0,
            air_flatness: 0.5,
            peaks: Vec::new(),
        };
        let diags = diagnose(&p, None, Family::BassVi);
        assert!(
            diags.iter().all(|d| !d.bands.contains(&0)),
            "no diagnosis may key on the Sub band (index 0): {:?}",
            keys(&diags)
        );
    }

    // ── stimulus-kind thresholds + band-confidence gating ──

    fn assert_same_thresholds(a: &Thresholds, b: &Thresholds) {
        assert_eq!(a.muddy_db, b.muddy_db);
        assert_eq!(a.boomy_db, b.boomy_db);
        assert_eq!(a.harsh_db, b.harsh_db);
        assert_eq!(a.fizzy_db, b.fizzy_db);
        assert_eq!(a.lost_db, b.lost_db);
        assert_eq!(a.wash_tail_db, b.wash_tail_db);
        assert_eq!(a.buried_lows_db, b.buried_lows_db);
        assert_eq!(a.spiky_spread_lu, b.spiky_spread_lu);
        assert_eq!(a.scene_delta_db, b.scene_delta_db);
        assert_eq!(a.tilt_db_per_oct, b.tilt_db_per_oct);
        assert_eq!(a.thin_db, b.thin_db);
        assert_eq!(a.muddy_centered_db, b.muddy_centered_db);
        assert_eq!(a.boomy_centered_db, b.boomy_centered_db);
        assert_eq!(a.harsh_centered_db, b.harsh_centered_db);
        assert_eq!(a.lost_centered_db, b.lost_centered_db);
        assert_eq!(a.thin_centered_db, b.thin_centered_db);
        assert_eq!(a.buried_centered_db, b.buried_centered_db);
    }

    #[test]
    fn thresholds_for_matches_pinned_consts_regardless_of_kind() {
        // ONE table per family now — `kind` no longer selects a different table
        // (the `*_CAPTURE` consts are gone), so both kinds must read identically
        // to the pinned HW-calibrated consts.
        for (fam, pinned) in [
            (Family::Guitar, &GUITAR),
            (Family::Bass, &BASS),
            (Family::BassVi, &BASS_VI),
        ] {
            assert_same_thresholds(fam.thresholds_for(StimulusKind::Synthetic), pinned);
            assert_same_thresholds(fam.thresholds_for(StimulusKind::Capture), pinned);
            assert_same_thresholds(fam.thresholds(), pinned); // thresholds() alias
        }
    }

    #[test]
    fn band_gate_suppresses_rule_when_primary_band_uncovered() {
        // A hot Lows band → boomy fires when every band is covered...
        let hot = resid_profile(
            Family::Guitar,
            0,
            GUITAR.boomy_db.max(GUITAR.boomy_centered_db) + 1.0,
        );
        let all_covered = vec![true; 6];
        assert!(keys(&diagnose_kind(
            &hot,
            None,
            Family::Guitar,
            StimulusKind::Synthetic,
            Some(&all_covered),
            PlaybackOffsets::NONE,
        ))
        .contains(&"boomy"));
        // ...but is SKIPPED when the Lows band was never excited by the stimulus.
        let mut lows_uncovered = vec![true; 6];
        lows_uncovered[0] = false;
        assert!(!keys(&diagnose_kind(
            &hot,
            None,
            Family::Guitar,
            StimulusKind::Synthetic,
            Some(&lows_uncovered),
            PlaybackOffsets::NONE,
        ))
        .contains(&"boomy"));
    }

    #[test]
    fn coverage_flat_bands_are_all_covered() {
        assert_eq!(coverage(&[1.0, 1.0, 1.0, 1.0, 1.0, 1.0]), vec![true; 6]);
    }

    #[test]
    fn coverage_sparse_take_covers_only_played_bands() {
        // 2 loud bands, 4 essentially dead (well below the 30 dB floor).
        let bands = [1.0, 1e-9, 1e-9, 1e-9, 1e-9, 1.0];
        assert_eq!(
            coverage(&bands),
            vec![true, false, false, false, false, true]
        );
    }

    #[test]
    fn coverage_thirty_db_boundary() {
        // Loudest = 0 dB (power 1.0). Exactly −30 dB (power 1e-3) still counts;
        // just past it (−30.5 dB) does not.
        let at_boundary = 10f64.powf(-30.0 / 10.0);
        let past_boundary = 10f64.powf(-30.5 / 10.0);
        assert_eq!(coverage(&[1.0, at_boundary]), vec![true, true]);
        assert_eq!(coverage(&[1.0, past_boundary]), vec![true, false]);
    }

    // ── output_coverage / Welch band_coverage / has_time_effect (R2) ──

    /// Deterministic bipolar LCG noise (Numerical Recipes constants) — a local
    /// copy of `psd::tests::Lcg`'s generator, which is private to its own
    /// test module.
    fn lcg_noise(n: usize, amp: f32, seed: u64) -> Vec<f32> {
        let mut state = seed;
        (0..n)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                let unit = (state >> 11) as f64 / (1u64 << 53) as f64;
                (unit as f32 * 2.0 - 1.0) * amp
            })
            .collect()
    }

    fn test_sine(freq: f32, amp: f32, n: usize, rate: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / rate).sin())
            .collect()
    }

    #[test]
    fn output_coverage_onset_zero_is_all_true() {
        let samples = vec![0.0f32; 200];
        let body = body_psd(&samples, 48_000, 0);
        assert_eq!(
            output_coverage_with_body(&samples, 48_000, 0, Family::Guitar, &body),
            vec![true; 6]
        );
    }

    #[test]
    fn output_coverage_onset_at_or_past_end_is_all_true() {
        let samples = vec![0.0f32; 200];
        let body = body_psd(&samples, 48_000, 200);
        assert_eq!(
            output_coverage_with_body(&samples, 48_000, 200, Family::Guitar, &body),
            vec![true; 6]
        );
        let body = body_psd(&samples, 48_000, 5_000);
        assert_eq!(
            output_coverage_with_body(&samples, 48_000, 5_000, Family::Guitar, &body),
            vec![true; 6]
        );
    }

    #[test]
    fn output_coverage_body_sine_covers_only_its_own_band() {
        const RATE: u32 = 48_000;
        let onset = 4096usize;
        // Near-silent pre-onset floor (−54 dBFS noise, well above the Hann
        // sidelobe leakage floor so the comparison isn't a coin flip) + a
        // 2 kHz sine body — the band containing 2 kHz (High-mids, 1–3 kHz)
        // must clear the floor; a distant band never excited by the sine
        // (Lows, 60–120 Hz) must not.
        let mut samples = lcg_noise(onset, 1e-3, 7);
        samples.extend(test_sine(2_000.0, 0.5, 48_000, RATE as f32));
        let body = body_psd(&samples, RATE, onset);
        let cov = output_coverage_with_body(&samples, RATE, onset, Family::Guitar, &body);
        assert_eq!(cov.len(), 6);
        assert!(cov[3], "High-mids (1–3 kHz) should be covered: {cov:?}");
        assert!(!cov[0], "Lows (60–120 Hz) should NOT be covered: {cov:?}");
    }

    #[test]
    fn band_coverage_welch_white_noise_covers_all_bands() {
        let samples = lcg_noise(96_000, 0.3, 11);
        assert_eq!(band_coverage(&samples, Family::Guitar), vec![true; 6]);
    }

    #[test]
    fn band_coverage_welch_single_sine_covers_only_its_band() {
        // Mirrors the old Goertzel-based behavior (`coverage_sparse_take_covers_
        // only_played_bands`), now on the Welch estimator.
        let samples = test_sine(2_000.0, 1.0, 96_000, 48_000.0);
        let cov = band_coverage(&samples, Family::Guitar);
        assert!(cov[3], "High-mids should be covered: {cov:?}");
        assert!(!cov[0], "Lows should not be covered: {cov:?}");
        assert!(!cov[5], "Air should not be covered: {cov:?}");
    }

    // ── shared post-onset body PSD (Task 1: one PSD per capture) ──

    #[test]
    fn from_capture_onset_zero_matches_legacy_whole_buffer_psd() {
        // onset=0 (every synthetic-stimulus call site) must stay byte-identical
        // to a direct whole-buffer welch_psd computation — the legacy space.
        const RATE: u32 = 48_000;
        let samples = test_sine(1_000.0, 0.4, RATE as usize, RATE as f32);
        let psd = body_psd(&samples, RATE, 0);
        let profile = SoundProfile::from_capture_with_psd(
            &samples,
            RATE,
            samples.len(),
            0,
            Family::Guitar,
            &psd,
        )
        .expect("finite loudness");
        let whole = crate::psd::welch_psd(&samples, RATE as f32);
        assert_eq!(profile.bands, whole.band_powers(Family::Guitar.bands()));
        assert_eq!(profile.air_flatness, whole.flatness(6_000.0, 12_000.0));
    }

    #[test]
    fn from_capture_excludes_preamble_from_body_psd() {
        // 1 s silence preamble + a 1 s sine body, onset at the boundary:
        // from_capture's bands must equal from_capture_with_psd fed the
        // explicitly-sliced body PSD, and must differ from the whole-buffer
        // (preamble-diluted) space.
        const RATE: u32 = 48_000;
        let onset = RATE as usize;
        let mut samples = vec![0.0f32; onset];
        samples.extend(test_sine(1_000.0, 0.4, RATE as usize, RATE as f32));
        let stim_samples = samples.len() - onset;
        let profile = SoundProfile::from_capture_with_psd(
            &samples,
            RATE,
            stim_samples,
            onset,
            Family::Guitar,
            &body_psd(&samples, RATE, onset),
        )
        .expect("finite loudness");

        let explicit_body = crate::psd::welch_psd(&samples[onset..], RATE as f32);
        let via_with_psd = SoundProfile::from_capture_with_psd(
            &samples,
            RATE,
            stim_samples,
            onset,
            Family::Guitar,
            &explicit_body,
        )
        .expect("finite loudness");
        assert_eq!(profile.bands, via_with_psd.bands);
        assert_eq!(profile.air_flatness, via_with_psd.air_flatness);

        let whole = crate::psd::welch_psd(&samples, RATE as f32);
        assert_ne!(
            profile.bands,
            whole.band_powers(Family::Guitar.bands()),
            "preamble-diluted whole-buffer bands must differ from the post-onset body bands"
        );
    }

    #[test]
    fn has_time_effect_curated_reverb_id() {
        assert!(has_time_effect(&chain(&["ACD_TMLargeHall"], &[])));
    }

    #[test]
    fn has_time_effect_curated_delay_id() {
        assert!(has_time_effect(&chain(&["ACD_MemoryMan"], &[])));
    }

    #[test]
    fn has_time_effect_convrvb_substring_catch_all() {
        // A model the curated tables don't list (e.g. an amp with baked-in
        // convolution reverb) still trips the conservative substring match.
        assert!(has_time_effect(&chain(&["ACD_SomethingConvRvb"], &[])));
    }

    #[test]
    fn has_time_effect_clean_chain_is_false() {
        let nodes = chain(
            &["ACD_HiwattDR103CanMod", "ACD_CabSimTMS", "ACD_TubeScreamer"],
            &[],
        );
        assert!(!has_time_effect(&nodes));
    }

    #[test]
    fn has_time_effect_reverb_named_amp_is_a_documented_false_positive() {
        // Not in REVERB_MIX/DELAY_IDS (it's an amp id, not an effect block) —
        // the "Reverb" substring alone trips it. Deliberate: see `has_time_
        // effect`'s doc comment on the asymmetry.
        assert!(has_time_effect(&chain(
            &["ACD_DeluxeReverb65BlondeNoFx"],
            &[]
        )));
    }

    #[test]
    fn doctor_tail_ms_owns_the_whole_policy() {
        // Unknown graph (empty nodes) → conservative full tail.
        assert_eq!(doctor_tail_ms(&[]), crate::leveller::DOCTOR_TAIL_MS);
        // Wet chain → full tail.
        assert_eq!(
            doctor_tail_ms(&chain(&["ACD_TMLargeHall"], &[])),
            crate::leveller::DOCTOR_TAIL_MS
        );
        // Known dry chain → short settle-only tail.
        assert_eq!(
            doctor_tail_ms(&chain(&["ACD_TubeScreamer"], &[])),
            crate::leveller::DOCTOR_TAIL_DRY_MS
        );
    }

    #[test]
    fn synthetic_full_coverage_gate_is_a_noop() {
        // A synthetic stimulus excites every band by construction, so its coverage
        // is all-true and gated diagnosis == ungated (the pinned behavior).
        let cov = coverage(&[1.0f64; 6]);
        assert_eq!(
            cov,
            vec![true; 6],
            "flat synthetic spectrum covers all bands"
        );
        let hot = resid_profile(Family::Guitar, 0, GUITAR.boomy_db + 1.0);
        let gated = keys(&diagnose_kind(
            &hot,
            None,
            Family::Guitar,
            StimulusKind::Synthetic,
            Some(&cov),
            PlaybackOffsets::NONE,
        ));
        let ungated = keys(&diagnose(&hot, None, Family::Guitar));
        assert_eq!(gated, ungated);
    }

    // ── value-aware cab hpf/lpf cuts ──

    #[test]
    fn boomy_cab_hpf_is_value_aware() {
        // Unknown current hpf → blind write, honest "Set…" title (may raise OR lower).
        let p = chain(&["ACD_TweedDeluxe", "ACD_CabSimTMS"], &[]);
        let rx = generate_rx("boomy", &p, Instrument::Guitar);
        assert_eq!(rx[0].kind, RxKind::OneClick);
        assert_eq!(rx[0].title, "Set the cab's low cut to 90 Hz");
        // Known + on the WRONG side (below 90) → the directional one-click at 90.
        let mut low = chain(&["ACD_TweedDeluxe", "ACD_CabSimTMS"], &[]);
        low[1].params.insert("hpf".into(), 40.0);
        let rx = generate_rx("boomy", &low, Instrument::Guitar);
        assert_eq!(rx[0].kind, RxKind::OneClick);
        assert_eq!(rx[0].title, "Raise the cab's low cut to 90 Hz");
        match &rx[0].ops[0] {
            DoctorOp::Param { param, value, .. } => {
                assert_eq!(param, "hpf");
                assert!((value - 90.0).abs() < f64::EPSILON);
            }
            other => panic!("expected Param, got {other:?}"),
        }
        // Known + already AT/PAST 90 → the one-click would LOWER it; skip → advisory.
        let mut high = chain(&["ACD_TweedDeluxe", "ACD_CabSimTMS"], &[]);
        high[1].params.insert("hpf".into(), 120.0);
        let rx = generate_rx("boomy", &high, Instrument::Guitar);
        assert_eq!(rx.len(), 1);
        assert_eq!(rx[0].kind, RxKind::Advisory);
    }

    #[test]
    fn fizzy_cab_lpf_is_value_aware() {
        // Unknown current lpf → blind write, honest "Set…" title.
        let p = chain(&["ACD_TweedDeluxe", "ACD_CabSimTMS"], &[]);
        let rx = generate_rx("fizzy", &p, Instrument::Guitar);
        assert_eq!(rx[0].kind, RxKind::OneClick);
        assert_eq!(rx[0].title, "Set the cab's high cut to 8 kHz");
        // Known + on the WRONG side (above 8 kHz) → the directional one-click at 8000.
        let mut hi = chain(&["ACD_TweedDeluxe", "ACD_CabSimTMS"], &[]);
        hi[1].params.insert("lpf".into(), 12000.0);
        let rx = generate_rx("fizzy", &hi, Instrument::Guitar);
        assert_eq!(rx[0].title, "Lower the cab's high cut to tame the fizz");
        match &rx[0].ops[0] {
            DoctorOp::Param { param, value, .. } => {
                assert_eq!(param, "lpf");
                assert!((value - 8000.0).abs() < f64::EPSILON);
            }
            other => panic!("expected Param, got {other:?}"),
        }
        // Known + already AT/PAST 8 kHz (below) → the one-click would RAISE it; skip.
        let mut lo = chain(&["ACD_TweedDeluxe", "ACD_CabSimTMS"], &[]);
        lo[1].params.insert("lpf".into(), 6000.0);
        let rx = generate_rx("fizzy", &lo, Instrument::Guitar);
        assert_eq!(rx.len(), 1);
        assert_eq!(rx[0].kind, RxKind::Advisory);
    }

    // ── playback-level (Fletcher–Munson) threshold offsets ──

    fn diag_keys_at(p: &SoundProfile, level: crate::profiles::PlaybackLevel) -> Vec<&'static str> {
        keys(&diagnose_kind(
            p,
            None,
            Family::Guitar,
            StimulusKind::Synthetic,
            None,
            playback_offsets(level),
        ))
    }

    #[test]
    fn stage_tightens_boomy_where_rehearsal_stays_silent() {
        use crate::profiles::PlaybackLevel::{Rehearsal, Stage};
        // dev(lows) = 3.0 dB — between Stage's 2.0 (4.0−2.0) and Rehearsal's 4.0
        // boomy threshold.
        let p = resid_profile(Family::Guitar, 0, 3.0);
        assert!(!diag_keys_at(&p, Rehearsal).contains(&"boomy"));
        assert!(diag_keys_at(&p, Stage).contains(&"boomy"));
    }

    #[test]
    fn quiet_relaxes_a_boomy_verdict_rehearsal_fires() {
        use crate::profiles::PlaybackLevel::{Quiet, Rehearsal};
        // dev(lows) = 5.0 — above Rehearsal's 4.0 but below Quiet's 6.0 (4.0+2.0).
        let p = resid_profile(Family::Guitar, 0, 5.0);
        assert!(diag_keys_at(&p, Rehearsal).contains(&"boomy"));
        assert!(!diag_keys_at(&p, Quiet).contains(&"boomy"));
    }

    #[test]
    fn stage_offset_lowers_the_fizzy_threshold() {
        use crate::profiles::PlaybackLevel::{Rehearsal, Stage};
        // fizzy fires when bdb[air]−bdb[highs] > fizzy_db (−9) + offset. Construct
        // air−highs = −9.5: below −9 (Rehearsal silent) but above Stage's −10 (fires),
        // proving the sign — a NEGATIVE offset makes fizzy fire MORE easily.
        let mut p = resid_profile(Family::Guitar, 0, 0.0);
        // Raise Air to sit 9.5 dB under Highs (the baseline rolls it 12 dB under).
        p.bands[5] = p.bands[4] * 10f64.powf(-9.5 / 10.0);
        assert!(!diag_keys_at(&p, Rehearsal).contains(&"fizzy"));
        assert!(diag_keys_at(&p, Stage).contains(&"fizzy"));
    }

    #[test]
    fn rehearsal_matches_legacy_diagnose_byte_for_byte() {
        // A hot low-mid (muddy) profile; the Rehearsal anchor must equal diagnose().
        let p = resid_profile(Family::Guitar, 1, GUITAR.muddy_db + 2.0);
        let legacy = serde_json::to_value(diagnose(&p, None, Family::Guitar)).unwrap();
        let rehearsal = serde_json::to_value(diagnose_kind(
            &p,
            None,
            Family::Guitar,
            StimulusKind::Synthetic,
            None,
            playback_offsets(crate::profiles::PlaybackLevel::Rehearsal),
        ))
        .unwrap();
        assert_eq!(legacy, rehearsal);
    }

    // ── R6: localized resonance (resonant/boxy) off the fine Welch PSD ──

    /// A capture = broadband noise + one strong sine, built via the REAL
    /// `welch_psd`/`SoundProfile::from_capture_with_psd` pipeline (never a
    /// hand-mocked `Psd`) — the resonant/boxy oracle fixture. 2 s at 48 kHz
    /// gives `find_peaks` ~22 Welch segments to average (SEG=8192, 50%
    /// overlap). The returned profile carries the measured `peaks`.
    fn ringing_profile(freq_hz: f32, sine_amp: f32) -> SoundProfile {
        const RATE: u32 = 48_000;
        let n = RATE as usize * 2;
        let mut samples = lcg_noise(n, 0.05, 71);
        for (s, t) in samples
            .iter_mut()
            .zip(test_sine(freq_hz, sine_amp, n, RATE as f32))
        {
            *s += t;
        }
        let psd = body_psd(&samples, RATE, 0);
        SoundProfile::from_capture_with_psd(&samples, RATE, samples.len(), 0, Family::Guitar, &psd)
            .expect("finite loudness")
    }

    #[test]
    fn oracle_resonant_fires_at_measured_freq_with_nearest_eq10_band() {
        // A clean 2.8 kHz ring (noise 0.05, sine 0.8) — empirically height≈60 dB,
        // Q≈478 (`cargo test` probe), miles past RESONANT_MIN_HEIGHT_DB(10)/
        // RESONANT_MIN_Q(4) and outside the 300–500 Hz boxy range. Log-nearest
        // EQ-10 band: |ln(2000/2800.78)|≈0.337 < |ln(4000/2800.78)|≈0.357, so
        // 2 kHz wins — matching the task's own worked example verbatim.
        let p = ringing_profile(2_800.0, 0.8);
        let peak = p.peaks[0];
        let nodes = chain(&["ACD_TweedDeluxe", "ACD_CabSimTMS"], &[]);
        let diags = diagnose_kind(
            &p,
            Some(&nodes),
            Family::Guitar,
            StimulusKind::Synthetic,
            None,
            PlaybackOffsets::NONE,
        );
        let d = diags
            .iter()
            .find(|d| d.key == "resonant")
            .expect("resonant should fire");
        assert_eq!(
            d.label,
            format!("Rings at {}", measured_freq_label(peak.freq_hz))
        );
        assert!(d.severity > 0.0, "severity {}", d.severity);
        let rx =
            d.rx.iter()
                .find(|r| r.kind == RxKind::Chain)
                .expect("an EQ-10 insert Rx");
        match &rx.ops[0] {
            DoctorOp::InsertNode {
                fender_id, params, ..
            } => {
                assert_eq!(fender_id, EQ10_STEREO);
                assert_eq!(params.len(), 1);
                assert_eq!(params[0].0, "gain2khz");
                assert!(
                    params[0].1 < 0.0,
                    "a resonant fix is always a CUT: {params:?}"
                );
            }
            other => panic!("expected InsertNode, got {other:?}"),
        }
    }

    #[test]
    fn oracle_boxy_range_sine_fires_boxy_not_resonant() {
        // Same shape, centered at 380 Hz — inside the 300–500 Hz boxy range and
        // easily past both rules' height/Q gates, so `boxy` (the more specific
        // verdict) must win and `resonant` must NOT also fire for this peak.
        let p = ringing_profile(380.0, 0.8);
        let peak = p.peaks[0];
        assert!(
            peak.height_db >= RESONANT_MIN_HEIGHT_DB && peak.q >= RESONANT_MIN_Q,
            "fixture must ALSO clear the resonant gate to prove the suppression: {peak:?}"
        );
        let nodes = chain(&["ACD_TweedDeluxe", "ACD_CabSimTMS"], &[]);
        let diags = diagnose_kind(
            &p,
            Some(&nodes),
            Family::Guitar,
            StimulusKind::Synthetic,
            None,
            PlaybackOffsets::NONE,
        );
        let keys = keys(&diags);
        assert!(keys.contains(&"boxy"), "{keys:?}");
        assert!(!keys.contains(&"resonant"), "{keys:?}");
        let d = diags.iter().find(|d| d.key == "boxy").unwrap();
        assert_eq!(
            d.label,
            format!("Boxy (a {} hump)", measured_freq_label(peak.freq_hz))
        );
    }

    #[test]
    fn oracle_below_gate_peak_is_silent() {
        // A real but weak 2.8 kHz peak (sine amp 0.001) — `find_peaks` still
        // registers it (height≈4.6 dB clears the 3 dB detection floor), but it
        // never reaches RESONANT_MIN_HEIGHT_DB(10)/BOXY_MIN_HEIGHT_DB(7), so
        // neither rule fires.
        let p = ringing_profile(2_800.0, 0.001);
        let peaks = &p.peaks;
        assert_eq!(
            peaks.len(),
            1,
            "fixture must register exactly one candidate peak: {peaks:?}"
        );
        assert!(
            peaks[0].height_db < BOXY_MIN_HEIGHT_DB,
            "fixture must sit under BOTH gates: {peaks:?}"
        );
        let keys = keys(&diagnose_kind(
            &p,
            None,
            Family::Guitar,
            StimulusKind::Synthetic,
            None,
            PlaybackOffsets::NONE,
        ));
        assert!(!keys.contains(&"resonant"), "{keys:?}");
        assert!(!keys.contains(&"boxy"), "{keys:?}");
    }

    #[test]
    fn empty_peaks_produce_no_localized_diags_and_match_the_peaked_baseline() {
        // The same ringing capture, diagnosed once with its measured
        // `profile.peaks` (resonant fires) and once with `peaks` cleared (the
        // PSD-less-profile shape: curated fixtures, hand-built vectors) —
        // every OTHER diag must be byte-identical, proving the localized rules
        // are a strict addition that never perturbs the pre-existing
        // band/tilt/washed/spiky/buried rules.
        let p = ringing_profile(2_800.0, 0.8);
        let mut peakless = p.clone();
        peakless.peaks = Vec::new();
        let diagnose_one = |profile: &SoundProfile| {
            diagnose_kind(
                profile,
                None,
                Family::Guitar,
                StimulusKind::Synthetic,
                None,
                PlaybackOffsets::NONE,
            )
        };
        let with_peaks = diagnose_one(&p);
        let without_peaks = diagnose_one(&peakless);
        assert!(keys(&with_peaks).contains(&"resonant"));
        assert!(!keys(&without_peaks).contains(&"resonant"));
        assert!(!keys(&without_peaks).contains(&"boxy"));
        let non_localized = |ds: &[Diag]| -> Vec<serde_json::Value> {
            ds.iter()
                .filter(|d| d.key != "resonant" && d.key != "boxy")
                .map(|d| serde_json::to_value(d).unwrap())
                .collect()
        };
        assert_eq!(non_localized(&with_peaks), non_localized(&without_peaks));
    }

    #[test]
    fn playback_offsets_are_monotonic() {
        use crate::profiles::PlaybackLevel::{Quiet, Rehearsal, Stage};
        // Louder ⇒ tighter (lower) thresholds, so `diagnose_levels`' firing set is
        // always a louder-suffix and `from_level` fully describes it. Guard the
        // relation any future offset edit must preserve.
        let q = playback_offsets(Quiet);
        let r = playback_offsets(Rehearsal);
        let s = playback_offsets(Stage);
        assert!(q.low_end_db > r.low_end_db && r.low_end_db > s.low_end_db);
        assert!(q.fizzy_db > r.fizzy_db && r.fizzy_db > s.fizzy_db);
    }

    #[test]
    fn diagnose_levels_tags_each_finding_with_its_quietest_level() {
        // A boomy-only profile that fires at Stage (2.0) and Rehearsal (4.0) but
        // NOT Quiet (6.0): dev(lows) = 5.0.
        let p = resid_profile(Family::Guitar, 0, 5.0);
        let leveled = diagnose_levels(&p, None, Family::Guitar, StimulusKind::Synthetic, None);
        let boomy = leveled
            .iter()
            .find(|d| d.diag.key == "boomy")
            .expect("boomy should fire at rehearsal+stage");
        assert_eq!(boomy.from_level, crate::profiles::PlaybackLevel::Rehearsal);
        // A level-independent finding (a mid scoop → lost) tags `quiet` (fires
        // everywhere). Add a scoop hot enough to fire, keep it in one profile.
        let scooped = resid_profile(Family::Guitar, 2, -(GUITAR.lost_db + 2.0));
        let leveled = diagnose_levels(
            &scooped,
            None,
            Family::Guitar,
            StimulusKind::Synthetic,
            None,
        );
        let lost = leveled
            .iter()
            .find(|d| d.diag.key == "lost")
            .expect("lost fires");
        assert_eq!(lost.from_level, crate::profiles::PlaybackLevel::Quiet);
    }

    // ── serde wire shapes ──

    #[test]
    fn leveled_diag_flattens_diag_and_adds_from_level() {
        // The flatten means the wire object carries the Diag fields at the top
        // level PLUS `fromLevel` — the src/lib/types.ts DoctorDiag mirror.
        // Binding (centered) gate + the Quiet low_end_db offset (+2.0) + a 1.0
        // margin clears the Quiet-relaxed threshold in BOTH consensus spaces —
        // the rule is a strict `>`, so sitting exactly on the boundary is not a
        // safe way to assert "fires at every level".
        let p = resid_profile(
            Family::Guitar,
            1,
            GUITAR.muddy_db.max(GUITAR.muddy_centered_db) + 2.0 + 1.0,
        );
        let leveled = diagnose_levels(&p, None, Family::Guitar, StimulusKind::Synthetic, None);
        let v = serde_json::to_value(&leveled[0]).unwrap();
        let obj = v.as_object().unwrap();
        assert!(obj.contains_key("key"), "flattened Diag field present");
        assert!(obj.contains_key("detail"));
        assert_eq!(obj["fromLevel"], "quiet");
    }

    #[test]
    fn doctor_op_serializes_camel_case() {
        let op = DoctorOp::Param {
            group_id: "G1".into(),
            node_id: "ACD_CabSimTMS".into(),
            param: "lpf".into(),
            value: 8000.0,
        };
        let v = serde_json::to_value(&op).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "kind": "param", "groupId": "G1", "nodeId": "ACD_CabSimTMS",
                "param": "lpf", "value": 8000.0
            })
        );
        let ins = DoctorOp::InsertNode {
            group_id: "G1".into(),
            before_fender_id: None,
            fender_id: "ACD_DynaComp".into(),
            params: vec![],
        };
        let v = serde_json::to_value(&ins).unwrap();
        assert_eq!(v["kind"], "insert_node");
        assert!(v.get("beforeFenderId").is_some());
        assert_eq!(v["fenderId"], "ACD_DynaComp");
    }

    #[test]
    fn insert_cpu_note_reports_real_delta_and_gates_caps() {
        let p = chain(&["ACD_TweedDeluxe"], &[]);
        let note = insert_cpu_note(&p, EQ10_STEREO).expect("EQ10 fits");
        assert!(note.starts_with('+') && note.ends_with("% CPU"), "{note}");
    }

    // ── real device-exported fixture (casing ground truth) ──

    #[test]
    fn scenario_fixture_round_trip() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../e2e/fixtures/scenario-presets.json");
        let Ok(raw) = std::fs::read_to_string(&path) else {
            eprintln!("skip: {} absent", path.display());
            return;
        };
        let rows: Vec<Value> = serde_json::from_str(&raw).unwrap();
        let preset: Value = serde_json::from_str(rows[0]["presetJson"].as_str().unwrap()).unwrap();
        // The reference preset (TweedDeluxe + pedals, no cab) exercises the
        // insert paths with REAL device JSON, through the SAME graph decoder
        // the backup scan uses (so the DoctorNode mapping is exercised too).
        let nodes: Vec<DoctorNode> = crate::session::extract_active_graph(&preset, None)
            .nodes
            .iter()
            .map(DoctorNode::from_graph_node)
            .collect();
        assert!(!nodes.is_empty(), "fixture graph decodes");
        let rx = generate_rx("fizzy", &nodes, Instrument::Guitar);
        assert!(!rx.is_empty());
        let rx = generate_rx("lost", &nodes, Instrument::Guitar);
        assert!(rx.iter().any(|r| r.kind == RxKind::Chain));
    }

    #[test]
    fn showcase_profile_diagnoses() {
        // The marketing-screenshot presets, judged under the deviation-from-
        // target + Theil–Sen tilt/local metric. Each mapped preset must produce
        // exactly its intended diagnosis pair; every other slot must be clear.
        // Guards docs/assets/doctor.png from silently reverting to "All clear"
        // on a threshold retune. Under Theil–Sen a tilt (dark) and a local bump
        // (muddy) CAN co-fire together (see showcase_profile's doc), so preset
        // 11 is dark+muddy; together the three presets cover five of the six
        // guitar diagnoses.
        let diag_set = |idx: u32| {
            let mut got = keys(&diagnose(&showcase_profile(idx), None, Instrument::Guitar));
            got.sort_unstable();
            got
        };
        assert_eq!(diag_set(4), vec!["lost", "washed"]); // Scooped Verse
        assert_eq!(diag_set(11), vec!["dark", "muddy"]); // Tweed Warm
        assert_eq!(diag_set(167), vec!["fizzy", "harsh"]); // Direct Acoustic
        assert!(diag_set(0).is_empty()); // any other preset → all clear
    }
}
