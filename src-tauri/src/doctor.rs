//! The Doctor diagnosis engine — PURE (no device I/O, no Tauri).
//!
//! Turns a re-amp capture's measurements (`SoundProfile`) into named tone
//! diagnoses (muddy / boomy / harsh / fizzy / washed / lost / buried) with
//! concrete, graph-derived prescriptions (`Rx` → `DoctorOp`s), plus the
//! scene-loudness consistency check. The device work (capture, apply) lives in
//! `leveller::doctor_capture` and the `doctor_*` commands; this module is the
//! rules.
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
//! feedback pedal that substring-matches "eq"). The `DIST_IDS` / `REVERB_MIX`
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
/// "balance space" (a band's dB offset from the sound's own spectral mean,
/// compared against the cohort median or the absolute neighbour expectation).
///
/// PROVISIONAL pending hardware calibration (probe --doctor sweeps); the
/// calibration pass edits values here and nowhere else.
pub struct Thresholds {
    /// Low-mid excess ⇒ muddy.
    pub muddy_db: f64,
    /// Lows excess ⇒ boomy.
    pub boomy_db: f64,
    /// High-mid spike ⇒ harsh.
    pub harsh_db: f64,
    /// Air relative to the sound's OWN presence band (`bal[Air] − bal[Highs]`)
    /// ⇒ fizzy. NOT cohort-relative: real-library calibration showed the Air
    /// band is bimodal across a library (cab'd presets roll off 25–44 dB,
    /// open ones 10–20), which makes the cohort median pathologically low and
    /// flags every open preset. Fizz is HF hash extending past 6 kHz — i.e.
    /// Air failing to roll off below the presence band, a property of the
    /// sound itself.
    pub fizzy_db: f64,
    /// Mids deficit (scoop) ⇒ lost in the mix.
    pub lost_db: f64,
    /// Post-stimulus tail RMS relative to the body (dB; closer to 0 = wetter)
    /// ⇒ washed out.
    pub wash_tail_db: f64,
    /// Lows deficit on a driven bass ⇒ buried clean tone (bass rule).
    pub buried_lows_db: f64,
    /// Dynamics spread (short-term-max − integrated LU) on a clean chain ⇒
    /// spiky. PROVISIONAL: ordinary presets read 0.12–0.8 LU under the 0.8 s
    /// LEVELING capture (see notes/doctor-calibration.md) — the Doctor capture
    /// appends a 2.5 s tail that can inflate spread on wet presets, so this
    /// value needs a fresh probe --doctor baseline before it is trusted.
    pub spiky_spread_lu: f64,
    /// Scene-to-base loudness jump ⇒ scene-consistency flag.
    pub scene_delta_db: f64,
}

/// Calibrated 2026-07-03 against a 14-preset real-library guitar sweep
/// (`probe --doctor 0..16 guitar-humbucker`): washed caught the three
/// genuinely wash-heavy presets (Shoegaze −0.3 dB tail, Reverse Delay −6.9,
/// Synth pad −2.7) with dry presets at −17…−25; muddy/boomy flagged the one
/// dark preset (+14.6 dB lows vs library); harsh flagged the two peaky
/// presets; fizzy was re-derived (see `Thresholds::fizzy_db`).
pub const GUITAR: Thresholds = Thresholds {
    muddy_db: 4.5,
    boomy_db: 5.0,
    harsh_db: 5.0,
    fizzy_db: -9.0,
    lost_db: 4.5,
    wash_tail_db: -13.0,
    buried_lows_db: f64::INFINITY, // bass-only rule
    spiky_spread_lu: 4.0,
    scene_delta_db: 3.0,
};

pub const BASS: Thresholds = Thresholds {
    muddy_db: 5.0,
    boomy_db: 6.0,
    harsh_db: 5.0,
    fizzy_db: -9.0,
    lost_db: 5.0,
    wash_tail_db: -13.0,
    buried_lows_db: 4.0,
    spiky_spread_lu: 4.0,
    scene_delta_db: 3.0,
};

/// PROVISIONAL — a straight copy of `BASS`, pending the attended Bass VI
/// calibration sweep (PR-7). Bass VI shares bass's low-fundamental character, so
/// bass thresholds are the honest starting point until `probe --doctor` on a
/// real Bass VI library re-derives them.
pub const BASS_VI: Thresholds = BASS;

/// The instrument family a sound is judged as. Drives both the threshold table
/// and the analysis-band LAYOUT: Guitar/Bass share the 6-band [`BANDS_6`], Bass
/// VI adds a 7th "Sub" band ([`BANDS_7`]) for its sub-60 Hz fundamentals. The
/// semantic band accessors (`idx_lows` … `idx_air`) hide the layout shift so the
/// rules read the same band by MEANING regardless of family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    Guitar,
    Bass,
    BassVi,
}

