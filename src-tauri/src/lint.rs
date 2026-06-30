//! Gain-stage audit & preset lint.
//!
//! A MEASURE audit that flags clipping + loudness outliers from re-amp measurements.
//! The device re-amp pass that produces the measurements is deferred to the manual
//! runbook (read-only policy) — the audit logic is tested against synthetic measures.
//! Fixed rule set; it reports (no auto-fix).

use serde::Serialize;

/// One lint/audit finding for a preset.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    pub list_index: u32,
    pub rule: String,
    pub message: String,
}

/// A re-amp measurement of one preset (the MEASURE input to the audit).
#[derive(Debug, Clone, Copy)]
pub struct AuditMeasure {
    pub list_index: u32,
    pub peak_dbfs: f64,
    pub loudness_lufs: f64,
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b)); // NaN-safe (a failed loudness measure won't panic)
    let n = v.len();
    if n == 0 {
        return 0.0;
    }
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// Audit re-amp measurements: flag clipping (`peak_dbfs >= 0`) and loudness outliers
/// (|loudness − median| > `outlier_lu`). MEASURE — pure over the supplied measures.
pub fn audit_measures(measures: &[AuditMeasure], outlier_lu: f64) -> Vec<Finding> {
    let med = median(measures.iter().map(|m| m.loudness_lufs).collect());
    let mut out = Vec::new();
    for m in measures {
        if m.peak_dbfs >= 0.0 {
            out.push(Finding {
                list_index: m.list_index,
                rule: "clip".into(),
                message: format!("peak {:.2} dBFS clips", m.peak_dbfs),
            });
        }
        if (m.loudness_lufs - med).abs() > outlier_lu {
            out.push(Finding {
                list_index: m.list_index,
                rule: "loudness-outlier".into(),
                message: format!(
                    "loudness {:.1} LUFS is >{outlier_lu} LU from the median {med:.1}",
                    m.loudness_lufs
                ),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // AC — audit flags clipping + loudness outliers from synthetic measures.
    #[test]
    fn audit_flags_clip_and_outliers() {
        let measures = [
            AuditMeasure {
                list_index: 0,
                peak_dbfs: -6.0,
                loudness_lufs: -20.0,
            },
            AuditMeasure {
                list_index: 1,
                peak_dbfs: 0.5,
                loudness_lufs: -20.5,
            }, // clips
            AuditMeasure {
                list_index: 2,
                peak_dbfs: -3.0,
                loudness_lufs: -30.0,
            }, // outlier (median ~-20.5)
        ];
        let findings = audit_measures(&measures, 5.0);
        let clip: Vec<u32> = findings
            .iter()
            .filter(|f| f.rule == "clip")
            .map(|f| f.list_index)
            .collect();
        let out: Vec<u32> = findings
            .iter()
            .filter(|f| f.rule == "loudness-outlier")
            .map(|f| f.list_index)
            .collect();
        assert_eq!(clip, vec![1]);
        assert_eq!(out, vec![2]);
    }

    // AC — lint report CSV export (golden).
}
