//! Scene-leveling job planning: amp classification, knob classification, and job build (shared with the scene-leveling commands).

use super::scene_bench::knob_bounds;
use crate::leveller;
use crate::proto;
use crate::scenes;
use crate::session;
use crate::session::Session;
use crate::{AmpKnobSpec, LevelBlockArg};

pub(crate) fn is_amp_category(category: &str) -> bool {
    matches!(
        category,
        "Combo Amps" | "Amp Heads" | "Bass Amps" | "Half Stacks"
    )
}

pub(crate) fn amp_model_ids() -> &'static std::collections::HashSet<String> {
    static IDS: std::sync::OnceLock<std::collections::HashSet<String>> = std::sync::OnceLock::new();
    IDS.get_or_init(|| {
        let Ok(catalog) = serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../src/models/tmp-model-guide.json"
        )) else {
            return std::collections::HashSet::new();
        };
        let Some(rows) = catalog.get("blocks").and_then(|v| v.as_array()) else {
            return std::collections::HashSet::new();
        };
        rows.iter()
            .filter_map(|row| {
                let block_id = row.get("block_id").and_then(|v| v.as_str())?;
                let category = row.get("category").and_then(|v| v.as_str())?;
                is_amp_category(category).then(|| block_id.to_string())
            })
            .collect()
    })
}

pub(crate) fn is_amp_model_id(model_id: &str) -> bool {
    // Device FenderIds carry cab/IR/convolution suffixes the catalog's bare amp bids
    // omit (e.g. "ACD_HiwattDR103CanModCabIR", "ACD_PrincetonReverb68CabIRConvRvb").
    // Strip them one at a time, checking after each — mirrors the frontend
    // `baseDeviceId` / blockArt `SUFFIX`. ("NoFx" is part of real base ids, not stripped.)
    const SUFFIXES: [&str; 5] = ["ConvRvb", "CabIR", "NoCab", "Cab", "IR"];
    let amps = amp_model_ids();
    let mut m = model_id;
    loop {
        if amps.contains(m) {
            return true;
        }
        match SUFFIXES.iter().find_map(|s| m.strip_suffix(s)) {
            Some(next) => m = next,
            // Last-gap bridge: a wet amp id (…CabIRConvRvb) strips to a bare id the
            // catalog only carries WITH the NoFx token (…BlondeVibratoNoFx). NoFx is
            // never stripped, so try appending it once. Mirrors blockArt.ts.
            None => return !m.ends_with("NoFx") && amps.contains(&format!("{m}NoFx")),
        }
    }
}

pub(crate) fn is_amp_output_level_param(parameter_id: &str) -> bool {
    parameter_id == "outputLevel"
}

/// Pick the route STRUCTURE graph from the pre-pass docs: the first doc that decodes
/// to a KNOWN routing template (`session::is_known_routing_template`). Routing is
/// scene-invariant, so one complete-enough doc defines lane membership for every
/// scene. Returns `None` when no doc carries a known template — the live field-3
/// partial truncates before the `template` tail, and silently defaulting to "series"
/// would re-introduce the parallel mislevel, so the caller must skip instead.
pub(crate) fn structure_graph(
    docs: &[(u32, Option<serde_json::Value>)],
) -> Option<session::ActiveGraph> {
    docs.iter()
        .filter_map(|(_, d)| d.as_ref())
        .map(|d| session::extract_active_graph(d, None))
        .find(|g| session::is_known_routing_template(g.template.as_deref()))
}

/// Preset-wide gate: the routing template must be KNOWN (the live field-3 partial
/// truncates before the `template` tail, and silently defaulting to "series" would
/// re-introduce the parallel mislevel). A known template — series, parallel-merged,
/// split-output, or dual-input — is classifiable; only an unknown/incomplete one is a
/// hard error. (Mic-only paths produce no guitar amp candidate and skip per-scene.)
pub(crate) fn check_levelable_routing(structure: &session::ActiveGraph) -> Result<(), String> {
    if !session::is_known_routing_template(structure.template.as_deref()) {
        return Err("routing template unknown or read incomplete — cannot classify".to_string());
    }
    Ok(())
}

