//! Doctor CAPTURE-space calibration sweep (`probe --doctor-calib`).
//!
//! The attended half of the "Doctor on captures" work: sweep a real library
//! through a REAL stimulus (a Tier-2 DI capture wav, `--stim`), measure every
//! selected slot's Doctor `SoundProfile` + the pre-onset noise-floor metric + the
//! stimulus band coverage, and — when a `--labels` ground-truth is supplied —
//! DERIVE proposed `Thresholds` values. There is no separate CAPTURE table:
//! `StimulusKind::Synthetic` and `StimulusKind::Capture` share one per-family
//! `Thresholds` table, and this sweep's derived values inform the R5 attended
//! recalibration of that shared table.
//!
//! Output is a DETERMINISTIC JSON report (no timestamps; the operator names the
//! `--out` file) plus a human markdown summary on stdout. Each row carries a
//! `replay` profile, the input `probe --doctor-loo` re-diagnoses offline.
//! READ-ONLY on the unit: loads + captures, never saves; the sweep ends re-amp
//! OFF.
//!
//! Usage:
//! ```text
//! probe --doctor-calib <slots_csv> --stim <wav> --family <guitar|bass|bass-vi> \
//!       [--labels <rules.json>] --out <report.json>
//! ```
//! `--labels` is `{"washed":[5,6],"muddy":[11],…}` — 0-based list-index slot
//! lists of the sounds that GENUINELY have each problem (the positives).

use crate::doctor;
use crate::footswitch;
use crate::leveller;
use crate::lufs;
use crate::session;
use crate::slot::ListIndex;
use crate::slot_read::read_slot_preset_parsed;

use crate::stimulus::read_stimulus_48k;

/// Margin (in the rule's own metric units) added above `p95(clean)` when a rule
/// has NO labelled positives — a conservative "fire only well past the cleanest
/// library" threshold. ponytail: a flat 3-unit margin; re-derive per-rule only if
/// the no-positive rules prove too eager on real captures.
const NO_POSITIVE_MARGIN: f64 = 3.0;

/// The seven diagnosis rules, in a FIXED order so the report is byte-deterministic
/// regardless of the (hash-ordered) `--labels` map.
const RULES: [&str; 7] = [
    "muddy", "boomy", "harsh", "fizzy", "lost", "washed", "buried",
];

/// Min/median/p90/max of a sample set (`None` when empty). Pure.
fn stats(xs: &[f64]) -> Option<(f64, f64, f64, f64)> {
    if xs.is_empty() {
        return None;
    }
    let mut s = xs.to_vec();
    s.sort_by(f64::total_cmp);
    Some((
        s[0],
        percentile_sorted(&s, 0.5),
        percentile_sorted(&s, 0.9),
        s[s.len() - 1],
    ))
}

/// Linear-interpolated percentile of a pre-sorted slice (`p` in 0..=1). Empty →
/// 0.0 (callers guard emptiness where it matters).
fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let rank = p.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    let frac = rank - lo as f64;
    sorted[lo] + (sorted[hi] - sorted[lo]) * frac
}

/// Propose a rule threshold from labelled `clean` vs `positive` metric values.
/// Every rule fires "metric > threshold", so the split is: the midpoint between
/// the worst (highest) clean value and the best (lowest) positive value cleanly
/// separates them. With NO positives, fall back to `p95(clean) + margin`.
/// Returns `(proposed_threshold, separation_margin)` where the margin is
/// `best_positive − worst_clean` (positive = a clean gap; ≤ 0 = the classes
/// overlap and the rule can't perfectly separate). `None` margin = one class is
/// empty, so no separation is measurable (the report's `positiveValues` tells the
/// two apart: empty = no positives labelled, non-empty = no clean samples). Never
/// a non-finite value — serde_json writes those as `null`, aliasing `None`.
fn propose_threshold(clean: &[f64], positive: &[f64]) -> (f64, Option<f64>) {
    if positive.is_empty() {
        let mut c = clean.to_vec();
        c.sort_by(f64::total_cmp);
        return (percentile_sorted(&c, 0.95) + NO_POSITIVE_MARGIN, None);
    }
    let best_positive = positive.iter().copied().fold(f64::INFINITY, f64::min);
    // No clean samples: anchor just below the marginal positive so it still fires.
    if clean.is_empty() {
        return (best_positive - NO_POSITIVE_MARGIN, None);
    }
    let worst_clean = clean.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (
        (worst_clean + best_positive) / 2.0,
        Some(best_positive - worst_clean),
    )
}

