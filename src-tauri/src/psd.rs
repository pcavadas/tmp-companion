//! Windowed Welch power-spectral-density estimator.
//!
//! The tone-diagnosis foundation: a LOW-VARIANCE spectral estimate that replaced the
//! noisy single-bin Goertzel probes formerly in [`crate::spectrum::band_energies`] (4
//! un-windowed probes per band → ~±2.5 dB variance + heavy HF spectral leakage). This
//! computes a Hann-windowed, 50%-overlap Welch periodogram average — the variance drops
//! with the segment count and the Hann taper kills the leakage that made a pure tone
//! bleed across bands.
//!
//! Production consumers: `doctor::body_psd` + `SoundProfile::from_capture_with_psd`
//! (the Doctor), and [`crate::spectrum::band_energies`] (the spectrum-report/EQ-match/
//! best-SIC feature — the Goertzel probes it used are gone, see that module's doc).

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

    /// log-power (`10·log10(psd)`, floored like [`Psd::flatness`]) for every bin.
    fn log_psd(&self) -> Vec<f64> {
        self.psd
            .iter()
            .map(|&p| 10.0 * p.max(1e-30).log10())
            .collect()
    }

    /// A ONE-OCTAVE MEDIAN-smoothed envelope of a dB `series` (one value per
    /// bin) for bins
    /// `from..=to` ONLY (the rest of the returned full-length vec stays at a
    /// `NEG_INFINITY` sentinel — computing the whole spectrum wasted ~9× the
    /// sort work on bins [`Psd::find_peaks`] never reads, with the costliest
    /// windows at the unused near-Nyquist end): for the bin at frequency `f`,
    /// the median of `log_psd` over `[f/2^(1/2), f·2^(1/2)]` (± 1/2 octave =
    /// 1 octave total). Median (not mean) so the envelope doesn't follow a
    /// peak into its own window — and the window must be ~2× WIDER than the
    /// widest peak the detector should see: the original ±1/6-octave window
    /// tracked a real cocked-wah resonance (≈1/3–1/2 octave wide, +15.7 dB of
    /// band-local prominence on HW) so closely that its excess read ≈0 and
    /// the peak was INVISIBLE (HW `probe --doctor-inject --block
    /// ACD_CryBabyGCB95`, 2026-07-16). A median over a monotonic stretch is
    /// ~the centre value, so broad rolloffs (the cab knee) still null out —
    /// only convex bumps gain excess. Bin 0 (DC, freq 0) borrows bin 1's
    /// frequency for its window so it isn't a degenerate single-bin "median".
    fn octave_envelope(&self, series: &[f64], from: usize, to: usize) -> Vec<f64> {
        const HALF_WINDOW_OCT: f64 = 1.0 / 2.0; // half-window exponent (2^(1/2) each side)
        let n = series.len();
        let mut env = vec![f64::NEG_INFINITY; n];
        let mut window = Vec::new();
        for (k, slot) in env.iter_mut().enumerate().take(to + 1).skip(from) {
            let f = if k == 0 { self.bin_hz } else { self.freq(k) };
            let lo_bin = ((f / 2f64.powf(HALF_WINDOW_OCT)) / self.bin_hz)
                .floor()
                .max(0.0) as usize;
            let hi_bin =
                (((f * 2f64.powf(HALF_WINDOW_OCT)) / self.bin_hz).ceil() as usize).min(n - 1);
            window.clear();
            window.extend_from_slice(&series[lo_bin..=hi_bin]);
            window.sort_by(f64::total_cmp);
            let mid = window.len() / 2;
            *slot = if window.len() % 2 == 1 {
                window[mid]
            } else {
                (window[mid - 1] + window[mid]) / 2.0
            };
        }
        env
    }

    /// Quality factor of the peak at `excess[peak_bin]`: center frequency
    /// over an estimated −3 dB width from a least-squares PARABOLA fit of
    /// `excess` over ±20 bins around the peak (`y ≈ h + c·x²` on the symmetric
    /// window; width = `2·√(3/−c)` bins). A walk to the first −3 dB crossing
    /// is hopelessly noise-bound: the Welch estimate's per-bin scatter (plus
    /// the capture/stimulus cross-term in transfer space) creates spurious
    /// crossings within a few bins of a genuinely 300 Hz-wide bump's top and
    /// read Q in the hundreds for a true Q≈9 resonance. The fit averages 41
    /// bins: a one-bin comb line has huge curvature (Q ≫ any ceiling), an
    /// octave-wide EQ bump has almost none (Q ≈ 1), and a wah-like resonance
    /// lands near its physical Q (the quadratic under-reads a Lorentzian's
    /// width by ~×0.83 — well inside the gate margins). Non-concave fits
    /// (c ≥ 0, a plateau/shoulder) report Q = 0 (maximally wide).
    fn peak_q(&self, excess: &[f64], peak_bin: usize) -> f64 {
        let n = excess.len();
        // Symmetric window so the odd fit terms vanish; clamp at the edges
        // (an edge-clipped peak just fits the width it can see).
        let half = 20.min(peak_bin).min(n - 1 - peak_bin);
        if half == 0 {
            return 0.0;
        }
        let (mut sy, mut sx2y, mut sx2, mut sx4, mut count) =
            (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for dx in -(half as i64)..=(half as i64) {
            let y = excess[(peak_bin as i64 + dx) as usize];
            if !y.is_finite() {
                continue; // apron sentinel — skip, the fit stays symmetric enough
            }
            let x2 = (dx * dx) as f64;
            sy += y;
            sx2y += x2 * y;
            sx2 += x2;
            sx4 += x2 * x2;
            count += 1.0;
        }
        let denom = sx4 - sx2 * sx2 / count;
        if denom <= 0.0 {
            return 0.0;
        }
        let c = (sx2y - sx2 * sy / count) / denom;
        if c >= -1e-9 {
            return 0.0; // flat or concave-up: no measurable narrowness
        }
        let width_bins = 2.0 * (3.0 / -c).sqrt();
        let width_hz = width_bins * self.bin_hz;
        if width_hz <= 0.0 {
            return 0.0;
        }
        self.freq(peak_bin) / width_hz
    }

    /// The chain's TRANSFER magnitude in dB per bin: this capture's log-PSD
    /// minus the STIMULUS's (`10·log10(capture/stimulus)`). The stimulus's own
    /// fine structure (the deterministic shaped-noise ridges) is present in
    /// both and cancels exactly, so a localized peak OF THE TRANSFER is a
    /// chain resonance, never a stimulus artifact — raw capture-space peak
    /// detection tripped on those ridges (HW 2026-07-16: `resonant` fired on
    /// 25/25 clean factory presets under a one-octave envelope; the ridges
    /// measured h≈12 dB on a CLEAN chain). `None` when the two grids differ
    /// (segment length / bin spacing) — callers treat that as "no
    /// localization".
    pub fn transfer_db(&self, stimulus: &Psd) -> Option<Vec<f64>> {
        if self.psd.len() != stimulus.psd.len() || self.bin_hz != stimulus.bin_hz {
            return None;
        }
        Some(
            self.log_psd()
                .iter()
                .zip(stimulus.log_psd())
                .map(|(c, s)| c - s)
                .collect(),
        )
    }

    /// Find localized spectral peaks of an arbitrary dB `series` on this PSD's
    /// bin grid (one value per bin — the production caller passes
    /// [`Psd::transfer_db`]; [`Psd::find_peaks`] passes the raw log-PSD):
    /// `series` minus a [`Psd::octave_envelope`] (so the envelope doesn't
    /// follow the peak itself), contiguous bins whose excess clears
    /// `min_height_db` become ONE peak (center = the run's maximum bin,
    /// height = its excess, Q = the −3 dB width around that bin, walked past
    /// the run and clamped at a one-octave apron outside `[lo_hz, hi_hz]` —
    /// the envelope is only computed there, and any peak whose −3 dB width
    /// exceeds an octave is Q ≲ 1.5, far below any Q gate, so the clamp can't
    /// change a verdict). Sorted highest-first. Empty on a degenerate PSD, an
    /// empty/inverted range, or a `series` that doesn't match the bin grid.
    pub fn find_peaks_in_db(
        &self,
        series: &[f64],
        lo_hz: f64,
        hi_hz: f64,
        min_height_db: f64,
    ) -> Vec<SpectralPeak> {
        let n = self.psd.len();
        if n == 0 || series.len() != n || self.bin_hz <= 0.0 || lo_hz > hi_hz {
            return Vec::new();
        }
        let lo_bin = (lo_hz / self.bin_hz).ceil().max(0.0) as usize;
        let hi_bin = ((hi_hz / self.bin_hz).floor() as usize).min(n - 1);
        if lo_bin > hi_bin {
            return Vec::new();
        }
        // One-octave apron each side: `peak_q`'s −3 dB walk may step outside
        // the scan range; past the apron the sentinel −∞ excess stops it.
        let walk_lo = lo_bin / 2;
        let walk_hi = (hi_bin * 2).min(n - 1);
        let envelope = self.octave_envelope(series, walk_lo, walk_hi);
        // −∞ outside the apron (a −∞ envelope would make excess +∞ there).
        let excess: Vec<f64> = series
            .iter()
            .zip(&envelope)
            .map(|(&l, &e)| {
                if e.is_finite() {
                    l - e
                } else {
                    f64::NEG_INFINITY
                }
            })
            .collect();

        let mut peaks = Vec::new();
        let mut k = lo_bin;
        while k <= hi_bin {
            if excess[k] >= min_height_db {
                let start = k;
                let mut end = k;
                while end < hi_bin && excess[end + 1] >= min_height_db {
                    end += 1;
                }
                let peak_bin = (start..=end)
                    .max_by(|&a, &b| excess[a].total_cmp(&excess[b]))
                    .unwrap_or(start);
                // Two independent narrowness reads, combined pessimistically
                // (NARROWER wins): the parabola fit measures a smooth bump's
                // true bandwidth but flattens a one-bin comb line over its
                // wide window, while the above-floor RUN width nails the comb
                // line (1–3 bins) but over-reads a tall bump (its skirt stays
                // above the floor far past −3 dB). max() lets either signal
                // veto a fake "wide" reading — a spike can't hide behind a
                // flat fit, a real resonance isn't penalized by its skirt.
                let run_q = self.freq(peak_bin) / ((end - start + 1) as f64 * self.bin_hz);
                peaks.push(SpectralPeak {
                    freq_hz: self.freq(peak_bin),
                    height_db: excess[peak_bin],
                    q: self.peak_q(&excess, peak_bin).max(run_q),
                });
                k = end + 1;
            } else {
                k += 1;
            }
        }
        peaks.sort_by(|a, b| b.height_db.total_cmp(&a.height_db));
        peaks
    }

    /// [`Psd::find_peaks_in_db`] over this PSD's OWN log-power — peaks vs the
    /// spectrum's own envelope (kept for the unit tests / any caller without a
    /// stimulus reference; production localization uses the transfer, see
    /// [`Psd::transfer_db`]).
    pub fn find_peaks(&self, lo_hz: f64, hi_hz: f64, min_height_db: f64) -> Vec<SpectralPeak> {
        self.find_peaks_in_db(&self.log_psd(), lo_hz, hi_hz, min_height_db)
    }
}

