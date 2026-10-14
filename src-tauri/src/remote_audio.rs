//! `RemoteAudio`: re-amp captures on a remote device, for the `remote` e2e tier
//! (`--features e2e`, `TMP_E2E_REMOTE_AUDIO=<addr>` beside `TMP_E2E_REMOTE`). The remote device
//! renders the stimulus through its loaded preset and returns the processed pair. It is offline and
//! self-consistent, never device evidence: its levels are not calibrated against the unit.
//!
//! WIRE. One capture job per connection (TCP, or a Unix stream socket), little-endian:
//! - request: `TWA1`, u16 version 1, u16 header length 24, u32 rate (44 100), u32 S, u32 R,
//!   u32 flags, then S stimulus samples for the re-amp input as i32, 24-bit signed (full scale
//!   ±2^23);
//! - reply: `TWA1`, i32 status, u32 channels, u32 frames, nine u32 diagnostics, then on OK
//!   `frames × channels` i32 samples in the same format: USB-Out 1/2 for R frames, from the
//!   block the stimulus started in.
//!
//! ENGAGEMENT. Every job asks for the remote device's engagement verdict ([`FLAGS`]): the remote
//! device drives a marker into the guitar inputs while the job runs and watches the dry instrument
//! sends (USB-Out 3/4), which re-amp leaves unwritten. A job whose dry sends carried the marker ran
//! without re-amp engaged, and comes back [`Status::NotEngaged`] (or [`Status::Mixed`] when
//! re-amp changed mid-job) with no samples, so a failed engage is a loud error here, never a
//! silent capture. With re-amp engaged no guitar input reaches the DSP, so the marker leaves
//! the capture untouched.
//!
//! The far end may refuse a request before reading its stimulus and close, so a failed send
//! still reads for the refusal. Hanging up cancels a running job: that is how a Stop lands,
//! so every wait here polls the Stop flag and drops the link on it.
//!
//! RATE. The remote device's audio runs at 44.1 kHz. The caller's stimulus is resampled down and
//! the capture back up with a band-limited FFT resampler ([`resample`]), so the leveller sees the
//! capture shape the unit's host streams return: two channels at the caller's rate, covering
//! the stimulus plus the tail. The stimulus goes over behind [`LEAD_IN_MS`] of silence, the
//! unit's inject latency, so it lands where it does in a unit capture.
//!
//! STATS. Every job is counted in [`STATS`], served at the e2e server's `/remote/audio`; the remote
//! device lane fails a run that had any failed job.

use std::io::ErrorKind;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use realfft::num_complex::Complex;
use realfft::RealFftPlanner;

use crate::audio::Capture;
use crate::error::LevelError;
use crate::hid::remote::{RemoteAddr, Stream};

/// The remote device's audio rate.
pub(crate) const REMOTE_RATE: u32 = 44_100;
const MAGIC: &[u8; 4] = b"TWA1";
const VERSION: u16 = 1;
const REQUEST_HEADER: u16 = 24;
/// Request flags: the marker in the guitar inputs (2) and the engagement verdict (4).
pub(crate) const FLAGS: u32 = 2 | 4;
/// Silence ahead of the stimulus: the unit's re-amp inject latency (~32 ms over USB,
/// `doctor_capture`'s `DOCTOR_PAD_MS` note), which the remote device does not have.
pub(crate) const LEAD_IN_MS: u32 = 31;
const DIAG: usize = 9;
const REPLY_HEADER: usize = 16 + 4 * DIAG;
/// The far end's longest job, either way (30 s at 44.1 kHz).
const MAX_FRAMES: u32 = 30 * REMOTE_RATE;
/// 24-bit full scale.
const FULL_SCALE: f32 = 8_388_608.0;
/// How often a send or a wait for the reply looks at the Stop flag.
const POLL: Duration = Duration::from_millis(100);
/// How long the stimulus may take to go out.
const SEND_BUDGET: Duration = Duration::from_secs(10);
/// Past the capture's own length: how long the remote device may take to answer.
const REPLY_SLACK: Duration = Duration::from_secs(15);
/// After a failed send, how long to wait for the refusal the far end sent instead.
const REFUSAL_WAIT: Duration = Duration::from_secs(2);
/// Remote-rate frames captured past the caller's window, so the resampler's edge stays outside it.
const EDGE_FRAMES: u32 = 256;

/// How many times slower than real time the remote device's audio runs (1 in tests, where the
/// env is never read, and in a build without the e2e server).
#[cfg(feature = "e2e")]
fn time_scale() -> f64 {
    crate::e2e_remote_time_scale()
}
#[cfg(not(feature = "e2e"))]
fn time_scale() -> f64 {
    1.0
}

