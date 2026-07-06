//! Probe entry points: per-scene amp-knob leveling measurement + one-shot level + diagnostics.

use super::level::load_and_filter_amp_candidates;
use super::scene_jobs::build_scene_jobs;
use super::scene_jobs::prepass_scene_docs;
use super::stimulus::probe_stimulus_path;
use super::stimulus::read_stimulus_calibrated;
use crate::audio;
use crate::leveller;
use crate::read_preset_scenes_fresh;
use crate::scenes;
use crate::session;
use crate::session::Session;

// Isolated per-scene re-amp measurement (the proven `measure_knob_at` shape, but
// with explicit control over scene-edit). Loads the preset in its OWN connection
// (revert + latch rule), then a fresh connection: load scene → optional scene-edit →
// set the knob → engage → capture → measure the loudest channel's integrated LUFS.
// Fresh stream per call (NOT the BatchedLive shared stream, whose windowed reads
// mis-measured scenes — Klon read -6.96 LUFS when the knob's true range is -40..-14).
// NO save.
#[allow(clippy::too_many_arguments)] // cohesive per-scene measurement params; a struct would only add ceremony
fn measure_scene_knob_isolated(
    list_index: u32,
    scene_slot: u32,
    group_id: &str,
    node_id: &str,
    param: &str,
    value: f32,
    scene_edit: bool,
    stim: &[f32],
) -> Result<f64, String> {
    use std::time::Duration;
    {
        let mut s = Session::connect()?;
        s.load_preset(list_index)?;
        std::thread::sleep(Duration::from_millis(1200));
    }
    std::thread::sleep(Duration::from_millis(400));
    let mut s = Session::connect()?;
    s.load_scene(scene_slot)?;
    std::thread::sleep(Duration::from_millis(300));
    if scene_edit {
        s.set_node_scene_edit(group_id, node_id, true)?;
        std::thread::sleep(Duration::from_millis(300));
    }
    s.change_parameter(group_id, node_id, param, value)?;
    std::thread::sleep(Duration::from_millis(300));
    s.set_reamp_mode(true)?;
    std::thread::sleep(Duration::from_millis(500));
    let cap = audio::reamp_capture(stim, 48_000, 800);
    let _ = s.set_reamp_mode(false);
    leveller::loudest_lufs(cap)
}

