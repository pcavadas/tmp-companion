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
//! later stage swaps `SoundProfile::from_capture` onto it and rewrites the dependent rules.

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
    /// `[lo, hi]` range — the one range-walk shared by [`Psd::band_power`],
    /// [`Psd::tilt`], and [`Psd::flatness`].
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

    /// Spectral tilt: the least-squares slope of `10·log10(psd)` vs `log2(freq)` over
    /// ~50 Hz–12 kHz, in **dB per octave** (one octave = one unit of `log2(freq)`).
    pub fn tilt(&self) -> f64 {
        let pts: Vec<(f64, f64)> = self
            .bins_in(50.0, 12_000.0)
            .filter(|&(_, p)| p > 0.0)
            .map(|(f, p)| (f.log2(), 10.0 * p.log10()))
            .collect();
        least_squares_fit(&pts).0
    }

    /// Power-weighted spectral centroid, in Hz.
    pub fn centroid(&self) -> f64 {
        let mut num = 0.0;
        let mut den = 0.0;
        for (k, &p) in self.psd.iter().enumerate() {
            num += self.freq(k) * p;
            den += p;
        }
        if den > 0.0 {
            num / den
        } else {
            0.0
        }
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

/// Ordinary-least-squares fit of `y` vs `x`: returns `(slope, intercept)`,
/// `(0.0, 0.0)` if under-determined; `slope` is `0.0` if `x` is constant.
/// Shared by [`Psd::tilt`] and [`crate::doctor::tilt_residuals`] — one
/// regression, two callers.
pub(crate) fn least_squares_fit(pts: &[(f64, f64)]) -> (f64, f64) {
    if pts.len() < 2 {
        return (0.0, 0.0);
    }
    let n = pts.len() as f64;
    let mean_x = pts.iter().map(|p| p.0).sum::<f64>() / n;
    let mean_y = pts.iter().map(|p| p.1).sum::<f64>() / n;
    let mut sxy = 0.0;
    let mut sxx = 0.0;
    for &(x, y) in pts {
        sxy += (x - mean_x) * (y - mean_y);
        sxx += (x - mean_x) * (x - mean_x);
    }
    let slope = if sxx > 0.0 { sxy / sxx } else { 0.0 };
    (slope, mean_y - slope * mean_x)
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

    /// Paul Kellet's refined pink-noise filter (accurate to ±0.05 dB above ~9 Hz):
    /// shapes white noise to a -3 dB/oct (1/f power) spectrum.
    fn pink_noise(n: usize, seed: u64) -> Vec<f32> {
        let mut rng = Lcg::new(seed);
        let (mut b0, mut b1, mut b2, mut b3, mut b4, mut b5, mut b6) =
            (0.0f64, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        (0..n)
            .map(|_| {
                let w = f64::from(rng.next_bipolar());
                b0 = 0.998_86 * b0 + w * 0.055_517_9;
                b1 = 0.993_32 * b1 + w * 0.075_075_9;
                b2 = 0.969_00 * b2 + w * 0.153_852_0;
                b3 = 0.866_50 * b3 + w * 0.310_485_6;
                b4 = 0.550_00 * b4 + w * 0.532_952_2;
                b5 = -0.761_6 * b5 - w * 0.016_898_0;
                let pink = b0 + b1 + b2 + b3 + b4 + b5 + b6 + w * 0.536_2;
                b6 = w * 0.115_926;
                (pink * 0.11) as f32
            })
            .collect()
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

    // White noise → flat spectrum: tilt ≈ 0 and a high flatness.
    #[test]
    fn white_noise_flat() {
        let psd = welch_psd(&white_noise(96_000, 1), RATE);
        assert!(psd.tilt().abs() <= 1.0, "white tilt {} !≈ 0", psd.tilt());
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

    // Pink (1/f) noise → tilt ≈ -3 dB/oct.
    #[test]
    fn pink_noise_tilt() {
        let psd = welch_psd(&pink_noise(192_000, 7), RATE);
        let t = psd.tilt();
        assert!((t - (-3.0)).abs() <= 1.0, "pink tilt {t} !≈ -3 dB/oct");
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

    // Centroid ordering: a low tone → low centroid, a high tone → high centroid.
    #[test]
    fn centroid_ordering() {
        let low = welch_psd(&sine(200.0, 1.0, 96_000), RATE).centroid();
        let high = welch_psd(&sine(5_000.0, 1.0, 96_000), RATE).centroid();
        assert!(
            low < high,
            "centroid ordering: 200 Hz {low} vs 5 kHz {high}"
        );
        assert!(low < 1_000.0, "200 Hz tone centroid {low} should be low");
        assert!(high > 3_000.0, "5 kHz tone centroid {high} should be high");
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
        assert!(psd.tilt().is_finite());
        assert!(psd.centroid().is_finite());
        assert!(psd.flatness(50.0, 12_000.0).is_finite());
    }

    #[test]
    fn edge_shorter_than_segment() {
        // 1000 samples → largest power of two ≤ 1000 is 512.
        let psd = welch_psd(&white_noise(1_000, 17), RATE);
        assert_eq!(psd.psd.len(), 512 / 2 + 1);
        assert!(psd.psd.iter().all(|p| p.is_finite() && *p >= 0.0));
        assert!(psd.tilt().is_finite());
        assert!(psd.centroid().is_finite());
    }

    #[test]
    fn edge_all_zeros() {
        let psd = welch_psd(&vec![0.0; 20_000], RATE);
        assert!(psd.psd.iter().all(|p| p.is_finite()));
        assert!(psd.psd.iter().all(|&p| p == 0.0), "zeros → zero PSD");
        assert_eq!(psd.band_power(100.0, 4_000.0), 0.0);
        assert_eq!(psd.centroid(), 0.0);
        assert!(psd.tilt().is_finite());
        assert!(psd.flatness(50.0, 12_000.0).is_finite());
    }
}