/// Pre-signal noise-floor metric: `20·log10(rms(capture[..signal_start]) /
/// rms(body))` dB — how far the leading (pre-signal) hiss sits under the
/// stimulus body. `body_end` is `body_start + body_len` from the same
/// `DoctorOnset` (where the played audio ends) — passed explicitly because
/// `signal_start` is pad-SHIFTED, so `signal_start + body_len` would overshoot
/// the body by one pad. `None` when the onset isn't confident or sits under
/// 10 ms of samples (no meaningful pre-window) or the body is silent. Pure.
fn noise_floor_db(
    samples: &[f32],
    rate: u32,
    signal_start: usize,
    confident: bool,
    body_end: usize,
) -> Option<f64> {
    let min_onset = rate as usize / 100; // 10 ms
    if !confident || signal_start < min_onset || signal_start > samples.len() {
        return None;
    }
    let body_end = body_end.min(samples.len());
    let pre = doctor::rms_f64(&samples[..signal_start]);
    let body = doctor::rms_f64(&samples[signal_start..body_end]);
    if body <= 0.0 {
        return None;
    }
    Some(20.0 * (pre / body).max(1e-6).log10())
}

/// FNV-1a 64-bit over the stimulus sample bytes — a deterministic content hash
/// for the report header (platform-stable, unlike `std::hash`).
fn stim_hash(samples: &[f32]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for s in samples {
        for b in s.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    format!("{h:016x}")
}

/// One rule's metric value(s), keyed by rule name: `(tilt-space, centered-space)`.
/// The two differ only for the CONSENSUS band rules (muddy/boomy/harsh/lost/
/// buried — see `doctor::Thresholds`'s doc); for fizzy/washed (no centered
/// gate exists) both fields carry the same single-space value, so every rule's
/// derivation can go through the one code path below.
type RuleMetric = (&'static str, f64, f64);

/// A slot's rule metrics + its broadband tilt slope: `(slot, tilt_slope, rule_metrics)`.
type SlotMetrics = (ListIndex, Option<f64>, Vec<RuleMetric>);

/// Per-slot rule metrics, in the SAME LHS space `doctor::diagnose` compares to
/// each threshold (every rule is "metric > threshold"): the target-deviation
/// tilt-split LOCAL ([`doctor::deviations`] / [`doctor::tilt_split`]) plus the
/// R6 CONSENSUS [`doctor::centered_deviations`] value for the tonal rules, the
/// self-difference for fizzy, and the time-domain tail metric for washed.
/// Also returns the broadband tilt slope (`tilt_split`'s discarded first value)
/// so callers building a `tiltDbPerOct` report field don't re-run the same
/// deviations+tilt_split pass.
fn rule_metrics(
    profile: &doctor::SoundProfile,
    coverage: &[bool],
    family: doctor::Family,
) -> (Option<f64>, Vec<RuleMetric>) {
    let bdb = doctor::band_db(&profile.bands);
    let dev = doctor::deviations(&bdb, family);
    // Same stimulus-transfer anchor `compute_rule_metrics` applies — keeps this
    // threshold-derivation metric in lockstep with the engine's verdict space.
    let dev = doctor::anchor_deviations(dev, profile.stim_bands.as_deref(), family);
    // The row's own coverage, matching `diagnose_kind`'s call — thresholds must
    // be derived in the SAME coverage-gated metric space the verdict path reads.
    let (slope, locals) = doctor::tilt_split(&dev, family, Some(coverage));
    // NaN where too few body bands are covered to center on (the engine's
    // consensus rules abstain there), so the report shows the gap.
    let centered = doctor::centered_deviations(&dev, family, Some(coverage))
        .unwrap_or_else(|| vec![f64::NAN; dev.len()]);
    let (lows, low_mids, mids, high_mids, _highs, _air) = family.semantic_bands();
    let fizzy = doctor::fizzy_metric(&bdb, profile.stim_bands.as_deref(), family);
    (
        slope,
        vec![
            ("muddy", locals[low_mids], centered[low_mids]),
            ("boomy", locals[lows], centered[lows]),
            ("harsh", locals[high_mids], centered[high_mids]),
            ("fizzy", fizzy, fizzy),
            ("lost", -locals[mids], -centered[mids]),
            ("washed", profile.tail_ratio_db, profile.tail_ratio_db),
            ("buried", -locals[lows], -centered[lows]),
        ],
    )
}

/// The engine's ACTUAL verdict keys for one measured row — recorded in the
/// sweep JSON so a sanity/false-fire check reads real engine output, not a
/// re-derivation (an earlier check silently read a MISSING `verdicts` key and
/// concluded "0 fires" from vacuous data — never again).
fn row_verdicts(
    profile: &doctor::SoundProfile,
    coverage: &[bool],
    family: doctor::Family,
) -> Vec<&'static str> {
    doctor::diagnose_kind(
        profile,
        None,
        family,
        doctor::StimulusKind::Synthetic,
        Some(coverage),
        doctor::PlaybackOffsets::NONE,
    )
    .into_iter()
    .map(|d| d.key)
    .collect()
}

/// Top localized peaks (`SoundProfile::peaks`, height-sorted) as JSON — the
/// resonant/boxy gate evidence per row.
fn peaks_json(profile: &doctor::SoundProfile) -> serde_json::Value {
    serde_json::Value::Array(
        profile
            .peaks
            .iter()
            .take(3)
            .map(|p| serde_json::json!({ "freqHz": p.freq_hz, "heightDb": p.height_db, "q": p.q }))
            .collect(),
    )
}

/// Everything the engine reads from this row, so `probe --doctor-loo` and the
/// held-out benchmark (`benchmark.rs`) can re-diagnose the sweep offline under
/// a re-derived calibration without another capture.
fn replay_json(r: &Row) -> serde_json::Value {
    serde_json::to_value(crate::benchmark::ReplayProfile::from_profile(
        &r.profile,
        Some(&r.coverage),
    ))
    .unwrap_or(serde_json::Value::Null)
}

/// One measured slot in the sweep.
struct Row {
    slot: ListIndex,
    profile: doctor::SoundProfile,
    coverage: Vec<bool>,
    noise_floor_db: Option<f64>,
}

/// One capture's `SoundProfile` + its CAPTURED OUTPUT's own coverage, in the
/// EXACT production measurement space (`leveller::analyze_capture`, the seam
/// `doctor_check` itself runs) — a sweep in any other space would derive
/// thresholds and targets the shipped engine never measures in. Shared by both
/// sweeps below; logs + `None` on failure so callers `let … else { continue }` —
/// one bad slot never ends a sweep, and no `?` can return past the sweep's
/// re-amp OFF cleanup. `label` distinguishes the user and factory sweeps' log
/// lines.
#[allow(clippy::too_many_arguments)]
fn profile_or_skip(
    label: &str,
    slot: ListIndex,
    sig: &leveller::DoctorSignal,
    rate: u32,
    stim: &[f32],
    tail_ms: u32,
    family: doctor::Family,
) -> Option<leveller::ProfiledCapture> {
    let stim_psd = leveller::stim_body_psd(stim, rate);
    match leveller::analyze_capture(sig, rate, stim, &stim_psd, family, tail_ms, None) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("[probe] {label}slot {slot}: profile failed: {e} (skipping)");
            None
        }
    }
}

