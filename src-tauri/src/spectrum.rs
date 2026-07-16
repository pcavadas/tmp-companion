//! Spectrum report, EQ-match & best-SIC.
//!
//! MEASURE: the analysis runs on a re-amp capture (the device pass is deferred to the
//! manual runbook under the read-only policy). The DSP here is pure + unit-tested on
//! synthetic signals: coarse band energies (the shared windowed Welch PSD estimator,
//! `crate::psd::welch_psd` — replaced the old 4-probe un-windowed Goertzel scan, which
//! carried ~±2.5 dB variance and heavy HF spectral leakage), tonal flags, EQ-match band
//! deltas (a *suggestion* applied via a block-parameter edit), and best-SIC ranking by
//! spectral distance. Fixed coarse bands; EQ-match suggests (the user applies);
//! best-SIC is a ranked suggestion.

use serde::Serialize;

/// Energy in each `(lo, hi)` band — [`crate::psd::Psd::band_powers`] off ONE windowed
/// Welch PSD estimate of `signal` (see the module doc for why this replaced the
/// per-band Goertzel probes). Every consumer here compares energies RELATIVELY —
/// `tonal_flags`'s ratios, `spectral_distance`/`eq_match_deltas`'s dB differences,
/// `rank_sics`'s distance ordering — so the switch from Goertzel's ad hoc
/// `power/N²` units to the PSD's true signal-power-per-Hz units changes no
/// downstream behavior.
pub fn band_energies(signal: &[f32], rate: f32, bands: &[(f32, f32)]) -> Vec<f64> {
    crate::psd::welch_psd(signal, rate).band_powers(bands)
}

/// The default coarse bands: low / low-mid / high-mid / high (Hz).
pub fn default_bands() -> Vec<(f32, f32)> {
    vec![
        (60.0, 250.0),
        (250.0, 1000.0),
        (1000.0, 4000.0),
        (4000.0, 12000.0),
    ]
}

/// Tonal flags from 4-band energies `[low, lowmid, highmid, high]`. Heuristic ratios.
pub fn tonal_flags(e: &[f64]) -> Vec<String> {
    let mut flags = Vec::new();
    if e.len() != 4 {
        return flags;
    }
    let (low, _lowmid, highmid, high) = (e[0], e[1], e[2], e[3]);
    let total: f64 = e.iter().sum::<f64>().max(1e-12);
    if (high + highmid) / total < 0.10 {
        flags.push("dark (little high-frequency content)".into());
    }
    if high / total > 0.50 {
        flags.push("harsh (high-frequency heavy)".into());
    }
    if low / total > 0.70 {
        flags.push("boomy (low-frequency heavy)".into());
    }
    flags
}

fn to_db(e: f64) -> f64 {
    10.0 * (e.max(1e-12)).log10()
}

/// Spectral distance: sum of squared per-band dB differences.
pub fn spectral_distance(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (to_db(*x) - to_db(*y)).powi(2))
        .sum()
}

/// Per-band EQ-match gain deltas (dB) that move `source` energies toward `reference`.
pub fn eq_match_deltas(source: &[f64], reference: &[f64]) -> Vec<f64> {
    source
        .iter()
        .zip(reference)
        .map(|(s, r)| to_db(*r) - to_db(*s))
        .collect()
}

/// Apply dB `deltas` to `source` energies (for previewing the match).
pub fn apply_deltas(source: &[f64], deltas: &[f64]) -> Vec<f64> {
    source
        .iter()
        .zip(deltas)
        .map(|(s, d)| s * 10f64.powf(d / 10.0))
        .collect()
}

/// A SIC candidate ranked against a target spectrum.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SicRank {
    pub sicid: String,
    pub distance: f64,
}

/// Rank SIC candidates by spectral distance to `target` (nearest first) — the
/// best-match suggestion.
pub fn rank_sics(target: &[f64], candidates: &[(String, Vec<f64>)]) -> Vec<SicRank> {
    let mut ranked: Vec<SicRank> = candidates
        .iter()
        .map(|(id, e)| SicRank {
            sicid: id.clone(),
            distance: spectral_distance(target, e),
        })
        .collect();
    // total_cmp: a NaN distance (from a degenerate capture) sorts deterministically
    // instead of panicking the comparator.
    ranked.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate).sin())
            .collect()
    }

    // AC — a synthetic tone's energy concentrates in its band.
    #[test]
    fn band_energies_on_synthetic() {
        let rate = 48000.0;
        let sig = sine(2000.0, rate, 4800); // sits in band index 2 (1000-4000)
        let e = band_energies(&sig, rate, &default_bands());
        let max_band = e
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        assert_eq!(max_band, 2, "2 kHz tone dominates the 1-4 kHz band: {e:?}");
    }

    // AC — tonal flags fire on a dark spectrum.
    #[test]
    fn tonal_flags_fire() {
        // Energy almost entirely in the low band → "dark".
        let dark = [1.0, 0.02, 0.01, 0.005];
        let flags = tonal_flags(&dark);
        assert!(flags.iter().any(|f| f.contains("dark")), "{flags:?}");
        // Bright/harsh.
        let harsh = [0.05, 0.05, 0.1, 1.0];
        assert!(tonal_flags(&harsh).iter().any(|f| f.contains("harsh")));
        // Balanced → no flags.
        assert!(tonal_flags(&[1.0, 1.0, 1.0, 1.0]).is_empty());
    }

    // AC — EQ-match deltas reduce the spectral distance.
    #[test]
    fn eqmatch_deltas_reduce_distance() {
        let source = [1.0, 0.5, 0.25, 0.1];
        let reference = [0.8, 0.8, 0.8, 0.8];
        let before = spectral_distance(&source, &reference);
        let deltas = eq_match_deltas(&source, &reference);
        let matched = apply_deltas(&source, &deltas);
        let after = spectral_distance(&matched, &reference);
        assert!(after < before, "after={after} before={before}");
        assert!(
            after < 1e-6,
            "applying full match deltas reaches the reference"
        );
    }

    // AC — best-SIC ranks by distance (nearest first).
    #[test]
    fn best_sic_ranks_by_distance() {
        let target = [1.0, 0.8, 0.6, 0.4];
        let candidates = vec![
            ("far".to_string(), vec![0.1, 0.1, 0.1, 1.0]),
            ("near".to_string(), vec![1.0, 0.8, 0.6, 0.45]),
            ("mid".to_string(), vec![0.8, 0.6, 0.5, 0.3]),
        ];
        let ranked = rank_sics(&target, &candidates);
        assert_eq!(ranked[0].sicid, "near");
        assert!(ranked[0].distance < ranked[1].distance);
    }
}
