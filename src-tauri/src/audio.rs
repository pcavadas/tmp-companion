//! Host audio I/O for the re-amp loop (cpal = native CoreAudio AUHAL on macOS).
//!
//! The TMP enumerates as a 4-in / 4-out USB-audio device. From the Mac's
//! perspective its *output* channels feed the device's USB-In jacks (re-amp:
//! USB-In 3 = instrument-channel entry) and its *input* channels carry the
//! device's USB-Out (USB-Out 1/2 = processed stereo). M2 builds simultaneous
//! play(ch3)/capture(ch1/2) on top of this; for now we enumerate so we can find
//! the TMP and confirm its channel layout against a real device.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::lufs::IncrementalLoudness;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, SupportedStreamConfig};
use serde::Serialize;

/// 0-based output channel that maps to the device's USB-In 3 (re-amp instrument
/// entry). USB-In 3 is the 3rd input → index 2 from the Mac's output stream.
const REAMP_INSTRUMENT_OUT_CH: usize = 2;

/// 0-based input channel carrying the device's USB-Out 3 = dry instrument send
/// (pre-DSP). Confirmed on hardware: a played guitar lands here at its real
/// output level while USB-Out 1/2 carry the processed signal. Used by Tier-2
/// calibration to measure the instrument's actual output.
pub const DRY_INSTRUMENT_IN_CH: usize = 2;

/// A Core Audio device with its max input/output channel counts and the sample
/// rates it advertises.
#[derive(Debug, Clone, Serialize)]
pub struct AudioDevice {
    pub name: String,
    pub input_channels: u16,
    pub output_channels: u16,
    pub sample_rates: Vec<u32>,
}

/// Enumerate Core Audio devices, merging the input/output views of each device
/// (a device appears in both lists) keyed by name.
pub fn enumerate() -> Vec<AudioDevice> {
    let host = cpal::default_host();
    let mut map: BTreeMap<String, AudioDevice> = BTreeMap::new();

    let mut note = |name: String, in_ch: u16, out_ch: u16, rates: &[u32]| {
        let e = map.entry(name.clone()).or_insert_with(|| AudioDevice {
            name,
            input_channels: 0,
            output_channels: 0,
            sample_rates: Vec::new(),
        });
        e.input_channels = e.input_channels.max(in_ch);
        e.output_channels = e.output_channels.max(out_ch);
        for r in rates {
            if !e.sample_rates.contains(r) {
                e.sample_rates.push(*r);
            }
        }
        e.sample_rates.sort_unstable();
    };

    if let Ok(devs) = host.input_devices() {
        for d in devs {
            let name = d.to_string();
            let (ch, rates) = max_channels_and_rates(d.supported_input_configs().ok());
            note(name, ch, 0, &rates);
        }
    }
    if let Ok(devs) = host.output_devices() {
        for d in devs {
            let name = d.to_string();
            let (ch, rates) = max_channels_and_rates(d.supported_output_configs().ok());
            note(name, 0, ch, &rates);
        }
    }

    map.into_values().collect()
}

/// Best guess at the TMP audio device: the name contains "Tone Master" (case
/// insensitive). Falls back to any device advertising ≥4 in and ≥4 out.
pub fn find_tmp(devices: &[AudioDevice]) -> Option<&AudioDevice> {
    devices
        .iter()
        .find(|d| d.name.to_lowercase().contains("tone master"))
        .or_else(|| {
            devices
                .iter()
                .find(|d| d.input_channels >= 4 && d.output_channels >= 4)
        })
}

fn max_channels_and_rates<I, C>(configs: Option<I>) -> (u16, Vec<u32>)
where
    I: Iterator<Item = C>,
    C: SupportedConfigLike,
{
    let mut ch = 0u16;
    let mut rates: Vec<u32> = Vec::new();
    if let Some(it) = configs {
        for c in it {
            ch = ch.max(c.channels());
            let (lo, hi) = c.sample_rate_range();
            for r in [lo, hi] {
                if !rates.contains(&r) {
                    rates.push(r);
                }
            }
        }
    }
    rates.sort_unstable();
    (ch, rates)
}

/// Tiny abstraction so `max_channels_and_rates` works over cpal's input and
/// output `SupportedStreamConfigRange` without duplicating the loop.
trait SupportedConfigLike {
    fn channels(&self) -> u16;
    fn sample_rate_range(&self) -> (u32, u32);
}

impl SupportedConfigLike for cpal::SupportedStreamConfigRange {
    fn channels(&self) -> u16 {
        cpal::SupportedStreamConfigRange::channels(self)
    }
    fn sample_rate_range(&self) -> (u32, u32) {
        (self.min_sample_rate(), self.max_sample_rate())
    }
}

/// Result of a re-amp capture: the processed return, interleaved across all
/// input channels (the caller picks the loudest = USB-Out 1/2).
pub struct Capture {
    pub interleaved: Vec<f32>,
    pub channels: usize,
    pub sample_rate: u32,
}

impl Capture {
    /// Split into per-channel mono buffers.
    pub fn channel(&self, ch: usize) -> Vec<f32> {
        self.interleaved
            .chunks(self.channels)
            .map(|f| f.get(ch).copied().unwrap_or(0.0))
            .collect()
    }

