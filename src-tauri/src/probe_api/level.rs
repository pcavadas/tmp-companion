//! Probe entry points: preset leveling + amp-candidate filtering + channel/capture/AGC diagnostics.

use super::scene_bench::knob_bounds;
use super::scene_jobs::is_amp_model_id;
use super::scene_jobs::is_amp_output_level_param;
use super::slot_write::load_then_discover_blocks;
use super::stimulus::probe_stimulus_path;
use super::stimulus::read_stimulus_48k;
use super::stimulus::read_stimulus_calibrated;
use crate::audio;
use crate::leveller;
use crate::lufs;
use crate::session;
use crate::session::Session;
use crate::LevelBlockArg;

/// Measure the currently selected preset/scene through re-amp without changing
/// preset level or block parameters. Optional `slot` loads a preset first in its
/// own connection; optional `scene_slot` recalls a scene before capture. No save.
pub fn probe_measure_current_lufs(
    topology_id: &str,
    slot: Option<u32>,
    scene_slot: Option<u32>,
    calibration_lufs: Option<f32>,
) -> Result<String, String> {
    let stim_path = probe_stimulus_path(topology_id)?;
    let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;
    if let Some(slot) = slot {
        {
            let mut s = Session::connect()?;
            s.load_preset(slot)?;
        }
        std::thread::sleep(std::time::Duration::from_millis(800));
    }
    let mut s = Session::connect()?;
    if let Some(scene) = scene_slot {
        s.load_scene(scene)?;
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    s.set_reamp_mode(true)?;
    std::thread::sleep(std::time::Duration::from_millis(500));
    let cap = audio::reamp_capture(&stim, 48_000, 800);
    let _ = s.set_reamp_mode(false);
    let cap = cap?;
    let (ch, _) = cap.loudest_channel();
    let loud = lufs::measure_mono(&cap.channel(ch), cap.sample_rate)?;
    if !loud.integrated_lufs.is_finite() {
        return Err("no finite signal captured (re-amp may not have routed)".to_string());
    }
    Ok(format!(
        "slot={} topology={topology_id} scene={} channel={ch} integrated_lufs={:.3} short_term_max_lufs={:.3}",
        slot.map(|s| s.to_string()).unwrap_or_else(|| "current".to_string()),
        scene_slot.map(|s| s.to_string()).unwrap_or_else(|| "current".to_string()),
        loud.integrated_lufs,
        loud.short_term_max_lufs,
    ))
}

/// HW probe: does re-amp survive a DISENGAGE → settle → RE-ENGAGE on ONE held HID
/// connection? The whole leveling speed story hinges on this. If a single held
/// session can do N `[load_scene → engage → capture → disengage]` cycles and read
/// the SAME loudness a fresh connection reads, we can keep Pro Control's one
/// persistent session (instant scene changes). If not, the proven once-engage-per-
/// connection rule stands. Non-destructive: loads + scene recalls + captures, NO
/// parameter writes. Measures each scene twice — once on a HELD session, once via
/// the proven FRESH-connection control — and compares.
pub fn probe_held_reengage(
    topology_id: &str,
    slot: u32,
    scenes: &[u32],
    calibration_lufs: Option<f32>,
) -> Result<String, String> {
    use std::time::Duration;
    let stim_path = probe_stimulus_path(topology_id)?;
    let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;

    let measure = |cap: Result<audio::Capture, String>| -> f64 {
        match cap {
            Ok(cap) => {
                let (ch, _) = cap.loudest_channel();
                lufs::measure_mono(&cap.channel(ch), cap.sample_rate)
                    .map(|l| l.integrated_lufs)
                    .unwrap_or(f64::NAN)
            }
            Err(_) => f64::NAN,
        }
    };

    // Load the preset in its OWN throwaway connection (the load+engage→silence rule).
    {
        let mut s = Session::connect()?;
        s.load_preset(slot)?;
    }
    std::thread::sleep(Duration::from_millis(800));

    let mut out = format!(
        "HELD-SESSION RE-ENGAGE probe — slot={slot} topology={topology_id} scenes={scenes:?}\n\n"
    );

    // ── HELD: ONE connection, N [load_scene → engage → capture → disengage] cycles.
    out += "[A] HELD session (one connection, re-engage per scene):\n";
    let mut held = Vec::new();
    {
        let mut s = Session::connect()?;
        for (i, &scene) in scenes.iter().enumerate() {
            s.load_scene(scene)?;
            std::thread::sleep(Duration::from_millis(500));
            let echo = s.set_reamp_mode(true)?;
            std::thread::sleep(Duration::from_millis(500));
            let cap = audio::reamp_capture(&stim, 48_000, 800);
            let _ = s.set_reamp_mode(false);
            std::thread::sleep(Duration::from_millis(500)); // disengage settle before next cycle
            let m = measure(cap);
            out += &format!(
                "    cycle {i}: scene={scene} engage_echo={echo:?} integrated_lufs={m:.3}\n"
            );
            held.push(m);
        }
    }

    // ── CONTROL: FRESH connection per scene (the proven measure_scene_asis shape).
    out += "\n[B] FRESH connection per scene (proven control):\n";
    let mut fresh = Vec::new();
    for &scene in scenes {
        let mut s = Session::connect()?;
        s.load_scene(scene)?;
        std::thread::sleep(Duration::from_millis(500));
        s.set_reamp_mode(true)?;
        std::thread::sleep(Duration::from_millis(500));
        let cap = audio::reamp_capture(&stim, 48_000, 800);
        let _ = s.set_reamp_mode(false);
        let m = measure(cap);
        out += &format!("    scene={scene} integrated_lufs={m:.3}\n");
        fresh.push(m);
    }

    // ── Verdict.
    let all_finite = held.iter().all(|m| m.is_finite());
    let (mut mn, mut mx) = (f64::MAX, f64::MIN);
    for m in held.iter().chain(fresh.iter()).filter(|m| m.is_finite()) {
        mn = mn.min(*m);
        mx = mx.max(*m);
    }
    let scenes_differ = (mx - mn).abs() > 1.0;
    let matches_fresh = held
        .iter()
        .zip(&fresh)
        .all(|(h, f)| h.is_finite() && f.is_finite() && (h - f).abs() < 1.5);
    out += "\nVERDICT:\n";
    out += &format!("    held all non-silent:                 {all_finite}\n");
    out += &format!("    scenes genuinely differ (>1 LU):     {scenes_differ}\n");
    out += &format!("    held matches fresh (per-scene <1.5LU): {matches_fresh}\n");
    out += &format!(
        "    => HELD SESSION {}\n",
        if all_finite && matches_fresh {
            "VIABLE — re-engage works on one connection; persistent-session leveling is on the table"
        } else {
            "NOT VIABLE — re-engage is unreliable on a held connection; keep fresh-connection-per-scene"
        }
    );
    Ok(out)
}

/// M3 one-shot leveling (the real path): fresh-connect, load `slot`, measure at
/// a reference level, solve the linear model for the exact `presetLevel` that
/// hits `target_lufs`, set it, and (if `save`) persist. Optionally re-measures
/// on a second fresh connection to confirm. Re-amp is always restored OFF.
pub fn probe_level_preset(
    slot: u32,
    target_lufs: f64,
    save: bool,
    verify: bool,
) -> Result<String, String> {
    let stim_path = std::env::var("TMP_LEVELLER_STIMULUS")
        .map_err(|_| "set TMP_LEVELLER_STIMULUS to the stimulus WAV".to_string())?;
    // Optional Tier-2 calibration: scale the stimulus to a measured LUFS.
    let cal = std::env::var("TMP_LEVELLER_CAL_LUFS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    let stim = read_stimulus_calibrated(&stim_path, cal)?;

    let opts = leveller::LevelOptions {
        save,
        verify,
        ..Default::default()
    };
    let r = leveller::level_preset(slot, &stim, target_lufs, opts, &[], || false)?;

    let mut out = format!(
        "slot {slot}: measured {:.2} LUFS @ ref {:.2}  (C={:.2})\n\
         → target {:.1} LUFS  ⇒  presetLevel={:.4}{}  (predicted {:.2} LUFS){}\n",
        r.measured_lufs,
        r.ref_level,
        r.constant_c,
        r.target_lufs,
        r.final_level,
        if r.clamped {
            " [CLAMPED — target unreachable]"
        } else {
            ""
        },
        r.predicted_lufs,
        if r.saved { "  [SAVED]" } else { "" },
    );
    if let Some(m) = r.verify_lufs {
        out += &format!(
            "verify (fresh capture @ {:.4}): {:.2} LUFS  (target {:.1}, err {:+.2} LU)\n",
            r.final_level,
            m,
            target_lufs,
            m - target_lufs
        );
    }
    Ok(out)
}

/// `probe --live-lufs` — install an advisory live-LUFS sink that PRINTS each streamed
/// reading, then run the SAME path as [`probe_level_preset`], validating the whole
/// live-LUFS backend headless before any frontend exists. The final `LevelResult` summary
/// must match a plain `--levelpreset` run (the advisory meter must not perturb the solve);
/// run the A/B on a REVERB/DELAY preset to catch any capture-length re-baseline.
pub fn probe_live_lufs(
    slot: u32,
    target_lufs: f64,
    save: bool,
    verify: bool,
) -> Result<String, String> {
    audio::set_live_lufs_sink(Box::new(|lufs, mom| {
        println!("live {lufs:.2} LUFS  (mom {mom:.1} dB)")
    }));
    let r = probe_level_preset(slot, target_lufs, save, verify);
    audio::clear_live_lufs_sink();
    r
}

/// Filter already-discovered blocks to amp `outputLevel` leveling candidates — amp
/// blocks' `outputLevel` controls, the only tone-safe per-scene leveling knob. The
/// single definition of "what counts as a leveling candidate", shared by every caller
/// (the scene-leveling driver, the diagnostics, and the bench's intel session — which
/// brings its own pre-discovered blocks).
pub(crate) fn filter_amp_candidates(blocks: Vec<session::LevelBlock>) -> Vec<LevelBlockArg> {
    blocks
        .into_iter()
        .filter(|b| is_amp_model_id(&b.model_id) && is_amp_output_level_param(&b.parameter_id))
        .map(|b| LevelBlockArg {
            group_id: b.group_id,
            node_id: b.node_id,
            parameter_id: b.parameter_id,
            value: b.value,
        })
        .collect()
}

/// Run the 1.8.45-safe block discovery (`load_then_discover_blocks`) and filter it to
/// amp `outputLevel` leveling candidates.
pub(crate) fn load_and_filter_amp_candidates(
    list_index: u32,
) -> Result<Vec<LevelBlockArg>, String> {
    Ok(filter_amp_candidates(load_then_discover_blocks(
        list_index,
    )?))
}

/// Closed-loop block-control leveling on the real device: enumerate `slot` to
/// find the chosen block's current value (for sensible search bounds), then drive
/// it via `ChangeParameter` in a closed loop to `target_lufs`. Amplitude params
/// (current value in 0..1) search [0,1]; dB-unit params (e.g. an IR `outputlevel`)
/// search a ±range around the current value. Stimulus via `TMP_LEVELLER_STIMULUS`.
pub fn probe_level_block(
    slot: u32,
    target_lufs: f64,
    group_id: String,
    node_id: String,
    parameter_id: String,
) -> Result<String, String> {
    let stim_path = std::env::var("TMP_LEVELLER_STIMULUS")
        .map_err(|_| "set TMP_LEVELLER_STIMULUS to the stimulus WAV".to_string())?;
    let cal = std::env::var("TMP_LEVELLER_CAL_LUFS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    let stim = read_stimulus_calibrated(&stim_path, cal)?;

    // Discover the block's current value to choose search bounds.
    let blocks = load_then_discover_blocks(slot)?;
    let cur = blocks
        .iter()
        .find(|b| b.group_id == group_id && b.node_id == node_id && b.parameter_id == parameter_id)
        .map(|b| b.value)
        .ok_or_else(|| {
            format!(
                "{group_id}/{node_id}/{parameter_id} not found among this preset's level blocks"
            )
        })?;
    let (lo, hi) = knob_bounds(cur);

    let knob = leveller::LevelKnob::Block {
        group_id,
        node_id,
        parameter_id,
        scene_slot: None,
    };
    let opts = leveller::LevelOptions {
        save: false,
        verify: true,
        ..Default::default()
    };
    let r = leveller::level_preset_block(slot, &stim, &knob, lo, hi, target_lufs, opts, || false)?;

    let mut out = format!(
        "slot {slot}  knob {}  (current {cur:.4}, bounds [{lo:.3}, {hi:.3}])\n\
         → solved {:.4} in {} iterations  (measured {:.2} LUFS, target {:.1}{})\n",
        knob.label(),
        r.final_level,
        r.iterations,
        r.measured_lufs,
        target_lufs,
        if r.clamped {
            "  [CLAMPED — target unreachable with this knob]"
        } else {
            ""
        },
    );
    if let Some(m) = r.verify_lufs {
        out += &format!(
            "verify (fresh capture @ {:.4}): {:.2} LUFS  (err {:+.2} LU)\n",
            r.final_level,
            m,
            m - target_lufs
        );
    }
    Ok(out)
}

/// N1 diagnostic (read-only): re-amp `slot` at `presetLevel = 0.5` and report
/// PER-CHANNEL integrated LUFS + RMS for every captured USB-Out channel. Tells us
/// whether a mono preset is MIRRORED onto both USB-Out 1&2 (ch0 ≈ ch1 → the
/// single-channel measure's +3 offset is uniform and cancels across presets) or
/// sits on ONE channel (ch1 ≪ ch0 → cross-preset variance for a stereo rig).
/// Loads + re-amps only; never writes/saves/clears. Stimulus = humbucker sample
/// (override with `TMP_LEVELLER_STIMULUS`).
pub fn probe_channels(slot: u32) -> Result<String, String> {
    let stim_path = match std::env::var("TMP_LEVELLER_STIMULUS") {
        Ok(p) => p,
        Err(_) => probe_stimulus_path("guitar-humbucker")?,
    };
    let stim = read_stimulus_48k(&stim_path)?;
    let cap = leveller::capture_full(slot, &stim, 0.5)?;
    let lufs_at = |c: usize| -> Option<f64> {
        lufs::measure_mono(&cap.channel(c), cap.sample_rate)
            .ok()
            .map(|l| l.integrated_lufs)
            .filter(|v| v.is_finite())
    };
    let mut out = format!(
        "slot {slot}: {} channels @ {} Hz\n",
        cap.channels, cap.sample_rate
    );
    for c in 0..cap.channels {
        let lufs = lufs_at(c).map_or("  -inf".to_string(), |v| format!("{v:>7.2}"));
        let rms = cap.channel_rms(c);
        let rms_db = if rms > 1e-9 {
            20.0 * (rms as f64).log10()
        } else {
            -120.0
        };
        out.push_str(&format!("  ch{c}: {lufs} LUFS   rms {rms_db:>7.2} dBFS\n"));
    }
    if let (Some(a), Some(b)) = (lufs_at(0), lufs_at(1)) {
        out.push_str(&format!("  ch0-ch1 delta: {:+.2} LU\n", a - b));
    }
    Ok(out)
}

/// Phase-4 GATE 1 spike: capture the device's USB-Out for `secs` seconds in
/// normal mode (no playback) while the user plays their real guitar, and report
/// each input channel's peak/RMS in dBFS. Validates that the dry instrument
/// (USB-Out 3 → input channel index 2) is capturable for Tier-2 calibration.
pub fn probe_capture_input(secs: f32) -> Result<String, String> {
    // Ensure normal mode (re-amp OFF) so the rear instrument input flows.
    if let Ok(mut s) = Session::connect() {
        let _ = s.set_reamp_mode(false);
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    let cap = audio::capture_input(secs, 48_000)?;
    let mut out = format!(
        "captured {secs:.1}s across {} input channels:\n",
        cap.channels
    );
    for ch in 0..cap.channels {
        let dbfs = |v: f32| {
            if v > 1e-9 {
                20.0 * v.log10()
            } else {
                f32::NEG_INFINITY
            }
        };
        let peak = cap.channel_peak(ch);
        let rms_dbfs = dbfs(cap.channel_rms(ch));
        // Both metrics on the IDENTICAL samples: (LUFS − RMS) is the K-weighting
        // boost; comparing it across guitars cancels playing level → brightness.
        let lufs = lufs::measure_mono(&cap.channel(ch), cap.sample_rate)
            .map(|l| l.integrated_lufs)
            .unwrap_or(f64::NEG_INFINITY);
        let boost = if lufs.is_finite() && rms_dbfs.is_finite() {
            format!("  K-boost {:+.2}", lufs - rms_dbfs as f64)
        } else {
            String::new()
        };
        let note = match ch {
            0 | 1 => " (USB-Out 1/2 — processed)",
            2 => " (USB-Out 3 — DRY INSTRUMENT)",
            3 => " (USB-Out 4 — dry mic/line)",
            _ => "",
        };
        out += &format!(
            "  ch{ch}: peak {:+.1} dBFS  rms {:+.1} dBFS  lufs {:+.1}{boost}{note}\n",
            dbfs(peak),
            rms_dbfs,
            lufs,
        );
    }
    Ok(out)
}

/// Phase-4 GATE 2 spike: map the re-amp inject's input→output transfer by sweeping
/// the injected stimulus amplitude (same `presetLevel`) and measuring captured
/// loudness at each. Each −6 dB amplitude step should drop output ~6 LU IF the
/// path is linear there. A clean preset that stays linear at low drive but
/// compresses near the top = normal amp behavior (Tier-2 valid). A path that's
/// flat at ALL levels = the tap/input is normalized (Tier-2 premise broken).
/// Stimulus via `TMP_LEVELLER_STIMULUS`. Load `slot` = a CLEAN preset.
pub fn probe_reamp_agc_test(slot: u32) -> Result<String, String> {
    let stim_path = std::env::var("TMP_LEVELLER_STIMULUS")
        .map_err(|_| "set TMP_LEVELLER_STIMULUS to the stimulus WAV".to_string())?;
    let base = read_stimulus_48k(&stim_path)?;
    let base_peak = base.iter().fold(0.0f32, |m, &x| m.max(x.abs()));

    // Load the preset in its own connection, settle.
    {
        let mut s = Session::connect()?;
        s.load_preset(slot)?;
        std::thread::sleep(std::time::Duration::from_millis(1200));
    }
    std::thread::sleep(std::time::Duration::from_millis(400));

    // Measure the injected stimulus (scaled) at a fixed presetLevel; fresh conn.
    let measure = |scale: f32| -> Result<f64, String> {
        let stim: Vec<f32> = base.iter().map(|x| x * scale).collect();
        let mut s = Session::connect()?;
        s.set_preset_level(0.5)?;
        std::thread::sleep(std::time::Duration::from_millis(300));
        let _ = s.set_reamp_mode(true)?;
        std::thread::sleep(std::time::Duration::from_millis(500));
        let cap = audio::reamp_capture(&stim, 48_000, 800);
        let _ = s.set_reamp_mode(false);
        let cap = cap?;
        let (ch, _) = cap.loudest_channel();
        let m = lufs::measure_mono(&cap.channel(ch), cap.sample_rate)?.integrated_lufs;
        if !m.is_finite() {
            return Err("no signal captured (re-amp may not have routed)".to_string());
        }
        Ok(m)
    };

    // Sweep amplitude in −6 dB steps: 1.0, 0.5, 0.25, 0.125 of the base peak.
    let scales = [1.0f32, 0.5, 0.25, 0.125];
    let mut out =
        format!("slot {slot} re-amp inject sweep (base peak {base_peak:.3}, presetLevel 0.5):\n");
    let mut prev: Option<f64> = None;
    let mut max_step_drop = 0.0f64; // most negative adjacent Δ (steepest = most linear)
    for sc in scales {
        let l = measure(sc)?;
        let step = prev.map(|p| l - p);
        out += &format!(
            "  peak {:.4}  →  {:.2} LUFS{}\n",
            base_peak * sc,
            l,
            step.map(|d| format!("   (Δ {d:+.2} LU vs prev −6 dB step)"))
                .unwrap_or_default(),
        );
        if let Some(d) = step {
            if d < max_step_drop {
                max_step_drop = d;
            }
        }
        prev = Some(l);
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
    let verdict = if max_step_drop < -3.0 {
        "LINEAR somewhere (a −6 dB step dropped >3 LU) → stimulus amplitude drives the chain; \
         Tier-2 calibration is valid ✓"
    } else if max_step_drop > -1.0 {
        "FLAT at every level (no −6 dB step dropped >1 LU) → the re-amp inject is normalized; \
         Tier-2 premise BROKEN ✗"
    } else {
        "WEAK response at all levels → inject amplitude barely matters here; Tier-2 value is \
         marginal — inspect the sweep before building calibration"
    };
    out += &format!("  steepest −6 dB step: {max_step_drop:+.2} LU\n  {verdict}\n");
    Ok(out)
}
