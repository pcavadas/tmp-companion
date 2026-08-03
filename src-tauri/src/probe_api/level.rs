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

/// The one engaged/floor criterion the diagnostic arms share: finite chain audio
/// meaningfully above the stationary floor, with real dynamics (a floor read is
/// near-flat). NaN comparisons are false, so a failed measure reads "not engaged".
fn is_engaged(l: &lufs::Loudness) -> bool {
    l.integrated_lufs.is_finite() && l.integrated_lufs > -50.0 && l.spread_lu() > 0.5
}

/// DIAGNOSTIC (reamp-stuck investigation): PASSIVE re-amp state read — zero HID
/// traffic. Plays the stimulus into USB-In 3 and captures USB-Out WITHOUT sending
/// any device command: chain audio in the capture (finite LUFS, real spread) means
/// re-amp is ENGAGED right now; silence/floor means it is OFF. Prints raw numbers;
/// the caller judges against a calibrated known-ON/known-OFF pair.
pub fn probe_reamp_state(topology_id: &str) -> Result<String, String> {
    let stim_path = probe_stimulus_path(topology_id)?;
    let mut stim = read_stimulus_48k(&stim_path)?;
    // TMP_REAMP_STATE_SILENT=1: inject digital silence instead — discriminates
    // "my stimulus through the chain" (engaged) from the chain's own hiss floor
    // (normal mode): a reading that PERSISTS under silent inject is device hiss.
    if std::env::var("TMP_REAMP_STATE_SILENT").is_ok() {
        stim.fill(0.0);
    }
    let cap = audio::reamp_capture(&stim, 48_000, 800)?;
    let (ch, _) = cap.loudest_channel();
    let samples = cap.channel(ch);
    let peak = cap.channel_peak(ch);
    let peak_db = if peak > 1e-9 {
        20.0 * (peak as f64).log10()
    } else {
        -120.0
    };
    match lufs::measure_mono(&samples, cap.sample_rate) {
        Ok(l) => Ok(format!(
            "reamp-state: loudest ch{ch}  {:.2} LUFS  spread {:.2} LU  peak {:.1} dBFS  => {}",
            l.integrated_lufs,
            l.spread_lu(),
            peak_db,
            if is_engaged(&l) {
                "ENGAGED (stimulus audible through chain)"
            } else {
                "off/floor (no chain audio)"
            }
        )),
        Err(e) => Ok(format!(
            "reamp-state: loudest ch{ch}  unmeasurable ({e})  peak {peak_db:.1} dBFS  => OFF (silent capture)"
        )),
    }
}

/// DIAGNOSTIC (reamp-stuck investigation): engage → idle `idle_ms` (optionally
/// heartbeating every 250 ms) → OFF on the SAME session → report the OFF's echo.
/// No audio at all. Discriminates the drop mechanisms: if OFF lands at idle 0 but
/// drops after a capture-length idle → session lapse; if heartbeats rescue it →
/// keep-alive fix shape; if OFF drops even at idle 0 → second-toggle-per-session
/// is inherently unreliable. Judge the REAL state with `--reamp-state` after.
pub fn probe_reamp_toggle_test(idle_ms: u64, heartbeat: bool) -> Result<String, String> {
    let mut s = Session::connect_lean()?;
    let on_echo = s.set_reamp_mode(true)?;
    if heartbeat {
        let mut left = idle_ms;
        while left > 0 {
            let step = left.min(250);
            std::thread::sleep(std::time::Duration::from_millis(step));
            let _ = s.heartbeat();
            left -= step;
        }
    } else {
        std::thread::sleep(std::time::Duration::from_millis(idle_ms));
    }
    let off_echo = s.set_reamp_mode(false)?;
    Ok(format!(
        "toggle-test: idle={idle_ms}ms hb={heartbeat}  ON echo={on_echo:?}  OFF echo={off_echo:?}"
    ))
}