    /// The channel index carrying the most energy (the processed output, vs the
    /// silent/dry channels), with its RMS — robust to exact channel mapping.
    pub fn loudest_channel(&self) -> (usize, f32) {
        (0..self.channels)
            .map(|c| (c, self.channel_rms(c)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap_or((0, 0.0))
    }

    /// Deterministic mono mixdown of the processed stereo pair (USB-Out 1/2 =
    /// capture channels 0/1): the per-sample average. Kills the argmax-mono flip
    /// `loudest_channel` has on stereo presets (L/R can trade loudest across
    /// runs, flipping spectral verdicts). Channel 2 (dry instrument send) and
    /// beyond are deliberately excluded. Falls back to channel 0 when the
    /// capture is mono.
    pub fn stereo_mix(&self) -> Vec<f32> {
        if self.channels < 2 {
            return self.channel(0);
        }
        self.interleaved
            .chunks(self.channels)
            .map(|f| (f.first().copied().unwrap_or(0.0) + f.get(1).copied().unwrap_or(0.0)) / 2.0)
            .collect()
    }
}

// ── Advisory live-LUFS sink ──────────────────────────────────────────────────
// A leveling command installs a closure (via `LiveLufsGuard` in lib.rs) that emits a Tauri
// event; `reamp_capture_real` calls `emit_live_lufs` on a fixed cadence with the converging
// integrated loudness so the UI can show a "measuring…" readout. The value is ADVISORY — it
// never feeds the solve; the authoritative measurement is unchanged.
//
// ponytail: global advisory sink — safe only because DEVICE_OP_LOCK serializes all leveling
// measurement (one capture at a time). If concurrent measurement is ever introduced, switch
// to a callback threaded through engage_measure_disengage.
type LiveLufsSink = Box<dyn Fn(f64, f64) + Send>;
static LIVE_LUFS_SINK: Mutex<Option<LiveLufsSink>> = Mutex::new(None);

/// Hop cadence for the advisory live-LUFS emit loop (~5 readings/sec).
const LIVE_LUFS_HOP_MS: u64 = 200;

/// Silent-hop level for the advisory momentary meter (the dB the VU rests at).
const MOMENTARY_FLOOR_DB: f64 = -70.0;

/// Install the advisory live-LUFS sink for the duration of one leveling run. Replaces any
/// prior sink (runs are serialized, so there is never more than one in flight).
pub fn set_live_lufs_sink(f: LiveLufsSink) {
    if let Ok(mut g) = LIVE_LUFS_SINK.lock() {
        *g = Some(f);
    }
}

/// Remove the advisory live-LUFS sink (called on `LiveLufsGuard` drop).
pub fn clear_live_lufs_sink() {
    if let Ok(mut g) = LIVE_LUFS_SINK.lock() {
        *g = None;
    }
}

/// Whether a sink is installed — lets `reamp_capture_real` skip the hop loop entirely on
/// probe/CLI paths (zero overhead, no extra buffer lock).
fn live_lufs_active() -> bool {
    LIVE_LUFS_SINK.lock().map(|g| g.is_some()).unwrap_or(false)
}

/// Emit one advisory reading to the installed sink, if any. `momentary` is the current hop's
/// plain RMS in dB (decorative meter fuel, not the solve). The lock is held only for the
/// call (per hop) — nothing else contends for it during a serialized run.
fn emit_live_lufs(integrated: f64, momentary: f64) {
    if let Ok(g) = LIVE_LUFS_SINK.lock() {
        if let Some(f) = g.as_ref() {
            f(integrated, momentary);
        }
    }
}

fn find_device<I: Iterator<Item = Device>>(mut devs: I) -> Option<Device> {
    devs.find(|d| d.to_string().to_lowercase().contains("tone master"))
}

/// Pick an f32 config on `target_rate` with at least `min_ch` channels.
fn pick_config(
    ranges: impl Iterator<Item = cpal::SupportedStreamConfigRange>,
    target_rate: u32,
    min_ch: u16,
) -> Option<SupportedStreamConfig> {
    ranges
        .filter(|r| {
            r.channels() >= min_ch
                && r.sample_format() == SampleFormat::F32
                && r.min_sample_rate() <= target_rate
                && r.max_sample_rate() >= target_rate
        })
        .min_by_key(|r| r.channels()) // smallest channel count that fits
        .map(|r| r.with_sample_rate(target_rate))
}

/// The resolved TMP devices + f32 stream configs for a re-amp session. Shared by
/// `reamp_capture` / `reamp_measure` / `LiveReamp::start` so the device lookup and
/// channel/rate negotiation (the fiddly, error-prone part) cannot diverge.
struct ReampStreams {
    out_dev: Device,
    in_dev: Device,
    out_cfg: SupportedStreamConfig,
    in_cfg: SupportedStreamConfig,
}

/// Find the TMP and pick a 48 kHz f32 output config (≥3 ch for USB-In 3) + input
/// config. Errors describe exactly which half is missing.
fn resolve_reamp_streams(sample_rate: u32) -> Result<ReampStreams, String> {
    let host = cpal::default_host();
    let out_dev = find_device(host.output_devices().map_err(|e| e.to_string())?)
        .ok_or("Tone Master Pro output device not found")?;
    let in_dev = find_device(host.input_devices().map_err(|e| e.to_string())?)
        .ok_or("Tone Master Pro input device not found")?;

    let out_cfg = pick_config(
        out_dev
            .supported_output_configs()
            .map_err(|e| e.to_string())?,
        sample_rate,
        (REAMP_INSTRUMENT_OUT_CH + 1) as u16,
    )
    .ok_or_else(|| format!("no f32 output config at {sample_rate} Hz with ≥3 channels"))?;
    let in_cfg = pick_config(
        in_dev
            .supported_input_configs()
            .map_err(|e| e.to_string())?,
        sample_rate,
        1,
    )
    .ok_or_else(|| format!("no f32 input config at {sample_rate} Hz"))?;

    Ok(ReampStreams {
        out_dev,
        in_dev,
        out_cfg,
        in_cfg,
    })
}

/// Build the re-amp OUTPUT stream that plays `stim` ONCE into USB-In 3 (channel
/// `REAMP_INSTRUMENT_OUT_CH`), silence on every other channel and past the stimulus
/// end, advancing `cursor`. The single source of truth for re-amp signal routing —
/// `reamp_capture` and `reamp_measure` both use it so the injected channel can't
/// drift. (`LiveReamp` loops the stimulus, so it keeps its own modulo variant.)
fn build_oneshot_output_stream(
    streams: &ReampStreams,
    stim: Arc<Vec<f32>>,
    cursor: Arc<AtomicUsize>,
) -> Result<cpal::Stream, String> {
    let out_ch = streams.out_cfg.channels() as usize;
    let err = |e| log::error!("[audio] stream error: {e}");
    streams
        .out_dev
        .build_output_stream(
            streams.out_cfg.config(),
            move |data: &mut [f32], _| {
                for frame in data.chunks_mut(out_ch) {
                    let i = cursor.fetch_add(1, Ordering::Relaxed);
                    let s = stim.get(i).copied().unwrap_or(0.0);
                    for (c, v) in frame.iter_mut().enumerate() {
                        *v = if c == REAMP_INSTRUMENT_OUT_CH { s } else { 0.0 };
                    }
                }
            },
            err,
            None,
        )
        .map_err(|e| format!("build output stream: {e}"))
}

/// Build the capture INPUT stream that appends the device's USB-Out return into
/// `captured`. Shared by `reamp_capture` and `reamp_measure`.
fn build_capture_input_stream(
    streams: &ReampStreams,
    captured: Arc<Mutex<Vec<f32>>>,
) -> Result<cpal::Stream, String> {
    let err = |e| log::error!("[audio] stream error: {e}");
    streams
        .in_dev
        .build_input_stream(
            streams.in_cfg.config(),
            move |data: &[f32], _| {
                if let Ok(mut buf) = captured.lock() {
                    buf.extend_from_slice(data);
                }
            },
            err,
            None,
        )
        .map_err(|e| format!("build input stream: {e}"))
}

/// Play `stimulus_mono` into the TMP's USB-In 3 while recording its processed
/// USB-Out return, for the stimulus duration plus `tail_ms` (to catch reverb/
/// delay decay). Requires re-amp mode already ON (caller's responsibility) and
/// the stimulus at `sample_rate` (the device rate). cpal streams are !Send, so
/// everything stays on the calling thread.
///
/// This is the FULL-CLIP capture (the whole waveform is returned) — used by the
/// spectrum/audit/calibration paths that need the samples. Leveling MEASUREMENTS
/// use [`reamp_measure`], which exits as soon as integrated LUFS converges.
/// Re-amp capture dispatcher. Production AND the ONLINE e2e tier (`TMP_E2E_ONLINE=1`) drive
/// the REAL device audio I/O; only the OFFLINE e2e tier (no audio hardware) substitutes the
/// deterministic fake. The runtime gate — rather than a compile-time `#[cfg]` — is what lets
/// the single `--features e2e` `e2e_server` binary run BOTH tiers: offline against SimDevice
/// and online against the plugged-in unit.
pub fn reamp_capture(
    stimulus_mono: &[f32],
    sample_rate: u32,
    tail_ms: u64,
) -> Result<Capture, String> {
    #[cfg(feature = "e2e")]
    if !crate::e2e_online() {
        // Offline: drive the physics-faithful capture model (the real loudness law +
        // a scene-relative outputLevel term), reading the installed SimDevice's DSP
        // state, so the offline suite is a genuine loudness oracle. `tail_ms` unused
        // (the model is deterministic, no decay tail to integrate).
        return Ok(crate::sim_device::e2e_capture(stimulus_mono, sample_rate));
    }
    reamp_capture_real(stimulus_mono, sample_rate, tail_ms)
}

/// Real re-amp capture over the device's USB audio I/O (the production path; also used by
/// the online e2e tier). Plays the stimulus into USB-In and records the processed USB-Out.
fn reamp_capture_real(
    stimulus_mono: &[f32],
    sample_rate: u32,
    tail_ms: u64,
) -> Result<Capture, String> {
    // ponytail: TMP_AUDIO_TIMING is throwaway probe instrumentation (stream cost breakdown).
    let timing = std::env::var("TMP_AUDIO_TIMING").is_ok();
    let t0 = Instant::now();
    let streams = resolve_reamp_streams(sample_rate)?;
    let t_resolve = t0.elapsed();
    let in_ch = streams.in_cfg.channels() as usize;

    let stim = Arc::new(stimulus_mono.to_vec());
    let cursor = Arc::new(AtomicUsize::new(0));
    let captured = Arc::new(Mutex::new(Vec::<f32>::with_capacity(
        stimulus_mono.len() * in_ch + in_ch * (sample_rate as usize),
    )));

    let out_stream = build_oneshot_output_stream(&streams, stim, cursor)?;
    let in_stream = build_capture_input_stream(&streams, captured.clone())?;

    in_stream.play().map_err(|e| format!("play input: {e}"))?;
    out_stream.play().map_err(|e| format!("play output: {e}"))?;
    let t_ready = t0.elapsed();

    let play_ms = stimulus_mono.len() as u64 * 1000 / sample_rate as u64;
    let total_ms = play_ms + tail_ms;

    if live_lufs_active() {
        // Advisory live-LUFS: emit the converging integrated loudness on a fixed cadence
        // while the SAME buffer fills. DEADLINE-bounded so total wall-clock stays exactly
        // `total_ms` regardless of hop count / emit latency — the authoritative buffer and
        // the final `measure_mono` are byte-identical to the blind-sleep branch below (the
        // meter is parallel, fed from COPIES of the new frames; meter errors are swallowed
        // so a bad reading never aborts a real capture). PICK_MS mirrors `reamp_measure`'s
        // loudest-channel settle.
        const PICK_MS: u64 = 400;
        let deadline = Instant::now() + Duration::from_millis(total_ms);
        let mut loud_ch: Option<usize> = None;
        let mut meter: Option<IncrementalLoudness> = None;
        let mut consumed_frames = 0usize;
        // Decorative per-hop momentary level (plain RMS dB, NOT K-weighted) for the live VU
        // bars — an empty hop re-emits the previous value (the floor before any audio).
        let mut momentary = MOMENTARY_FLOOR_DB;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            std::thread::sleep(remaining.min(Duration::from_millis(LIVE_LUFS_HOP_MS)));

            // Copy only the NEW interleaved frames out from under the lock, then release.
            let (total_frames, new_interleaved) = {
                let b = captured.lock().map_err(|_| "capture buffer poisoned")?;
                let total = b.len() / in_ch;
                let from = consumed_frames * in_ch;
                let to = total * in_ch;
                let slice = if to > from {
                    b[from..to].to_vec()
                } else {
                    Vec::new()
                };
                (total, slice)
            };

            match loud_ch {
                None => {
                    if (total_frames as u64) * 1000 / sample_rate as u64 >= PICK_MS {
                        let pick = Capture {
                            interleaved: new_interleaved,
                            channels: in_ch,
                            sample_rate,
                        };
                        let ch = pick.loudest_channel().0;
                        if let Ok(mut m) = IncrementalLoudness::new(sample_rate) {
                            let _ = m.add(&pick.channel(ch));
                            loud_ch = Some(ch);
                            meter = Some(m);
                            consumed_frames = total_frames;
                        }
                    }
                }
                Some(ch) if !new_interleaved.is_empty() => {
                    let mono: Vec<f32> = new_interleaved[ch..]
                        .iter()
                        .step_by(in_ch)
                        .copied()
                        .collect();
                    consumed_frames = total_frames;
                    let r = rms(&mono) as f64;
                    momentary = if r > 0.0 {
                        (20.0 * r.log10()).max(MOMENTARY_FLOOR_DB)
                    } else {
                        MOMENTARY_FLOOR_DB
                    };
                    if let Some(m) = meter.as_mut() {
                        let _ = m.add(&mono);
                    }
                }
                Some(_) => {}
            }

            if let Some(v) = meter
                .as_ref()
                .and_then(|m| m.integrated().ok())
                .filter(|v| v.is_finite())
            {
                emit_live_lufs(v, momentary);
            }
        }
    } else {
        std::thread::sleep(Duration::from_millis(total_ms));
    }

    let t_sleep_done = t0.elapsed();
    drop(out_stream);
    drop(in_stream);
    if timing {
        eprintln!(
            "[audio-timing] resolve={}ms build+play={}ms teardown={}ms (window={total_ms}ms)",
            t_resolve.as_millis(),
            (t_ready - t_resolve).as_millis(),
            (t0.elapsed() - t_sleep_done).as_millis()
        );
    }

    let interleaved = captured
        .lock()
        .map_err(|_| "capture buffer poisoned")?
        .clone();
    Ok(Capture {
        interleaved,
        channels: in_ch,
        sample_rate,
    })
}

/// Tuning for the [`reamp_measure`] capture.
///
/// Two presets:
/// - [`MeasureOpts::full`] (the leveling DEFAULT): integrate the whole stimulus +
///   decay tail, NO early exit — reproduces the legacy full-capture metric (its only
///   win over the old path is the settle-overlap: the pre-roll replaces the fixed
///   post-engage sleep). USE THIS for anything that writes to a preset.
/// - [`MeasureOpts::adaptive`] (opt-in / harness): early-exit on convergence. FASTER,
///   but the offline harness (`probe --measure-converge-replay`) proved it diverges up
///   to ~0.25 LU from the full metric on time-effect/reverb presets — the post-stimulus
///   decay tail pulls the full integrated down and an early exit omits it. Adopting it
///   is a measurement RE-BASELINE, not a drop-in speedup; gated on a product decision.
#[derive(Debug, Clone, Copy)]
pub struct MeasureOpts {
    /// Capture discarded before measuring — absorbs the re-amp routing settle and
    /// the stimulus attack, so callers no longer sleep a fixed post-engage settle.
    pub preroll_ms: u64,
    /// Poll/measure cadence: how often the integrated value is recomputed.
    pub hop_ms: u64,
    /// Convergence tolerance: `|I(now) − I(prev)| < eps_lu` counts as one stable hop.
    /// Only consulted when `early_exit`.
    pub eps_lu: f64,
    /// Consecutive stable hops required before exiting early. Only when `early_exit`.
    pub stable_k: u32,
    /// Floor on measured (post-preroll) time before convergence may trigger.
    pub min_measure_ms: u64,
    /// Hard ceiling on measured (post-preroll) time — the exit point when
    /// `early_exit` is false.
    pub max_capture_ms: u64,
    /// When false (default), never exit on convergence — run the full window so the
    /// metric matches the legacy full capture.
    pub early_exit: bool,
}

impl MeasureOpts {
    /// Accuracy-preserving default: full stimulus + ~0.8 s tail, no early exit.
    /// `max_capture_ms` = (6.0 s stimulus − 0.5 s preroll) + 0.8 s tail = 6.3 s of
    /// post-preroll capture, matching the legacy 6.0 s + 0.8 s window minus the
    /// settle now folded into the pre-roll.
    pub fn full() -> Self {
        MeasureOpts {
            preroll_ms: 500,
            hop_ms: 200,
            eps_lu: 0.03,
            stable_k: 3,
            min_measure_ms: 6300,
            max_capture_ms: 6300,
            early_exit: false,
        }
    }

