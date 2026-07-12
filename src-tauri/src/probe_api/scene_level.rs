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
    let (docs, _) = prepass_scene_docs(list_index, &all_slots)?;

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
    let (docs, _) = prepass_scene_docs(list_index, &all_slots)?;

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
    let (docs, _) = prepass_scene_docs(list_index, &[scene_slot])?;
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
    let (docs, _) = prepass_scene_docs(list_index, &[scene_slot])?;
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

/// Bisection probe (`probe --bisect-scene`): run the PROVEN-POTENT isolated
/// write-measure shape at an exact value, optionally prefixed by a jointk-style
/// as-is capture (`asis`) and/or suffixed by a same-connection save (`save`) —
/// the two elements the inert jointk path adds. Prints the measured LUFS per
/// stage so the poisoning element is identified directly on hardware.
#[allow(clippy::too_many_arguments)] // a HW-bisection CLI arm; a struct would only add ceremony
pub fn probe_bisect_scene(
    list_index: u32,
    scene_slot: u32,
    group_id: String,
    node_id: String,
    value: f32,
    with_asis: bool,
    with_save: bool,
    topology_id: String,
) -> Result<String, String> {
    use std::time::Duration;
    let stim_path = probe_stimulus_path(&topology_id)?;
    let stim = read_stimulus_calibrated(&stim_path, None)?;
    let mut out = format!(
        "=== bisect-scene idx {list_index} scene {scene_slot} {group_id}/{node_id} value {value} asis={with_asis} save={with_save} ===\n"
    );

    if with_asis {
        // jointk's leading shape: own load connection, then a write-LESS engage.
        {
            let mut s = Session::connect()?;
            s.load_preset(list_index)?;
            std::thread::sleep(Duration::from_millis(1200));
        }
        std::thread::sleep(Duration::from_millis(400));
        let asis = {
            let mut s = Session::connect()?;
            s.load_scene(scene_slot)?;
            std::thread::sleep(Duration::from_millis(300));
            leveller::engage_measure_disengage(&mut s, &stim)?.integrated_lufs
        };
        out += &format!("as-is capture: {asis:.2} LUFS\n");
        std::thread::sleep(Duration::from_millis(400));
    }

    // The potent isolated write-measure — inline so a same-connection save can follow.
    {
        let mut s = Session::connect()?;
        s.load_preset(list_index)?;
        std::thread::sleep(Duration::from_millis(1200));
    }
    std::thread::sleep(Duration::from_millis(400));
    // TMP_BISECT_EDIT_SETTLE overrides the scene-edit→write settle (default 600 =
    // set_knobs' SETTLE_AFTER_SCENE_EDIT_MS; the potent isolated fn uses 300).
    let edit_settle: u64 = std::env::var("TMP_BISECT_EDIT_SETTLE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);
    // TMP_BISECT_SCENE_SETTLE: load_scene→scene_edit gap (default 300) — anchor test.
    let scene_settle: u64 = std::env::var("TMP_BISECT_SCENE_SETTLE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let mut s = Session::connect()?;
    s.load_scene(scene_slot)?;
    std::thread::sleep(Duration::from_millis(scene_settle));
    s.set_node_scene_edit(&group_id, &node_id, true)?;
    std::thread::sleep(Duration::from_millis(edit_settle));
    s.change_parameter(&group_id, &node_id, "outputLevel", value)?;
    std::thread::sleep(Duration::from_millis(300));
    let written = leveller::engage_measure_disengage(&mut s, &stim)?.integrated_lufs;
    out += &format!("write({value}) settle={edit_settle}ms capture: {written:.2} LUFS\n");
    if with_save {
        // TMP_BISECT_SAVE_MODE: same (post-engage same connection, the old apply shape)
        // | fresh (drop + save on a NEW connection) | rewrite (re-write post-engage on
        // the same connection, then save — the re-assert shape minus load_scene).
        let mode = std::env::var("TMP_BISECT_SAVE_MODE").unwrap_or_else(|_| "same".into());
        match mode.as_str() {
            "fresh" => {
                drop(s);
                std::thread::sleep(Duration::from_millis(400));
                let mut s2 = Session::connect()?;
                s2.save_current_preset(list_index)?;
            }
            "rewrite" => {
                s.change_parameter(&group_id, &node_id, "outputLevel", value)?;
                std::thread::sleep(Duration::from_millis(300));
                s.save_current_preset(list_index)?;
            }
            _ => {
                s.save_current_preset(list_index)?;
            }
        }
        out += &format!("saved (mode={mode})\n");
    }
    Ok(out)
}

/// FAITHFUL UI-path repro (`probe --jointk-scenes`): drive the REAL
/// `level_scenes_oneshot` (jointk → `apply_levels` → `correct_iter`, incl. the
/// batch-end single save) exactly as the app's `level_scenes_apply_batched` does
/// when the wizard levels scene rows — scenes sharing a target batched into ONE
/// call, per-name target overrides. Unlike `probe_level_preset_scenes` (its own
/// simplified one-shot),
/// this exercises the production scene-leveling code path verbatim, and prints
/// each scene's job (knob picks + `current`) plus the full outcome — the
/// diagnostics the UI run doesn't log. Point it at a SCRATCH preset when
/// save=1: it persists like the real run.
pub fn probe_jointk_scenes(
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
    let resolve = |name: &str| -> f64 {
        overrides
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, t)| *t)
            .unwrap_or(default_target)
    };

    let scenes = read_preset_scenes_fresh(list_index)?;
    let candidates = load_and_filter_amp_candidates(list_index)?;
    let mut out = format!(
        "=== jointk-scenes repro · preset idx {list_index} · default {default_target} · save={save} ===\n\
         scenes ({}): {:?}\namp candidates: {}\n",
        scenes.scenes.len(),
        scenes.scenes,
        candidates.len(),
    );

    // TMP_JOINTK_ONLY=<sceneSlot>: restrict to one scene (fast bisection runs).
    let only: Option<u32> = std::env::var("TMP_JOINTK_ONLY")
        .ok()
        .and_then(|v| v.parse().ok());
    // The app's BATCHED shape: scenes sharing a resolved target go in ONE
    // prepass + runner call (one preset load, one batch-end save) — the wizard
    // groups adjacent same-target scene rows the same way. Group order follows
    // first appearance, so the device walks scenes in preset order per group.
    let mut groups: Vec<(f64, Vec<u32>)> = Vec::new();
    for (i, name) in scenes.scenes.iter().enumerate() {
        let slot = i as u32;
        if only.is_some_and(|o| o != slot) {
            continue;
        }
        let target = resolve(name);
        match groups.iter_mut().find(|(t, _)| *t == target) {
            Some((_, slots)) => slots.push(slot),
            None => groups.push((target, vec![slot])),
        }
    }
    for (target, slots) in groups {
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
        let (docs, restore_scene) = prepass_scene_docs(list_index, &slots)?;
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
        let jobs = match build_scene_jobs(&slots, &candidates, &docs) {
            Ok(j) => j,
            Err(e) => {
                out += &format!("group target {target:.1} slots {slots:?} [BUILD FAIL: {e}]\n");
                continue;
            }
        };
        let job_desc: Vec<String> = jobs
            .iter()
            .map(|j| match &j.skip {
                Some(r) => format!("FS[{}] SKIP: {r}", j.scene_slot),
                None => j
                    .knobs
                    .iter()
                    .map(|k| format!("FS[{}] {:?} current={:.3}", j.scene_slot, k.knob, k.current))
                    .collect::<Vec<_>>()
                    .join(" + "),
            })
            .collect();
        out += &format!(
            "group target {target:.1} slots {slots:?} restore_scene={restore_scene:?}\n  jobs=[{}]\n",
            job_desc.join("; ")
        );
        match leveller::level_scenes_oneshot(
            list_index,
            &jobs,
            &stim,
            target,
            save,
            restore_scene,
            |_, _| {},
            || false,
        ) {
            Ok(outcomes) => {
                for o in &outcomes {
                    out += &format!("   → {o:?}\n");
                }
            }
            Err(e) => out += &format!("   → RUN FAIL: {e}\n"),
        }
    }
    Ok(out)
}