/// How a scene's amp knob set relates to the signal sum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParallelKind {
    /// One knob (series master / single amp) — no rebalance concept.
    Single,
    /// Two+ lane amps that RE-MERGE into one path (`gtrParallel*`): their mix is
    /// rebalanceable (rebalance only on a path merge).
    Merged,
    /// Two+ lane amps on SEPARATE physical outputs (`gtrSplit`/`gtrMicParallel`):
    /// joint-k for level, but NO rebalance (no shared mix between separate outs).
    SplitOutput,
}

/// Classify a scene into the SET of guitar-amp `outputLevel` knobs to drive, by amp
/// POSITION in the route graph (not the template string). Assumes [`check_levelable_routing`]
/// passed (known template). Levels against the USB 1/2 capture; no output→USB routing is
/// read (the user owns routing). Returns the knobs (`(group, node, current)`; >1 only for
/// a parallel/split scene → joint-k) or `Err(per-scene skip reason)`:
///
/// - Series → the LAST active amp in flow order (a post-merge amp counts as the series
///   master: scaling it scales the whole summed output).
/// - Parallel-merged / split-output / independent rails → the last active amp PER lane
///   (joint-k); a lane routed off USB contributes nothing to the capture but its amp is
///   still scaled by the shared factor.
/// - No active guitar amp (incl. mic-only presets), an active-amp lane with no
///   `outputLevel` control, multi-split amp spread, or a pre-split amp mixed with lane
///   amps → `Err` (never a partial joint-k).
pub(crate) fn classify_scene_knobs(
    structure: &session::ActiveGraph,
    scene_doc: &serde_json::Value,
    candidates: &[LevelBlockArg],
) -> Result<(Vec<AmpKnobSpec>, ParallelKind), String> {
    use session::Stage;
    // The amp's outputLevel candidate value, if it has one (None = no outputLevel knob).
    let ol = |g: &str, n: &str| {
        candidates
            .iter()
            .find(|c| {
                c.group_id == g && c.node_id == n && is_amp_output_level_param(&c.parameter_id)
            })
            .map(|c| c.value)
    };
    // Current value: the scene overlay's outputLevel if present, else the candidate value.
    let current = |g: &str, n: &str, fallback: f32| {
        session::extract_level_blocks(scene_doc)
            .into_iter()
            .find(|b| {
                b.group_id == g && b.node_id == n && is_amp_output_level_param(&b.parameter_id)
            })
            .map(|b| b.value)
            .unwrap_or(fallback)
    };
    // Active (non-bypassed in this scene) amp nodes, in route-graph flow order. Restricted
    // to GUITAR groups: re-amp drives the instrument input, so only the guitar chain is
    // captured at USB-Out (the leveling target); mic-input amps aren't reachable and have
    // no outputLevel candidate anyway. Bypass comes from the scene overlay, falling back
    // to the structure node when the scene doc doesn't carry it.
    let active: Vec<&session::GraphNode> = structure
        .nodes
        .iter()
        .filter(|nd| nd.group_id.starts_with('G') && is_amp_model_id(&nd.model))
        .filter(|nd| {
            match scenes::block_bypass_in_live_graph(scene_doc, &nd.group_id, &nd.node_id) {
                Some(b) => !b,
                None => !nd.bypassed,
            }
        })
        .collect();
    if active.is_empty() {
        return Err("no active guitar amp in scene".to_string());
    }

    // Parallel lanes that sum into / are captured at the USB-Out: every re-merging
    // stage split's two lanes, PLUS split-OUTPUT lanes (`gtrSplit`) and independent rails
    // (`gtrMicParallel`). We deliberately do NOT read the device's output→USB routing —
    // the leveler simply levels whatever the preset sends to USB 1/2 (the loudest-channel
    // capture); the user owns which path(s) reach USB 1/2. A split lane routed OFF USB
    // contributes nothing to the capture, so the joint-k solve is driven by the on-USB
    // lane; its amp is still scaled by the same factor (a side effect the user accepts by
    // managing routing). `post_merge` (Series stages after the last split) only applies to
    // re-merging stage splits, where a post-merge amp is the single series master.
    let group_of = |blocks: &[session::GraphNode]| -> Vec<String> {
        blocks.iter().map(|b| b.group_id.clone()).collect()
    };
    // Each split carries its KIND: a re-merging stage split is `Merged` (its lanes sum
    // back into one path → rebalancing their mix is meaningful); split-OUTPUT / rail
    // splits are `SplitOutput` (lanes go to separate physical outs → no shared mix to
    // rebalance).
    let mut splits: Vec<(Vec<String>, Vec<String>, ParallelKind)> = Vec::new();
    let mut post_merge: Vec<String> = Vec::new();
    let mut seen_split = false;
    for st in &structure.stages {
        match st {
            Stage::Series { blocks } => {
                if seen_split {
                    post_merge.extend(group_of(blocks));
                }
            }
            Stage::Split { a, b } => {
                seen_split = true;
                post_merge.clear(); // only Series groups after the LAST split count
                splits.push((group_of(a), group_of(b), ParallelKind::Merged));
            }
        }
    }
    if let Some(op) = &structure.outputs {
        splits.push((
            group_of(&op.a.blocks),
            group_of(&op.b.blocks),
            ParallelKind::SplitOutput,
        ));
        post_merge.clear();
    }
    if let Some(lanes) = &structure.lanes {
        if lanes.len() == 2 {
            splits.push((
                group_of(&lanes[0].blocks),
                group_of(&lanes[1].blocks),
                ParallelKind::SplitOutput,
            ));
            post_merge.clear();
        }
    }
    let in_groups = |gs: &[String], g: &str| gs.iter().any(|x| x == g);
    let split_groups: Vec<String> = splits
        .iter()
        .flat_map(|(a, b, _)| a.iter().chain(b))
        .cloned()
        .collect();
    // `active` is in structure.nodes order = flow order, so `.last()` of a filtered
    // subset is the last amp in flow.
    let last_in = |gs: &[String]| {
        active
            .iter()
            .rev()
            .copied()
            .find(|nd| in_groups(gs, &nd.group_id))
    };

    let resolve = |nd: &session::GraphNode| -> Result<(String, String, f32), String> {
        let v = ol(&nd.group_id, &nd.node_id).ok_or_else(|| {
            format!(
                "active amp {} has no outputLevel control — can't scene-level it",
                nd.node_id
            )
        })?;
        Ok((
            nd.group_id.clone(),
            nd.node_id.clone(),
            current(&nd.group_id, &nd.node_id, v),
        ))
    };

    // 1. A post-merge amp is the series master → single knob.
    if let Some(nd) = last_in(&post_merge) {
        return Ok((vec![resolve(nd)?], ParallelKind::Single));
    }

    // 2. Parallel: active amps in split lanes. Only the clean case — a SINGLE split's
    //    lanes, no pre-split/inter-split amp mixed in — joint-ks; anything more tangled
    //    is skipped rather than risk a wrong scaling.
    let mut amp_split_kind: Option<ParallelKind> = None;
    let mut amp_splits = 0usize;
    let mut lane_amps: Vec<&session::GraphNode> = Vec::new();
    for (a, b, kind) in &splits {
        let mut this = 0;
        if let Some(nd) = last_in(a) {
            lane_amps.push(nd);
            this += 1;
        }
        if let Some(nd) = last_in(b) {
            lane_amps.push(nd);
            this += 1;
        }
        if this > 0 {
            amp_splits += 1;
            amp_split_kind = Some(*kind);
        }
    }
    let trunk_amp = active
        .iter()
        .copied()
        .any(|nd| !in_groups(&split_groups, &nd.group_id) && !in_groups(&post_merge, &nd.group_id));
    if !lane_amps.is_empty() {
        if amp_splits > 1 {
            return Err("complex multi-split routing — level manually".to_string());
        }
        if trunk_amp {
            return Err("mixed pre-split + parallel amps — level manually".to_string());
        }
        let kind = amp_split_kind.unwrap_or(ParallelKind::Merged);
        let knobs = lane_amps
            .into_iter()
            .map(resolve)
            .collect::<Result<Vec<_>, _>>()?;
        // A single-amp parallel (only one lane has an amp) is just a single knob.
        let kind = if knobs.len() < 2 {
            ParallelKind::Single
        } else {
            kind
        };
        return Ok((knobs, kind));
    }

    // 3. Pure series (no split-lane amps): the last active amp overall is the master.
    Ok((
        vec![resolve(active.last().copied().unwrap())?],
        ParallelKind::Single,
    ))
}