    /// Experimental adaptive early-exit (RE-BASELINE — see the type docs). Tuned by
    /// the offline harness; not for write paths without sign-off.
    pub fn adaptive() -> Self {
        MeasureOpts {
            preroll_ms: 500,
            hop_ms: 200,
            eps_lu: 0.03,
            stable_k: 3,
            min_measure_ms: 1500,
            max_capture_ms: 5500,
            early_exit: true,
        }
    }
}

impl Default for MeasureOpts {
    fn default() -> Self {
        MeasureOpts::full()
    }
}

/// The convergence decision shared by the live [`reamp_measure`] and the offline
/// [`replay_measure`], so the harness tunes the SAME state machine production runs.
/// Feed successive integrated readings; `update` returns true once `stable_k`
/// consecutive readings have moved less than `eps_lu`. Non-finite readings (not
/// enough above-gate signal yet) reset nothing and never trigger convergence.
pub struct ConvergenceTracker {
    eps_lu: f64,
    stable_k: u32,
    last: f64,
    stable: u32,
}

impl ConvergenceTracker {
    pub fn new(eps_lu: f64, stable_k: u32) -> Self {
        ConvergenceTracker {
            eps_lu,
            stable_k,
            last: f64::NAN,
            stable: 0,
        }
    }

    pub fn update(&mut self, cur: f64) -> bool {
        if !cur.is_finite() {
            return false;
        }
        if self.last.is_finite() && (cur - self.last).abs() < self.eps_lu {
            self.stable += 1;
        } else {
            self.stable = 0;
        }
        self.last = cur;
        self.stable >= self.stable_k
    }
}

/// Adaptive re-amp loudness measurement: same isolated fresh stream pair as
/// [`reamp_capture`] (NOT the shared `LiveReamp` ring buffer, which mis-measured on
/// HW), but it discards a pre-roll, feeds the loudest channel into an incremental
/// ITU-R BS.1770 meter, and returns integrated LUFS as soon as the value converges
/// (or at `max_capture_ms`). Returns `Err` if no finite signal was captured (re-amp
/// did not route). Requires re-amp mode already ON.
///
/// The pre-roll skip folds in what used to be a fixed post-engage settle: callers
/// engage re-amp and call this directly — the discarded pre-roll covers the routing
/// transient. No tail: the integrated relative gate discards quiet decay, so a tail
/// only costs wall-clock.
pub fn reamp_measure(
    stimulus_mono: &[f32],
    sample_rate: u32,
    opts: MeasureOpts,
) -> Result<f64, String> {
    if stimulus_mono.is_empty() {
        return Err("empty re-amp stimulus".to_string());
    }
    // ~400 ms of post-preroll audio before the loudest channel is fixed — plenty
    // for a stable RMS pick on the stationary shaped-noise stimulus.
    const PICK_MS: u64 = 400;

    let streams = resolve_reamp_streams(sample_rate)?;
    let in_ch = streams.in_cfg.channels() as usize;

    let stim = Arc::new(stimulus_mono.to_vec());
    let cursor = Arc::new(AtomicUsize::new(0));
    // Bounded by max_capture_ms wall-clock, so a plain Vec can't grow without limit.
    let captured = Arc::new(Mutex::new(Vec::<f32>::with_capacity(
        (sample_rate as usize) * in_ch * (opts.max_capture_ms as usize / 1000 + 2),
    )));

    let out_stream = build_oneshot_output_stream(&streams, stim, cursor)?;
    let in_stream = build_capture_input_stream(&streams, captured.clone())?;
    in_stream.play().map_err(|e| format!("play input: {e}"))?;
    out_stream.play().map_err(|e| format!("play output: {e}"))?;

    // Pre-roll: discard the routing/attack transient, then mark the frame-aligned
    // offset where measurement begins.
    std::thread::sleep(Duration::from_millis(opts.preroll_ms));
    let preroll_off = {
        let b = captured.lock().map_err(|_| "capture buffer poisoned")?;
        (b.len() / in_ch) * in_ch
    };

    let mut loud_ch: Option<usize> = None;
    let mut meter: Option<IncrementalLoudness> = None;
    let mut consumed_frames = 0usize;
    let mut tracker = ConvergenceTracker::new(opts.eps_lu, opts.stable_k);
    let measure_start = Instant::now();

    loop {
        std::thread::sleep(Duration::from_millis(opts.hop_ms));
        let elapsed_ms = measure_start.elapsed().as_millis() as u64;

        // One lock per hop: read the post-preroll frame count and copy only the NEW
        // interleaved frames [consumed_frames, total_frames) out from under the lock.
        let (total_frames, new_interleaved) = {
            let b = captured.lock().map_err(|_| "capture buffer poisoned")?;
            let total = b.len().saturating_sub(preroll_off) / in_ch;
            let from = preroll_off + consumed_frames * in_ch;
            let to = preroll_off + total * in_ch;
            let slice = if to > from {
                b[from..to].to_vec()
            } else {
                Vec::new()
            };
            (total, slice)
        };

        // Fix the loudest channel once enough post-preroll audio exists. Before the
        // pick `consumed_frames` is 0, so `new_interleaved` is everything so far.
        match loud_ch {
            None => {
                if total_frames as u64 * 1000 / sample_rate as u64 >= PICK_MS {
                    let pick = Capture {
                        interleaved: new_interleaved,
                        channels: in_ch,
                        sample_rate,
                    };
                    let ch = pick.loudest_channel().0;
                    let mut m = IncrementalLoudness::new(sample_rate)?;
                    m.add(&pick.channel(ch))?; // feed everything captured up to the pick
                    loud_ch = Some(ch);
                    meter = Some(m);
                    consumed_frames = total_frames;
                } else if elapsed_ms >= opts.max_capture_ms {
                    break; // no audio arrived in time → fall through to the Err below
                } else {
                    continue;
                }
            }
            Some(ch) if !new_interleaved.is_empty() => {
                // Deinterleave the chosen channel by striding the new frames.
                let mono: Vec<f32> = new_interleaved[ch..]
                    .iter()
                    .step_by(in_ch)
                    .copied()
                    .collect();
                consumed_frames = total_frames;
                meter.as_mut().unwrap().add(&mono)?;
            }
            Some(_) => {}
        }

        if elapsed_ms < opts.min_measure_ms {
            continue;
        }
        let cur = meter.as_ref().unwrap().integrated().unwrap_or(f64::NAN);
        if opts.early_exit && tracker.update(cur) {
            break;
        }
        if elapsed_ms >= opts.max_capture_ms {
            break;
        }
    }

    drop(out_stream);
    drop(in_stream);

    match meter.and_then(|m| m.integrated().ok()) {
        Some(v) if v.is_finite() => Ok(v),
        _ => Err("no signal captured (re-amp may not have routed)".to_string()),
    }
}

/// Result of an offline [`replay_measure`].
#[derive(Debug, Clone, Copy)]
pub struct ReplayResult {
    /// Integrated LUFS at the exit point (where `reamp_measure` would have stopped).
    pub integrated_lufs: f64,
    /// Measured (post-preroll) time consumed before exit.
    pub exit_ms: u64,
    /// Whether convergence (not the hard cap / buffer end) triggered the exit.
    pub converged: bool,
}

/// Offline twin of [`reamp_measure`]: replay an already-captured MONO channel
/// through the SAME pre-roll skip → hop-fed incremental meter → [`ConvergenceTracker`]
/// so the adaptive constants can be tuned against reference clips with no device.
/// "Elapsed" is derived from samples consumed, not wall-clock.
pub fn replay_measure(
    mono: &[f32],
    sample_rate: u32,
    opts: MeasureOpts,
) -> Result<ReplayResult, String> {
    if mono.is_empty() {
        return Err("empty replay buffer".to_string());
    }
    let preroll = (sample_rate as u64 * opts.preroll_ms / 1000) as usize;
    let hop = (sample_rate as u64 * opts.hop_ms / 1000).max(1) as usize;
    if preroll >= mono.len() {
        return Err("preroll exceeds clip length".to_string());
    }
    let body = &mono[preroll..];

    let mut meter = IncrementalLoudness::new(sample_rate)?;
    let mut tracker = ConvergenceTracker::new(opts.eps_lu, opts.stable_k);
    let mut fed = 0usize;
    let mut converged = false;
    while fed < body.len() {
        let end = (fed + hop).min(body.len());
        meter.add(&body[fed..end])?;
        fed = end;
        let elapsed_ms = fed as u64 * 1000 / sample_rate as u64;
        if elapsed_ms < opts.min_measure_ms {
            continue;
        }
        let cur = meter.integrated().unwrap_or(f64::NAN);
        if opts.early_exit && tracker.update(cur) {
            converged = true;
            break;
        }
        if elapsed_ms >= opts.max_capture_ms {
            break;
        }
    }
    let exit_ms = fed as u64 * 1000 / sample_rate as u64;
    let integrated = meter.integrated().unwrap_or(f64::NAN);
    Ok(ReplayResult {
        integrated_lufs: integrated,
        exit_ms,
        converged,
    })
}

/// Seconds of capture history [`LiveReamp`] retains (ring buffer) — must cover
/// the longest window `recent_capture` is asked for, with margin.
const LIVE_RING_SECS: usize = 8;

/// A continuously-running re-amp stream. Unlike [`reamp_capture`], this loops the
/// stimulus forever and lets the caller measure recent capture windows after live
/// parameter changes without rebuilding CoreAudio streams.
pub struct LiveReamp {
    _out_stream: cpal::Stream,
    _in_stream: cpal::Stream,
    captured: Arc<Mutex<std::collections::VecDeque<f32>>>,
    channels: usize,
    sample_rate: u32,
}

impl LiveReamp {
    /// Start looping `stimulus_mono` into USB-In 3 while recording the processed
    /// return. Requires re-amp mode already ON.
    pub fn start(stimulus_mono: &[f32], sample_rate: u32) -> Result<Self, String> {
        if stimulus_mono.is_empty() {
            return Err("empty re-amp stimulus".to_string());
        }

        let streams = resolve_reamp_streams(sample_rate)?;
        let ReampStreams {
            out_dev,
            in_dev,
            out_cfg,
            in_cfg,
        } = streams;

        let out_ch = out_cfg.channels() as usize;
        let in_ch = in_cfg.channels() as usize;
        let stim = Arc::new(stimulus_mono.to_vec());
        let cursor = Arc::new(AtomicUsize::new(0));
        let captured = Arc::new(Mutex::new(
            std::collections::VecDeque::<f32>::with_capacity(
                sample_rate as usize * LIVE_RING_SECS * in_ch + 4096,
            ),
        ));
        let err = |e| log::error!("[audio] live stream error: {e}");

        let stim_cb = stim.clone();
        let cur_cb = cursor.clone();
        let out_stream = out_dev
            .build_output_stream(
                out_cfg.config(),
                move |data: &mut [f32], _| {
                    for frame in data.chunks_mut(out_ch) {
                        let i = cur_cb.fetch_add(1, Ordering::Relaxed) % stim_cb.len();
                        let s = stim_cb[i];
                        for (c, v) in frame.iter_mut().enumerate() {
                            *v = if c == REAMP_INSTRUMENT_OUT_CH { s } else { 0.0 };
                        }
                    }
                },
                err,
                None,
            )
            .map_err(|e| format!("build output stream: {e}"))?;

        // Ring-buffer the capture: keep only the recent tail the callers can ask
        // for. Unbounded growth here OOM'd the whole machine on a long benchmark
        // run (multi-channel 48 kHz × minutes of stream × dozens of
        // rows). VecDeque so the front-trim is a head-pointer advance — a Vec
        // drain re-based multiple MB on the realtime callback in the worst case.
        let cap_samples = sample_rate as usize * LIVE_RING_SECS * in_ch;
        let cap_cb = captured.clone();
        let in_stream = in_dev
            .build_input_stream(
                in_cfg.config(),
                move |data: &[f32], _| {
                    if let Ok(mut buf) = cap_cb.lock() {
                        buf.extend(data.iter().copied());
                        if buf.len() > cap_samples {
                            let excess = buf.len() - cap_samples;
                            buf.drain(..excess);
                        }
                    }
                },
                err,
                None,
            )
            .map_err(|e| format!("build input stream: {e}"))?;

        in_stream.play().map_err(|e| format!("play input: {e}"))?;
        out_stream.play().map_err(|e| format!("play output: {e}"))?;

        Ok(Self {
            _out_stream: out_stream,
            _in_stream: in_stream,
            captured,
            channels: in_ch,
            sample_rate,
        })
    }