/// READ-ONLY outcome proof: measure each scene's ACTUAL captured loudness from the
/// SAVED preset state — load preset, load scene, engage, measure (NO knob change). The
/// honest "did leveling land?" check, independent of the leveling math.
pub fn probe_measure_scene_levels(list_index: u32, topology_id: String) -> Result<String, String> {
    use std::time::Duration;
    let stim_path = probe_stimulus_path(&topology_id)?;
    let cal = std::env::var("TMP_LEVELLER_CAL_LUFS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    let stim = read_stimulus_calibrated(&stim_path, cal)?;
    let scenes = read_preset_scenes_fresh(list_index)?;
    let mut slots: Vec<(u32, String)> = scenes
        .scenes
        .iter()
        .enumerate()
        .map(|(i, n)| (i as u32, n.clone()))
        .collect();
    slots.push((session::BASE_SCENE_SLOT, "Base".to_string()));

    let measure = |scene_slot: u32| -> Result<f64, String> {
        {
            let mut s = Session::connect()?;
            s.load_preset(list_index)?;
            std::thread::sleep(Duration::from_millis(1200));
        }
        std::thread::sleep(Duration::from_millis(400));
        let mut s = Session::connect()?;
        s.load_scene(scene_slot)?;
        std::thread::sleep(Duration::from_millis(400));
        s.set_reamp_mode(true)?;
        std::thread::sleep(Duration::from_millis(500));
        let cap = audio::reamp_capture(&stim, 48_000, 800);
        let _ = s.set_reamp_mode(false);
        leveller::loudest_lufs(cap)
    };

    let mut out = format!(
        "=== preset {:03} saved-state scene loudness ===\n",
        list_index + 1
    );
    for (slot, name) in slots {
        match measure(slot) {
            Ok(m) => out += &format!("scene {slot:>2} {name:<18} {m:.2} LUFS\n"),
            Err(e) => out += &format!("scene {slot:>2} {name:<18} [FAIL: {e}]\n"),
        }
    }
    Ok(out)
}

/// HW driver for the per-scene leveling goal: level a preset's Base + every FS scene
/// to a target, with optional per-scene-NAME target overrides. Mirrors the app's run:
///   • Base → `presetLevel` (one-shot `level_preset`), done FIRST so the FS scenes
///     measure under the final preset gain.
///   • FS scenes → amp `outputLevel` via the BatchedLive runner, GROUPED by resolved
///     target (so scenes wanting different targets each get their own batched pass).
/// Reuses the 1.8.45-safe block discovery (`load_then_discover_blocks`). Stimulus comes
/// from `topology_id`; `save` persists. Returns a human-readable per-scene report.
pub fn probe_level_preset_scenes(
    list_index: u32,
    default_target: f64,
    topology_id: String,
    save: bool,
    overrides: Vec<(String, f64)>,
) -> Result<String, String> {
    if !default_target.is_finite() {
        return Err("default target LUFS must be finite".to_string());
    }
    let stim_path = probe_stimulus_path(&topology_id)?;
    let cal = std::env::var("TMP_LEVELLER_CAL_LUFS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    let stim = read_stimulus_calibrated(&stim_path, cal)?;

    // Resolve a per-scene target by NAME (case-insensitive), else the default.
    let resolve = |name: &str| -> f64 {
        overrides
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, t)| *t)
            .unwrap_or(default_target)
    };

    let mut out = String::new();
    out += &format!(
        "=== preset {:03} (list index {list_index}) · default {:.1} LU · save={save} ===\n",
        list_index + 1,
        default_target,
    );

    // 1) scene names (field-8 read; 1.8.45-safe).
    let scenes = read_preset_scenes_fresh(list_index)?;
    out += &format!("scenes ({}): {:?}\n", scenes.scenes.len(), scenes.scenes);

    // 2) Base → presetLevel FIRST (a "base"/"BASE" override targets it).
    let base_target = overrides
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("base"))
        .map(|(_, t)| *t)
        .unwrap_or(default_target);
    let opts = leveller::LevelOptions {
        save,
        verify: true,
        ..Default::default()
    };
    let br = leveller::level_preset(list_index, &stim, base_target, opts, &[], || false)?;
    out += &format!(
        "Base  → target {:.1}  presetLevel={:.4}  verify {:.2} LU (err {:+.2}){}{}\n",
        base_target,
        br.final_level,
        br.verify_lufs.unwrap_or(f64::NAN),
        br.verify_lufs.map(|m| m - base_target).unwrap_or(f64::NAN),
        if br.clamped { "  [CLAMPED]" } else { "" },
        if br.saved { "  [SAVED]" } else { "" },
    );

    if scenes.scenes.is_empty() {
        out += "(no FS scenes)\n";
        return Ok(out);
    }

    // 3a) amp candidates via the 1.8.45-safe rich-session discovery.
    let candidates = load_and_filter_amp_candidates(list_index)?;
    if candidates.is_empty() {
        return Err("no amp outputLevel controls found — cannot scene-level".to_string());
    }
    out += &format!("amp candidates: {}\n", candidates.len());

    // 3b) ONE un-engaged pre-pass over every FS scene → pick each scene's active amp.
    let all_slots: Vec<u32> = (0..scenes.scenes.len() as u32).collect();
    let docs = prepass_scene_docs(list_index, &all_slots)?;

    // 3c) ONE-SHOT open-loop per scene on the active amp `outputLevel`. HW-verified:
    //     captured_LUFS = 20*log10(outputLevel) + C is LINEAR with ~25 LU authority, so
    //     measure ONCE at a reference level, solve C, set the exact level — no closed
    //     loop, no clamp-flailing. Scene-edit ON isolates the write to the scene overlay.
    //     Measurement is ISOLATED (fresh stream per point) because the BatchedLive shared
    //     stream mis-measured scenes (Klon read -6.96 when its true range is -40..-14).
    const SCENE_REF: f32 = 0.5;
    for slot in all_slots {
        let name = scenes.scenes[slot as usize].clone();
        let target = resolve(&name);
        // active amp for this scene (first un-bypassed amp outputLevel)
        let knob = match build_scene_jobs(&[slot], &candidates, &docs)
            .ok()
            .and_then(|j| j.into_iter().next())
            .and_then(|j| j.knobs.into_iter().next())
            .map(|kt| kt.knob)
        {
            Some(leveller::LevelKnob::Block {
                group_id,
                node_id,
                parameter_id,
                ..
            }) => (group_id, node_id, parameter_id),
            _ => {
                out += &format!("FS[{slot}] {name:<18} [SKIP: no active amp outputLevel]\n");
                continue;
            }
        };
        let (g, n, p) = knob;
        // measure once at the reference outputLevel (scene-edit on for isolation)
        let measured =
            match measure_scene_knob_isolated(list_index, slot, &g, &n, &p, SCENE_REF, true, &stim)
            {
                Ok(m) => m,
                Err(e) => {
                    out += &format!("FS[{slot}] {name:<18} [FAIL measure: {e}]\n");
                    continue;
                }
            };
        let c = measured - 20.0 * (SCENE_REF as f64).log10();
        let (final_level, clamped, predicted) = leveller::solve_level(c, target);
        // apply + save: own load connection, then a fresh set connection.
        if save {
            {
                let mut s = Session::connect()?;
                s.load_preset(list_index)?;
                std::thread::sleep(std::time::Duration::from_millis(1200));
            }
            std::thread::sleep(std::time::Duration::from_millis(400));
            let mut s = Session::connect()?;
            s.load_scene(slot)?;
            std::thread::sleep(std::time::Duration::from_millis(300));
            s.set_node_scene_edit(&g, &n, true)?;
            std::thread::sleep(std::time::Duration::from_millis(300));
            s.change_parameter(&g, &n, &p, final_level)?;
            std::thread::sleep(std::time::Duration::from_millis(300));
            s.save_current_preset(list_index)?;
        }
        // verify: re-measure isolated at the solved level (validates the model end-to-end).
        let verify =
            measure_scene_knob_isolated(list_index, slot, &g, &n, &p, final_level, true, &stim)
                .ok();
        out += &format!(
            "FS[{slot}] {name:<18} target {:.1}  C={:.2}  outputLevel={:.4}  predicted {:.2}  verify {:.2} (err {:+.2}){}{}\n",
            target,
            c,
            final_level,
            predicted,
            verify.unwrap_or(f64::NAN),
            verify.map(|v| v - target).unwrap_or(f64::NAN),
            if clamped { "  [CLAMPED]" } else { "" },
            if save { "  [SAVED]" } else { "" },
        );
    }
    Ok(out)
}

