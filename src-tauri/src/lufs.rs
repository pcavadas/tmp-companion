//! Loudness measurement (ITU-R BS.1770 / EBU R128) via the pure-Rust `ebur128`
//! crate. The closed loop levels on the **gated integrated** LUFS; we also
//! report **short-term max** because the relative gate discards quiet decays
//! and palm-mute gaps, so integrated alone understates a dynamic clean tone vs
//! a compressed high-gain one (the clean-vs-distorted mismatch the research
//! flagged).
//!
//! **Metering convention (PR2 re-baseline):** the processed USB pair (capture
//! channels 0/1) is measured as standard 2-channel BS.1770 (energy sum across
//! channels, [`measure_stereo`]) — dry channel 2+ is never included. On the
//! TMP's mirrored dual-mono USB output this reads a fixed +3.0103 dB
//! (10·log10(2), algebraically exact for identical channels regardless of
//! content) over the single-channel read the app used before this PR —
//! ground-truthed against an external BS.1770 stereo meter (dual-mono +3.01 LU
//! over mono on the same clip, agreeing with ffmpeg's independent `ebur128` to
//! 0.02 LU). The INPUT/stimulus side (self-measurement, dry-DI calibration,
//! the floor-guard spread) deliberately stays on [`measure_mono`] — those
//! reads are single-channel by construction (the dry instrument send), not a
//! metering choice.