    /// Clone the most recent `window_ms` of captured audio.
    pub fn recent_capture(&self, window_ms: u64) -> Result<Capture, String> {
        let frames = (self.sample_rate as usize * window_ms as usize / 1000).max(1);
        let samples = frames * self.channels;
        let buf = self
            .captured
            .lock()
            .map_err(|_| "live capture buffer poisoned".to_string())?;
        if buf.len() < self.channels {
            return Err("no live audio captured yet".to_string());
        }
        let start = buf.len().saturating_sub(samples);
        Ok(Capture {
            interleaved: buf.iter().skip(start).copied().collect(),
            channels: self.channels,
            sample_rate: self.sample_rate,
        })
    }
}

/// Capture the device's USB-Out (all input channels from the Mac's view) for
/// `secs` seconds WITHOUT playing anything. Used for Tier-2 calibration: with the
/// device in normal mode and the user playing their real guitar, the dry
/// instrument send appears on USB-Out 3 (input channel index 2) and lets us
/// measure that instrument's actual output level.
pub fn capture_input(secs: f32, sample_rate: u32) -> Result<Capture, String> {
    let host = cpal::default_host();
    let in_dev = find_device(host.input_devices().map_err(|e| e.to_string())?)
        .ok_or("Tone Master Pro input device not found")?;
    let in_cfg = pick_config(
        in_dev
            .supported_input_configs()
            .map_err(|e| e.to_string())?,
        sample_rate,
        1,
    )
    .ok_or_else(|| format!("no f32 input config at {sample_rate} Hz"))?;
    let in_ch = in_cfg.channels() as usize;

    let captured = Arc::new(Mutex::new(Vec::<f32>::with_capacity(
        (secs as usize + 1) * sample_rate as usize * in_ch,
    )));
    let err = |e| log::error!("[audio] input stream error: {e}");
    let cap_cb = captured.clone();
    let in_stream = in_dev
        .build_input_stream(
            in_cfg.config(),
            move |data: &[f32], _| {
                if let Ok(mut buf) = cap_cb.lock() {
                    buf.extend_from_slice(data);
                }
            },
            err,
            None,
        )
        .map_err(|e| format!("build input stream: {e}"))?;

    in_stream.play().map_err(|e| format!("play input: {e}"))?;
    std::thread::sleep(Duration::from_millis((secs * 1000.0) as u64));
    drop(in_stream);

    let interleaved = captured
        .lock()
        .map_err(|_| "capture buffer poisoned")?
        .clone();
    Ok(Capture {
        interleaved,
        channels: in_ch,
        sample_rate,
    })
}

/// RMS amplitude (linear) of a sample slice.
fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|x| x * x).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