/// Placeholder target for the knob-only probe arms (authority / mute-floor /
/// classify): those paths never solve, so the stamped target is never read —
/// any finite value serves.
pub(crate) const KNOB_ONLY_PROBE_TARGET_LUFS: f64 = -23.0;

/// Build per-scene [`leveller::SceneJob`]s from the pre-pass docs, ROUTING-AWARE:
/// classify each scene's amp set by position in the route graph (series=last amp;
/// parallel-merged=one amp per lane → joint-k) via [`classify_scene_knobs`], taking
/// each knob's CURRENT value from that scene's overlay. A scene the classifier can't
/// safely level (unknown/incomplete routing, mic/dual-input, split-output pending the
/// routing read, an amp lane with no outputLevel knob, tangled multi-split) becomes an
/// `Err` for that scene — never a silent single-amp fallback.
pub(crate) fn build_scene_jobs(
    scene_slots: &[u32],
    candidates: &[LevelBlockArg],
    docs: &[(u32, Option<serde_json::Value>)],
    target_lufs: f64,
) -> Result<Vec<leveller::SceneJob>, String> {
    if !candidates
        .iter()
        .any(|c| is_amp_output_level_param(&c.parameter_id))
    {
        return Err("per-scene leveling needs an amp outputLevel control".to_string());
    }
    let structure = structure_graph(docs).ok_or_else(|| {
        "no complete routing read (template missing from every scene doc) — \
         can't classify scene routing safely"
            .to_string()
    })?;
    // Preset-wide un-levelable routing (unknown template / mic / split-output) is a hard
    // error — the whole preset can't be scene-leveled. Per-SCENE issues below become skip
    // jobs so one bad scene doesn't abort the batch.
    check_levelable_routing(&structure)?;
    let jobs = scene_slots
        .iter()
        .map(|scene| {
            let doc = docs
                .iter()
                .find(|(s2, _)| s2 == scene)
                .and_then(|(_, d)| d.clone())
                .unwrap_or(serde_json::Value::Null);
            let scene_slot = if *scene >= session::BASE_SCENE_SLOT {
                None
            } else {
                Some(*scene)
            };
            match classify_scene_knobs(&structure, &doc, candidates) {
                Ok((triples, kind)) => {
                    let knobs = triples
                        .into_iter()
                        .map(|(group_id, node_id, current)| {
                            let (lo, hi) = knob_bounds(current);
                            leveller::KnobTarget {
                                knob: leveller::LevelKnob::Block {
                                    group_id,
                                    node_id,
                                    parameter_id: "outputLevel".to_string(),
                                    scene_slot,
                                },
                                lo,
                                hi,
                                current,
                            }
                        })
                        .collect::<Vec<_>>();
                    let rebalanceable = kind == ParallelKind::Merged && knobs.len() >= 2;
                    leveller::SceneJob {
                        scene_slot: *scene,
                        // Stamped with the batch target; the app command overrides it per
                        // wire job for a mixed-target batch.
                        target_lufs,
                        knobs,
                        skip: None,
                        rebalanceable,
                    }
                }
                Err(reason) => leveller::SceneJob {
                    scene_slot: *scene,
                    target_lufs,
                    knobs: Vec::new(),
                    skip: Some(reason),
                    rebalanceable: false,
                },
            }
        })
        .collect();
    Ok(jobs)
}

