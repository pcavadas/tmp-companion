//! `probe --doctor-window-ab` — capture-WINDOW A/B evidence arm.
//!
//! The ORIGINAL Doctor capture window was the full 6 s stimulus + 2500 ms tail
//! ([`ORACLE_TAIL_MS`] — pinned here, deliberately NOT `leveller::DOCTOR_TAIL_MS`,
//! which became the shorter production value once this arm's 2026-07-16 run
//! validated the shrink: 6 slots incl. two wet, 0 verdict flips, Δtilt ≤ 0.08,
//! band deltas within wash-preset run variance). The production window is now
//! 3 s + 1500 ms (`leveller::DOCTOR_STIM_MS`/`DOCTOR_TAIL_MS`); re-run this arm
//! against the SAME original oracle to re-validate any future window change. Per the repo's hard-won lesson (CLAUDE.md's leveling
//! capture-window note): a capture-window change is a RE-BASELINE, validated
//! only against a full-capture oracle — never self-consistently (a level→verify
//! round-trip that reuses the same short method would hide the very offset it's
//! supposed to catch). This arm produces that oracle-anchored evidence; it does
//! NOT change the production window itself. Attended — a supervisor drives it on
//! real hardware later and reads the deltas.
//!
//! Usage:
//! ```text
//! probe --doctor-window-ab <slots_csv> --stim <wav> [--family <guitar|bass|bass-vi>] [--out <report.json>]
//! ```
//! Captures each slot THREE ways on its own fresh connections (oracle: full
//! stimulus + `DOCTOR_TAIL_MS`; 3 s-stim + 1500 ms tail; 4 s-stim + 1500 ms tail)
//! via `leveller::doctor_capture` (LOADS the probed slots — accepted for this
//! attended arm, same recipe as `--doctor-calib`), builds each capture's
//! `SoundProfile` off ONE shared post-onset body PSD (the Task-1 seams,
//! `doctor::body_psd` + `SoundProfile::from_capture_with_psd`), and reports per
//! (slot, variant) band dB / deviation / tilt slope / tail ratio / spread /
//! fired-verdict-kind list, then the b/c DELTAS vs the oracle (band dB per band,
//! tilt, tail ratio, and whether the fired-verdict SET changed — the headline).
//! Human-readable table on stdout + an optional deterministic JSON `--out`.
//! READ-ONLY on the unit in the sense of never saving; ends re-amp OFF.

use crate::doctor;
use crate::leveller;

use super::analyze::analyze_capture;
use super::stimulus::read_stimulus_calibrated;

/// The ORIGINAL full-window tail the oracle captures with (see module doc).
const ORACLE_TAIL_MS: u64 = 2_500;
/// Post-3 s/4 s tail — see the module doc for the plan's proposed shorter window.
const SHORT_TAIL_MS: u64 = 1_500;

/// One capture's derived Doctor measurements — everything the A/B compares.
struct Capture {
    band_db: Vec<f64>,
    deviations: Vec<f64>,
    tilt_slope: Option<f64>,
    tail_ratio_db: f64,
    spread_lu: f64,
    verdicts: Vec<&'static str>,
}

/// Capture `slot` re-amped with `stim_slice` + `tail_ms`, then derive its full
/// Doctor measurement set — the SAME target-deviation/Theil-Sen/diagnosis path
/// `doctor_check` runs, off ONE shared body PSD (Task 1's seams).
fn capture_variant(
    slot: u32,
    stim_slice: &[f32],
    tail_ms: u64,
    family: doctor::Family,
) -> Result<Capture, String> {
    let (samples, rate) =
        leveller::doctor_capture(slot, None, &[], stim_slice, Some(0.5), tail_ms, false)?;
    // Raw, unpadded stim → the estimated onset feeds the body PSD directly
    // (`pad_aware: false`) — see `analyze_capture`'s doc for why this differs
    // from `doctor_inject`'s padded/`doctor_signal_start` variant.
    let read = analyze_capture(stim_slice, &samples, rate, family, false)?;
    if !read.onset_confident {
        eprintln!(
            "[probe] doctor-window-ab: onset not confidently found for slot {slot} (tail {tail_ms}ms) — un-aligned split"
        );
    }
    Ok(Capture {
        band_db: read.band_db,
        deviations: read.deviations,
        tilt_slope: read.tilt_slope,
        tail_ratio_db: read.tail_ratio_db,
        spread_lu: read.spread_lu,
        verdicts: read.verdicts,
    })
}

