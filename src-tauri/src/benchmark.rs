//! Held-out accuracy benchmarks for the Doctor and for leveling (#200). PURE:
//! no device, no audio, no filesystem — `probe_api/benchmark.rs` does the file
//! I/O and the `probe` arms call it.
//!
//! WHY: green tests prove state and flow contracts, not population accuracy. The
//! Doctor's [`doctor::GUITAR`] gates and its guitar target curve were both
//! derived from the same 25-preset factory sweep they would be judged on, so
//! their false-positive rate on sounds they have never seen is unknown. Two
//! tools here answer that without new captures where possible:
//!
//! 1. **Leave-one-out** ([`doctor_leave_one_out`]) over a factory sweep report
//!    (`probe --doctor-calib-factory` rows carry a `replay` profile). Every
//!    factory preset is treated as healthy, so any verdict on a held-out preset
//!    is a false positive. Three modes, each re-deriving more of the
//!    calibration WITHOUT the held-out preset before diagnosing it:
//!    - `targetOnly`: the target curve only (median shape, rounded to 0.5 dB),
//!      shipped gates — isolates the target's in-sample bias;
//!    - `maxPlusMargin`: target plus every population-derived gate at the
//!      healthy population's max + the R4 repeatability margin (the derivation
//!      `doctor::GUITAR`'s doc states);
//!    - `p75Consensus`: target plus the two-space consensus band gates at the
//!      population's 75th percentile (the derivation `doctor::Thresholds`'s
//!      consensus doc states), tilt and fizzy still max + margin.
//!
//!    The washed gate is ground truth (a verified 90%-wet reverb), the bass-only
//!    `buried` gate needs a graph, and the localized resonant/boxy gates came
//!    from a separate injection matrix, so no mode re-derives those.
//!
//! 2. **Labeled corpus scoring** ([`score_corpus`]): a held-out corpus of
//!    labeled Doctor captures and leveling results, each row tagged `tuning`
//!    or `validation`. Only `validation` rows are scored, so the data that set
//!    a threshold never grades it. Doctor: per-diagnosis precision, recall,
//!    false-positive and abstention rates. Leveling: the target-error
//!    distribution, false clamps, persistence mismatches and non-idempotent
//!    re-runs. Explicit acceptance [`Thresholds`] turn the metrics into a
//!    machine-readable pass/fail with named violations.
//!
//! A corpus without complete [`Provenance`] (hardware, firmware, stimulus,
//! capture policy, labeling) is refused, so a result can always be traced back
//! to what produced it. The format, the split rule and the simulator's limits
//! are in `notes/benchmarks.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::doctor::{self, Calibration, Diag, Family, SoundProfile, StimulusKind};
use crate::psd::SpectralPeak;

/// The R4 repeatability margin for the band rules (3σ ≈ 1.2 dB), as
/// `doctor::GUITAR`'s doc records it.
pub const BAND_MARGIN_DB: f64 = 1.2;
/// The R4 repeatability margin for the broadband tilt (3σ, σ ≈ 0.05 dB/oct).
pub const TILT_MARGIN_DB_PER_OCT: f64 = 0.15;
/// The consensus gates' percentile ("~p75 of the healthy factory population",
/// `doctor::Thresholds`'s doc).
pub const CONSENSUS_PERCENTILE: f64 = 0.75;

/// Every diagnosis key the engine can emit, in a FIXED order so reports are
/// byte-deterministic.
pub const RULE_KEYS: [&str; 12] = [
    "muddy", "boomy", "harsh", "fizzy", "lost", "thin", "dark", "bright", "washed", "buried",
    "resonant", "boxy",
];

// ─── replayable profiles ───────────────────────────────────────────────────

/// One localized spectral peak, as the sweep report records it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeakRecord {
    pub freq_hz: f64,
    pub height_db: f64,
    pub q: f64,
}

/// Everything the diagnosis engine reads from one capture, serialized so a
/// sweep can be re-diagnosed offline under a different [`Calibration`]. The
/// sweep reports write it per row (`replay`); a corpus row carries one too.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayProfile {
    /// Raw linear band powers (the family's layout).
    pub bands: Vec<f64>,
    /// The stimulus's raw linear band powers, when the capture recorded them
    /// (drives the stimulus-transfer anchor).
    #[serde(default)]
    pub stim_bands: Option<Vec<f64>>,
    /// The captured output's per-band coverage; `None` = every band covered.
    #[serde(default)]
    pub coverage: Option<Vec<bool>>,
    pub integrated_lufs: f64,
    pub spread_lu: f64,
    #[serde(default)]
    pub momentary_spread_lu: f64,
    pub tail_ratio_db: f64,
    #[serde(default)]
    pub air_flatness: f64,
    #[serde(default)]
    pub peaks: Vec<PeakRecord>,
}

impl ReplayProfile {
    /// Record a measured profile and its coverage.
    pub fn from_profile(profile: &SoundProfile, coverage: Option<&[bool]>) -> Self {
        ReplayProfile {
            bands: profile.bands.clone(),
            stim_bands: profile.stim_bands.clone(),
            coverage: coverage.map(<[bool]>::to_vec),
            integrated_lufs: profile.integrated_lufs,
            spread_lu: profile.spread_lu,
            momentary_spread_lu: profile.momentary_spread_lu,
            tail_ratio_db: profile.tail_ratio_db,
            air_flatness: profile.air_flatness,
            peaks: profile
                .peaks
                .iter()
                .map(|p| PeakRecord {
                    freq_hz: p.freq_hz,
                    height_db: p.height_db,
                    q: p.q,
                })
                .collect(),
        }
    }

    /// The engine-side profile this record replays.
    pub fn to_profile(&self) -> SoundProfile {
        SoundProfile {
            bands: self.bands.clone(),
            integrated_lufs: self.integrated_lufs,
            spread_lu: self.spread_lu,
            momentary_spread_lu: self.momentary_spread_lu,
            tail_ratio_db: self.tail_ratio_db,
            air_flatness: self.air_flatness,
            peaks: self
                .peaks
                .iter()
                .map(|p| SpectralPeak {
                    freq_hz: p.freq_hz,
                    height_db: p.height_db,
                    q: p.q,
                })
                .collect(),
            stim_bands: self.stim_bands.clone(),
        }
    }