/// Un-engaged pre-pass for the app's batched scene leveling: ONE rich session
/// loads the preset and harvests each requested scene's live field-3 doc (the
/// knob-pick input). Base (`session::BASE_SCENE_SLOT`) is served from the
/// post-load push ONLY when that push's `lastLoadedScene` already names base;
/// otherwise the pre-pass recalls base explicitly (the post-load doc reflects
/// whatever scene was last active, which is not necessarily base). Must run
/// before any re-amp engage — the device pushes no field-3 while engaged.
///
/// ACTIVE-SCENE GAP (HW, the Arpeges `doc=0B` build fail): the device pushes
/// field-3 only on a CHANGE, so recalling the scene that is ALREADY active —
/// the preset's saved `lastLoadedScene`, materialized by the `load_preset`
/// above — yields NO push and the harvest comes back empty. Which scene that
/// is depends on the preset's last save, so the failure moves between runs
/// (it presents as a random per-scene "can't classify routing" skip). The
/// post-load doc IS that scene's doc (the load materialized its state), so
/// serve the already-active scene from the last harvested doc instead of
/// sending a doomed no-change recall.
/// The prepass result: per-scene docs plus the preset's ORIGINAL active scene (its
/// saved `lastLoadedScene`, materialized by the prepass `load_preset`) — the scene the
/// batch-end save must restore so the preset persists in the state it was loaded in.
pub(crate) type SceneDocs = (Vec<(u32, Option<serde_json::Value>)>, Option<u32>);