/// Sorted-set equality — order is not semantic for "which verdicts fired".
fn verdict_set(v: &[&'static str]) -> Vec<&'static str> {
    let mut s = v.to_vec();
    s.sort_unstable();
    s
}

fn capture_to_json(c: &Capture) -> serde_json::Value {
    serde_json::json!({
        "bandDb": c.band_db,
        "deviations": c.deviations,
        "tiltDbPerOct": c.tilt_slope,
        "tailRatioDb": c.tail_ratio_db,
        "spreadLu": c.spread_lu,
        "verdicts": c.verdicts,
    })
}

pub fn probe_doctor_window_ab(
    slots: &[u32],
    stim_path: &str,
    family_id: &str,
    out_path: Option<&str>,
) -> Result<String, String> {
    // A typo'd --family would silently sweep + report under the wrong band
    // layout — fail loudly first (mirrors --doctor-calib).
    let family = super::parse_family_arg(family_id)?;
    let stim = read_stimulus_calibrated(stim_path, None)?;
    // A shorter source would make `min` silently collapse the oracle onto the
    // 3 s variant — identical captures, meaningless zero deltas.
    const ORACLE_STIM_SAMPLES: usize = 6 * 48_000;
    if stim.len() < ORACLE_STIM_SAMPLES {
        return Err(format!(
            "doctor-window-ab needs a ≥6 s stimulus for the oracle window (got {:.1} s)",
            stim.len() as f64 / 48_000.0
        ));
    }
    // Variant b TRACKS the production window (re-running this arm re-validates
    // whatever ships); the oracle tail (`ORACLE_TAIL_MS`) and the 4 s fallback
    // are deliberately PINNED literals — the oracle must never drift with the
    // production constants, and 4 s has no production counterpart.
    let three_s = stim.len().min(leveller::doctor_stim_samples());
    let four_s = stim.len().min(4 * 48_000);

    let mut rows: Vec<serde_json::Value> = Vec::new();
    let mut out = format!(
        "doctor-window-ab ({family_id}, {} slot(s), stim={stim_path})\n",
        slots.len()
    );
    let mut max_delta_b = 0.0f64;
    let mut max_delta_c = 0.0f64;
    let mut flips_b = 0usize;
    let mut flips_c = 0usize;

    for &slot in slots {
        eprintln!("[probe] doctor-window-ab: slot {slot} — oracle (full stim)…");
        let oracle = match capture_variant(slot, &stim, ORACLE_TAIL_MS, family) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[probe] slot {slot}: oracle capture failed: {e} (skipping slot)");
                continue;
            }
        };
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));

        eprintln!("[probe] doctor-window-ab: slot {slot} — 3 s stim + 1.5 s tail…");
        let b = match capture_variant(slot, &stim[..three_s], SHORT_TAIL_MS, family) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[probe] slot {slot}: 3s capture failed: {e} (skipping slot)");
                continue;
            }
        };
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));

        eprintln!("[probe] doctor-window-ab: slot {slot} — 4 s stim + 1.5 s tail…");
        let c = match capture_variant(slot, &stim[..four_s], SHORT_TAIL_MS, family) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[probe] slot {slot}: 4s capture failed: {e} (skipping slot)");
                continue;
            }
        };
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));

        let band_delta = |x: &Capture| -> Vec<f64> {
            oracle
                .band_db
                .iter()
                .zip(&x.band_db)
                .map(|(o, v)| v - o)
                .collect()
        };
        let delta_b = band_delta(&b);
        let delta_c = band_delta(&c);
        let tilt_delta = |x: &Capture| match (oracle.tilt_slope, x.tilt_slope) {
            (Some(o), Some(v)) => Some(v - o),
            _ => None,
        };
        let oracle_set = verdict_set(&oracle.verdicts);
        let b_flip = verdict_set(&b.verdicts) != oracle_set;
        let c_flip = verdict_set(&c.verdicts) != oracle_set;
        if b_flip {
            flips_b += 1;
        }
        if c_flip {
            flips_c += 1;
        }
        max_delta_b = max_delta_b.max(delta_b.iter().fold(0.0f64, |m, d| m.max(d.abs())));
        max_delta_c = max_delta_c.max(delta_c.iter().fold(0.0f64, |m, d| m.max(d.abs())));

        out += &format!(
            "slot {slot}\n  oracle  tilt={:>6.2?} tail={:>6.1} verdicts={:?}\n  3s/1.5s tilt={:>6.2?} tail={:>6.1} Δband={delta_b:>5.1?} Δtilt={:?} Δtail={:>+5.2} flip={b_flip} verdicts={:?}\n  4s/1.5s tilt={:>6.2?} tail={:>6.1} Δband={delta_c:>5.1?} Δtilt={:?} Δtail={:>+5.2} flip={c_flip} verdicts={:?}\n",
            oracle.tilt_slope,
            oracle.tail_ratio_db,
            oracle.verdicts,
            b.tilt_slope,
            b.tail_ratio_db,
            tilt_delta(&b),
            b.tail_ratio_db - oracle.tail_ratio_db,
            b.verdicts,
            c.tilt_slope,
            c.tail_ratio_db,
            tilt_delta(&c),
            c.tail_ratio_db - oracle.tail_ratio_db,
            c.verdicts,
        );

        rows.push(serde_json::json!({
            "slot": slot,
            "oracle": capture_to_json(&oracle),
            "threeSec": {
                "capture": capture_to_json(&b),
                "deltaBandDb": delta_b,
                "deltaTiltDbPerOct": tilt_delta(&b),
                "deltaTailRatioDb": b.tail_ratio_db - oracle.tail_ratio_db,
                "verdictSetChanged": b_flip,
            },
            "fourSec": {
                "capture": capture_to_json(&c),
                "deltaBandDb": delta_c,
                "deltaTiltDbPerOct": tilt_delta(&c),
                "deltaTailRatioDb": c.tail_ratio_db - oracle.tail_ratio_db,
                "verdictSetChanged": c_flip,
            },
        }));
    }

    // Belt-and-braces re-amp OFF even on a mid-sweep error.
    super::reamp_off_best_effort();

    if rows.is_empty() {
        return Err("no sound captured".to_string());
    }

    out += &format!(
        "doctor-window-ab summary: {} slot(s) — max |Δband dB| 3s={max_delta_b:.2} 4s={max_delta_c:.2}; verdict-set flips 3s={flips_b} 4s={flips_c}\n",
        rows.len(),
    );

    if let Some(path) = out_path {
        let report = serde_json::json!({
            "stimulus": stim_path,
            "family": family_id,
            "bands": family.bands(),
            "bandLabels": family.labels(),
            "oracleTailMs": ORACLE_TAIL_MS,
            "shortTailMs": SHORT_TAIL_MS,
            "rows": rows,
            "summary": {
                "maxAbsBandDeltaDb3s": max_delta_b,
                "maxAbsBandDeltaDb4s": max_delta_c,
                "verdictFlips3s": flips_b,
                "verdictFlips4s": flips_c,
            },
        });
        let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        std::fs::write(path, &json).map_err(|e| format!("write {path}: {e}"))?;
        out += &format!("  → {path}\n");
    }

    Ok(out)
}