impl Capture {
    /// Per-channel peak absolute amplitude (linear, 0..1).
    pub fn channel_peak(&self, ch: usize) -> f32 {
        self.channel(ch).iter().fold(0.0f32, |m, &s| m.max(s.abs()))
    }

    /// Per-channel RMS amplitude (linear). Tracks sustained output level (the
    /// Tier-2 calibration metric) far better than peak, which is dominated by
    /// pick-attack transients regardless of pickup output.
    pub fn channel_rms(&self, ch: usize) -> f32 {
        rms(&self.channel(ch))
    }
}

// ───────────────────────── Capture onset estimation ─────────────────────────

/// Longest lag the onset search considers (USB round-trip latency is tens of ms;
/// 250 ms is a generous ceiling).
const ONSET_MAX_LAG_MS: usize = 250;
/// Envelope hop — sets the estimate's resolution (well inside the ±5 ms goal).
const ONSET_HOP_MS: usize = 2;
/// Normalized-correlation floor below which the estimate is not trusted.
/// HW-calibrated (2026-07-16, 15 captures × 5 presets): chains that preserve
/// ANY envelope find the true ~32 ms latency with corr 0.24–0.48 (fuzz
/// compression floors it near 0.24), while envelope-DESTROYING chains (reverse
/// delay, shoegaze wash) sit ≤ 0.08 — 0.15 splits the clusters with margin
/// both ways. The original 0.5 rejected every real capture.
const ONSET_MIN_CORR: f64 = 0.15;
/// Latency plausibility ceiling: the rig's true inject latency measured a tight
/// 30–34 ms across every preset/run, while envelope-destroyed chains produced
/// artifact lags of 190–222 ms (wash buildup correlating with the stimulus
/// head). A best lag beyond this is an artifact regardless of its correlation.
const ONSET_MAX_PLAUSIBLE_LAG_MS: usize = 120;

