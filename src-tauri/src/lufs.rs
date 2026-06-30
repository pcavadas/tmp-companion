//! Loudness measurement (ITU-R BS.1770 / EBU R128) via the pure-Rust `ebur128`
//! crate. The closed loop levels on the **gated integrated** LUFS; we also
//! report **short-term max** because the relative gate discards quiet decays
//! and palm-mute gaps, so integrated alone understates a dynamic clean tone vs
//! a compressed high-gain one (the clean-vs-distorted mismatch the research
//! flagged). The capture path measures a single (mono) processed channel; keep
//! that convention fixed — a mono read sits ~3 dB below the same signal as
//! dual-mono.

use ebur128::{EbuR128, Mode};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Loudness {
    /// Gated integrated loudness (the leveling target metric).
    pub integrated_lufs: f64,
    /// Maximum short-term (3 s window) loudness over the clip.
    pub short_term_max_lufs: f64,
}

impl Loudness {
    /// Short-term max − integrated, in LU: the capture's dynamics spread. Gain-
    /// invariant (both terms shift equally with level), so it characterizes the
    /// PRESET, not the level it was measured at. A large spread means the relative
    /// gate discarded a lot of quiet material — the integrated metric understates
    /// the preset's peaks vs a compressed one, so leveling results should be
    /// flagged "verify by ear".
    pub fn spread_lu(&self) -> f64 {
        self.short_term_max_lufs - self.integrated_lufs
    }
}

/// Measure a mono buffer of `f32` samples in [-1, 1]. Feeds the meter in 100 ms
/// hops so we can track the short-term maximum as the window slides.
pub fn measure_mono(samples: &[f32], sample_rate: u32) -> Result<Loudness, String> {
    if samples.is_empty() {
        return Err("empty audio buffer".into());
    }
    let mut meter = EbuR128::new(1, sample_rate, Mode::I | Mode::S)
        .map_err(|e| format!("ebur128 init: {e:?}"))?;

    let hop = (sample_rate as usize / 10).max(1); // 100 ms
    let mut st_max = f64::NEG_INFINITY;
    for chunk in samples.chunks(hop) {
        meter
            .add_frames_f32(chunk)
            .map_err(|e| format!("ebur128 add_frames: {e:?}"))?;
        if let Ok(st) = meter.loudness_shortterm() {
            if st.is_finite() && st > st_max {
                st_max = st;
            }
        }
    }

    let integrated = meter
        .loudness_global()
        .map_err(|e| format!("ebur128 loudness_global: {e:?}"))?;

    Ok(Loudness {
        integrated_lufs: integrated,
        short_term_max_lufs: if st_max.is_finite() { st_max } else { integrated },
    })
}

/// Incremental integrated-loudness meter for the adaptive capture: feed frames as
/// they arrive and query the gated integrated LUFS repeatedly to watch convergence.
/// `Mode::I` only — the adaptive measurement path solves on integrated loudness and
/// never reads short-term, so the 3 s short-term window is dead weight here.
pub struct IncrementalLoudness {
    meter: EbuR128,
}

impl IncrementalLoudness {
    pub fn new(sample_rate: u32) -> Result<Self, String> {
        let meter =
            EbuR128::new(1, sample_rate, Mode::I).map_err(|e| format!("ebur128 init: {e:?}"))?;
        Ok(Self { meter })
    }

    /// Feed a mono chunk of `f32` samples in [-1, 1]. An empty chunk is a no-op.
    pub fn add(&mut self, samples: &[f32]) -> Result<(), String> {
        if samples.is_empty() {
            return Ok(());
        }
        self.meter
            .add_frames_f32(samples)
            .map_err(|e| format!("ebur128 add_frames: {e:?}"))
    }

    /// Gated integrated loudness over everything fed so far. May be non-finite
    /// (`-inf`) until enough above-gate signal has accumulated; callers treat a
    /// non-finite value as "not enough signal yet".
    pub fn integrated(&self) -> Result<f64, String> {
        self.meter
            .loudness_global()
            .map_err(|e| format!("ebur128 loudness_global: {e:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn sine(freq: f32, secs: f32, rate: u32, amp: f32) -> Vec<f32> {
        let n = (secs * rate as f32) as usize;
        (0..n)
            .map(|i| amp * (2.0 * PI * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    #[test]
    fn halving_amplitude_drops_loudness_by_about_6db() {
        let rate = 48_000;
        let full = sine(1000.0, 5.0, rate, 0.5);
        let half: Vec<f32> = full.iter().map(|x| x * 0.5).collect();
        let lf = measure_mono(&full, rate).unwrap().integrated_lufs;
        let lh = measure_mono(&half, rate).unwrap().integrated_lufs;
        // -6.02 dB expected; allow gating/rounding slack.
        assert!(
            (lf - lh - 6.02).abs() < 0.2,
            "expected ~6.02 LU drop, got {}",
            lf - lh
        );
    }

    #[test]
    fn louder_signal_measures_higher() {
        let rate = 48_000;
        let quiet = measure_mono(&sine(1000.0, 4.0, rate, 0.1), rate).unwrap();
        let loud = measure_mono(&sine(1000.0, 4.0, rate, 0.8), rate).unwrap();
        assert!(loud.integrated_lufs > quiet.integrated_lufs);
        assert!(loud.short_term_max_lufs >= loud.integrated_lufs - 1.0);
    }

    #[test]
    fn empty_buffer_errors() {
        assert!(measure_mono(&[], 48_000).is_err());
    }

    #[test]
    fn dynamic_signal_has_larger_spread_than_steady() {
        // A loud passage followed by a long ~9 dB-down section (a "dynamic clean
        // tone"): the quieter part sits ABOVE the −10 LU relative gate, so it
        // drags the integrated reading down while the short-term max rides the
        // loud passage — a clearly larger spread than a steady tone's. (A fully
        // silent tail would be gated OUT and show no spread.)
        let rate = 48_000;
        let steady = sine(1000.0, 8.0, rate, 0.5);
        let mut dynamic = sine(1000.0, 3.5, rate, 0.7);
        dynamic.extend(sine(1000.0, 8.0, rate, 0.25));
        let s = measure_mono(&steady, rate).unwrap();
        let d = measure_mono(&dynamic, rate).unwrap();
        assert!(s.spread_lu() < 1.0, "steady spread {}", s.spread_lu());
        assert!(d.spread_lu() > 3.0, "dynamic spread {}", d.spread_lu());
    }
}