/// Migration alias — the old name for [`Family`] before the Bass VI split.
pub type Instrument = Family;

impl Family {
    pub fn thresholds(self) -> &'static Thresholds {
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
    /// Index of the first band of the shared 6-band block (0 for guitar/bass; 1
    /// for Bass VI, whose Sub band sits at index 0). The `idx_*` accessors offset
    /// from here so every rule addresses a band by MEANING, not raw index.
    fn base(self) -> usize {
        match self {
            Family::BassVi => 1,
            _ => 0,
        }
    }
    pub fn idx_lows(self) -> usize {
        self.base()
    }
    pub fn idx_low_mids(self) -> usize {
        self.base() + 1
    }
    pub fn idx_mids(self) -> usize {
        self.base() + 2
    }
    pub fn idx_high_mids(self) -> usize {
        self.base() + 3
    }
    pub fn idx_highs(self) -> usize {
        self.base() + 4
    }
    pub fn idx_air(self) -> usize {
        self.base() + 5
    }
}

/// One captured sound's measurements — everything `diagnose` needs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundProfile {
    /// Mean Goertzel band power per [`Family::bands`] band (raw power, not dB).
    /// Length follows the family's layout (6 guitar/bass, 7 Bass VI).
    pub bands: Vec<f64>,
    pub integrated_lufs: f64,
    /// Short-term-max − integrated LUFS (see `lufs::Loudness::spread_lu`):
    /// gain-invariant dynamics spread of the capture.
    pub spread_lu: f64,
    /// Post-stimulus tail RMS vs body RMS, in dB (see [`tail_energy_ratio`]).
    pub tail_ratio_db: f64,
}

impl SoundProfile {
    /// Build a profile from one captured mono signal: the 6 Doctor-band
    /// energies, integrated loudness, and the reverb-wash tail ratio. Shared by
    /// the `doctor_check` command and the `probe --doctor` calibration sweep.
    /// `onset` = where the stimulus starts inside the capture (0 = un-aligned).
    /// Bands + loudness deliberately stay WHOLE-BUFFER (leading silence dilutes
    /// band powers uniformly, so the relative `balance` space is unchanged and
    /// the gated LUFS discards it) — only the body/tail split aligns.
    pub fn from_capture(
        samples: &[f32],
        rate: u32,
        stimulus_samples: usize,
        onset: usize,
        family: Family,
    ) -> Result<SoundProfile, String> {
        let bands = crate::spectrum::band_energies(samples, rate as f32, family.bands());
        let loudness = crate::lufs::measure_mono(samples, rate)?;
        let integrated_lufs = loudness.integrated_lufs;
        // A silent capture measures −inf — route the sound to the errors lane
        // (the leveller's sentinel philosophy) instead of poisoning the cohort
        // median and the scene deltas with non-finite numbers.
        if !integrated_lufs.is_finite() {
            return Err("no signal on USB 1/2 — the sound is silent".to_string());
        }
        Ok(SoundProfile {
            bands,
            integrated_lufs,
            spread_lu: loudness.spread_lu(),
            tail_ratio_db: tail_energy_ratio(samples, rate, stimulus_samples, onset),
        })
    }
}

/// Curated Doctor `SoundProfile`s for the marketing-screenshot showcase
/// (`TMP_E2E_SHOWCASE=1`). The offline fake re-amp returns the raw stimulus for
/// every preset, so every sound would measure identically and the Results page
/// would read "All clear". Instead `doctor_check` injects these per showcase list
/// index (`commands/doctor.rs`), so the REAL `diagnose` engine renders genuine
/// cards.
///
/// The three mapped presets together cover all six guitar diagnoses (each carries
/// two) under the ABSOLUTE-fallback path: the tour selects exactly these three
/// PLAIN presets (3 sounds < [`MIN_COHORT`], so `diagnose` gets `cohort = None`).
/// Band values are dB offsets (re-centered by [`balance`]); `fizzy` (Air − Highs)
/// and `washed` (tail) ride independently of the band-shape rules. Verified by the
/// `showcase_profile_diagnoses` test — keep those presets PLAIN and scene-less.
#[cfg(any(test, feature = "e2e"))]
pub(crate) fn showcase_profile(list_index: u32) -> SoundProfile {
    // (band dB `[Lo, LoM, Mid, HiM, Hi, Air]`, tail_ratio_db). Scooped Verse (index 4)
    // carries ONLY `lost` among the band-shape rules, so its `lost` diagnosis sorts
    // first in the row — the tour expands it to feature the add-a-compressor fix.
    let (db, tail): ([f64; 6], f64) = match list_index {
        4 => ([0.0, 1.0, -5.0, 1.0, -1.0, -16.0], -8.0), // Scooped Verse → lost + washed
        11 => ([16.0, 10.0, -6.0, -14.0, -8.0, -24.0], -80.0), // Tweed Warm → muddy + boomy
        167 => ([0.0, 1.0, 3.0, 10.0, 4.0, 2.0], -80.0), // Direct Acoustic → harsh + fizzy
        _ => ([0.0, 1.0, 1.0, 0.0, -2.0, -14.0], -80.0), // any other preset → all clear
    };
    SoundProfile {
        bands: db.iter().map(|d| 10f64.powf(d / 10.0)).collect(),
        integrated_lufs: -18.0,
        // Steady by construction — the showcase never features the spiky card.
        spread_lu: 0.0,
        tail_ratio_db: tail,
    }
}

