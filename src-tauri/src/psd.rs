//! Windowed Welch power-spectral-density estimator.
//!
//! The tone-diagnosis foundation: a LOW-VARIANCE spectral estimate to replace the
//! noisy single-bin Goertzel probes in [`crate::spectrum::band_energies`] (4 un-windowed
//! probes per band → ~±2.5 dB variance + heavy HF spectral leakage). This computes a
//! Hann-windowed, 50%-overlap Welch periodogram average — the variance drops with the
//! segment count and the Hann taper kills the leakage that made a pure tone bleed across
//! bands.
//!
//! PURELY ADDITIVE for now: nothing in production wires this in yet (the Goertzel stays
//! for the spectrum/EQ-match/best-SIC feature). Reachable via the unit tests below; a
//! production consumers are `doctor::body_psd` + `SoundProfile::from_capture_with_psd`.

use realfft::RealFftPlanner;

/// Welch segment length (a power of two so realfft picks its fast even path). Short
/// signals fall back to the largest power of two that fits.
const SEG: usize = 8192;

/// A one-sided power spectral density.
///
/// `psd[k]` is the density at frequency `k * bin_hz` for `k` in `0..=seg/2`, in
/// units of signal-power per Hz. Integrating it over frequency (`Σ psd[k]·bin_hz`)
/// recovers the signal's mean-square power (Parseval).
#[derive(Debug, Clone)]
pub struct Psd {
    /// One-sided PSD, bins `0..=seg/2`.
    pub psd: Vec<f64>,
    /// Frequency spacing between adjacent bins, `rate / seg`.
    pub bin_hz: f64,
    /// The sample rate the estimate was computed at.
    pub rate: f32,
}

/// Largest power of two ≤ `n` (0 for `n == 0`).
fn largest_pow2_le(n: usize) -> usize {
    if n == 0 {
        0
    } else {
        1usize << n.ilog2()
    }
}

/// Estimate the one-sided PSD of `signal` via the Welch method.
///
/// Hann-windowed segments of length [`SEG`] (or the largest power of two ≤ `signal.len()`
/// for short signals), 50% overlap, averaging the per-segment periodograms.
///
/// **Normalization** (matches scipy's `welch(..., scaling='density')`): each periodogram
/// is `|X[k]|² / (rate · S)` where `S = Σ w[n]²` is the Hann window's power gain, then the
/// bins `1..seg/2` are doubled for the one-sided fold (DC and Nyquist are not). With this
/// scaling `Σ psd[k]·bin_hz` recovers the signal's mean-square power (Parseval), and the
/// PSD is independent of the segment length and window.
pub fn welch_psd(signal: &[f32], rate: f32) -> Psd {
    // Segment length: SEG, else the largest power of two that fits. Need ≥2 for a real FFT.
    let seg = if signal.len() >= SEG {
        SEG
    } else {
        largest_pow2_le(signal.len())
    };
    let bins = seg / 2 + 1;
    if seg < 2 || rate <= 0.0 {
        // Degenerate input (empty / single sample) — a well-formed but empty estimate.
        return Psd {
            psd: vec![0.0; bins],
            bin_hz: 0.0,
            rate,
        };
    }

    // Periodic ("DFT-even") Hann window and its power gain S = Σ w².
    let window: Vec<f64> = (0..seg)
        .map(|n| 0.5 * (1.0 - (2.0 * std::f64::consts::PI * n as f64 / seg as f64).cos()))
        .collect();
    let s: f64 = window.iter().map(|w| w * w).sum();

    let mut planner = RealFftPlanner::<f64>::new();
    let r2c = planner.plan_fft_forward(seg);
    let mut scratch_in = r2c.make_input_vec();
    let mut spectrum = r2c.make_output_vec();

    let hop = seg / 2; // 50% overlap
    let mut acc = vec![0.0f64; bins];
    let mut segments = 0usize;
    let mut start = 0usize;
    while start + seg <= signal.len() {
        for (dst, (&x, w)) in scratch_in
            .iter_mut()
            .zip(signal[start..start + seg].iter().zip(&window))
        {
            *dst = f64::from(x) * w;
        }
        // Only errors on a length mismatch, which we control; skip the segment if so.
        if r2c.process(&mut scratch_in, &mut spectrum).is_ok() {
            for (a, c) in acc.iter_mut().zip(&spectrum) {
                *a += c.norm_sqr();
            }
            segments += 1;
        }
        start += hop;
    }

    let bin_hz = f64::from(rate) / seg as f64;
    if segments == 0 {
        return Psd {
            psd: vec![0.0; bins],
            bin_hz,
            rate,
        };
    }

    // Average across segments, apply the density normalization, and one-sided doubling.
    let denom = f64::from(rate) * s * segments as f64;
    let psd: Vec<f64> = acc
        .iter()
        .enumerate()
        .map(|(k, &p)| {
            let one_sided = if k == 0 || k == seg / 2 { 1.0 } else { 2.0 };
            one_sided * p / denom
        })
        .collect();

    Psd { psd, bin_hz, rate }
}