    /// Checks the band vectors match `family`'s layout.
    pub fn validate(&self, family: Family) -> Result<(), String> {
        let n = family.bands().len();
        if self.bands.len() != n {
            return Err(format!("{} bands, the family has {n}", self.bands.len()));
        }
        if let Some(s) = &self.stim_bands {
            if s.len() != n {
                return Err(format!("{} stimulus bands, the family has {n}", s.len()));
            }
        }
        if let Some(c) = &self.coverage {
            if c.len() != n {
                return Err(format!("{} coverage flags, the family has {n}", c.len()));
            }
        }
        Ok(())
    }
}

fn diagnose(r: &ReplayProfile, family: Family, kind: StimulusKind, cal: &Calibration) -> Vec<Diag> {
    doctor::diagnose_calibrated(&r.to_profile(), family, kind, r.coverage.as_deref(), cal)
}

// ─── small statistics ──────────────────────────────────────────────────────

/// Linear-interpolated percentile of `xs` (`p` in 0..=1). `None` when empty.
pub fn percentile(xs: &[f64], p: f64) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    let mut s = xs.to_vec();
    s.sort_by(f64::total_cmp);
    let rank = p.clamp(0.0, 1.0) * (s.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    Some(s[lo] + (s[hi] - s[lo]) * (rank - lo as f64))
}

/// Upper end of the 95% Wilson score interval for `k` events in `n` trials —
/// with n = 25, zero observed false positives still allows a rate up to ≈13%,
/// which a bare `0/25` hides. `None` when `n` is 0.
pub fn wilson_upper(k: usize, n: usize) -> Option<f64> {
    if n == 0 {
        return None;
    }
    let z = 1.959_963_984_540_054_f64;
    let (k, n) = (k as f64, n as f64);
    let p = k / n;
    let denom = 1.0 + z * z / n;
    let centre = p + z * z / (2.0 * n);
    let spread = z * ((p * (1.0 - p) + z * z / (4.0 * n)) / n).sqrt();
    Some(((centre + spread) / denom).min(1.0))
}

fn ratio(k: usize, n: usize) -> Option<f64> {
    (n > 0).then(|| k as f64 / n as f64)
}

fn round_half(x: f64) -> f64 {
    (x * 2.0).round() / 2.0
}

// ─── calibration re-derivation ─────────────────────────────────────────────

/// How a re-derived calibration sets its population gates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GatePolicy {
    /// Keep the shipped gates; re-derive the target curve only.
    TargetOnly,
    /// Every population gate at max + its R4 margin.
    MaxPlusMargin,
    /// The consensus band gates (both spaces) at [`CONSENSUS_PERCENTILE`];
    /// tilt and fizzy at max + margin.
    Percentile75,
}

impl GatePolicy {
    pub const ALL: [GatePolicy; 3] = [
        GatePolicy::TargetOnly,
        GatePolicy::MaxPlusMargin,
        GatePolicy::Percentile75,
    ];

    pub fn id(self) -> &'static str {
        match self {
            GatePolicy::TargetOnly => "targetOnly",
            GatePolicy::MaxPlusMargin => "maxPlusMargin",
            GatePolicy::Percentile75 => "p75Consensus",
        }
    }
}

/// One profile's band shape in the space the target curve is authored in:
/// band dB, stimulus-anchored to the family's reference stimulus (a no-op for
/// the reference stimulus itself), minus its own mean.
fn target_shape(r: &ReplayProfile, family: Family) -> Vec<f64> {
    let bdb = doctor::band_db(&r.bands);
    let anchored: Vec<f64> = match (r.stim_bands.as_deref(), doctor::stim_ref(family)) {
        (Some(stim), Some(reference)) => bdb
            .iter()
            .zip(doctor::band_db(stim))
            .zip(reference)
            .map(|((d, s), &rf)| d - (s - rf))
            .collect(),
        _ => bdb,
    };
    let mean = anchored.iter().sum::<f64>() / anchored.len().max(1) as f64;
    anchored.into_iter().map(|d| d - mean).collect()
}

/// The target curve a population implies: the per-band median of its shapes,
/// rounded to 0.5 dB (how the shipped guitar curve was authored).
pub fn derive_target(rows: &[&ReplayProfile], family: Family) -> Vec<f64> {
    let shapes: Vec<Vec<f64>> = rows.iter().map(|r| target_shape(r, family)).collect();
    (0..family.bands().len())
        .map(|i| round_half(doctor::median(shapes.iter().map(|s| s[i]).collect())))
        .collect()
}

/// Per-row rule metrics under a given target — the values the gates are set on.
struct PopulationMetrics {
    /// (tilt-split local, centered) per consensus rule, keyed by rule.
    consensus: BTreeMap<&'static str, (Vec<f64>, Vec<f64>)>,
    fizzy: Vec<f64>,
    tilt_abs: Vec<f64>,
}

fn population_metrics(
    rows: &[&ReplayProfile],
    family: Family,
    target: &[f64],
) -> PopulationMetrics {
    let (lows, low_mids, mids, high_mids, highs, air) = family.semantic_bands();
    let mut consensus: BTreeMap<&'static str, (Vec<f64>, Vec<f64>)> = BTreeMap::new();
    let mut fizzy = Vec::new();
    let mut tilt_abs = Vec::new();
    for r in rows {
        let bdb = doctor::band_db(&r.bands);
        let dev = doctor::anchor_deviations(
            doctor::deviations_from(&bdb, target),
            r.stim_bands.as_deref(),
            family,
        );
        let (slope, locals) = doctor::tilt_split(&dev, family, r.coverage.as_deref());
        let centered = doctor::centered_deviations(&dev, family);
        let covered = |i: usize| r.coverage.as_ref().is_none_or(|c| c[i]);
        let mut push = |key: &'static str, band: usize, sign: f64| {
            if covered(band) {
                let e = consensus.entry(key).or_default();
                e.0.push(sign * locals[band]);
                e.1.push(sign * centered[band]);
            }
        };
        push("muddy", low_mids, 1.0);
        push("boomy", lows, 1.0);
        push("harsh", high_mids, 1.0);
        push("lost", mids, -1.0);
        push("thin", lows, -1.0);
        if covered(air) && covered(highs) {
            fizzy.push(bdb[air] - bdb[highs]);
        }
        if let Some(s) = slope {
            tilt_abs.push(s.abs());
        }
    }
    PopulationMetrics {
        consensus,
        fizzy,
        tilt_abs,
    }
}

