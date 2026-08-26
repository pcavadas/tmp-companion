//! Host audio I/O for the re-amp loop, over `cpal` on both macOS and Linux.
//!
//! The TMP enumerates as a 4-in / 4-out USB-audio device. From the host's
//! perspective its *output* channels feed the device's USB-In jacks (re-amp:
//! USB-In 3 = instrument-channel entry) and its *input* channels carry the
//! device's USB-Out (USB-Out 1/2 = processed stereo).
//!
//! **Device resolution is the one platform boundary** (`find_device`'s `imp`
//! module, mirroring `hid.rs`'s shape): CoreAudio names cpal devices by their USB
//! product string, so a case-insensitive "tone master" substring match is
//! unambiguous on macOS. ALSA carries no such string, and that same substring
//! match is actively WRONG on Linux — cpal also surfaces a non-functional
//! `usbstream:` hint device that renders as the identical "Tone Master Pro" name
//! and sorts first (HW-measured, fw 1.8.58, `probe --audio-devices`), so the naive
//! port would have picked a 0-channel dead end over the real PCM device. Linux
//! instead resolves deterministically via `/proc/asound` by USB vendor id (the
//! TMP's HID and audio interfaces sit on DIFFERENT product ids — 0x0044 vs
//! 0x0047 — so match on "has PCM", not a specific id) and opens the exact
//! `hw:CARD=<id>,DEV=0` PCM.
//!
//! **Format is the second boundary, handled at the stream-callback level, not a
//! platform `cfg`.** The TMP's ALSA `hw:` interface is S32_LE (I32) only — no F32
//! at all (HW-measured). `pick_config` accepts F32 (macOS/CoreAudio) or I32
//! (Linux `hw:`), and `fill_output_frames_f32`/`read_input_frames_f32` convert at
//! the boundary using `dasp_sample`'s exact 2^31 scaling, so every measurement
//! above this module stays in f32 regardless of which format the negotiated
//! stream actually used. Deliberately `hw:`, not `plughw:`: ALSA's `plug` layer
//! converts channel COUNT as well as format, and `pick_config`'s "smallest
//! channel count that fits" would ask `plughw:` for 3 channels — not the
//! physical 4 — inviting an unverified remix of the exact USB-In-3 routing
//! re-amp depends on. `hw:` only ever advertises the physical count.
//!
//! **PipeWire/WirePlumber claiming the TMP as a system audio device blocks `hw:`'s
//! exclusive open with EBUSY** (HW-measured) whenever it's actively holding the
//! card — a WirePlumber rule excluding the TMP by USB vendor/product id
//! (`device.disabled`) is required on Linux dev machines; this module has no way
//! to detect or work around a live PipeWire hold itself.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::lufs::IncrementalLoudness;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Data, Device, SampleFormat, SupportedStreamConfig};
use serde::Serialize;

/// 0-based output channel that maps to the device's USB-In 3 (re-amp instrument
/// entry). USB-In 3 is the 3rd input → index 2 from the Mac's output stream.
const REAMP_INSTRUMENT_OUT_CH: usize = 2;

/// 0-based input channel carrying the device's USB-Out 3 = dry instrument send
/// (pre-DSP). Confirmed on hardware: a played guitar lands here at its real
/// output level while USB-Out 1/2 carry the processed signal. Used by Tier-2
/// calibration to measure the instrument's actual output.
pub const DRY_INSTRUMENT_IN_CH: usize = 2;

/// Parse a `/proc/asound/cardN/usbid` body (`"1ed8:0047\n"`, lowercase hex,
/// colon-separated, no zero-padding) into `(vendor, product)`. Pure so the
/// Linux card-identity lookup is unit-tested on macOS CI too. `pub(crate)`:
/// shared with `probe_api::audio_devices`, which walks `/proc/asound` to
/// report every Fender-VID card regardless of which product id carries PCM.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn parse_asound_usbid(usbid: &str) -> Option<(u16, u16)> {
    let (v, p) = usbid.trim().split_once(':')?;
    let vendor = u16::from_str_radix(v, 16).ok()?;
    let product = u16::from_str_radix(p, 16).ok()?;
    Some((vendor, product))
}

/// A host audio device with its max input/output channel counts, the sample
/// rates and sample formats it advertises, and (on Linux, where a device name
/// alone is ambiguous between `hw:`/`plughw:`/dmix/etc aliases of the same
/// card) the exact ALSA PCM id cpal would open it as.
#[derive(Debug, Clone, Serialize)]
pub struct AudioDevice {
    pub name: String,
    pub driver: Option<String>,
    pub input_channels: u16,
    pub output_channels: u16,
    pub sample_rates: Vec<u32>,
    pub sample_formats: Vec<String>,
}