/// A localized spectral peak found by [`Psd::find_peaks`]: center frequency,
/// height above the smoothed spectral envelope (dB), and quality factor
/// (center / the −3 dB-from-peak width).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpectralPeak {
    pub freq_hz: f64,
    pub height_db: f64,
    pub q: f64,
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

    // ── find_peaks ──────────────────────────────────────────────────────────

    fn add(a: &[f32], b: &[f32]) -> Vec<f32> {
        a.iter().zip(b).map(|(&x, &y)| x + y).collect()
    }

    fn scale(a: &[f32], g: f32) -> Vec<f32> {
        a.iter().map(|&x| x * g).collect()
    }

    // Broadband noise + one strong sine → exactly one peak, at the sine's
    // frequency within one bin, with a plausible (> 1) Q.
    #[test]
    fn find_peaks_locates_one_strong_sine() {
        let noise = scale(&white_noise(96_000, 21), 0.05);
        let sig = add(&noise, &sine(2_000.0, 0.8, 96_000));
        let psd = welch_psd(&sig, RATE);
        let peaks = psd.find_peaks(200.0, 8_000.0, 6.0);
        assert_eq!(peaks.len(), 1, "expected exactly one peak, got {peaks:?}");
        let p = peaks[0];
        assert!(
            (p.freq_hz - 2_000.0).abs() <= psd.bin_hz + 1e-6,
            "peak at {} Hz, expected ≈2000 Hz (bin_hz={})",
            p.freq_hz,
            psd.bin_hz
        );
        assert!(p.height_db > 6.0, "height {}", p.height_db);
        assert!(p.q > 1.0, "plausible Q, got {}", p.q);
    }

    // Plain white noise → no peaks at a sane min_height (nothing localized
    // stands out of the smoothed envelope by 6 dB).
    #[test]
    fn find_peaks_plain_noise_finds_none() {
        let psd = welch_psd(&white_noise(96_000, 31), RATE);
        let peaks = psd.find_peaks(200.0, 8_000.0, 6.0);
        assert!(
            peaks.is_empty(),
            "white noise should have no 6 dB peaks, got {peaks:?}"
        );
    }

    // Two well-separated sines of different heights → two peaks, height-sorted
    // (the louder 5 kHz tone first).
    #[test]
    fn find_peaks_two_sines_height_sorted() {
        let noise = scale(&white_noise(96_000, 41), 0.02);
        let sig = add(
            &add(&noise, &sine(1_000.0, 0.3, 96_000)),
            &sine(5_000.0, 0.9, 96_000),
        );
        let psd = welch_psd(&sig, RATE);
        let peaks = psd.find_peaks(200.0, 8_000.0, 6.0);
        assert_eq!(peaks.len(), 2, "expected two peaks, got {peaks:?}");
        assert!(
            peaks[0].height_db >= peaks[1].height_db,
            "height-sorted highest-first: {peaks:?}"
        );
        assert!(
            (peaks[0].freq_hz - 5_000.0).abs() <= psd.bin_hz * 2.0,
            "louder peak should be the 5 kHz tone: {peaks:?}"
        );
        assert!(
            (peaks[1].freq_hz - 1_000.0).abs() <= psd.bin_hz * 2.0,
            "quieter peak should be the 1 kHz tone: {peaks:?}"
        );
    }

    // Degenerate ranges/PSDs never panic.
    #[test]
    fn find_peaks_edge_cases() {
        let psd = welch_psd(&white_noise(96_000, 51), RATE);
        assert!(psd.find_peaks(8_000.0, 200.0, 6.0).is_empty()); // inverted range
        assert!(psd.find_peaks(50_000.0, 60_000.0, 6.0).is_empty()); // past Nyquist
        let empty = welch_psd(&[], RATE);
        assert!(empty.find_peaks(200.0, 8_000.0, 6.0).is_empty());
    }
}