/// Re-derive a calibration from a healthy population under `policy`. A gate
/// whose metric no row measured keeps its shipped value.
pub fn derive_calibration(
    rows: &[&ReplayProfile],
    family: Family,
    policy: GatePolicy,
) -> Calibration {
    let target = derive_target(rows, family);
    let mut t = *family.thresholds();
    if policy != GatePolicy::TargetOnly {
        let pm = population_metrics(rows, family, &target);
        let max_plus =
            |xs: &[f64], m: f64| xs.iter().copied().max_by(f64::total_cmp).map(|v| v + m);
        let band_gate = |xs: &[f64]| match policy {
            GatePolicy::Percentile75 => percentile(xs, CONSENSUS_PERCENTILE),
            _ => max_plus(xs, BAND_MARGIN_DB),
        };
        let set = |key: &str, tilt: &mut f64, centered: &mut f64| {
            if let Some((l, c)) = pm.consensus.get(key) {
                if let Some(g) = band_gate(l) {
                    *tilt = g;
                }
                if let Some(g) = band_gate(c) {
                    *centered = g;
                }
            }
        };
        set("muddy", &mut t.muddy_db, &mut t.muddy_centered_db);
        set("boomy", &mut t.boomy_db, &mut t.boomy_centered_db);
        set("harsh", &mut t.harsh_db, &mut t.harsh_centered_db);
        set("lost", &mut t.lost_db, &mut t.lost_centered_db);
        if family == Family::Guitar {
            set("thin", &mut t.thin_db, &mut t.thin_centered_db);
        }
        if let Some(g) = max_plus(&pm.fizzy, BAND_MARGIN_DB) {
            t.fizzy_db = g;
        }
        if let Some(g) = max_plus(&pm.tilt_abs, TILT_MARGIN_DB_PER_OCT) {
            t.tilt_db_per_oct = g;
        }
    }
    Calibration {
        target,
        thresholds: t,
    }
}

// ─── leave-one-out ─────────────────────────────────────────────────────────

/// False-positive tally for one rule over a healthy population.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FpRate {
    pub fires: usize,
    pub sounds: usize,
    pub rate: Option<f64>,
    /// 95% Wilson upper bound on the rate.
    pub rate_upper95: Option<f64>,
}

impl FpRate {
    fn new(fires: usize, sounds: usize) -> Self {
        FpRate {
            fires,
            sounds,
            rate: ratio(fires, sounds),
            rate_upper95: wilson_upper(fires, sounds),
        }
    }
}

/// One mode's result: per-rule and any-rule false-positive rates, plus which
/// held-out sound fired what.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LooMode {
    pub per_rule: BTreeMap<&'static str, FpRate>,
    /// Sounds with at least one verdict (per-sound false alarms compound
    /// across rules).
    pub any_rule: FpRate,
    /// Held-out sound id → the verdicts it drew.
    pub fired: BTreeMap<String, Vec<&'static str>>,
}

/// The full leave-one-out report.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LooReport {
    pub family: String,
    pub sounds: usize,
    /// The shipped calibration on the same sounds — what in-sample looks like.
    pub in_sample: LooMode,
    /// Keyed by [`GatePolicy::id`].
    pub modes: BTreeMap<&'static str, LooMode>,
}

fn tally(fired: BTreeMap<String, Vec<&'static str>>, sounds: usize) -> LooMode {
    let per_rule = RULE_KEYS
        .iter()
        .map(|&k| {
            let n = fired.values().filter(|v| v.contains(&k)).count();
            (k, FpRate::new(n, sounds))
        })
        .collect();
    let any = fired.values().filter(|v| !v.is_empty()).count();
    LooMode {
        per_rule,
        any_rule: FpRate::new(any, sounds),
        fired: fired.into_iter().filter(|(_, v)| !v.is_empty()).collect(),
    }
}

fn keys(diags: &[Diag]) -> Vec<&'static str> {
    diags.iter().map(|d| d.key).collect()
}

/// Leave-one-out false-positive estimate over a healthy population of
/// `(id, profile)` rows: for each row, re-derive the calibration from the
/// others under every [`GatePolicy`] and diagnose the held-out row with it.
/// Needs at least 3 rows (2 to derive from).
pub fn doctor_leave_one_out(
    rows: &[(String, ReplayProfile)],
    family: Family,
    kind: StimulusKind,
) -> Result<LooReport, String> {
    if rows.len() < 3 {
        return Err(format!(
            "leave-one-out needs at least 3 sounds, got {}",
            rows.len()
        ));
    }
    for (id, r) in rows {
        r.validate(family).map_err(|e| format!("sound {id}: {e}"))?;
    }
    let shipped = Calibration::shipped(family);
    let in_sample = rows
        .iter()
        .map(|(id, r)| (id.clone(), keys(&diagnose(r, family, kind, &shipped))))
        .collect();
    let mut modes = BTreeMap::new();
    for policy in GatePolicy::ALL {
        let mut fired = BTreeMap::new();
        for (i, (id, held_out)) in rows.iter().enumerate() {
            let others: Vec<&ReplayProfile> = rows
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, (_, r))| r)
                .collect();
            let cal = derive_calibration(&others, family, policy);
            fired.insert(id.clone(), keys(&diagnose(held_out, family, kind, &cal)));
        }
        modes.insert(policy.id(), tally(fired, rows.len()));
    }
    Ok(LooReport {
        family: family_id(family).to_string(),
        sounds: rows.len(),
        in_sample: tally(in_sample, rows.len()),
        modes,
    })
}

fn family_id(family: Family) -> &'static str {
    match family {
        Family::Guitar => "guitar",
        Family::Bass => "bass",
        Family::BassVi => "bass-vi",
    }
}

// ─── labeled corpus ────────────────────────────────────────────────────────