fn to_db(p: f64) -> f64 {
    10.0 * p.max(1e-12).log10()
}

/// A sound's spectral "balance": each band's dB offset from the sound's own
/// mean band level. Level-invariant, so cohort comparison is about tone shape,
/// not loudness.
pub fn balance(bands: &[f64]) -> Vec<f64> {
    let db: Vec<f64> = bands.iter().copied().map(to_db).collect();
    let n = db.len().max(1);
    let mean = db.iter().sum::<f64>() / n as f64;
    db.iter().map(|d| d - mean).collect()
}

/// Per-band median of the cohort's balances — the "what this player's library
/// sounds like" reference `diagnose` judges deviations against. All profiles in
/// a cohort share ONE layout (cohorts are per-family), so the output length
/// matches their band count.
pub fn cohort_median(profiles: &[&SoundProfile]) -> Vec<f64> {
    let Some(first) = profiles.first() else {
        return Vec::new();
    };
    let balances: Vec<Vec<f64>> = profiles.iter().map(|p| balance(&p.bands)).collect();
    (0..balance(&first.bands).len())
        .map(|i| {
            let mut v: Vec<f64> = balances.iter().map(|b| b[i]).collect();
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        })
        .collect()
}

/// Minimum cohort size for relative judging; below this `diagnose` falls back
/// to absolute neighbour-expectation heuristics.
// ponytail: cohort median, add a persisted rolling baseline only if
// single-sound runs prove unreliable in calibration.
pub const MIN_COHORT: usize = 4;