/// Estimate where the played stimulus actually STARTS inside a capture (the
/// capture begins at stream start, before the audio has propagated through
/// cpal/USB/DSP). Envelope cross-correlation, not waveform: distortion
/// decorrelates the waveform but the amplitude envelope survives any chain, and
/// a constant hiss floor (high-gain presets hiss from engage) defeats an
/// energy-onset detector but not a correlator. Returns `(onset_samples,
/// confident)`; low confidence returns `(0, false)` — the caller keeps the
/// un-aligned behavior.
pub(crate) fn estimate_onset(stimulus: &[f32], capture: &[f32], rate: u32) -> (usize, bool) {
    let hop = (rate as usize * ONSET_HOP_MS / 1000).max(1);
    let max_lag_hops = ONSET_MAX_LAG_MS / ONSET_HOP_MS;
    // Envelope of the stimulus head (~1.5 s) and of the capture head (+ lag room).
    let head_hops = (1500 / ONSET_HOP_MS).min(stimulus.len() / hop);
    if head_hops < 50 {
        return (0, false); // too short to correlate meaningfully
    }
    let env = |x: &[f32], hops: usize| -> Vec<f64> {
        (0..hops)
            .map(|i| {
                let s = &x[i * hop..((i + 1) * hop).min(x.len())];
                (s.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>()
                    / s.len().max(1) as f64)
                    .sqrt()
            })
            .collect()
    };
    let cap_hops = (head_hops + max_lag_hops).min(capture.len() / hop);
    if cap_hops <= head_hops {
        return (0, false);
    }
    let se = env(stimulus, head_hops);
    let ce = env(capture, cap_hops);
    // Zero-mean the stimulus envelope once; correlate at each candidate lag.
    let smean = se.iter().sum::<f64>() / se.len() as f64;
    let sz: Vec<f64> = se.iter().map(|v| v - smean).collect();
    let snorm = sz.iter().map(|v| v * v).sum::<f64>().sqrt();
    if snorm <= 0.0 {
        return (0, false);
    }
    let mut best = (0usize, f64::NEG_INFINITY);
    for lag in 0..=(cap_hops - head_hops).min(max_lag_hops) {
        let win = &ce[lag..lag + head_hops];
        let cmean = win.iter().sum::<f64>() / win.len() as f64;
        let mut dot = 0.0;
        let mut cnorm = 0.0;
        for (s, c) in sz.iter().zip(win.iter().map(|v| v - cmean)) {
            dot += s * c;
            cnorm += c * c;
        }
        let corr = if cnorm > 0.0 {
            dot / (snorm * cnorm.sqrt())
        } else {
            f64::NEG_INFINITY
        };
        if corr > best.1 {
            best = (lag, corr);
        }
    }
    if best.1 >= ONSET_MIN_CORR && best.0 * ONSET_HOP_MS <= ONSET_MAX_PLAUSIBLE_LAG_MS {
        (best.0 * hop, true)
    } else {
        // The diagnosing tell rides in this log: a best lag PINNED at the search
        // ceiling means real latency exceeds ONSET_MAX_LAG_MS (raise the bound);
        // a mid-range lag with low corr means the chain destroyed the envelope.
        log::warn!(
            "estimate_onset: not confident (best corr {:.3} vs {ONSET_MIN_CORR} at lag {} ms, plausible ≤ {ONSET_MAX_PLAUSIBLE_LAG_MS} ms)",
            best.1,
            best.0 * ONSET_HOP_MS
        );
        (0, false)
    }
}

