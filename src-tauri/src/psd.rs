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

/// The flanks (octaves from the peak, each side) whose median levels set the
/// baseline [`Psd::peak_q`] measures a peak's height and width against. Far
/// enough out that a Q 2 bump has decayed to about 1 dB there, so the bump
/// does not lift its own baseline; a line through the two flanks (in log
/// frequency) absorbs a chain's broadband slope.
const PEAK_FLANK_OCT: (f64, f64) = (1.0, 1.5);

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
    welch_psd_seg(signal, rate, SEG)
}

/// Segment length for a NOISE-FLOOR estimate ([`welch_psd_floor`]).
const FLOOR_SEG: usize = 2048;

/// [`welch_psd`] with [`FLOOR_SEG`]-point segments, for a short noise-floor window.
///
/// The Doctor's pre-onset floor window is ~11,100 samples, which fits ONE 8192-point
/// segment: a single Hann periodogram (2 degrees of freedom per bin) put the per-band
/// floor estimate's scatter at roughly 1.5–2.5 dB in the narrow low bands, so a band
/// near the SNR margin flipped covered/uncovered from run to run. 2048-point segments
/// average ~9 overlapping periodograms over the same window. The density stays unbiased
/// (the normalization is segment-length independent); only resolution drops, which a
/// noise floor doesn't need. Compare its bands against a finer-grid PSD with
/// [`Psd::band_mean_density`], never [`Psd::band_power`], whose bin-centre rounding
/// differs between grids.
pub fn welch_psd_floor(signal: &[f32], rate: f32) -> Psd {
    welch_psd_seg(signal, rate, FLOOR_SEG)
}