pub(crate) fn prepass_scene_docs(slot: u32, scene_slots: &[u32]) -> Result<SceneDocs, String> {
    let mut s = Session::connect()?;
    for _ in 0..8 {
        s.heartbeat()?;
        s.pump_collect(120)?;
    }
    s.raw.clear();
    s.send_and_collect(&proto::load_preset((slot + 1) as u64, 1), 300)?;
    for _ in 0..6 {
        s.heartbeat()?;
        s.pump_collect(200)?;
    }
    let post_load_doc = s.current_preset_value().ok();
    // The wire scene the device currently has materialized (0-based scenes[] index;
    // BASE_SCENE_SLOT = base). Tracked across the loop so only a genuinely-inactive
    // scene is recalled.
    let mut active: Option<u32> = post_load_doc
        .as_ref()
        .and_then(|d| d.get("lastLoadedScene"))
        .and_then(serde_json::Value::as_u64)
        .map(|v| v as u32);
    // The doc reflecting the CURRENTLY-active scene's materialized state — this is
    // whatever scene `lastLoadedScene` names, NOT necessarily the base scene.
    let mut active_doc = post_load_doc.clone();
    // The preset's ORIGINAL active scene, before any recall below — returned so the
    // batch-end save can restore it.
    let original_active = active;
    // A genuine base-scene document, fetched lazily and kept distinct from
    // `active_doc`: the post-load doc reflects `lastLoadedScene`, which may name a
    // non-base scene, so it must not be reused for base-scene classification.
    let mut base_doc: Option<serde_json::Value> = None;
    let mut docs = Vec::with_capacity(scene_slots.len());
    for &scene in scene_slots {
        if scene >= session::BASE_SCENE_SLOT {
            if base_doc.is_none() {
                if active == Some(session::BASE_SCENE_SLOT) {
                    base_doc = active_doc.clone();
                } else {
                    s.raw.clear();
                    s.send_and_collect(&proto::load_scene(session::BASE_SCENE_SLOT as u64), 300)?;
                    let mut doc = None;
                    for _ in 0..4 {
                        s.heartbeat()?;
                        s.pump_collect(150)?;
                        if let Ok(v) = s.current_preset_value() {
                            doc = Some(v);
                            break;
                        }
                    }
                    active = Some(session::BASE_SCENE_SLOT);
                    active_doc = doc.clone();
                    base_doc = doc;
                }
            }
            docs.push((scene, base_doc.clone()));
        } else if active == Some(scene) {
            docs.push((scene, active_doc.clone()));
        } else {
            s.raw.clear();
            s.send_and_collect(&proto::load_scene(scene as u64), 300)?;
            let mut doc = None;
            for _ in 0..4 {
                s.heartbeat()?;
                s.pump_collect(150)?;
                if let Ok(v) = s.current_preset_value() {
                    doc = Some(v);
                    break;
                }
            }
            active = Some(scene);
            active_doc = doc.clone();
            docs.push((scene, doc));
        }
    }
    Ok((docs, original_active))
}

