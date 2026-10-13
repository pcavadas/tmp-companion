//! Probe stimulus entry points: `--lufs`/`--measure-wav` file meters, the offline capture
//! harness, `--doctor`, `--stim-ab`, `--capture-wav` and `--scale-wav`. The shared stimulus
//! and WAV I/O they call is `crate::stimulus`.

use super::SCRATCH_SLOTS as SCRATCH;
use crate::audio;
use crate::doctor;
use crate::footswitch;
use crate::leveller;
use crate::lufs;
use crate::read_slot_preset_parsed;
use crate::session;
use crate::slot::ListIndex;
use crate::stimulus::{
    capture_dry_di, is_clipped_capture, read_stimulus_48k, read_stimulus_calibrated_with_shortfall,
    read_wav_interleaved, read_wav_mono, write_wav_mono,
};
use crate::topologies;

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

/// OFFLINE HARNESS (1/3): capture ONE full re-amp clip of `slot` at the leveling
/// reference level (0.5) via the topology stimulus and write it to `out` (mono,
/// 48 kHz f32) — building a corpus of real processed clips so the adaptive-capture
/// constants can be tuned with no device. DEVICE OP (load + engage + full capture).
pub fn probe_capture_reference(
    slot: ListIndex,
    topology_id: &str,
    out: &str,
) -> Result<String, String> {
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
    slot: ListIndex,
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
    slot: ListIndex,
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
pub fn probe_measure_adaptive(slot: ListIndex, topology_id: &str) -> Result<String, String> {
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
            leveller::processed_lufs(audio::reamp_capture(&stim, 48_000, 800)).map_err(String::from)
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
pub fn probe_doctor(
    slots: &[(ListIndex, Option<u32>)],
    topology_id: &str,
) -> Result<String, String> {
    // Mirrors doctor_check's capture space: the production 3 s Doctor window.
    let stim = leveller::doctor_stim_slice(read_stimulus_48k(&probe_stimulus_path(topology_id)?)?);
    let instrument = doctor::Family::from_topology(
        topologies::by_id(topology_id)
            .map(|t| t.instrument)
            .unwrap_or("guitar"),
    );

    type Sound = (
        ListIndex,
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
                let onset = leveller::doctor_onset(&stim, &samples, rate);
                if !onset.confident() {
                    eprintln!(
                        "[probe] slot {slot}: onset not confidently found ({:?}) — un-aligned split",
                        onset.source
                    );
                }
                // Share ONE body PSD between the profile and the coverage gate
                // (mirrors `analyze_capture` / production `doctor_check`) so this
                // calibration sweep's printed verdicts match what the app would
                // actually fire, instead of the gate-free `diagnose` shim.
                let body_psd = doctor::body_psd(&samples, rate, onset.signal_start);
                let stim_psd = crate::psd::welch_psd(&stim, rate as f32);
                // Skip-and-continue like the capture-failure arm below — a `?`
                // here (e.g. one silent-inject slot) would discard the whole
                // sweep, including every slot already captured.
                match doctor::SoundProfile::from_capture_with_psd(
                    &samples,
                    rate,
                    onset.body_len,
                    onset.body_start,
                    leveller::DOCTOR_TAIL_MS,
                    instrument,
                    &body_psd,
                    Some(&stim_psd),
                ) {
                    Ok(profile) => {
                        let coverage = doctor::output_coverage_with_body(
                            &samples,
                            rate,
                            onset.signal_start,
                            instrument,
                            &body_psd,
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
    slots: &[ListIndex],
    wav_a: &str,
    wav_b: &str,
    ref_level: f32,
) -> Result<String, String> {
    let stim_a = read_stimulus_48k(wav_a)?;
    let stim_b = read_stimulus_48k(wav_b)?;
    // Floor reads are handled INSIDE measure_c now (the production floor guard:
    // stimulus-aware momentary-spread trip → same-ref retry) — a persistent floor read
    // surfaces as leveller::FLOOR_READ_ERR in the row.
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
mod measure_wav_auto_tests {
    use super::measure_wav_auto;
    use crate::stimulus::{write_wav, write_wav_mono, write_wav_stereo};
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