/// The Welch estimator with segments of at most `max_seg` points.
fn welch_psd_seg(signal: &[f32], rate: f32, max_seg: usize) -> Psd {
    // Segment length: max_seg, else the largest power of two that fits. Need ≥2 for a
    // real FFT.
    let seg = if signal.len() >= max_seg {
        max_seg
    } else {
        largest_pow2_le(signal.len())
    };
    let bins = seg / 2 + 1;
    if seg < 2 || rate <= 0.0 {
        // Degenerate input (empty / single sample) — a well-formed but empty estimate.
        return Psd {
            psd: vec![0.0; bins],
            bin_hz: 0.0,
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

    Psd { psd, bin_hz }
}

impl Psd {
    /// Centre frequency of bin `k`.
    fn freq(&self, k: usize) -> f64 {
        k as f64 * self.bin_hz
    }

    /// The `(freq, psd)` pairs whose bin centre frequency falls in the half-open
    /// `[lo, hi)` range — the one range-walk shared by [`Psd::band_power`] and
    /// [`Psd::flatness`]. Half-open so adjacent bands sharing an edge that lands
    /// exactly on a bin (3 and 6 kHz at 48 kHz / 8192 points) count it once (#259).
    fn bins_in(&self, lo: f64, hi: f64) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.psd.iter().enumerate().filter_map(move |(k, &p)| {
            let f = self.freq(k);
            (f >= lo && f < hi).then_some((f, p))
        })
    }

    /// The power-domain mean of PSDs on one grid — how the Doctor combines the
    /// channels of a capture (#250). One PSD comes back unchanged; an empty list
    /// gives an empty PSD. Every PSD must share the first one's grid (the
    /// channels of one capture always do); a mismatched one is skipped.
    pub fn power_mean(mut psds: Vec<Psd>) -> Psd {
        if psds.len() <= 1 {
            return psds.pop().unwrap_or(Psd {
                psd: Vec::new(),
                bin_hz: 0.0,
            });
        }
        let first = psds.swap_remove(0);
        let mut sum = first.psd;
        let mut n = 1.0;
        for p in &psds {
            if p.psd.len() == sum.len() && p.bin_hz == first.bin_hz {
                for (s, v) in sum.iter_mut().zip(&p.psd) {
                    *s += v;
                }
                n += 1.0;
            }
        }
        for s in &mut sum {
            *s /= n;
        }
        Psd {
            psd: sum,
            bin_hz: first.bin_hz,
        }
    }

    /// True band power over `[lo, hi)` = `Σ psd[bin]·bin_hz` for bins whose centre
    /// frequency falls in the (half-open) range.
    pub fn band_power(&self, lo: f32, hi: f32) -> f64 {
        if self.bin_hz <= 0.0 {
            return 0.0;
        }
        self.bins_in(f64::from(lo), f64::from(hi))
            .map(|(_, p)| p * self.bin_hz)
            .sum()
    }

    /// Mean density over the bins whose centre falls in `[lo, hi)` (0 when none do).
    /// Unlike [`Psd::band_power`] it doesn't depend on how many bin centres the band
    /// edges happen to catch, so it compares bands across PSDs on DIFFERENT grids.
    /// On one grid the ratio of two PSDs' mean densities equals the ratio of their
    /// band powers.
    pub fn band_mean_density(&self, lo: f32, hi: f32) -> f64 {
        let (sum, count) = self
            .bins_in(f64::from(lo), f64::from(hi))
            .fold((0.0, 0usize), |(s, c), (_, p)| (s + p, c + 1));
        if count == 0 {
            0.0
        } else {
            sum / count as f64
        }
    }

    /// [`Psd::band_power`] for each `(lo, hi)` band.
    pub fn band_powers(&self, bands: &[(f32, f32)]) -> Vec<f64> {
        bands
            .iter()
            .map(|&(lo, hi)| self.band_power(lo, hi))
            .collect()
    }

    /// Spectral flatness (SFM) over `[lo, hi)` = geometric-mean / arithmetic-mean of the
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

    /// Quality factor of the peak at `series[peak_bin]`: center frequency over
    /// the width between the two frequencies where the peak falls to HALF its
    /// height in dB (#251). That is the RBJ definition of a peaking filter's
    /// bandwidth (between the `gain/2` dB points), for which
    /// `f0 / (f_hi − f_lo)` is exactly its Q, so the reading tracks true Q at
    /// any centre frequency and any peak height.
    ///
    /// Height here is measured against a baseline through the median levels
    /// of two flanks [`PEAK_FLANK_OCT`] away on each side (a line in log
    /// frequency), NOT against the one-octave detection envelope: that
    /// envelope rides a wide bump's own skirt, so it would read a Q 2 bump
    /// narrow. The reported `height_db` stays the envelope excess the gates
    /// were calibrated on.
    ///
    /// The old estimator read Q from a parabola fit over a FIXED ±20 bins
    /// (half an octave at 300 Hz, a twentieth at 3.5 kHz) and from the run
    /// width above the ABSOLUTE 3 dB detection floor: true Q 2–12 all read
    /// 4.6–7.5 at 300 Hz, Q 8 read 15.5 at 3.5 kHz, and a Q 2 bump read 6
    /// everywhere.
    ///
    /// The walk stops at the first of TWO consecutive bins at or below half
    /// height, so one bin of Welch scatter can't end it early, and the
    /// crossing is interpolated linearly between bins. A side with no
    /// crossing inside the flank is mirrored geometrically from the other
    /// side; with neither, the peak is wider than the flanks and reads as
    /// such. A non-positive height reads Q = 0.
    fn peak_q(&self, series: &[f64], peak_bin: usize) -> f64 {
        let n = series.len();
        let f0 = self.freq(peak_bin);
        if f0 <= 0.0 || self.bin_hz <= 0.0 || n < 3 {
            return 0.0;
        }
        let bin_at = |hz: f64| ((hz / self.bin_hz).round().max(1.0) as usize).min(n - 1);
        let (near, far) = PEAK_FLANK_OCT;
        let flank = |lo_hz: f64, hi_hz: f64| -> Option<(f64, f64)> {
            let (a, b) = (bin_at(lo_hz), bin_at(hi_hz));
            let vals: Vec<f64> = series[a.min(b)..=b.max(a)]
                .iter()
                .copied()
                .filter(|v| v.is_finite())
                .collect();
            if vals.is_empty() || lo_hz >= self.freq(n - 1) {
                return None;
            }
            let mut v = vals;
            v.sort_by(f64::total_cmp);
            let m = v.len() / 2;
            let med = if v.len() % 2 == 1 {
                v[m]
            } else {
                (v[m - 1] + v[m]) / 2.0
            };
            Some(((lo_hz * hi_hz).sqrt().log2(), med))
        };
        let left = flank(f0 / 2f64.powf(far), f0 / 2f64.powf(near));
        let right = flank(f0 * 2f64.powf(near), f0 * 2f64.powf(far));
        let baseline = |hz: f64| -> f64 {
            match (left, right) {
                (Some((xl, yl)), Some((xr, yr))) => yl + (yr - yl) * (hz.log2() - xl) / (xr - xl),
                (Some((_, y)), None) | (None, Some((_, y))) => y,
                (None, None) => 0.0,
            }
        };
        let bump = |k: usize| series[k] - baseline(self.freq(k).max(self.bin_hz));
        let top = bump(peak_bin);
        if top.is_nan() || top <= 0.0 {
            return 0.0;
        }
        let level = top / 2.0;
        let reach_lo = bin_at(f0 / 2f64.powf(near));
        let reach_hi = bin_at(f0 * 2f64.powf(near));
        // The interpolated frequency of the half-height crossing walking from
        // the peak in `step` (−1 or +1); `None` when the flank comes first.
        let edge = |step: i64| -> Option<f64> {
            let mut k = peak_bin as i64;
            loop {
                let next = k + step;
                let after = next + step;
                if next < reach_lo as i64 || next > reach_hi as i64 {
                    return None;
                }
                let v = bump(next as usize);
                if !v.is_finite() {
                    return None;
                }
                let confirmed = v <= level
                    && (after < 0
                        || after as usize >= n
                        || !bump(after as usize).is_finite()
                        || bump(after as usize) <= level);
                if confirmed {
                    let prev = bump(k as usize);
                    let frac = if prev > v {
                        (prev - level) / (prev - v)
                    } else {
                        0.0
                    };
                    let (fa, fb) = (self.freq(k as usize), self.freq(next as usize));
                    return Some(fa + frac.clamp(0.0, 1.0) * (fb - fa));
                }
                k = next;
            }
        };
        let (f_lo, f_hi) = match (edge(-1), edge(1)) {
            (Some(lo), Some(hi)) => (lo, hi),
            (Some(lo), None) => (lo, f0 * f0 / lo.max(f64::MIN_POSITIVE)),
            (None, Some(hi)) => (f0 * f0 / hi, hi),
            (None, None) => (f0 / 2f64.powf(near), f0 * 2f64.powf(near)),
        };
        let width_hz = f_hi - f_lo;
        if width_hz <= 0.0 {
            return 0.0;
        }
        f0 / width_hz
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

    /// [`Psd::transfer_db`] read only where the stimulus EXCITES the chain
    /// (#235). A pitched stimulus puts its energy in harmonic lines; between
    /// them the capture holds the chain's own noise, reverb and intermodulation
    /// products, so `capture − stimulus` there reads tall, narrow fake peaks
    /// that no chain owns (HW 2026-10-15: they fired `resonant` and `boxy` on
    /// healthy factory presets, at the same frequencies in every capture). A
    /// bin is observable when the stimulus's level there reaches its own
    /// one-octave median; every other bin's transfer is interpolated linearly
    /// between the nearest observable bins on each side. Only
    /// `lo_hz..=hi_hz`, widened by the flanks [`Psd::peak_q`] reads, is
    /// bridged; the rest is the plain transfer. On the 2026-10-15 raw
    /// captures this removed 5 of the 6 healthy `resonant` fires and 2 of the
    /// 5 `boxy` ones while both injected defects kept their height within
    /// 1.2 dB. `None` exactly when [`Psd::transfer_db`] is.
    pub fn observable_transfer_db(
        &self,
        stimulus: &Psd,
        lo_hz: f64,
        hi_hz: f64,
    ) -> Option<Vec<f64>> {
        let mut t = self.transfer_db(stimulus)?;
        let n = t.len();
        if n < 3 || self.bin_hz <= 0.0 || lo_hz > hi_hz {
            return Some(t);
        }
        let reach = 2f64.powf(PEAK_FLANK_OCT.1);
        let from = ((lo_hz / reach / self.bin_hz).floor().max(1.0) as usize).min(n - 1);
        let to = ((hi_hz * reach / self.bin_hz).ceil() as usize).min(n - 1);
        let s = stimulus.log_psd();
        let env = stimulus.octave_envelope(&s, from, to);
        let observable = |k: usize| k < from || k > to || s[k] >= env[k];
        let mut k = from;
        while k <= to {
            if observable(k) {
                k += 1;
                continue;
            }
            let start = k;
            while k <= to && !observable(k) {
                k += 1;
            }
            // Bin `start − 1` is observable (`from − 1` counts as such, being
            // outside the bridged range); `k` is the first observable bin after
            // the run, or one past the end.
            let left = start - 1;
            let (lv, rv) = (t[left], if k < n { t[k] } else { t[left] });
            let span = (k - left) as f64;
            for (i, v) in t.iter_mut().enumerate().take(k.min(n)).skip(start) {
                *v = lv + (rv - lv) * (i - left) as f64 / span;
            }
        }
        Some(t)
    }

    /// Find localized spectral peaks of an arbitrary dB `series` on this PSD's
    /// bin grid (one value per bin — the production caller passes
    /// [`Psd::transfer_db`]; [`Psd::find_peaks`] passes the raw log-PSD):
    /// `series` minus a [`Psd::octave_envelope`] (so the envelope doesn't
    /// follow the peak itself), contiguous bins whose excess clears
    /// `min_height_db` become ONE peak (center = the run's maximum bin,
    /// height = its excess, Q = [`Psd::peak_q`]'s half-height width; the
    /// envelope is computed only over the scan range plus that walk's reach). Sorted
    /// highest-first. Empty on a degenerate PSD, an
    /// empty/inverted range, or a `series` that doesn't match the bin grid.
    pub fn find_peaks_in_db(
        &self,
        series: &[f64],
        lo_hz: f64,
        hi_hz: f64,
        min_height_db: f64,
    ) -> Vec<SpectralPeak> {
        self.find_peaks_with_apron(series, lo_hz, hi_hz, min_height_db, fit_apron)
    }

    /// [`Psd::find_peaks_in_db`] with the envelope computed over `apron(lo_bin,
    /// hi_bin, n)`'s inclusive bin range — split out so a test can pin that a wider
    /// apron changes no output.
    fn find_peaks_with_apron(
        &self,
        series: &[f64],
        lo_hz: f64,
        hi_hz: f64,
        min_height_db: f64,
        apron: fn(usize, usize, usize) -> (usize, usize),
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
        let (walk_lo, walk_hi) = apron(lo_bin, hi_bin, n);
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
                // The half-height width reads a one-bin comb line as a
                // needle (one or two bins wide → Q in the tens to hundreds)
                // and a smooth bump at its own bandwidth, so no second,
                // floor-relative run width is combined in any more (#251).
                peaks.push(SpectralPeak {
                    freq_hz: self.freq(peak_bin),
                    height_db: excess[peak_bin],
                    q: self.peak_q(series, peak_bin),
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
    #[cfg(test)] // production detects peaks in TRANSFER space via `find_peaks_in_db`
    pub fn find_peaks(&self, lo_hz: f64, hi_hz: f64, min_height_db: f64) -> Vec<SpectralPeak> {
        self.find_peaks_in_db(&self.log_psd(), lo_hz, hi_hz, min_height_db)
    }
}

/// The envelope range [`Psd::find_peaks_in_db`] computes for scan bins
/// `lo_bin..=hi_bin` of `n`: the scan range itself. Only the detection reads
/// the envelope (`peak_q` measures against its own flanks on the raw series,
/// #251), and each bin's envelope depends only on the input series, so
/// computing more changes no output (#197 trimmed a one-octave apron for the
/// same reason).
fn fit_apron(lo_bin: usize, hi_bin: usize, _n: usize) -> (usize, usize) {
    (lo_bin, hi_bin)
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

    // BUG→GATE (#259): band edges that land exactly on a bin (3 and 6 kHz at
    // 48 kHz / 8192 points) were counted in both adjacent bands. Contiguous
    // half-open bands now sum to the full-range power, every bin once.
    #[test]
    fn contiguous_bands_count_every_bin_once() {
        let p = welch_psd(&white_noise(48_000, 9), RATE);
        let edges = [
            0.0f32, 250.0, 750.0, 1500.0, 3000.0, 6000.0, 12_000.0, 24_001.0,
        ];
        let bands: Vec<(f32, f32)> = edges.windows(2).map(|w| (w[0], w[1])).collect();
        let sum: f64 = p.band_powers(&bands).iter().sum();
        let full: f64 = p.psd.iter().map(|v| v * p.bin_hz).sum();
        assert!(
            (sum - full).abs() <= full * 1e-12,
            "bands {sum} vs full {full}"
        );
        assert_eq!(p.freq(512), 3000.0, "3 kHz must sit exactly on a bin");
    }

    // #250: the power mean keeps one PSD unchanged and leaves dual mono
    // bit-identical; it averages two different channels bin by bin.
    #[test]
    fn power_mean_is_exact_on_one_or_identical_channels() {
        let a = welch_psd(&white_noise(16_384, 3), RATE);
        let b = welch_psd(&white_noise(16_384, 4), RATE);
        assert_eq!(Psd::power_mean(vec![a.clone()]).psd, a.psd);
        assert_eq!(Psd::power_mean(vec![a.clone(), a.clone()]).psd, a.psd);
        let m = Psd::power_mean(vec![a.clone(), b.clone()]);
        for k in [10, 200, 3000] {
            assert!((m.psd[k] - (a.psd[k] + b.psd[k]) / 2.0).abs() <= 1e-15 * m.psd[k].max(1.0));
        }
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

    /// RBJ peaking biquad (`gain_db` at `f0`, quality `q`) run over `x`.
    fn peaking(x: &[f32], f0: f64, q: f64, gain_db: f64) -> Vec<f32> {
        let a = 10f64.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f64::consts::PI * f0 / f64::from(RATE);
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha / a;
        let b = [
            (1.0 + alpha * a) / a0,
            -2.0 * cos / a0,
            (1.0 - alpha * a) / a0,
        ];
        let fb = [-2.0 * cos / a0, (1.0 - alpha / a) / a0];
        let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        x.iter()
            .map(|&v| {
                let v = f64::from(v);
                let y = b[0] * v + b[1] * x1 + b[2] * x2 - fb[0] * y1 - fb[1] * y2;
                (x2, x1, y2, y1) = (x1, v, y1, y);
                y as f32
            })
            .collect()
    }

    // #251: 3 s of white noise through a +12 dB peaking filter, against the
    // clean noise (the production transfer path). The strongest peak's Q must
    // track the true Q within ±30% across the band the localized rules judge.
    #[test]
    fn peak_q_tracks_true_q_across_frequency() {
        let clean = scale(&white_noise(144_000, 251), 0.1);
        let stim = welch_psd(&clean, RATE);
        let mut worst = 0.0f64;
        for f0 in [300.0, 700.0, 2_000.0, 3_500.0] {
            for q in [2.0, 4.0, 8.0, 12.0] {
                let cap = welch_psd(&peaking(&clean, f0, q, 12.0), RATE);
                let transfer = cap.transfer_db(&stim).unwrap();
                let peaks = cap.find_peaks_in_db(&transfer, 200.0, 8_000.0, 3.0);
                let p = peaks.first().expect("a peak");
                assert!(
                    (p.freq_hz - f0).abs() < f0 * 0.1,
                    "{f0} Hz Q{q}: at {}",
                    p.freq_hz
                );
                let err = p.q / q - 1.0;
                worst = worst.max(err.abs());
                assert!(err.abs() <= 0.30, "{f0} Hz Q{q}: read q {:.2}", p.q);
            }
        }
        eprintln!("worst relative Q error {worst:.3}");
    }

    /// A stimulus with deep spectral dips: white noise through a feed-forward
    /// comb (`x[n] − 0.95·x[n − 37]`), notches every ≈1.3 kHz — the kind a
    /// plucked string's pick position cuts into a pitched stimulus.
    fn combed(n: usize) -> Vec<f32> {
        let x = scale(&white_noise(n, 2_350), 0.1);
        (0..n)
            .map(|i| x[i] - if i >= 37 { 0.95 * x[i - 37] } else { 0.0 })
            .collect()
    }

    // BUG→GATE (#235): where the stimulus dips, the capture holds mostly the
    // chain's own noise, so the plain transfer reads tall fake peaks there.
    // The observable transfer bridges those bins; a real peaking EQ survives.
    #[test]
    fn the_observable_transfer_ignores_the_stimulus_dips() {
        let clean = combed(144_000);
        let stim = welch_psd(&clean, RATE);
        let noisy = add(&clean, &scale(&white_noise(144_000, 235), 0.01));
        let cap = welch_psd(&noisy, RATE);
        let tallest = |t: &[f64]| {
            cap.find_peaks_in_db(t, 200.0, 4_000.0, 3.0)
                .first()
                .map_or(0.0, |p| p.height_db)
        };
        let plain = tallest(&cap.transfer_db(&stim).unwrap());
        let observable = tallest(&cap.observable_transfer_db(&stim, 200.0, 8_000.0).unwrap());
        assert!(
            plain > 7.5,
            "the plain transfer should show the dips: {plain:.1}"
        );
        assert!(
            observable < 3.0,
            "observable transfer peak {observable:.1} dB"
        );

        let ring = welch_psd(&peaking(&noisy, 2_000.0, 8.0, 12.0), RATE);
        let t = ring.observable_transfer_db(&stim, 200.0, 8_000.0).unwrap();
        let p = ring
            .find_peaks_in_db(&t, 200.0, 4_000.0, 3.0)
            .into_iter()
            .next()
            .expect("the ring");
        assert!(
            (p.freq_hz - 2_000.0).abs() < 200.0,
            "ring at {:.0} Hz",
            p.freq_hz
        );
        assert!(p.height_db > 7.5, "ring height {:.1} dB", p.height_db);
    }

    #[test]
    fn a_single_bin_line_still_reads_as_a_needle() {
        // The comb-line class the Q ceiling rejects: a pure tone over noise.
        let sig = add(
            &scale(&white_noise(144_000, 252), 0.05),
            &sine(2_500.0, 0.3, 144_000),
        );
        let psd = welch_psd(&sig, RATE);
        let p = psd.find_peaks(200.0, 8_000.0, 3.0)[0];
        assert!(p.q > 85.0, "needle read q {:.1}", p.q);
    }

    // #197 / #251: the envelope is computed over the scan range only. Each bin's
    // envelope depends only on the input series, so the peaks must be
    // bit-identical to a one-octave apron's — including a peak at each edge of
    // the scan range.
    #[test]
    fn trimmed_apron_finds_bit_identical_peaks() {
        fn octave_apron(lo_bin: usize, hi_bin: usize, n: usize) -> (usize, usize) {
            (lo_bin / 2, (hi_bin * 2).min(n - 1))
        }
        let noise = scale(&white_noise(96_000, 61), 0.05);
        let sig = [210.0, 1_000.0, 2_800.0, 7_900.0]
            .iter()
            .fold(noise, |acc, &f| add(&acc, &sine(f, 0.4, 96_000)));
        let psd = welch_psd(&sig, RATE);
        let series = psd.log_psd();
        for (lo, hi) in [(200.0, 8_000.0), (800.0, 1_200.0), (0.0, 24_000.0)] {
            let trimmed = psd.find_peaks_with_apron(&series, lo, hi, 3.0, fit_apron);
            let octave = psd.find_peaks_with_apron(&series, lo, hi, 3.0, octave_apron);
            assert!(!trimmed.is_empty(), "{lo}–{hi} Hz found no peaks");
            assert_eq!(trimmed, octave, "{lo}–{hi} Hz");
        }
    }

    // #258: on one grid the mean-density ratio equals the band-power ratio, so
    // switching the coverage gate to it changes nothing when the grids match.
    #[test]
    fn band_mean_density_ratio_matches_band_power_ratio_on_one_grid() {
        let a = welch_psd(&white_noise(48_000, 71), RATE);
        let b = welch_psd(&scale(&white_noise(48_000, 72), 0.3), RATE);
        for (lo, hi) in [(60.0, 120.0), (250.0, 500.0), (6_000.0, 12_000.0)] {
            let power = a.band_power(lo, hi) / b.band_power(lo, hi);
            let density = a.band_mean_density(lo, hi) / b.band_mean_density(lo, hi);
            assert!(
                (power / density - 1.0).abs() < 1e-12,
                "{lo}–{hi}: {power} vs {density}"
            );
        }
        assert_eq!(a.band_mean_density(30_000.0, 40_000.0), 0.0);
    }

    // #258: the floor estimator averages several short segments, and its density
    // stays unbiased: white noise reads the same mean density on both grids.
    #[test]
    fn floor_psd_is_unbiased_against_the_full_grid() {
        let x = white_noise(96_000, 81);
        let full = welch_psd(&x, RATE);
        let floor = welch_psd_floor(&x, RATE);
        assert!((floor.bin_hz - f64::from(RATE) / FLOOR_SEG as f64).abs() < 1e-9);
        let (f, g) = (
            full.band_mean_density(500.0, 12_000.0),
            floor.band_mean_density(500.0, 12_000.0),
        );
        assert!(
            (db(f) - db(g)).abs() < 0.2,
            "full {} dB vs floor {} dB",
            db(f),
            db(g)
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
