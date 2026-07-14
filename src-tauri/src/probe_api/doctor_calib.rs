//! Doctor CAPTURE-space calibration sweep (`probe --doctor-calib`).
//!
//! The attended half of the "Doctor on captures" work: sweep a real library
//! through a REAL stimulus (a Tier-2 DI capture wav, `--stim`), measure every
//! selected slot's Doctor `SoundProfile` + the pre-onset noise-floor metric + the
//! stimulus band coverage, and — when a `--labels` ground-truth is supplied —
//! DERIVE proposed `Thresholds` for the CAPTURE table (`doctor::*_CAPTURE`) that
//! separate the labelled-positive slots from the clean ones per rule.
//!
//! Output is a DETERMINISTIC JSON report (no timestamps; the operator names the
//! `--out` file) plus a human markdown summary on stdout. READ-ONLY on the unit:
//! loads + captures, never saves; the sweep ends re-amp OFF.
//!
//! Usage:
//! ```text
//! probe --doctor-calib <slots_csv> --stim <wav> --family <guitar|bass|bass-vi> \
//!       [--labels <rules.json>] --out <report.json>
//! ```
//! `--labels` is `{"washed":[5,6],"muddy":[11],…}` — 0-based list-index slot
//! lists of the sounds that GENUINELY have each problem (the positives).

use crate::audio;
use crate::doctor;
use crate::footswitch;
use crate::leveller;
use crate::lufs;
use crate::read_slot_preset_parsed;
use crate::session;

use super::stimulus::read_stimulus_48k;

/// Margin (in the rule's own metric units) added above `p95(clean)` when a rule
/// has NO labelled positives — a conservative "fire only well past the cleanest
/// library" threshold. ponytail: a flat 3-unit margin; re-derive per-rule only if
/// the no-positive rules prove too eager on real captures.
const NO_POSITIVE_MARGIN: f64 = 3.0;