/// Shallow-merge a saved scene's sparse overlay onto a cloned base `audioGraph` (mutated in
/// place). For every node in every group of both `guitarNodes` and `micNodes`, if the scene
/// carries `<graph>.<group>.<FenderId or nodeId>.dspUnitParameters`, those keys win over the
/// cloned node's `dspUnitParameters` (base keys the overlay omits survive). A `splitMix`
/// overlay merges key-level onto the cloned `audioGraph.splitMix`.
fn overlay_scene_onto_graph(graph: &mut serde_json::Value, scene: &serde_json::Value) {
    for key in ["guitarNodes", "micNodes"] {
        let overlay_groups = scene.get(key).and_then(|g| g.as_object());
        let Some(groups) = graph.get_mut(key).and_then(|g| g.as_object_mut()) else {
            continue;
        };
        for (group, nodes) in groups.iter_mut() {
            let Some(nodes) = nodes.as_array_mut() else {
                continue;
            };
            for node in nodes.iter_mut() {
                // Overlay keyed by FenderId (nodeId fallback), matching overlay_ab.rs.
                let fid = node
                    .get("FenderId")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
                let nid = node
                    .get("nodeId")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
                let overlay_params = overlay_groups
                    .and_then(|g| g.get(group))
                    .and_then(|grp| {
                        fid.as_deref()
                            .and_then(|f| grp.get(f))
                            .or_else(|| nid.as_deref().and_then(|n| grp.get(n)))
                    })
                    .and_then(|n| n.get("dspUnitParameters"))
                    .and_then(|p| p.as_object());
                let Some(overlay_params) = overlay_params else {
                    continue;
                };
                let Some(obj) = node.as_object_mut() else {
                    continue;
                };
                let Some(dsp) = obj
                    .entry("dspUnitParameters")
                    .or_insert_with(|| serde_json::json!({}))
                    .as_object_mut()
                else {
                    continue;
                };
                for (k, v) in overlay_params {
                    dsp.insert(k.clone(), v.clone());
                }
            }
        }
    }
    if let Some(overlay_split) = scene.get("splitMix").and_then(|s| s.as_object()) {
        if let Some(base_split) = graph
            .as_object_mut()
            .map(|g| g.entry("splitMix").or_insert_with(|| serde_json::json!({})))
            .and_then(|s| s.as_object_mut())
        {
            for (k, v) in overlay_split {
                base_split.insert(k.clone(), v.clone());
            }
        }
    }
}