/// Requested-but-not-captured slots — recorded in the JSON and flagged
/// INCOMPLETE on stdout so a biased subset never reads as complete evidence.
fn skipped_slots(requested: &[ListIndex], rows: &[Row]) -> Vec<ListIndex> {
    requested
        .iter()
        .copied()
        .filter(|s| !rows.iter().any(|r| r.slot == *s))
        .collect()
}

pub fn probe_doctor_calib(
    slots: &[ListIndex],
    stim_path: &str,
    family_id: &str,
    labels_path: Option<&str>,
    out_path: &str,
) -> Result<String, String> {
    // A typo'd `--family` here would sweep + derive threshold values under the
    // wrong band layout. Fail loudly first.
    let family = super::parse_family_arg(family_id)?;
    // Production Doctor window (3 s slice + the shorter tail): thresholds/targets
    // must be derived in the SAME capture space `doctor_check` measures in.
    let stim = leveller::doctor_stim_slice(read_stimulus_48k(stim_path)?);
    let stim_loud = lufs::measure_mono(&stim, 48_000)?;

    // Fresh-connection re-amp OFF on every exit path (mid-sweep error included).
    let _reamp_off = super::ReampOffGuard;
    let mut rows: Vec<Row> = Vec::new();
    for &slot in slots {
        // One field-8 slot read drives the base-sound force-bypass isolation
        // (every on/off block off) — the same recipe as `probe --doctor` —
        // and, off the SAME read, the production tail (`doctor::doctor_tail_ms`,
        // same policy `commands/doctor.rs` uses) for THIS slot's graph, so
        // calibration evidence is gathered in production's capture space
        // instead of a pinned literal.
        let (fb, tail_ms): (Vec<(String, String, bool)>, u64) = match read_slot_preset_parsed(slot)
        {
            Ok((preset, _, _)) => {
                let fb = footswitch::all_onoff_blocks(
                    preset.get("ftsw").unwrap_or(&serde_json::Value::Null),
                )
                .into_iter()
                .map(|(g, n)| (g, n, true))
                .collect();
                (
                    fb,
                    u64::from(super::doctor_inject::tail_ms_for_doc(&preset)),
                )
            }
            Err(e) => {
                // Capturing WITHOUT isolation would let active on/off blocks
                // contaminate the derived thresholds while the row still reads
                // as valid evidence — skip; `skipped_slots` marks the sweep
                // INCOMPLETE instead.
                eprintln!("[probe] slot {slot}: preset read failed ({e}) — skipping");
                continue;
            }
        };
        match leveller::doctor_capture(slot, None, &fb, &[], &stim, Some(0.5), tail_ms, false) {
            Ok((samples, rate)) => {
                let Some(leveller::ProfiledCapture {
                    profile,
                    coverage,
                    onset,
                    ..
                }) = profile_or_skip("", slot, &samples, rate, &stim, tail_ms as u32, family)
                else {
                    continue;
                };
                // the floor window ends at the pad-shifted start.
                let noise_floor_db = noise_floor_db(
                    &samples.mix,
                    rate,
                    onset.signal_start,
                    onset.confident(),
                    onset.body_end(),
                );
                rows.push(Row {
                    slot,
                    profile,
                    coverage,
                    noise_floor_db,
                });
            }
            Err(e) => eprintln!("[probe] slot {slot}: capture failed: {e} (skipping)"),
        }
    }

    if rows.is_empty() {
        return Err("no sound captured".to_string());
    }

    // Ground-truth positive slot lists per rule (0-based list indices).
    let labels: std::collections::HashMap<String, Vec<ListIndex>> = match labels_path {
        Some(p) => {
            let raw = std::fs::read_to_string(p).map_err(|e| format!("read {p}: {e}"))?;
            serde_json::from_str(&raw).map_err(|e| format!("parse labels {p}: {e}"))?
        }
        None => std::collections::HashMap::new(),
    };

    // Per-slot rule metrics + tilt slope (shared by the report rows and the
    // derivation).
    let metrics_by_slot: Vec<SlotMetrics> = rows
        .iter()
        .map(|r| {
            let (slope, metrics) = rule_metrics(&r.profile, &r.coverage, family);
            (r.slot, slope, metrics)
        })
        .collect();

    let report_rows: Vec<serde_json::Value> = rows
        .iter()
        .zip(&metrics_by_slot)
        .map(|(r, (_, slope, _))| {
            serde_json::json!({
                "slot": r.slot,
                "balanceDb": doctor::balance(&r.profile.bands),
                "tailRatioDb": r.profile.tail_ratio_db,
                "spreadLu": r.profile.spread_lu,
                "noiseFloorDb": r.noise_floor_db,
                "integratedLufs": r.profile.integrated_lufs,
                "coverage": r.coverage,
                "tiltDbPerOct": slope,
                "verdicts": row_verdicts(&r.profile, &r.coverage, family),
                "peaks": peaks_json(&r.profile),
                "replay": replay_json(r),
            })
        })
        .collect();

    // Per-rule derivation (only rules present in --labels), BOTH consensus
    // spaces independently — tilt and centered each get their own clean/
    // positive split + proposed threshold (R6: a band rule now fires only when
    // BOTH spaces clear their own gate, see `doctor::Thresholds`'s doc).
    let mut derivations = serde_json::Map::new();
    let mut md = String::new();
    for rule in RULES {
        let Some(positives) = labels.get(rule) else {
            continue;
        };
        let (mut clean_t, mut positive_t) = (Vec::new(), Vec::new());
        let (mut clean_c, mut positive_c) = (Vec::new(), Vec::new());
        for (slot, _slope, m) in &metrics_by_slot {
            let (vt, vc) = m
                .iter()
                .find(|(k, _, _)| *k == rule)
                .map(|(_, vt, vc)| (*vt, *vc))
                .unwrap_or((0.0, 0.0));
            if positives.contains(slot) {
                positive_t.push(vt);
                positive_c.push(vc);
            } else {
                clean_t.push(vt);
                clean_c.push(vc);
            }
        }
        // Positive slots labelled but not captured this sweep — report them so
        // the operator notices (every captured slot's metrics vec always
        // carries all RULES, so "missing" means "not captured at all").
        let missing: Vec<ListIndex> = positives
            .iter()
            .copied()
            .filter(|s| !metrics_by_slot.iter().any(|(slot, _, _)| slot == s))
            .collect();
        let (proposed_t, sep_t) = propose_threshold(&clean_t, &positive_t);
        let (proposed_c, sep_c) = propose_threshold(&clean_c, &positive_c);
        let (cmin_t, cmed_t, cp90_t, cmax_t) = stats(&clean_t).unwrap_or((0.0, 0.0, 0.0, 0.0));
        let (cmin_c, cmed_c, cp90_c, cmax_c) = stats(&clean_c).unwrap_or((0.0, 0.0, 0.0, 0.0));
        let space_json = |proposed: f64,
                          separation: Option<f64>,
                          positive: &[f64],
                          (cmin, cmed, cp90, cmax): (f64, f64, f64, f64)| {
            serde_json::json!({
                "cleanStats": { "min": cmin, "median": cmed, "p90": cp90, "max": cmax },
                "positiveValues": positive,
                "proposedThreshold": proposed,
                "separationMargin": separation,
            })
        };
        derivations.insert(
            rule.to_string(),
            serde_json::json!({
                "tilt": space_json(proposed_t, sep_t, &positive_t, (cmin_t, cmed_t, cp90_t, cmax_t)),
                "centered": space_json(proposed_c, sep_c, &positive_c, (cmin_c, cmed_c, cp90_c, cmax_c)),
                "missingLabelledSlots": missing,
            }),
        );
        md += &format!(
            "  {rule:<7} tilt     proposed={proposed_t:>8.3}  sep={}  clean[min/med/p90/max]={cmin_t:.2}/{cmed_t:.2}/{cp90_t:.2}/{cmax_t:.2}  pos={:?}\n",
            sep_t.map_or("   n/a".to_string(), |s| format!("{s:>6.2}")),
            positive_t.iter().map(|v| format!("{v:.2}")).collect::<Vec<_>>(),
        );
        md += &format!(
            "  {rule:<7} centered proposed={proposed_c:>8.3}  sep={}  clean[min/med/p90/max]={cmin_c:.2}/{cmed_c:.2}/{cp90_c:.2}/{cmax_c:.2}  pos={:?}\n",
            sep_c.map_or("   n/a".to_string(), |s| format!("{s:>6.2}")),
            positive_c.iter().map(|v| format!("{v:.2}")).collect::<Vec<_>>(),
        );
    }

    let skipped = skipped_slots(slots, &rows);
    let report = serde_json::json!({
        "stimulus": {
            "path": stim_path,
            "hash": stim_hash(&stim),
            "integratedLufs": stim_loud.integrated_lufs,
            "spreadLu": stim_loud.spread_lu(),
        },
        "family": family_id,
        "bands": family.bands(),
        "bandLabels": family.labels(),
        "skippedSlots": skipped,
        "captureParams": {
            // Per-slot now (`doctor::doctor_tail_ms` off each slot's own graph,
            // 300 ms known-dry / 1500 ms wet-or-unknown) — no longer one fixed
            // number, so this reports the policy, not a single capture value.
            "doctorTailMsPolicy": "graph-derived per slot (doctor::doctor_tail_ms)",
            "refLevel": 0.5,
            "bandCoverageDb": doctor::BAND_COVERAGE_DB,
        },
        "metric": "target-deviation-theil-sen",
        "rows": report_rows,
        "derivations": serde_json::Value::Object(derivations),
    });
    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(out_path, &json).map_err(|e| format!("write {out_path}: {e}"))?;

    let mut out = format!(
        "doctor-calib sweep ({family_id}, {} sounds, target-deviation) → {out_path}\n  stim {stim_path} ({:.2} LUFS, spread {:.2} LU)\n  slot |     LUFS |  tail dB | noise dB | spread\n",
        rows.len(),
        stim_loud.integrated_lufs,
        stim_loud.spread_lu(),
    );
    if !skipped.is_empty() {
        out += &format!(
            "  INCOMPLETE — requested slots {skipped:?} could not be captured; the derivations cover a subset\n"
        );
    }
    for r in &rows {
        out += &format!(
            "  {:>4} | {:>8.2} | {:>8.1} | {:>8} | {:>6.2}\n",
            r.slot,
            r.profile.integrated_lufs,
            r.profile.tail_ratio_db,
            r.noise_floor_db
                .map_or("   n/a".to_string(), |v| format!("{v:.1}")),
            r.profile.spread_lu,
        );
    }
    if !md.is_empty() {
        out += "  proposed CAPTURE thresholds (per labelled rule):\n";
        out += &md;
    }
    Ok(out)
}