/// Per-family cohort medians: each family's sounds are judged against their OWN
/// library median — a bass preset judged against a guitar cohort reads falsely
/// boomy, and families don't even share a band LAYOUT (Bass VI has 7), so a
/// median can never mix them. `None` for a family whose group is under
/// [`MIN_COHORT`] (that group's sounds diagnose with the absolute fallback);
/// only families actually present in `profiles` appear as keys.
pub fn cohorts_by_instrument(
    profiles: &[(Family, &SoundProfile)],
) -> HashMap<Family, Option<Vec<f64>>> {
    let mut out: HashMap<Family, Option<Vec<f64>>> = HashMap::new();
    for fam in [Family::Guitar, Family::Bass, Family::BassVi] {
        let refs: Vec<&SoundProfile> = profiles
            .iter()
            .filter(|(f, _)| *f == fam)
            .map(|(_, p)| *p)
            .collect();
        if refs.is_empty() {
            continue;
        }
        out.insert(
            fam,
            (refs.len() >= MIN_COHORT).then(|| cohort_median(&refs)),
        );
    }
    out
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
/// 50 ms leak into a 2.5 s tail). Pass 0 to keep the un-aligned legacy split.
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
    let rms = |s: &[f32]| -> f64 {
        if s.is_empty() {
            return 0.0;
        }
        (s.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>() / s.len() as f64).sqrt()
    };
    let body = rms(&samples[onset..body_end]);
    let tail = rms(&samples[body_end..]);
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
    pub label: &'static str,
    pub sev: Sev,
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
    /// Scene-consistency trim: re-level `scene` so it sits `targetDeltaDb`
    /// above the base scene (applied via the scene-leveling machinery).
    SceneTrim {
        scene: u32,
        #[serde(rename = "targetDeltaDb")]
        target_delta_db: f64,
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
    /// The node's allowlisted current param values (reverb mix names + EQ-10
    /// band gains — see `session::GraphNode.params`), for value-aware
    /// prescriptions. `default` so pre-params payloads still deserialize.
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
    /// (group, node_id) of the standalone cab or the amp carrying an embedded
    /// cab (`cab_sim_id` present). Both expose the same `hpf`/`lpf`
    /// controlIds (schema-verified).
    cab: Option<(String, String)>,
    /// Existing EQ-10 stereo node, if any: (group, node_id, current band gains).
    eq10: Option<(String, String, HashMap<String, f64>)>,
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
            f.cab = Some((n.group_id.clone(), n.node_id.clone()));
        }
        if n.model == EQ10_STEREO {
            f.eq10 = Some((n.group_id.clone(), n.node_id.clone(), n.params.clone()));
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

/// EQ-10 insert carrying `gains` (controlId → dB) on the fresh node; all other
/// bands stay at the device default (0 dB).
fn eq10_insert(nodes: &[DoctorNode], group: &str, gains: &[(&str, f64)]) -> Option<DoctorOp> {
    insert_cpu_note(nodes, EQ10_STEREO)?;
    Some(DoctorOp::InsertNode {
        group_id: group.to_string(),
        before_fender_id: None,
        fender_id: EQ10_STEREO.to_string(),
        params: gains.iter().map(|(p, v)| ((*p).to_string(), *v)).collect(),
    })
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

/// The muddy/harsh EQ move: reuse the chain's EQ-10 when present, else insert
/// one. `gains` are RELATIVE moves (e.g. −3 = cut 3 dB): on an existing EQ-10
/// whose current band values are known they're applied on top (`current + move`,
/// clamped to the band range) under `reuse_title`; when a band's current value
/// is unknown the write is the absolute value and the title says that truth
/// ("Set the 250 Hz band to −3 dB"). A fresh insert starts at 0 dB, so absolute
/// == relative there. Returns None when no EQ-10 exists and the insert fails
/// the caps.
fn eq_move(
    nodes: &[DoctorNode],
    facts: &GraphFacts,
    reuse_title: &str,
    reuse_detail: &str,
    insert_title: &str,
    insert_detail: &str,
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
            reuse_title.to_string()
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
            detail: reuse_detail.to_string(),
            cpu_note: "no CPU change".to_string(),
            ops,
            chain: None,
        });
    }
    let group = facts
        .cab
        .as_ref()
        .map(|(g, _)| g.clone())
        .or_else(|| facts.front.as_ref().map(|(g, _)| g.clone()))?;
    let cpu_note = insert_cpu_note(nodes, EQ10_STEREO)?;
    let op = eq10_insert(nodes, &group, gains)?;
    Some(Rx {
        kind: RxKind::Chain,
        title: insert_title.to_string(),
        detail: insert_detail.to_string(),
        cpu_note,
        ops: vec![op],
        chain: Some(chain_preview(
            nodes,
            "after · +EQ",
            EQ10_STEREO,
            nodes.len(),
        )),
    })
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
    if let Some((group, node)) = &facts.cab {
        // Both the standalone cab and CabIR amps expose `hpf` (20–500) /
        // `lpf` (1000–20000) — schema-verified, same controlIds.
        let param = if is_low_cut { "hpf" } else { "lpf" };
        return Some(Rx {
            kind: RxKind::OneClick,
            title: cab_title.to_string(),
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
    let (cab_group, cab_node) = facts.cab.as_ref()?;
    let idx = nodes
        .iter()
        .position(|n| &n.group_id == cab_group && &n.node_id == cab_node)?;
    // A reverb/delay earlier in the cab's own group would then also sit before
    // the inserted comp — compressing its wet tail pumps, so bail to the
    // advisory-only path (the caller's None branch) instead of anchoring a
    // placement that contradicts the prescription's own detail text.
    let time_effect_before_cab = nodes[..idx]
        .iter()
        .any(|n| n.group_id == *cab_group && !n.bypassed && is_time_effect(&n.model));
    if time_effect_before_cab {
        return None;
    }
    // Anchor = the NEXT node in the SAME group: the wire's beforeFenderId is a
    // same-group anchor and a cross-group one is silently dropped (proto.rs
    // field 2), so a cab that ends its group appends (None) — same position.
    // Bypassed neighbours still anchor: position is chain order, not
    // audibility — skipping one would drift the comp when it's re-enabled.
    let before = nodes
        .get(idx + 1)
        .filter(|n| &n.group_id == cab_group)
        .map(|n| n.node_id.clone());
    let cpu_note = insert_cpu_note(nodes, COMPRESSOR_STUDIO)?;
    Some(Rx {
        kind: RxKind::Chain,
        title: "Add a studio compressor after the cab".to_string(),
        detail: "Evens out the level after the cab, transparently — the right fix when the swings come from your playing rather than an effect doing its job."
            .to_string(),
        cpu_note,
        ops: vec![DoctorOp::InsertNode {
            group_id: cab_group.clone(),
            before_fender_id: before,
            fender_id: COMPRESSOR_STUDIO.to_string(),
            params: Vec::new(),
        }],
        chain: Some(chain_preview(nodes, "after · +COMP", COMPRESSOR_STUDIO, idx + 1)),
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
                "Cut 3 dB around 300 Hz on your EQ",
                "Dips the muddy low-mids on the EQ you already have — the note stays, the mud goes.",
                "Add a 10-band EQ and cut 3 dB around 300 Hz",
                "Puts a graphic EQ after the cab and dips the muddy low-mids — the note stays, the mud goes.",
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
                "Cut 2 dB around 1–3 kHz",
                "Dips the harsh high-mids right where the spike lives and leaves the rest of the tone alone.",
                "Cut 2 dB around 1–3 kHz",
                "Dips the harsh high-mids right where the spike lives and leaves the rest of the tone alone.",
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
        _ => Vec::new(),
    }
}

// ─── diagnosis ───────────────────────────────────────────────────────────────

/// Diagnose one sound. `cohort` is the run-cohort balance median when the run
/// measured ≥ [`MIN_COHORT`] sounds (relative judging), else `None` (absolute
/// neighbour-expectation fallback). `nodes` (the preset's chain, from the
/// backup scan's graph) enriches detection (drive presence) and drives
/// prescriptions; diagnosis still works without it (graph-dependent rx are
/// simply absent).
pub fn diagnose(
    profile: &SoundProfile,
    nodes: Option<&[DoctorNode]>,
    instrument: Family,
    cohort: Option<&[f64]>,
) -> Vec<Diag> {
    let t = instrument.thresholds();
    let bal = balance(&profile.bands);
    let n = bal.len();
    // Deviation per band: vs cohort median, or vs the sound's own neighbour
    // expectation (the band's dB above the mean of its two spectral
    // neighbours) when the cohort is too small to trust.
    let dev = |i: usize| -> f64 {
        match cohort {
            Some(med) => bal[i] - med[i],
            None => {
                let lo = if i == 0 { bal[1] } else { bal[i - 1] };
                let hi = if i == n - 1 { bal[n - 2] } else { bal[i + 1] };
                bal[i] - (lo + hi) / 2.0
            }
        }
    };
    let (lows, low_mids, mids, high_mids, highs, air) = (
        instrument.idx_lows(),
        instrument.idx_low_mids(),
        instrument.idx_mids(),
        instrument.idx_high_mids(),
        instrument.idx_highs(),
        instrument.idx_air(),
    );
    let facts = nodes.map(graph_facts);
    let mut out = Vec::new();
    let mut push = |key: &'static str,
                    label: &'static str,
                    sev: Sev,
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
            bands,
            detail,
            explain,
            rx,
        });
    };

    if dev(low_mids) > t.muddy_db {
        push(
            "muddy",
            "Muddy",
            Sev::High,
            vec![low_mids],
            format!("buildup around 250–350 Hz ({:+.1} dB)", dev(low_mids)),
            "There's a buildup in the low-mids that stacks up with the bass player.",
        );
    }
    if dev(lows) > t.boomy_db {
        push(
            "boomy",
            "Boomy",
            Sev::Med,
            vec![lows],
            format!("excess energy below 100 Hz ({:+.1} dB)", dev(lows)),
            "Too much deep low end — it booms and loses focus once you turn up.",
        );
    }
    if dev(high_mids) > t.harsh_db {
        push(
            "harsh",
            "Harsh",
            Sev::High,
            vec![high_mids],
            format!("spike around 1–3 kHz ({:+.1} dB)", dev(high_mids)),
            "A sharp peak in the high-mids makes it harsh and tiring to listen to.",
        );
    }
    // Fizz is judged against the sound's own presence band, not the cohort —
    // see `Thresholds::fizzy_db` for the calibration rationale.
    if bal[air] - bal[highs] > t.fizzy_db {
        push(
            "fizzy",
            "Fizzy",
            Sev::Med,
            vec![air],
            format!(
                "content above 6 kHz only {:.1} dB under the presence band",
                bal[highs] - bal[air]
            ),
            "Fizzy, buzzy top end — the kind that sounds like radio static on the note tails.",
        );
    }
    if -dev(mids) > t.lost_db {
        push(
            "lost",
            "Gets lost in the mix",
            Sev::High,
            vec![mids],
            format!("mids scooped {:.1} dB around 800 Hz", -dev(mids)),
            "The mids are scooped, so it sounds big alone but disappears with a full band.",
        );
    }
    if profile.tail_ratio_db > t.wash_tail_db {
        let detail = format!(
            "decay tail only {:.0} dB under the note",
            -profile.tail_ratio_db
        );
        push(
            "washed",
            "Washed out",
            Sev::Med,
            vec![],
            detail,
            "The reverb is drowning the note — it washes out instead of ringing clearly.",
        );
    }
    // Spread is gain-invariant and preset-intrinsic (lufs::spread_lu), so it's
    // judged absolutely, cohort-independent — like fizzy. Graph required: a
    // drive pedal compresses naturally, so spiky is a CLEAN-chain finding.
    if profile.spread_lu > t.spiky_spread_lu && facts.as_ref().map(|f| f.has_drive) == Some(false) {
        push(
            "spiky",
            "Spiky",
            Sev::Med,
            vec![],
            format!("swings {:.1} LU between peaks and average", profile.spread_lu),
            "The level jumps between loud peaks and a much quieter average — it pokes out of the mix one moment and disappears the next.",
        );
    }
    if matches!(instrument, Family::Bass | Family::BassVi)
        && facts.as_ref().map(|f| f.has_drive) == Some(true)
        && -dev(lows) > t.buried_lows_db
    {
        push(
            "buried",
            "Buried clean tone",
            Sev::Med,
            vec![],
            "drive stacked in series".to_string(),
            "Your clean low end is buried under the drive — common on a driven bass sound.",
        );
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
                    "{worst_name} measured {worst_delta:+.1} dB vs the base scene, but the first scene's USB reading can differ from the footswitch (a known device quirk) — check it by ear before trimming."
                ),
            )],
            Some(scene) => vec![Rx {
                kind: RxKind::OneClick,
                title: format!("Trim {worst_name} to +2 dB and add a mid boost"),
                detail: format!(
                    "Pros keep lead sounds only +1–3 dB louder and lean on a mid boost to cut through — not raw volume. This trims {worst_name} and nudges its mids up."
                ),
                cpu_note: "no CPU change".to_string(),
                ops: vec![DoctorOp::SceneTrim {
                    scene,
                    target_delta_db: 2.0,
                }],
                chain: None,
            }],
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
        let tail_n = SR as usize * 5 / 2; // 2.5 s Doctor tail
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

    /// A "healthy" spectrum (flat with the natural ≥ 20 dB Air rolloff a cab
    /// gives) with one band offset by `db`. Air stays rolled off so the fizz
    /// rule (which fires on MISSING rolloff) never muddies the other rules'
    /// assertions.
    fn profile_with(band: usize, db: f64) -> SoundProfile {
        let mut bands = vec![1.0; 6];
        bands[5] = 10f64.powf(-20.0 / 10.0);
        bands[band] = 10f64.powf(db / 10.0);
        SoundProfile {
            bands,
            integrated_lufs: -20.0,
            spread_lu: 0.0,
            tail_ratio_db: -40.0,
        }
    }

    /// Cohort median = the healthy baseline's own balance (what a library of
    /// healthy cab'd sounds medians out to), so `dev` isolates the offset a
    /// test injects.
    fn flat_cohort() -> Vec<f64> {
        balance(&profile_with(0, 0.0).bands)
    }

    fn keys(diags: &[Diag]) -> Vec<&'static str> {
        diags.iter().map(|d| d.key).collect()
    }

    // ── rules fire just-above, stay silent just-below ──

    #[test]
    fn muddy_fires_above_threshold_only() {
        let cohort = flat_cohort();
        // dev(1) after self-normalization is slightly under the raw offset, so
        // overshoot the constant by a couple dB for the firing case.
        let hot = profile_with(1, GUITAR.muddy_db + 3.0);
        let cold = profile_with(1, GUITAR.muddy_db - 1.0);
        assert!(keys(&diagnose(&hot, None, Instrument::Guitar, Some(&cohort))).contains(&"muddy"));
        assert!(
            !keys(&diagnose(&cold, None, Instrument::Guitar, Some(&cohort))).contains(&"muddy")
        );
    }

    #[test]
    fn boomy_and_harsh_fire_on_their_bands() {
        let cohort = flat_cohort();
        for (band, key, thr) in [
            (0usize, "boomy", GUITAR.boomy_db),
            (3, "harsh", GUITAR.harsh_db),
        ] {
            let hot = profile_with(band, thr + 3.0);
            let got = diagnose(&hot, None, Instrument::Guitar, Some(&cohort));
            assert!(keys(&got).contains(&key), "{key} should fire");
        }
    }

    #[test]
    fn fizzy_fires_on_missing_air_rolloff() {
        // Fizz = Air failing to roll off below the presence band (own-spectrum
        // rule, cohort-independent — see Thresholds::fizzy_db).
        let mut hash = profile_with(2, 0.0);
        hash.bands[5] = hash.bands[4]; // no rolloff at all: air == highs
        assert!(keys(&diagnose(&hash, None, Instrument::Guitar, None)).contains(&"fizzy"));
        // A cab'd sound (−20 dB air, the profile_with default) never fizzes.
        let cabbed = profile_with(2, 0.0);
        assert!(!keys(&diagnose(&cabbed, None, Instrument::Guitar, None)).contains(&"fizzy"));
    }

    #[test]
    fn lost_fires_on_mid_scoop() {
        let cohort = flat_cohort();
        let scooped = profile_with(2, -(GUITAR.lost_db + 3.0));
        assert!(
            keys(&diagnose(&scooped, None, Instrument::Guitar, Some(&cohort))).contains(&"lost")
        );
    }

    #[test]
    fn washed_fires_on_wet_tail() {
        let mut p = profile_with(2, 0.0);
        p.tail_ratio_db = GUITAR.wash_tail_db + 5.0;
        assert!(keys(&diagnose(&p, None, Instrument::Guitar, None)).contains(&"washed"));
        p.tail_ratio_db = GUITAR.wash_tail_db - 5.0;
        assert!(!keys(&diagnose(&p, None, Instrument::Guitar, None)).contains(&"washed"));
    }

    // ── absolute fallback (no cohort) ──

    #[test]
    fn absolute_fallback_judges_vs_neighbours() {
        // A lone hot low-mid band still reads muddy without any cohort.
        let hot = profile_with(1, GUITAR.muddy_db + 4.0);
        assert!(keys(&diagnose(&hot, None, Instrument::Guitar, None)).contains(&"muddy"));
        let flat = profile_with(1, 0.0);
        assert!(keys(&diagnose(&flat, None, Instrument::Guitar, None)).is_empty());
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
        let cohort = flat_cohort();
        let clean = chain(&["ACD_TweedDeluxe", "ACD_CabSimTMS"], &[]);
        let driven = chain(
            &["ACD_TubeScreamer", "ACD_TweedDeluxe", "ACD_CabSimTMS"],
            &[],
        );

        let mut hot = profile_with(0, 0.0);
        hot.spread_lu = GUITAR.spiky_spread_lu + 1.0;
        assert!(keys(&diagnose(
            &hot,
            Some(&clean),
            Instrument::Guitar,
            Some(&cohort)
        ))
        .contains(&"spiky"));
        // A drive block in the chain means the amp is already compressing it —
        // spiky is a clean-chain-only finding.
        assert!(!keys(&diagnose(
            &hot,
            Some(&driven),
            Instrument::Guitar,
            Some(&cohort)
        ))
        .contains(&"spiky"));

        let mut cold = profile_with(0, 0.0);
        cold.spread_lu = GUITAR.spiky_spread_lu - 1.0;
        assert!(!keys(&diagnose(
            &cold,
            Some(&clean),
            Instrument::Guitar,
            Some(&cohort)
        ))
        .contains(&"spiky"));

        // Without a graph we can't assert "clean" — never fires.
        assert!(!keys(&diagnose(&hot, None, Instrument::Guitar, Some(&cohort))).contains(&"spiky"));
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
        let cohort = flat_cohort();
        let mut scooped_lows = profile_with(0, -(BASS.buried_lows_db + 3.0));
        scooped_lows.tail_ratio_db = -40.0;
        let driven = chain(&["ACD_ModernBassOverdrive", "ACD_TweedDeluxe"], &[]);
        let got = diagnose(
            &scooped_lows,
            Some(&driven),
            Instrument::Bass,
            Some(&cohort),
        );
        assert!(keys(&got).contains(&"buried"));
        // Same profile on guitar, or without a drive → silent.
        let got = diagnose(
            &scooped_lows,
            Some(&driven),
            Instrument::Guitar,
            Some(&cohort),
        );
        assert!(!keys(&got).contains(&"buried"));
        let clean = chain(&["ACD_TweedDeluxe"], &[]);
        let got = diagnose(&scooped_lows, Some(&clean), Instrument::Bass, Some(&cohort));
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
        match &sc.rx[0].ops[0] {
            DoctorOp::SceneTrim {
                scene,
                target_delta_db,
            } => {
                assert_eq!(*scene, 1);
                assert!((target_delta_db - 2.0).abs() < f64::EPSILON);
            }
            other => panic!("expected SceneTrim, got {other:?}"),
        }
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
    fn scene_zero_worst_never_gets_wire_trim() {
        // The open loadScene(0) anomaly: a worst scene at wire index 0 gets a
        // verify-by-ear advisory, never a SceneTrim op.
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
        // The same jump on wire index 1 keeps the trim (control).
        let scenes = vec![(
            "Lead".to_string(),
            Some("FS1".to_string()),
            -14.0,
            Some(1u32),
        )];
        let sc = scene_consistency("Rhythm", -20.0, &scenes, Instrument::Guitar).unwrap();
        assert!(matches!(sc.rx[0].ops[0], DoctorOp::SceneTrim { .. }));
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
    fn scene_worst_keeps_trim_with_footswitch_rows_present() {
        // An FS row present but a SCENE is the worst: the SceneTrim branch
        // still fires and the FS row still appears in the delta table.
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
        assert!(matches!(
            sc.rx[0].ops[0],
            DoctorOp::SceneTrim { scene: 1, .. }
        ));
        assert!(sc.rows.iter().any(|r| r.name == "Boost"));
    }

    // ── per-instrument cohorts ──

    #[test]
    fn cohorts_partition_by_instrument() {
        let guitars: Vec<SoundProfile> = (0..MIN_COHORT).map(|_| profile_with(0, 0.0)).collect();
        let basses: Vec<SoundProfile> = (0..2).map(|_| profile_with(0, 6.0)).collect();
        let mut all: Vec<(Instrument, &SoundProfile)> =
            guitars.iter().map(|p| (Instrument::Guitar, p)).collect();
        all.extend(basses.iter().map(|p| (Instrument::Bass, p)));
        let cohorts = cohorts_by_instrument(&all);
        let guitar = cohorts.get(&Family::Guitar).cloned().flatten();
        assert!(guitar.is_some(), "guitar group reaches MIN_COHORT");
        // Under-minimum bass group stays absolute (present as key, but None).
        assert_eq!(cohorts.get(&Family::Bass), Some(&None));
        // BassVi absent from the run → no key at all.
        assert!(!cohorts.contains_key(&Family::BassVi));
        // The guitar median must not be dragged toward the hot bass lows.
        assert!((guitar.unwrap()[0] - flat_cohort()[0]).abs() < 1e-9);
    }

    // ── silent capture (#6) ──

    #[test]
    fn silent_capture_errors_instead_of_minus_inf() {
        let silence = vec![0.0f32; 96_000];
        let err = SoundProfile::from_capture(&silence, 48_000, 48_000, 0, Family::Guitar)
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
            assert_eq!(fam.idx_lows(), 0);
            assert_eq!(fam.idx_air(), 5);
        }
        // Bass VI: a 7th "Sub" band at index 0; the six semantic bands shift +1.
        assert_eq!(Family::BassVi.bands().len(), 7);
        assert_eq!(Family::BassVi.bands()[0], (30.0, 60.0));
        assert_eq!(Family::BassVi.labels()[0], "Sub");
        assert_eq!(Family::BassVi.idx_lows(), 1);
        assert_eq!(Family::BassVi.idx_low_mids(), 2);
        assert_eq!(Family::BassVi.idx_mids(), 3);
        assert_eq!(Family::BassVi.idx_high_mids(), 4);
        assert_eq!(Family::BassVi.idx_highs(), 5);
        assert_eq!(Family::BassVi.idx_air(), 6);
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
        };
        let diags = diagnose(&p, None, Family::BassVi, None);
        assert!(
            diags.iter().all(|d| !d.bands.contains(&0)),
            "no diagnosis may key on the Sub band (index 0): {:?}",
            keys(&diags)
        );
    }

    #[test]
    fn cohorts_never_mix_families() {
        // A full guitar cohort + a full bass-vi cohort pool SEPARATELY (they don't
        // even share a band count), each judged against its own median.
        let guitars: Vec<SoundProfile> = (0..MIN_COHORT).map(|_| profile_with(0, 0.0)).collect();
        let bassvis: Vec<SoundProfile> = (0..MIN_COHORT)
            .map(|_| SoundProfile {
                bands: vec![1.0; 7],
                integrated_lufs: -20.0,
                spread_lu: 0.0,
                tail_ratio_db: -40.0,
            })
            .collect();
        let mut all: Vec<(Family, &SoundProfile)> =
            guitars.iter().map(|p| (Family::Guitar, p)).collect();
        all.extend(bassvis.iter().map(|p| (Family::BassVi, p)));
        let cohorts = cohorts_by_instrument(&all);
        let g = cohorts.get(&Family::Guitar).cloned().flatten().unwrap();
        let bvi = cohorts.get(&Family::BassVi).cloned().flatten().unwrap();
        assert_eq!(g.len(), 6, "guitar median keeps the 6-band layout");
        assert_eq!(bvi.len(), 7, "bass-vi median keeps its 7-band layout");
    }

    // ── serde wire shapes ──

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
        // The marketing-screenshot presets, judged on the ABSOLUTE-fallback path
        // (cohort = None, as the 3-sound showcase run uses). Each mapped preset must
        // produce exactly its intended diagnosis pair; every other slot must be clear.
        // Guards docs/assets/doctor.png from silently reverting to "All clear" on a
        // threshold retune. (Together the three cover all six guitar diagnoses.)
        let diag_set = |idx: u32| {
            let mut got = keys(&diagnose(
                &showcase_profile(idx),
                None,
                Instrument::Guitar,
                None,
            ));
            got.sort_unstable();
            got
        };
        assert_eq!(diag_set(4), vec!["lost", "washed"]); // Scooped Verse
        assert_eq!(diag_set(11), vec!["boomy", "muddy"]); // Tweed Warm
        assert_eq!(diag_set(167), vec!["fizzy", "harsh"]); // Direct Acoustic
        assert!(diag_set(0).is_empty()); // any other preset → all clear
    }
}
