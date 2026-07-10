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

/// The six player-named analysis bands (Hz). Doctor-specific — the legacy
/// 4-band `spectrum::default_bands` stays untouched for the older commands.
/// 12 kHz top matches the practical capture ceiling `spectrum` already uses.
pub fn doctor_bands() -> [(f32, f32); 6] {
    [
        (60.0, 120.0),     // Lows
        (120.0, 400.0),    // Low-mids
        (400.0, 1000.0),   // Mids
        (1000.0, 3000.0),  // High-mids
        (3000.0, 6000.0),  // Highs
        (6000.0, 12000.0), // Air
    ]
}

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
    scene_delta_db: 3.0,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instrument {
    Guitar,
    Bass,
}

impl Instrument {
    pub fn thresholds(self) -> &'static Thresholds {
        match self {
            Instrument::Guitar => &GUITAR,
            Instrument::Bass => &BASS,
        }
    }
    /// Map a topology's `instrument` field ("guitar" | "bass").
    pub fn from_topology(instrument: &str) -> Instrument {
        if instrument.eq_ignore_ascii_case("bass") {
            Instrument::Bass
        } else {
            Instrument::Guitar
        }
    }
}

/// One captured sound's measurements — everything `diagnose` needs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundProfile {
    /// Mean Goertzel band power per [`doctor_bands`] band (raw power, not dB).
    pub bands: [f64; 6],
    pub integrated_lufs: f64,
    /// Post-stimulus tail RMS vs body RMS, in dB (see [`tail_energy_ratio`]).
    pub tail_ratio_db: f64,
}