/// Where a corpus came from. Every field is required: a result that cannot be
/// traced to its hardware, firmware, stimulus, capture policy and labeling is
/// not evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    /// The unit (model and an identifier for the specific unit).
    pub hardware: String,
    pub firmware: String,
    /// Stimulus file and its content hash (the sweep reports' `stimulus.hash`).
    pub stimulus: String,
    pub stimulus_hash: String,
    /// Capture window, engage shape, pickup/DI variant, and so on.
    pub capture_policy: String,
    /// Who labeled the rows, and how (by ear, by injected defect, …).
    pub labeling: String,
}

impl Provenance {
    fn check(&self) -> Result<(), String> {
        let fields = [
            ("hardware", &self.hardware),
            ("firmware", &self.firmware),
            ("stimulus", &self.stimulus),
            ("stimulusHash", &self.stimulus_hash),
            ("capturePolicy", &self.capture_policy),
            ("labeling", &self.labeling),
        ];
        let missing: Vec<&str> = fields
            .iter()
            .filter(|(_, v)| v.trim().is_empty())
            .map(|(k, _)| *k)
            .collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "provenance is incomplete: {} empty",
                missing.join(", ")
            ))
        }
    }
}

/// Which side of the split a row is on. Only `validation` rows are scored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Split {
    Tuning,
    Validation,
}

/// One labeled Doctor capture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorRow {
    pub id: String,
    pub split: Split,
    /// The diagnoses this sound GENUINELY has; every other rule is a negative.
    pub labels: Vec<String>,
    pub replay: ReplayProfile,
}

/// One leveling result. Accepts the validate-log's snake_case keys
/// (`validate_log.rs`) so its rows only need `measured_lufs` and `split` added.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelingRow {
    #[serde(alias = "label")]
    pub id: String,
    pub split: Split,
    #[serde(alias = "target_lufs")]
    pub target_lufs: f64,
    /// The independent (ffmpeg) read of the saved state; `None` = unmeasured.
    #[serde(default, alias = "measured_lufs")]
    pub measured_lufs: Option<f64>,
    #[serde(default)]
    pub clamped: bool,
    /// Ground truth: whether the target was really out of reach. `None` =
    /// unlabeled, so the row can't count a false clamp.
    #[serde(default, alias = "expect_clamp")]
    pub expect_clamp: Option<bool>,
    #[serde(default, alias = "persist_mismatch")]
    pub persist_mismatch: Option<bool>,
    /// Writes an immediate re-run of the same target made (0 = idempotent).
    #[serde(default, alias = "rerun_writes")]
    pub rerun_writes: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Corpus {
    pub provenance: Provenance,
    pub family: String,
    /// `"synthetic"` or `"capture"` (picks the fizzy flatness gate).
    #[serde(default = "default_stimulus_kind")]
    pub stimulus_kind: String,
    #[serde(default)]
    pub doctor: Vec<DoctorRow>,
    #[serde(default)]
    pub leveling: Vec<LevelingRow>,
}

fn default_stimulus_kind() -> String {
    "synthetic".to_string()
}

/// Per-diagnosis confusion counts and rates over the validation rows.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleScore {
    pub tp: usize,
    pub fp: usize,
    #[serde(rename = "fn")]
    pub fn_: usize,
    pub tn: usize,
    /// Rows where the rule could not judge (its band was not excited).
    pub abstained: usize,
    pub precision: Option<f64>,
    pub recall: Option<f64>,
    pub fp_rate: Option<f64>,
    pub abstention_rate: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorScore {
    pub sounds: usize,
    pub per_rule: BTreeMap<&'static str, RuleScore>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelingScore {
    pub rows: usize,
    /// Rows with an independent read and no clamp — the error distribution's n.
    pub measured: usize,
    pub unmeasured: usize,
    pub median_abs_error_lu: Option<f64>,
    pub p90_abs_error_lu: Option<f64>,
    pub max_abs_error_lu: Option<f64>,
    pub mean_signed_error_lu: Option<f64>,
    /// Clamped although the label says the target was reachable.
    pub false_clamps: usize,
    pub persist_mismatches: usize,
    /// Re-runs that wrote again (should be none).
    pub non_idempotent_reruns: usize,
}

/// Acceptance thresholds. Every field is optional; an absent one is not
/// checked. Set them from a measured baseline (`notes/benchmarks.md`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Thresholds {
    #[serde(default)]
    pub doctor: BTreeMap<String, RuleThresholds>,
    #[serde(default)]
    pub leveling: LevelingThresholds,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuleThresholds {
    pub min_precision: Option<f64>,
    pub min_recall: Option<f64>,
    pub max_fp_rate: Option<f64>,
    pub max_abstention_rate: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LevelingThresholds {
    pub max_p90_abs_error_lu: Option<f64>,
    pub max_abs_error_lu: Option<f64>,
    pub max_false_clamps: Option<usize>,
    pub max_persist_mismatches: Option<usize>,
    pub max_non_idempotent_reruns: Option<usize>,
    pub max_unmeasured: Option<usize>,
}

/// The machine-readable benchmark result.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkResult {
    pub provenance: Provenance,
    pub family: String,
    /// Rows skipped because they are tuning data.
    pub tuning_rows_skipped: usize,
    pub doctor: Option<DoctorScore>,
    pub leveling: Option<LevelingScore>,
    pub violations: Vec<String>,
    pub pass: bool,
}

fn parse_family(id: &str) -> Result<Family, String> {
    match id {
        "guitar" => Ok(Family::Guitar),
        "bass" => Ok(Family::Bass),
        "bass-vi" => Ok(Family::BassVi),
        other => Err(format!(
            "unknown family {other:?} (guitar | bass | bass-vi)"
        )),
    }
}

fn parse_kind(id: &str) -> Result<StimulusKind, String> {
    match id {
        "synthetic" => Ok(StimulusKind::Synthetic),
        "capture" => Ok(StimulusKind::Capture),
        other => Err(format!(
            "unknown stimulusKind {other:?} (synthetic | capture)"
        )),
    }
}

/// Whether `rule` can judge `r` at all, by the engine's own structural gates
/// (`doctor::rule_judgeable`). A replay carries no graph, so `buried` always
/// abstains here.
fn rule_can_judge(rule: &str, r: &ReplayProfile, family: Family) -> bool {
    doctor::rule_judgeable(rule, family, r.coverage.as_deref(), false)
}