/// The eight diagnosis rules, in a FIXED order so the report is byte-deterministic
/// regardless of the (hash-ordered) `--labels` map.
const RULES: [&str; 8] = [
    "muddy", "boomy", "harsh", "fizzy", "lost", "washed", "spiky", "buried",
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

/// Pre-onset noise-floor metric: `20·log10(rms(capture[..onset]) / rms(body))`
/// dB — how far the leading (pre-signal) hiss sits under the stimulus body. `None`
/// when the onset isn't confident or sits under 10 ms of samples (no meaningful
/// pre-window to measure) or the body is silent. Pure.
fn noise_floor_db(
    samples: &[f32],
    rate: u32,
    onset: usize,
    confident: bool,
    stimulus_samples: usize,
) -> Option<f64> {
    let min_onset = rate as usize / 100; // 10 ms
    if !confident || onset < min_onset || onset > samples.len() {
        return None;
    }
    let body_end = onset.saturating_add(stimulus_samples).min(samples.len());
    let pre = doctor::rms_f64(&samples[..onset]);
    let body = doctor::rms_f64(&samples[onset..body_end]);
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

/// Per-slot rule metrics, in the SAME LHS space `doctor::diagnose` compares to
/// each threshold (every rule is "metric > threshold"). `dev` mirrors diagnose:
/// vs the cohort median, or the neighbour-expectation fallback when the sweep is
/// under `MIN_COHORT`.
fn rule_metrics(
    profile: &doctor::SoundProfile,
    family: doctor::Family,
    cohort: Option<&[f64]>,
) -> Vec<(&'static str, f64)> {
    let bal = doctor::balance(&profile.bands);
    let dev = |i: usize| doctor::band_dev(&bal, cohort, i);
    let (lows, low_mids, mids, high_mids, highs, air) = family.semantic_bands();
    vec![
        ("muddy", dev(low_mids)),
        ("boomy", dev(lows)),
        ("harsh", dev(high_mids)),
        ("fizzy", bal[air] - bal[highs]),
        ("lost", -dev(mids)),
        ("washed", profile.tail_ratio_db),
        ("spiky", profile.spread_lu),
        ("buried", -dev(lows)),
    ]
}

/// One measured slot in the sweep.
struct Row {
    slot: u32,
    profile: doctor::SoundProfile,
    coverage: Vec<bool>,
    noise_floor_db: Option<f64>,
}

pub fn probe_doctor_calib(
    slots: &[u32],
    stim_path: &str,
    family_id: &str,
    labels_path: Option<&str>,
    out_path: &str,
) -> Result<String, String> {
    // `from_topology` silently defaults unrecognized strings to Guitar — fine for
    // `--doctor` (topology ids), but a typo'd `--family` here would sweep + derive
    // `*_CAPTURE` thresholds under the wrong band layout. Fail loudly first.
    if !matches!(
        family_id.to_ascii_lowercase().as_str(),
        "guitar" | "bass" | "bass-vi"
    ) {
        return Err(format!(
            "unrecognized --family '{family_id}' (expected guitar|bass|bass-vi)"
        ));
    }
    let family = doctor::Family::from_topology(family_id);
    let stim = read_stimulus_48k(stim_path)?;
    let stim_loud = lufs::measure_mono(&stim, 48_000)?;

    let mut rows: Vec<Row> = Vec::new();
    for &slot in slots {
        // One field-8 slot read drives the base-sound force-bypass isolation
        // (every on/off block off) — the same recipe as `probe --doctor`.
        let fb: Vec<(String, String, bool)> = match read_slot_preset_parsed(slot) {
            Ok((preset, _, _)) => {
                footswitch::all_onoff_blocks(preset.get("ftsw").unwrap_or(&serde_json::Value::Null))
                    .into_iter()
                    .map(|(g, n)| (g, n, true))
                    .collect()
            }
            Err(e) => {
                eprintln!("[probe] slot {slot}: preset read failed ({e}) — no isolation");
                Vec::new()
            }
        };
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
        match leveller::doctor_capture(slot, None, &fb, &stim, Some(0.5), false) {
            Ok((samples, rate)) => {
                let (onset, confident) = audio::estimate_onset(&stim, &samples, rate);
                let profile =
                    doctor::SoundProfile::from_capture(&samples, rate, stim.len(), onset, family)?;
                let coverage = doctor::band_coverage(&stim, family);
                let noise_floor_db = noise_floor_db(&samples, rate, onset, confident, stim.len());
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
    // Belt-and-braces re-amp OFF even on a mid-sweep error.
    let _ = session::Session::connect().and_then(|mut s| s.set_reamp_mode(false).map(|_| ()));

    if rows.is_empty() {
        return Err("no sound captured".to_string());
    }

    let cohort: Option<Vec<f64>> = (rows.len() >= doctor::MIN_COHORT).then(|| {
        let refs: Vec<&doctor::SoundProfile> = rows.iter().map(|r| &r.profile).collect();
        doctor::cohort_median(&refs)
    });

    // Ground-truth positive slot lists per rule (0-based list indices).
    let labels: std::collections::HashMap<String, Vec<u32>> = match labels_path {
        Some(p) => {
            let raw = std::fs::read_to_string(p).map_err(|e| format!("read {p}: {e}"))?;
            serde_json::from_str(&raw).map_err(|e| format!("parse labels {p}: {e}"))?
        }
        None => std::collections::HashMap::new(),
    };

    // Per-slot rule metrics (shared by the report rows and the derivation).
    let metrics_by_slot: Vec<(u32, Vec<(&'static str, f64)>)> = rows
        .iter()
        .map(|r| (r.slot, rule_metrics(&r.profile, family, cohort.as_deref())))
        .collect();

    let report_rows: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "slot": r.slot,
                "balanceDb": doctor::balance(&r.profile.bands),
                "tailRatioDb": r.profile.tail_ratio_db,
                "spreadLu": r.profile.spread_lu,
                "noiseFloorDb": r.noise_floor_db,
                "integratedLufs": r.profile.integrated_lufs,
                "coverage": r.coverage,
            })
        })
        .collect();

    // Per-rule derivation (only rules present in --labels).
    let mut derivations = serde_json::Map::new();
    let mut md = String::new();
    for rule in RULES {
        let Some(positives) = labels.get(rule) else {
            continue;
        };
        let metric_of = |slot: u32| -> Option<f64> {
            metrics_by_slot
                .iter()
                .find(|(s, _)| *s == slot)
                .and_then(|(_, m)| m.iter().find(|(k, _)| *k == rule).map(|(_, v)| *v))
        };
        let (mut clean, mut positive) = (Vec::new(), Vec::new());
        for (slot, m) in &metrics_by_slot {
            let v = m
                .iter()
                .find(|(k, _)| *k == rule)
                .map(|(_, v)| *v)
                .unwrap_or(0.0);
            if positives.contains(slot) {
                positive.push(v);
            } else {
                clean.push(v);
            }
        }
        // Positive slots labelled but not captured this sweep are dropped by
        // metric_of returning None — report them so the operator notices.
        let missing: Vec<u32> = positives
            .iter()
            .copied()
            .filter(|s| metric_of(*s).is_none())
            .collect();
        let (proposed, separation) = propose_threshold(&clean, &positive);
        let (cmin, cmed, cp90, cmax) = stats(&clean).unwrap_or((0.0, 0.0, 0.0, 0.0));
        derivations.insert(
            rule.to_string(),
            serde_json::json!({
                "cleanStats": { "min": cmin, "median": cmed, "p90": cp90, "max": cmax },
                "positiveValues": positive,
                "proposedThreshold": proposed,
                "separationMargin": separation,
                "missingLabelledSlots": missing,
            }),
        );
        md += &format!(
            "  {rule:<7} proposed={proposed:>8.3}  sep={}  clean[min/med/p90/max]={cmin:.2}/{cmed:.2}/{cp90:.2}/{cmax:.2}  pos={:?}\n",
            separation.map_or("   n/a".to_string(), |s| format!("{s:>6.2}")),
            positive.iter().map(|v| format!("{v:.2}")).collect::<Vec<_>>(),
        );
    }

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
        "captureParams": {
            "doctorTailMs": leveller::DOCTOR_TAIL_MS,
            "refLevel": 0.5,
            "bandCoverageDb": doctor::BAND_COVERAGE_DB,
        },
        "cohort": if cohort.is_some() { "median" } else { "absolute" },
        "rows": report_rows,
        "derivations": serde_json::Value::Object(derivations),
    });
    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(out_path, &json).map_err(|e| format!("write {out_path}: {e}"))?;

    let mut out = format!(
        "doctor-calib sweep ({family_id}, {} sounds, cohort={}) → {out_path}\n  stim {stim_path} ({:.2} LUFS, spread {:.2} LU)\n  slot |     LUFS |  tail dB | noise dB | spread\n",
        rows.len(),
        if cohort.is_some() { "median" } else { "absolute" },
        stim_loud.integrated_lufs,
        stim_loud.spread_lu(),
    );
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
    factory_slots: &[u32],
    stim_path: &str,
    family_id: &str,
    out_path: &str,
) -> Result<String, String> {
    if !matches!(
        family_id.to_ascii_lowercase().as_str(),
        "guitar" | "bass" | "bass-vi"
    ) {
        return Err(format!(
            "unrecognized --family '{family_id}' (expected guitar|bass|bass-vi)"
        ));
    }
    let family = doctor::Family::from_topology(family_id);
    let stim = read_stimulus_48k(stim_path)?;
    let stim_loud = lufs::measure_mono(&stim, 48_000)?;

    let mut rows: Vec<Row> = Vec::new();
    for &slot in factory_slots {
        // Load on its OWN connection (load + engage in one connection captures
        // silence — see `doctor_capture_current`); FactoryPresets tab = 4, 1-based.
        {
            match session::Session::connect() {
                Ok(mut s) => {
                    if let Err(e) = s.load_preset_raw(u64::from(slot) + 1, 4) {
                        eprintln!("[probe] factory slot {slot}: load failed: {e} (skipping)");
                        continue;
                    }
                }
                Err(e) => {
                    eprintln!("[probe] factory slot {slot}: connect failed: {e} (skipping)");
                    continue;
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
        match leveller::doctor_capture_current(&stim, Some(0.5)) {
            Ok((samples, rate)) => {
                let (onset, _confident) = audio::estimate_onset(&stim, &samples, rate);
                let profile =
                    doctor::SoundProfile::from_capture(&samples, rate, stim.len(), onset, family)?;
                let coverage = doctor::band_coverage(&stim, family);
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
    // Belt-and-braces re-amp OFF even on a mid-sweep error.
    let _ = session::Session::connect().and_then(|mut s| s.set_reamp_mode(false).map(|_| ()));

    if rows.is_empty() {
        return Err("no sound captured".to_string());
    }

    let report_rows: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "slot": r.slot,
                "balanceDb": doctor::balance(&r.profile.bands),
                "tailRatioDb": r.profile.tail_ratio_db,
                "spreadLu": r.profile.spread_lu,
                "integratedLufs": r.profile.integrated_lufs,
                "coverage": r.coverage,
            })
        })
        .collect();

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
    });
    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(out_path, &json).map_err(|e| format!("write {out_path}: {e}"))?;

    let mut out = format!(
        "doctor-calib-factory sweep ({family_id}, {} sounds) → {out_path}\n  stim {stim_path} ({:.2} LUFS, spread {:.2} LU)\n  slot |     LUFS |  tail dB | spread\n",
        rows.len(),
        stim_loud.integrated_lufs,
        stim_loud.spread_lu(),
    );
    for r in &rows {
        out += &format!(
            "  {:>4} | {:>8.2} | {:>8.1} | {:>6.2}\n",
            r.slot, r.profile.integrated_lufs, r.profile.tail_ratio_db, r.profile.spread_lu,
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