/// READ-ONLY diagnostic for the scene-leveling amp-pick: for a preset, harvest each
/// scene's live doc (un-engaged pre-pass) and print, per scene, every level-type
/// control (model/node/param = value) plus each amp candidate's live BYPASS state.
/// Reveals which amp `build_scene_jobs` would pick (first un-bypassed) and whether
/// the scene's loudness is governed by that amp or by something else (the other amp,
/// a post-amp boost, a delay/IR `level`). No writes, no re-amp.
pub fn probe_scene_amp_diag(list_index: u32) -> Result<String, String> {
    let scenes = read_preset_scenes_fresh(list_index)?;
    let amp_cands = load_and_filter_amp_candidates(list_index)?;
    let mut all_slots: Vec<u32> = (0..scenes.scenes.len() as u32).collect();
    all_slots.push(session::BASE_SCENE_SLOT);
    let docs = prepass_scene_docs(list_index, &all_slots)?;

    let mut out = format!(
        "preset {:03} · scenes {:?}\namp candidates: {:?}\n",
        list_index + 1,
        scenes.scenes,
        amp_cands
            .iter()
            .map(|c| format!("{}/{} outputLevel={:.3}", c.group_id, c.node_id, c.value))
            .collect::<Vec<_>>(),
    );
    for (slot, doc) in &docs {
        let name = if *slot >= session::BASE_SCENE_SLOT {
            "Base".to_string()
        } else {
            scenes
                .scenes
                .get(*slot as usize)
                .cloned()
                .unwrap_or_default()
        };
        out += &format!("\n── scene {slot} ({name}) ──\n");
        let Some(d) = doc else {
            out += "  (no doc harvested)\n";
            continue;
        };
        out += "  amp bypass:\n";
        for c in &amp_cands {
            let bypass = scenes::block_bypass_in_live_graph(d, &c.group_id, &c.node_id);
            out += &format!("    {}/{} bypass={bypass:?}\n", c.group_id, c.node_id);
        }
        out += "  level controls in this scene's doc:\n";
        for b in session::extract_level_blocks(d) {
            out += &format!(
                "    {}/{} [{}] {} = {:.4}\n",
                b.group_id, b.node_id, b.model_id, b.parameter_id, b.value
            );
        }
    }
    Ok(out)
}

