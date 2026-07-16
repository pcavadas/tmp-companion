//! Stimulus WAV I/O + calibration + probe measurement entry points.

use crate::audio;
use crate::doctor;
use crate::footswitch;
use crate::leveller;
use crate::lufs;
use crate::read_slot_preset_parsed;
use crate::session;
use crate::topologies;

/// Read a WAV file and downmix to mono f32 in [-1, 1] (fixed mono convention).
/// Returns (samples, sample_rate).
fn read_wav_mono(path: &str) -> Result<(Vec<f32>, u32), String> {
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
    let mono: Vec<f32> = interleaved
        .chunks(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect();
    Ok((mono, spec.sample_rate))
}

/// Read a WAV file, downmix to mono f32, and measure its loudness. Used by
/// `probe --lufs <wav>` to validate `lufs.rs` against an external oracle
/// (pyloudnorm / ffmpeg ebur128) without any device.
pub fn measure_wav_file(path: &str) -> Result<lufs::Loudness, String> {
    let (mono, rate) = read_wav_mono(path)?;
    lufs::measure_mono(&mono, rate)
}

/// Write a mono f32 buffer to a WAV (the offline reference-clip corpus format +
/// the Tier-2 calibration capture store via `profiles::store_capture`).
pub(crate) fn write_wav_mono(path: &str, samples: &[f32], sample_rate: u32) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| format!("create {path}: {e}"))?;
    for &s in samples {
        w.write_sample(s)
            .map_err(|e| format!("write {path}: {e}"))?;
    }
    w.finalize().map_err(|e| format!("finalize {path}: {e}"))
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
    let r = audio::replay_measure(&mono, rate, opts)?;
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
            leveller::loudest_lufs(audio::reamp_capture(&stim, 48_000, 800))
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
    let stim = read_stimulus_48k(&probe_stimulus_path(topology_id)?)?;
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
    );
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
                let profile = doctor::SoundProfile::from_capture(
                    &samples,
                    rate,
                    stim.len(),
                    onset,
                    instrument,
                )?;
                sounds.push((slot, scene, profile, nodes));
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
    for (slot, scene, profile, nodes) in &sounds {
        let diags = doctor::diagnose(profile, nodes.as_deref(), instrument);
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
    if let Some(target_lufs) = calibration_lufs {
        let stim_lufs = lufs::measure_mono(&stim, 48_000)?.integrated_lufs;
        if stim_lufs.is_finite() {
            let mut g = 10f32.powf((target_lufs - stim_lufs as f32) / 20.0);
            let peak = stim.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
            if peak * g > 0.99 {
                let shortfall = 20.0 * (peak * g / 0.99).log10();
                log::warn!(
                    "stimulus calibration capped: {path} cannot reach {target_lufs:.1} LUFS \
                     without clipping — driving {shortfall:.1} LU softer"
                );
                shortfall_lu = Some(shortfall);
                g = 0.99 / peak; // guard against clipping the injected signal
            }
            for s in &mut stim {
                *s *= g;
            }
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
pub(crate) fn capture_dry_di(secs: f32) -> Result<(Vec<f32>, f32), String> {
    // Ensure normal mode (re-amp OFF) so the front instrument input flows.
    if let Ok(mut s) = session::Session::connect() {
        let _ = s.set_reamp_mode(false);
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    let cap = audio::capture_input(secs.clamp(2.0, 30.0), 48_000)?;
    let mono = cap.channel(audio::DRY_INSTRUMENT_IN_CH);
    let peak = cap.channel_peak(audio::DRY_INSTRUMENT_IN_CH);
    if peak < 1e-4 {
        return Err("no instrument signal captured — play continuously during \
                    the capture (guitar in the front INSTRUMENT input, volume up)"
            .to_string());
    }
    Ok((mono, peak))
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
    let (mono, peak) = capture_dry_di(secs)?;
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