fn score_doctor(
    rows: &[&DoctorRow],
    family: Family,
    kind: StimulusKind,
) -> Result<DoctorScore, String> {
    for r in rows {
        r.replay
            .validate(family)
            .map_err(|e| format!("doctor row {}: {e}", r.id))?;
        if let Some(bad) = r.labels.iter().find(|l| !RULE_KEYS.contains(&l.as_str())) {
            return Err(format!("doctor row {}: unknown label {bad:?}", r.id));
        }
    }
    let shipped = Calibration::shipped(family);
    let verdicts: Vec<Vec<&'static str>> = rows
        .iter()
        .map(|r| keys(&diagnose(&r.replay, family, kind, &shipped)))
        .collect();
    let per_rule = RULE_KEYS
        .iter()
        .map(|&rule| {
            let (mut tp, mut fp, mut fn_, mut tn, mut abstained) = (0, 0, 0, 0, 0);
            for (r, v) in rows.iter().zip(&verdicts) {
                if !rule_can_judge(rule, &r.replay, family) {
                    abstained += 1;
                    continue;
                }
                let truth = r.labels.iter().any(|l| l == rule);
                match (truth, v.contains(&rule)) {
                    (true, true) => tp += 1,
                    (false, true) => fp += 1,
                    (true, false) => fn_ += 1,
                    (false, false) => tn += 1,
                }
            }
            (
                rule,
                RuleScore {
                    tp,
                    fp,
                    fn_,
                    tn,
                    abstained,
                    precision: ratio(tp, tp + fp),
                    recall: ratio(tp, tp + fn_),
                    fp_rate: ratio(fp, fp + tn),
                    abstention_rate: ratio(abstained, rows.len()),
                },
            )
        })
        .collect();
    Ok(DoctorScore {
        sounds: rows.len(),
        per_rule,
    })
}

fn score_leveling(rows: &[&LevelingRow]) -> LevelingScore {
    let mut errors = Vec::new();
    let mut unmeasured = 0;
    for r in rows {
        match r.measured_lufs {
            // A clamped row never reached its target by design — no error.
            Some(m) if !r.clamped => errors.push(m - r.target_lufs),
            Some(_) => {}
            None => unmeasured += 1,
        }
    }
    let abs: Vec<f64> = errors.iter().map(|e| e.abs()).collect();
    LevelingScore {
        rows: rows.len(),
        measured: errors.len(),
        unmeasured,
        median_abs_error_lu: percentile(&abs, 0.5),
        p90_abs_error_lu: percentile(&abs, 0.9),
        max_abs_error_lu: percentile(&abs, 1.0),
        mean_signed_error_lu: (!errors.is_empty())
            .then(|| errors.iter().sum::<f64>() / errors.len() as f64),
        false_clamps: rows
            .iter()
            .filter(|r| r.clamped && r.expect_clamp == Some(false))
            .count(),
        persist_mismatches: rows
            .iter()
            .filter(|r| r.persist_mismatch == Some(true))
            .count(),
        non_idempotent_reruns: rows
            .iter()
            .filter(|r| r.rerun_writes.is_some_and(|w| w > 0))
            .count(),
    }
}

/// `value` must be ≤ `max` (or ≥ `min` when `at_least`). An unmeasurable value
/// against a set threshold is a violation too: no evidence is not a pass.
fn check(
    out: &mut Vec<String>,
    name: &str,
    value: Option<f64>,
    bound: Option<f64>,
    at_least: bool,
) {
    let Some(b) = bound else { return };
    match value {
        None => out.push(format!("{name}: not measurable (threshold {b})")),
        Some(v) if at_least && v < b => out.push(format!("{name}: {v:.4} < {b}")),
        Some(v) if !at_least && v > b => out.push(format!("{name}: {v:.4} > {b}")),
        Some(_) => {}
    }
}