/// DECISIVE diagnostic: does the active amp's `outputLevel` actually move ONE scene's
/// loudness, and does scene-edit ISOLATE the write? For `scene_slot`, picks the active
/// amp (un-bypassed) and measures captured LUFS at a LOW and HIGH outputLevel under two
/// write modes:
///   • GLOBAL  — `change_parameter` only (writes the base/global value).
///   • SCENE   — `set_node_scene_edit(true)` then `change_parameter` (scene overlay).
/// Reads back the BASE scene's outputLevel after the SCENE write to confirm isolation.
/// Interpretation: GLOBAL Δ large but SCENE Δ ≈ 0 ⇒ scene-edit not isolating on 1.8.45;
/// both Δ ≈ 0 ⇒ genuine no authority (effect-dominated); both large ⇒ authority fine,
/// the clamp is the loop's noisy-slope bug. NO SAVE (reloads the stored preset each step).
pub fn probe_scene_knob_authority(
    list_index: u32,
    scene_slot: u32,
    topology_id: String,
) -> Result<String, String> {
    let stim_path = probe_stimulus_path(&topology_id)?;
    let cal = std::env::var("TMP_LEVELLER_CAL_LUFS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    let stim = read_stimulus_calibrated(&stim_path, cal)?;

    // Pick the active amp for this scene (same logic as build_scene_jobs).
    let candidates = load_and_filter_amp_candidates(list_index)?;
    let docs = prepass_scene_docs(list_index, &[scene_slot])?;
    let job = build_scene_jobs(&[scene_slot], &candidates, &docs)?;
    let knob = job
        .into_iter()
        .next()
        .and_then(|j| j.knobs.into_iter().next())
        .ok_or("no scene job built")?
        .knob;
    let (group_id, node_id, param) = match &knob {
        leveller::LevelKnob::Block {
            group_id,
            node_id,
            parameter_id,
            ..
        } => (group_id.clone(), node_id.clone(), parameter_id.clone()),
        _ => return Err("expected a Block knob".to_string()),
    };

    // measure the scene's loudness after setting outputLevel=v under a write mode
    // (the shared isolated-measure helper; global write = no scene-edit).
    let measure = |scene_edit: bool, v: f32| -> Result<f64, String> {
        measure_scene_knob_isolated(
            list_index, scene_slot, &group_id, &node_id, &param, v, scene_edit, &stim,
        )
    };

    let lo = 0.05f32;
    let hi = 0.95f32;
    let g_lo = measure(false, lo)?;
    let g_hi = measure(false, hi)?;
    let s_lo = measure(true, lo)?;
    let s_hi = measure(true, hi)?;

    Ok(format!(
        "scene {scene_slot} active amp {group_id}/{node_id} {param}\n\
         GLOBAL write: outputLevel {lo} → {g_lo:.2} LUFS | {hi} → {g_hi:.2} LUFS | Δ = {:.2} LU\n\
         SCENE  write: outputLevel {lo} → {s_lo:.2} LUFS | {hi} → {s_hi:.2} LUFS | Δ = {:.2} LU\n\
         interpretation: global Δ large + scene Δ≈0 ⇒ scene-edit NOT isolating; \
         both Δ≈0 ⇒ no authority; both large ⇒ authority OK (clamp = loop bug)\n",
        g_hi - g_lo,
        s_hi - s_lo,
    ))
}

/// READ-ONLY mute-isolation diagnostic (`probe --mute-floor`) for the rebalance flow: for a
/// 2-amp MERGED scene, report the combined output, the both-lanes-muted floor, and each lane
/// SOLO with its margin above the floor. A small margin ⇒ `outputLevel`=0 isn't deep silence
/// and the muted lane bleeds into the solo, so the equal-solo balance is only approximate.
/// NO SAVE. Validates the `verify_by_ear` heuristic `level_scenes_rebalance` applies.
pub fn probe_mute_floor(
    list_index: u32,
    scene_slot: u32,
    topology_id: String,
) -> Result<String, String> {
    let stim_path = probe_stimulus_path(&topology_id)?;
    let cal = std::env::var("TMP_LEVELLER_CAL_LUFS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    let stim = read_stimulus_calibrated(&stim_path, cal)?;

    // Build the scene job the same way the leveler does, then take its first two amp lanes.
    let candidates = load_and_filter_amp_candidates(list_index)?;
    let docs = prepass_scene_docs(list_index, &[scene_slot])?;
    let job = build_scene_jobs(&[scene_slot], &candidates, &docs)?
        .into_iter()
        .next()
        .ok_or("no scene job built")?;
    if job.knobs.len() < 2 {
        return Err(format!(
            "scene {scene_slot} has {} amp knob(s) — mute-floor needs a 2-amp merged parallel scene",
            job.knobs.len()
        ));
    }
    let a = &job.knobs[0];
    let b = &job.knobs[1];
    leveller::mute_floor_report(list_index, &a.knob, a.current, &b.knob, b.current, &stim)
}