/// DIAGNOSTIC (re-test of the "re-amp engages reliably only ONCE per connection"
/// gotcha): run `cycles` × (engage → capture → disengage) on ONE session, judging
/// each engage by the only trustworthy signal — a finite captured loudness (the
/// `ReAmpModeChanged` echo is documented flaky). Each cycle's engage+capture is
/// PRODUCTION-IDENTICAL (`engage_measure_disengage`'s shape: quiet engage,
/// `SETTLE_AFTER_REAMP_MS`, fully idle capture — a v1 of this arm that
/// heartbeat-paced the capture and pre-toggled a heartbeat read FLOOR from cycle 1
/// while the production path measured perfectly back-to-back, so extra HID traffic
/// around the engage is itself inject-hostile). Only the post-capture OFF gets a
/// heartbeat burst first (the post-idle command-drop rescue), and inter-cycle gaps
/// stay under the idle cliff. NB an all-cycles-ENGAGED result transfers to
/// production ONLY for this exact quiet shape; a later-cycle floor confirms the
/// once-per-connection rule under the best-known-safe pacing.
pub fn probe_reamp_multi_engage(topology_id: &str, cycles: u32) -> Result<String, String> {
    let stim_path = probe_stimulus_path(topology_id)?;
    let stim = read_stimulus_48k(&stim_path)?;
    // Engagement proof only — a short slice keeps per-cycle idle small; loudness
    // comparability across cycles matters, absolute LUFS does not.
    let stim = &stim[..stim.len().min(3 * 48_000)];

    let run = || -> Result<String, String> {
        // Fresh preset load in its OWN connection before the multi-engage session —
        // the arm-per-load hypothesis: every working production engage follows a
        // fresh `load_preset` on a prior connection (v4 without this floored from
        // cycle 1 even with the production setter+engage pair, while the setter
        // observably landed — the floor scaled with it). If the load re-arms
        // exactly one engage, cycle 1 ENGAGES and later cycles floor.
        {
            // The verdict below compares loudness ACROSS cycles, so it only means
            // anything on the known scenario preset — a unit without the seeded
            // fixture at 400 (empty slot, or some unrelated user preset) could
            // invert it. Name-confirm in the same list space before loading.
            super::slot_write::confirm_slot_name(400, "E2E Reference")?;
            let mut s = Session::connect_lean()?;
            s.load_preset(400)?;
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
        let mut s = Session::connect_lean()?;
        let mut report = String::new();
        let mut engaged_cycles = 0u32;
        for cycle in 1..=cycles {
            // Each cycle re-plays `arm_measurement`'s EXACT pre-engage triple —
            // recall base → set level → settle → engage. Nothing weaker armed the
            // engage on this arm's session shape: a bare engage (v2), heartbeat+
            // engage (v1), recall+engage (v3), and setter+engage (v4/v5, where the
            // floor SCALED with the setter, proving the write landed) ALL read
            // floor from cycle 1 while `--levelpreset` measured perfectly in the
            // same minutes. The 0.5 ref is a working-copy write, never saved.
            s.load_scene(session::BASE_SCENE_SLOT)?;
            std::thread::sleep(std::time::Duration::from_millis(300));
            s.set_preset_level(0.5)?;
            std::thread::sleep(std::time::Duration::from_millis(300));
            s.set_reamp_mode(true)?;
            std::thread::sleep(std::time::Duration::from_millis(500));
            // Production-identical capture: the session idles for the full drain.
            let cap = audio::reamp_capture(stim, 48_000, 800)?;

            let (ch, _) = cap.loudest_channel();
            let verdict = match lufs::measure_mono(&cap.channel(ch), cap.sample_rate) {
                Ok(l) if is_engaged(&l) => {
                    engaged_cycles += 1;
                    format!(
                        "ENGAGED  {:.2} LUFS  spread {:.2} LU",
                        l.integrated_lufs,
                        l.spread_lu()
                    )
                }
                Ok(l) => format!(
                    "floor    {:.2} LUFS  spread {:.2} LU",
                    l.integrated_lufs,
                    l.spread_lu()
                ),
                Err(e) => format!("OFF      silent capture ({e})"),
            };
            report.push_str(&format!("cycle {cycle}/{cycles}: {verdict}\n"));

            // The OFF follows a capture-length idle, which drops bare commands —
            // burst heartbeats first (the documented post-idle rescue), then
            // disengage and give the DSP a beat before the next cycle's engage.
            for _ in 0..3 {
                let _ = s.heartbeat();
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            s.set_reamp_mode(false)?;
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        report.push_str(&format!(
            "verdict: {engaged_cycles}/{cycles} cycles engaged on ONE connection — {}\n",
            if engaged_cycles == cycles {
                "re-engage WORKED under this exact quiet shape (production would have to \
                 adopt it verbatim to inherit the result)"
            } else if engaged_cycles <= 1 {
                "the once-per-connection rule STANDS (quiet production-shaped cycles do \
                 not rescue re-engage)"
            } else {
                "PARTIAL — re-engage is unreliable, not impossible; keep \
                 fresh-connect-per-engage"
            }
        ));
        Ok(report)
    };
    // Run-end backstop, success or failure: any early `?` above can exit with
    // re-amp engaged, the documented input-muted strand.
    let out = run();
    leveller::reamp_off_guaranteed("probe --reamp-multi-engage");
    out
}

/// Measure the currently selected preset/scene through re-amp without changing
/// preset level or block parameters. Optional `slot` loads a preset first in its
/// own connection; optional `scene_slot` recalls a scene before capture. No save.
///
/// NOT floor-guarded (deliberately — this is the repro instrumentation seam, one
/// capture with everything derived from it): a failed inject reads as the device's
/// stationary output floor, so the headline is stamped FLOOR/SILENT when the capture
/// fails `probe_reamp_state`'s engaged criterion instead of being retried.
pub fn probe_measure_current_lufs(
    topology_id: &str,
    slot: Option<u32>,
    scene_slot: Option<u32>,
    calibration_lufs: Option<f32>,
) -> Result<String, String> {
    let stim_path = probe_stimulus_path(topology_id)?;
    let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;
    // Repro instrumentation: ONE capture, everything derived from it — the headline
    // (loudest channel, matching production's pick) PLUS every channel's loudness,
    // so the argmax (broadband RMS across ALL channels incl. the ch2 dry DI tap)
    // is observable per measurement. No floor-guard retry (diagnostic seam).
    let cap = leveller::capture_asis_full(slot, scene_slot, &stim)?;
    let (win, _) = cap.loudest_channel();
    let loud = crate::lufs::measure_mono(&cap.channel(win), cap.sample_rate)
        .map_err(|e| format!("loudest-channel measure failed: {e}"))?;
    let mut per_channel = String::new();
    for c in 0..cap.channels {
        let rms = cap.channel_rms(c);
        let line = match crate::lufs::measure_mono(&cap.channel(c), cap.sample_rate) {
            Ok(l) if l.integrated_lufs.is_finite() => format!(
                "  ch{c}: lufs={:.3} stm={:.3} rms_dbfs={:.1}{}",
                l.integrated_lufs,
                l.short_term_max_lufs,
                20.0 * f32::max(rms, 1e-9).log10(),
                if c == win { "  <-- argmax winner" } else { "" }
            ),
            _ => format!(
                "  ch{c}: silent/unmeasurable rms_dbfs={:.1}{}",
                20.0 * f32::max(rms, 1e-9).log10(),
                if c == win { "  <-- argmax winner" } else { "" }
            ),
        };
        per_channel.push_str(&line);
        per_channel.push('\n');
    }
    // No floor-guard retry on this diagnostic seam, so a silent/failed inject WOULD
    // print the device's stationary floor as if it were a measurement — stamp the
    // headline with `probe_reamp_state`'s engaged/floor criterion instead.
    let verdict = if is_engaged(&loud) {
        ""
    } else {
        "  << FLOOR/SILENT — not a valid measurement (failed inject?)"
    };
    Ok(format!(
        "slot={} topology={topology_id} scene={} integrated_lufs={:.3} short_term_max_lufs={:.3}{verdict}\n{per_channel}",
        slot.map(|s| s.to_string())
            .unwrap_or_else(|| "current".to_string()),
        scene_slot
            .map(|s| s.to_string())
            .unwrap_or_else(|| "current".to_string()),
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

    let mut opts = leveller::LevelOptions {
        save,
        verify,
        ..Default::default()
    };
    // A saving run must re-stamp the preset's original `lastLoadedScene` (the base-context
    // measurement leaves base active at save time); a dry run never saves, so skip the read.
    if save {
        opts.restore_scene =
            crate::read_saved_preset(slot).and_then(|doc| crate::last_loaded_scene(&doc));
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    }
    // probe = raw benchmark behavior: no idempotency skip, always measure+apply+save.
    let result = leveller::level_preset(slot, &stim, target_lufs, opts, &[], None, || false);
    // Run-end backstop, success or failure (see `reamp_off_guaranteed`: the
    // device drops an in-session OFF sent after ~1 s of idle — every capture).
    leveller::reamp_off_guaranteed("probe --levelpreset");
    let r = result?;

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

/// E0: `probe --verify-fresh-load <listIdx>` — exercises `leveller::ensure_fresh_load`
/// end-to-end against a FAKE registry entry built from the slot's OWN current field-8
/// `presetLevel` (a non-destructive read; nothing is written). A healthy device should
/// harvest a match on the very first load re-issue, so this doubles as a fresh-equality
/// invariant check and a timing sample of the barrier's rich-session cost. Prints
/// PASS/FAIL plus the witness and elapsed time; NON-DESTRUCTIVE (loads only, no set/save).
pub fn probe_verify_fresh_load(list_index: u32) -> Result<String, String> {
    let preset = crate::read_saved_preset(list_index)
        .ok_or_else(|| format!("slot {list_index}: could not read the saved preset (field-8)"))?;
    let level = crate::audiograph::preset_level(&preset)
        .ok_or_else(|| format!("slot {list_index}: saved preset has no presetLevel"))?
        as f32;
    leveller::register_slot_save(list_index, leveller::SaveWitness::PresetLevel(level));
    let t0 = std::time::Instant::now();
    let result = leveller::ensure_fresh_load(list_index, &mut || false);
    let elapsed = t0.elapsed();
    let mut out = format!(
        "--verify-fresh-load slot={list_index} witness=PresetLevel({level:.4}) elapsed={:.2}s\n",
        elapsed.as_secs_f64()
    );
    match result {
        Ok(()) => {
            out += "PASS: ensure_fresh_load returned Ok. A sub-second elapsed means the \
                    harvest matched on the first load (or the registry/time-gate fast path \
                    fired — check `elapsed` against the ~1-2s a rich session normally costs \
                    to tell which); several seconds means it retried before matching.\n";
        }
        Err(e) => {
            out += &format!("FAIL: ensure_fresh_load returned Err({e})\n");
        }
    }
    Ok(out)
}