#[cfg(test)]
mod onset_tests {
    use super::*;
    use std::f32::consts::PI;

    const SR: u32 = 48_000;

    /// A pluck train with a distinctive envelope (like the shipped stimuli).
    fn plucky(secs: f32) -> Vec<f32> {
        let n = (secs * SR as f32) as usize;
        let note = SR as usize / 2; // 500 ms notes
        (0..n)
            .map(|i| {
                let t = (i % note) as f32 / SR as f32;
                let env = (-t / 0.12).exp();
                env * (2.0 * PI * 220.0 * i as f32 / SR as f32).sin() * 0.5
            })
            .collect()
    }

    #[test]
    fn recovers_a_known_lag_through_a_clipping_chain() {
        let stim = plucky(2.0);
        let lag = (SR as usize) * 75 / 1000; // 75 ms of leading silence
        let mut cap = vec![0.0f32; lag];
        // A crushing nonlinear "chain" — waveform decorrelates, envelope survives.
        cap.extend(stim.iter().map(|&x| (x * 8.0).tanh() * 0.4));
        cap.extend(std::iter::repeat_n(0.0f32, SR as usize / 2));
        let (onset, confident) = estimate_onset(&stim, &cap, SR);
        assert!(confident);
        let err = (onset as i64 - lag as i64).unsigned_abs() as usize;
        assert!(err <= SR as usize * 5 / 1000, "onset {onset} vs lag {lag}");
    }

    #[test]
    fn hiss_before_the_onset_does_not_fool_it() {
        let stim = plucky(2.0);
        let lag = (SR as usize) * 120 / 1000; // 120 ms
                                              // Constant hiss floor from engage (the high-gain preset case).
        let mut cap: Vec<f32> = (0..lag).map(|i| ((i * 7919) % 97) as f32 * 2e-4).collect();
        cap.extend(
            stim.iter()
                .enumerate()
                .map(|(i, &x)| (x * 3.0).tanh() * 0.4 + ((i * 7919) % 97) as f32 * 2e-4),
        );
        let (onset, confident) = estimate_onset(&stim, &cap, SR);
        assert!(confident);
        let err = (onset as i64 - lag as i64).unsigned_abs() as usize;
        assert!(err <= SR as usize * 5 / 1000, "onset {onset} vs lag {lag}");
    }

    #[test]
    fn uncorrelated_capture_reports_no_confidence_and_zero() {
        let stim = plucky(2.0);
        // Stationary noise, no envelope relation to the stimulus.
        let cap: Vec<f32> = (0..(SR as usize * 3))
            .map(|i| ((i * 104729) % 1009) as f32 / 1009.0 * 0.2 - 0.1)
            .collect();
        let (onset, confident) = estimate_onset(&stim, &cap, SR);
        assert!(!confident);
        assert_eq!(onset, 0);
    }

    #[test]
    fn implausibly_late_match_is_rejected_even_with_high_correlation() {
        // A perfect envelope match at 200 ms — beyond any real inject latency
        // (HW: 30–34 ms across every preset/run). The wash-artifact case: the
        // lag plausibility ceiling must reject it no matter how well the
        // buildup correlates with the stimulus head.
        let stim = plucky(2.0);
        let lag = (SR as usize) * 200 / 1000;
        let mut cap = vec![0.0f32; lag];
        cap.extend(stim.iter().copied());
        cap.extend(std::iter::repeat_n(0.0f32, SR as usize / 2));
        let (onset, confident) = estimate_onset(&stim, &cap, SR);
        assert!(!confident, "200 ms lag must be implausible");
        assert_eq!(onset, 0);
    }

