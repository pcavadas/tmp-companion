//! `probe --doctor-loo` and `probe --benchmark` — the file I/O around the pure
//! held-out benchmarks in `benchmark.rs` (#200). OFFLINE: neither arm touches
//! the device; both read JSON a sweep or a labeling pass already produced and
//! write a deterministic JSON result (the operator names `--out`).
//!
//! ```text
//! probe --doctor-loo <factory_report.json> [--kind synthetic|capture] [--out <loo.json>]
//! probe --benchmark <corpus.json> [--thresholds <thresholds.json>] --out <result.json>
//! ```
//! `--doctor-loo` reads a `probe --doctor-calib-factory` report whose rows carry
//! `replay` profiles. `--benchmark` exits non-zero when a threshold is violated.

use crate::benchmark::{self, Corpus, ReplayProfile, Thresholds};
use crate::doctor::StimulusKind;

fn read_json(path: &str) -> Result<serde_json::Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("parse {path}: {e}"))
}

fn write_json(path: &str, value: &impl serde::Serialize) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(path, json + "\n").map_err(|e| format!("write {path}: {e}"))
}

fn fmt_rate(r: &benchmark::FpRate) -> String {
    match (r.rate, r.rate_upper95) {
        (Some(p), Some(u)) => format!(
            "{:>2}/{:<2} {:>5.1}% (≤{:.1}%)",
            r.fires,
            r.sounds,
            p * 100.0,
            u * 100.0
        ),
        _ => "n/a".to_string(),
    }
}

/// Leave-one-out false-positive estimate over a factory sweep report.
pub fn probe_doctor_loo(
    report_path: &str,
    kind: &str,
    out_path: Option<&str>,
) -> Result<String, String> {
    let kind = match kind {
        "synthetic" => StimulusKind::Synthetic,
        "capture" => StimulusKind::Capture,
        other => return Err(format!("unknown --kind {other:?} (synthetic | capture)")),
    };
    let report = read_json(report_path)?;
    let family_id = report["family"]
        .as_str()
        .ok_or("the report has no `family`")?;
    let family = super::parse_family_arg(family_id)?;
    let rows = report["rows"]
        .as_array()
        .ok_or("the report has no `rows`")?;
    let mut sounds = Vec::with_capacity(rows.len());
    for row in rows {
        let id = row["slot"].to_string();
        let replay = row.get("replay").filter(|v| !v.is_null()).ok_or_else(|| {
            format!(
                "row {id} has no `replay` profile — re-run the sweep with a probe that records one"
            )
        })?;
        let replay: ReplayProfile =
            serde_json::from_value(replay.clone()).map_err(|e| format!("row {id}: {e}"))?;
        sounds.push((id, replay));
    }
    let loo = benchmark::doctor_leave_one_out(&sounds, family, kind)?;
    if let Some(out) = out_path {
        write_json(out, &loo)?;
    }

    let mut md = format!(
        "doctor leave-one-out ({family_id}, {} healthy sounds; every verdict is a false positive)\n",
        loo.sounds
    );
    md += "  rule     | in-sample               ";
    for mode in loo.modes.keys() {
        md += &format!("| {mode:<24}");
    }
    md += "\n";
    let mut rows: Vec<(&str, Vec<&benchmark::FpRate>)> = benchmark::RULE_KEYS
        .iter()
        .map(|&k| {
            let mut v = vec![&loo.in_sample.per_rule[k]];
            v.extend(loo.modes.values().map(|m| &m.per_rule[k]));
            (k, v)
        })
        .collect();
    let mut any = vec![&loo.in_sample.any_rule];
    any.extend(loo.modes.values().map(|m| &m.any_rule));
    rows.push(("any rule", any));
    for (rule, rates) in rows {
        md += &format!("  {rule:<8} ");
        for r in rates {
            md += &format!("| {:<24}", fmt_rate(r));
        }
        md += "\n";
    }
    if let Some(out) = out_path {
        md += &format!("  → {out}\n");
    }
    Ok(md)
}

/// Score a labeled corpus. Returns the summary and whether every threshold held.
pub fn probe_benchmark(
    corpus_path: &str,
    thresholds_path: Option<&str>,
    out_path: &str,
) -> Result<(String, bool), String> {
    let corpus: Corpus = serde_json::from_value(read_json(corpus_path)?)
        .map_err(|e| format!("{corpus_path}: {e}"))?;
    let thresholds: Thresholds = match thresholds_path {
        Some(p) => serde_json::from_value(read_json(p)?).map_err(|e| format!("{p}: {e}"))?,
        None => Thresholds::default(),
    };
    let result = benchmark::score_corpus(&corpus, &thresholds)?;
    write_json(out_path, &result)?;

    let mut md = format!(
        "benchmark {corpus_path} ({}, fw {}) → {out_path}\n",
        result.family, result.provenance.firmware
    );
    if let Some(d) = &result.doctor {
        md += &format!("  doctor: {} validation sounds\n", d.sounds);
        for (rule, s) in &d.per_rule {
            if s.tp + s.fp + s.fn_ == 0 {
                continue;
            }
            let pct =
                |v: Option<f64>| v.map_or("  n/a".to_string(), |x| format!("{:>5.1}", x * 100.0));
            md += &format!(
                "    {rule:<8} precision {}%  recall {}%  fp {}%  abstain {}%\n",
                pct(s.precision),
                pct(s.recall),
                pct(s.fp_rate),
                pct(s.abstention_rate),
            );
        }
    }
    if let Some(l) = &result.leveling {
        let lu = |v: Option<f64>| v.map_or("n/a".to_string(), |x| format!("{x:.2}"));
        md += &format!(
            "  leveling: {} rows, {} measured; |error| median {} / p90 {} / max {} LU; false clamps {}, persist mismatches {}, non-idempotent re-runs {}\n",
            l.rows,
            l.measured,
            lu(l.median_abs_error_lu),
            lu(l.p90_abs_error_lu),
            lu(l.max_abs_error_lu),
            l.false_clamps,
            l.persist_mismatches,
            l.non_idempotent_reruns,
        );
    }
    if result.pass {
        md += "  PASS\n";
    } else {
        md += "  FAIL\n";
        for v in &result.violations {
            md += &format!("    {v}\n");
        }
    }
    Ok((md, result.pass))
}