/// Enumerate host audio devices, merging the input/output views of each device
/// (a device appears in both lists) keyed by **(name, driver)**, not name
/// alone: on Linux several distinct ALSA PCMs (a `hw:` device, a `dmix:`
/// wrapper, a `surroundNN:` hint…) can share the exact same display name, and
/// merging those by name alone would union their channel/rate/format ranges
/// into a chimera that describes none of them (HW-observed: `hw:CARD=Pro_1,
/// DEV=0`'s real S32_LE/4ch/[44100,96000] getting merged with an unrelated
/// hint into a bogus 64ch/F32/[4000,4294967295] entry). `driver` is the exact
/// ALSA pcm_id, so it disambiguates; two entries with the same name AND driver
/// really are the same device's input/output view.
pub fn enumerate() -> Vec<AudioDevice> {
    let host = cpal::default_host();
    let mut map: BTreeMap<(String, Option<String>), AudioDevice> = BTreeMap::new();

    let mut note = |name: String,
                    driver: Option<String>,
                    in_ch: u16,
                    out_ch: u16,
                    rates: &[u32],
                    formats: &[SampleFormat]| {
        let key = (name.clone(), driver.clone());
        let e = map.entry(key).or_insert_with(|| AudioDevice {
            name,
            driver,
            input_channels: 0,
            output_channels: 0,
            sample_rates: Vec::new(),
            sample_formats: Vec::new(),
        });
        e.input_channels = e.input_channels.max(in_ch);
        e.output_channels = e.output_channels.max(out_ch);
        for r in rates {
            if !e.sample_rates.contains(r) {
                e.sample_rates.push(*r);
            }
        }
        e.sample_rates.sort_unstable();
        for f in formats {
            let s = format!("{f:?}");
            if !e.sample_formats.contains(&s) {
                e.sample_formats.push(s);
            }
        }
        e.sample_formats.sort_unstable();
    };

    if let Ok(devs) = host.input_devices() {
        for d in devs {
            let name = d.to_string();
            let driver = d.description().ok().and_then(|desc| {
                desc.driver()
                    .filter(|drv| *drv != name)
                    .map(|drv| drv.to_string())
            });
            let (ch, rates, formats) = channels_rates_formats(d.supported_input_configs().ok());
            note(name, driver, ch, 0, &rates, &formats);
        }
    }
    if let Ok(devs) = host.output_devices() {
        for d in devs {
            let name = d.to_string();
            let driver = d.description().ok().and_then(|desc| {
                desc.driver()
                    .filter(|drv| *drv != name)
                    .map(|drv| drv.to_string())
            });
            let (ch, rates, formats) = channels_rates_formats(d.supported_output_configs().ok());
            note(name, driver, 0, ch, &rates, &formats);
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

fn channels_rates_formats<I, C>(configs: Option<I>) -> (u16, Vec<u32>, Vec<SampleFormat>)
where
    I: Iterator<Item = C>,
    C: SupportedConfigLike,
{
    let mut ch = 0u16;
    let mut rates: Vec<u32> = Vec::new();
    let mut formats: Vec<SampleFormat> = Vec::new();
    if let Some(it) = configs {
        for c in it {
            ch = ch.max(c.channels());
            let (lo, hi) = c.sample_rate_range();
            for r in [lo, hi] {
                if !rates.contains(&r) {
                    rates.push(r);
                }
            }
            let f = c.sample_format();
            if !formats.contains(&f) {
                formats.push(f);
            }
        }
    }
    rates.sort_unstable();
    (ch, rates, formats)
}

/// Tiny abstraction so `channels_rates_formats` works over cpal's input and
/// output `SupportedStreamConfigRange` without duplicating the loop.
trait SupportedConfigLike {
    fn channels(&self) -> u16;
    fn sample_rate_range(&self) -> (u32, u32);
    fn sample_format(&self) -> SampleFormat;
}

impl SupportedConfigLike for cpal::SupportedStreamConfigRange {
    fn channels(&self) -> u16 {
        cpal::SupportedStreamConfigRange::channels(self)
    }
    fn sample_rate_range(&self) -> (u32, u32) {
        (self.min_sample_rate(), self.max_sample_rate())
    }
    fn sample_format(&self) -> SampleFormat {
        cpal::SupportedStreamConfigRange::sample_format(self)
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

    /// [`Self::channel`], but a LOUD error when the capture doesn't carry the
    /// index at all. For load-bearing ABSOLUTE indices (the dry-DI tap) —
    /// `channel`'s zero-pad would masquerade a structurally missing channel as
    /// "the player played nothing".
    pub fn require_channel(&self, ch: usize) -> Result<Vec<f32>, String> {
        if ch >= self.channels {
            return Err(format!(
                "capture carries {} channel(s) — no channel index {ch}; expected the \
                 TMP's {TMP_NATIVE_CHANNELS}-channel USB-Out (is a non-TMP audio \
                 device being matched?)",
                self.channels
            ));
        }
        Ok(self.channel(ch))
    }

    /// The louder of the two PROCESSED channels (USB-Out 1/2 = capture channels
    /// 0/1), with its RMS. Channel 2+ (the dry instrument send) is excluded on
    /// purpose: a guitar plugged in during a leveling run would win a full
    /// argmax and every measurement would read the dry DI instead of the amp.
    pub fn loudest_channel(&self) -> (usize, f32) {
        (0..self.channels.min(2))
            .map(|c| (c, self.channel_rms(c)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap_or((0, 0.0))
    }

    /// Deterministic mono mixdown of the processed stereo pair (USB-Out 1/2 =
    /// capture channels 0/1): the per-sample average. Kills the argmax-mono flip
    /// `loudest_channel` has on stereo presets (L/R can trade loudest across
    /// runs, flipping spectral verdicts). Channel 2 (dry instrument send) and
    /// beyond are deliberately excluded. Falls back to channel 0 when the
    /// capture is mono. Doctor's band/PSD analysis (`leveller::to_stereo`) stays
    /// on this AVERAGE mixdown — see [`Self::processed_stereo`] for the
    /// SUM-convention view the LUFS meter uses instead.
    pub fn stereo_mix(&self) -> Vec<f32> {
        if self.channels < 2 {
            return self.channel(0);
        }
        self.interleaved
            .chunks(self.channels)
            .map(|f| (f.first().copied().unwrap_or(0.0) + f.get(1).copied().unwrap_or(0.0)) / 2.0)
            .collect()
    }

    /// Interleaved 2-channel view of the processed pair (USB-Out 1/2 = capture
    /// channels 0/1) for the standard BS.1770 stereo measure (`lufs::measure_stereo`)
    /// — dry channel 2+ is excluded exactly like `stereo_mix`/`loudest_channel`.
    /// `None` for a genuinely 1-channel capture: the caller falls back to
    /// [`lufs::measure_mono`] on channel 0 rather than duplicating it into fake
    /// dual-mono, which would invent the +3.01 dB the hardware never produced.
    pub fn processed_stereo(&self) -> Option<Vec<f32>> {
        if self.channels < 2 {
            return None;
        }
        Some(
            self.interleaved
                .chunks(self.channels)
                .flat_map(|f| {
                    [
                        f.first().copied().unwrap_or(0.0),
                        f.get(1).copied().unwrap_or(0.0),
                    ]
                })
                .collect(),
        )
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

/// Deinterleave `new_interleaved` (raw `in_ch`-channel frames) down to the
/// processed pair (capture channels 0/1) as an interleaved 2-ch buffer — or
/// return it verbatim when the capture is genuinely mono (`in_ch < 2`). Shared
/// by the live advisory loop (`reamp_capture_real`) and the adaptive measure
/// (`reamp_measure`) so both feed [`IncrementalLoudness`] the SAME
/// stereo-or-mono-fallback view [`Capture::processed_stereo`] uses for a full
/// capture — the converging readout must match the final number.
fn processed_pair_or_mono(new_interleaved: &[f32], in_ch: usize) -> Vec<f32> {
    if in_ch < 2 {
        return new_interleaved.to_vec();
    }
    new_interleaved
        .chunks(in_ch)
        .flat_map(|f| {
            [
                f.first().copied().unwrap_or(0.0),
                f.get(1).copied().unwrap_or(0.0),
            ]
        })
        .collect()
}

/// [`IncrementalLoudness::new`] (mono) or [`IncrementalLoudness::new_stereo`]
/// depending on `in_ch`, mirroring [`processed_pair_or_mono`]'s fallback.
fn incremental_loudness_for(in_ch: usize, sample_rate: u32) -> Result<IncrementalLoudness, String> {
    if in_ch >= 2 {
        IncrementalLoudness::new_stereo(sample_rate)
    } else {
        IncrementalLoudness::new(sample_rate)
    }
}

/// The TMP's native per-side USB channel count (a 4-in / 4-out interface).
const TMP_NATIVE_CHANNELS: u16 = 4;

/// Among "tone master" name matches, the index of the device to open: the
/// first carrying the TMP's NATIVE channel count, else the first match at all.
/// An aggregate that merely CONTAINS the TMP (e.g. "Tone Master Pro + mic")
/// also matches by name and can precede the physical unit in CoreAudio's
/// unspecified enumeration order, but concatenates its sub-devices' channels —
/// absolute indices (the dry-DI tap `DRY_INSTRUMENT_IN_CH` above all) then
/// land on the wrong lane, silently measuring a microphone instead of the
/// guitar. A TMP-only aggregate keeps the native count and layout, so the
/// count test admits it. macOS-only: Linux resolves by `/proc/asound` PCM id
/// (see the `imp` modules), so the native-count tiebreak isn't needed there.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn pick_match_index(channel_counts: &[u16]) -> Option<usize> {
    channel_counts
        .iter()
        .position(|&c| c == TMP_NATIVE_CHANNELS)
        .or((!channel_counts.is_empty()).then_some(0))
}

/// Pick the TMP out of a cpal device iterator. The platform boundary — see the
/// `imp` modules below. `channels_of` reports a candidate's channel count in the
/// caller's direction (input vs output configs): the macOS `imp` uses it to
/// prefer the native-4 unit over a name-matching aggregate; the Linux and
/// fallback `imp`s resolve differently and ignore it.
fn find_device<I, F>(devs: I, channels_of: F) -> Option<Device>
where
    I: Iterator<Item = Device>,
    F: Fn(&Device) -> u16,
{
    imp::find_device(devs, channels_of)
}

/// The ALSA card `find_device` would resolve to on Linux, or `None` if no
/// Fender-VID card with PCM is present. `pub(crate)` so `probe --audio-devices`
/// can report the PRODUCTION resolution (not a re-derived guess) without reaching
/// into the private `imp` module itself — `imp` stays the one platform boundary.
#[cfg(target_os = "linux")]
pub(crate) fn linux_audio_card_id() -> Option<String> {
    imp::tmp_audio_card_id()
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{pick_match_index, Device};

    /// CoreAudio names devices for cpal by their USB product string, so a "tone
    /// master" substring is the match — but a "Tone Master Pro + mic" AGGREGATE
    /// matches the same substring and can precede the physical unit in CoreAudio's
    /// unspecified enumeration order, concatenating its sub-devices' channels so
    /// absolute indices (the dry-DI tap) land on the wrong lane. Prefer the match
    /// whose channel count is the native 4 (`channels_of` counts in the caller's
    /// direction); a TMP-only aggregate keeps the native count, so it's still
    /// admitted, and with no native match we fall back to the first name hit.
    pub(super) fn find_device<I, F>(devs: I, channels_of: F) -> Option<Device>
    where
        I: Iterator<Item = Device>,
        F: Fn(&Device) -> u16,
    {
        let mut matches: Vec<Device> = devs
            .filter(|d| d.to_string().to_lowercase().contains("tone master"))
            .collect();
        let counts: Vec<u16> = matches.iter().map(channels_of).collect();
        pick_match_index(&counts).map(|i| matches.swap_remove(i))
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{parse_asound_usbid, Device};
    use cpal::traits::DeviceTrait;

    /// ALSA carries no USB product string, and cpal enumerates a name-substring
    /// match ambiguously on Linux: a non-functional `usbstream:` hint (0 channels)
    /// also renders as "Tone Master Pro" and sorts before the real `hw:`/`plughw:`
    /// PCM device (HW-measured via `probe --audio-devices`, fw 1.8.58 — the
    /// production `find_device` picked the hint, not the audio interface, before
    /// this port). Resolve deterministically instead: walk `/proc/asound` for the
    /// Fender-VID card that carries PCM (the TMP's HID and audio interfaces are on
    /// DIFFERENT USB product ids — 0x0044 vs 0x0047 — so match by vendor id + "has
    /// a pcm device", not a specific product id), then pick the cpal device whose
    /// exact ALSA pcm_id (`description().driver()`) is that card's `hw:` PCM. `hw:`
    /// (not `plughw:`) deliberately: `plughw:`'s `plug` layer converts channel
    /// COUNT as well as format/rate, and `pick_config`'s "smallest channel count
    /// that fits" would ask it for 3 channels — not the physical 4 — inviting an
    /// unverified channel remix of the precise USB-In-3 routing re-amp depends on.
    /// `hw:` only ever advertises the physical count, so `pick_config` can only
    /// ever land on exactly 4 there. The resulting I32-only format is handled at
    /// the stream-callback boundary (`pick_config`, `fill_output_frames_f32`,
    /// `read_input_frames_f32`). `_channels_of` is unused here — the `/proc/asound`
    /// PCM-id resolution is already unambiguous, so the macOS native-count tiebreak
    /// doesn't apply — but the parameter keeps `find_device`'s signature uniform.
    pub(super) fn find_device<I, F>(mut devs: I, _channels_of: F) -> Option<Device>
    where
        I: Iterator<Item = Device>,
        F: Fn(&Device) -> u16,
    {
        let card_id = tmp_audio_card_id()?;
        let want = format!("hw:CARD={card_id},DEV=0");
        devs.find(|d| {
            d.description()
                .ok()
                .and_then(|desc| desc.driver().map(str::to_string))
                .as_deref()
                == Some(want.as_str())
        })
    }

    /// The ALSA short id (`/proc/asound/cardN/id`, e.g. `"Pro_1"`) of the Fender
    /// card that carries PCM. `pub(crate)` so `probe_api::audio_devices` can report
    /// what this path resolves to without re-walking `/proc/asound` itself.
    pub(crate) fn tmp_audio_card_id() -> Option<String> {
        const FENDER_VID: u16 = 0x1ED8;
        let mut cards: Vec<String> = std::fs::read_dir("/proc/asound")
            .ok()?
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.starts_with("card"))
            .collect();
        cards.sort();
        for card in cards {
            let dir = format!("/proc/asound/{card}");
            let Ok(usbid) = std::fs::read_to_string(format!("{dir}/usbid")) else {
                continue;
            };
            let Some((vendor, _product)) = parse_asound_usbid(&usbid) else {
                continue;
            };
            if vendor != FENDER_VID {
                continue;
            }
            let has_pcm = std::fs::read_dir(&dir)
                .map(|it| {
                    it.filter_map(|e| e.ok())
                        .any(|e| e.file_name().to_string_lossy().starts_with("pcm"))
                })
                .unwrap_or(false);
            if !has_pcm {
                continue;
            }
            if let Ok(id) = std::fs::read_to_string(format!("{dir}/id")) {
                return Some(id.trim().to_string());
            }
        }
        None
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod imp {
    use super::Device;

    pub(super) fn find_device<I, F>(_devs: I, _channels_of: F) -> Option<Device>
    where
        I: Iterator<Item = Device>,
        F: Fn(&Device) -> u16,
    {
        None
    }
}

/// Pick a config on `target_rate` with at least `min_ch` channels, F32 or I32
/// (falling back to I32 only when no F32 config exists). F32 is what CoreAudio
/// always offers; Linux ALSA `hw:` devices are commonly integer-only — the TMP's
/// own USB-audio-class interface is S32_LE (I32) exclusively, no F32 at the `hw:`
/// level (HW-measured, `probe --audio-devices`). The four stream-build sites
/// convert to/from f32 at the callback boundary (`fill_output_frames_f32` /
/// `read_input_frames_f32`) so everything above them keeps working in f32.
fn pick_config(
    ranges: impl Iterator<Item = cpal::SupportedStreamConfigRange>,
    target_rate: u32,
    min_ch: u16,
) -> Option<SupportedStreamConfig> {
    ranges
        .filter(|r| {
            r.channels() >= min_ch
                && matches!(r.sample_format(), SampleFormat::F32 | SampleFormat::I32)
                && r.min_sample_rate() <= target_rate
                && r.max_sample_rate() >= target_rate
        })
        // Smallest channel count that fits, F32 breaking a tie over I32 — ties only
        // arise on hosts that offer both at the same channel count (not the TMP on
        // either platform today, but keeps macOS's exact prior behavior explicit
        // rather than accidental).
        .min_by_key(|r| (r.channels(), r.sample_format() != SampleFormat::F32))
        .map(|r| r.with_sample_rate(target_rate))
}

/// f32 -> i32, matching `dasp_sample`'s own scaling exactly (the crate cpal's typed
/// `f32`/`i32` stream builders use internally) so a captured LUFS reading is
/// identical regardless of which format the negotiated stream happened to use.
fn f32_to_i32(s: f32) -> i32 {
    (s * 2_147_483_648.0) as i32
}

/// i32 -> f32, the exact inverse of [`f32_to_i32`] (same `dasp_sample` convention).
fn i32_to_f32(s: i32) -> f32 {
    s as f32 / 2_147_483_648.0
}

/// Fill a format-agnostic output buffer one frame at a time: `next_sample()` is
/// called once per frame and written to `inject_ch`, every other channel gets
/// silence. The format-agnostic counterpart to a typed `build_output_stream::<f32,
/// _, _>` callback — needed because `pick_config` can now hand back an I32 config
/// (Linux `hw:`), and cpal's typed callback API requires the Rust type to match the
/// negotiated format at compile time. Logs and no-ops on a format `pick_config`
/// cannot produce (defensive; not reachable in practice).
fn fill_output_frames_f32(
    data: &mut Data,
    channels: usize,
    inject_ch: usize,
    mut next_sample: impl FnMut() -> f32,
) {
    match data.sample_format() {
        SampleFormat::F32 => {
            if let Some(buf) = data.as_slice_mut::<f32>() {
                for frame in buf.chunks_mut(channels) {
                    let s = next_sample();
                    for (c, v) in frame.iter_mut().enumerate() {
                        *v = if c == inject_ch { s } else { 0.0 };
                    }
                }
            }
        }
        SampleFormat::I32 => {
            if let Some(buf) = data.as_slice_mut::<i32>() {
                for frame in buf.chunks_mut(channels) {
                    let s = f32_to_i32(next_sample());
                    for (c, v) in frame.iter_mut().enumerate() {
                        *v = if c == inject_ch { s } else { 0 };
                    }
                }
            }
        }
        other => log::error!("[audio] unsupported output sample format {other:?}"),
    }
}

/// Read a format-agnostic input buffer as interleaved f32 samples, calling `push`
/// once per sample in wire order. The format-agnostic counterpart to
/// [`fill_output_frames_f32`] for capture.
fn read_input_frames_f32(data: &Data, mut push: impl FnMut(f32)) {
    match data.sample_format() {
        SampleFormat::F32 => {
            if let Some(buf) = data.as_slice::<f32>() {
                for &s in buf {
                    push(s);
                }
            }
        }
        SampleFormat::I32 => {
            if let Some(buf) = data.as_slice::<i32>() {
                for &s in buf {
                    push(i32_to_f32(s));
                }
            }
        }
        other => log::error!("[audio] unsupported input sample format {other:?}"),
    }
}

/// The resolved TMP devices + stream configs (F32 or I32 — see [`pick_config`]) for
/// a re-amp session. Shared by `reamp_capture` / `reamp_measure` / `LiveReamp::start`
/// so the device lookup and channel/rate negotiation (the fiddly, error-prone part)
/// cannot diverge.
struct ReampStreams {
    out_dev: Device,
    in_dev: Device,
    out_cfg: SupportedStreamConfig,
    in_cfg: SupportedStreamConfig,
}

/// Find the TMP and pick a 48 kHz output config (≥3 ch for USB-In 3) + input
/// config. Errors describe exactly which half is missing.
fn resolve_reamp_streams(sample_rate: u32) -> Result<ReampStreams, String> {
    let host = cpal::default_host();
    let out_dev = find_device(host.output_devices().map_err(|e| e.to_string())?, |d| {
        channels_rates_formats(d.supported_output_configs().ok()).0
    })
    .ok_or("Tone Master Pro output device not found")?;
    let in_dev = find_device(host.input_devices().map_err(|e| e.to_string())?, |d| {
        channels_rates_formats(d.supported_input_configs().ok()).0
    })
    .ok_or("Tone Master Pro input device not found")?;

    let out_cfg = pick_config(
        out_dev
            .supported_output_configs()
            .map_err(|e| e.to_string())?,
        sample_rate,
        (REAMP_INSTRUMENT_OUT_CH + 1) as u16,
    )
    .ok_or_else(|| format!("no F32/I32 output config at {sample_rate} Hz with ≥3 channels"))?;
    let in_cfg = pick_config(
        in_dev
            .supported_input_configs()
            .map_err(|e| e.to_string())?,
        sample_rate,
        1,
    )
    .ok_or_else(|| format!("no F32/I32 input config at {sample_rate} Hz"))?;

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
    let fmt = streams.out_cfg.sample_format();
    let err = |e| log::error!("[audio] stream error: {e}");
    streams
        .out_dev
        .build_output_stream_raw(
            streams.out_cfg.config(),
            fmt,
            move |data: &mut Data, _| {
                fill_output_frames_f32(data, out_ch, REAMP_INSTRUMENT_OUT_CH, || {
                    let i = cursor.fetch_add(1, Ordering::Relaxed);
                    stim.get(i).copied().unwrap_or(0.0)
                });
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
    let fmt = streams.in_cfg.sample_format();
    let err = |e| log::error!("[audio] stream error: {e}");
    streams
        .in_dev
        .build_input_stream_raw(
            streams.in_cfg.config(),
            fmt,
            move |data: &Data, _| {
                if let Ok(mut buf) = captured.lock() {
                    read_input_frames_f32(data, |s| buf.push(s));
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
    // A Stop landed during the pre-capture settles — don't spin up the CoreAudio streams
    // just to tear them down one hop later.
    if crate::device_gate::op_aborted() {
        return Err(crate::leveller::CANCELLED.to_string());
    }
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

    {
        // Advisory live-LUFS: emit the converging integrated loudness on a fixed cadence
        // while the SAME buffer fills. DEADLINE-bounded so total wall-clock stays exactly
        // `total_ms` regardless of hop count / emit latency — the authoritative buffer and
        // the final measurement (leveller.rs's stereo-or-mono-fallback hub) are
        // byte-identical to a blind `sleep(total_ms)` (the meter is parallel, fed from
        // COPIES of the new frames; meter errors are swallowed so a bad reading never
        // aborts a real capture). PICK_MS mirrors `reamp_measure`'s settle before the
        // meter starts (and, separately, the momentary-VU channel pick).
        //
        // The loop runs even with NO sink installed (the Doctor, `probe`) — it is also
        // what makes the capture window STOPPABLE: this is the single longest wait in a
        // leveling/Doctor run (6.8 s / 4.7 s), so polling the abort flag per hop is the
        // difference between "Stop" landing in ~0.2 s and sitting out the whole capture.
        // Bailing mid-capture is free: a cancelled measurement is discarded either way,
        // and the caller still sends its re-amp OFF on the open session.
        const PICK_MS: u64 = 400;
        let live = live_lufs_active();
        let deadline = Instant::now() + Duration::from_millis(total_ms);
        let mut loud_ch: Option<usize> = None;
        let mut meter: Option<IncrementalLoudness> = None;
        let mut consumed_frames = 0usize;
        // Decorative per-hop momentary level (plain RMS dB, NOT K-weighted) for the live VU
        // bars — an empty hop re-emits the previous value (the floor before any audio).
        let mut momentary = MOMENTARY_FLOOR_DB;
        while Instant::now() < deadline {
            // The hop sleep IS the abort poll — one cadence, no second timer.
            let remaining = deadline.saturating_duration_since(Instant::now());
            crate::sleep_or_cancel(
                remaining
                    .min(Duration::from_millis(LIVE_LUFS_HOP_MS))
                    .as_millis() as u64,
            )?;
            if !live {
                continue;
            }

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
                            interleaved: new_interleaved.clone(),
                            channels: in_ch,
                            sample_rate,
                        };
                        // `ch` drives only the momentary VU pick below — the LUFS
                        // meter itself always gets the processed pair (or the mono
                        // fallback), never a single argmax-picked channel.
                        let ch = pick.loudest_channel().0;
                        if let Ok(mut m) = incremental_loudness_for(in_ch, sample_rate) {
                            let _ = m.add(&processed_pair_or_mono(&new_interleaved, in_ch));
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
                        let _ = m.add(&processed_pair_or_mono(&new_interleaved, in_ch));
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
/// HW), but it discards a pre-roll, feeds the processed pair (or the mono fallback
/// for a genuinely 1-channel capture) into an incremental ITU-R BS.1770 meter, and
/// returns integrated LUFS as soon as the value converges
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
    // ~400 ms of post-preroll audio before the meter starts (and, separately, the
    // momentary-VU loudest-channel pick fixes) — plenty for a stable RMS pick on
    // the stationary shaped-noise stimulus.
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
        // As in `reamp_capture_real`: `ch` is kept only so `loud_ch` marks "have we
        // picked yet" (its value is otherwise unused now) — the LUFS meter always
        // gets the processed pair (or the mono fallback), never a single channel.
        match loud_ch {
            None => {
                if total_frames as u64 * 1000 / sample_rate as u64 >= PICK_MS {
                    let pick = Capture {
                        interleaved: new_interleaved.clone(),
                        channels: in_ch,
                        sample_rate,
                    };
                    let ch = pick.loudest_channel().0;
                    let mut m = incremental_loudness_for(in_ch, sample_rate)?;
                    m.add(&processed_pair_or_mono(&new_interleaved, in_ch))?; // feed everything captured up to the pick
                    loud_ch = Some(ch);
                    meter = Some(m);
                    consumed_frames = total_frames;
                } else if elapsed_ms >= opts.max_capture_ms {
                    break; // no audio arrived in time → fall through to the Err below
                } else {
                    continue;
                }
            }
            Some(_) if !new_interleaved.is_empty() => {
                consumed_frames = total_frames;
                meter
                    .as_mut()
                    .unwrap()
                    .add(&processed_pair_or_mono(&new_interleaved, in_ch))?;
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

/// Offline twin of [`reamp_measure`]: replay an already-captured buffer of
/// `channels` channels (1 = mono, 2 = interleaved processed pair) through the
/// SAME pre-roll skip → hop-fed incremental meter → [`ConvergenceTracker`] so
/// the adaptive constants can be tuned against reference clips with no device.
/// "Elapsed" is derived from FRAMES consumed (`channels`-aware), not wall-clock.
pub fn replay_measure(
    interleaved: &[f32],
    sample_rate: u32,
    channels: u32,
    opts: MeasureOpts,
) -> Result<ReplayResult, String> {
    if interleaved.is_empty() {
        return Err("empty replay buffer".to_string());
    }
    let ch = channels.max(1) as usize;
    let preroll = (sample_rate as u64 * opts.preroll_ms / 1000) as usize * ch;
    let hop = ((sample_rate as u64 * opts.hop_ms / 1000).max(1) as usize) * ch;
    if preroll >= interleaved.len() {
        return Err("preroll exceeds clip length".to_string());
    }
    let body = &interleaved[preroll..];

    let mut meter = if ch >= 2 {
        IncrementalLoudness::new_stereo(sample_rate)?
    } else {
        IncrementalLoudness::new(sample_rate)?
    };
    let mut tracker = ConvergenceTracker::new(opts.eps_lu, opts.stable_k);
    let mut fed = 0usize;
    let mut converged = false;
    while fed < body.len() {
        let end = (fed + hop).min(body.len());
        meter.add(&body[fed..end])?;
        fed = end;
        let elapsed_ms = (fed / ch) as u64 * 1000 / sample_rate as u64;
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
    let exit_ms = (fed / ch) as u64 * 1000 / sample_rate as u64;
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

/// Append `data` to the capture ring, trimming the FRONT so the buffer never exceeds
/// `cap` samples. The OOM guard behind the `LiveReamp` ring: unbounded capture growth
/// (multi-channel 48 kHz × minutes × dozens of rows) once locked up the whole machine.
/// Extracted from the realtime input callback so the invariant is unit-testable without
/// CoreAudio hardware. `VecDeque` so the trim is a head-pointer advance, not a re-base.
fn ring_append(buf: &mut std::collections::VecDeque<f32>, data: &[f32], cap: usize) {
    buf.extend(data.iter().copied());
    if buf.len() > cap {
        let excess = buf.len() - cap;
        buf.drain(..excess);
    }
}

/// Format-agnostic counterpart to [`ring_append`] for the raw stream callback: F32
/// is a zero-copy passthrough (byte-identical to the original always-f32 path); I32
/// (Linux `hw:`) converts sample-by-sample straight into the ring, so the realtime
/// callback never allocates on either format.
fn ring_append_raw(buf: &mut std::collections::VecDeque<f32>, data: &Data, cap: usize) {
    match data.sample_format() {
        SampleFormat::F32 => {
            if let Some(s) = data.as_slice::<f32>() {
                ring_append(buf, s, cap);
            }
        }
        SampleFormat::I32 => {
            if let Some(s) = data.as_slice::<i32>() {
                buf.extend(s.iter().map(|&v| i32_to_f32(v)));
                if buf.len() > cap {
                    let excess = buf.len() - cap;
                    buf.drain(..excess);
                }
            }
        }
        other => log::error!("[audio] unsupported input sample format {other:?}"),
    }
}

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
        let out_fmt = out_cfg.sample_format();
        let out_stream = out_dev
            .build_output_stream_raw(
                out_cfg.config(),
                out_fmt,
                move |data: &mut Data, _| {
                    fill_output_frames_f32(data, out_ch, REAMP_INSTRUMENT_OUT_CH, || {
                        let i = cur_cb.fetch_add(1, Ordering::Relaxed) % stim_cb.len();
                        stim_cb[i]
                    });
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
        let in_fmt = in_cfg.sample_format();
        let in_stream = in_dev
            .build_input_stream_raw(
                in_cfg.config(),
                in_fmt,
                move |data: &Data, _| {
                    if let Ok(mut buf) = cap_cb.lock() {
                        ring_append_raw(&mut buf, data, cap_samples);
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
/// measure that instrument's actual output level. The config MUST carry that
/// index — a sub-3-channel negotiation fails here, loudly, instead of letting
/// `Capture::channel`'s zero-pad read as "the player played nothing".
pub fn capture_input(secs: f32, sample_rate: u32) -> Result<Capture, String> {
    let host = cpal::default_host();
    let in_dev = find_device(host.input_devices().map_err(|e| e.to_string())?, |d| {
        channels_rates_formats(d.supported_input_configs().ok()).0
    })
    .ok_or("Tone Master Pro input device not found")?;
    let in_cfg = pick_config(
        in_dev
            .supported_input_configs()
            .map_err(|e| e.to_string())?,
        sample_rate,
        (DRY_INSTRUMENT_IN_CH + 1) as u16,
    )
    .ok_or_else(|| {
        format!(
            "no F32/I32 input config at {sample_rate} Hz with ≥{} channels — the dry \
             instrument tap is USB-Out 3; is a non-TMP device named \"Tone Master\" \
             selected?",
            DRY_INSTRUMENT_IN_CH + 1
        )
    })?;
    let in_ch = in_cfg.channels() as usize;

    let captured = Arc::new(Mutex::new(Vec::<f32>::with_capacity(
        (secs as usize + 1) * sample_rate as usize * in_ch,
    )));
    let in_fmt = in_cfg.sample_format();
    let err = |e| log::error!("[audio] input stream error: {e}");
    let cap_cb = captured.clone();
    let in_stream = in_dev
        .build_input_stream_raw(
            in_cfg.config(),
            in_fmt,
            move |data: &Data, _| {
                if let Ok(mut buf) = cap_cb.lock() {
                    read_input_frames_f32(data, |s| buf.push(s));
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
/// `pub(crate)` so `leveller::DOCTOR_ONSET_MAX_LATENCY_MS` derives from this
/// ONE value instead of carrying its own copy.
pub(crate) const ONSET_MAX_PLAUSIBLE_LAG_MS: usize = 120;

// ───────────────────── Floor-relative energy onset step ─────────────────────
//
// HW evidence (fs13 `ACD_TMLargePlate` 65%-wet, `probe --doctor-fs 407 13`,
// 2026-08-24): the correlation curve above is FLAT on a wash chain (0.27–0.37
// across every lag 0–250 ms, no peak) — its confidence gate can't detect a
// peakless curve, so a wash preset's tail split rides on whichever lag the
// noise floor happens to land on. The Doctor stimulus always carries a played
// silent pad (`leveller::DOCTOR_PAD_MS`) ahead of the body, so a floor-relative
// energy step is deterministic (±2 ms) on every chain observed, wet or dry —
// see `leveller::doctor_onset`, which tries this FIRST and falls back to the
// correlator above.

/// Hop width (ms) for the coarse pre-signal floor estimate — wider than
/// `ONSET_HOP_MS`'s 2 ms search resolution so a single engage-click doesn't
/// spike the floor's own RMS; the floor takes the MAX across these hops (a
/// mean would be dragged down by the click instead of raised by it).
const ENERGY_FLOOR_HOP_MS: usize = 20;
/// A capture is "hot from sample 0" (no silent pre-roll to floor against) when
/// the floor window sits within this many dB of the loudest hop in the whole
/// capture — the step-search below would find nothing meaningful.
const ENERGY_HOT_FROM_ZERO_DB: f64 = 6.0;
/// Absolute floor for a digital-zero (silent) capture: without this, a true
/// zero-RMS floor produces a zero threshold that any noise trips immediately.
const ENERGY_ABS_FLOOR: f64 = 1e-5;

/// [`estimate_signal_start`]'s floor-estimate window — long enough to smooth
/// an engage click into the floor (not trigger on it) while sitting inside the
/// Doctor pad's ~230 ms floor coverage (`leveller::DOCTOR_PAD_MS` + true
/// latency). HW-derived (`fs13_wash_envelope_2ms` fixture, 11 real captures).
pub(crate) const ONSET_ENERGY_FLOOR_WINDOW_MS: usize = 150;
/// Amplitude step (dB, RMS) above the floor that marks the true onset — the
/// real floor sits −90…−100 dB with a few dB of engage-pop wobble; the step
/// itself is ~60 dB, so 12 dB clears the wobble with wide margin.
const ONSET_ENERGY_STEP_DB: f64 = 12.0;
/// Consecutive 2 ms hops the step must hold for before it's trusted.
const ONSET_ENERGY_HOLD_HOPS: usize = 3;

/// Find where the capture's energy steps up from its own pre-signal floor —
/// the Doctor's primary onset estimator (see the module note above).
///
/// [`ONSET_ENERGY_FLOOR_WINDOW_MS`] is scanned in `ENERGY_FLOOR_HOP_MS` hops
/// and the floor reference is the MAX of those hops' RMS (never a mean — an
/// engage pop must raise the threshold, not trigger it). The search then
/// scans forward in `ONSET_HOP_MS` hops for the first run of
/// [`ONSET_ENERGY_HOLD_HOPS`] consecutive hops whose RMS exceeds
/// `floor_ref · 10^(ONSET_ENERGY_STEP_DB/20)` (plus [`ENERGY_ABS_FLOOR`] so a
/// true digital-zero floor can't produce a zero threshold), returning the
/// sample index of the run's FIRST hop.
///
/// Returns `None` when no hop ever qualifies (a silent capture) or when the
/// floor window is already within [`ENERGY_HOT_FROM_ZERO_DB`] of the loudest
/// hop in the capture (no silent pre-roll to step away from — the capture is
/// hot from sample 0). The search only ever begins AFTER the floor window, so
/// a signal that starts INSIDE the first [`ONSET_ENERGY_FLOOR_WINDOW_MS`] of
/// the capture is never scanned for — it reads as hot-from-zero and this
/// returns `None`, not a found-but-early onset. Relative to a pad of `P` ms
/// (see `leveller::DOCTOR_PAD_MS`), that bounds the negative latency this can
/// ever report at `-(P - ONSET_ENERGY_FLOOR_WINDOW_MS)` — an envelope, not a
/// tightly-attained bound, since the floor window itself rounds UP to a whole
/// number of [`ENERGY_FLOOR_HOP_MS`] sub-hops, making the practical reach a
/// few ms tighter than the nominal figure.
pub(crate) fn estimate_signal_start(capture: &[f32], rate: u32) -> Option<usize> {
    if capture.is_empty() {
        return None;
    }
    let hop = (rate as usize * ONSET_HOP_MS / 1000).max(1);
    let floor_hop = (rate as usize * ENERGY_FLOOR_HOP_MS / 1000).max(1);
    let floor_hops = ONSET_ENERGY_FLOOR_WINDOW_MS
        .div_ceil(ENERGY_FLOOR_HOP_MS)
        .max(1);
    let floor_samples = (floor_hops * floor_hop).min(capture.len());
    let floor_ref = (0..floor_hops)
        .map(|i| {
            let start = (i * floor_hop).min(capture.len());
            let end = ((i + 1) * floor_hop).min(capture.len());
            crate::doctor::rms_f64(&capture[start..end])
        })
        .fold(0.0f64, f64::max)
        .max(ENERGY_ABS_FLOOR);

    let total_hops = capture.len() / hop;
    if total_hops == 0 {
        return None;
    }
    // ONE 2 ms-hop RMS pass, reused below for both the loudest-hop fold and
    // the step search — the floor window above keeps its own coarser 20 ms
    // hops (a click-smoothing concern the step search doesn't share).
    let hop_rms: Vec<f64> = (0..total_hops)
        .map(|i| crate::doctor::rms_f64(&capture[i * hop..((i + 1) * hop).min(capture.len())]))
        .collect();
    let loudest = hop_rms.iter().copied().fold(0.0f64, f64::max);
    if loudest <= 0.0 {
        return None; // digital silence throughout
    }
    if 20.0 * (loudest / floor_ref).log10() < ENERGY_HOT_FROM_ZERO_DB {
        return None; // no quiet pre-roll to step away from
    }

    let threshold = floor_ref * 10f64.powf(ONSET_ENERGY_STEP_DB / 20.0);
    let start_hop = floor_samples / hop;
    let mut run = 0usize;
    for (i, &r) in hop_rms.iter().enumerate().skip(start_hop) {
        if r > threshold {
            run += 1;
            if run >= ONSET_ENERGY_HOLD_HOPS {
                return Some((i + 1 - ONSET_ENERGY_HOLD_HOPS) * hop);
            }
        } else {
            run = 0;
        }
    }
    None
}

/// Estimate where the played stimulus actually STARTS inside a capture (the
/// capture begins at stream start, before the audio has propagated through
/// cpal/USB/DSP). Envelope cross-correlation, not waveform: distortion
/// decorrelates the waveform but the amplitude envelope survives any chain, and
/// a constant hiss floor (high-gain presets hiss from engage) defeats an
/// energy-onset detector but not a correlator. Returns `(onset_samples,
/// confident)`; low confidence returns `(0, false)` — the caller keeps the
/// un-aligned behavior.
///
/// Doctor callers no longer use this as their primary onset estimate — its
/// confidence gate is a corr floor + lag ceiling, neither of which detects a
/// PEAKLESS correlation curve (measured flat, 0.27–0.37 over every lag 0–250 ms,
/// on a 65%-wet reverb chain). [`leveller::doctor_onset`] tries
/// [`estimate_signal_start`] first and falls back to this correlator.
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
    use crate::test_support::plucky;

    const SR: u32 = 48_000;

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
    fn f32_i32_round_trip_matches_dasp_sample_scaling() {
        // Exact `dasp_sample` formulas (2^31, not i32::MAX) — a mismatch here would
        // silently mis-scale every LUFS reading captured over the Linux hw:/I32 path.
        assert_eq!(f32_to_i32(0.0), 0);
        assert_eq!(f32_to_i32(0.5), 1_073_741_824); // 2^30
        assert_eq!(f32_to_i32(-0.5), -1_073_741_824);
        assert_eq!(i32_to_f32(0), 0.0);
        assert_eq!(i32_to_f32(1_073_741_824), 0.5);
        for s in [-1.0, -0.25, 0.0, 0.001, 0.25, 0.999] {
            let roundtrip = i32_to_f32(f32_to_i32(s));
            assert!(
                (roundtrip - s).abs() < 1e-9,
                "{s} round-tripped to {roundtrip}"
            );
        }
    }

    /// Build a `Data` over a local buffer for the raw-callback tests below. `unsafe`
    /// only because `Data::from_parts` is — the buffer outlives every call made with
    /// it in these tests, matching its safety contract.
    fn data_from<T: cpal::SizedSample>(buf: &mut [T], fmt: SampleFormat) -> Data {
        unsafe { Data::from_parts(buf.as_mut_ptr().cast(), buf.len(), fmt) }
    }

    #[test]
    fn fill_output_frames_f32_injects_into_the_target_channel_only() {
        let mut buf = vec![0.0f32; 8]; // 2 frames × 4 channels
        let mut data = data_from(&mut buf, SampleFormat::F32);
        let mut samples = [0.7f32, -0.3].into_iter();
        fill_output_frames_f32(&mut data, 4, 2, || samples.next().unwrap());
        assert_eq!(buf, vec![0.0, 0.0, 0.7, 0.0, 0.0, 0.0, -0.3, 0.0]);
    }

    #[test]
    fn fill_output_frames_i32_injects_the_scaled_sample_into_the_target_channel_only() {
        let mut buf = vec![0i32; 8]; // 2 frames × 4 channels
        let mut data = data_from(&mut buf, SampleFormat::I32);
        let mut samples = [0.5f32, -0.5].into_iter();
        fill_output_frames_f32(&mut data, 4, 2, || samples.next().unwrap());
        assert_eq!(buf, vec![0, 0, 1_073_741_824, 0, 0, 0, -1_073_741_824, 0]);
    }

    #[test]
    fn read_input_frames_f32_passes_f32_through_unchanged() {
        let mut buf = vec![0.1f32, -0.2, 0.3];
        let data = data_from(&mut buf, SampleFormat::F32);
        let mut got = Vec::new();
        read_input_frames_f32(&data, |s| got.push(s));
        assert_eq!(got, vec![0.1, -0.2, 0.3]);
    }

    #[test]
    fn read_input_frames_i32_converts_every_sample() {
        let mut buf = vec![1_073_741_824i32, -1_073_741_824, 0];
        let data = data_from(&mut buf, SampleFormat::I32);
        let mut got = Vec::new();
        read_input_frames_f32(&data, |s| got.push(s));
        assert_eq!(got, vec![0.5, -0.5, 0.0]);
    }

    // The "locked up my machine" gate: the LiveReamp capture ring stays bounded at
    // `cap` samples no matter how much sustained input is pushed (unbounded growth once
    // OOM'd the whole Mac). Feeds many chunks totalling far more than `cap` and asserts
    // the length never exceeds it, and that the RETAINED tail is the most recent samples.
    #[test]
    fn ring_append_stays_bounded_under_sustained_pushes() {
        let cap = 1000usize;
        let mut buf = std::collections::VecDeque::<f32>::new();
        let mut n = 0f32;
        for _ in 0..500 {
            let chunk: Vec<f32> = (0..64)
                .map(|_| {
                    n += 1.0;
                    n
                })
                .collect();
            ring_append(&mut buf, &chunk, cap);
            assert!(
                buf.len() <= cap,
                "ring exceeded its cap: {} > {cap}",
                buf.len()
            );
        }
        // 500×64 = 32000 pushed, cap 1000 → the tail is the LAST 1000 (…, 31999, 32000).
        assert_eq!(buf.len(), cap);
        assert_eq!(
            buf.back().copied(),
            Some(32000.0),
            "keeps the newest sample"
        );
        assert_eq!(
            buf.front().copied(),
            Some(32000.0 - cap as f32 + 1.0),
            "front is exactly `cap` samples back — older history trimmed"
        );
    }

    #[test]
    fn ring_append_raw_i32_converts_and_stays_bounded_like_the_f32_path() {
        let cap = 4usize;
        let mut buf = std::collections::VecDeque::<f32>::new();
        let mut src = vec![1_073_741_824i32, -1_073_741_824, 0, 1_073_741_824, 0];
        let data = data_from(&mut src, SampleFormat::I32);
        ring_append_raw(&mut buf, &data, cap);
        assert_eq!(buf.len(), cap, "trimmed to cap exactly like ring_append");
        assert_eq!(
            buf.iter().copied().collect::<Vec<_>>(),
            vec![-0.5, 0.0, 0.5, 0.0],
            "dropped the oldest sample, converted the rest"
        );
    }

    #[test]
    fn parse_asound_usbid_reads_the_kernels_lowercase_colon_form() {
        // A real TMP audio-interface card (fw 1.8.58, HW-captured).
        assert_eq!(parse_asound_usbid("1ed8:0047\n"), Some((0x1ed8, 0x0047)));
        // No trailing newline, no leading zeros stripped either way.
        assert_eq!(parse_asound_usbid("07ca:313a"), Some((0x07ca, 0x313a)));
    }

    #[test]
    fn parse_asound_usbid_rejects_malformed_input() {
        for bad in ["", "1ed8", "1ed8:", ":0047", "zzzz:0047", "1ed8:0047:extra"] {
            assert_eq!(parse_asound_usbid(bad), None, "input {bad:?}");
        }
    }

    #[test]
    fn channels_rates_formats_merges_and_dedupes_across_config_ranges() {
        struct Fake {
            ch: u16,
            lo: u32,
            hi: u32,
            fmt: SampleFormat,
        }
        impl SupportedConfigLike for Fake {
            fn channels(&self) -> u16 {
                self.ch
            }
            fn sample_rate_range(&self) -> (u32, u32) {
                (self.lo, self.hi)
            }
            fn sample_format(&self) -> SampleFormat {
                self.fmt
            }
        }
        let configs = vec![
            Fake {
                ch: 4,
                lo: 44_100,
                hi: 48_000,
                fmt: SampleFormat::I32,
            },
            Fake {
                ch: 2,
                lo: 48_000,
                hi: 48_000,
                fmt: SampleFormat::F32,
            },
            // Same format as the first row — must not duplicate in the output.
            Fake {
                ch: 4,
                lo: 44_100,
                hi: 48_000,
                fmt: SampleFormat::I32,
            },
        ];
        let (ch, rates, formats) = channels_rates_formats(Some(configs.into_iter()));
        assert_eq!(ch, 4, "max channel count across all ranges");
        assert_eq!(rates, vec![44_100, 48_000]);
        assert_eq!(formats, vec![SampleFormat::I32, SampleFormat::F32]);
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
    fn processed_stereo_interleaves_the_pair_and_excludes_dry() {
        // Same 3-channel capture as `stereo_mix`'s test: ch0/ch1 must come through
        // UNMIXED (SUM convention — the LUFS meter, not the average `stereo_mix`
        // uses), and ch2 (dry send) must not leak in.
        let cap = Capture {
            interleaved: vec![1.0, 0.4, 9.0, 0.5, 0.2, 9.0],
            channels: 3,
            sample_rate: 48_000,
        };
        assert_eq!(cap.processed_stereo(), Some(vec![1.0, 0.4, 0.5, 0.2]));
    }

    #[test]
    fn processed_stereo_is_none_for_a_genuinely_mono_capture() {
        // T2/D-fallback: a true 1-channel capture must NOT be duplicated into fake
        // dual-mono (that would invent the +3.01 dB the hardware never produced) —
        // the caller falls back to `lufs::measure_mono` instead.
        let cap = Capture {
            interleaved: vec![0.25, -0.5, 0.75],
            channels: 1,
            sample_rate: 48_000,
        };
        assert_eq!(cap.processed_stereo(), None);
    }

    #[test]
    fn loudest_channel_never_picks_the_dry_send() {
        // ch2 (dry DI tap) is the loudest — a plugged-in guitar during a run.
        // The argmax must stay on the processed pair (ch0/ch1).
        let cap = Capture {
            interleaved: vec![1.0, 0.5, 9.0, 1.0, 0.5, 9.0],
            channels: 3,
            sample_rate: 48_000,
        };
        let (ch, _) = cap.loudest_channel();
        assert!(ch < 2, "argmax picked the dry send channel {ch}");
    }

    #[test]
    fn loudest_channel_picks_the_louder_processed_channel() {
        let cap = Capture {
            interleaved: vec![0.1, 0.8, 0.1, 0.8],
            channels: 2,
            sample_rate: 48_000,
        };
        assert_eq!(cap.loudest_channel().0, 1);
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

    // ── the "calibration can't see the guitar" bug class ─────────────────────
    // User-reported: Settings calibration read a fabricated-silent (or wrong-
    // device) lane as "no instrument signal" while a mic-carrying aggregate
    // named "Tone Master …" measured room sound instead of the dry DI.

    #[test]
    fn require_channel_errors_on_a_structurally_missing_dry_channel() {
        // channel()'s zero-pad must NOT reach the dry-DI reader: a capture
        // negotiated with too few channels fails loudly instead of reading
        // synthesized silence as "the player played nothing".
        let cap = Capture {
            interleaved: vec![0.1, 0.2, 0.1, 0.2],
            channels: 2,
            sample_rate: 48_000,
        };
        let err = cap.require_channel(DRY_INSTRUMENT_IN_CH).unwrap_err();
        assert!(err.contains("2 channel"), "unhelpful error: {err}");
        // A carried index still reads the strict way.
        assert_eq!(cap.require_channel(1).unwrap(), vec![0.2, 0.2]);
    }

    #[test]
    fn require_channel_returns_the_dry_lane_on_a_native_capture() {
        let cap = Capture {
            interleaved: vec![1.0, 2.0, 3.0, 4.0, 1.0, 2.0, 3.5, 4.0],
            channels: TMP_NATIVE_CHANNELS as usize,
            sample_rate: 48_000,
        };
        assert_eq!(
            cap.require_channel(DRY_INSTRUMENT_IN_CH).unwrap(),
            vec![3.0, 3.5]
        );
    }

    #[test]
    fn device_match_prefers_the_native_channel_count_over_an_aggregate() {
        // A "Tone Master Pro + mic" aggregate matches the name substring too
        // and CAN precede the physical unit in CoreAudio's unspecified
        // enumeration order — the native 4-ch unit must win regardless.
        assert_eq!(pick_match_index(&[6, 4]), Some(1));
        assert_eq!(pick_match_index(&[4, 6]), Some(0));
        // No native-count match: first match, so the channel guards downstream
        // fail loudly instead of a blanket "device not found".
        assert_eq!(pick_match_index(&[6, 2]), Some(0));
        assert_eq!(pick_match_index(&[]), None);
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
        let r = replay_measure(&full_clip, rate, 1, MeasureOpts::adaptive()).unwrap();
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
    fn replay_stereo_converges_to_the_stereo_oneshot_reading() {
        // The stereo counterpart of `replay_stationary_converges_early_and_matches_full`
        // — `replay_measure(..., 2, ...)` must converge on the SAME reading
        // `lufs::measure_stereo` gets one-shot (proof the frame-count hop scaling
        // survives 2-ch replay, not just the live capture loops).
        let rate = 48_000;
        let tone = sine(1000.0, 6.0, rate, 0.5);
        let interleaved: Vec<f32> = tone.iter().flat_map(|&s| [s, s]).collect();
        let full = crate::lufs::measure_stereo(&interleaved, rate)
            .unwrap()
            .integrated_lufs;
        let r = replay_measure(&interleaved, rate, 2, MeasureOpts::adaptive()).unwrap();
        assert!(
            r.converged,
            "stationary dual-mono tone should converge early"
        );
        assert!(
            (r.integrated_lufs - full).abs() < 0.2,
            "adaptive {:.3} vs stereo one-shot {:.3}",
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
        let r = replay_measure(&clip, rate, 1, MeasureOpts::full()).unwrap();
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
        let r = replay_measure(&clip, rate, 1, MeasureOpts::adaptive()).unwrap();
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
        let r = replay_measure(&silence, rate, 1, MeasureOpts::adaptive()).unwrap();
        assert!(
            !r.integrated_lufs.is_finite(),
            "silence has no finite loudness"
        );
        assert!(!r.converged);
    }

    #[test]
    fn replay_rejects_empty_and_short() {
        assert!(replay_measure(&[], 48_000, 1, MeasureOpts::default()).is_err());
        // shorter than the pre-roll → err
        let short = vec![0.1f32; 48_000 / 100]; // 10 ms
        assert!(replay_measure(&short, 48_000, 1, MeasureOpts::default()).is_err());
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
