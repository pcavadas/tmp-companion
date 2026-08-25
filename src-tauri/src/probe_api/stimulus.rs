//! Stimulus WAV I/O + calibration + probe measurement entry points.

use super::SCRATCH_SLOTS as SCRATCH;
use crate::audio;
use crate::backup_read::Usb3Strip;
use crate::doctor;
use crate::footswitch;
use crate::leveller;
use crate::lufs;
use crate::read_slot_preset_parsed;
use crate::session;
use crate::topologies;

/// Read a WAV file's raw interleaved samples as f32 in [-1, 1] (handles both
/// float32 and integer PCM), without downmixing. Returns (interleaved,
/// sample_rate, channels). Shared by [`read_wav_mono`] (which downmixes) and
/// [`measure_wav_auto`] (which needs the channel count to pick a measurement
/// convention).
fn read_wav_interleaved(path: &str) -> Result<(Vec<f32>, u32, usize), String> {
    let mut reader = hound::WavReader::open(path).map_err(|e| format!("open {path}: {e}"))?;
    let spec = reader.spec();
    let ch = spec.channels.max(1) as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap_or(0.0)).collect(),
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.unwrap_or(0) as f32 / max)
                .collect()
        }
    };
    Ok((interleaved, spec.sample_rate, ch))
}

/// Read a WAV file and downmix to mono f32 in [-1, 1] (fixed mono convention).
/// Returns (samples, sample_rate).
fn read_wav_mono(path: &str) -> Result<(Vec<f32>, u32), String> {
    let (interleaved, rate, ch) = read_wav_interleaved(path)?;
    let mono: Vec<f32> = interleaved
        .chunks(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect();
    Ok((mono, rate))
}

/// Read a WAV file, downmix to mono f32, and measure its loudness. Used by
/// `probe --lufs <wav>` to validate `lufs.rs` against an external oracle
/// (pyloudnorm / ffmpeg ebur128) without any device. Kept as the pre-PR2
/// mono-only entry point; [`measure_wav_auto`] is the stereo-aware successor
/// `probe --measure-wav` uses.
pub fn measure_wav_file(path: &str) -> Result<lufs::Loudness, String> {
    let (mono, rate) = read_wav_mono(path)?;
    lufs::measure_mono(&mono, rate)
}

/// Read a WAV file and measure it through the PRODUCTION output-side
/// convention (module header, `leveller::measure_processed`): a ≥2-channel
/// file measures its first two channels as standard 2-ch BS.1770
/// (`lufs::measure_stereo`) — extra channels beyond the pair are dropped
/// rather than mixed in, matching `Capture::processed_stereo`'s dry-channel
/// exclusion — and a genuinely 1-channel file measures via `lufs::measure_mono`.
/// Device-free (reads a file, never opens HID or touches re-amp): the
/// Companion side of `probe --measure-wav` / `scripts/meter-parity.sh`.
pub fn measure_wav_auto(path: &str) -> Result<(lufs::Loudness, usize), String> {
    let (interleaved, rate, ch) = read_wav_interleaved(path)?;
    if ch >= 2 {
        let pair: Vec<f32> = interleaved
            .chunks(ch)
            .flat_map(|f| {
                [
                    f.first().copied().unwrap_or(0.0),
                    f.get(1).copied().unwrap_or(0.0),
                ]
            })
            .collect();
        Ok((lufs::measure_stereo(&pair, rate)?, ch))
    } else {
        Ok((lufs::measure_mono(&interleaved, rate)?, ch))
    }
}

/// Write a mono f32 buffer to a WAV (the offline reference-clip corpus format +
/// the Tier-2 calibration capture store via `profiles::store_capture`).
pub(crate) fn write_wav_mono(path: &str, samples: &[f32], sample_rate: u32) -> Result<(), String> {
    write_wav(path, samples, sample_rate, 1)
}

/// Write an INTERLEAVED 2-channel f32 buffer to a WAV — the processed-pair dump
/// format `probe --dump-wav` writes (see [`dump_processed_capture`]).
pub(crate) fn write_wav_stereo(
    path: &str,
    interleaved: &[f32],
    sample_rate: u32,
) -> Result<(), String> {
    write_wav(path, interleaved, sample_rate, 2)
}

fn write_wav(
    path: &str,
    interleaved: &[f32],
    sample_rate: u32,
    channels: u16,
) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| format!("create {path}: {e}"))?;
    for &s in interleaved {
        w.write_sample(s)
            .map_err(|e| format!("write {path}: {e}"))?;
    }
    w.finalize().map_err(|e| format!("finalize {path}: {e}"))
}

/// Write a re-amp `Capture`'s PROCESSED PAIR (never the dry channel) to a
/// 32-bit-float WAV in `dir`, named from `label` (the caller builds it from
/// slot/scene so a directory of dumps stays self-describing) — the
/// `probe --dump-wav <dir>` add-on. A genuinely 1-channel capture falls back to
/// a mono file (mirrors `Capture::processed_stereo`'s own fallback: duplicating
/// it into fake dual-mono would invent the +3.01 dB the hardware never
/// produced). Pure add-on: only called when `--dump-wav` is passed, so a probe
/// invocation without it is byte-for-byte unaffected. Creates `dir` if absent.
///
/// `label` is NOT made unique here — a repeated `--measure-current`/`--measure-scene`
/// call with the same slot/scene (the same caller-built label) OVERWRITES the prior
/// dump. A caller wanting to keep every take (e.g. a same-day A/B) must vary `label`
/// itself (a timestamp suffix).
pub(crate) fn dump_processed_capture(
    cap: &audio::Capture,
    dir: &str,
    label: &str,
) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create dir {dir}: {e}"))?;
    let path = format!("{}/{label}.wav", dir.trim_end_matches('/'));
    match cap.processed_stereo() {
        Some(stereo) => write_wav_stereo(&path, &stereo, cap.sample_rate)?,
        None => write_wav_mono(&path, &cap.channel(0), cap.sample_rate)?,
    }
    Ok(path)
}

/// OFFLINE HARNESS (1/3): capture ONE full re-amp clip of `slot` at the leveling
/// reference level (0.5) via the topology stimulus and write it to `out` (mono,
/// 48 kHz f32) — building a corpus of real processed clips so the adaptive-capture
/// constants can be tuned with no device. DEVICE OP (load + engage + full capture).
pub fn probe_capture_reference(slot: u32, topology_id: &str, out: &str) -> Result<String, String> {
    let stim = read_stimulus_48k(&probe_stimulus_path(topology_id)?)?;
    let (mono, rate) = leveller::capture_samples(slot, &stim, 0.5)?;
    write_wav_mono(out, &mono, rate)?;
    Ok(format!(
        "captured slot {slot} ({} samples = {:.2}s @ {rate} Hz) → {out}\n",
        mono.len(),
        mono.len() as f32 / rate as f32
    ))
}

/// HW PROBE (WRITE): capture a short stimulus burst plus a long silent `tail_ms`
/// afterward, so the raw WAV shows a reverb/delay tail's own decay curve directly
/// — rather than trying to infer contamination risk from the ~1.3 s inter-connection
/// floor alone (see device-manual-gaps.md gap 6). `scene`, when given, is the
/// 0-based `scenes[]` wire index to recall after the load (same convention as
/// `--measure-scene`) — needed to reach a preset's wet SCENE rather than just its
/// base/default state.
///
/// Scratch slots need no override. A non-scratch preset requires
/// `TMP_ALLOW_NONSCRATCH_TAIL_DECAY=1`: this path writes `PresetLevel` on the
/// target's WORKING COPY (via `doctor_capture`'s `ref_level`) — unsaved, discarded
/// on the next reload, but real, so testing a real device-created preset should be
/// a deliberate opt-in, same as `--reamp-wav`'s `TMP_ALLOW_NONSCRATCH_REAMP`.
pub fn probe_tail_decay(
    slot: u32,
    scene: Option<u32>,
    stimulus_wav: &str,
    tail_ms: u64,
    out: &str,
) -> Result<String, String> {
    if !SCRATCH.contains(&slot) && std::env::var("TMP_ALLOW_NONSCRATCH_TAIL_DECAY").is_err() {
        return Err(format!(
            "refusing --tail-decay on list index {slot}: outside the scratch zone {SCRATCH:?}, \
             and this path WRITES (PresetLevel) to the working copy. \
             Re-run with TMP_ALLOW_NONSCRATCH_TAIL_DECAY=1 if measuring this preset is intended."
        ));
    }
    let stim = read_stimulus_48k(stimulus_wav)?;
    let (samples, rate) =
        leveller::doctor_capture(slot, scene, &[], &[], &stim, Some(0.5), tail_ms, false)?;
    write_wav_mono(out, &samples, rate)?;
    // The stimulus is 48 kHz by construction (`read_stimulus_48k`); `rate` is the
    // CAPTURE rate `doctor_capture` returns and need not match it — using `rate`
    // here would misreport the stimulus duration by their ratio if they differ.
    let stim_ms = stim.len() as u64 * 1000 / 48_000;
    Ok(format!(
        "captured slot {slot} scene {scene:?}: {stim_ms}ms stimulus + {tail_ms}ms tail = {} samples @ {rate} Hz → {out}\n",
        samples.len()
    ))
}