/// Score a corpus's validation rows and check them against `thresholds`.
pub fn score_corpus(corpus: &Corpus, thresholds: &Thresholds) -> Result<BenchmarkResult, String> {
    corpus.provenance.check()?;
    let family = parse_family(&corpus.family)?;
    let kind = parse_kind(&corpus.stimulus_kind)?;
    let doctor_rows: Vec<&DoctorRow> = corpus
        .doctor
        .iter()
        .filter(|r| r.split == Split::Validation)
        .collect();
    let level_rows: Vec<&LevelingRow> = corpus
        .leveling
        .iter()
        .filter(|r| r.split == Split::Validation)
        .collect();
    if doctor_rows.is_empty() && level_rows.is_empty() {
        return Err("the corpus has no validation rows to score".to_string());
    }
    let tuning_rows_skipped =
        corpus.doctor.len() + corpus.leveling.len() - doctor_rows.len() - level_rows.len();
    let doctor = (!doctor_rows.is_empty())
        .then(|| score_doctor(&doctor_rows, family, kind))
        .transpose()?;
    let leveling = (!level_rows.is_empty()).then(|| score_leveling(&level_rows));

    let mut violations = Vec::new();
    for (rule, t) in &thresholds.doctor {
        let Some(s) = doctor.as_ref().and_then(|d| d.per_rule.get(rule.as_str())) else {
            violations.push(format!("doctor.{rule}: no validation rows scored it"));
            continue;
        };
        check(
            &mut violations,
            &format!("doctor.{rule}.precision"),
            s.precision,
            t.min_precision,
            true,
        );
        check(
            &mut violations,
            &format!("doctor.{rule}.recall"),
            s.recall,
            t.min_recall,
            true,
        );
        check(
            &mut violations,
            &format!("doctor.{rule}.fpRate"),
            s.fp_rate,
            t.max_fp_rate,
            false,
        );
        check(
            &mut violations,
            &format!("doctor.{rule}.abstentionRate"),
            s.abstention_rate,
            t.max_abstention_rate,
            false,
        );
    }
    let lt = &thresholds.leveling;
    let lv = leveling.as_ref();
    let count = |f: fn(&LevelingScore) -> usize| lv.map(|l| f(l) as f64);
    check(
        &mut violations,
        "leveling.p90AbsErrorLu",
        lv.and_then(|l| l.p90_abs_error_lu),
        lt.max_p90_abs_error_lu,
        false,
    );
    check(
        &mut violations,
        "leveling.maxAbsErrorLu",
        lv.and_then(|l| l.max_abs_error_lu),
        lt.max_abs_error_lu,
        false,
    );
    check(
        &mut violations,
        "leveling.falseClamps",
        count(|l| l.false_clamps),
        lt.max_false_clamps.map(|v| v as f64),
        false,
    );
    check(
        &mut violations,
        "leveling.persistMismatches",
        count(|l| l.persist_mismatches),
        lt.max_persist_mismatches.map(|v| v as f64),
        false,
    );
    check(
        &mut violations,
        "leveling.nonIdempotentReruns",
        count(|l| l.non_idempotent_reruns),
        lt.max_non_idempotent_reruns.map(|v| v as f64),
        false,
    );
    check(
        &mut violations,
        "leveling.unmeasured",
        count(|l| l.unmeasured),
        lt.max_unmeasured.map(|v| v as f64),
        false,
    );

    Ok(BenchmarkResult {
        provenance: corpus.provenance.clone(),
        family: corpus.family.clone(),
        tuning_rows_skipped,
        doctor,
        leveling,
        pass: violations.is_empty(),
        violations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A guitar-layout replay whose band dB equal `shape_db` (+ a level offset).
    fn replay(shape_db: [f64; 6], level_db: f64) -> ReplayProfile {
        ReplayProfile {
            bands: shape_db
                .iter()
                .map(|d| 10f64.powf((d + level_db) / 10.0))
                .collect(),
            stim_bands: None,
            coverage: None,
            integrated_lufs: -20.0,
            spread_lu: 3.0,
            momentary_spread_lu: 3.0,
            tail_ratio_db: -30.0,
            air_flatness: 0.1,
            peaks: vec![],
        }
    }

    /// The shipped target shape with a small, deterministic per-row wobble.
    fn healthy_population(n: usize) -> Vec<(String, ReplayProfile)> {
        let t = doctor::target_curve(Family::Guitar);
        (0..n)
            .map(|i| {
                let w = |k: usize| (((i * 7 + k * 3) % 5) as f64 - 2.0) * 0.4;
                let shape = [
                    t[0] + w(0),
                    t[1] + w(1),
                    t[2] + w(2),
                    t[3] + w(3),
                    t[4] + w(4),
                    t[5] + w(5),
                ];
                (format!("s{i}"), replay(shape, -40.0 + i as f64))
            })
            .collect()
    }

    #[test]
    fn replay_round_trips_through_the_engine_profile() {
        let mut r = replay([1.0, 2.0, 3.0, 4.0, 5.0, 6.0], -30.0);
        r.peaks = vec![PeakRecord {
            freq_hz: 2600.0,
            height_db: 15.0,
            q: 8.0,
        }];
        r.stim_bands = Some(vec![1e-4; 6]);
        let back = ReplayProfile::from_profile(&r.to_profile(), None);
        assert_eq!(back, r);
        // serde_json's default float parse may round the last ulp.
        let json = serde_json::to_string(&r).unwrap();
        let parsed: ReplayProfile = serde_json::from_str(&json).unwrap();
        for (a, b) in parsed.bands.iter().zip(&r.bands) {
            assert!((a - b).abs() <= b * 1e-12, "{a} vs {b}");
        }
        assert_eq!(parsed.peaks, r.peaks);
        assert_eq!(parsed.stim_bands, r.stim_bands);
    }

    #[test]
    fn shipped_calibration_matches_diagnose_kind() {
        // A scooped, boomy sound fires something under the shipped table; the
        // calibrated entry point must agree with the production one exactly.
        let r = replay([5.0, -2.0, -15.0, 13.0, 13.5, -14.5], -30.0);
        let p = r.to_profile();
        let prod = keys(&doctor::diagnose_kind(
            &p,
            None,
            Family::Guitar,
            StimulusKind::Synthetic,
            None,
            doctor::PlaybackOffsets::NONE,
        ));
        let cal = keys(&diagnose(
            &r,
            Family::Guitar,
            StimulusKind::Synthetic,
            &Calibration::shipped(Family::Guitar),
        ));
        assert!(!prod.is_empty(), "fixture should fire");
        assert_eq!(prod, cal);
    }

    #[test]
    fn derived_target_is_the_rounded_median_shape_and_level_invariant() {
        let pop = healthy_population(9);
        let refs: Vec<&ReplayProfile> = pop.iter().map(|(_, r)| r).collect();
        let target = derive_target(&refs, Family::Guitar);
        // The wobble is symmetric around the shipped curve, whose mean is −0.
        for (got, want) in target.iter().zip(doctor::target_curve(Family::Guitar)) {
            assert!((got - want).abs() <= 0.5, "{target:?}");
            assert_eq!(*got, round_half(*got));
        }
    }

    #[test]
    fn max_plus_margin_gates_never_fire_inside_the_population() {
        // Every sound sits inside the healthy population's max, so a gate at
        // max + margin derived WITH it cannot fire on it.
        let pop = healthy_population(12);
        let refs: Vec<&ReplayProfile> = pop.iter().map(|(_, r)| r).collect();
        let cal = derive_calibration(&refs, Family::Guitar, GatePolicy::MaxPlusMargin);
        for (_, r) in &pop {
            let v = diagnose(r, Family::Guitar, StimulusKind::Synthetic, &cal);
            assert!(v.is_empty(), "{:?}", keys(&v));
        }
    }

    #[test]
    fn leave_one_out_flags_the_outlier_and_only_it() {
        let mut pop = healthy_population(10);
        let t = doctor::target_curve(Family::Guitar);
        // A heavy low-mid hump: muddy by any population's standard.
        pop.push((
            "outlier".to_string(),
            replay([t[0], t[1] + 12.0, t[2], t[3], t[4], t[5]], -30.0),
        ));
        let rep = doctor_leave_one_out(&pop, Family::Guitar, StimulusKind::Synthetic).unwrap();
        assert_eq!(rep.sounds, 11);
        let m = &rep.modes["maxPlusMargin"];
        assert!(
            m.fired.get("outlier").is_some_and(|v| v.contains(&"muddy")),
            "{m:?}"
        );
        assert_eq!(m.fired.len(), 1, "{:?}", m.fired);
        assert_eq!(m.per_rule["muddy"].fires, 1);
        assert_eq!(m.any_rule.fires, 1);
        assert_eq!(rep.modes.len(), 3);
    }

    #[test]
    fn leave_one_out_refuses_tiny_or_malformed_populations() {
        let pop = healthy_population(2);
        assert!(doctor_leave_one_out(&pop, Family::Guitar, StimulusKind::Synthetic).is_err());
        let mut pop = healthy_population(4);
        pop[1].1.bands.pop();
        let e = doctor_leave_one_out(&pop, Family::Guitar, StimulusKind::Synthetic).unwrap_err();
        assert!(e.contains("s1"), "{e}");
    }

    #[test]
    fn wilson_upper_bound() {
        // 0 of 25 still allows ≈13.3%.
        let u = wilson_upper(0, 25).unwrap();
        assert!((u - 0.1332).abs() < 1e-3, "{u}");
        assert!(wilson_upper(0, 0).is_none());
        assert_eq!(wilson_upper(5, 5), Some(1.0));
    }

    #[test]
    fn percentile_interpolates() {
        assert_eq!(percentile(&[3.0, 1.0, 2.0, 4.0, 5.0], 0.5), Some(3.0));
        assert_eq!(percentile(&[0.0, 10.0], 0.75), Some(7.5));
        assert_eq!(percentile(&[], 0.5), None);
    }

    fn provenance() -> Provenance {
        Provenance {
            hardware: "Tone Master Pro #1".into(),
            firmware: "1.8.58".into(),
            stimulus: "guitar-humbucker.wav".into(),
            stimulus_hash: "abc".into(),
            capture_policy: "doctor window, fresh connection per engage".into(),
            labeling: "by ear, two listeners".into(),
        }
    }

    fn corpus() -> Corpus {
        let t = doctor::target_curve(Family::Guitar);
        let muddy = replay([t[0], t[1] + 12.0, t[2], t[3], t[4], t[5]], -30.0);
        let clean = replay([t[0], t[1], t[2], t[3], t[4], t[5]], -30.0);
        let row = |id: &str, split, labels: &[&str], replay: &ReplayProfile| DoctorRow {
            id: id.into(),
            split,
            labels: labels.iter().map(|s| s.to_string()).collect(),
            replay: replay.clone(),
        };
        let lvl =
            |id: &str, split, measured: Option<f64>, clamped, expect, persist, rerun| LevelingRow {
                id: id.into(),
                split,
                target_lufs: -17.0,
                measured_lufs: measured,
                clamped,
                expect_clamp: expect,
                persist_mismatch: persist,
                rerun_writes: rerun,
            };
        Corpus {
            provenance: provenance(),
            family: "guitar".into(),
            stimulus_kind: "synthetic".into(),
            doctor: vec![
                row("a", Split::Validation, &["muddy"], &muddy),
                row("b", Split::Validation, &[], &clean),
                // Labeled muddy but measures clean: a miss.
                row("c", Split::Validation, &["muddy"], &clean),
                // Tuning data is never scored.
                row("d", Split::Tuning, &[], &muddy),
            ],
            leveling: vec![
                lvl(
                    "l1",
                    Split::Validation,
                    Some(-17.2),
                    false,
                    None,
                    Some(false),
                    Some(0),
                ),
                lvl(
                    "l2",
                    Split::Validation,
                    Some(-16.5),
                    false,
                    None,
                    None,
                    None,
                ),
                lvl(
                    "l3",
                    Split::Validation,
                    Some(-25.0),
                    true,
                    Some(false),
                    None,
                    None,
                ),
                lvl(
                    "l4",
                    Split::Validation,
                    None,
                    false,
                    None,
                    Some(true),
                    Some(2),
                ),
                lvl("l5", Split::Tuning, Some(-30.0), false, None, None, None),
            ],
        }
    }

    #[test]
    fn corpus_scores_only_validation_rows() {
        let r = score_corpus(&corpus(), &Thresholds::default()).unwrap();
        assert_eq!(r.tuning_rows_skipped, 2);
        assert!(r.pass);
        let d = r.doctor.unwrap();
        assert_eq!(d.sounds, 3);
        let m = &d.per_rule["muddy"];
        assert_eq!((m.tp, m.fp, m.fn_, m.tn, m.abstained), (1, 0, 1, 1, 0));
        assert_eq!(m.precision, Some(1.0));
        assert_eq!(m.recall, Some(0.5));
        assert_eq!(m.fp_rate, Some(0.0));
        let l = r.leveling.unwrap();
        assert_eq!((l.rows, l.measured, l.unmeasured), (4, 2, 1));
        assert!((l.max_abs_error_lu.unwrap() - 0.5).abs() < 1e-9);
        assert!((l.mean_signed_error_lu.unwrap() - 0.15).abs() < 1e-9);
        assert_eq!(l.false_clamps, 1);
        assert_eq!(l.persist_mismatches, 1);
        assert_eq!(l.non_idempotent_reruns, 1);
    }

    #[test]
    fn thresholds_name_every_violation_and_unmeasurable_is_not_a_pass() {
        let mut t = Thresholds::default();
        t.doctor.insert(
            "muddy".into(),
            RuleThresholds {
                min_recall: Some(0.9),
                max_fp_rate: Some(0.1),
                ..Default::default()
            },
        );
        // No validation row ever fires or is labeled boxy: precision is undefined.
        t.doctor.insert(
            "boxy".into(),
            RuleThresholds {
                min_precision: Some(0.5),
                ..Default::default()
            },
        );
        t.leveling = LevelingThresholds {
            max_abs_error_lu: Some(0.3),
            max_false_clamps: Some(0),
            max_persist_mismatches: Some(0),
            max_non_idempotent_reruns: Some(0),
            max_unmeasured: Some(0),
            ..Default::default()
        };
        let r = score_corpus(&corpus(), &t).unwrap();
        assert!(!r.pass);
        let v = r.violations.join("\n");
        for name in [
            "doctor.muddy.recall",
            "doctor.boxy.precision: not measurable",
            "leveling.maxAbsErrorLu",
            "leveling.falseClamps",
            "leveling.persistMismatches",
            "leveling.nonIdempotentReruns",
            "leveling.unmeasured",
        ] {
            assert!(v.contains(name), "missing {name} in:\n{v}");
        }
        assert!(!v.contains("doctor.muddy.fpRate"), "{v}");
    }

    #[test]
    fn corpus_refuses_missing_provenance_unknown_labels_and_no_validation_rows() {
        let mut c = corpus();
        c.provenance.firmware = " ".into();
        let e = score_corpus(&c, &Thresholds::default()).unwrap_err();
        assert!(e.contains("firmware"), "{e}");

        let mut c = corpus();
        c.doctor[0].labels = vec!["mudy".into()];
        let e = score_corpus(&c, &Thresholds::default()).unwrap_err();
        assert!(e.contains("mudy"), "{e}");

        let mut c = corpus();
        c.doctor.iter_mut().for_each(|r| r.split = Split::Tuning);
        c.leveling.iter_mut().for_each(|r| r.split = Split::Tuning);
        assert!(score_corpus(&c, &Thresholds::default()).is_err());
    }

    #[test]
    fn uncovered_band_counts_as_abstention_not_a_negative() {
        let mut c = corpus();
        // Row b's low-mids were never excited: muddy can't judge it.
        c.doctor[1].replay.coverage = Some(vec![true, false, true, true, true, true]);
        let r = score_corpus(&c, &Thresholds::default()).unwrap();
        let m = &r.doctor.unwrap().per_rule["muddy"];
        assert_eq!((m.tn, m.abstained), (0, 1));
        assert!((m.abstention_rate.unwrap() - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn leveling_rows_accept_validate_log_keys() {
        let row: LevelingRow = serde_json::from_str(
            r#"{"label":"scene:slot404:scene2","split":"validation","target_lufs":-17.0,
                "measured_lufs":-17.1,"clamped":false,"persist_mismatch":null}"#,
        )
        .unwrap();
        assert_eq!(row.id, "scene:slot404:scene2");
        assert_eq!(row.measured_lufs, Some(-17.1));
    }

    #[test]
    fn a_misspelled_threshold_key_is_refused_not_silently_unset() {
        for bad in [
            r#"{"doctor":{"muddy":{"max_fp_rate":0.05}}}"#,
            r#"{"leveling":{"maxP90AbsErrorLU":0.5}}"#,
            r#"{"levelling":{}}"#,
        ] {
            assert!(serde_json::from_str::<Thresholds>(bad).is_err(), "{bad}");
        }
        let ok: Thresholds =
            serde_json::from_str(r#"{"doctor":{"muddy":{"maxFpRate":0.05}}}"#).unwrap();
        assert_eq!(ok.doctor["muddy"].max_fp_rate, Some(0.05));
    }

    #[test]
    fn judgeability_mirrors_the_engine_gates() {
        let g = Family::Guitar;
        let none: [bool; 6] = [false; 6];
        assert!(doctor::rule_judgeable("thin", g, None, false));
        assert!(!doctor::rule_judgeable("thin", Family::Bass, None, false));
        assert!(!doctor::rule_judgeable("buried", Family::Bass, None, false));
        assert!(doctor::rule_judgeable("buried", Family::Bass, None, true));
        assert!(!doctor::rule_judgeable("buried", g, None, true));
        for rule in ["boxy", "resonant", "thin", "dark", "muddy"] {
            assert!(
                !doctor::rule_judgeable(rule, g, Some(&none), false),
                "{rule}"
            );
        }
        assert!(doctor::rule_judgeable("washed", g, Some(&none), false));
    }

    /// Drift guard: across shapes, coverage masks and localized peaks, every
    /// verdict the engine returns is one `rule_judgeable` admits.
    #[test]
    fn no_verdict_fires_on_a_rule_counted_as_abstaining() {
        let t = doctor::target_curve(Family::Guitar);
        let shapes = [
            [12.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [-12.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 12.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, -12.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 12.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 0.0, -6.0, 12.0],
            [8.0, 4.0, 0.0, -4.0, -8.0, -12.0],
            [-8.0, -4.0, 0.0, 4.0, 8.0, 12.0],
        ];
        let peaks = vec![
            PeakRecord {
                freq_hz: 400.0,
                height_db: 20.0,
                q: 6.0,
            },
            PeakRecord {
                freq_hz: 2600.0,
                height_db: 20.0,
                q: 8.0,
            },
        ];
        let kind = StimulusKind::Synthetic;
        let shipped = Calibration::shipped(Family::Guitar);
        let mut fired = std::collections::BTreeSet::new();
        for shape in shapes {
            let mut r = replay(
                [
                    t[0] + shape[0],
                    t[1] + shape[1],
                    t[2] + shape[2],
                    t[3] + shape[3],
                    t[4] + shape[4],
                    t[5] + shape[5],
                ],
                -30.0,
            );
            r.peaks = peaks.clone();
            r.tail_ratio_db = -5.0;
            for mask in 0u8..64 {
                r.coverage = Some((0..6).map(|i| mask & (1 << i) != 0).collect());
                for d in diagnose(&r, Family::Guitar, kind, &shipped) {
                    fired.insert(d.key);
                    assert!(
                        rule_can_judge(d.key, &r, Family::Guitar),
                        "{} fired on mask {mask:06b}, shape {shape:?}",
                        d.key
                    );
                }
            }
        }
        // Not vacuous: the gated rules really fire somewhere in the sweep.
        for rule in [
            "boxy", "resonant", "thin", "dark", "bright", "muddy", "boomy",
        ] {
            assert!(fired.contains(rule), "{rule} never fired: {fired:?}");
        }
    }

    #[test]
    fn a_graphless_buried_label_abstains_rather_than_counting_a_miss() {
        let mut c = corpus();
        c.doctor[0].labels.push("buried".into());
        let r = score_corpus(&c, &Thresholds::default()).unwrap();
        let m = &r.doctor.unwrap().per_rule["buried"];
        assert_eq!((m.tp, m.fn_, m.abstained), (0, 0, 3));
    }
}