/// FACTORY-bank variant for REFERENCE-curve derivation. Loads each factory preset
/// (tabEnum = 4 `FactoryPresets`, 1-based `presetSlot` = list index + 1) on its own
/// connection, then captures it AS-LOADED — its designed default tone, the intended
/// "good tone" baseline — re-amped through the DI `stim`. No labels/derivation and
/// no base force-bypass (unlike `probe_doctor_calib`): the per-slot `balanceDb` rows
/// feed the offline per-band median → the per-topology reference curve. READ-ONLY:
/// load + capture, never saves; ends re-amp OFF.
pub fn probe_doctor_calib_factory(
    factory_slots: &[ListIndex],
    stim_path: &str,
    family_id: &str,
    out_path: &str,
) -> Result<String, String> {
    let family = super::parse_family_arg(family_id)?;
    // Production Doctor window (3 s slice + the shorter tail): thresholds/targets
    // must be derived in the SAME capture space `doctor_check` measures in.
    let stim = leveller::doctor_stim_slice(read_stimulus_48k(stim_path)?);
    let stim_loud = lufs::measure_mono(&stim, 48_000)?;

    // Fresh-connection re-amp OFF on every exit path (mid-sweep error included).
    let _reamp_off = super::ReampOffGuard;
    let mut rows: Vec<Row> = Vec::new();
    for &slot in factory_slots {
        // Load on its OWN connection (load + engage in one connection captures
        // silence — see `doctor_capture_current`); FactoryPresets tab = 4, 1-based.
        // Conservative default tail (wet/unknown) until the load's own graph
        // resolves below.
        let mut tail_ms = u64::from(leveller::DOCTOR_TAIL_MS);
        {
            match session::Session::connect() {
                Ok(mut s) => {
                    if let Err(e) = s.load_factory_preset(slot) {
                        eprintln!("[probe] factory slot {slot}: load failed: {e} (skipping)");
                        continue;
                    }
                    // `load_factory_preset` discards its own field-3 push
                    // (fire-and-forget `transact_eager`) — pump once more to
                    // harvest it, then resolve THIS preset's production tail
                    // (`doctor::doctor_tail_ms`) so the capture below matches
                    // the window `commands/doctor.rs` actually uses for it.
                    let _ = s.pump_collect_alive(700);
                    match s.current_preset_value() {
                        Ok(doc) => tail_ms = u64::from(super::doctor_inject::tail_ms_for_doc(&doc)),
                        Err(e) => eprintln!(
                            "[probe] factory slot {slot}: graph resolve failed ({e}) — falling back to the conservative {tail_ms}ms tail"
                        ),
                    }
                }
                Err(e) => {
                    eprintln!("[probe] factory slot {slot}: connect failed: {e} (skipping)");
                    continue;
                }
            }
        }
        match leveller::doctor_capture_current(&stim, None, &[], Some(0.5), tail_ms) {
            Ok((samples, rate)) => {
                let Some(leveller::ProfiledCapture {
                    profile, coverage, ..
                }) = profile_or_skip(
                    "factory ",
                    slot,
                    &samples,
                    rate,
                    &stim,
                    tail_ms as u32,
                    family,
                )
                else {
                    continue;
                };
                rows.push(Row {
                    slot,
                    profile,
                    coverage,
                    noise_floor_db: None,
                });
            }
            Err(e) => eprintln!("[probe] factory slot {slot}: capture failed: {e} (skipping)"),
        }
    }

    if rows.is_empty() {
        return Err("no sound captured".to_string());
    }

    let report_rows: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let (slope, _metrics) = rule_metrics(&r.profile, &r.coverage, family);
            serde_json::json!({
                "slot": r.slot,
                "balanceDb": doctor::balance(&r.profile.bands),
                "tailRatioDb": r.profile.tail_ratio_db,
                "spreadLu": r.profile.spread_lu,
                "integratedLufs": r.profile.integrated_lufs,
                "coverage": r.coverage,
                "tiltDbPerOct": slope,
                "verdicts": row_verdicts(&r.profile, &r.coverage, family),
                "peaks": peaks_json(&r.profile),
                "replay": replay_json(r),
            })
        })
        .collect();

    let skipped = skipped_slots(factory_slots, &rows);
    let report = serde_json::json!({
        "stimulus": {
            "path": stim_path,
            "hash": stim_hash(&stim),
            "integratedLufs": stim_loud.integrated_lufs,
            "spreadLu": stim_loud.spread_lu(),
        },
        "family": family_id,
        "bands": family.bands(),
        "bandLabels": family.labels(),
        "source": "factory",
        "rows": report_rows,
        "skippedSlots": skipped,
    });
    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(out_path, &json).map_err(|e| format!("write {out_path}: {e}"))?;

    let mut out = format!(
        "doctor-calib-factory sweep ({family_id}, {} sounds) → {out_path}\n  stim {stim_path} ({:.2} LUFS, spread {:.2} LU)\n  slot |     LUFS |  tail dB | spread\n",
        rows.len(),
        stim_loud.integrated_lufs,
        stim_loud.spread_lu(),
    );
    if !skipped.is_empty() {
        out += &format!(
            "  INCOMPLETE — requested slots {skipped:?} could not be captured; the reference medians cover a subset\n"
        );
    }
    for r in &rows {
        out += &format!(
            "  {:>4} | {:>8.2} | {:>8.1} | {:>6.2}\n",
            r.slot, r.profile.integrated_lufs, r.profile.tail_ratio_db, r.profile.spread_lu,
        );
    }
    Ok(out)
}