/// HW PROBE: re-amp an **arbitrary** stimulus WAV through `slot` and save the
/// captured USB 1/2 audio. Sibling of [`probe_capture_reference`], which resolves
/// its stimulus from the bundled per-topology WAV — this one takes any 48 kHz mono
/// file, which is what a swept-sine bandwidth test needs.
///
/// Read UNNORMALIZED (`read_stimulus_48k`, no LUFS scaling): a chirp's flat drive
/// per octave is the whole point, and calibration scaling would tilt it.
///
/// Band-edge caveat: run it through a CLEAN, effects-free preset. A distorting amp
/// model generates harmonics that fill the octave above the fundamental sweep and
/// muddy exactly the band edge the test reads.
pub fn probe_reamp_wav(
    slot: u32,
    stimulus_wav: &str,
    out: &str,
    ref_level: f32,
    bypass_all: bool,
    bypass_nodes: &str,
) -> Result<String, String> {
    // Reject rather than silently clamp: `capture_full_at` clamps ref_level to
    // 0.05..=1.0 downstream, but the reported string below prints the ARGUMENT,
    // not the clamped value — so an out-of-range or non-finite level would
    // measure at one value while claiming another.
    if !ref_level.is_finite() || !(0.05..=1.0).contains(&ref_level) {
        return Err(format!(
            "ref_level {ref_level} must be a finite value in 0.05..=1.0"
        ));
    }
    // THIS IS A WRITE PATH, despite reading like a measurement: `capture_full_at`
    // sets `bypass` flags and `PresetLevel` on the target preset's working copy
    // (`leveller.rs`). Nothing is saved — a reload discards it — but the write is
    // real, so which preset it lands on should be a deliberate choice.
    //
    // Measuring a non-scratch preset is sometimes necessary (the unit may hold only
    // one preset of the template under test), so this is an explicit opt-in rather
    // than a refusal.
    if !SCRATCH.contains(&slot) && std::env::var("TMP_ALLOW_NONSCRATCH_REAMP").is_err() {
        return Err(format!(
            "refusing --reamp-wav on list index {slot}: outside the scratch zone {SCRATCH:?}, \
             and this path WRITES (bypass flags + PresetLevel) to the working copy. \
             Re-run with TMP_ALLOW_NONSCRATCH_REAMP=1 if measuring this preset is intended."
        ));
    }
    let stim = read_stimulus_48k(stimulus_wav)?;

    // Explicit `GROUP/NODE,GROUP/NODE` list — isolates ONE lane of a Split or
    // Parallel template so a capture can be attributed to a specific lane, which
    // is what "what actually reaches USB 1/2 on a Split?" needs.
    if !bypass_nodes.is_empty() {
        // Reject a token missing GROUP/ rather than silently defaulting to G1 —
        // a typo'd lane selector would otherwise bypass the wrong group and the
        // resulting capture would look like a valid but wrong measurement.
        let forced: Vec<(String, String, bool)> = bypass_nodes
            .split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| {
                let (g, n) = t
                    .split_once('/')
                    .ok_or_else(|| format!("bypass-nodes token {t:?} is not GROUP/NODE"))?;
                if g.is_empty() || n.is_empty() {
                    return Err(format!("bypass-nodes token {t:?} is not GROUP/NODE"));
                }
                Ok((g.to_string(), n.to_string(), true))
            })
            .collect::<Result<Vec<_>, String>>()?;
        // bypass_nodes was non-empty text but every token was blank after
        // trim/filter (e.g. "," or " , "): reject rather than silently falling
        // through to an unbypassed capture, which would look like a valid
        // all-lanes measurement instead of a malformed argument.
        if forced.is_empty() {
            return Err(format!(
                "bypass-nodes={bypass_nodes:?} produced no GROUP/NODE tokens after parsing"
            ));
        }
        let (mono, rate) = leveller::capture_samples_bypassed(slot, &stim, ref_level, &forced)?;
        write_wav_mono(out, &mono, rate)?;
        let loud = lufs::measure_mono(&mono, rate)
            .map(|l| l.integrated_lufs)
            .unwrap_or(f64::NAN);
        return Ok(format!(
            "re-amped {stimulus_wav} through slot {slot} @ level {ref_level} [bypassed {:?}] \
             → {out}\n  {} samples = {:.2}s @ {rate} Hz, integrated {loud:.3} LUFS\n",
            forced
                .iter()
                .map(|(g, n, _)| format!("{g}/{n}"))
                .collect::<Vec<_>>(),
            mono.len(),
            mono.len() as f32 / rate as f32
        ));
    }

    // `bypass_all` turns the preset into approximately a wire, so the capture
    // reads the PATH's bandwidth instead of the amp model's HF rolloff. The node
    // list comes from `load_then_discover_blocks` — the SAME level-control list
    // used elsewhere in this crate — not a full live-graph walk; see the caveat
    // below for what that means for the "approximately a wire" claim.
    let mut forced: Vec<(String, String, bool)> = Vec::new();
    if bypass_all {
        // Node discovery goes through `load_then_discover_blocks` — the
        // 1.8.45-safe rich-lean-session path. A plain `connect()` handshake does
        // NOT reliably carry `currentPresetDataChanged` on this firmware
        // ("no preset JSON in handshake", HW), so reading the graph directly off
        // a handshake fails.
        //
        // Caveat, reported in the output so it can't be silently over-claimed:
        // this enumerates blocks that expose a LEVEL-type control. A block with
        // no level parameter is not in the list and stays active.
        let blocks = crate::load_then_discover_blocks(slot)?;
        for b in &blocks {
            let pair = (b.group_id.clone(), b.node_id.clone(), true);
            if !forced.contains(&pair) {
                forced.push(pair);
            }
        }
        // Discovery returning nothing is NOT "nothing to bypass" — it means the
        // level-control enumeration failed or the preset exposes none. Falling through
        // would run `capture_samples` and label an UN-bypassed capture as bypassed,
        // exactly the silent-wrong-measurement the `bypass-nodes` empty-list check
        // rejects. Refuse instead.
        if forced.is_empty() {
            return Err(format!(
                "bypass: no level-control blocks discovered on slot {slot}, so nothing would be \
                 bypassed — refusing rather than capturing an unbypassed take labelled bypassed"
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
    }

    let (mono, rate) = if forced.is_empty() {
        leveller::capture_samples(slot, &stim, ref_level)?
    } else {
        leveller::capture_samples_bypassed(slot, &stim, ref_level, &forced)?
    };
    write_wav_mono(out, &mono, rate)?;
    let loud = lufs::measure_mono(&mono, rate)
        .map(|l| l.integrated_lufs)
        .unwrap_or(f64::NAN);
    Ok(format!(
        "re-amped {stimulus_wav} through slot {slot} @ level {ref_level}{} → {out}\n  \
         {} samples = {:.2}s @ {rate} Hz, integrated {loud:.3} LUFS\n",
        if forced.is_empty() {
            String::new()
        } else {
            format!(
                " [force-bypassed {}: {:?}]",
                forced.len(),
                forced
                    .iter()
                    .map(|(g, n, _)| format!("{g}/{n}"))
                    .collect::<Vec<_>>()
            )
        },
        mono.len(),
        mono.len() as f32 / rate as f32
    ))
}

/// OFFLINE HARNESS (2/3): recompute integrated LUFS over increasing prefixes of a
/// reference clip vs the full read, to anchor `min_measure_ms`/`max_capture_ms`. No
/// device — pure analysis on a clip captured by `--capture-reference`.
pub fn probe_measure_prefix_sweep(wav: &str) -> Result<String, String> {
    let (mono, rate) = read_wav_mono(wav)?;
    let full = lufs::measure_mono(&mono, rate)?.integrated_lufs;
    let total_s = mono.len() as f32 / rate as f32;
    let mut out = format!(
        "prefix sweep {wav}  ({total_s:.2}s @ {rate} Hz)  full integrated = {full:.3} LUFS\n  \
         prefix_s   integrated   Δ vs full\n"
    );
    for &secs in &[0.5f32, 1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0] {
        let n = ((secs * rate as f32) as usize).min(mono.len());
        if n == 0 {
            continue;
        }
        let v = lufs::measure_mono(&mono[..n], rate)
            .map(|l| l.integrated_lufs)
            .unwrap_or(f64::NAN);
        out += &format!("  {secs:>6.1}    {v:>9.3}    {:>+8.3}\n", v - full);
        if secs >= total_s {
            break;
        }
    }
    Ok(out)
}

/// OFFLINE HARNESS (3/3): replay a reference clip through the SAME convergence state
/// machine `reamp_measure` uses with the given (eps, k, preroll), reporting the exit
/// time and the error vs the full-clip read. Use to confirm a candidate tuning stays
/// within ±0.07 LU on every corpus clip. No device.
pub fn probe_measure_converge_replay(
    wav: &str,
    eps_lu: f64,
    stable_k: u32,
    preroll_ms: u64,
) -> Result<String, String> {
    let (mono, rate) = read_wav_mono(wav)?;
    let full = lufs::measure_mono(&mono, rate)?.integrated_lufs;
    // Exercise the adaptive early-exit path (the harness exists to tune it).
    let opts = audio::MeasureOpts {
        eps_lu,
        stable_k,
        preroll_ms,
        ..audio::MeasureOpts::adaptive()
    };
    let r = audio::replay_measure(&mono, rate, 1, opts)?;
    Ok(format!(
        "replay {wav}\n  opts: preroll={}ms hop={}ms eps={} k={} min={}ms max={}ms\n  \
         exit={}ms converged={}  integrated={:.3} LUFS  full={full:.3}  Δ={:+.3} LU\n",
        opts.preroll_ms,
        opts.hop_ms,
        opts.eps_lu,
        opts.stable_k,
        opts.min_measure_ms,
        opts.max_capture_ms,
        r.exit_ms,
        r.converged,
        r.integrated_lufs,
        r.integrated_lufs - full
    ))
}

/// HW A/B for the RE-BASELINE decision: on one preset, measure the captured LUFS
/// the FULL way (production metric: settle → full stimulus + 0.8 s tail → integrate)
/// and the ADAPTIVE way (`reamp_measure` early-exit), on two fresh connections, and
/// report both values, their delta, and both wall-clock times. This is the live
/// counterpart to the offline `--measure-converge-replay`; it keeps the adaptive
/// `reamp_measure` reachable and lets the operator judge the speed/accuracy trade on real
/// presets before any decision to wire adaptive into the leveling path. Read-only
/// (no save); ends with a guaranteed re-amp OFF.
pub fn probe_measure_adaptive(slot: u32, topology_id: &str) -> Result<String, String> {
    let stim = read_stimulus_48k(&probe_stimulus_path(topology_id)?)?;
    // Load once in its own connection (the set-after-load override gotcha).
    {
        let mut s = session::Session::connect()?;
        s.load_preset(slot)?;
        std::thread::sleep(std::time::Duration::from_millis(1200));
    }

    let measure_once = |adaptive: bool| -> Result<(f64, u128), String> {
        std::thread::sleep(std::time::Duration::from_millis(400));
        let t = std::time::Instant::now();
        let mut s = session::Session::connect()?;
        s.set_preset_level(0.5)?;
        std::thread::sleep(std::time::Duration::from_millis(300));
        let _ = s.set_reamp_mode(true)?;
        let lufs = if adaptive {
            audio::reamp_measure(&stim, 48_000, audio::MeasureOpts::adaptive())
        } else {
            std::thread::sleep(std::time::Duration::from_millis(500));
            leveller::processed_lufs(audio::reamp_capture(&stim, 48_000, 800))
        };
        let _ = s.set_reamp_mode(false);
        Ok((lufs?, t.elapsed().as_millis()))
    };

    let (full_lufs, full_ms) = measure_once(false)?;
    let (adapt_lufs, adapt_ms) = measure_once(true)?;
    // Guaranteed re-amp OFF on a fresh connection.
    let _ = session::Session::connect().and_then(|mut s| s.set_reamp_mode(false).map(|_| ()));

    Ok(format!(
        "preset {slot} @ presetLevel 0.5 ({topology_id})\n  \
         FULL     {full_lufs:>8.3} LUFS   {full_ms:>5} ms\n  \
         ADAPTIVE {adapt_lufs:>8.3} LUFS   {adapt_ms:>5} ms\n  \
         Δ(adaptive−full) = {:+.3} LU   time saved = {} ms\n",
        adapt_lufs - full_lufs,
        full_ms.saturating_sub(adapt_ms)
    ))
}

/// Doctor calibration sweep (`probe --doctor`): for each `(list index, scene)`
/// entry (scene `None` = the BASE sound; `Some(wire index)` = one scene, the
/// probe CSV's `slot:scene` form), capture the sound with the Doctor tail
/// (`leveller::doctor_capture`), compute its band profile + time-domain metrics,
/// then diagnose each sound on its own measurements (the deterministic
/// target-deviation metric) and print one JSON line per sound plus a human table —
/// the headless iteration loop for tuning `doctor::Thresholds`. Read-only: loads +
/// captures, NEVER saves; every capture path ends re-amp OFF.
pub fn probe_doctor(slots: &[(u32, Option<u32>)], topology_id: &str) -> Result<String, String> {
    // Mirrors doctor_check's capture space: the production 3 s Doctor window.
    let stim = leveller::doctor_stim_slice(read_stimulus_48k(&probe_stimulus_path(topology_id)?)?);
    let instrument = doctor::Family::from_topology(
        topologies::by_id(topology_id)
            .map(|t| t.instrument)
            .unwrap_or("guitar"),
    );

    type Sound = (
        u32,
        Option<u32>,
        doctor::SoundProfile,
        Option<Vec<doctor::DoctorNode>>,
        Vec<bool>,
    );
    // Fresh-connection re-amp OFF on every exit path — this sweep previously
    // had NO belt-and-braces cleanup at all (a mid-sweep failure left the
    // device's re-amp state wherever the error found it).
    let _reamp_off = super::ReampOffGuard;
    let mut sounds: Vec<Sound> = Vec::new();
    for &(slot, scene) in slots {
        // One field-8 slot read (quiet line, NO LoadPreset) drives BOTH the graph
        // facts and the base-sound force-bypass isolation (every on/off block off).
        // Truncated JSON still yields the guitarNodes prefix + ftsw; on read error
        // we degrade to no graph facts + no isolation. A SCENE sound gets NO
        // isolation writes (mirrors `doctor_check`: its own bypass overrides
        // define it).
        let mut nodes: Option<Vec<doctor::DoctorNode>> = None;
        let mut fb: Vec<(String, String, bool)> = Vec::new();
        match read_slot_preset_parsed(slot) {
            Ok((preset, _, _)) => {
                nodes = Some(
                    session::extract_active_graph(&preset, None)
                        .nodes
                        .iter()
                        .map(doctor::DoctorNode::from_graph_node)
                        .collect(),
                );
                if scene.is_none() {
                    fb = footswitch::all_onoff_blocks(
                        preset.get("ftsw").unwrap_or(&serde_json::Value::Null),
                    )
                    .into_iter()
                    .map(|(g, n)| (g, n, true))
                    .collect();
                }
            }
            Err(e) => eprintln!(
                "[probe] slot {slot}: preset read failed ({e}) — no graph facts, no isolation"
            ),
        }
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
        // Calibration sweep: always the full tail (this is the reference recipe
        // R4/R5 re-baseline against, not the app's per-sound dry-tail shortcut).
        match leveller::doctor_capture(
            slot,
            scene,
            &fb,
            &[],
            &stim,
            Some(0.5),
            u64::from(leveller::DOCTOR_TAIL_MS),
            false,
        ) {
            Ok((samples, rate)) => {
                let (onset, confident) = audio::estimate_onset(&stim, &samples, rate);
                if !confident {
                    eprintln!(
                        "[probe] slot {slot}: onset not confidently found — un-aligned split"
                    );
                }
                // Share ONE body PSD between the profile and the coverage gate
                // (mirrors `analyze_capture` / production `doctor_check`) so this
                // calibration sweep's printed verdicts match what the app would
                // actually fire, instead of the gate-free `diagnose` shim.
                let psd_onset = leveller::doctor_signal_start(onset, confident);
                let body_psd = doctor::body_psd(&samples, rate, psd_onset);
                let stim_psd = crate::psd::welch_psd(&stim, rate as f32);
                // Skip-and-continue like the capture-failure arm below — a `?`
                // here (e.g. one silent-inject slot) would discard the whole
                // sweep, including every slot already captured.
                match doctor::SoundProfile::from_capture_with_psd(
                    &samples,
                    rate,
                    stim.len(),
                    onset,
                    instrument,
                    &body_psd,
                    Some(&stim_psd),
                ) {
                    Ok(profile) => {
                        let coverage = doctor::output_coverage_with_body(
                            &samples, rate, psd_onset, instrument, &body_psd,
                        );
                        sounds.push((slot, scene, profile, nodes, coverage));
                    }
                    Err(e) => eprintln!("[probe] slot {slot}: profile failed: {e} (skipping)"),
                }
            }
            Err(e) => eprintln!("[probe] slot {slot}: capture failed: {e} (skipping)"),
        }
    }
    if sounds.is_empty() {
        return Err("no sound captured".to_string());
    }

    let mut out = format!(
        "doctor sweep ({topology_id}, {} sounds, target-deviation)\n  slot |     LUFS |  tail dB | balance dB ({}) | diagnoses\n",
        sounds.len(),
        instrument.labels().join(" ")
    );
    for (slot, scene, profile, nodes, coverage) in &sounds {
        let diags = doctor::diagnose_kind(
            profile,
            nodes.as_deref(),
            instrument,
            doctor::StimulusKind::Synthetic,
            Some(coverage),
            doctor::PlaybackOffsets::NONE,
        );
        let bal = doctor::balance(&profile.bands);
        let label = match scene {
            Some(s) => format!("{slot}:{s}"),
            None => slot.to_string(),
        };
        out += &format!(
            "  {label:>4} | {:>8.2} | {:>8.1} | {} | {}\n",
            profile.integrated_lufs,
            profile.tail_ratio_db,
            bal.iter()
                .map(|b| format!("{b:>+5.1}"))
                .collect::<Vec<_>>()
                .join(" "),
            if diags.is_empty() {
                "—".to_string()
            } else {
                diags.iter().map(|d| d.key).collect::<Vec<_>>().join(", ")
            }
        );
        // Machine-readable line per sound (jq-able for calibration notes).
        let json = serde_json::json!({
            "slot": slot,
            "scene": scene,
            "profile": profile,
            "balanceDb": bal.clone(),
            "diagnoses": diags,
        });
        println!("{json}");
    }
    // Belt-and-braces: leave the unit re-amp OFF even if a capture errored out.
    let _ = session::Session::connect().and_then(|mut s| s.set_reamp_mode(false).map(|_| ()));
    Ok(out)
}

/// HW A/B of TWO stimuli per preset: for each slot, `measure_c` with stimulus A then
/// B and report the solved ceilings + dynamics spreads and ΔC. Quantifies how
/// sensitive each preset's leveling is to the stimulus character (e.g. the shipped
/// plucked noise vs a real chord DI captured with `--capture-input`) — data for the
/// playing-style question before any product change. Read-only (no save, no level
/// write persists); ends with a guaranteed re-amp OFF. Per-slot errors print in-row
/// and the sweep continues.
pub fn probe_stim_ab(
    slots: &[u32],
    wav_a: &str,
    wav_b: &str,
    ref_level: f32,
) -> Result<String, String> {
    let stim_a = read_stimulus_48k(wav_a)?;
    let stim_b = read_stimulus_48k(wav_b)?;
    // Floor reads are handled INSIDE measure_c now (the production floor guard:
    // stimulus-aware spread trip → same-ref retry → level-shift confirm) — a
    // persistent floor read surfaces as leveller::FLOOR_READ_ERR in the row.
    let mut out = format!(
        "stimulus A/B @ ref {ref_level:.2}\n  A = {wav_a}\n  B = {wav_b}\n\
         \n  slot |      C_A |      C_B |      ΔC | spread_A | spread_B\n"
    );
    for &slot in slots {
        // measure_c owns its own connection/gap pacing (the level_setlist precedent).
        let row = leveller::measure_c(slot, &stim_a, ref_level, &[])
            .and_then(|a| leveller::measure_c(slot, &stim_b, ref_level, &[]).map(|b| (a, b)));
        match row {
            Ok((a, b)) => {
                out += &format!(
                    "  {slot:>4} | {:>8.3} | {:>8.3} | {:>+7.3} | {:>8.2} | {:>8.2}\n",
                    a.c,
                    b.c,
                    b.c - a.c,
                    a.dynamic_spread_lu,
                    b.dynamic_spread_lu,
                );
            }
            Err(e) => out += &format!("  {slot:>4} | FAILED: {e}\n"),
        }
    }
    // Guaranteed re-amp OFF on a fresh connection — propagate a cleanup failure so a
    // "successful" A/B can't silently leave the device stuck in re-amp mode.
    session::Session::connect().and_then(|mut s| s.set_reamp_mode(false).map(|_| ()))?;
    Ok(out)
}

/// Read a stimulus WAV as mono f32, requiring the device's 48 kHz clock rate.
pub(crate) fn read_stimulus_48k(path: &str) -> Result<Vec<f32>, String> {
    let (stim, srate) = read_wav_mono(path)?;
    if srate != 48_000 {
        return Err(format!("stimulus must be 48 kHz (got {srate})"));
    }
    Ok(stim)
}

/// Read a 48 kHz stimulus and, if the instrument profile is Tier-2 calibrated,
/// scale it so its **K-weighted loudness (LUFS)** matches the measured real output
/// so the amp is driven as the real guitar drives it (re-amp inject is not AGC'd
/// — verified on device). K-weighted (not flat RMS): the perceptual weighting that
/// tracks how hard a pickup actually drives the amp — bright pickups aren't
/// under-counted — and it's the same scale the leveler targets on the output.
/// Caps the gain so the scaled peak stays ≤ 0.99 (no digital clip); when that cap
/// engages the calibrated loudness is UNREACHABLE and every measurement under-drives
/// the amp — the second return element is how many LU short the stimulus falls
/// (`None` when the target is reachable). Surfaced to the user at calibrate time
/// (`calibrate_profile`); the `log::warn!` covers every leveling caller.
pub(crate) fn read_stimulus_calibrated_with_shortfall(
    path: &str,
    calibration_lufs: Option<f32>,
) -> Result<(Vec<f32>, Option<f32>), String> {
    let mut stim = read_stimulus_48k(path)?;
    let mut shortfall_lu = None;

    // The gain to apply before the shared peak cap below: solved from the
    // calibration target when the stimulus's own loudness is finite (a
    // non-finite reading — e.g. a silent stimulus — means NO scaling at all,
    // matching the old calibrated branch's short-circuit); `1.0` (a no-op
    // multiply) when uncalibrated, so the cap-to-0.99 logic is the ONE shared
    // path for both branches — the stimulus passes VERBATIM into the re-amp
    // inject otherwise, which has NO limiter, so an explicit/env-var WAV (or,
    // in principle, a stored Tier-2 capture, though `calibrate_profile`
    // already clip-gates those at capture time) at/above full scale would
    // clip the inject and corrupt the whole downstream measurement.
    let mut gain: Option<f32> = Some(1.0);
    if let Some(target_lufs) = calibration_lufs {
        let stim_lufs = lufs::measure_mono(&stim, 48_000)?.integrated_lufs;
        gain = stim_lufs
            .is_finite()
            .then(|| 10f32.powf((target_lufs - stim_lufs as f32) / 20.0));
    }

    if let Some(mut g) = gain {
        let peak = stim.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        if peak * g > 0.99 {
            match calibration_lufs {
                Some(target_lufs) => {
                    let shortfall = 20.0 * (peak * g / 0.99).log10();
                    log::warn!(
                        "stimulus calibration capped: {path} cannot reach {target_lufs:.1} LUFS \
                         without clipping — driving {shortfall:.1} LU softer"
                    );
                    shortfall_lu = Some(shortfall);
                }
                None => {
                    log::warn!(
                        "stimulus inject peak {peak:.3} would clip the re-amp path — scaling to 0.99: {path}"
                    );
                }
            }
            g = 0.99 / peak; // guard against clipping the injected signal
        }
        for s in &mut stim {
            *s *= g;
        }
    }
    Ok((stim, shortfall_lu))
}

/// [`read_stimulus_calibrated_with_shortfall`] for the callers that only need the
/// samples (all leveling/probe paths — the warn above still fires for them).
pub(crate) fn read_stimulus_calibrated(
    path: &str,
    calibration_lufs: Option<f32>,
) -> Result<Vec<f32>, String> {
    read_stimulus_calibrated_with_shortfall(path, calibration_lufs).map(|(stim, _)| stim)
}

pub(crate) fn probe_stimulus_path(topology_id: &str) -> Result<String, String> {
    // An alias id is not a WAV stem — resolve to the parent topology's id first.
    let topology_id = crate::topologies::canonical_id(topology_id);
    let cwd = std::env::current_dir().map_err(|e| format!("current dir: {e}"))?;
    let candidates = [
        cwd.join("resources")
            .join("samples")
            .join(format!("{topology_id}.wav")),
        cwd.join("apps")
            .join("tmp-companion")
            .join("src-tauri")
            .join("resources")
            .join("samples")
            .join(format!("{topology_id}.wav")),
    ];
    candidates
        .iter()
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().to_string())
        .ok_or_else(|| format!("no bundled stimulus found for topology {topology_id:?}"))
}

/// Capture the dry instrument (USB-Out 3) for `secs` (normal mode forced, re-amp
/// OFF) and return `(mono samples, peak)`. The ONE dry-DI capture recipe — shared
/// by `calibrate_profile` (Tier-2) and `probe --capture-wav` so their peak/silence
/// guards can't drift apart.
/// `strip` is the device mixer's USB 3 state from the startup backup snapshot, and is
/// consulted ONLY to explain a silent take (see [`silent_dry_diagnosis`]) — never to
/// refuse a capture that landed. `None` when unknown (no snapshot, or a caller that
/// has none, like `probe --capture-wav`).
pub(crate) fn capture_dry_di(
    secs: f32,
    strip: Option<&Usb3Strip>,
) -> Result<(Vec<f32>, f32), String> {
    // Force normal mode (re-amp OFF) so the front instrument input flows to
    // USB-Out 3 — VERIFIED, not best-effort. In re-amp mode both input jacks are
    // muted and USB-Out 3/4 are disabled, so an OFF that never reached the unit
    // reads as "no instrument signal" with the real cause invisible (a real run:
    // the monitor's pause-ack came late, this connect raced the still-seized
    // device, and the swallowed failure left a silent take). A failed connect is
    // reported, NOT retried — hammering re-opens resets the HID lockout
    // (`danger.md`), so the message must not invite a fast retry.
    //
    // `connect_lean` (not `connect`): the OFF is a single setter needing no handshake
    // payload, and the lean shape is the narrowest window onto a device this call is
    // already racing — the same shape `leveller::reamp_off_guaranteed` sends every
    // run-end OFF through. Failing loud is safe here: `set_reamp_mode` errors only on
    // transport failure (the flaky `ReAmpModeChanged` echo comes back as
    // `Option<bool>`, never `Err`), so a green send is a real send.
    {
        let mut s = session::Session::connect_lean().map_err(|e| {
            format!(
                "could not reach the device to switch re-amp OFF before the capture \
                 ({e}) — close Pro Control if it is running; otherwise the device is \
                 in its post-session open lockout, which every retry RESTARTS: wait \
                 ~30 s without clicking Calibrate, or unplug and replug the unit \
                 (a replug also refreshes the mixer snapshot)"
            )
        })?;
        s.set_reamp_mode(false)
            .map_err(|e| format!("re-amp OFF was not accepted by the device ({e})"))?;
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    let cap = audio::capture_input(secs.clamp(2.0, 30.0), 48_000)?;
    let mono = cap.require_channel(audio::DRY_INSTRUMENT_IN_CH)?;
    let lanes: Vec<DryLane> = (0..cap.channels)
        .map(|c| DryLane::of(&cap.channel(c), 48_000))
        .collect();
    if let Some(fault) = dry_take_fault(&lanes, strip) {
        return Err(fault);
    }
    let peak = lanes
        .get(audio::DRY_INSTRUMENT_IN_CH)
        .map_or(0.0, |l| l.peak);
    Ok((mono, peak))
}

/// One capture lane as the dry-take guards see it: its peak — for the dBFS
/// readout only — and whether it carries MEASURABLE audio.
///
/// `measurable` is the load-bearing field, and it is deliberately NOT an amplitude
/// threshold. A tuned peak floor cannot separate "no signal" from "quiet signal"
/// across units: the dev unit (fw 1.8.45) idles at −74.6 dBFS on the dry lane and
/// −71.5 on the processed pair, BOTH above the −80 dBFS floor this replaced — so a
/// not-playing take there cleared the gate entirely and surfaced as the terse
/// non-finite-LUFS error, never reaching [`silent_dry_diagnosis`]. Moving that same
/// peak test later would have been worse than leaving it: the processed pair's own
/// floor clears the threshold too, so the lane hint would have blamed a MUTED USB 3
/// on a perfectly clean unit. Integrated loudness settles both with no tunable at
/// all: BS.1770's −70 LUFS ABSOLUTE gate makes any floor below it non-finite, and
/// both measured floors sit well under it (a floor loud enough to integrate ABOVE
/// −70 LUFS is signal to any silence gate; the downstream active-window and
/// spread checks own that case). That is why no constant is retuned here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DryLane {
    /// Peak absolute amplitude (linear, 0..1). Display only.
    pub peak: f32,
    /// `true` when the lane has a FINITE integrated loudness.
    pub measurable: bool,
}

impl DryLane {
    fn of(samples: &[f32], sample_rate: u32) -> Self {
        Self {
            peak: samples.iter().fold(0.0f32, |m, &s| m.max(s.abs())),
            // A meter that refuses the buffer outright (too short to gate) is
            // "not measurable" in exactly the sense this guard means.
            measurable: lufs::measure_mono(samples, sample_rate)
                .is_ok_and(|l| l.integrated_lufs.is_finite()),
        }
    }
}

/// The ONE verdict on a dry-DI take: `Some(diagnosis)` when USB-Out 3 carried
/// nothing measurable. Pure, so the whole guard is unit-testable without a device
/// — [`capture_dry_di`] only builds the lanes and propagates.
///
/// Shared by BOTH callers on purpose (`calibrate_profile` and `probe --capture-wav`)
/// so their silence guards can't drift. That does tighten `--capture-wav`: it now
/// refuses an unmeasurable take instead of writing a WAV that later reads `-inf`
/// LUFS. Deliberate — that WAV was never usable.
pub(crate) fn dry_take_fault(lanes: &[DryLane], strip: Option<&Usb3Strip>) -> Option<String> {
    let carried = lanes
        .get(audio::DRY_INSTRUMENT_IN_CH)
        .is_some_and(|l| l.measurable);
    (!carried).then(|| silent_dry_diagnosis(lanes, strip))
}

/// Why a dry-DI take read silent on USB-Out 3, told from which OTHER lanes carried
/// MEASURABLE audio (index = capture channel: 0/1 processed, 2 dry instrument,
/// 3 dry mic/line). Reached only through [`dry_take_fault`].
/// The lane that DID carry signal names the routing fault, so the error is
/// self-diagnosing instead of blaming the player: signal on USB-Out 4 means the
/// guitar is in the MIC/LINE jack; signal on 1/2 only means the unit IS
/// processing the guitar but its USB 3 dry send is off (USB mode / mixer strip);
/// nothing anywhere means nothing reached the unit (not playing, or its input
/// muted by re-amp).
///
/// A `strip` whose state makes silence CERTAIN ([`usb3_silent_cause`]) outranks all
/// three lane guesses — it is a fact about the send, not an inference from what the
/// other lanes carried. It is consulted only on this silent path: the snapshot can be
/// stale, and prepending it to EVERY `capture_dry_di` failure (the shape this
/// replaced) turned unrelated faults — a mid-capture unplug, a channel-count
/// mismatch — into a confident "USB 3 is MUTED" misdiagnosis.
pub(crate) fn silent_dry_diagnosis(lanes: &[DryLane], strip: Option<&Usb3Strip>) -> String {
    let lane = |i: usize| {
        lanes.get(i).copied().unwrap_or(DryLane {
            peak: 0.0,
            measurable: false,
        })
    };
    // USB-Out 1/2 is reported as one lane: either channel carrying audio means the
    // unit IS processing, and the readout shows the louder of the pair.
    let processed = DryLane {
        peak: lane(0).peak.max(lane(1).peak),
        measurable: lane(0).measurable || lane(1).measurable,
    };
    // A lane below the meter's gate prints as "silent" rather than as its noise
    // floor's dBFS — an idle −74.6 dBFS lane carries nothing, and saying so beats
    // printing a number that looks like signal.
    let dbfs = |l: DryLane| {
        if l.measurable {
            format!("{:.0} dBFS", 20.0 * l.peak.log10())
        } else {
            "silent".to_string()
        }
    };
    let certain = strip.and_then(usb3_silent_cause);
    let hint = if let Some(cause) = certain.as_deref() {
        cause
    } else if lane(3).measurable {
        "the signal is on USB-Out 4 (the MIC/LINE jack) — plug the guitar into the \
         front INSTRUMENT input"
    } else if processed.measurable {
        "the device is processing a signal but USB-Out 3 carries no dry instrument \
         send — the USB 3 channel is probably MUTED in the device mixer (on the unit: \
         Mixer → USB 3 → unmute M, fader up); also check Global Settings → I/O → USB \
         is in Standard mode"
    } else {
        "nothing reached the device's USB outs — play continuously during the capture \
         (guitar in the front INSTRUMENT input, volume up); if the guitar is also \
         inaudible through the unit, its input is muted (re-amp mode); if it IS \
         audible, the USB 3 channel may be muted in the device mixer (Mixer → USB 3 → \
         unmute M)"
    };
    format!(
        "no instrument signal captured — {hint} [USB-Out 1/2 {} · 3 {} · 4 {}]",
        dbfs(processed),
        dbfs(lane(2)),
        dbfs(lane(3))
    )
}

/// #124 pre-flight, mute half: a muted `USB 3` strip is no dry send at all — the
/// DEFINITE cause of a silent take, so it outranks [`silent_dry_diagnosis`]'s lane
/// hint. No live mixer write is confirmed on this hardware, so the fix is the
/// user's, on the unit.
pub(crate) fn usb3_muted_hint(strip: &Usb3Strip) -> Option<String> {
    strip.mute.then(|| {
        "USB 3 is MUTED in the device mixer, so the dry instrument send is off — on \
         the unit: Global Settings → Mixer → USB 3 → unmute (M), then calibrate again"
            .to_string()
    })
}

/// Every `USB 3` strip state under which a silent USB-Out 3 is EXPLAINED rather than
/// guessed at — the two that make the send carry nothing at all:
/// - muted ([`usb3_muted_hint`]);
/// - `POST` with the fader all the way down, where the fader IS the send's gain.
///
/// `PRE` is fader-independent, so a `PRE` strip at any fader explains nothing and
/// returns `None` — the lane hints then speak. Distinct from [`usb3_fader_fault`],
/// which judges whether a take that LANDED is metrologically usable; this one only
/// explains a take that produced nothing.
pub(crate) fn usb3_silent_cause(strip: &Usb3Strip) -> Option<String> {
    usb3_muted_hint(strip).or_else(|| {
        (!strip.pre && strip.fader.abs() <= 0.01).then(|| {
            "USB 3 is on POST with its fader at zero, so the dry instrument send is \
             silent — on the unit: Global Settings → Mixer → USB 3 → set it to PRE (or \
             raise the fader to unity), then calibrate again"
                .to_string()
        })
    })
}

/// #124 pre-flight, gain half: on `POST` the fader scales the dry send, so an
/// off-unity fader yields a DI whose loudness is NOT the instrument's. Refused
/// rather than compensated: the fader's dB law is unmeasured. `PRE` at any
/// fader, or `POST` at unity, is clean.
///
/// THIS ONE DOES VETO A TAKE THAT LANDED, deliberately — the asymmetry with
/// [`usb3_muted_hint`] is the whole point, and mistaking it for a bug is a
/// well-trodden path (both a review pass and CodeRabbit filed it as one). The reason
/// is that a successful take persists TWO things, not just the scalar: `store_capture`
/// writes the capture, and `resolve_stimulus_for_leveling` then injects that WAV
/// **verbatim at gain 1** as the profile's stimulus. A fader-scaled capture therefore
/// drives every later re-amp through a nonlinear amp chain at the wrong level — wrong
/// breakup, wrong `C`, silently wrong leveling and Doctor verdicts until someone
/// recalibrates. Warning instead of refusing would trade a recoverable annoyance for
/// durable, invisible corruption. (`clipped` is NOT a counter-precedent: it warns AND
/// `store_capture` unlinks the capture, so no corrupt artifact survives.)
///
/// The snapshot behind it CAN be stale, which is why the message names the refresh:
/// a detach fires `resetLibraryScan`, so a replug re-reads the mixer state.
pub(crate) fn usb3_fader_fault(strip: &Usb3Strip) -> Option<String> {
    (!strip.pre && (strip.fader - 1.0).abs() > 0.01).then(|| {
        format!(
            "USB 3 is on POST with its fader at {:.0} % — the dry instrument send would be \
             fader-scaled, and the capture is stored as the leveling stimulus, so this take \
             cannot be trusted; on the unit set USB 3 to PRE (or its fader to unity), then \
             unplug and replug the unit so the app re-reads the mixer, and calibrate again",
            strip.fader * 100.0
        )
    })
}

#[cfg(test)]
mod usb3_strip_tests {
    use super::{usb3_fader_fault, usb3_muted_hint};
    use crate::backup_read::{usb3_strip, Usb3Strip};

    // The bug's third report (fw 1.8.58, 2026-08-21): the take read
    // `[USB-Out 1/2 -14 dBFS · 3 silent · 4 silent]` and the unit's own settings
    // snapshot held this strip — verbatim from its `support/device-settings.json`.
    const UNIT_1_8_58: &str = r#"{"mixerSaveData":{"usb3":{"faderLevel":1.0,"linkToMasterLvl":false,"muteActive":true,"preEnabled":true,"soloActive":false},"usb4":{"faderLevel":1.0,"linkToMasterLvl":false,"muteActive":true,"preEnabled":true,"soloActive":false}}}"#;

    #[test]
    fn the_reporting_units_snapshot_decodes_to_a_muted_strip() {
        let s = usb3_strip(UNIT_1_8_58).expect("strip present");
        assert_eq!(
            s,
            Usb3Strip {
                mute: true,
                fader: 1.0,
                pre: true
            }
        );
        let hint = usb3_muted_hint(&s).expect("muted hint");
        assert!(hint.contains("MUTED") && hint.contains("Mixer"), "{hint}");
        assert!(usb3_fader_fault(&s).is_none(), "PRE at unity is gain-clean");
    }

    #[test]
    fn post_with_an_off_unity_fader_is_refused_pre_is_not() {
        let post = Usb3Strip {
            mute: false,
            fader: 0.5,
            pre: false,
        };
        let f = usb3_fader_fault(&post).expect("fader fault");
        assert!(f.contains("POST") && f.contains("50 %"), "{f}");
        let pre = Usb3Strip { pre: true, ..post };
        assert!(usb3_fader_fault(&pre).is_none(), "PRE is fader-independent");
        let post_unity = Usb3Strip { fader: 1.0, ..post };
        assert!(usb3_fader_fault(&post_unity).is_none());
        assert!(usb3_muted_hint(&post).is_none());
    }

    #[test]
    fn a_snapshot_without_the_strip_is_none_not_a_false_fault() {
        assert!(usb3_strip(r#"{"mixerSaveData":{"usb12":{}}}"#).is_none());
        assert!(usb3_strip("not json").is_none());
    }
}

#[cfg(test)]
mod dry_diagnosis_tests {
    use super::{dry_take_fault, silent_dry_diagnosis, usb3_silent_cause, DryLane, Usb3Strip};

    /// A lane carrying real audio at `peak`.
    fn live(peak: f32) -> DryLane {
        DryLane {
            peak,
            measurable: true,
        }
    }

    /// A lane with nothing measurable on it — `peak` is its NOISE FLOOR, which on a
    /// real unit is emphatically not zero (see the floor incident below).
    fn floor(peak: f32) -> DryLane {
        DryLane {
            peak,
            measurable: false,
        }
    }

    // The "calibration can't see the guitar" bug class, second report (fw 1.8.58):
    // the take read silent on USB-Out 3 and the old error only said "play louder".
    // The diagnosis must name the lane that DID carry signal.

    #[test]
    fn signal_on_the_mic_line_lane_points_at_the_wrong_jack() {
        let d = silent_dry_diagnosis(&[live(0.2), live(0.2), floor(0.0), live(0.3)], None);
        assert!(d.contains("MIC/LINE"), "{d}");
        assert!(d.contains("4 -10 dBFS"), "{d}");
    }

    #[test]
    fn processed_only_points_at_the_usb_dry_send() {
        let d = silent_dry_diagnosis(&[live(0.5), live(0.4), floor(0.0), floor(0.0)], None);
        assert!(d.contains("USB 3") && d.contains("MUTED"), "{d}");
        assert!(d.contains("1/2 -6 dBFS"), "{d}");
        assert!(d.contains("3 silent"), "{d}");
    }

    #[test]
    fn all_silent_names_the_muted_input() {
        let d = silent_dry_diagnosis(&[floor(0.0), floor(0.0), floor(0.0), floor(0.0)], None);
        assert!(
            d.contains("re-amp") && d.contains("muted in the device mixer"),
            "{d}"
        );
        assert!(d.contains("1/2 silent"), "{d}");
    }

    #[test]
    fn a_short_capture_is_read_as_silent_lanes_not_a_panic() {
        let d = silent_dry_diagnosis(&[floor(0.0), floor(0.0), floor(0.0)], None);
        assert!(d.contains("4 silent"), "{d}");
    }

    // ── the snapshot is a TIE-BREAKER on the silent path, nothing more ────────
    // Review found the mute hint prepended to EVERY `capture_dry_di` failure, so a
    // mid-capture unplug reported "USB 3 is MUTED … [could not reach the device]".
    // The strip now reaches only this function, and only outranks the lane guesses
    // when it makes silence CERTAIN.

    #[test]
    fn a_muted_strip_outranks_the_lane_guess_and_keeps_the_readout() {
        let muted = Usb3Strip {
            mute: true,
            fader: 1.0,
            pre: true,
        };
        // Lanes alone would blame the MIC/LINE jack; the strip knows better.
        let d = silent_dry_diagnosis(&[live(0.2), live(0.2), floor(0.0), live(0.3)], Some(&muted));
        assert!(d.contains("MUTED"), "{d}");
        assert!(
            !d.contains("MIC/LINE"),
            "lane guess should be outranked: {d}"
        );
        assert!(d.contains("4 -10 dBFS"), "readout must survive: {d}");
    }

    #[test]
    fn post_at_zero_is_a_certain_silent_cause_but_pre_is_not() {
        let post_zero = Usb3Strip {
            mute: false,
            fader: 0.0,
            pre: false,
        };
        assert!(
            usb3_silent_cause(&post_zero).is_some_and(|c| c.contains("POST") && c.contains("zero"))
        );

        // PRE is fader-independent — it explains nothing, so the lane hints speak.
        let pre_zero = Usb3Strip {
            pre: true,
            ..post_zero
        };
        assert!(usb3_silent_cause(&pre_zero).is_none());
        let d = silent_dry_diagnosis(
            &[floor(0.0), floor(0.0), floor(0.0), live(0.3)],
            Some(&pre_zero),
        );
        assert!(d.contains("MIC/LINE"), "{d}");

        // A clean strip never manufactures a cause.
        let clean = Usb3Strip {
            mute: false,
            fader: 1.0,
            pre: true,
        };
        assert!(usb3_silent_cause(&clean).is_none());
    }

    // ── the noise-floor incident (fw 1.8.45, probe --capture-input, 2026-08-24) ──
    // The dev unit idles at −74.6 dBFS on the dry lane and −71.5 on the processed
    // pair. BOTH clear the −80 dBFS peak floor the guard used to key on, so a
    // not-playing take there was classed as GOOD: it returned Ok and surfaced much
    // later as the terse "too quiet to measure", never running this diagnosis.
    // Guarding on integrated loudness instead fixes it with no tunable — and the
    // second assertion is the reason the LANE test had to move too: the processed
    // pair's floor clears the very same threshold, so a peak-keyed lane hint would
    // have told a user on a spotless unit that USB 3 was muted.

    /// −74.6 dBFS, the dev unit's idle dry lane.
    const IDLE_DRY_PEAK: f32 = 1.86e-4;
    /// −71.5 dBFS, the same unit's idle processed pair.
    const IDLE_PROCESSED_PEAK: f32 = 2.66e-4;

    #[test]
    fn an_idle_lane_above_the_old_peak_floor_still_faults_and_blames_no_one() {
        let lanes = [
            floor(IDLE_PROCESSED_PEAK),
            floor(IDLE_PROCESSED_PEAK),
            floor(IDLE_DRY_PEAK),
            floor(IDLE_DRY_PEAK),
        ];
        // It must be a fault at all — this is what the peak floor let through.
        let d = dry_take_fault(&lanes, None).expect("an unmeasurable dry lane must fault");
        assert!(d.contains("play continuously"), "{d}");
        assert!(
            !d.contains("processing a signal"),
            "an idle processed pair is not signal: {d}"
        );
        assert!(d.contains("1/2 silent") && d.contains("3 silent"), "{d}");
    }

    #[test]
    fn a_measurable_dry_lane_is_no_fault() {
        assert!(dry_take_fault(&[live(0.2), live(0.2), live(0.1), floor(0.0)], None).is_none());
    }

    #[test]
    fn a_capture_that_lacks_the_dry_lane_faults_rather_than_passing() {
        assert!(dry_take_fault(&[live(0.2), live(0.2)], None).is_some());
    }

    /// Deterministic pseudo-noise at `peak` — xorshift, so this can never flake.
    fn noise(peak: f32, samples: usize) -> Vec<f32> {
        let mut x = 0x2545_f491_4f6c_dd1d_u64;
        (0..samples)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                ((x >> 40) as f32 / 8_388_608.0 - 1.0) * peak
            })
            .collect()
    }

    // Every test above works on `DryLane` LITERALS, which proves the branching but
    // assumes the mapping. This one runs real buffers through the meter, because
    // "this unit's floors sit under the −70 LUFS absolute gate" is the empirical
    // premise the whole guard rests on: if that gate did NOT kill this unit's
    // −74.6 dBFS floor, the guard would pass exactly the takes it exists to catch,
    // and every literal-based test above would still be green.
    #[test]
    fn the_meter_maps_a_noise_floor_to_unmeasurable_and_a_played_level_to_measurable() {
        let floor = DryLane::of(&noise(IDLE_DRY_PEAK, 48_000 * 3), 48_000);
        assert!(
            !floor.measurable,
            "a −74.6 dBFS floor must fall under the −70 LUFS absolute gate"
        );
        assert!(
            floor.peak > 0.0 && floor.peak <= IDLE_DRY_PEAK,
            "{:?}",
            floor
        );

        // The processed pair's floor is louder and must ALSO stay unmeasurable —
        // this is the half that keeps the lane hint from crying "MUTED".
        assert!(!DryLane::of(&noise(IDLE_PROCESSED_PEAK, 48_000 * 3), 48_000).measurable);

        // A normally played level measures, so the gate is not simply always-off.
        let played = DryLane::of(&noise(0.2, 48_000 * 3), 48_000);
        assert!(played.measurable, "{:?}", played);
    }
}

/// True clipping flat-tops the waveform — a RUN of consecutive samples pinned at
/// full scale. A clean guitar pick attack is a sub-millisecond transient (one or
/// two samples at the apex) that a device output meter's ballistics smooth away
/// but a raw sample-peak catches, so a bare `peak >= 0.99` gate over-fires on hot
/// transients (the DI tap is a bit-exact USB bus — no analog headroom to lose, so
/// only genuine flat-topping corrupts a take). Flag a capture as clipped only when
/// `MIN_CLIP_RUN` consecutive samples sit at/above `CLIP_LEVEL`.
const CLIP_LEVEL: f32 = 0.999;
const MIN_CLIP_RUN: usize = 4; // ≈83 µs @ 48 kHz — impossible for a clean transient apex
pub(crate) fn is_clipped_capture(samples: &[f32]) -> bool {
    let mut run = 0usize;
    for &x in samples {
        if x.abs() >= CLIP_LEVEL {
            run += 1;
            if run >= MIN_CLIP_RUN {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

/// Capture the dry instrument (USB-Out 3) for `secs` while the user plays and
/// save it as a 48 kHz mono f32 WAV — the real-DI side of `--stim-ab`. Reports
/// peak (clip check — the dry tap has no limiter) and integrated LUFS.
pub fn probe_capture_wav(path: &str, secs: f32) -> Result<String, String> {
    let (mono, peak) = capture_dry_di(secs, None)?;
    let lufs = lufs::measure_mono(&mono, 48_000)?.integrated_lufs;
    write_wav_mono(path, &mono, 48_000)?;
    Ok(format!(
        "wrote {path}: {:.1}s  peak {:+.1} dBFS{}  integrated {lufs:.2} LUFS\n",
        mono.len() as f32 / 48_000.0,
        20.0 * peak.log10(),
        if is_clipped_capture(&mono) {
            "  ⚠ CLIPPED — recapture softer"
        } else {
            ""
        },
    ))
}

/// Scale a 48 kHz WAV to a target integrated LUFS via the production Tier-2
/// calibration transform (`read_stimulus_calibrated_with_shortfall`, 0.99 peak
/// cap included) and write the result — the matched-synthetic side of `--stim-ab`.
pub fn probe_scale_wav(src: &str, dst: &str, target_lufs: f32) -> Result<String, String> {
    let (stim, shortfall) = read_stimulus_calibrated_with_shortfall(src, Some(target_lufs))?;
    let got = lufs::measure_mono(&stim, 48_000)?.integrated_lufs;
    write_wav_mono(dst, &stim, 48_000)?;
    Ok(format!(
        "wrote {dst}: {got:.2} LUFS (target {target_lufs:.2}{})\n",
        match shortfall {
            Some(lu) => format!(", peak-capped {lu:.1} LU short"),
            None => String::new(),
        },
    ))
}

#[cfg(test)]
mod stimulus_shortfall_tests {
    use super::read_stimulus_calibrated_with_shortfall;

    fn wav() -> String {
        format!(
            "{}/resources/samples/guitar-humbucker.wav",
            env!("CARGO_MANIFEST_DIR")
        )
    }

    #[test]
    fn unreachable_target_reports_shortfall_and_caps_peak() {
        let (stim, shortfall) = read_stimulus_calibrated_with_shortfall(&wav(), Some(0.0)).unwrap();
        let shortfall = shortfall.expect("0 LUFS is far above the stimulus ceiling");
        assert!(
            shortfall > 0.0,
            "shortfall must be positive LU: {shortfall}"
        );
        let peak = stim.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        assert!(peak <= 0.99 + 1e-4, "capped peak must stay ≤ 0.99: {peak}");
    }

    #[test]
    fn reachable_target_has_no_shortfall() {
        let (_, shortfall) = read_stimulus_calibrated_with_shortfall(&wav(), Some(-60.0)).unwrap();
        assert_eq!(shortfall, None);
    }

    #[test]
    fn uncalibrated_has_no_shortfall() {
        let (_, shortfall) = read_stimulus_calibrated_with_shortfall(&wav(), None).unwrap();
        assert_eq!(shortfall, None);
    }
}

#[cfg(test)]
mod di_inject_clip_guard_tests {
    use super::{read_stimulus_calibrated_with_shortfall, write_wav_mono};

    /// Write a tiny mono 48 kHz fixture WAV to the temp dir (reusing the same
    /// `write_wav_mono` the production capture paths write with).
    fn fixture(tag: &str, samples: &[f32]) -> String {
        let path =
            std::env::temp_dir().join(format!("tmp-stim-clip-{tag}-{}.wav", std::process::id()));
        write_wav_mono(path.to_str().unwrap(), samples, 48_000).unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    fn verbatim_full_scale_peak_gets_capped_and_shape_preserved() {
        let samples = [0.5f32, -1.0, 0.25, 1.0, -0.1];
        let path = fixture("full", &samples);
        let (out, shortfall) = read_stimulus_calibrated_with_shortfall(&path, None).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(shortfall, None, "no calibration target → no shortfall");
        let peak = out.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        assert!(peak <= 0.99 + 1e-4, "peak {peak} not capped at 0.99");
        assert_eq!(out.len(), samples.len());
        // Uniform scale-down: every sample's ratio to the original is identical.
        let g = out[1] / samples[1];
        assert!(g <= 1.0, "must never scale UP: g={g}");
        for (o, s) in out.iter().zip(samples.iter()) {
            assert!(
                (o - s * g).abs() < 1e-6,
                "ratio not preserved: {o} vs {s}*{g}"
            );
        }
    }

    #[test]
    fn verbatim_safe_peak_is_bit_identical() {
        let samples = [0.5f32, -0.3, 0.1, -0.5, 0.0];
        let path = fixture("half", &samples);
        let (out, shortfall) = read_stimulus_calibrated_with_shortfall(&path, None).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(shortfall, None);
        assert_eq!(out, samples, "0.5 peak must pass through unscaled");
    }
}

#[cfg(test)]
mod clip_gate_tests {
    use super::is_clipped_capture;

    #[test]
    fn isolated_transient_apexes_are_not_clipping() {
        // A clean take: low sustained level with occasional single-sample full-scale
        // pick-attack apexes — never a sustained flat top. The old `peak >= 0.99`
        // gate rejected exactly this; the run-length gate must accept it.
        let mut s = vec![0.2f32; 48_000];
        for i in (0..s.len()).step_by(4000) {
            s[i] = 1.0;
        }
        assert!(!is_clipped_capture(&s));
    }

    #[test]
    fn sustained_flat_top_is_clipping() {
        let mut s = vec![0.2f32; 48_000];
        for x in s.iter_mut().skip(100).take(8) {
            *x = 1.0; // 8 consecutive pinned samples = genuine overload
        }
        assert!(is_clipped_capture(&s));
    }
}

#[cfg(test)]
mod measure_wav_auto_tests {
    use super::{measure_wav_auto, write_wav, write_wav_mono, write_wav_stereo};
    use std::f32::consts::PI;

    fn sine(freq: f32, secs: f32, rate: u32, amp: f32) -> Vec<f32> {
        let n = (secs * rate as f32) as usize;
        (0..n)
            .map(|i| amp * (2.0 * PI * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    fn interleave2(l: &[f32], r: &[f32]) -> Vec<f32> {
        l.iter().zip(r).flat_map(|(&a, &b)| [a, b]).collect()
    }

    fn fixture(tag: &str) -> String {
        std::env::temp_dir()
            .join(format!(
                "tmp-measure-wav-auto-{tag}-{}.wav",
                std::process::id()
            ))
            .to_string_lossy()
            .to_string()
    }

    #[test]
    fn mono_file_measures_via_the_mono_convention() {
        let rate = 48_000;
        let tone = sine(1000.0, 4.0, rate, 0.4);
        let path = fixture("mono");
        write_wav_mono(&path, &tone, rate).unwrap();
        let (l, ch) = measure_wav_auto(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(ch, 1);
        let direct = crate::lufs::measure_mono(&tone, rate).unwrap();
        assert!((l.integrated_lufs - direct.integrated_lufs).abs() < 1e-6);
    }

    #[test]
    fn dual_mono_stereo_file_reads_3_01_over_a_mono_file_of_the_same_tone() {
        // The offline proof that `--measure-wav` picks the OUTPUT-side (2-ch)
        // convention for a stereo file, not a mono downmix — matches the
        // `lufs.rs` module-header +3.0103 dB dual-mono constant.
        let rate = 48_000;
        let tone = sine(1000.0, 4.0, rate, 0.4);
        let mono_path = fixture("dualmono-mono");
        let stereo_path = fixture("dualmono-stereo");
        write_wav_mono(&mono_path, &tone, rate).unwrap();
        write_wav_stereo(&stereo_path, &interleave2(&tone, &tone), rate).unwrap();
        let (mono, mono_ch) = measure_wav_auto(&mono_path).unwrap();
        let (stereo, stereo_ch) = measure_wav_auto(&stereo_path).unwrap();
        let _ = std::fs::remove_file(&mono_path);
        let _ = std::fs::remove_file(&stereo_path);
        assert_eq!(mono_ch, 1);
        assert_eq!(stereo_ch, 2);
        assert!(
            (stereo.integrated_lufs - mono.integrated_lufs - 3.0103).abs() < 0.05,
            "mono {:.3} stereo {:.3}",
            mono.integrated_lufs,
            stereo.integrated_lufs
        );
    }

    #[test]
    fn extra_channels_beyond_the_pair_are_dropped_not_mixed_in() {
        // A 3-channel file (mirrors a `--dump-wav`-adjacent capture with a dry
        // send appended) must measure identically to its own first-2-channel
        // truncation — never mix the 3rd channel in.
        let rate = 48_000;
        let l = sine(1000.0, 3.0, rate, 0.4);
        let r = sine(1300.0, 3.0, rate, 0.2);
        let dry = sine(220.0, 3.0, rate, 0.9); // loud — would visibly skew a mix-in
        let three: Vec<f32> = l
            .iter()
            .zip(&r)
            .zip(&dry)
            .flat_map(|((&a, &b), &c)| [a, b, c])
            .collect();
        let path = fixture("three-ch");
        write_wav(&path, &three, rate, 3).unwrap();
        let (three_ch_result, ch) = measure_wav_auto(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(ch, 3);
        let pair_only = super::lufs::measure_stereo(&interleave2(&l, &r), rate).unwrap();
        assert!(
            (three_ch_result.integrated_lufs - pair_only.integrated_lufs).abs() < 1e-6,
            "3-ch read {:.3} must match pair-only {:.3}",
            three_ch_result.integrated_lufs,
            pair_only.integrated_lufs
        );
    }
}

#[cfg(test)]
mod dump_processed_capture_tests {
    use super::dump_processed_capture;
    use crate::audio::Capture;

    #[test]
    fn stereo_capture_dumps_the_processed_pair_and_drops_the_dry_channel() {
        let cap = Capture {
            interleaved: vec![1.0, 0.4, 0.0, 0.5, 0.2, 0.0], // 2 frames, 3 channels (ch2 = dry)
            channels: 3,
            sample_rate: 48_000,
        };
        let dir = std::env::temp_dir()
            .join(format!("tmp-dump-capture-{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let path = dump_processed_capture(&cap, &dir, "slot12_scene3").unwrap();
        assert!(path.ends_with("slot12_scene3.wav"));
        let (samples, rate, ch) = super::read_wav_interleaved(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(ch, 2, "dry channel 2 must be excluded from the dump");
        assert_eq!(rate, 48_000);
        assert!((samples[0] - 1.0).abs() < 1e-6);
        assert!((samples[1] - 0.4).abs() < 1e-6);
        assert!((samples[2] - 0.5).abs() < 1e-6);
        assert!((samples[3] - 0.2).abs() < 1e-6);
    }

    #[test]
    fn mono_capture_falls_back_to_a_mono_dump() {
        let cap = Capture {
            interleaved: vec![0.3, -0.2, 0.1],
            channels: 1,
            sample_rate: 48_000,
        };
        let dir = std::env::temp_dir()
            .join(format!("tmp-dump-capture-mono-{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let path = dump_processed_capture(&cap, &dir, "current_current").unwrap();
        let (samples, _, ch) = super::read_wav_interleaved(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            ch, 1,
            "a genuinely mono capture must not be duplicated to fake stereo"
        );
        assert_eq!(samples, vec![0.3, -0.2, 0.1]);
    }
}
