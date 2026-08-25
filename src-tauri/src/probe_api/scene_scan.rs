//! Probe entry points: scene enumeration (passive / load / full-live) + scene classify + one-shot level.

use super::level::load_and_filter_amp_candidates;
use super::scene_jobs::prepass_scene_docs;
use super::scene_jobs::structure_graph;
use super::scene_jobs::{build_scene_jobs, KNOB_ONLY_PROBE_TARGET_LUFS};
use super::slot_write::probe_connect_and_list;
use super::stimulus::probe_stimulus_path;
use super::stimulus::read_stimulus_calibrated;
use crate::leveller;
use crate::proto;
use crate::session;
use crate::session::Session;

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
    commit: bool,
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
    let (docs, restore_scene) = prepass_scene_docs(list_index, &scene_slots)?;
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    // THE field-8 read for this run: the routing-structure fallback AND the raw scene
    // overlays `set_knobs` needs for its Scene Edit decision.
    let saved = super::scene_jobs::read_saved_preset(list_index);
    let jobs = build_scene_jobs(
        &scene_slots,
        &candidates,
        &docs,
        target_lufs,
        saved.as_ref(),
    )?;
    // `commit` = repro instrumentation: run the REAL deferred-save path (the app's shape).
    let outcomes = if rebalance {
        leveller::level_scenes_rebalance(
            list_index,
            &jobs,
            &stim,
            commit,
            restore_scene.filter(|_| commit),
            saved.as_ref(),
            // No headroom trade on this dev arm — it runs the jobs verbatim.
            None,
            |_, _| {},
            || false,
        )
    } else {
        leveller::level_scenes_oneshot(
            list_index,
            &jobs,
            &stim,
            commit,
            restore_scene.filter(|_| commit),
            saved.as_ref(),
            // No headroom trade on this dev arm — it runs the jobs verbatim.
            None,
            |_, _| {},
            || false,
        )
    };
    // Guaranteed re-amp OFF regardless of outcome (a stranded re-amp mutes the input).
    leveller::reamp_off_guaranteed("scene-level");
    let outcomes = outcomes?;
    let mut out = format!(
        "{} leveling preset list_index={list_index} → target {target_lufs:.1} LUFS (topology {topology_id})\n",
        if commit { "COMMIT" } else { "NO-SAVE" },
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

/// `probe --knob-sweep <listIdx> <group> <node> <param> <v1,v2,…>` — repro
/// instrumentation: measure the captured loudness at each knob value on isolated fresh
/// re-amp captures (the `measure_fs_at` recipe, no bypass forcing), in the preset's
/// natural post-load state at its stored presetLevel. Working-copy writes are discarded
/// by a final reload; ends with a guaranteed re-amp OFF. Stimulus via
/// TMP_LEVELLER_STIMULUS (injected verbatim).
pub fn probe_knob_sweep(
    list_index: u32,
    group: &str,
    node: &str,
    param: &str,
    values: &[f32],
) -> Result<String, String> {
    let stim_path = std::env::var("TMP_LEVELLER_STIMULUS")
        .map_err(|_| "set TMP_LEVELLER_STIMULUS to the stimulus WAV".to_string())?;
    let stim = read_stimulus_calibrated(&stim_path, None)?;
    {
        let mut s = Session::connect_lean()?;
        s.load_preset(list_index)?;
        std::thread::sleep(std::time::Duration::from_millis(
            leveller::settle_after_load_ms(),
        ));
    }
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    let mut out = format!("[probe --knob-sweep] list_index={list_index} {group}/{node}.{param}\n");
    for v in values {
        // Probe sweep of a saved preset: no run-owned `presetLevel` to assert.
        let l = leveller::measure_fs_at(None, (group, node, param), &[], &stim, *v, None)?;
        out += &format!(
            "  {param}={v:.3} → integrated {:.3} LUFS  short-term-max {:.3}\n",
            l.integrated_lufs, l.short_term_max_lufs
        );
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    }
    // Discard the sweep pollution, then the guaranteed OFF.
    if let Ok(mut s) = Session::connect_lean() {
        let _ = s.load_preset(list_index);
    }
    leveller::reamp_off_guaranteed("knob-sweep");
    Ok(out)
}

/// `probe --measure-pair <listIdx> <topology> <presetLevel> <g:n:p=v>…` — P0 repro
/// instrumentation for the headroom-trade physics: measure the captured loudness at an
/// explicit (`presetLevel` × block-param) point on one isolated fresh re-amp capture
/// (bundled stimulus, base recall inside `measure_pair_at`). Working-copy writes are
/// discarded by a final reload; ends with a guaranteed re-amp OFF.
pub fn probe_measure_pair(
    list_index: u32,
    topology_id: &str,
    preset_level: f32,
    scene: Option<u32>,
    writes: &[(String, String, String, f32)],
) -> Result<String, String> {
    let stim = read_stimulus_calibrated(&super::stimulus::probe_stimulus_path(topology_id)?, None)?;
    {
        let mut s = Session::connect_lean()?;
        s.load_preset(list_index)?;
        std::thread::sleep(std::time::Duration::from_millis(
            leveller::settle_after_load_ms(),
        ));
    }
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    let result = leveller::measure_pair_at(scene, preset_level, writes, &stim);
    // Discard the pair pollution, then the guaranteed OFF (bound before the `?`).
    if let Ok(mut s) = Session::connect_lean() {
        let _ = s.load_preset(list_index);
    }
    leveller::reamp_off_guaranteed("measure-pair");
    let l = result?;
    let wtxt: Vec<String> = writes
        .iter()
        .map(|(g, n, p, v)| format!("{g}/{n}.{p}={v:.4}"))
        .collect();
    Ok(format!(
        "[probe --measure-pair] list_index={list_index} scene={scene:?} presetLevel={preset_level:.4} {} \
         → integrated {:.3} LUFS  short-term-max {:.3}\n",
        wtxt.join(" "),
        l.integrated_lufs,
        l.short_term_max_lufs
    ))
}

/// `probe --scene-doc <listIdx> <scene…>` — repro instrumentation: load the preset,
/// then recall the given scenes IN ORDER on ONE held session, harvesting the device's
/// RENDERED field-3 doc after each recall and printing the amp/vibe param values.
/// Lets the caller control the arrival order, to catch rendered-vs-stored divergence
/// (scene-materialization infidelity) deterministically. NON-DESTRUCTIVE: no writes.
pub fn probe_scene_doc(list_index: u32, scenes: &[u32]) -> Result<String, String> {
    const NODES: [(&str, &str); 2] = [("G1", "ACD_HiwattDR103CanMod"), ("G4", "ACD_UniVibe")];
    fn fmt_doc(label: &str, doc: &serde_json::Value) -> String {
        let mut out = format!(
            "[{label}] lastLoadedScene={:?}\n",
            doc.get("lastLoadedScene")
        );
        for (g, n) in NODES {
            match crate::scenes::guitar_node(doc, g, n)
                .and_then(|node| node.get("dspUnitParameters"))
                .and_then(|p| p.as_object())
            {
                Some(params) => {
                    let mut kv: Vec<String> = params
                        .iter()
                        .filter_map(|(k, v)| v.as_f64().map(|f| format!("{k}={f:.4}")))
                        .collect();
                    kv.sort();
                    out += &format!("  {g}/{n}: {}\n", kv.join(" "));
                }
                None => out += &format!("  {g}/{n}: <absent/truncated>\n"),
            }
        }
        out
    }
    let mut s = Session::connect()?;
    for _ in 0..8 {
        s.heartbeat()?;
        s.pump_collect(120)?;
    }
    s.raw.clear();
    s.send_and_collect(&proto::load_preset((list_index + 1) as u64, 1), 300)?;
    for _ in 0..6 {
        s.heartbeat()?;
        s.pump_collect(200)?;
    }
    let mut out = format!("[probe --scene-doc] list_index={list_index}\n");
    match s.current_preset_value() {
        Ok(d) => out += &fmt_doc("post-load", &d),
        Err(e) => out += &format!("[post-load] no doc: {e}\n"),
    }
    for &sc in scenes {
        s.raw.clear();
        s.send_and_collect(&proto::load_scene(sc as u64), 300)?;
        let mut doc = None;
        for _ in 0..4 {
            s.heartbeat()?;
            s.pump_collect(150)?;
            if let Ok(v) = s.current_preset_value() {
                doc = Some(v);
                break;
            }
        }
        match doc {
            Some(d) => out += &fmt_doc(&format!("recall scene {sc}"), &d),
            None => out += &format!("[recall scene {sc}] NO DOC harvested\n"),
        }
    }
    Ok(out)
}

/// `probe --scene-node-doc <listIdx> <group> <node> <scene…>` — repro instrumentation:
/// the [`probe_scene_doc`] recipe (load, then recall the given scenes IN ORDER on ONE
/// held session, harvesting the rendered field-3 doc after each recall) generalized to
/// a caller-chosen node instead of the hard-coded Hiwatt/UniVibe pair. Prints the
/// node's FULL rendered `dspUnitParameters` (bools and strings included, not just
/// floats) plus the doc's `ftsw` active flags — the discriminator for how a partial
/// (bypass-only) scene overlay and `ftswStates` materialize on recall.
/// NON-DESTRUCTIVE: no writes, no re-amp.
pub fn probe_scene_node_doc(
    list_index: u32,
    group: &str,
    node: &str,
    scenes: &[u32],
) -> Result<String, String> {
    fn fmt_doc(label: &str, doc: &serde_json::Value, group: &str, node: &str) -> String {
        let mut out = format!(
            "[{label}] lastLoadedScene={:?}\n",
            doc.get("lastLoadedScene")
        );
        match crate::scenes::guitar_node(doc, group, node)
            .and_then(|n| n.get("dspUnitParameters"))
            .and_then(|p| p.as_object())
        {
            Some(params) => {
                let mut kv: Vec<String> = params.iter().map(|(k, v)| format!("{k}={v}")).collect();
                kv.sort();
                out += &format!("  {group}/{node}: {}\n", kv.join(" "));
            }
            None => out += &format!("  {group}/{node}: <absent/truncated>\n"),
        }
        match doc.get("ftsw").and_then(|f| f.as_array()) {
            Some(slots) => {
                let active: Vec<String> = slots
                    .iter()
                    .enumerate()
                    .flat_map(|(i, slot)| {
                        slot.as_array().into_iter().flatten().map(move |a| {
                            format!(
                                "FS{i}:{}{}",
                                a.get("customLabel").and_then(|l| l.as_str()).unwrap_or("?"),
                                if a.get("isActive").and_then(|b| b.as_bool()) == Some(true) {
                                    "=ON"
                                } else {
                                    "=off"
                                }
                            )
                        })
                    })
                    .collect();
                out += &format!("  ftsw: {}\n", active.join(" "));
            }
            None => out += "  ftsw: <absent/truncated>\n",
        }
        out
    }
    let mut s = Session::connect()?;
    for _ in 0..8 {
        s.heartbeat()?;
        s.pump_collect(120)?;
    }
    s.raw.clear();
    s.send_and_collect(&proto::load_preset((list_index + 1) as u64, 1), 300)?;
    for _ in 0..6 {
        s.heartbeat()?;
        s.pump_collect(200)?;
    }
    let mut out = format!("[probe --scene-node-doc] list_index={list_index} {group}/{node}\n");
    match s.current_preset_value() {
        Ok(d) => out += &fmt_doc("post-load", &d, group, node),
        Err(e) => out += &format!("[post-load] no doc: {e}\n"),
    }
    for &sc in scenes {
        s.raw.clear();
        s.send_and_collect(&proto::load_scene(sc as u64), 300)?;
        let mut doc = None;
        for _ in 0..4 {
            s.heartbeat()?;
            s.pump_collect(150)?;
            if let Ok(v) = s.current_preset_value() {
                doc = Some(v);
                break;
            }
        }
        match doc {
            Some(d) => out += &fmt_doc(&format!("recall scene {sc}"), &d, group, node),
            None => out += &format!("[recall scene {sc}] NO DOC harvested\n"),
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
    let (docs, _) = prepass_scene_docs(list_index, &scene_slots)?;
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
    // Classify-only: the stamped target is never read (no solve), so any finite value serves.
    let jobs = build_scene_jobs(
        &scene_slots,
        &candidates,
        &docs,
        KNOB_ONLY_PROBE_TARGET_LUFS,
        None,
    )?;
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