/// SAVED-JSON alternative to the live `prepass_scene_docs`: derive each requested scene's
/// synthetic field-3 doc from a field-8 read instead of recalling scenes on the unit (no
/// user-visible scene-hopping). Each doc is `{ "audioGraph": ... }` shaped exactly like the
/// live push the consumers read (`session::extract_active_graph`/`extract_level_blocks`,
/// `scenes::block_bypass_in_live_graph`): the whole base graph for a base-scene slot
/// (`session::BASE_SCENE_SLOT`), or the base graph with `scenes[i]`'s sparse per-node
/// `dspUnitParameters` (+ `splitMix`) overlaid for an FS scene. The restore scene is the
/// preset's `lastLoadedScene` (`None` when absent).
///
/// Returns `None` (the caller must fall back to the live prepass — NEVER a partial answer)
/// when `audioGraph` is missing OR any requested FS scene index is absent from `scenes[]` or
/// isn't an object (a truncated field-8 read).
///
/// Overlay agreement validated against the live prepass by `probe --overlay-ab` (76/76
/// scene-amp pairs, 0 bypass mismatches). Shipped DARK — see `prepass_scene_docs_via`.
pub(crate) fn scene_docs_from_saved(
    preset: &serde_json::Value,
    scene_slots: &[u32],
) -> Option<SceneDocs> {
    let base_graph = preset.get("audioGraph")?;
    let restore = preset
        .get("lastLoadedScene")
        .and_then(serde_json::Value::as_u64)
        .map(|v| v as u32);
    let scenes = preset.get("scenes").and_then(|s| s.as_array());
    let mut docs = Vec::with_capacity(scene_slots.len());
    for &slot in scene_slots {
        if slot >= session::BASE_SCENE_SLOT {
            docs.push((slot, Some(serde_json::json!({ "audioGraph": base_graph }))));
            continue;
        }
        // FS scene: the index must exist and be an object, else the read is truncated → bail
        // (a partial answer would silently mis-classify the missing scenes).
        let scene = scenes
            .and_then(|a| a.get(slot as usize))
            .filter(|s| s.is_object())?;
        let mut graph = base_graph.clone();
        overlay_scene_onto_graph(&mut graph, scene);
        docs.push((slot, Some(serde_json::json!({ "audioGraph": graph }))));
    }
    Some((docs, restore))
}

/// Scene-doc prepass with a switch between the SAVED-JSON path and the live recall path.
/// `use_overlay` = read the field-8 preset and derive the docs via `scene_docs_from_saved`;
/// on a `None` (missing graph / truncated scene) or read error it logs the reason and falls
/// back to the live `prepass_scene_docs`. `!use_overlay` goes straight to the live prepass.
///
/// Shipped DARK: the sole production call site passes `use_overlay = false`, so default
/// behavior is byte-identical to `prepass_scene_docs`. The overlay path was validated by
/// `probe --overlay-ab` (76/76 scene-amp pairs agree, 0 bypass mismatches).
///
/// ADOPTION-TIME TODO (before flipping to `true`): `read_slot_preset_parsed` opens its OWN
/// session and does NOT recall the preset on the unit, but the batched-apply runner contract
/// requires the preset to already be current (the apply session sends no LoadPreset — the
/// live prepass's own `load_preset` is what makes it current today). Enabling the overlay
/// path must therefore add a `load_preset(slot)` so the apply lands on the right preset.
pub(crate) fn prepass_scene_docs_via(
    slot: u32,
    scene_slots: &[u32],
    use_overlay: bool,
) -> Result<SceneDocs, String> {
    if use_overlay {
        match crate::read_slot_preset_parsed(slot) {
            Ok((preset, _, _)) => {
                if let Some(docs) = scene_docs_from_saved(&preset, scene_slots) {
                    return Ok(docs);
                }
                log::info!(
                    "scene-doc overlay: slot {slot} field-8 read lacked audioGraph or a requested scene — falling back to live prepass"
                );
            }
            Err(e) => log::info!(
                "scene-doc overlay: slot {slot} field-8 read failed ({e}) — falling back to live prepass"
            ),
        }
    }
    prepass_scene_docs(slot, scene_slots)
}

#[cfg(test)]
#[path = "scene_jobs_tests.rs"]
mod scene_jobs_tests;
