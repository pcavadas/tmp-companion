//! `probe --doctor-window-ab` — capture-WINDOW A/B evidence arm.
//!
//! The ORIGINAL Doctor capture window was the full 6 s stimulus + 2500 ms tail
//! ([`ORACLE_TAIL_MS`] — pinned here, deliberately NOT `leveller::DOCTOR_TAIL_MS`,
//! which became the shorter production value once this arm's 2026-07-16 run
//! validated the shrink: 6 slots incl. two wet, 0 verdict flips, Δtilt ≤ 0.08,
//! band deltas within wash-preset run variance). The production window is now
//! 3 s stim + a GRAPH-GATED tail (`leveller::DOCTOR_STIM_MS`/`doctor::doctor_tail_ms`
//! — 300 ms `DOCTOR_TAIL_DRY_MS` for a known-dry chain, else the full 1500 ms
//! `DOCTOR_TAIL_MS`); re-run this arm against the SAME original oracle to
//! re-validate any future window change. Per the repo's hard-won lesson (CLAUDE.md's leveling
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
//! stimulus + the pinned 2.5 s [`ORACLE_TAIL_MS`]; the production-prepared
//! 3 s arm — `doctor_stim_slice`, pad-aware, tail resolved per slot via
//! `doctor::doctor_tail_ms` off that slot's live graph, same as
//! `commands/doctor.rs`; 4 s-stim + the pinned 1.5 s [`SHORT_TAIL_MS`])
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
/// The 4 s fallback's tail — pinned independently of production (see module doc).
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
    pad_aware: bool,
) -> Result<Capture, String> {
    let (samples, rate) =
        leveller::doctor_capture(slot, None, &[], stim_slice, Some(0.5), tail_ms, false)?;
    // `pad_aware`: true for the production-prepared 3 s arm (padded stim, body
    // PSD starts at `doctor_signal_start` — exactly `doctor_check`'s path);
    // false for the raw oracle/4 s slices, whose estimated onset feeds the body
    // PSD directly — see `analyze_capture`'s doc.
    let read = analyze_capture(stim_slice, &samples, rate, family, pad_aware)?;
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
    // whatever ships), including the production stimulus PREPARATION: the same
    // `doctor_stim_slice` pad+slice and pad-aware analysis `doctor_check` uses —
    // a raw un-padded slice would validate a capture path production never
    // runs. The oracle tail (`ORACLE_TAIL_MS`) and the 4 s fallback are
    // deliberately PINNED literals — the oracle must never drift with the
    // production constants, and 4 s has no production counterpart.
    let prod_stim = leveller::doctor_stim_slice(stim.clone());
    let four_s = stim.len().min(4 * 48_000);
    // Variant b's tail must match what `doctor_check`/`doctor_apply` actually
    // pick for THIS slot's graph — 300 ms dry, 1500 ms wet/unknown
    // (`doctor::doctor_tail_ms`, `commands/doctor.rs`'s `tail_ms`) — else a
    // dry preset's shipped 300 ms window goes unvalidated by an arm labeled
    // "production". Resolved once per slot below via the same
    // `read_slot_preset_parsed` + `extract_active_graph` pattern
    // `doctor_inject`/`doctor_defects` use.
    // Cap the oracle to exactly 6 s too — a longer --stim source (e.g. a Tier-2
    // capture) would otherwise let the oracle capture on more signal than the b/c
    // variants, shifting tail placement and invalidating the delta comparison.
    let oracle_stim = &stim[..ORACLE_STIM_SAMPLES];

    let mut rows: Vec<serde_json::Value> = Vec::new();
    let mut out = format!(
        "doctor-window-ab ({family_id}, {} slot(s), stim={stim_path})\n",
        slots.len()
    );
    let mut max_delta_b = 0.0f64;
    let mut max_delta_c = 0.0f64;
    let mut flips_b = 0usize;
    let mut flips_c = 0usize;
    let mut skipped: Vec<u32> = Vec::new();

    for &slot in slots {
        // Each capture sleeps the reconnect gap BEFORE its result is matched, so
        // a failed capture (which has already churned the device) paces the next
        // connection exactly like a successful one instead of cascading failures
        // across the remaining slots. One shape for all three variants.
        let mut settle_or_skip = |label: &str, res: Result<Capture, String>| -> Option<Capture> {
            std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
            match res {
                Ok(c) => Some(c),
                Err(e) => {
                    eprintln!("[probe] slot {slot}: {label} capture failed: {e} (skipping slot)");
                    skipped.push(slot);
                    None
                }
            }
        };

        eprintln!("[probe] doctor-window-ab: slot {slot} — oracle (full stim)…");
        let Some(oracle) = settle_or_skip(
            "oracle",
            capture_variant(slot, oracle_stim, ORACLE_TAIL_MS, family, false),
        ) else {
            continue;
        };

        let prod_tail_ms = match crate::read_slot_preset_parsed(slot) {
            Ok((preset, _, _)) => {
                let nodes: Vec<doctor::DoctorNode> =
                    crate::session::extract_active_graph(&preset, None)
                        .nodes
                        .iter()
                        .map(doctor::DoctorNode::from_graph_node)
                        .collect();
                u64::from(doctor::doctor_tail_ms(&nodes))
            }
            Err(e) => {
                eprintln!(
                    "[probe] slot {slot}: graph resolve failed ({e}) — falling back to the conservative {SHORT_TAIL_MS}ms tail"
                );
                SHORT_TAIL_MS
            }
        };
        eprintln!(
            "[probe] doctor-window-ab: slot {slot} — production 3 s stim (padded) + {prod_tail_ms} ms tail…"
        );
        let Some(b) = settle_or_skip(
            "3s",
            capture_variant(slot, &prod_stim, prod_tail_ms, family, true),
        ) else {
            continue;
        };

        eprintln!("[probe] doctor-window-ab: slot {slot} — 4 s stim + 1.5 s tail…");
        let Some(c) = settle_or_skip(
            "4s",
            capture_variant(slot, &stim[..four_s], SHORT_TAIL_MS, family, false),
        ) else {
            continue;
        };

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
            "slot {slot}\n  oracle  tilt={:>6.2?} tail={:>6.1} verdicts={:?}\n  3s/{prod_tail_ms}ms tilt={:>6.2?} tail={:>6.1} Δband={delta_b:>5.1?} Δtilt={:?} Δtail={:>+5.2} flip={b_flip} verdicts={:?}\n  4s/1.5s tilt={:>6.2?} tail={:>6.1} Δband={delta_c:>5.1?} Δtilt={:?} Δtail={:>+5.2} flip={c_flip} verdicts={:?}\n",
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
                "tailMs": prod_tail_ms,
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
    // A skipped slot carries no evidence — a zero-flip summary must never read
    // as complete when part of the requested sweep couldn't be captured.
    if !skipped.is_empty() {
        out += &format!(
            "doctor-window-ab: INCOMPLETE — slots {skipped:?} could not be captured; rerun before trusting the deltas\n"
        );
    }

    if let Some(path) = out_path {
        let report = serde_json::json!({
            "stimulus": stim_path,
            "family": family_id,
            "bands": family.bands(),
            "bandLabels": family.labels(),
            "oracleTailMs": ORACLE_TAIL_MS,
            "shortTailMs": SHORT_TAIL_MS,
            "rows": rows,
            "skippedSlots": skipped,
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