/// HW semantics probe for the deferred single-save scene flow
/// (`probe --defer-scenes <listIndex> <groupId> <nodeId> <scene:value,…>`):
/// write several scenes' `outputLevel` UNSAVED (each on its own fresh connection,
/// mirroring the runner's write shape incl. a re-amp capture per write), then ONE
/// final save. The caller exports the slot afterwards and checks which writes
/// persisted. `TMP_DEFER_FINAL` picks the final-save shape:
///   `asis`   — save with the last-written scene active (does one save persist ALL
///              accumulated unsaved scene overlays?)
///   `return` — `loadScene(first written scene)` again, then save (does re-recalling
///              a scene REVERT its own unsaved write?)
///   `base`   — `loadScene(8)`, then save (the restore-original-state shape).
pub fn probe_defer_scenes(
    list_index: u32,
    group_id: String,
    node_id: String,
    writes: Vec<(u32, f32)>,
) -> Result<String, String> {
    use std::time::Duration;
    let stim_path = probe_stimulus_path("guitar-singlecoil")?;
    let stim = read_stimulus_calibrated(&stim_path, None)?;
    let final_mode = std::env::var("TMP_DEFER_FINAL").unwrap_or_else(|_| "asis".into());
    let mut out = format!(
        "=== defer-scenes idx {list_index} {group_id}/{node_id} writes={writes:?} final={final_mode} ===\n"
    );

    {
        let mut s = Session::connect()?;
        s.load_preset(list_index)?;
        std::thread::sleep(Duration::from_millis(1200));
    }
    std::thread::sleep(Duration::from_millis(400));

    for (scene, value) in &writes {
        let mut s = Session::connect()?;
        s.load_scene(*scene)?;
        std::thread::sleep(Duration::from_millis(150));
        s.set_node_scene_edit(&group_id, &node_id, true)?;
        std::thread::sleep(Duration::from_millis(300));
        s.change_parameter(&group_id, &node_id, "outputLevel", *value)?;
        std::thread::sleep(Duration::from_millis(300));
        let lufs = leveller::engage_measure_disengage(&mut s, &stim)?.integrated_lufs;
        out += &format!("scene {scene} write({value}) capture: {lufs:.2} LUFS (unsaved)\n");
        drop(s);
        std::thread::sleep(Duration::from_millis(400));
    }

    let mut s = Session::connect()?;
    match final_mode.as_str() {
        "return" => {
            let first = writes.first().map(|(sc, _)| *sc).unwrap_or(0);
            s.load_scene(first)?;
            std::thread::sleep(Duration::from_millis(300));
            out += &format!("final: re-recalled scene {first}, saving…\n");
        }
        "base" => {
            s.load_scene(session::BASE_SCENE_SLOT)?;
            std::thread::sleep(Duration::from_millis(300));
            out += "final: recalled base (8), saving…\n";
        }
        _ => out += "final: saving as-is (last scene active)…\n",
    }
    s.save_current_preset(list_index)?;
    out += "saved\n";
    Ok(out)
}
