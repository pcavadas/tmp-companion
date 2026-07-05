//! Probe entry points: scene enumeration (passive / load / full-live) + scene classify + one-shot level.

use crate::session::Session;
use crate::leveller;
use crate::proto;
use crate::session;
use super::level::load_and_filter_amp_candidates;
use super::scene_jobs::build_scene_jobs;
use super::scene_jobs::prepass_scene_docs;
use super::scene_jobs::structure_graph;
use super::slot_write::probe_connect_and_list;
use super::stimulus::probe_stimulus_path;
use super::stimulus::read_stimulus_calibrated;

/// NO-SAVE joint-k leveling run (`probe --level-scenes <listIdx> <target> <topology> [scene…]`):
/// the REAL `build_scene_jobs` → `level_scenes_oneshot` path with `save=false`, so it
/// measures/solves/applies (writing the amp `outputLevel`(s) to the live edit buffer) and
/// then RELOADS the stored preset to discard the edit — nothing is persisted. Validates
/// joint-k on hardware: for a parallel preset both lane amps are scaled by one factor and
/// the verify capture reports the achieved LUFS vs target. Ends with a guaranteed re-amp OFF.
pub fn probe_level_scenes_oneshot(
    list_index: u32,
    target_lufs: f64,
    topology_id: String,
    scene_slots: Vec<u32>,
    rebalance: bool,
) -> Result<String, String> {
    let scene_slots = if scene_slots.is_empty() {
        vec![session::BASE_SCENE_SLOT]
    } else {
        scene_slots
    };
    let stim_path = probe_stimulus_path(&topology_id)?;
    let cal = std::env::var("TMP_LEVELLER_CAL_LUFS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    let stim = read_stimulus_calibrated(&stim_path, cal)?;
    let candidates = load_and_filter_amp_candidates(list_index)?;
    let docs = prepass_scene_docs(list_index, &scene_slots)?;
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    let jobs = build_scene_jobs(&scene_slots, &candidates, &docs)?;
    // NO SAVE — restores the stored preset after measuring.
    let outcomes = if rebalance {
        leveller::level_scenes_rebalance(
            list_index,
            &jobs,
            &stim,
            target_lufs,
            false,
            |_, _| {},
            || false,
        )
    } else {
        leveller::level_scenes_oneshot(
            list_index,
            &jobs,
            &stim,
            target_lufs,
            false,
            |_, _| {},
            || false,
        )
    };
    // Guaranteed re-amp OFF regardless of outcome (a stranded re-amp mutes the input).
    let _ = Session::connect().and_then(|mut s| s.set_reamp_mode(false).map(|_| ()));
    let outcomes = outcomes?;
    let mut out = format!(
        "NO-SAVE leveling preset list_index={list_index} → target {target_lufs:.1} LUFS (topology {topology_id})\n"
    );
    for o in &outcomes {
        match &o.failure {
            Some(f) => out += &format!("  scene {} → FAILED/SKIP: {f}\n", o.scene_slot),
            None => {
                let lufs = o.final_lufs.unwrap_or(f64::NAN);
                out += &format!(
                    "  scene {} → achieved {lufs:.2} LUFS (err {:+.2})  level={:.4}{}\n",
                    o.scene_slot,
                    lufs - target_lufs,
                    o.final_level.unwrap_or(0.0),
                    if o.clamped { "  CLAMPED" } else { "" },
                );
            }
        }
    }
    Ok(out)
}

/// NON-DESTRUCTIVE classifier check (`probe --classify <listIdx> [scene…]`): load the
/// preset, harvest the pre-pass scene docs, and print how `build_scene_jobs` classifies
/// each scene's amp-knob set (routing → series last-amp / parallel joint-k / skip).
/// No re-amp, no parameter writes, no save — just loads + reads field-3. The headless
/// proof that the routing-aware classifier sees a real preset (e.g. 027 parallel) right.
pub fn probe_classify_scenes(list_index: u32, scene_slots: Vec<u32>) -> Result<String, String> {
    let scene_slots = if scene_slots.is_empty() {
        vec![session::BASE_SCENE_SLOT]
    } else {
        scene_slots
    };
    let candidates = load_and_filter_amp_candidates(list_index)?;
    let docs = prepass_scene_docs(list_index, &scene_slots)?;
    let template = structure_graph(&docs)
        .and_then(|g| g.template)
        .unwrap_or_else(|| "<unknown>".to_string());
    let mut out = format!(
        "preset list_index={list_index} template={template}\n  amp outputLevel candidates: {}\n",
        if candidates.is_empty() {
            "(none)".to_string()
        } else {
            candidates
                .iter()
                .map(|c| format!("{}/{}={:.3}", c.group_id, c.node_id, c.value))
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    let jobs = build_scene_jobs(&scene_slots, &candidates, &docs)?;
    for j in &jobs {
        if let Some(reason) = &j.skip {
            out += &format!("  scene {} → SKIP: {reason}\n", j.scene_slot);
            continue;
        }
        let knobs = j
            .knobs
            .iter()
            .map(|kt| match &kt.knob {
                leveller::LevelKnob::Block {
                    group_id, node_id, ..
                } => {
                    format!("{group_id}/{node_id}@{:.3}", kt.current)
                }
                leveller::LevelKnob::PresetLevel => "presetLevel".to_string(),
            })
            .collect::<Vec<_>>();
        let mode = if knobs.len() > 1 { "JOINT-K" } else { "single" };
        out += &format!("  scene {} → {mode} {:?}\n", j.scene_slot, knobs);
    }
    Ok(out)
}

/// Recall scene `scene_slot` (0-based `scenes[]` index; 8 = base) on the device's
/// CURRENT preset — the
/// headless runbook entry for HW-validating `loadScene` (PresetMessage 101).
/// Non-destructive: a live state change, persists nothing. Verify the recall by
/// diffing `--activegraph` bypass states before/after.
pub fn probe_load_scene(scene_slot: u32) -> Result<(), String> {
    Session::connect()?.load_scene(scene_slot)
}

/// Retained passive-scene re-validation probe: minimal connect, then a
/// NON-DESTRUCTIVE field-8 read per non-empty preset (connection_request re-arm,
/// one connection, zero LoadPreset). The UI no longer runs this eagerly; it uses
/// `read_preset_scenes` lazily per selected preset. Compare against `probe --scenes`
/// (the destructive LoadPreset→125 benchmark) for parity.
pub fn probe_scan_scenes_passive() -> Result<String, String> {
    use std::time::Instant;
    let overall = Instant::now();
    let mut s = Session::connect()?;
    let presets = s.list_my_presets()?;
    // Drain the handshake flood before the first re-armed read (a read fired
    // mid-flood is dropped device-side — the classic 0/25).
    s.drain_until_quiet(250, 20)?;
    let non_empty: Vec<_> = presets
        .iter()
        .filter(|p| !session::is_empty_slot_name(&p.name))
        .cloned()
        .collect();
    let mut out = format!(
        "[scenes-passive] {} presets — field-8 slot reads, NO LoadPreset\n",
        non_empty.len()
    );
    let (mut ok, mut missed) = (0u32, 0u32);
    for p in &non_empty {
        let t0 = Instant::now();
        match s.read_slot_preset_json(p.slot + 1)? {
            Some(json) => {
                ok += 1;
                let names = session::scene_names_from_slot_json(&json);
                let desc = match &names {
                    Some(n) if n.is_empty() => "(no scenes)".to_string(),
                    Some(n) => format!("{} scenes: {}", n.len(), n.join(", ")),
                    None => format!("(scenes unknown — partial cut early, {}B)", json.len()),
                };
                out += &format!(
                    "  {:>3}  {:34}  {desc}  {:.2}s\n",
                    p.slot,
                    p.name,
                    t0.elapsed().as_secs_f64()
                );
            }
            None => {
                missed += 1;
                out += &format!(
                    "  {:>3}  {:34}  NO REPLY  {:.2}s\n",
                    p.slot,
                    p.name,
                    t0.elapsed().as_secs_f64()
                );
            }
        }
    }
    out += &format!(
        "\n[scenes-passive] {ok}/{} OK, {missed} unanswered | {:.1}s total, {:.2}s avg\n",
        non_empty.len(),
        overall.elapsed().as_secs_f64(),
        overall.elapsed().as_secs_f64() / non_empty.len().max(1) as f64,
    );
    Ok(out)
}

/// POC: LoadPreset → sceneListResponse(125) loop on a single heartbeat session.
/// One handshake, then rapid LoadPreset + harvest scene names per slot.
pub fn probe_scan_scenes_load() -> Result<String, String> {
    use std::time::Instant;

    let presets = probe_connect_and_list()?;
    let non_empty: Vec<_> = presets
        .iter()
        .filter(|p| !session::is_empty_slot_name(&p.name))
        .cloned()
        .collect();
    let to_scan = non_empty.len();

    let mut s = Session::connect()?;
    // Sustain dense heartbeats for ~2s to enter "live controller" mode — the
    // device only pushes unsolicited data (sceneListResponse, PresetLoaded) on
    // a session with sustained heartbeat cadence.
    for _ in 0..16 {
        s.heartbeat()?;
        s.pump_collect(120)?;
    }

    let mut out =
        format!("[scenes-load] {to_scan} presets — LoadPreset → sceneList(125) on live session\n");
    let mut ok_count = 0u32;
    let mut fail_count = 0u32;
    let overall_start = Instant::now();

    for p in &non_empty {
        let t0 = Instant::now();
        s.raw.clear();
        // LoadPreset via send_and_collect (not load_preset which discards
        // the HID reports — the sceneListResponse push would be lost).
        s.send_and_collect(&proto::load_preset((p.slot + 1) as u64, 1), 300)?;
        // Pump for the unsolicited sceneListResponse(125) push.
        let mut scenes: Option<Vec<String>> = None;
        let mut seen = 0usize;
        for _ in 0..8 {
            s.pump_collect(150)?;
            let bodies = s.push_bodies();
            for b in bodies.iter().skip(seen) {
                if let Some(names) = session::decode_scene_list(b) {
                    scenes = Some(names);
                    break;
                }
            }
            seen = bodies.len();
            if scenes.is_some() {
                break;
            }
        }
        if scenes.is_none() {
            s.raw.clear();
            let _ = s.send_and_collect(&proto::scene_list_request(), 300);
            for _ in 0..4 {
                let bodies = s.push_bodies();
                if let Some(names) = bodies.iter().find_map(|b| session::decode_scene_list(b)) {
                    scenes = Some(names);
                    break;
                }
                let _ = s.pump_collect(200);
            }
        }
        let elapsed = t0.elapsed();
        match scenes {
            Some(names) => {
                ok_count += 1;
                if names.is_empty() {
                    out.push_str(&format!(
                        "  {:>3}  {:34}  (no scenes)  {:.2}s\n",
                        p.slot,
                        p.name,
                        elapsed.as_secs_f64(),
                    ));
                } else {
                    out.push_str(&format!(
                        "  {:>3}  {:34}  {} scenes: {}  {:.2}s\n",
                        p.slot,
                        p.name,
                        names.len(),
                        names.join(", "),
                        elapsed.as_secs_f64(),
                    ));
                }
            }
            None => {
                fail_count += 1;
                out.push_str(&format!(
                    "  {:>3}  {:34}  FAIL  {:.2}s\n",
                    p.slot,
                    p.name,
                    elapsed.as_secs_f64(),
                ));
            }
        }
        // Keep alive.
        let _ = s.heartbeat();
    }

    let total_elapsed = overall_start.elapsed();
    let avg = if ok_count + fail_count > 0 {
        total_elapsed.as_secs_f64() / (ok_count + fail_count) as f64
    } else {
        0.0
    };
    out.push_str(&format!(
        "\n[scenes-load] {ok_count}/{to_scan} OK, {fail_count} failed | {:.1}s total, {:.2}s avg\n",
        total_elapsed.as_secs_f64(),
        avg,
    ));
    Ok(out)
}

/// Fast full scene scan: LoadPreset on a live session → harvest the field-3
/// `currentPresetDataChanged` push (~17KB JSON with scenes, ftsw, audioGraph).
/// Same speed as `--scenes` (~0.5s/preset) but with full block details.
/// Changes the active preset on the device.
pub fn probe_scan_scenes_full_live() -> Result<String, String> {
    use std::time::Instant;

    let presets = probe_connect_and_list()?;
    let non_empty: Vec<_> = presets
        .iter()
        .filter(|p| !session::is_empty_slot_name(&p.name))
        .cloned()
        .collect();
    let to_scan = non_empty.len();

    let mut s = Session::connect()?;
    for _ in 0..16 {
        s.heartbeat()?;
        s.pump_collect(120)?;
    }

    let mut out = format!(
        "[scenes-full-live] {to_scan} presets — LoadPreset → field-3 currentPresetDataChanged\n"
    );
    let mut ok_count = 0u32;
    let mut fail_count = 0u32;
    let overall_start = Instant::now();

    for p in &non_empty {
        let t0 = Instant::now();
        s.raw.clear();
        s.send_and_collect(&proto::load_preset((p.slot + 1) as u64, 1), 300)?;
        let mut live: Option<session::CurrentPresetLive> = None;
        let mut seen = 0usize;
        for _ in 0..12 {
            s.pump_collect(150)?;
            let bodies = s.push_bodies();
            for b in bodies.iter().skip(seen) {
                if let Some(l) = session::decode_current_preset_live(b) {
                    if l.scene_names.is_some() || l.graph.is_some() {
                        live = Some(l);
                        break;
                    }
                }
            }
            seen = bodies.len();
            if live.is_some() {
                break;
            }
        }
        let elapsed = t0.elapsed();
        match live {
            Some(l) => {
                ok_count += 1;
                let scenes = l.scene_names.as_deref().unwrap_or(&[]);
                let has_ftsw = l.ftsw.is_some();
                let has_graph = l.graph.is_some();
                if scenes.is_empty() {
                    out.push_str(&format!(
                        "  {:>3}  {:34}  (no scenes)  ftsw={} graph={}  {:.2}s\n",
                        p.slot,
                        p.name,
                        has_ftsw,
                        has_graph,
                        elapsed.as_secs_f64(),
                    ));
                } else {
                    out.push_str(&format!(
                        "  {:>3}  {:34}  {} scenes: {}  ftsw={} graph={}  {:.2}s\n",
                        p.slot,
                        p.name,
                        scenes.len(),
                        scenes.join(", "),
                        has_ftsw,
                        has_graph,
                        elapsed.as_secs_f64(),
                    ));
                }
            }
            None => {
                fail_count += 1;
                out.push_str(&format!(
                    "  {:>3}  {:34}  FAIL  {:.2}s\n",
                    p.slot,
                    p.name,
                    elapsed.as_secs_f64(),
                ));
            }
        }
        let _ = s.heartbeat();
    }

    let total_elapsed = overall_start.elapsed();
    let avg = if ok_count + fail_count > 0 {
        total_elapsed.as_secs_f64() / (ok_count + fail_count) as f64
    } else {
        0.0
    };
    out.push_str(&format!(
        "\n[scenes-full-live] {ok_count}/{to_scan} OK, {fail_count} failed | {:.1}s total, {:.2}s avg\n",
        total_elapsed.as_secs_f64(), avg,
    ));
    Ok(out)
}