impl SoundProfile {
    /// Build a profile from one captured mono signal: the 6 Doctor-band
    /// energies, integrated loudness, and the reverb-wash tail ratio. Shared by
    /// the `doctor_check` command and the `probe --doctor` calibration sweep.
    pub fn from_capture(
        samples: &[f32],
        rate: u32,
        stimulus_samples: usize,
    ) -> Result<SoundProfile, String> {
        let bands: [f64; 6] = crate::spectrum::band_energies(samples, rate as f32, &doctor_bands())
            .try_into()
            .map_err(|_| "band count".to_string())?;
        let integrated_lufs = crate::lufs::measure_mono(samples, rate)?.integrated_lufs;
        // A silent capture measures −inf — route the sound to the errors lane
        // (the leveller's sentinel philosophy) instead of poisoning the cohort
        // median and the scene deltas with non-finite numbers.
        if !integrated_lufs.is_finite() {
            return Err("no signal on USB 1/2 — the sound is silent".to_string());
        }
        Ok(SoundProfile {
            bands,
            integrated_lufs,
            tail_ratio_db: tail_energy_ratio(samples, rate, stimulus_samples),
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
        bands: db.map(|d| 10f64.powf(d / 10.0)),
        integrated_lufs: -18.0,
        tail_ratio_db: tail,
    }
}

fn to_db(p: f64) -> f64 {
    10.0 * p.max(1e-12).log10()
}

/// A sound's spectral "balance": each band's dB offset from the sound's own
/// mean band level. Level-invariant, so cohort comparison is about tone shape,
/// not loudness.
pub fn balance(bands: &[f64; 6]) -> [f64; 6] {
    let db: Vec<f64> = bands.iter().copied().map(to_db).collect();
    let mean = db.iter().sum::<f64>() / 6.0;
    let mut out = [0.0; 6];
    for (o, d) in out.iter_mut().zip(db) {
        *o = d - mean;
    }
    out
}

/// Per-band median of the cohort's balances — the "what this player's library
/// sounds like" reference `diagnose` judges deviations against.
pub fn cohort_median(profiles: &[&SoundProfile]) -> [f64; 6] {
    let mut med = [0.0; 6];
    if profiles.is_empty() {
        return med;
    }
    let balances: Vec<[f64; 6]> = profiles.iter().map(|p| balance(&p.bands)).collect();
    for (i, m) in med.iter_mut().enumerate() {
        let mut v: Vec<f64> = balances.iter().map(|b| b[i]).collect();
        v.sort_by(f64::total_cmp);
        *m = v[v.len() / 2];
    }
    med
}

/// Minimum cohort size for relative judging; below this `diagnose` falls back
/// to absolute neighbour-expectation heuristics.
// ponytail: cohort median, add a persisted rolling baseline only if
// single-sound runs prove unreliable in calibration.
pub const MIN_COHORT: usize = 4;

/// Per-instrument cohort medians `(guitar, bass)`: each instrument's sounds are
/// judged against their OWN library median — a bass preset judged against a
/// guitar cohort reads falsely boomy. `None` for an instrument whose group is
/// under [`MIN_COHORT`] (that group's sounds diagnose with the absolute
/// fallback).
pub fn cohorts_by_instrument(
    profiles: &[(Instrument, &SoundProfile)],
) -> (Option<[f64; 6]>, Option<[f64; 6]>) {
    let median_of = |inst: Instrument| {
        let refs: Vec<&SoundProfile> = profiles
            .iter()
            .filter(|(i, _)| *i == inst)
            .map(|(_, p)| *p)
            .collect();
        (refs.len() >= MIN_COHORT).then(|| cohort_median(&refs))
    };
    (median_of(Instrument::Guitar), median_of(Instrument::Bass))
}

/// Post-stimulus tail energy vs stimulus-body energy, in dB (≤ 0 in practice;
/// a dry sound decays fast → strongly negative; a drowning reverb/delay tail
/// keeps ringing → closer to 0). Returns −80 (a "silent tail" floor) when the
/// capture has no tail window.
pub fn tail_energy_ratio(samples: &[f32], _rate: u32, stimulus_samples: usize) -> f64 {
    if samples.len() <= stimulus_samples || stimulus_samples == 0 {
        return -80.0;
    }
    let rms = |s: &[f32]| -> f64 {
        if s.is_empty() {
            return 0.0;
        }
        (s.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>() / s.len() as f64).sqrt()
    };
    let body = rms(&samples[..stimulus_samples]);
    let tail = rms(&samples[stimulus_samples..]);
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
    /// Indices into [`doctor_bands`] that light up in the UI; empty = a
    /// time-domain finding (washed / buried).
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

const CAB_STANDALONE: &str = "ACD_CabSimTMS";
const EQ10_STEREO: &str = "ACD_TenBandEQStereo"; // never the Mono variant (absent from the product profile)
const HIGH_LOW_PASS: &str = "ACD_HighLowPass";
const COMPRESSOR: &str = "ACD_DynaComp"; // classic 2-knob comp, cheapest schema-verified pick

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
fn chain_preview(nodes: &[DoctorNode], template: &str, inserted: &str, at_front: bool) -> Value {
    let mut blocks: Vec<Value> = nodes
        .iter()
        .map(|n| serde_json::json!({ "model": n.model }))
        .collect();
    let added = serde_json::json!({ "model": inserted, "added": true });
    if at_front {
        blocks.insert(0, added);
    } else {
        blocks.push(added);
    }
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
        chain: Some(chain_preview(nodes, "after · +EQ", EQ10_STEREO, false)),
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

/// The compressor move (lost / buried). Comp-aware: an ACTIVE comp already in
/// the chain → advisory to raise its sustain; a BYPASSED one → advisory to
/// switch it back on; none → insert one in front.
fn comp_front(nodes: &[DoctorNode], facts: &GraphFacts) -> Option<Rx> {
    match facts.comp {
        Some(false) => {
            return Some(advisory(
                "Bring up the sustain on the compressor you already have",
                "Your chain already runs a compressor — raising its sustain evens out your picking without adding another block.",
            ));
        }
        Some(true) => {
            return Some(advisory(
                "Switch your compressor back on",
                "There's a compressor in the chain but it's switched off — turning it back on evens out your picking.",
            ));
        }
        None => {}
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
        chain: Some(chain_preview(nodes, "after · +COMP", COMPRESSOR, true)),
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
    instrument: Instrument,
    cohort: Option<&[f64; 6]>,
) -> Vec<Diag> {
    let t = instrument.thresholds();
    let bal = balance(&profile.bands);
    // Deviation per band: vs cohort median, or vs the sound's own neighbour
    // expectation (the band's dB above the mean of its two spectral
    // neighbours) when the cohort is too small to trust.
    let dev = |i: usize| -> f64 {
        match cohort {
            Some(med) => bal[i] - med[i],
            None => {
                let lo = if i == 0 { bal[1] } else { bal[i - 1] };
                let hi = if i == 5 { bal[4] } else { bal[i + 1] };
                bal[i] - (lo + hi) / 2.0
            }
        }
    };
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

    if dev(1) > t.muddy_db {
        push(
            "muddy",
            "Muddy",
            Sev::High,
            vec![1],
            format!("buildup around 250–350 Hz ({:+.1} dB)", dev(1)),
            "There's a buildup in the low-mids that stacks up with the bass player.",
        );
    }
    if dev(0) > t.boomy_db {
        push(
            "boomy",
            "Boomy",
            Sev::Med,
            vec![0],
            format!("excess energy below 100 Hz ({:+.1} dB)", dev(0)),
            "Too much deep low end — it booms and loses focus once you turn up.",
        );
    }
    if dev(3) > t.harsh_db {
        push(
            "harsh",
            "Harsh",
            Sev::High,
            vec![3],
            format!("spike around 1–3 kHz ({:+.1} dB)", dev(3)),
            "A sharp peak in the high-mids makes it harsh and tiring to listen to.",
        );
    }
    // Fizz is judged against the sound's own presence band, not the cohort —
    // see `Thresholds::fizzy_db` for the calibration rationale.
    if bal[5] - bal[4] > t.fizzy_db {
        push(
            "fizzy",
            "Fizzy",
            Sev::Med,
            vec![5],
            format!(
                "content above 6 kHz only {:.1} dB under the presence band",
                bal[4] - bal[5]
            ),
            "Fizzy, buzzy top end — the kind that sounds like radio static on the note tails.",
        );
    }
    if -dev(2) > t.lost_db {
        push(
            "lost",
            "Gets lost in the mix",
            Sev::High,
            vec![2],
            format!("mids scooped {:.1} dB around 800 Hz", -dev(2)),
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
    if instrument == Instrument::Bass
        && facts.as_ref().map(|f| f.has_drive) == Some(true)
        && -dev(0) > t.buried_lows_db
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
mod tests {
    use super::*;

    /// A "healthy" spectrum (flat with the natural ≥ 20 dB Air rolloff a cab
    /// gives) with one band offset by `db`. Air stays rolled off so the fizz
    /// rule (which fires on MISSING rolloff) never muddies the other rules'
    /// assertions.
    fn profile_with(band: usize, db: f64) -> SoundProfile {
        let mut bands = [1.0; 6];
        bands[5] = 10f64.powf(-20.0 / 10.0);
        bands[band] = 10f64.powf(db / 10.0);
        SoundProfile {
            bands,
            integrated_lufs: -20.0,
            tail_ratio_db: -40.0,
        }
    }

    /// Cohort median = the healthy baseline's own balance (what a library of
    /// healthy cab'd sounds medians out to), so `dev` isolates the offset a
    /// test injects.
    fn flat_cohort() -> [f64; 6] {
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
        let d = tail_energy_ratio(&dry, rate, body.len());
        let w = tail_energy_ratio(&wet, rate, body.len());
        assert!(w > d, "wet tail must read hotter ({w} vs {d})");
        assert!(w > -6.0 && d < -40.0);
    }

    #[test]
    fn tail_ratio_guards_short_capture() {
        assert_eq!(tail_energy_ratio(&[0.1; 100], 48_000, 100), -80.0);
        assert_eq!(tail_energy_ratio(&[0.1; 50], 48_000, 100), -80.0);
        assert_eq!(tail_energy_ratio(&[], 48_000, 0), -80.0);
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
        let (guitar, bass) = cohorts_by_instrument(&all);
        assert!(guitar.is_some(), "guitar group reaches MIN_COHORT");
        assert!(bass.is_none(), "under-minimum bass group stays absolute");
        // The guitar median must not be dragged toward the hot bass lows.
        assert!((guitar.unwrap()[0] - flat_cohort()[0]).abs() < 1e-9);
    }

    // ── silent capture (#6) ──

    #[test]
    fn silent_capture_errors_instead_of_minus_inf() {
        let silence = vec![0.0f32; 96_000];
        let err = SoundProfile::from_capture(&silence, 48_000, 48_000)
            .expect_err("silence must not produce a profile");
        assert!(err.contains("silent"), "{err}");
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