/// OFFLINE: re-diagnose raw Doctor captures (multichannel float WAVs of the
/// whole capture window plus its tail) through the shipped engine, with no
/// unit attached. The capture runs through the same `leveller::analyze_capture`
/// seam a live sweep uses, so a detector or constant change can be checked
/// against recorded hardware audio. `stim_path` is the stimulus the capture was
/// played through; the tail is whatever the capture holds past it.
pub fn probe_doctor_replay(
    wavs_csv: &str,
    stim_path: &str,
    family_id: &str,
) -> Result<String, String> {
    let family = super::parse_family_arg(family_id)?;
    let stim = leveller::doctor_stim_slice(crate::stimulus::read_stimulus_48k(stim_path)?);
    let mut out = String::new();
    for wav in wavs_csv.split(',').map(str::trim).filter(|w| !w.is_empty()) {
        let mut r = hound::WavReader::open(wav).map_err(|e| format!("open {wav}: {e}"))?;
        let spec = r.spec();
        // The stimulus is 48 kHz and `doctor_onset` sizes the body window from its length,
        // so a capture at any other rate would shift the window into the tail.
        if spec.sample_rate != leveller::RATE {
            return Err(format!(
                "capture {wav} has sample rate {}, expected {}",
                spec.sample_rate,
                leveller::RATE
            ));
        }
        let interleaved: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>(),
            hound::SampleFormat::Int => {
                let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
                r.samples::<i32>()
                    .map(|s| s.map(|v| v as f32 / scale))
                    .collect::<Result<_, _>>()
            }
        }
        .map_err(|e| format!("read {wav}: {e}"))?;
        let cap = crate::audio::Capture {
            interleaved,
            channels: usize::from(spec.channels),
            sample_rate: spec.sample_rate,
        };
        let frames = cap.interleaved.len() / cap.channels.max(1);
        let tail_ms = (frames.saturating_sub(stim.len()) as u64 * 1000
            / u64::from(cap.sample_rate.max(1))) as u32;
        let sig = leveller::DoctorSignal::from_capture(&cap);
        let Some(pc) = profile_or_skip(
            "replay ",
            ListIndex(0),
            &sig,
            cap.sample_rate,
            &stim,
            tail_ms,
            family,
        ) else {
            out += &format!("{wav}\tprofile failed\n");
            continue;
        };
        let peaks: Vec<String> = pc
            .profile
            .peaks
            .iter()
            .filter(|p| {
                p.freq_hz < doctor::RESONANT_MAX_FREQ_HZ
                    && (doctor::RESONANT_MIN_Q..=doctor::RESONANT_MAX_Q).contains(&p.q)
                    && p.height_db >= doctor::BOXY_MIN_HEIGHT_DB
            })
            .take(6)
            .map(|p| format!("{:.0}Hz h{:.1} q{:.1}", p.freq_hz, p.height_db, p.q))
            .collect();
        let (locals, centered) = doctor::band_locals(&pc.profile, family, Some(&pc.coverage));
        let fmt = |v: &[f64]| {
            v.iter()
                .map(|x| format!("{x:+.1}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        out += &format!(
            "{wav}\ttail {:.1} dB\t[{}]\t{}\tlocals {}\tcentered {}\n",
            pc.profile.tail_ratio_db,
            row_verdicts(&pc.profile, &pc.coverage, family).join(","),
            peaks.join("; "),
            fmt(&locals),
            centered.as_deref().map_or_else(|| "-".to_string(), fmt),
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn propose_midpoint_between_worst_clean_and_best_positive() {
        // clean maxes at 2.0, positives bottom out at 6.0 → threshold 4.0, sep 4.0.
        let clean = [0.0, 1.0, 2.0];
        let positive = [6.0, 8.0, 10.0];
        let (thr, sep) = propose_threshold(&clean, &positive);
        assert!((thr - 4.0).abs() < 1e-9, "{thr}");
        assert!((sep.unwrap() - 4.0).abs() < 1e-9);
    }

    #[test]
    fn propose_overlap_gives_negative_separation() {
        // Classes overlap (clean up to 7, positive down to 5) → sep < 0.
        let (thr, sep) = propose_threshold(&[0.0, 7.0], &[5.0, 9.0]);
        assert!((thr - 6.0).abs() < 1e-9); // (7 + 5)/2
        assert!(sep.unwrap() < 0.0);
    }

    #[test]
    fn propose_no_clean_anchors_below_best_positive_with_finite_margin() {
        // No clean samples → threshold just under the marginal positive, and the
        // margin is None (not Infinity — serde_json would alias that to null).
        let (thr, sep) = propose_threshold(&[], &[6.0, 8.0]);
        assert!((thr - (6.0 - NO_POSITIVE_MARGIN)).abs() < 1e-9, "{thr}");
        assert!(sep.is_none());
    }

    #[test]
    fn propose_no_positives_uses_p95_plus_margin() {
        // 0..=100 clean, no positives → p95 = 95, + 3 margin = 98, no separation.
        let clean: Vec<f64> = (0..=100).map(|i| i as f64).collect();
        let (thr, sep) = propose_threshold(&clean, &[]);
        assert!((thr - (95.0 + NO_POSITIVE_MARGIN)).abs() < 1e-9, "{thr}");
        assert!(sep.is_none());
    }

    #[test]
    fn stats_and_percentile() {
        let (min, med, p90, max) = stats(&[1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
        assert_eq!((min, max), (1.0, 5.0));
        assert!((med - 3.0).abs() < 1e-9);
        assert!((p90 - 4.6).abs() < 1e-9); // 0.9*(5-1)=3.6 → between idx3(4) and idx4(5)
        assert!(stats(&[]).is_none());
    }

    #[test]
    fn noise_floor_known_hiss_and_short_onset() {
        let sr = 48_000u32;
        let onset = sr as usize / 10; // 100 ms → confidently over the 10 ms floor
                                      // Pre-onset hiss at amplitude 0.01, body at 0.1 → ratio 20·log10(0.1) = −20 dB.
        let mut cap = vec![0.01f32; onset];
        cap.extend(std::iter::repeat_n(0.1f32, sr as usize));
        let nf = noise_floor_db(&cap, sr, onset, true, sr as usize).expect("floor");
        assert!((nf - (-20.0)).abs() < 0.5, "got {nf}");
        // Onset under 10 ms of samples → None (no pre-window to trust).
        assert!(noise_floor_db(&cap, sr, sr as usize / 1000, true, sr as usize).is_none());
        // Not confident → None regardless of onset.
        assert!(noise_floor_db(&cap, sr, onset, false, sr as usize).is_none());
    }
}