    #[test]
    fn heavily_compressed_envelope_still_confident_at_a_plausible_lag() {
        // Fuzz-style crush: hard clipping flattens the envelope so the
        // correlation lands well under the old 0.5 bar (HW measured 0.24 on a
        // fuzz preset) — the recalibrated floor must still accept the true lag.
        let stim = plucky(2.0);
        let lag = (SR as usize) * 32 / 1000; // the measured rig latency
        let mut cap = vec![0.0f32; lag];
        cap.extend(stim.iter().map(|&x| (x * 40.0).tanh() * 0.3));
        cap.extend(std::iter::repeat_n(0.0f32, SR as usize / 2));
        let (onset, confident) = estimate_onset(&stim, &cap, SR);
        assert!(confident);
        let err = (onset as i64 - lag as i64).unsigned_abs() as usize;
        assert!(err <= SR as usize * 5 / 1000, "onset {onset} vs lag {lag}");
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
    fn stereo_mix_averages_the_processed_pair_and_excludes_dry() {
        // 3-channel interleaved: ch0=[1,1], ch1=[0,0], ch2=[9,9] (dry send).
        let cap = Capture {
            interleaved: vec![1.0, 0.0, 9.0, 1.0, 0.0, 9.0],
            channels: 3,
            sample_rate: 48_000,
        };
        assert_eq!(cap.stereo_mix(), vec![0.5, 0.5]);
    }

    #[test]
    fn stereo_mix_passes_through_mono() {
        let cap = Capture {
            interleaved: vec![0.25, -0.5, 0.75],
            channels: 1,
            sample_rate: 48_000,
        };
        assert_eq!(cap.stereo_mix(), vec![0.25, -0.5, 0.75]);
    }

    #[test]
    fn live_lufs_sink_install_emit_clear() {
        let hits = Arc::new(AtomicUsize::new(0));
        let last = Arc::new(Mutex::new(0.0f64));
        let (h, l) = (hits.clone(), last.clone());
        set_live_lufs_sink(Box::new(move |v, _m| {
            h.fetch_add(1, Ordering::SeqCst);
            *l.lock().unwrap() = v;
        }));
        assert!(live_lufs_active());
        emit_live_lufs(-23.4, -30.0);
        emit_live_lufs(-18.0, -25.0);
        clear_live_lufs_sink();
        assert!(!live_lufs_active());
        emit_live_lufs(-99.0, -99.0); // no sink installed → ignored
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        assert_eq!(*last.lock().unwrap(), -18.0);
    }

    #[test]
    fn tracker_triggers_after_k_stable() {
        let mut t = ConvergenceTracker::new(0.03, 3);
        assert!(!t.update(-20.0)); // first reading: no prior to compare
        assert!(!t.update(-20.01)); // 1 stable
        assert!(!t.update(-20.00)); // 2 stable
        assert!(t.update(-20.02)); // 3 stable → converged
    }

    #[test]
    fn tracker_resets_on_jump() {
        let mut t = ConvergenceTracker::new(0.03, 3);
        t.update(-20.0);
        t.update(-20.0); // 1
        t.update(-30.0); // jump → reset
        assert!(!t.update(-30.0)); // 1 again
        assert!(!t.update(-30.0)); // 2
        assert!(t.update(-30.0)); // 3 → converged
    }

    #[test]
    fn tracker_ignores_nonfinite() {
        let mut t = ConvergenceTracker::new(0.03, 2);
        assert!(!t.update(f64::NEG_INFINITY));
        assert!(!t.update(-20.0)); // first finite
        assert!(!t.update(f64::NAN)); // ignored, stable not advanced
        assert!(!t.update(-20.0)); // 1 stable
        assert!(t.update(-20.0)); // 2 stable → converged
    }

    #[test]
    fn replay_stationary_converges_early_and_matches_full() {
        let rate = 48_000;
        let full_clip = sine(1000.0, 6.0, rate, 0.5);
        let full = crate::lufs::measure_mono(&full_clip, rate)
            .unwrap()
            .integrated_lufs;
        let r = replay_measure(&full_clip, rate, MeasureOpts::adaptive()).unwrap();
        assert!(r.converged, "stationary tone should converge early");
        assert!(r.exit_ms < 4000, "expected early exit, got {}ms", r.exit_ms);
        assert!(
            (r.integrated_lufs - full).abs() < 0.2,
            "adaptive {:.3} vs full {:.3}",
            r.integrated_lufs,
            full
        );
    }

    #[test]
    fn full_opts_never_early_exit() {
        // The leveling default integrates the whole window — even a dead-stationary
        // tone must not converge-exit (that's the accuracy-preserving contract).
        let rate = 48_000;
        let clip = sine(1000.0, 6.0, rate, 0.5);
        let r = replay_measure(&clip, rate, MeasureOpts::full()).unwrap();
        assert!(!r.converged, "full() must never early-exit");
        assert!(
            r.exit_ms >= 4000,
            "full() should run the whole window, got {}ms",
            r.exit_ms
        );
    }

    #[test]
    fn replay_ramping_does_not_exit_early() {
        // Continuously-rising amplitude (~+5.7 dB/s): the gated integrated keeps
        // climbing > eps every hop, so it never gets stable_k stable hops → runs to
        // the hard cap instead of converging on a false plateau.
        let rate = 48_000;
        let secs = 6.0f32;
        let n = (secs * rate as f32) as usize;
        let clip: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / rate as f32;
                let amp = 0.02 * (45.0f32).powf(t / secs); // 0.02 → 0.9
                amp * (2.0 * PI * 1000.0 * t).sin()
            })
            .collect();
        let r = replay_measure(&clip, rate, MeasureOpts::adaptive()).unwrap();
        assert!(!r.converged, "a rising ramp should not converge");
        assert!(
            r.exit_ms >= 4000,
            "should run near the cap, got {}ms",
            r.exit_ms
        );
    }

    #[test]
    fn replay_silent_is_nonfinite() {
        let rate = 48_000;
        let silence = vec![0.0f32; rate as usize * 3];
        let r = replay_measure(&silence, rate, MeasureOpts::adaptive()).unwrap();
        assert!(
            !r.integrated_lufs.is_finite(),
            "silence has no finite loudness"
        );
        assert!(!r.converged);
    }

    #[test]
    fn replay_rejects_empty_and_short() {
        assert!(replay_measure(&[], 48_000, MeasureOpts::default()).is_err());
        // shorter than the pre-roll → err
        let short = vec![0.1f32; 48_000 / 100]; // 10 ms
        assert!(replay_measure(&short, 48_000, MeasureOpts::default()).is_err());
    }

    #[test]
    fn incremental_matches_measure_mono() {
        let rate = 48_000;
        let clip = sine(1000.0, 4.0, rate, 0.4);
        let oneshot = crate::lufs::measure_mono(&clip, rate)
            .unwrap()
            .integrated_lufs;
        let mut m = IncrementalLoudness::new(rate).unwrap();
        for hop in clip.chunks(rate as usize / 10) {
            m.add(hop).unwrap();
        }
        let inc = m.integrated().unwrap();
        assert!(
            (inc - oneshot).abs() < 1e-6,
            "incremental {inc:.6} vs one-shot {oneshot:.6}"
        );
    }
}