/// A job's outcome, as the far end reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    Ok,
    Busy,
    BadRequest,
    NoAudio,
    Stalled,
    Cancelled,
    Poisoned,
    NotEngaged,
    Mixed,
    Unknown(i32),
}

impl Status {
    fn from_wire(v: i32) -> Status {
        match v {
            0 => Status::Ok,
            1 => Status::Busy,
            2 => Status::BadRequest,
            3 => Status::NoAudio,
            4 => Status::Stalled,
            5 => Status::Cancelled,
            6 => Status::Poisoned,
            7 => Status::NotEngaged,
            8 => Status::Mixed,
            v => Status::Unknown(v),
        }
    }

    fn error(self) -> String {
        match self {
            Status::Ok => "remote capture: ok".into(),
            Status::Busy => "remote capture: the remote device is running another capture".into(),
            Status::BadRequest => {
                "remote capture: the remote device refused the request (malformed or too long)"
                    .into()
            }
            Status::NoAudio => {
                "remote capture: the remote device's audio loop is not running".into()
            }
            Status::Stalled => {
                "remote capture: the remote device's audio stalled mid-capture".into()
            }
            Status::Cancelled => "remote capture: the remote device cancelled the capture".into(),
            Status::Poisoned => {
                "remote capture: the remote device's audio retired itself after a cancel \
                                 that did not complete; restart the remote device"
                    .into()
            }
            Status::NotEngaged => "remote capture: re-amp was not engaged (the guitar inputs \
                                   reached the return)"
                .into(),
            Status::Mixed => "remote capture: re-amp changed mid-capture".into(),
            Status::Unknown(v) => {
                format!("remote capture: unknown status {v} from the remote device")
            }
        }
    }
}

/// This process's remote capture counters (see STATS in the module doc).
pub(crate) struct Stats {
    jobs: AtomicU64,
    ok: AtomicU64,
    failed: AtomicU64,
    cancelled: AtomicU64,
    not_engaged: AtomicU64,
    mixed: AtomicU64,
    late_blocks: AtomicU64,
    slips: AtomicU64,
    max_late_us: AtomicU64,
    last_error: Mutex<Option<String>>,
}

pub(crate) static STATS: Stats = Stats {
    jobs: AtomicU64::new(0),
    ok: AtomicU64::new(0),
    failed: AtomicU64::new(0),
    cancelled: AtomicU64::new(0),
    not_engaged: AtomicU64::new(0),
    mixed: AtomicU64::new(0),
    late_blocks: AtomicU64::new(0),
    slips: AtomicU64::new(0),
    max_late_us: AtomicU64::new(0),
    last_error: Mutex::new(None),
};

impl Stats {
    fn note(&self, outcome: &Result<Reply, LevelError>) {
        self.jobs.fetch_add(1, Ordering::Relaxed);
        let failure = match outcome {
            Ok(r) if r.status == Status::Ok => {
                self.ok.fetch_add(1, Ordering::Relaxed);
                let [_, _, late, max_late_us, slips, ..] = r.diag;
                self.late_blocks
                    .fetch_add(u64::from(late), Ordering::Relaxed);
                self.slips.fetch_add(u64::from(slips), Ordering::Relaxed);
                self.max_late_us
                    .fetch_max(u64::from(max_late_us), Ordering::Relaxed);
                return;
            }
            Ok(r) => {
                match r.status {
                    Status::NotEngaged => self.not_engaged.fetch_add(1, Ordering::Relaxed),
                    Status::Mixed => self.mixed.fetch_add(1, Ordering::Relaxed),
                    _ => 0,
                };
                r.status.error()
            }
            Err(e) if e.is_cancelled() => {
                self.cancelled.fetch_add(1, Ordering::Relaxed);
                return;
            }
            Err(e) => e.to_string(),
        };
        self.failed.fetch_add(1, Ordering::Relaxed);
        *self
            .last_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(failure);
    }

    /// The counters as the e2e server's `/remote/audio` serves them.
    pub(crate) fn json(&self) -> serde_json::Value {
        let n = |a: &AtomicU64| a.load(Ordering::Relaxed);
        serde_json::json!({
            "jobs": n(&self.jobs),
            "ok": n(&self.ok),
            "failed": n(&self.failed),
            "cancelled": n(&self.cancelled),
            "notEngaged": n(&self.not_engaged),
            "mixed": n(&self.mixed),
            "lateBlocks": n(&self.late_blocks),
            "slips": n(&self.slips),
            "maxLateUs": n(&self.max_late_us),
            "lastError": self
                .last_error
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone(),
        })
    }
}

