//! Capture records: who a re-amp capture measured, and what it heard.
//!
//! The capture tap ([`super::capture`]) runs on the caller's thread and only clones the
//! stimulus and the interleaved capture into the recorder's queue. Everything here runs
//! on the writer thread: the processed pair, the BS.1770 loudness, per-channel peak and
//! RMS over EVERY returned channel (so a stale third or fourth channel shows), and the
//! optional WAV of the processed pair.
//!
//! The capture's identity comes from [`capture_ctx`], which the callers that know it
//! (the leveller's capture seams) set around the capture. The outermost context wins,
//! so a strict re-measure's validation identity is not replaced by the inner seam's.

use std::cell::RefCell;
use std::time::Instant;

use crate::audio::Capture;

/// Who a capture measured. Only built while recording.
#[derive(Clone, Debug, Default)]
pub(crate) struct CaptureCtx {
    pub label: String,
    pub slot: Option<u32>,
    pub scene_slot: Option<u32>,
    pub switch: Option<u32>,
}

thread_local! {
    static CTX: RefCell<Option<CaptureCtx>> = const { RefCell::new(None) };
}

/// Clears the context it set when dropped. A guard that found a context already set
/// leaves it alone.
pub(crate) struct CtxGuard {
    owns: bool,
}

impl Drop for CtxGuard {
    fn drop(&mut self) {
        if self.owns {
            CTX.with(|c| *c.borrow_mut() = None);
        }
    }
}

/// Name the captures made on this thread until the guard drops. Inert (no allocation)
/// when the recorder is off.
pub(crate) fn capture_ctx(
    label: &str,
    slot: Option<u32>,
    scene_slot: Option<u32>,
    switch: Option<u32>,
) -> CtxGuard {
    if super::recorder().is_none() {
        return CtxGuard { owns: false };
    }
    set_ctx(CaptureCtx {
        label: label.to_string(),
        slot,
        scene_slot,
        switch,
    })
}

fn set_ctx(ctx: CaptureCtx) -> CtxGuard {
    CTX.with(|c| {
        let mut c = c.borrow_mut();
        if c.is_some() {
            return CtxGuard { owns: false };
        }
        *c = Some(ctx);
        CtxGuard { owns: true }
    })
}

pub(crate) fn current_ctx() -> Option<CaptureCtx> {
    CTX.with(|c| c.borrow().clone())
}

/// One capture, as handed from the tap to the writer.
pub(crate) struct CaptureJob {
    pub backend: &'static str,
    pub t_start: Instant,
    pub t_end: Instant,
    pub stimulus: Vec<f32>,
    pub rate: u32,
    pub tail_ms: u64,
    pub result: Result<Capture, String>,
    pub ctx: Option<CaptureCtx>,
}

/// What the writer derives from a capture.
pub(crate) struct CaptureStats {
    pub channels: usize,
    pub frames: usize,
    pub lufs_i: Option<f64>,
    pub true_peak_dbtp: Option<f64>,
    pub spread_lu: Option<f64>,
    pub peak: Vec<f64>,
    pub rms: Vec<f64>,
    pub lufs_err: Option<String>,
}

/// Loudness from the processed pair only (a genuinely mono capture is metered as
/// mono), peak and RMS from every channel.
pub(crate) fn stats(cap: &Capture) -> CaptureStats {
    let channels = cap.channels.max(1);
    let frames = cap.interleaved.len() / channels;
    let peak = (0..cap.channels)
        .map(|c| f64::from(cap.channel_peak(c)))
        .collect();
    let rms = (0..cap.channels)
        .map(|c| f64::from(cap.channel_rms(c)))
        .collect();
    let loud = match cap.processed_stereo() {
        Some(pair) => crate::lufs::measure_stereo(&pair, cap.sample_rate),
        None => crate::lufs::measure_mono(&cap.channel(0), cap.sample_rate),
    };
    let (lufs_i, true_peak_dbtp, spread_lu, lufs_err) = match loud {
        Ok(l) => (
            Some(l.integrated_lufs),
            Some(l.true_peak_dbtp),
            Some(l.spread_lu()),
            None,
        ),
        Err(e) => (None, None, None, Some(e)),
    };
    CaptureStats {
        channels: cap.channels,
        frames,
        lufs_i,
        true_peak_dbtp,
        spread_lu,
        peak,
        rms,
        lufs_err,
    }
}

/// Write the processed pair (or the single channel of a mono capture) as a float WAV.
pub(crate) fn write_pair_wav(path: &str, cap: &Capture) -> Result<(), String> {
    match cap.processed_stereo() {
        Some(pair) => crate::stimulus::write_wav_stereo(path, &pair, cap.sample_rate),
        None => crate::stimulus::write_wav_mono(path, &cap.channel(0), cap.sample_rate),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn four_channel(frames: usize) -> Capture {
        let mut interleaved = Vec::with_capacity(frames * 4);
        for i in 0..frames {
            let s = ((i as f32) * 0.05).sin();
            // Processed pair, the dry send, and a stale fourth channel.
            interleaved.extend_from_slice(&[0.5 * s, 0.25 * s, 0.9 * s, 0.01]);
        }
        Capture {
            interleaved,
            channels: 4,
            sample_rate: 48_000,
        }
    }

    #[test]
    fn a_four_channel_capture_has_four_peaks_and_rms_and_meters_only_the_pair() {
        let cap = four_channel(48_000 * 2);
        let st = stats(&cap);
        assert_eq!(st.channels, 4);
        assert_eq!(st.frames, 96_000);
        assert_eq!(st.peak.len(), 4);
        assert_eq!(st.rms.len(), 4);
        assert!((st.peak[3] - 0.01).abs() < 1e-6);
        assert!(st.peak[2] > st.peak[0]);

        // Loudness is the processed pair's, whatever the dry channel holds.
        let pair = Capture {
            interleaved: cap.processed_stereo().unwrap(),
            channels: 2,
            sample_rate: 48_000,
        };
        let want = crate::lufs::measure_stereo(&pair.interleaved, 48_000)
            .unwrap()
            .integrated_lufs;
        assert!((st.lufs_i.unwrap() - want).abs() < 1e-9);
    }

    #[test]
    fn the_outer_context_wins() {
        let outer = set_ctx(CaptureCtx {
            label: "validate".into(),
            slot: Some(404),
            ..CaptureCtx::default()
        });
        {
            let _inner = set_ctx(CaptureCtx {
                label: "capture".into(),
                ..CaptureCtx::default()
            });
            assert_eq!(current_ctx().unwrap().label, "validate");
        }
        assert_eq!(current_ctx().unwrap().slot, Some(404));
        drop(outer);
        assert!(current_ctx().is_none());
    }
}