impl Psd {
    /// Centre frequency of bin `k`.
    fn freq(&self, k: usize) -> f64 {
        k as f64 * self.bin_hz
    }

    /// The `(freq, psd)` pairs whose bin centre frequency falls in the inclusive
    /// `[lo, hi]` range — the one range-walk shared by [`Psd::band_power`] and
    /// [`Psd::flatness`].
    fn bins_in(&self, lo: f64, hi: f64) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.psd.iter().enumerate().filter_map(move |(k, &p)| {
            let f = self.freq(k);
            (f >= lo && f <= hi).then_some((f, p))
        })
    }

    /// True band power over `[lo, hi]` = `Σ psd[bin]·bin_hz` for bins whose centre
    /// frequency falls in the (inclusive) range.
    pub fn band_power(&self, lo: f32, hi: f32) -> f64 {
        if self.bin_hz <= 0.0 {
            return 0.0;
        }
        self.bins_in(f64::from(lo), f64::from(hi))
            .map(|(_, p)| p * self.bin_hz)
            .sum()
    }

    /// [`Psd::band_power`] for each `(lo, hi)` band.
    pub fn band_powers(&self, bands: &[(f32, f32)]) -> Vec<f64> {
        bands
            .iter()
            .map(|&(lo, hi)| self.band_power(lo, hi))
            .collect()
    }

    /// Spectral flatness (SFM) over `[lo, hi]` = geometric-mean / arithmetic-mean of the
    /// in-range PSD bins, in `[0, 1]` (1 = perfectly flat, →0 = tonal).
    pub fn flatness(&self, lo: f32, hi: f32) -> f64 {
        let mut sum_ln = 0.0;
        let mut sum = 0.0;
        let mut count = 0usize;
        for (_, p) in self.bins_in(f64::from(lo), f64::from(hi)) {
            // Floor to keep ln finite; AM-GM keeps the ratio in [0, 1].
            let pf = p.max(1e-30);
            sum_ln += pf.ln();
            sum += pf;
            count += 1;
        }
        if count == 0 || sum <= 0.0 {
            return 0.0;
        }
        let geo = (sum_ln / count as f64).exp();
        let arith = sum / count as f64;
        (geo / arith).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    /// Deterministic LCG (Numerical Recipes constants) so every noise test is
    /// bit-reproducible — no unseeded randomness.
    struct Lcg(u64);
    impl Lcg {
        fn new(seed: u64) -> Self {
            Lcg(seed)
        }
        fn next_unit(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            // Top 53 bits → uniform in [0, 1).
            ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
        }
        fn next_bipolar(&mut self) -> f32 {
            (self.next_unit() * 2.0 - 1.0) as f32
        }
    }

    fn white_noise(n: usize, seed: u64) -> Vec<f32> {
        let mut rng = Lcg::new(seed);
        (0..n).map(|_| rng.next_bipolar()).collect()
    }

    fn sine(freq: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / RATE).sin())
            .collect()
    }

    fn mean_square(x: &[f32]) -> f64 {
        if x.is_empty() {
            return 0.0;
        }
        x.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / x.len() as f64
    }

    fn db(x: f64) -> f64 {
        10.0 * x.max(1e-30).log10()
    }

    // White noise → a high flatness (near-flat spectrum).
    #[test]
    fn white_noise_flat() {
        let psd = welch_psd(&white_noise(96_000, 1), RATE);
        let f = psd.flatness(50.0, 12_000.0);
        assert!(f >= 0.5, "white flatness {f} should be ≥ 0.5");
    }

    // Pure 2 kHz tone → its band dominates and an octave-away band is ≥40 dB lower.
    // This is the windowing win the un-windowed Goertzel can't make.
    #[test]
    fn tone_band_dominates_no_leak() {
        let psd = welch_psd(&sine(2_000.0, 1.0, 96_000), RATE);
        let in_band = psd.band_power(1_500.0, 2_500.0);
        let octave_away = psd.band_power(3_500.0, 5_500.0);
        assert!(in_band > 0.0, "in-band power must be positive");
        let ratio_db = db(in_band) - db(octave_away);
        assert!(
            ratio_db >= 40.0,
            "octave-away band should be ≥40 dB down, got {ratio_db:.1} dB"
        );
    }

    // Parseval: Σ psd·bin_hz ≈ mean-square of the signal (white noise + a tone).
    #[test]
    fn parseval_white() {
        let sig = white_noise(96_000, 3);
        let psd = welch_psd(&sig, RATE);
        let integral: f64 = psd.psd.iter().map(|&p| p * psd.bin_hz).sum();
        let ms = mean_square(&sig);
        let rel = (integral - ms).abs() / ms;
        assert!(
            rel <= 0.10,
            "Parseval white: integral {integral} vs ms {ms} (rel {rel})"
        );
    }

    #[test]
    fn parseval_tone() {
        let sig = sine(2_000.0, 0.5, 96_000);
        let psd = welch_psd(&sig, RATE);
        let integral: f64 = psd.psd.iter().map(|&p| p * psd.bin_hz).sum();
        let ms = mean_square(&sig); // ≈ amp²/2 = 0.125
        let rel = (integral - ms).abs() / ms;
        assert!(
            rel <= 0.10,
            "Parseval tone: integral {integral} vs ms {ms} (rel {rel})"
        );
    }

    // SFM: a pure tone is far from flat; white noise is near-flat.
    #[test]
    fn flatness_tone_vs_white() {
        let tf = welch_psd(&sine(2_000.0, 1.0, 96_000), RATE).flatness(50.0, 12_000.0);
        assert!(tf < 0.1, "tone flatness {tf} should be ≪ 0.1");
        let wf = welch_psd(&white_noise(96_000, 5), RATE).flatness(50.0, 12_000.0);
        assert!(wf >= 0.5, "white flatness {wf} should be ≥ 0.5");
    }

    // band_powers matches band_power element-wise.
    #[test]
    fn band_powers_matches_scalar() {
        let psd = welch_psd(&white_noise(48_000, 9), RATE);
        let bands = [(60.0, 250.0), (250.0, 1_000.0), (1_000.0, 4_000.0)];
        let batched = psd.band_powers(&bands);
        assert_eq!(batched.len(), bands.len());
        for (i, &(lo, hi)) in bands.iter().enumerate() {
            assert_eq!(batched[i], psd.band_power(lo, hi));
        }
    }

    // Pure impl → the same input yields a bit-identical PSD twice.
    #[test]
    fn determinism() {
        let sig = white_noise(50_000, 11);
        let a = welch_psd(&sig, RATE);
        let b = welch_psd(&sig, RATE);
        assert_eq!(a.psd, b.psd, "welch_psd must be deterministic");
        assert_eq!(a.bin_hz, b.bin_hz);
    }

    // Shape: full-length signal → SEG/2+1 bins at the expected spacing.
    #[test]
    fn psd_shape() {
        let psd = welch_psd(&white_noise(96_000, 13), RATE);
        assert_eq!(psd.psd.len(), SEG / 2 + 1);
        assert!((psd.bin_hz - f64::from(RATE) / SEG as f64).abs() < 1e-9);
    }

    // Edge cases: no panic, finite/sane output.
    #[test]
    fn edge_empty() {
        let psd = welch_psd(&[], RATE);
        assert!(psd.psd.iter().all(|p| p.is_finite()));
        assert!(psd.band_power(100.0, 200.0).is_finite());
        assert!(psd.flatness(50.0, 12_000.0).is_finite());
    }

    #[test]
    fn edge_shorter_than_segment() {
        // 1000 samples → largest power of two ≤ 1000 is 512.
        let psd = welch_psd(&white_noise(1_000, 17), RATE);
        assert_eq!(psd.psd.len(), 512 / 2 + 1);
        assert!(psd.psd.iter().all(|p| p.is_finite() && *p >= 0.0));
    }

    #[test]
    fn edge_all_zeros() {
        let psd = welch_psd(&vec![0.0; 20_000], RATE);
        assert!(psd.psd.iter().all(|p| p.is_finite()));
        assert!(psd.psd.iter().all(|&p| p == 0.0), "zeros → zero PSD");
        assert_eq!(psd.band_power(100.0, 4_000.0), 0.0);
        assert!(psd.flatness(50.0, 12_000.0).is_finite());
    }
}