use ebur128::{EbuR128, Mode};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Loudness {
    /// Gated integrated loudness (the leveling target metric).
    pub integrated_lufs: f64,
    /// Maximum short-term (3 s window) loudness over the clip.
    pub short_term_max_lufs: f64,
    /// Maximum true peak (dBTP, oversampled) over the clip — 20·log10 of the linear
    /// peak `ebur128` reports. A zero/silent peak floors to −120.0 rather than −inf.
    pub true_peak_dbtp: f64,
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

/// Shared BS.1770 measure over an interleaved buffer of `channels` channels.
/// `measure_mono`/`measure_stereo` are thin `channels` = 1/2 wrappers so the two
/// conventions can't drift: the 100 ms short-term hop is a FRAME count (scaled by
/// `channels` for an interleaved buffer — verified: chunked == one-shot that way)
/// and the true peak is `max` over every channel (`EbuR128::true_peak(ch)` is
/// PER-CHANNEL — reading only channel 0 under a 2-ch meter would silently report
/// the left channel's peak as the clip's true peak). dBTP is never shifted by the
/// channel count.
///
/// A buffer whose length isn't a multiple of `channels` is trimmed to a whole
/// number of frames BEFORE chunking: the crate's own frame view
/// (`ebur128::Interleaved::new`) rejects a non-divisible slice outright with
/// `Error::NoMem` rather than silently dropping the remainder the way
/// `.chunks()` does, so a trailing partial frame must be trimmed here or
/// `add_frames_f32` fails with an opaque "NoMem" instead of the documented
/// no-partial-frame tolerance. `measure_mono` (channels == 1) never has a
/// remainder, so the trim is a no-op there.
fn measure(interleaved: &[f32], sample_rate: u32, channels: u32) -> Result<Loudness, String> {
    if interleaved.is_empty() {
        return Err("empty audio buffer".into());
    }
    // See the doc comment above for why a non-frame-aligned buffer is trimmed
    // rather than handed straight to `.chunks()`.
    let usable_len = interleaved.len() - interleaved.len() % channels as usize;
    if usable_len == 0 {
        return Err("empty audio buffer".into());
    }
    let interleaved = &interleaved[..usable_len];
    let mut meter = EbuR128::new(channels, sample_rate, Mode::I | Mode::S | Mode::TRUE_PEAK)
        .map_err(|e| format!("ebur128 init: {e:?}"))?;

    let hop = (sample_rate as usize / 10).max(1) * channels as usize; // 100 ms of FRAMES
    let mut st_max = f64::NEG_INFINITY;
    for chunk in interleaved.chunks(hop) {
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
    let mut true_peak_linear = 0.0f64;
    for ch in 0..channels {
        let tp = meter
            .true_peak(ch)
            .map_err(|e| format!("ebur128 true_peak({ch}): {e:?}"))?;
        if tp > true_peak_linear {
            true_peak_linear = tp;
        }
    }
    let true_peak_dbtp = if true_peak_linear > 0.0 {
        20.0 * true_peak_linear.log10()
    } else {
        -120.0
    };

    Ok(Loudness {
        integrated_lufs: integrated,
        short_term_max_lufs: if st_max.is_finite() {
            st_max
        } else {
            integrated
        },
        true_peak_dbtp,
    })
}

/// Measure a mono buffer of `f32` samples in [-1, 1]. The INPUT/stimulus-side
/// convention (self-measurement, dry-DI calibration, the floor-guard spread) —
/// see the module header. Kept forever regardless of how the output-side
/// convention evolves.
pub fn measure_mono(samples: &[f32], sample_rate: u32) -> Result<Loudness, String> {
    measure(samples, sample_rate, 1)
}

/// Measure an INTERLEAVED 2-channel buffer of `f32` samples in [-1, 1] — the
/// OUTPUT-side convention (see the module header): standard 2-ch BS.1770 over
/// the processed USB pair. An odd `interleaved.len()` drops its trailing
/// sample (no complete frame) rather than erroring, matching `measure_mono`'s
/// no-partial-frame tolerance.
pub fn measure_stereo(interleaved: &[f32], sample_rate: u32) -> Result<Loudness, String> {
    measure(interleaved, sample_rate, 2)
}

/// Incremental integrated-loudness meter for the adaptive capture: feed frames as
/// they arrive and query the gated integrated LUFS repeatedly to watch convergence.
/// `Mode::I` only — the adaptive measurement path solves on integrated loudness and
/// never reads short-term, so the 3 s short-term window is dead weight here.
/// `new` (1-ch, mono) and `new_stereo` (2-ch, interleaved) are the two entry
/// points, mirroring [`measure_mono`]/[`measure_stereo`]; `add` tolerates a
/// non-frame-aligned chunk by carrying the trailing partial frame across calls
/// (see its doc comment) rather than requiring the caller to pre-align.
pub struct IncrementalLoudness {
    meter: EbuR128,
    channels: u32,
    /// Trailing samples from the last `add` that didn't complete a frame — at
    /// most `channels - 1` of them. Prepended to the next call's samples.
    pending: Vec<f32>,
}

impl IncrementalLoudness {
    pub fn new(sample_rate: u32) -> Result<Self, String> {
        Self::new_channels(sample_rate, 1)
    }

    /// 2-channel (interleaved) incremental meter — the OUTPUT-side convention for
    /// the adaptive/live-advisory paths once the capture is genuinely stereo.
    pub fn new_stereo(sample_rate: u32) -> Result<Self, String> {
        Self::new_channels(sample_rate, 2)
    }

    fn new_channels(sample_rate: u32, channels: u32) -> Result<Self, String> {
        let meter = EbuR128::new(channels, sample_rate, Mode::I)
            .map_err(|e| format!("ebur128 init: {e:?}"))?;
        Ok(Self {
            meter,
            channels,
            pending: Vec::new(),
        })
    }

    /// Feed a chunk of `f32` samples in [-1, 1] (interleaved, if the meter is
    /// stereo). The chunk need NOT be frame-aligned: a trailing partial frame
    /// (fewer than `channels` samples) is carried over and prepended to the
    /// next `add` call rather than fed to `ebur128` (which errors on a
    /// non-frame-aligned slice) or trimmed outright (which would desync L/R
    /// mid-stream for a stereo meter). A dangling sample never flushed by a
    /// later call is simply dropped — there is no explicit finish/flush API.
    /// An empty chunk is a no-op.
    pub fn add(&mut self, samples: &[f32]) -> Result<(), String> {
        if samples.is_empty() {
            return Ok(());
        }
        let channels = self.channels as usize;
        // Fast path: no carry-over pending and the chunk is already
        // frame-aligned — feed it straight through, no extra allocation.
        if self.pending.is_empty() && samples.len().is_multiple_of(channels) {
            return self
                .meter
                .add_frames_f32(samples)
                .map_err(|e| format!("ebur128 add_frames: {e:?}"));
        }
        self.pending.extend_from_slice(samples);
        let usable_len = self.pending.len() - self.pending.len() % channels;
        if usable_len > 0 {
            self.meter
                .add_frames_f32(&self.pending[..usable_len])
                .map_err(|e| format!("ebur128 add_frames: {e:?}"))?;
        }
        self.pending.drain(..usable_len);
        Ok(())
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
    fn true_peak_tracks_amplitude() {
        let rate = 48_000;
        for amp in [0.9_f32, 0.5, 0.1] {
            let l = measure_mono(&sine(1000.0, 3.5, rate, amp), rate).unwrap();
            let expected = 20.0 * f64::from(amp).log10();
            assert!(
                (l.true_peak_dbtp - expected).abs() < 0.3,
                "amp={amp}: expected ~{expected:.2} dBTP, got {:.2}",
                l.true_peak_dbtp
            );
        }
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

    fn interleave2(l: &[f32], r: &[f32]) -> Vec<f32> {
        l.iter().zip(r).flat_map(|(&a, &b)| [a, b]).collect()
    }

    #[test]
    fn dual_mono_stereo_reads_3_01_over_mono() {
        // Ground-truthed constant (module header): a mono buffer duplicated onto
        // both channels reads a fixed +3.0103 dB (10*log10(2)) under the 2-ch
        // meter — the TMP's mirrored USB-Out shape.
        let rate = 48_000;
        let tone = sine(1000.0, 4.0, rate, 0.4);
        let mono = measure_mono(&tone, rate).unwrap().integrated_lufs;
        let stereo = measure_stereo(&interleave2(&tone, &tone), rate)
            .unwrap()
            .integrated_lufs;
        assert!(
            (stereo - mono - 3.0103).abs() < 0.05,
            "mono {mono:.3} stereo {stereo:.3} delta {:.3}",
            stereo - mono
        );
    }

    #[test]
    fn stereo_matches_hand_computed_energy_sum_on_independent_channels() {
        // The test that actually discriminates a genuine 2-ch BS.1770 meter from
        // "measure channel 0 and add a constant": two DIFFERENT-level, uncorrelated
        // tones on L/R. A dual-mono shortcut would report L's level +3.01 regardless
        // of R; a real 2-ch meter reports 10*log10(z_l + z_r) - 0.691 (BS.1770's
        // -0.691 LUFS offset), which is NOT simply "louder channel + 3.01" once the
        // channels differ.
        let rate = 48_000;
        let l = sine(1000.0, 5.0, rate, 0.5);
        let r = sine(1300.0, 5.0, rate, 0.2);
        let stereo = measure_stereo(&interleave2(&l, &r), rate).unwrap();
        let ml = measure_mono(&l, rate).unwrap().integrated_lufs;
        let mr = measure_mono(&r, rate).unwrap().integrated_lufs;
        // BS.1770 integrated loudness before gating is -0.691 + 10*log10(sum of
        // per-channel mean-square); recovering each channel's "z" from its own
        // (also -0.691-offset) mono reading and summing reproduces the stereo
        // value — the two tones are stationary and above the relative gate, so
        // gating doesn't reorder anything here.
        let zl = 10f64.powf((ml + 0.691) / 10.0);
        let zr = 10f64.powf((mr + 0.691) / 10.0);
        let hand_computed = -0.691 + 10.0 * (zl + zr).log10();
        assert!(
            (stereo.integrated_lufs - hand_computed).abs() < 0.1,
            "stereo {:.3} vs hand-computed sum {hand_computed:.3}",
            stereo.integrated_lufs
        );
        // And it must NOT equal "louder channel (L) + 3.01" — the dual-mono
        // shortcut a hand-rolled per-channel-and-add-a-constant bug would produce.
        assert!(
            (stereo.integrated_lufs - (ml + 3.0103)).abs() > 0.3,
            "stereo reading must not collapse to dual-mono-of-the-loudest-channel"
        );
    }

    #[test]
    fn stereo_true_peak_is_the_max_of_either_channel() {
        // T3: `EbuR128::true_peak(ch)` is PER-CHANNEL. An asymmetric pair must
        // report the LOUDER channel's peak, not channel 0's.
        let rate = 48_000;
        let quiet = sine(1000.0, 2.0, rate, 0.2);
        let loud = sine(1000.0, 2.0, rate, 0.9);
        let ch0_quiet = measure_stereo(&interleave2(&quiet, &loud), rate).unwrap();
        let ch0_loud = measure_stereo(&interleave2(&loud, &quiet), rate).unwrap();
        let expected = 20.0 * 0.9f64.log10();
        assert!(
            (ch0_quiet.true_peak_dbtp - expected).abs() < 0.3,
            "loud-on-ch1: expected ~{expected:.2} dBTP, got {:.2}",
            ch0_quiet.true_peak_dbtp
        );
        assert!(
            (ch0_loud.true_peak_dbtp - expected).abs() < 0.3,
            "loud-on-ch0: expected ~{expected:.2} dBTP, got {:.2}",
            ch0_loud.true_peak_dbtp
        );
    }

    #[test]
    fn stereo_trailing_partial_frame_is_dropped_not_errored() {
        // The doc comment on `measure` promises a trailing partial frame is
        // dropped, not an error — but `ebur128::Interleaved::new` (this crate's
        // internal frame view) rejects any slice whose length isn't a multiple
        // of `channels` with `Error::NoMem`, which `add_frames_f32` surfaces as
        // an opaque "ebur128 add_frames: NoMem". A [l, r, trailing] buffer (one
        // full stereo frame plus one orphan sample) must measure identically to
        // the same buffer with the orphan trimmed off, not error.
        let rate = 48_000;
        let l = sine(1000.0, 4.0, rate, 0.4);
        let r = sine(1300.0, 4.0, rate, 0.15);
        let mut clip = interleave2(&l, &r);
        let trimmed = measure_stereo(&clip, rate).unwrap();
        clip.push(0.9); // one orphan sample — not a complete stereo frame
        let with_trailing = measure_stereo(&clip, rate)
            .expect("a trailing partial frame must be dropped, not error");
        assert_eq!(
            trimmed.integrated_lufs, with_trailing.integrated_lufs,
            "trailing orphan sample must not change the reading"
        );
    }

    #[test]
    fn stereo_single_sample_is_empty_buffer_not_a_crate_error() {
        // A 1-sample buffer under a 2-channel meter has no complete frame at
        // all — that must surface as the same "empty audio buffer" error the
        // zero-length case already returns, not a crate-internal NoMem error.
        let err = measure_stereo(&[0.5], 48_000).unwrap_err();
        assert_eq!(err, "empty audio buffer");
    }

    #[test]
    fn incremental_stereo_matches_measure_stereo() {
        let rate = 48_000;
        let l = sine(1000.0, 4.0, rate, 0.4);
        let r = sine(1300.0, 4.0, rate, 0.15);
        let clip = interleave2(&l, &r);
        let oneshot = measure_stereo(&clip, rate).unwrap().integrated_lufs;
        let mut m = IncrementalLoudness::new_stereo(rate).unwrap();
        // Hop by FRAMES*channels (100 ms of frames), mirroring `measure`'s own hop.
        let hop = (rate as usize / 10) * 2;
        for chunk in clip.chunks(hop) {
            m.add(chunk).unwrap();
        }
        let inc = m.integrated().unwrap();
        assert!(
            (inc - oneshot).abs() < 1e-6,
            "incremental {inc:.6} vs one-shot {oneshot:.6}"
        );
    }

    #[test]
    fn incremental_stereo_survives_odd_split_boundaries() {
        // `replay_measure` slices `body[fed..end]` in fixed-size steps that have
        // no relation to the 2-channel frame size, so a chunk boundary can land
        // one sample into a stereo frame. That must carry across `add` calls
        // rather than erroring (the crate's `NoMem` on a non-frame-aligned
        // slice) or being trimmed (which would desync L/R mid-stream).
        let rate = 48_000;
        let l = sine(1000.0, 4.0, rate, 0.4);
        let r = sine(1300.0, 4.0, rate, 0.15);
        let clip = interleave2(&l, &r);
        let oneshot = measure_stereo(&clip, rate).unwrap().integrated_lufs;

        // Split at an ODD sample offset (one past a whole-frame prefix), then
        // feed the rest in one more chunk.
        let mut m = IncrementalLoudness::new_stereo(rate).unwrap();
        let split = (rate as usize / 10) * 2 + 1; // 100 ms of frames, plus one orphan sample
        m.add(&clip[..split]).unwrap();
        m.add(&clip[split..]).unwrap();
        let inc = m.integrated().unwrap();
        assert!(
            (inc - oneshot).abs() < 1e-6,
            "odd-split incremental {inc:.6} vs one-shot {oneshot:.6}"
        );

        // All-odd chunk size (3) — proves mid-stream carries keep channel
        // alignment across many boundary crossings, not just one.
        let mut m3 = IncrementalLoudness::new_stereo(rate).unwrap();
        for chunk in clip.chunks(3) {
            m3.add(chunk).unwrap();
        }
        let inc3 = m3.integrated().unwrap();
        assert!(
            (inc3 - oneshot).abs() < 1e-6,
            "chunk-of-3 incremental {inc3:.6} vs one-shot {oneshot:.6}"
        );
    }
}