/// One job's reply. `samples` is empty unless the status is [`Status::Ok`].
#[derive(Debug)]
pub(crate) struct Reply {
    pub status: Status,
    pub channels: u32,
    pub frames: u32,
    /// injected, recorded, late blocks, max lateness (µs), slips, PCM events, marker frames,
    /// live dry-send blocks, non-zero return samples.
    pub diag: [u32; DIAG],
    pub samples: Vec<i32>,
}

/// A float sample in 24-bit signed full scale, clamped.
pub(crate) fn to_s24(x: f32) -> i32 {
    (x * FULL_SCALE)
        .round()
        .clamp(-(FULL_SCALE - 1.0), FULL_SCALE - 1.0) as i32
}

/// A 24-bit signed sample in the low bits of an i32 (sign-extended or not) as a float.
pub(crate) fn from_s24(v: i32) -> f32 {
    ((v << 8) >> 8) as f32 / FULL_SCALE
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// The smallest number at least `n` with no prime factor above 7: a fast FFT length.
fn smooth_at_least(n: usize) -> usize {
    (n.max(1)..)
        .find(|&v| {
            let mut v = v;
            for f in [2, 3, 5, 7] {
                while v % f == 0 {
                    v /= f;
                }
            }
            v == 1
        })
        .unwrap_or(n)
}

/// Band-limited resampling of `x` from `from` Hz to `to` Hz: the whole signal's spectrum, cut
/// or zero-extended to the new length, so content below both Nyquists comes through exactly
/// and nothing above the lower one folds back. The signal is padded with 20 ms of silence
/// first, so the transform's wrap-around stays out of it. The result has
/// `round(len · to / from)` samples.
pub(crate) fn resample(x: &[f32], from: u32, to: u32) -> Result<Vec<f32>, String> {
    if from == 0 || to == 0 {
        return Err(format!("resample {from} Hz -> {to} Hz: a rate is zero"));
    }
    if from == to || x.is_empty() {
        return Ok(x.to_vec());
    }
    let g = gcd(from, to);
    let (p, q) = ((from / g) as usize, (to / g) as usize);
    let out_len = ((x.len() as u64 * to as u64 + from as u64 / 2) / from as u64) as usize;
    let guard = from as usize / 50;
    let k = smooth_at_least((x.len() + guard).div_ceil(p));
    let (n, m) = (k * p, k * q);

    let mut planner = RealFftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(n);
    let inverse = planner.plan_fft_inverse(m);
    let mut buf: Vec<f64> = x.iter().map(|&v| f64::from(v)).collect();
    buf.resize(n, 0.0);
    let mut spectrum = forward.make_output_vec();
    forward
        .process(&mut buf, &mut spectrum)
        .map_err(|e| format!("resample: forward FFT: {e}"))?;

    let mut out_spectrum = inverse.make_input_vec();
    let keep = spectrum.len().min(out_spectrum.len());
    out_spectrum[..keep].copy_from_slice(&spectrum[..keep]);
    // The top kept bin sits at the lower Nyquist: drop it, since a real signal's Nyquist bin
    // must be real and this one is a single bin at the band edge either way.
    if keep > 1 {
        out_spectrum[keep - 1] = Complex::new(0.0, 0.0);
    }
    out_spectrum[0].im = 0.0;
    let mut y = inverse.make_output_vec();
    inverse
        .process(&mut out_spectrum, &mut y)
        .map_err(|e| format!("resample: inverse FFT: {e}"))?;
    let scale = 1.0 / n as f64;
    Ok(y.iter()
        .take(out_len)
        .map(|&v| (v * scale) as f32)
        .collect())
}

fn request(stim: &[i32], frames: u32) -> Vec<u8> {
    let mut b = Vec::with_capacity(REQUEST_HEADER as usize + 4 * stim.len());
    b.extend_from_slice(MAGIC);
    b.extend_from_slice(&VERSION.to_le_bytes());
    b.extend_from_slice(&REQUEST_HEADER.to_le_bytes());
    b.extend_from_slice(&REMOTE_RATE.to_le_bytes());
    b.extend_from_slice(&(stim.len() as u32).to_le_bytes());
    b.extend_from_slice(&frames.to_le_bytes());
    b.extend_from_slice(&FLAGS.to_le_bytes());
    for s in stim {
        b.extend_from_slice(&s.to_le_bytes());
    }
    b
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// The job link; dropping it hangs up, which cancels a job the remote device is still running.
struct Link(Stream);

impl Drop for Link {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

fn is_timeout(e: &std::io::Error) -> bool {
    matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
}

/// Send the request, polling `aborted`. `Err` carries why the send failed.
fn send(link: &Link, mut req: &[u8], aborted: &dyn Fn() -> bool) -> Result<(), SendFail> {
    let deadline = Instant::now() + SEND_BUDGET;
    link.0
        .set_write_timeout(POLL)
        .map_err(|e| SendFail::Io(e.to_string()))?;
    while !req.is_empty() {
        if aborted() {
            return Err(SendFail::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(SendFail::Io(format!(
                "the remote device took no stimulus for {} s",
                SEND_BUDGET.as_secs()
            )));
        }
        match link.0.write(req) {
            Ok(0) => return Err(SendFail::Io("the link closed".into())),
            Ok(n) => req = &req[n..],
            Err(e) if e.kind() == ErrorKind::Interrupted || is_timeout(&e) => {}
            Err(e) => return Err(SendFail::Io(e.to_string())),
        }
    }
    Ok(())
}

enum SendFail {
    Cancelled,
    Io(String),
}

/// Run one capture job: `stim` into the re-amp input, `frames` frames back, at the remote device's
/// rate. A Stop (`aborted`) hangs up and returns [`LevelError::Cancelled`].
pub(crate) fn run_job(
    addr: &RemoteAddr,
    stim: &[i32],
    frames: u32,
    aborted: &dyn Fn() -> bool,
) -> Result<Reply, LevelError> {
    if stim.len() > MAX_FRAMES as usize || frames == 0 || frames > MAX_FRAMES {
        return Err(LevelError::Device(format!(
            "remote capture: {} frames in, {frames} out; the remote device takes 1 to {MAX_FRAMES}",
            stim.len()
        )));
    }
    let link = Link(Stream::connect(addr).map_err(|e| {
        LevelError::Device(format!(
            "remote capture: no remote device audio listening at {addr} ({e})"
        ))
    })?);
    let sent = match send(&link, &request(stim, frames), aborted) {
        Ok(()) => Ok(()),
        Err(SendFail::Cancelled) => return Err(LevelError::Cancelled),
        Err(SendFail::Io(e)) => Err(e),
    };
    let budget = match sent {
        // A device whose audio clock runs D times slower takes D times as long, slack included.
        Ok(()) => (Duration::from_secs_f64(f64::from(frames) / f64::from(REMOTE_RATE))
            + REPLY_SLACK)
            .mul_f64(time_scale()),
        Err(_) => REFUSAL_WAIT,
    };
    let failed = |what: String| {
        LevelError::Device(match &sent {
            Ok(()) => format!("remote capture: {what}"),
            Err(e) => format!("remote capture: sending the stimulus failed ({e}), then {what}"),
        })
    };

    let deadline = Instant::now() + budget;
    let mut buf: Vec<u8> = Vec::with_capacity(REPLY_HEADER);
    let mut need = REPLY_HEADER;
    let mut head: Option<(Status, u32, u32, [u32; DIAG])> = None;
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        if buf.len() >= need {
            if head.is_some() {
                break;
            }
            if &buf[..4] != MAGIC {
                return Err(failed(
                    "the reply is not a remote device audio reply".into(),
                ));
            }
            let status = Status::from_wire(u32_at(&buf, 4) as i32);
            let (channels, got) = (u32_at(&buf, 8), u32_at(&buf, 12));
            let mut diag = [0u32; DIAG];
            for (i, d) in diag.iter_mut().enumerate() {
                *d = u32_at(&buf, 16 + 4 * i);
            }
            if status != Status::Ok {
                return Ok(Reply {
                    status,
                    channels,
                    frames: got,
                    diag,
                    samples: Vec::new(),
                });
            }
            if channels != 2 || got != frames {
                return Err(failed(format!(
                    "the remote device answered {channels} channel(s) x {got} frames, not 2 x {frames}"
                )));
            }
            need = REPLY_HEADER + 4 * 2 * frames as usize;
            head = Some((status, channels, got, diag));
            continue;
        }
        if aborted() {
            return Err(LevelError::Cancelled);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(failed(format!(
                "no reply within {:.1} s",
                budget.as_secs_f64()
            )));
        }
        // macOS refuses the timeout with EINVAL once the far end has closed its side; a read
        // then cannot block (it returns the buffered reply or end-of-stream), so go on to it.
        if let Err(e) = link.0.set_read_timeout(POLL.min(left)) {
            if e.kind() != ErrorKind::InvalidInput {
                return Err(failed(format!("read: {e}")));
            }
        }
        let want = (need - buf.len()).min(chunk.len());
        match link.0.read(&mut chunk[..want]) {
            Ok(0) => {
                return Err(failed(format!(
                    "the remote device closed the link after {} of {need} reply bytes",
                    buf.len()
                )))
            }
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == ErrorKind::Interrupted || is_timeout(&e) => {}
            Err(e) => return Err(failed(format!("read: {e}"))),
        }
    }
    let Some((status, channels, frames, diag)) = head else {
        return Err(failed("no reply header".into()));
    };
    let samples = buf[REPLY_HEADER..]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| i32::from_le_bytes(*b))
        .collect();
    Ok(Reply {
        status,
        channels,
        frames,
        diag,
        samples,
    })
}

/// Play `stimulus` (mono, `rate` Hz) into the remote device's re-amp input and return its USB-Out
/// 1/2 for the stimulus plus `tail_ms`, at `rate`. Re-amp is already ON (the caller's job).
pub(crate) fn capture(
    addr: &RemoteAddr,
    stimulus: &[f32],
    rate: u32,
    tail_ms: u64,
    aborted: &dyn Fn() -> bool,
) -> Result<Capture, LevelError> {
    if aborted() {
        return Err(LevelError::Cancelled);
    }
    if rate == 0 {
        return Err(LevelError::Device("remote capture: sample rate 0".into()));
    }
    let want = stimulus.len() + (tail_ms as usize).saturating_mul(rate as usize) / 1000;
    let lead = (REMOTE_RATE * LEAD_IN_MS / 1000) as usize;
    let mut stim = vec![0i32; lead];
    stim.extend(
        resample(stimulus, rate, REMOTE_RATE)
            .map_err(LevelError::Device)?
            .into_iter()
            .map(to_s24),
    );
    let frames = u32::try_from((want as u64 * u64::from(REMOTE_RATE)).div_ceil(u64::from(rate)))
        .unwrap_or(u32::MAX)
        .saturating_add(EDGE_FRAMES);
    let outcome = run_job(addr, &stim, frames, aborted);
    STATS.note(&outcome);
    let reply = outcome?;
    if reply.status != Status::Ok {
        return Err(LevelError::Device(reply.status.error()));
    }
    let [injected, recorded, late, max_late_us, slips, pcm_events, _, _, nonzero] = reply.diag;
    log::debug!(
        "remote capture: {injected} frames in, {recorded} out ({} x {}), {nonzero} non-zero; \
         late blocks {late} (max {max_late_us} us), slips {slips}, PCM events {pcm_events}",
        reply.channels,
        reply.frames
    );
    let channel = |c: usize| -> Result<Vec<f32>, LevelError> {
        let remote: Vec<f32> = reply.samples[c..]
            .iter()
            .step_by(2)
            .map(|&v| from_s24(v))
            .collect();
        resample(&remote, REMOTE_RATE, rate).map_err(LevelError::Device)
    };
    let (left, right) = (channel(0)?, channel(1)?);
    let mut interleaved = Vec::with_capacity(want * 2);
    for i in 0..want {
        interleaved.push(left.get(i).copied().unwrap_or(0.0));
        interleaved.push(right.get(i).copied().unwrap_or(0.0));
    }
    Ok(Capture {
        interleaved,
        channels: 2,
        sample_rate: rate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;

    fn sine(rate: u32, hz: f64, secs: f64, amp: f64) -> Vec<f32> {
        (0..(secs * f64::from(rate)) as usize)
            .map(|k| {
                (amp * (2.0 * std::f64::consts::PI * hz * k as f64 / f64::from(rate)).sin()) as f32
            })
            .collect()
    }

    fn rms_db(x: &[f32]) -> f64 {
        let ms = x.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / x.len() as f64;
        10.0 * ms.log10()
    }

    fn never() -> bool {
        false
    }

    #[test]
    fn resample_keeps_the_rounded_length() {
        for (len, from, to, want) in [
            (48_000, 48_000, 44_100, 44_100),
            (1000, 48_000, 44_100, 919),
            (44_100, 44_100, 48_000, 48_000),
            (7, 44_100, 48_000, 8),
        ] {
            let y = resample(&vec![0.0; len], from, to).unwrap();
            assert_eq!(y.len(), want, "{len} samples {from} -> {to}");
        }
        assert_eq!(
            resample(&[0.5, -0.5], 48_000, 48_000).unwrap(),
            vec![0.5, -0.5]
        );
        assert!(resample(&[0.5], 0, 48_000).is_err());
    }

    #[test]
    fn resample_matches_the_tone_at_the_new_rate() {
        // A 1 kHz tone at 48 kHz, resampled to 44.1 kHz, is the same tone sampled at 44.1 kHz
        // away from the edges (where the burst's own start and stop ring).
        let y = resample(&sine(48_000, 1000.0, 1.0, 0.5), 48_000, 44_100).unwrap();
        let want = sine(44_100, 1000.0, 1.0, 0.5);
        let worst = (4410..39_690)
            .map(|k| (y[k] - want[k]).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 1e-3, "worst error {worst}");
    }

    #[test]
    fn resample_round_trip_keeps_the_level() {
        let x = sine(48_000, 997.0, 2.0, 0.25);
        let there = resample(&x, 48_000, 44_100).unwrap();
        let back = resample(&there, 44_100, 48_000).unwrap();
        assert_eq!(back.len(), x.len());
        let mid = 9600..86_400;
        let delta = rms_db(&back[mid.clone()]) - rms_db(&x[mid]);
        assert!(delta.abs() < 0.001, "round trip moved the level {delta} dB");
    }

    #[test]
    fn resample_drops_content_above_the_lower_nyquist() {
        // 23 kHz is legal at 48 kHz and above 44.1 kHz's Nyquist: it must vanish, not fold.
        let y = resample(&sine(48_000, 23_000.0, 1.0, 0.5), 48_000, 44_100).unwrap();
        let level = rms_db(&y[4410..39_690]);
        assert!(level < -80.0, "23 kHz came through at {level} dBFS");
    }

    #[test]
    fn s24_codec_round_trips_clamps_and_sign_extends() {
        assert_eq!(to_s24(0.5), 4_194_304);
        assert_eq!(to_s24(1.0), 8_388_607);
        assert_eq!(to_s24(-2.0), -8_388_607);
        assert_eq!(to_s24(f32::NAN), 0);
        assert_eq!(from_s24(-1), -1.0 / FULL_SCALE);
        assert_eq!(
            from_s24(0x00FF_FFFF),
            -1.0 / FULL_SCALE,
            "low 24 bits, not sign-extended"
        );
        assert_eq!(from_s24(4_194_304), 0.5);
        for x in [0.0f32, 0.25, -0.75, 0.999] {
            assert!((from_s24(to_s24(x)) - x).abs() <= 1.0 / FULL_SCALE);
        }
    }

    /// What a fake far end saw of one request.
    struct Seen {
        rate: u32,
        frames: u32,
        stim: Vec<i32>,
    }

    fn read_request(s: &mut TcpStream) -> Seen {
        let mut h = [0u8; 24];
        s.read_exact(&mut h).unwrap();
        assert_eq!(&h[..4], MAGIC);
        assert_eq!(u16::from_le_bytes([h[4], h[5]]), VERSION);
        assert_eq!(u16::from_le_bytes([h[6], h[7]]), REQUEST_HEADER);
        assert_eq!(
            u32_at(&h, 20),
            FLAGS,
            "every job asks for the engagement verdict"
        );
        let (rate, n, frames) = (u32_at(&h, 8), u32_at(&h, 12), u32_at(&h, 16));
        let mut body = vec![0u8; 4 * n as usize];
        s.read_exact(&mut body).unwrap();
        let stim = body
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| i32::from_le_bytes(*b))
            .collect();
        Seen { rate, frames, stim }
    }

    fn reply_header(status: i32, channels: u32, frames: u32) -> Vec<u8> {
        let mut r = MAGIC.to_vec();
        r.extend_from_slice(&status.to_le_bytes());
        r.extend_from_slice(&channels.to_le_bytes());
        r.extend_from_slice(&frames.to_le_bytes());
        r.extend_from_slice(&[0u8; 4 * DIAG]);
        r
    }

    /// A far end on an ephemeral port, running `peer` on the one connection it accepts.
    fn far_end(peer: impl FnOnce(TcpStream) + Send + 'static) -> RemoteAddr {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = RemoteAddr::Tcp(l.local_addr().unwrap().to_string());
        std::thread::spawn(move || {
            if let Ok((s, _)) = l.accept() {
                peer(s);
            }
        });
        addr
    }

    /// The far end as the remote device answers a static chain: the stimulus back on both channels,
    /// one 32-frame block late, then silence.
    fn identity(seen: mpsc::Sender<(u32, u32, usize)>) -> impl FnOnce(TcpStream) + Send {
        move |mut s| {
            let req = read_request(&mut s);
            let mut out = reply_header(0, 2, req.frames);
            for k in 0..req.frames as usize {
                let v = k
                    .checked_sub(32)
                    .and_then(|i| req.stim.get(i))
                    .copied()
                    .unwrap_or(0);
                out.extend_from_slice(&v.to_le_bytes());
                out.extend_from_slice(&v.to_le_bytes());
            }
            s.write_all(&out).unwrap();
            let _ = seen.send((req.rate, req.frames, req.stim.len()));
        }
    }

    /// What an identity remote returns, at the caller's rate: the lead-in, the stimulus, then
    /// silence, cut to the capture window.
    fn reference(stim: &[f32], rate: u32, window: usize) -> Vec<f32> {
        let mut r = vec![0.0; (rate * LEAD_IN_MS / 1000) as usize];
        r.extend_from_slice(stim);
        r.resize(window, 0.0);
        r
    }

    #[test]
    fn a_capture_comes_back_at_the_callers_rate_with_the_stimulus_level() {
        let (tx, rx) = mpsc::channel();
        let addr = far_end(identity(tx));
        let stim = sine(48_000, 1000.0, 1.0, 0.25);
        let cap = capture(&addr, &stim, 48_000, 200, &never).unwrap();
        assert_eq!((cap.channels, cap.sample_rate), (2, 48_000));
        assert_eq!(cap.interleaved.len(), 2 * (48_000 + 9600));
        let (rate, frames, sent) = rx.recv().unwrap();
        assert_eq!(rate, REMOTE_RATE);
        assert_eq!(
            sent,
            1367 + 44_100,
            "the lead-in, then the stimulus at 44.1 kHz"
        );
        assert_eq!(
            frames,
            52_920 + EDGE_FRAMES,
            "stimulus + tail at 44.1 kHz, plus the edge"
        );
        // Nothing comes back before the lead-in has passed.
        let lead = (48 * LEAD_IN_MS) as usize;
        assert!(cap.channel(0)[..lead - 48].iter().all(|v| v.abs() < 1e-4));
        // Dual mono through the production meter reads the mono stimulus + 3.01 LU, as the
        // unit's mirrored USB-Out does. The reference carries the same lead-in and silent
        // tail: gated blocks that straddle them count toward the integrated level.
        let padded = reference(&stim, 48_000, stim.len() + 9600);
        let stim_lufs = crate::lufs::measure_mono(&padded, 48_000)
            .unwrap()
            .integrated_lufs;
        let got = crate::leveller::processed_lufs(Ok(cap)).unwrap();
        let delta = got - (stim_lufs + 3.01);
        assert!(
            delta.abs() < 0.02,
            "capture read {got}, stimulus {stim_lufs}: off by {delta}"
        );
    }

    // The leveling stimulus itself (broadband, the full 6 s + 0.8 s window): the resampling
    // both ways must not move its integrated level.
    #[test]
    fn the_leveling_stimulus_keeps_its_level_through_the_remote_rate() {
        let (tx, _rx) = mpsc::channel();
        let addr = far_end(identity(tx));
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/samples/guitar-humbucker.wav"
        );
        let stim = crate::read_stimulus_calibrated(path, None).unwrap();
        let cap = capture(&addr, &stim, 48_000, 800, &never).unwrap();
        let padded = reference(&stim, 48_000, stim.len() + 38_400);
        let stim_lufs = crate::lufs::measure_mono(&padded, 48_000)
            .unwrap()
            .integrated_lufs;
        let got = crate::leveller::processed_lufs(Ok(cap)).unwrap();
        let delta = got - (stim_lufs + 3.01);
        assert!(
            delta.abs() < 0.02,
            "capture read {got}, stimulus {stim_lufs}: off by {delta}"
        );
    }

    #[test]
    fn a_status_comes_back_as_its_reply() {
        let addr = far_end(|mut s| {
            let _ = read_request(&mut s);
            s.write_all(&reply_header(3, 2, 0)).unwrap();
        });
        let reply = run_job(&addr, &[1, 2, 3], 100, &never).unwrap();
        assert_eq!(reply.status, Status::NoAudio);
        assert!(reply.samples.is_empty());
        let addr = far_end(|mut s| {
            let _ = read_request(&mut s);
            s.write_all(&reply_header(3, 2, 0)).unwrap();
        });
        let Err(err) = capture(&addr, &[0.1; 480], 48_000, 0, &never) else {
            panic!("a NO_AUDIO reply must not be a capture");
        };
        assert!(
            err.to_string().contains("audio loop is not running"),
            "{err}"
        );
    }

    // The engagement verdict: a job the remote device ran without re-amp engaged is a loud error,
    // and both outcomes are counted for the remote lane.
    #[test]
    fn a_job_without_re_amp_engaged_is_an_error_and_is_counted() {
        let before = STATS.json();
        for (status, want) in [(7, "re-amp was not engaged"), (8, "changed mid-capture")] {
            let addr = far_end(move |mut s| {
                let _ = read_request(&mut s);
                s.write_all(&reply_header(status, 2, 0)).unwrap();
            });
            let Err(err) = capture(&addr, &[0.1; 4800], 48_000, 100, &never) else {
                panic!("status {status} must not be a capture");
            };
            assert!(err.to_string().contains(want), "{err}");
        }
        let after = STATS.json();
        let grew = |k: &str| after[k].as_u64().unwrap() - before[k].as_u64().unwrap();
        assert!(grew("notEngaged") >= 1 && grew("mixed") >= 1, "{after}");
        assert!(grew("failed") >= 2 && grew("jobs") >= 2, "{after}");
        assert!(after["lastError"].is_string(), "{after}");
    }

    #[test]
    fn a_short_or_malformed_reply_is_an_error() {
        let addr = far_end(|mut s| {
            let req = read_request(&mut s);
            let mut out = reply_header(0, 2, req.frames);
            out.extend_from_slice(&[0u8; 40]);
            s.write_all(&out).unwrap();
        });
        let err = run_job(&addr, &[0; 10], 100, &never).unwrap_err();
        assert!(err.to_string().contains("closed the link after"), "{err}");

        let addr = far_end(|mut s| {
            let req = read_request(&mut s);
            s.write_all(&reply_header(0, 4, req.frames)).unwrap();
        });
        let err = run_job(&addr, &[0; 10], 100, &never).unwrap_err();
        assert!(err.to_string().contains("not 2 x 100"), "{err}");

        let addr = far_end(|mut s| {
            let _ = read_request(&mut s);
            let mut out = reply_header(0, 2, 100);
            out[..4].copy_from_slice(b"HTTP");
            s.write_all(&out).unwrap();
        });
        let err = run_job(&addr, &[0; 10], 100, &never).unwrap_err();
        assert!(
            err.to_string().contains("not a remote device audio reply"),
            "{err}"
        );
    }

    #[test]
    fn a_stop_hangs_up_and_cancels() {
        let (tx, rx) = mpsc::channel();
        let addr = far_end(move |mut s| {
            let _ = read_request(&mut s);
            // Never answer: wait for the hang-up, as the remote device does while a job runs.
            let mut b = [0u8; 1];
            let _ = tx.send(matches!(s.read(&mut b), Ok(0)));
        });
        let t0 = Instant::now();
        let stop = move || t0.elapsed() > Duration::from_millis(300);
        let err = run_job(&addr, &[0; 4410], 44_100, &stop).unwrap_err();
        assert!(err.is_cancelled(), "{err}");
        assert!(
            t0.elapsed() < Duration::from_secs(2),
            "the Stop took {:?}",
            t0.elapsed()
        );
        assert!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            "the far end saw the hang-up"
        );
    }

    #[test]
    fn no_listener_and_an_oversized_job_are_refused() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = RemoteAddr::Tcp(l.local_addr().unwrap().to_string());
        drop(l);
        let err = run_job(&addr, &[0; 10], 100, &never).unwrap_err();
        assert!(
            err.to_string().contains("no remote device audio listening"),
            "{err}"
        );
        let err = run_job(&addr, &[0; 10], MAX_FRAMES + 1, &never).unwrap_err();
        assert!(
            err.to_string().contains("the remote device takes 1 to"),
            "{err}"
        );
    }

    // The far end refuses before reading the stimulus and closes: the send fails, and the
    // refusal is still read. A Unix socket keeps the refusal readable after the far end's
    // close (a TCP reset could discard it), as the remote device's own audio socket does.
    #[cfg(unix)]
    #[test]
    fn a_refusal_before_the_stimulus_is_still_read() {
        use std::os::unix::net::UnixListener;
        let dir = std::env::temp_dir().join(format!("remote-audio-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("refuse.sock");
        let _ = std::fs::remove_file(&path);
        let l = UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut h = [0u8; 20];
                s.read_exact(&mut h).unwrap();
                s.write_all(&reply_header(1, 2, 0)).unwrap();
            }
        });
        let addr = RemoteAddr::Unix(path.clone());
        let reply = run_job(&addr, &vec![0; 30 * 44_100], 100, &never).unwrap();
        assert_eq!(reply.status, Status::Busy);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
