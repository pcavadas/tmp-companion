//! Scene-leveling job planning: amp classification, knob classification, and job build (shared with the scene-leveling commands).

use super::scene_bench::knob_bounds;
use crate::footswitch::{self, max_referenced_scene};
use crate::leveller;
use crate::proto;
use crate::scenes;
use crate::session;
use crate::session::Session;
use crate::{param_class, AmpKnobSpec, LevelBlockArg};

pub(crate) fn is_amp_category(category: &str) -> bool {
    matches!(
        category,
        "Combo Amps" | "Amp Heads" | "Bass Amps" | "Half Stacks"
    )
}

/// Collect catalog block ids from `tmp-model-guide.json` whose `category` satisfies
/// `is_match`. Shared by `amp_model_ids` (amp categories, cached behind its own
/// `OnceLock`) and `fixture_gates::cab_model_ids` (`"Cabinets" | "IR"`), so amp-ness
/// and cab-ness can never disagree about what the catalog says.
pub(crate) fn model_ids_by_category(
    is_match: impl Fn(&str) -> bool,
) -> std::collections::HashSet<String> {
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
            is_match(category).then(|| block_id.to_string())
        })
        .collect()
}

pub(crate) fn amp_model_ids() -> &'static std::collections::HashSet<String> {
    static IDS: std::sync::OnceLock<std::collections::HashSet<String>> = std::sync::OnceLock::new();
    IDS.get_or_init(|| model_ids_by_category(is_amp_category))
}

/// Collapse a device FenderId's cab/IR/convolution suffixes, testing `contains`
/// after each strip, and return the first form that matches (or `None`). Device
/// FenderIds carry suffixes a catalog's bare bid lacks (e.g.
/// "ACD_HiwattDR103CanModCabIR", "ACD_PrincetonReverb68CabIRConvRvb") — strip them
/// one at a time, CHECKING BEFORE each strip (so an id already catalogued WITH a
/// suffix, like the `…CabIRConvRvb` reverb amps, matches directly and is never
/// over-stripped), mirroring the frontend `baseDeviceId` / blockArt `SUFFIX`.
/// ("NoFx" is part of real base ids, so it is never itself stripped.) The
/// last-gap bridge appends "NoFx" once: a wet amp id (…CabIRConvRvb) strips to a
/// bare id the catalog only carries WITH the NoFx token (…BlondeVibratoNoFx).
///
/// Shared by `is_amp_model_id` (catalog amp classification) and
/// `param_class::classify` (block-scoped parameter overrides) so the one
/// suffix-collapse rule can't drift between the two call sites.
/// Device FenderId cab/IR/convolution suffixes, in strip-priority order — shared by
/// `resolve_base_id` (repeated strip-and-check) and `bakes_in_a_cab` (a single suffix
/// scan). "CabIRConvRvb" is deliberately absent: it already ends in "ConvRvb", so it is
/// unreachable in an `any(ends_with)` scan.
const CAB_SUFFIXES: [&str; 5] = ["ConvRvb", "CabIR", "NoCab", "Cab", "IR"];

pub(crate) fn resolve_base_id(model_id: &str, contains: impl Fn(&str) -> bool) -> Option<String> {
    let mut m = model_id;
    loop {
        if contains(m) {
            return Some(m.to_string());
        }
        match CAB_SUFFIXES.iter().find_map(|s| m.strip_suffix(s)) {
            Some(next) => m = next,
            None => {
                if m.ends_with("NoFx") {
                    return None;
                }
                let with_nofx = format!("{m}NoFx");
                return contains(&with_nofx).then_some(with_nofx);
            }
        }
    }
}

/// Does this device FenderId carry its cabinet BAKED IN (a combo / amp+cab-merged
/// model)? The device spells those with a cab/IR suffix on the bare amp id
/// (`ACD_DeluxeReverb65BlondeVibratoNoFx` → `…NoFxCabIR`). `NoCab` is the explicit
/// OPPOSITE and must never match, even though it ends in "Cab" — checked FIRST, before
/// the shared suffix scan below. Used by `fixture_gates`' cab rule
/// (`e2e::every_guitar_amp_in_every_fixture_reaches_a_cab`'s `is_cab_merged_amp`) — its
/// only caller today, hence `#[cfg(test)]` (avoids a dead-code lint on a plain lib build).
#[cfg(test)]
pub(crate) fn bakes_in_a_cab(model_id: &str) -> bool {
    if model_id.ends_with("NoCab") {
        return false;
    }
    CAB_SUFFIXES.iter().any(|s| model_id.ends_with(s))
}

pub(crate) fn is_amp_model_id(model_id: &str) -> bool {
    let amps = amp_model_ids();
    resolve_base_id(model_id, |m| amps.contains(m)).is_some()
}

pub(crate) fn is_amp_output_level_param(parameter_id: &str) -> bool {
    parameter_id == "outputLevel"
}

/// The stand-in for a scene whose doc never arrived — a borrowable `Null`, so the job builder
/// can hand out `&Value` without cloning a real doc to get one.
static NULL_DOC: serde_json::Value = serde_json::Value::Null;

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

/// The graph's guitar-amp nodes, in route-graph flow order. Restricted to GUITAR groups:
/// re-amp drives the instrument input, so only the guitar chain is captured at USB-Out (the
/// leveling target); mic-input amps aren't reachable and have no `outputLevel` candidate.
fn amp_nodes(structure: &session::ActiveGraph) -> Vec<&session::GraphNode> {
    structure
        .nodes
        .iter()
        .filter(|nd| nd.group_id.starts_with('G') && is_amp_model_id(&nd.model))
        .collect()
}

/// Whether `doc` states the bypass of EVERY one of `amps` — the single definition of
/// "usable scene doc" that [`classify_scene_knobs`] refuses on and
/// [`scenes_missing_amp_bypass`] scans for, so the two can never disagree. All, not any: a
/// doc cut BETWEEN two amps answers the first and lets the second fall back to base, which
/// levels a two-amp scene as the wrong pair.
fn answers_all_amp_bypasses(amps: &[&session::GraphNode], doc: &serde_json::Value) -> bool {
    amps.iter()
        .all(|nd| scenes::block_bypass_in_live_graph(doc, &nd.group_id, &nd.node_id).is_some())
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
    // Active (non-bypassed in this scene) amp nodes, in route-graph flow order. Bypass comes
    // from the scene overlay, falling back to the structure node when the scene doc doesn't
    // carry it.
    let amps = amp_nodes(structure);
    // REFUSE rather than decide from the base graph when the scene doc can't answer for EVERY
    // amp. `structure` is the BASE graph (`saved_fallback`), so the `None` arm below resolves
    // bypass from base — catastrophic for a doc that never arrived or was cut before or between
    // the amps: a scene that SWAPS amps then gets leveled
    // on whichever amp BASE leaves on, a knob outside that scene's signal path. The solve sweeps
    // it, nothing moves, and the row reports a `no_authority` clamp indistinguishable from a
    // genuine ceiling (HW: Friedman HBE slot 28 — every scene drove `ACD_BE100` while the Twin the
    // swap scene needs was never written).
    //
    // A well-formed doc always answers: the live prepass materialises the whole graph, and
    // `scene_docs_from_saved` merges the sparse overlay ONTO the base graph. So "no amp is
    // answerable" means the doc is unusable, not that the scene is unusual — and this row's own
    // `skip` is the honest outcome (per-scene skip, never a batch abort).
    if !answers_all_amp_bypasses(&amps, scene_doc) {
        return Err(
            "scene doc does not answer every amp's bypass state (absent or truncated before or \
             between the amp nodes) — refusing to classify against the base graph"
                .to_string(),
        );
    }
    let active: Vec<&session::GraphNode> = amps
        .into_iter()
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
///
/// `saved_fallback` = the slot's field-8 saved preset JSON, used ONLY for the routing
/// STRUCTURE when no live doc carries a complete `audioGraph.template`: a preset with a
/// large audioGraph (many blocks) overruns the device's LEAN-session field-3 push (~3.4 KB
/// observed; a dense healthy session can deliver the full doc, but the prepass sessions
/// empirically get the lean push) in EVERY scene doc, so classification failed for the
/// whole preset ("some presets
/// just never scene-level"). Routing is scene-invariant and the prepass `load_preset`
/// materialized exactly the saved preset, so the saved base graph is authoritative for
/// template + lane membership. Knob VALUES still come from the live per-scene docs.
pub(crate) fn build_scene_jobs(
    scene_slots: &[u32],
    candidates: &[LevelBlockArg],
    docs: &[(u32, Option<serde_json::Value>)],
    target_lufs: f64,
    saved_fallback: Option<&serde_json::Value>,
) -> Result<Vec<leveller::SceneJob>, String> {
    build_scene_jobs_with_handles(
        scene_slots,
        candidates,
        docs,
        target_lufs,
        saved_fallback,
        &[],
    )
}

/// A row's USER-CHOSEN leveling control, as the wire job named it: the block param the solve
/// should sweep INSTEAD of the active amp's `outputLevel`. Its class, bounds and per-scene
/// current value are resolved by [`build_scene_jobs_with_handles`] off the SAVED preset + that
/// scene's doc, so the caller ships coordinates only.
pub(crate) struct SceneHandleSpec<'a> {
    pub group_id: &'a str,
    pub node_id: &'a str,
    pub parameter_id: &'a str,
}

/// [`build_scene_jobs`] with the per-row handles threaded in — `handles` is sparse, keyed by
/// wire scene slot, and a row it names is built from THAT control instead of the scene's amp.
///
/// Why the handle belongs HERE and not in a patch-up pass afterwards: the amp prerequisites
/// (an `outputLevel` candidate, a readable routing template) are inputs the handle rows do not
/// need — the user already named the knob. Failing them preset-wide errored rows that never
/// touched the amp classifier. So the prerequisite is evaluated ONCE and consulted only by the
/// rows that need it; a failure becomes THOSE rows' `skip`, and the handle rows level on.
///
/// The one exception keeps the amp-only contract intact: with NO handles at all (every probe
/// seam, the redistribution runner, a batch where nobody named a control) a prerequisite
/// failure is still the BATCH's `Err`, not N identical per-row skips.
pub(crate) fn build_scene_jobs_with_handles(
    scene_slots: &[u32],
    candidates: &[LevelBlockArg],
    docs: &[(u32, Option<serde_json::Value>)],
    target_lufs: f64,
    saved_fallback: Option<&serde_json::Value>,
    handles: &[(u32, SceneHandleSpec)],
) -> Result<Vec<leveller::SceneJob>, String> {
    // The AMP-`outputLevel` classifier's preset-wide inputs, resolved once. `Err` = no row can
    // take the amp path (its reason is what those rows report).
    let amp_prereq: Result<session::ActiveGraph, String> = (|| {
        if !candidates
            .iter()
            .any(|c| is_amp_output_level_param(&c.parameter_id))
        {
            return Err("per-scene leveling needs an amp outputLevel control".to_string());
        }
        let structure = structure_graph(docs)
            .or_else(|| {
                saved_fallback
                    .map(|v| session::extract_active_graph(v, None))
                    .filter(|g| session::is_known_routing_template(g.template.as_deref()))
            })
            .ok_or_else(|| {
                "no complete routing read (template missing from every scene doc) — \
                 can't classify scene routing safely"
                    .to_string()
            })?;
        // Preset-wide un-levelable routing (unknown template / mic / split-output) stops the
        // amp path for the whole preset. Per-SCENE issues below become skip jobs so one bad
        // scene doesn't abort the batch.
        check_levelable_routing(&structure)?;
        Ok(structure)
    })();
    if handles.is_empty() {
        if let Err(e) = &amp_prereq {
            return Err(e.clone());
        }
    }
    let jobs = scene_slots
        .iter()
        .map(|scene| {
            let doc: &serde_json::Value = docs
                .iter()
                .find(|(s2, _)| s2 == scene)
                .and_then(|(_, d)| d.as_ref())
                .unwrap_or(&NULL_DOC);
            let scene_slot = if *scene >= session::BASE_SCENE_SLOT {
                None
            } else {
                Some(*scene)
            };
            if let Some((_, h)) = handles.iter().find(|(s2, _)| s2 == scene) {
                return handle_scene_job(*scene, scene_slot, target_lufs, h, doc, saved_fallback);
            }
            let classified = match &amp_prereq {
                Ok(structure) => classify_scene_knobs(structure, doc, candidates),
                Err(reason) => Err(reason.clone()),
            };
            match classified {
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
                    // PLAN-TIME twin of `scene_write_verdict`'s `Unknown => Refuse`. A row whose
                    // SAVED overlay can't be classified is one the write path would refuse
                    // anyway — but leaving it in the batch until then is not merely late, it is
                    // SILENT: `plan_trade_for_batch` scores it `benefits: false` (via
                    // `benefits_from_base_raise`), so its deficit never reaches the trade, no
                    // headroom is bought for it, and it clamps with nothing red. Refusing here
                    // is what puts the reason on the user's row — and spares the scene an engage.
                    //
                    // Scoped to a saved doc that CLAIMS overlay authority (it carries a `scenes`
                    // array) and still can't answer. A doc with no `scenes` at all is not a
                    // failed overlay read — it is the routing-TEMPLATE fallback the oversized-
                    // audioGraph class depends on (`scene_jobs_saved_fallback_supplies_missing_
                    // template`), and `scenes` is the first section a field-8 cut takes (HW:
                    // 22/25 presets read "scenes unknown"), so refusing on its mere absence
                    // would skip almost every row for the probe/bench callers, whose saved doc
                    // is never required to be complete. The batched command path — the only one
                    // that runs the trade — reads complete-or-fail, so there a present-but-
                    // unanswerable `scenes` is genuine malformation, not truncation.
                    if let (Some(scene_idx), Some(saved)) = (scene_slot, saved_fallback) {
                        let claims_authority = saved
                            .get("scenes")
                            .and_then(|s| s.as_array())
                            .is_some_and(|a| !a.is_empty());
                        if let Some(node) =
                            knobs.iter().filter(|_| claims_authority).find_map(|kt| {
                                match &kt.knob {
                                    leveller::LevelKnob::Block { node_id, .. }
                                        if matches!(
                                            scene_overlay(saved, scene_idx, node_id),
                                            SceneOverlay::Unknown
                                        ) =>
                                    {
                                        Some(node_id.clone())
                                    }
                                    _ => None,
                                }
                            })
                        {
                            return skip_scene_job(
                                *scene,
                                target_lufs,
                                format!(
                                    "the saved preset can't answer this scene's overlay for \
                                     {node} (truncated or malformed `scenes`) — refusing rather \
                                     than leveling a row the headroom trade cannot model"
                                ),
                            );
                        }
                    }
                    let rebalanceable = kind == ParallelKind::Merged && knobs.len() >= 2;
                    leveller::SceneJob {
                        scene_slot: *scene,
                        // Stamped with the batch target; the app command overrides it per
                        // wire job for a mixed-target batch.
                        target_lufs,
                        knobs,
                        skip: None,
                        rebalanceable,
                        handle: None,
                        // Filled by `leveller::prepass_scene_ceilings` when the caller runs
                        // the reordered (measure-everything-first) run.
                        prepass: None,
                        // Base isolation (A3/A4), when this job is the base row, is stamped
                        // onto it AFTER this builder returns — see
                        // `commands::level_scenes::level_scenes_apply_batched`.
                        force_bypass: Vec::new(),
                    }
                }
                Err(reason) => skip_scene_job(*scene, target_lufs, reason),
            }
        })
        .collect();
    Ok(jobs)
}

/// A scene the batch reports as a failed outcome and moves past — never an abort.
fn skip_scene_job(scene: u32, target_lufs: f64, reason: String) -> leveller::SceneJob {
    leveller::SceneJob {
        scene_slot: scene,
        target_lufs,
        knobs: Vec::new(),
        skip: Some(reason),
        rebalanceable: false,
        handle: None,
        // A skip job is never measured, so it never carries a prepass reading.
        prepass: None,
        force_bypass: Vec::new(),
    }
}

/// One scene's job built on the USER'S OWN control. Classified off the SAVED preset (an
/// unrecognised param refuses HERE, before any device work) with the value AUTHORED IN THIS
/// SCENE as both the solve's starting point and the wet-floor anchor — the scene's own
/// overlaid value when it has one (the prepass doc already carries the overlay merged onto
/// base), never base's.
///
/// A per-row problem becomes that row's `skip`, exactly like the amp path's "no active guitar
/// amp": one unclassifiable handle must not cost the other scenes their run.
fn handle_scene_job(
    scene: u32,
    scene_slot: Option<u32>,
    target_lufs: f64,
    h: &SceneHandleSpec,
    doc: &serde_json::Value,
    saved: Option<&serde_json::Value>,
) -> leveller::SceneJob {
    let node_param = |v: &serde_json::Value| {
        crate::commands::level_footswitch::node_param_f64(v, h.node_id, h.parameter_id)
    };
    let Some(preset) = saved else {
        return skip_scene_job(
            scene,
            target_lufs,
            format!(
                "could not read the saved preset to classify {} on {} — leveling an \
                 unclassified parameter could change the sound instead of its loudness",
                h.parameter_id, h.node_id
            ),
        );
    };
    let current = node_param(doc)
        .or_else(|| node_param(preset))
        .unwrap_or(0.0) as f32;
    let target =
        match leveller::FsParamTarget::classified(preset, h.node_id, h.parameter_id, current) {
            Ok(t) => t,
            Err(refusal) => return skip_scene_job(scene, target_lufs, refusal),
        };
    let (lo, hi) = target.bounds();
    leveller::SceneJob {
        scene_slot: scene,
        target_lufs,
        knobs: vec![leveller::KnobTarget {
            knob: leveller::LevelKnob::Block {
                group_id: h.group_id.to_string(),
                node_id: h.node_id.to_string(),
                parameter_id: h.parameter_id.to_string(),
                scene_slot,
            },
            lo,
            hi,
            current,
        }],
        skip: None,
        // A user-chosen handle is one control, not a lane pair: joint-k's mix-preserving
        // rebalance has nothing to act on.
        rebalanceable: false,
        handle: Some(target),
        prepass: None,
        force_bypass: Vec::new(),
    }
}

/// The ONE field-8 saved-preset read a leveling run gets, THE source for everything the
/// saved document answers: the raw per-node scene overlays ([`scene_overlay`]) and
/// [`build_scene_jobs`]'s routing-structure fallback.
/// Read it once per preset and thread the document — never add a second read.
///
/// GAP CONTRACT (the HID open-lockout is real: every failed exclusive open resets it, so
/// hammering never recovers): this function does NOT sleep before its own read — the call
/// sites place it where nothing has just closed a session, or where the caller already slept
/// `RECONNECT_GAP_MS`; sleeping here as well doubled the gap to 800 ms, landing at the edge of
/// the lockout window instead of safely under it. It sleeps ONCE, AFTER the read, so whoever
/// opens next gets a properly-spaced session boundary. A failed read returns `None` and every
/// consumer degrades to its pre-read behaviour.
pub(crate) fn read_saved_preset(list_index: u32) -> Option<serde_json::Value> {
    let result = match crate::read_slot_preset_parsed(list_index) {
        Ok((preset, _, _)) => Some(preset),
        Err(e) => {
            log::warn!("scene jobs slot {list_index}: field-8 saved-preset read failed ({e})");
            None
        }
    };
    crate::settle(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    result
}

/// [`read_saved_preset`]'s COMPLETE-OR-FAIL sibling, for the scene LEVELING planners.
///
/// The tolerant read above is right for consumers that degrade gracefully when a section
/// is missing. A scene-leveling run is not one of them: a large preset's field-8 read is
/// tail-truncated (per-slot deterministic — re-reading never lengthens it), and the tail
/// that gets cut is `scenes`. The planner then simply does not see the last scene, so the
/// run levels the scenes it can see and silently leaves the others untouched — the user
/// reads that as "the Clean scene failed". HW-verified on the Friedman HBE (device slot
/// 28): the field-8 read returns 3 of 4 scenes, the third cut mid-record with no
/// `sceneName` and 9 of 15 nodes, and "Clean" is absent entirely.
///
/// So this routes through [`crate::read_slot_preset_complete`], which falls back to a
/// name-guarded device backup — the only transport carrying the whole document — and
/// refuses rather than returning a partial one. Same GAP CONTRACT as its sibling: no sleep
/// before the read, one `RECONNECT_GAP_MS` after it.
pub(crate) fn read_saved_preset_complete(list_index: u32) -> Result<serde_json::Value, String> {
    read_saved_preset_complete_sections(list_index, &["scenes"])
}

/// [`read_saved_preset_complete`] with an explicit required-sections list — A3's widened read
/// for a batch that carries a BASE row: the base isolation derivation additionally needs
/// `ftsw` (`doctor_force_bypass`'s own input), and a partial `ftsw` under-isolates silently
/// rather than erroring, so it must be COMPLETE-OR-FAIL exactly like `scenes` already is. Same
/// GAP CONTRACT as the sibling above (no sleep before the read, one `RECONNECT_GAP_MS` after).
pub(crate) fn read_saved_preset_complete_sections(
    list_index: u32,
    sections: &[&str],
) -> Result<serde_json::Value, String> {
    let result = crate::read_slot_preset_complete(list_index, sections).map(|(p, _, _)| p);
    crate::settle(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    result
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

/// One node's RAW scene-overlay state in one FS scene — the provenance
/// [`overlay_scene_onto_graph`] destroys (after the merge a base value is indistinguishable
/// from an overlaid one, so overlay presence can only be read from the raw scene).
///
/// Load-bearing for the scene-write rule (HW-proven, fw 1.8.45 scratch slot 30; the isolation
/// matrix is written up in `slot_write::probe_set_scene_param`): `set_node_scene_edit(node,
/// true)` RESEEDS that node's overlay from base, so it must be enabled ONLY for a node with
/// no overlay in the target scene — with an overlay present the enable WIPES the scene's
/// stored params, without one the write LEAKS TO BASE. Both mistakes corrupt the preset, so
/// "can't tell" is its own state and never collapses into `Absent`.
///
/// FOUR states, not three (HW-verified fw 1.8.45): an overlay carrying ONLY the bypass
/// family is a node whose Scene Edit flag is DISABLED — its knobs are SHARED with base, and
/// the enable-dropped write lands on BASE rather than on the overlay. That is
/// [`SceneOverlay::BypassOnly`], and it must never collapse into `Full`.
///
/// A BYPASS-ONLY overlay's whole permitted key set — the block's per-scene STATE keys;
/// NONE of these is a knob, so an overlay whose keys are a subset of these carries no
/// per-scene knob values at all (HW-verified fw 1.8.45 — see [`SceneOverlay::BypassOnly`]).
/// `bypass` is the on/off flag and `bypassType` its companion enum; `clipState`,
/// `muteInput`, `muteOutput` are the other non-knob per-block state fields
/// [`crate::footswitch::is_levelable_param`] also excludes — shared so the two exclusion
/// lists (an overlay's "carries no knobs" test and a footswitch candidate's "is this a
/// knob" test) can't drift apart: an overlay of `{bypass, clipState}` must classify
/// `BypassOnly` exactly like a bare `{bypass}` one. An EMPTY param map is a subset too and
/// classifies `BypassOnly` deliberately: nothing is overlaid, so the knobs are shared with
/// base exactly as in the flag-disabled case, and the conservative (refusing) answer is the
/// correct one. Widening this set only ever fails TOWARD `BypassOnly` (the refusing,
/// conservative direction), never away from it.
const BYPASS_ONLY_KEYS: [&str; 5] = [
    "bypass",
    "bypassType",
    "clipState",
    "muteInput",
    "muteOutput",
];

/// Consumed by `leveller::set_knobs`' Scene Edit enable decision. (The footswitch bake gate
/// in `footswitch::plan_footswitch_jobs` no longer reads scene overlays at all — the assign
/// gate (2026-08-19) decides bake-vs-assign purely off whether the switch already carries a
/// `param` fn for the selected control, so this type's other former consumer,
/// `scene_overlays_change_param`, is gone.)
pub(crate) enum SceneOverlay<'a> {
    /// The scene carries KNOB params for this node (its Scene Edit flag is ENABLED) — write
    /// WITHOUT the Scene Edit enable; the write lands on the overlay (HW). The Scene-Edit
    /// FLAG STATE alone decides the landing, not per-param containment: an enable-less write
    /// of a param this overlay does NOT yet carry still lands on the overlay, EXTENDING it
    /// per-param (HW-verified fw 1.8.45, crafted Full-partial overlay: a TubeScreamer scene-0
    /// overlay carrying `blend`/`overdrive`/`tone` but not `level`, base `level` 0.65 — an
    /// enable-less `changeParameter(level, 0.22)` landed IN the overlay, every sibling param
    /// survived unchanged with no reseed, base stayed 0.65, other scenes untouched).
    Full(&'a serde_json::Map<String, serde_json::Value>),
    /// The overlay exists but carries ONLY the bypass family ([`BYPASS_ONLY_KEYS`]) — the
    /// block's Scene Edit flag is DISABLED, so this scene SHARES the node's knobs with the
    /// base preset. HW-verified fw 1.8.45: a scene-context param write with the enable
    /// dropped against such a node lands on BASE (measured: base gain 2.5 → 7.0, the
    /// bypass-only overlay unchanged, other scenes' full overlays untouched). Neither write
    /// shape is safe — enabling reseeds, omitting leaks to base — so a scene-scoped knob
    /// write must REFUSE, exactly like [`SceneOverlay::Unknown`], but with a cause the user
    /// can act on. Carries the params so the bypass-family gates can still read them.
    BypassOnly(&'a serde_json::Map<String, serde_json::Value>),
    /// The scene exists and carries no entry for this node — the enable is what materialises
    /// the overlay, so it is REQUIRED here.
    Absent,
    /// Presence unknown: a base slot (base has no overlay concept) or a truncated field-8
    /// read (`scenes` sits at the document tail, so a cut takes it first — HW: 22/25 presets
    /// read "scenes unknown"). Neither write shape is safe; the caller must refuse.
    Unknown,
}

/// Read `scenes[scene].{guitarNodes,micNodes}.<group>.<FenderId|nodeId>.dspUnitParameters`
/// for `node` (its `nodeId` or `FenderId`) out of a saved (field-8) preset. Pure — no device
/// I/O. See [`SceneOverlay`] for why the three states must stay distinct.
pub(crate) fn scene_overlay<'a>(
    preset: &'a serde_json::Value,
    scene: u32,
    node: &str,
) -> SceneOverlay<'a> {
    // String-keyed wrapper: resolve the roster triple, then defer to the resolved-roster
    // variant. Callers that ALREADY hold `(group, node_id, fender_id)` — the per-scene
    // pickers, which walk `audiograph::roster` once for the whole preset — call
    // [`scene_overlay_for`] directly instead of paying a fresh whole-graph roster walk per
    // (scene, node) pair.
    let Some((group, node_id, fender_id)) = crate::audiograph::roster_entry(preset, node) else {
        // No roster hit is only meaningful once the scene body itself is readable: a base
        // slot or a truncated `scenes` tail must still answer `Unknown`, not `Absent`.
        return scene_body(preset, scene).map_or(SceneOverlay::Unknown, |_| SceneOverlay::Absent);
    };
    scene_overlay_for(preset, scene, (&group, &node_id, &fender_id))
}

/// One PARAM's read out of a scene overlay — [`SceneOverlay`] narrowed to a single key, so the
/// two scene-witness comparators (`leveller::scene_overlay_witness_value`'s Fix-3 match-only
/// read, `leveller::persisted_value`'s scene arm) collapse the same four overlay states the same
/// way instead of each hand-rolling the `Full`/`BypassOnly` param lookup.
pub(crate) enum SceneParamRead<'a> {
    /// The overlay carries `param`.
    Value(&'a serde_json::Value),
    /// The overlay exists (or the scene has none for this node) but not for `param` — a genuine
    /// miss, from [`SceneOverlay::Full`]/[`SceneOverlay::BypassOnly`] without the key, or
    /// [`SceneOverlay::Absent`].
    Absent,
    /// Presence unanswerable ([`SceneOverlay::Unknown`]) — a base slot or a truncated field-8
    /// read.
    Unknown,
}

/// [`scene_overlay`] narrowed to one `param`'s raw JSON value via [`SceneParamRead`]. Pure — no
/// device I/O; same node-id-string signature as [`scene_overlay`] (a caller already holding the
/// resolved roster triple should go through [`scene_overlay_for`] instead, as `scene_overlay`
/// itself documents).
pub(crate) fn overlay_param<'a>(
    preset: &'a serde_json::Value,
    scene: u32,
    node: &str,
    param: &str,
) -> SceneParamRead<'a> {
    match scene_overlay(preset, scene, node) {
        SceneOverlay::Full(params) | SceneOverlay::BypassOnly(params) => params
            .get(param)
            .map_or(SceneParamRead::Absent, SceneParamRead::Value),
        SceneOverlay::Absent => SceneParamRead::Absent,
        SceneOverlay::Unknown => SceneParamRead::Unknown,
    }
}

/// The scene's raw body (`scenes[scene]`), or `None` for a base slot / a truncated read.
fn scene_body(preset: &serde_json::Value, scene: u32) -> Option<&serde_json::Value> {
    if scene >= session::BASE_SCENE_SLOT {
        return None;
    }
    preset
        .get("scenes")
        .and_then(|s| s.as_array())
        .and_then(|a| a.get(scene as usize))
        .filter(|s| s.is_object())
}

/// [`scene_overlay`] for a caller that already holds the node's resolved roster triple
/// `(group, node_id, fender_id)` — same answer, without re-walking the whole base graph per
/// lookup. The pickers resolve the roster ONCE per preset and then ask per (scene, node).
pub(crate) fn scene_overlay_for<'a>(
    preset: &'a serde_json::Value,
    scene: u32,
    (group, node_id, fender_id): (&str, &str, &str),
) -> SceneOverlay<'a> {
    let Some(body) = scene_body(preset, scene) else {
        return SceneOverlay::Unknown;
    };
    // The overlay is keyed by FenderId with a nodeId fallback (exactly like
    // `overlay_scene_onto_graph`); the node's group picks the graph.
    // Group ids are disjoint across the two graphs (G1..G7 guitar / M1..M4 mic), so the
    // group key alone picks the right one.
    let entry = ["guitarNodes", "micNodes"].iter().find_map(|graph| {
        let nodes = body.get(graph)?.get(group)?;
        nodes.get(fender_id).or_else(|| nodes.get(node_id))
    });
    match entry {
        None => SceneOverlay::Absent,
        Some(e) => match e.get("dspUnitParameters").and_then(|p| p.as_object()) {
            // Bypass-family-only ⇒ the node's Scene Edit flag is OFF and its knobs are
            // shared with base (see [`BYPASS_ONLY_KEYS`]); anything beyond that set is a
            // genuine per-scene knob overlay.
            Some(params) => {
                if params
                    .keys()
                    .all(|k| BYPASS_ONLY_KEYS.contains(&k.as_str()))
                {
                    SceneOverlay::BypassOnly(params)
                } else {
                    SceneOverlay::Full(params)
                }
            }
            // An overlay entry whose body isn't a param object is a cut read, not "no overlay".
            None => SceneOverlay::Unknown,
        },
    }
}

/// The overlay's OWN bypass verdict: `Some(bool)` when the overlay's `dspUnitParameters`
/// states one (`Full`/`BypassOnly` carrying a `bypass` key), `None` when it doesn't
/// (`Absent`, `Unknown`, or a `Full`/`BypassOnly` overlay with no `bypass` key at all) —
/// shared by [`shared_write_is_scene_local`]'s `effective_bypass` and
/// [`scene_handle_rows_scanned`]'s `enables_block` derivation, which both used to hand-write
/// this same match.
fn overlay_bypass(overlay: &SceneOverlay) -> Option<bool> {
    match overlay {
        SceneOverlay::Full(p) | SceneOverlay::BypassOnly(p) => {
            p.get("bypass").and_then(serde_json::Value::as_bool)
        }
        SceneOverlay::Absent | SceneOverlay::Unknown => None,
    }
}

/// The MUTABLE counterpart to [`scene_overlay_for`] — the write-side key resolver, so a scene
/// overlay write can never key a node differently than [`scene_overlay_for`] would read it back.
/// Locates `scenes[scene].{guitarNodes,micNodes}.<group>` (group ids are disjoint across the two
/// graphs — same rationale as [`scene_overlay_for`]'s own read) and returns the node's overlay
/// entry keyed by the SAME FenderId-first/nodeId-fallback rule: updates whichever key an
/// existing entry already uses, and only when NEITHER key is present yet inserts a fresh
/// nodeId-keyed `{"dspUnitParameters": {}}` entry. `None` for an unknown scene/group (nothing to
/// write onto) — callers must skip silently, exactly like the read side's `Absent`/`Unknown`.
///
/// `#[cfg(feature = "e2e")]`: today's only writer of a scene overlay's raw JSON is the sim's own
/// lazy-commit model (`sim_device::patch_scene_overlays`) — the real device is written over the
/// wire (`Session::change_parameter` + `setNodeSceneEdit`), never by mutating a local JSON doc.
#[cfg(feature = "e2e")]
pub(crate) fn scene_overlay_entry_mut<'a>(
    preset: &'a mut serde_json::Value,
    scene: u32,
    (group, node_id, fender_id): (&str, &str, &str),
) -> Option<&'a mut serde_json::Value> {
    let body = preset
        .get_mut("scenes")
        .and_then(|s| s.as_array_mut())
        .and_then(|a| a.get_mut(scene as usize))?;
    // A read-only pass picks WHICH graph carries `group` first, so the mutable resolution
    // below borrows `body` exactly once — a loop that both `get_mut`s and early-`return`s
    // per iteration trips the borrow checker (each iteration looks like it could still be
    // holding the previous one's borrow for lifetime `'a`).
    let graph = ["guitarNodes", "micNodes"].into_iter().find(|g| {
        body.get(*g)
            .and_then(|g| g.get(group))
            .is_some_and(|v| v.is_object())
    })?;
    let nodes = body
        .get_mut(graph)
        .and_then(|g| g.get_mut(group))
        .and_then(|g| g.as_object_mut())?;
    let key = if nodes.contains_key(fender_id) {
        fender_id
    } else {
        node_id
    };
    Some(
        nodes
            .entry(key.to_string())
            .or_insert_with(|| serde_json::json!({ "dspUnitParameters": {} })),
    )
}

/// Which refused [`SceneOverlay`] state produced a [`SceneWriteVerdict::Refuse`] — the
/// machine-readable half of the refusal, alongside its user-facing `reason` string. A caller
/// that needs to distinguish the two (the scene-handle picker's scope annotation) matches on
/// this instead of sniffing the reason text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefusedScope {
    /// [`SceneOverlay::BypassOnly`], not cleared by the audibility guard — the scene SHARES
    /// this node's knobs with base.
    SharedWithBase,
    /// [`SceneOverlay::Unknown`] — a truncated field-8 read / a base slot, presence
    /// unanswerable.
    Unknown,
}

/// Where a scene-context knob write on one node would LAND, and what the writer must send to
/// put it there. The ONE write-landing policy — every lane that writes a knob under a scene
/// (`leveller::set_knobs`, the Doctor's prescription apply) reads it here, so the four
/// [`SceneOverlay`] states can never be answered two ways.
pub(crate) enum SceneWriteVerdict {
    /// Safe to write with the Scene Edit enable DROPPED. Two cases reach here:
    /// * [`SceneOverlay::Full`], the node's Scene Edit flag is already ON. The flag
    ///   state alone decides the landing, so the write lands on the overlay even for a param
    ///   the overlay does not yet carry, and re-enabling would RESEED the overlay from base
    ///   (HW 3-cell matrix, fw 1.8.45).
    /// * A [`SceneOverlay::BypassOnly`] node whose [`shared_write_is_scene_local`]
    ///   reads `true`: the write DELIBERATELY lands on the shared BASE value, not an overlay
    ///   (there is none to land in), because the leak is confirmed audible ONLY in this scene.
    WriteDirect,
    /// No overlay for this node in this scene ([`SceneOverlay::Absent`]) — the enable is what
    /// MATERIALISES one, so `set_node_scene_edit(node, true)` is REQUIRED before the write;
    /// without it the write leaks to base. Nothing to reseed away here.
    NeedsEnable,
    /// Neither write shape is safe. `reason` is the user-facing text; `scope` is the
    /// machine-readable state behind it: [`RefusedScope::SharedWithBase`]
    /// ([`SceneOverlay::BypassOnly`], not audibility-cleared — the enable-dropped write would
    /// land on BASE and change every sharing scene, the enable would reseed) or
    /// [`RefusedScope::Unknown`] ([`SceneOverlay::Unknown`] — a truncated field-8 read / a
    /// base slot, presence unanswerable).
    Refuse { scope: RefusedScope, reason: String },
}

/// [`SceneWriteVerdict::Refuse`]'s [`SceneOverlay::BypassOnly`] wording — a shared function so
/// every refusing arm of [`scene_write_verdict_for_param`] hands out identical text, whether the
/// audibility guard never ran (no param in hand) or ran and read `false`.
fn bypass_only_refuse_message(scene: u32, node: &str) -> String {
    format!(
        "scene {scene} shares {node}'s knobs with the base preset (Scene Edit off for that \
         block) — a scene write would change every sharing scene; level Base instead"
    )
}

/// [`SceneWriteVerdict::Refuse`]'s [`SceneOverlay::Unknown`] wording, shared for the same
/// reason as [`bypass_only_refuse_message`].
fn unknown_refuse_message(scene: u32, node: &str) -> String {
    format!(
        "refusing to write {node} in scene {scene} — the saved preset does not say whether \
         that node already has a scene overlay (truncated field-8 read), and both write \
         shapes corrupt it (enable reseeds an existing overlay from base; omitting it leaks \
         the write to base)"
    )
}

/// The write-landing verdict for `node`/`param` in `scene` of the SAVED (field-8) `preset` —
/// pure, no device I/O, decided before any write so an unanswerable overlay state refuses with
/// the preset untouched. The refusal strings are user-facing and live here, next to the states
/// they describe, so the leveling and Doctor lanes tell the player the same story. The ONE
/// write-landing policy for every scene-writing lane (`leveller::set_knobs`, the Doctor's
/// prescription apply — both know the param they're about to write, so both call this form; an
/// earlier paramless `scene_write_verdict` existed for a caller with no param in hand, but every
/// real caller HAD one, so it was folded in here rather than kept as unreachable code).
///
/// [`SceneOverlay::Full`] and [`SceneOverlay::Absent`] answer exactly as the param-blind rule
/// would (`WriteDirect` / `NeedsEnable` — the landing there never depends on which param is
/// being written). The one param-SENSITIVE arm is [`SceneOverlay::BypassOnly`]: instead of an
/// outright refuse, it consults [`shared_write_is_scene_local`] and, when the leak-to-base
/// write would be audible ONLY in `scene`, allows it through as [`SceneWriteVerdict::WriteDirect`]
/// (still with NO scene-edit enable — the write is meant to land on the shared base param,
/// never reseed the overlay).
///
/// REJECTED ALTERNATIVE (do not "simplify" into it): enable Scene Edit (reseeding the overlay
/// from base), then rewrite `bypass` back to its scene value, then write the param — that would
/// leave the node as a genuine `Full` overlay instead of a shared BASE write. It was rejected
/// because a cancel landing between the bypass rewrite and the param write — plus the leveller's
/// ALWAYS-RUN post-cancel deferred-save cleanup (`danger.md`) — could persist the scene with the
/// node's `bypass` flipped OFF and the param unset: a genuine corruption window, HW-unverified
/// because nobody has forced that race on real hardware. The BASE-write shape this function
/// allows instead has no such window: it is one single write, on a param that was ALREADY
/// shared with base before this call, so a cancel mid-write leaves the same shared state the
/// scene started in, only with base's own value moved (audible nowhere but `scene`, by
/// construction of the predicate).
pub(crate) fn scene_write_verdict_for_param(
    preset: &serde_json::Value,
    scene: u32,
    node: &str,
    param: &str,
) -> SceneWriteVerdict {
    match scene_overlay(preset, scene, node) {
        SceneOverlay::Full(_) => SceneWriteVerdict::WriteDirect,
        SceneOverlay::Absent => SceneWriteVerdict::NeedsEnable,
        SceneOverlay::BypassOnly(_) => {
            if shared_write_is_scene_local(preset, scene, node, param) {
                SceneWriteVerdict::WriteDirect
            } else {
                SceneWriteVerdict::Refuse {
                    scope: RefusedScope::SharedWithBase,
                    reason: bypass_only_refuse_message(scene, node),
                }
            }
        }
        SceneOverlay::Unknown => SceneWriteVerdict::Refuse {
            scope: RefusedScope::Unknown,
            reason: unknown_refuse_message(scene, node),
        },
    }
}

/// True iff a plain (enable-dropped) leak-to-base write of `param` on `node` — the shape a
/// [`SceneOverlay::BypassOnly`] node gets, since its Scene Edit flag is off and there is no
/// overlay to land in — would be audible in `scene` and ONLY `scene`. HW-confirmed bug class
/// (preset 28 "Friedman HBE", `ACD_Boost`/`gain`): the node is bypassed in base and in every
/// OTHER scene, and only the Solo scene's bypass-only overlay flips it on, so a shared write is
/// inaudible everywhere the knobs are shared and therefore safe to send exactly as scene_write_
/// verdict would today refuse.
///
/// Effective bypass for one scene: `overlay.bypass ?? base.bypass`, applied UNIFORMLY across
/// every [`SceneOverlay`] kind (a `Full` overlay that happens to omit `bypass` inherits base
/// exactly like `Absent` and `BypassOnly` do) — an overlay can flip bypass without carrying
/// every other knob, so the family key alone decides audibility, never the overlay KIND.
///
/// True requires ALL of:
/// * `scene`'s effective bypass is `false` (the write would be audible here);
/// * base's node carries a `bypass` key and it reads `true` (a MISSING key answers `false` —
///   can't confirm the shared value is silent in base to begin with);
/// * every OTHER scene either has effective bypass `true` (the leak is silent there too), or
///   carries its OWN [`SceneOverlay::Full`] overlay that already contains `param` (a per-scene
///   knob PINS that scene against whatever base holds, insulating it from the leak).
///
/// Refuses (`false`) outright, before any of the above, when: the `scenes` array is truncated
/// (`footswitch::max_referenced_scene` names an index the array doesn't reach — the same guard
/// [`scenes_restating_base`] uses, and for the same reason: "every other scene" is unanswerable
/// with a cut tail); the base node identity is AMBIGUOUS ([`base_node_matches`] > 1 hit); or
/// [`footswitch::node_targeted_by_assign`] says the node is targeted by ANY `ftsw` entry (an
/// `"on-off"` row whose `nodes[]` contains it, or a `"param"`-function assign whose `nodeId` is
/// it) or ANY EXP/MIDI-EXP jack binding (`exp1`/`exp2`/`midiExp1`/`midiExp2`/`toe`, any `func`)
/// — those write the node over a DIFFERENT wire path this scan never models, so a node they
/// touch can be audible in ways the overlay table alone can't rule out.
fn shared_write_is_scene_local(
    preset: &serde_json::Value,
    scene: u32,
    node: &str,
    param: &str,
) -> bool {
    let Some(scenes) = preset.get("scenes").and_then(|s| s.as_array()) else {
        return false;
    };
    if scene as usize >= scenes.len() {
        return false;
    }
    if max_referenced_scene(preset).is_some_and(|m| m as usize >= scenes.len()) {
        return false;
    }
    let mut base_hits = base_node_matches(preset, node);
    if base_hits.len() != 1 {
        return false;
    }
    let Some(base_bypass) = base_hits
        .pop()
        .flatten()
        .and_then(|p| p.get("bypass").and_then(|v| v.as_bool()))
    else {
        return false;
    };
    if !base_bypass {
        return false; // already audible in base — a shared write is never scene-local
    }
    if crate::footswitch::node_targeted_by_assign(preset, node) {
        return false;
    }

    // Roster triple resolved ONCE — every per-scene overlay lookup below reuses it via
    // `scene_overlay_for` instead of re-walking the whole base graph per scene (`scene_overlay`
    // pays that walk on every call). A miss here can't happen for a real base node: `base_hits`
    // above already confirmed exactly one match by nodeId OR FenderId, and every real base node
    // carries both — but refuse rather than assume it if one ever doesn't.
    let Some((group, node_id, fender_id)) = crate::audiograph::roster_entry(preset, node) else {
        return false;
    };
    let triple = (group.as_str(), node_id.as_str(), fender_id.as_str());

    // `overlay.bypass ?? base.bypass`, the same fallback for every overlay kind — see the
    // doc comment above for why the KIND must not gate this lookup.
    let effective_bypass = |s: u32| -> bool {
        let overlay = scene_overlay_for(preset, s, triple);
        // A cut read for one of the OTHER scenes can't be trusted to answer "bypassed" —
        // fails both the bypass and the pin test below, so the whole predicate refuses.
        // `overlay_bypass` folds `Unknown` into `None` same as `Absent`, so it must be
        // special-cased here rather than falling through to the `base_bypass` fallback.
        if matches!(overlay, SceneOverlay::Unknown) {
            return false;
        }
        overlay_bypass(&overlay).unwrap_or(base_bypass)
    };
    if effective_bypass(scene) {
        return false; // not audible in `scene` itself
    }
    (0..scenes.len() as u32).all(|s| {
        s == scene || {
            let pins = matches!(
                scene_overlay_for(preset, s, triple),
                SceneOverlay::Full(p) if p.contains_key(param)
            );
            effective_bypass(s) || pins
        }
    })
}

// `max_referenced_scene` lives in `crate::footswitch` (shared with the field-8
// truncation-detection fallback in `commands::presets` — see its doc comment there).

/// Tolerance for "this overlay merely restates the base value". Params are 0..1 knob floats
/// that JSON round-trips exactly, so this only absorbs a last-bit difference.
const SCENE_PARAM_EPS: f64 = 1e-6;

/// Do two param values differ? Numeric pairs compare within [`SCENE_PARAM_EPS`], everything
/// else (bools like `bypass`, strings, a type change) by exact JSON equality.
fn values_differ(base: &serde_json::Value, overlay: &serde_json::Value) -> bool {
    match (base.as_f64(), overlay.as_f64()) {
        (Some(b), Some(o)) => (b - o).abs() > SCENE_PARAM_EPS,
        _ => base != overlay,
    }
}

/// Every BASE-graph node answering to `node` — by `nodeId` OR `FenderId`, the same two-id rule
/// [`scene_overlay`] resolves by — each with its `dspUnitParameters` (`None` = the node carries
/// none). More than one hit means the id is AMBIGUOUS, which the caller refuses outright.
fn base_node_matches(
    preset: &serde_json::Value,
    node: &str,
) -> Vec<Option<serde_json::Map<String, serde_json::Value>>> {
    let mut hits = Vec::new();
    crate::audiograph::for_each_node(preset, |obj| {
        let is_node = ["nodeId", "FenderId"]
            .iter()
            .any(|k| obj.get(*k).and_then(|v| v.as_str()) == Some(node));
        if is_node {
            hits.push(
                obj.get("dspUnitParameters")
                    .and_then(|p| p.as_object())
                    .cloned(),
            );
        }
    });
    hits
}

/// The saved document's `lastLoadedScene` (0-based scene index, or the base wire slot) —
/// the value every save path must re-stamp via a pre-save recall (`LevelOptions::
/// restore_scene` / the footswitch writer's restore param). One helper because the
/// extraction now has a call site per save-capable entry point.
pub(crate) fn last_loaded_scene(preset: &serde_json::Value) -> Option<u32> {
    preset
        .get("lastLoadedScene")
        .and_then(serde_json::Value::as_u64)
        .map(|v| v as u32)
}

/// Trace when a save path's `lastLoadedScene` re-stamp is silently disarmed: `restore`
/// resolved to `None` on a preset that HAS scenes, so the save may rewrite the on-load
/// scene. Fine (and silent) for a scene-less preset. `tag` names the calling lane.
pub(crate) fn warn_missing_restore_scene(
    tag: &str,
    slot: u32,
    preset: &serde_json::Value,
    restore: Option<u32>,
) {
    if restore.is_none()
        && preset
            .get("scenes")
            .and_then(|s| s.as_array())
            .is_some_and(|s| !s.is_empty())
    {
        log::warn!(
            "{tag} slot={slot}: preset has scenes but no readable lastLoadedScene — \
             the save may re-stamp the on-load scene"
        );
    }
}

/// The scenes whose overlay CARRIES `param` on `node` with the BASE value (within
/// [`SCENE_PARAM_EPS`]) — the safe targets for a bake MIRROR write. A device-authored
/// full-param overlay MASKS base (HW, Hiwatt slot 31: scene overlays governed the DSP while
/// base stayed untouched), so a baked base value is inert in any scene whose overlay restates
/// the param — mirroring the solved value there makes the bake effective, and is loss-free
/// because the overlay held exactly the base value. A scene that authored its OWN value is
/// NEVER mirrored (the divergence is intent — e.g. the Hiwatt's "Base Scene" mutes its trem
/// with `level: 0.0`), and a scene whose overlay omits the param inherits base, so there is
/// nothing to write. Guards are conservative the same way: `scenes` absent, a truncated
/// entry, an array cut SHORT of the scenes the document still references
/// ([`max_referenced_scene`]), or an AMBIGUOUS node identity all answer empty — a mirror is
/// an optimization, never worth a blind write.
pub(crate) fn scenes_restating_base(
    preset: &serde_json::Value,
    node: &str,
    param: &str,
) -> Vec<u32> {
    let Some(scenes) = preset.get("scenes").and_then(|s| s.as_array()) else {
        return Vec::new();
    };
    if max_referenced_scene(preset).is_some_and(|m| m as usize >= scenes.len()) {
        return Vec::new();
    }
    let mut base = base_node_matches(preset, node);
    if base.len() > 1 {
        return Vec::new();
    }
    let Some(base_v) = base.pop().flatten().and_then(|b| b.get(param).cloned()) else {
        return Vec::new();
    };
    (0..scenes.len() as u32)
        .filter(|&scene| match scene_overlay(preset, scene, node) {
            SceneOverlay::Full(params) => params
                .get(param)
                .is_some_and(|overlay| !values_differ(&base_v, overlay)),
            // CALL-SITE DECISION (three-state split): a BypassOnly overlay needs NO mirror
            // write. Its Scene Edit flag is off, so the scene READS the base value for every
            // knob — it already follows base for `param` and therefore inherits the bake
            // automatically. That is the pleasant consequence of sharing, not a gap: writing
            // a mirror there would be the leak-to-base write this split exists to prevent.
            // (Structurally it also cannot match: `param` is never a bypass-family key —
            // `footswitch::is_levelable_param` excludes those — so the map lookup would miss
            // anyway. The arm is explicit so the REASON survives, not just the outcome.)
            SceneOverlay::BypassOnly(_) | SceneOverlay::Absent | SceneOverlay::Unknown => false,
        })
        .collect()
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
///
/// `saved` is the caller's COMPLETE saved preset: the amp roster the scene-doc repair scan
/// checks every live doc against, and the answer it splices in. `None` = no repair.
pub(crate) fn prepass_scene_docs_via(
    slot: u32,
    scene_slots: &[u32],
    use_overlay: bool,
    saved: Option<&serde_json::Value>,
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
    // Freshness barrier: this is the run_batched (live-branch) prepass load — a same-slot
    // scene-leveling run started shortly after this preset's own earlier deferred-scene save
    // could otherwise materialize the PRE-save doc here (`leveller::ensure_fresh_load`'s own
    // doc has the HW evidence). No-op when the slot has no pending save in the registry.
    crate::leveller::ensure_fresh_load(slot, &mut || crate::op_aborted())?;
    let (mut docs, restore) = prepass_scene_docs(slot, scene_slots)?;
    if let Some(preset) = saved {
        backfill_scene_docs_from_saved(slot, preset, &mut docs);
    }
    Ok((docs, restore))
}

/// The scenes whose doc cannot answer EVERY amp's bypass — the same [`answers_all_amp_bypasses`]
/// [`classify_scene_knobs`] refuses on. Keyed on the AMP question, not on "the doc is `Null`":
/// a doc cut *after* some nodes but *before* the amps is a live partial that reads as present
/// and would slip past a null check while producing the identical wrong-amp fallback.
///
/// The amp roster comes from `structure`, the caller's COMPLETE saved graph — never from the
/// docs themselves: a live doc cut between two amps lists one amp and would answer for
/// itself. A graph with no amps yields an empty list and the classifier's own error stands.
pub(crate) fn scenes_missing_amp_bypass(
    structure: &session::ActiveGraph,
    docs: &[(u32, Option<serde_json::Value>)],
) -> Vec<u32> {
    let amps = amp_nodes(structure);
    if amps.is_empty() {
        return Vec::new();
    }
    docs.iter()
        .filter(|(_, d)| {
            d.as_ref()
                .is_none_or(|doc| !answers_all_amp_bypasses(&amps, doc))
        })
        .map(|(s, _)| *s)
        .collect()
}

/// Splice the SAVED preset's answer into every scene doc that cannot answer the amp question,
/// in place. `false` = nothing needed it, or the preset could not answer either. Pure —
/// [`backfill_scene_docs_from_saved`] owns the device read.
///
/// Repairs an unanswerable doc; never overrides an answerable one, and never substitutes base.
/// Takes the unanswerable scenes as an argument rather than rescanning: the device caller
/// scans first to decide whether the saved read is worth paying for at all, and a second scan
/// here could only ever produce the same list.
pub(crate) fn repair_scene_docs_from(
    docs: &mut [(u32, Option<serde_json::Value>)],
    preset: &serde_json::Value,
    needy: &[u32],
) -> bool {
    if needy.is_empty() {
        return false;
    }
    // Ask for ONLY the scenes that need repair: `scene_docs_from_saved` is all-or-nothing, so
    // passing the full roster would let one unreadable scene discard the answer for the rest.
    let Some((repaired, _)) = scene_docs_from_saved(preset, needy) else {
        return false;
    };
    let mut filled = false;
    for (scene, doc) in repaired {
        if let Some(slotted) = docs.iter_mut().find(|(s, _)| *s == scene) {
            filled |= doc.is_some();
            slotted.1 = doc;
        }
    }
    filled
}

/// Repair, in place, every live scene doc that cannot answer the amp question, from the
/// caller's COMPLETE saved preset (`prepass_scene_docs_via`'s `saved`): its graph is the amp
/// roster the scan checks each doc against, and its scene overlays are the answer spliced
/// in. Pure apart from the log lines; `repair_scene_docs_from` is the splice.
fn backfill_scene_docs_from_saved(
    slot: u32,
    preset: &serde_json::Value,
    docs: &mut [(u32, Option<serde_json::Value>)],
) {
    let structure = session::extract_active_graph(preset, None);
    let needy = scenes_missing_amp_bypass(&structure, docs);
    if needy.is_empty() {
        return;
    }
    if !repair_scene_docs_from(docs, preset, &needy) {
        log::warn!(
            "scene-doc repair: slot {slot} saved preset could not answer scenes {needy:?} — \
             they will be skipped, not guessed"
        );
    } else {
        log::info!(
            "scene-doc repair: slot {slot} filled scenes {needy:?} from the saved preset (the live \
             prepass pushed no usable amp state for them)"
        );
    }
}

// ───────────────────── Scene handle picker (enumeration) ─────────────────────
// Moved here from `commands::level_scenes` (the IPC command layer): `scene_handle_rows`'s
// whole dependency mass — `scene_overlay`, `scene_write_verdict_for_param`, `SceneOverlay` —
// already lives in this module, and `backup_read::read_backup_archive` calls
// `base_handle_candidates_scanned`/`scene_handle_rows` straight off the backup scan with no command
// or device involved, so it has no business reaching into the command layer for an
// algorithm. `list_scene_level_handles` (the `#[tauri::command]` wrapper) stays in
// `commands::level_scenes`; both it and `backup_read` keep addressing everything below
// unqualified via the crate-root re-export (`crate::scene_handle_rows`, `crate::SceneHandleRow`, …).

/// One control a scene row could be leveled on, with the two annotations the picker cannot
/// derive on its own. `class`/`range` come from [`crate::param_class`]; `current` is the
/// value AUTHORED IN THAT SCENE (overlay if present, else base).
///
/// Block DISPLAY info is `groupId`/`nodeId`/`fenderId` only — the frontend owns the
/// friendly name (it has the models catalog; the backend deliberately does not, the same
/// split as the amp candidates the Level view already sends down).
#[derive(serde::Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
// `pub`, not `pub(crate)`: `backup_read::BackupPresetRow.base_handles` (a `pub` field, like
// its `amp_candidates`/`blocks` siblings) embeds this type, and a less-visible type inside a
// more-visible field is a hard warning (`private_interfaces`) — same reason `LevelBlockArg`
// is `pub` rather than `pub(crate)`. The command layer that actually SENDS this over IPC
// still only reaches it via `pub(crate)` command fns, so nothing outside the crate can name
// this type through them; only the backup-scan re-export needed the bump.
pub struct SceneHandleCandidate {
    pub(crate) group_id: String,
    pub(crate) node_id: String,
    pub(crate) fender_id: String,
    pub(crate) parameter_id: String,
    /// `"level_linear" | "level_db" | "wet_mix"` — the classifier's verdict, serialized
    /// straight off [`param_class::ParamClass`] (no hand-rolled mapping to drift from the
    /// table). On the `candidates` list `"other"` never appears — `level_candidates_for_node`
    /// admits only params the classifier recognises. On `allCandidates` it CAN: that list is
    /// every numeric control of the block, annotated so the picker can sort and warn.
    pub(crate) class: param_class::ParamClass,
    /// The param's usable `[lo, hi]` — NOT `[0,1]` for every control (`ACD_Boost.gain` is
    /// raw dB over `[0, 12]`). For a `wet_mix` the LOW bound is already raised to the wet
    /// floor, so the picker shows the range the solve may actually write.
    pub(crate) range: [f32; 2],
    pub(crate) current: f32,
    /// Does writing this control in THIS scene affect ONLY this scene?
    /// * `"isolated"` — the scene carries a knob overlay for the node (or none at all, in
    ///   which case the Scene Edit enable materialises one). The write stays here. ALSO
    ///   `"isolated"` for a bypass-only node whose leak-to-base write
    ///   `shared_write_is_scene_local` confirms is audible ONLY in this scene (every other
    ///   sharing scene stays bypassed, or pins this param with its own overlay) — the write
    ///   still lands on the shared base value, but nothing ELSE it's shared with can hear it.
    /// * `"shared_with_base"` — the node's Scene Edit flag is OFF (a bypass-only overlay)
    ///   and the leak above is NOT confirmed scene-local, so this scene reads the BASE knob
    ///   and a write would audibly change another sharing scene too. `set_knobs` REFUSES
    ///   such a write, so the picker must warn rather than offer it silently.
    /// * `"unknown"` — the saved read could not answer (a truncated `scenes` tail). Both
    ///   write shapes corrupt from there, so the write is refused too.
    pub(crate) scope: String,
    /// `"full"` = the control has room in both directions. `"lowers_only"` = its authored
    /// value already sits at (or within a whisker of) the top of its range, so this handle
    /// can only make the scene QUIETER — the picker should say so before the user finds out
    /// from a clamped row.
    pub(crate) headroom: String,
    /// Issue 5 (Boost preselect): does THIS scene turn the block on? `true` iff the BASE graph
    /// has the node bypassed AND this scene's own overlay un-bypasses it (`Absent` — sharing
    /// base's bypass state — never counts, only a positive `bypass: false` in the scene's own
    /// `Full`/`BypassOnly` overlay does) AND the node has at least one leveling candidate
    /// (mirrors [`footswitch::is_levelable_param`]'s gate, so this can never be true for a
    /// node the safe-default picker wouldn't offer at all). A signal for the frontend's
    /// preselect ONLY — it plays no part in `scope`/`headroom` or any solve decision. Always
    /// `false` on [`base_handle_candidates_scanned`]'s output: Base has no overlay/scene
    /// concept to enable anything relative to.
    pub(crate) enables_block: bool,
}

/// The handle candidates for ONE scene.
#[derive(serde::Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
// `pub`: see `SceneHandleCandidate`'s doc — `backup_read::BackupPresetRow.scene_handles`
// embeds this type in a `pub` field.
pub struct SceneHandleRow {
    /// 0-based `scenes[]` wire index. FS scenes only — base handles are the preset lane's
    /// own picker (`list_level_blocks`), and the overlay annotations below are meaningless
    /// for base (it has no overlay concept).
    pub(crate) scene_slot: u32,
    pub(crate) candidates: Vec<SceneHandleCandidate>,
    /// EVERY numeric control of every block in this scene, class-annotated and level-class
    /// first — the combined picker's source. A SUPERSET of `candidates`, which stays the
    /// safe-default list every pre-selection reads. Additive on the wire, so an existing
    /// consumer is unaffected.
    pub(crate) all_candidates: Vec<SceneHandleCandidate>,
}

/// Within this fraction of the range's top, a control counts as having no room to go up.
/// A knob authored at 0.995 of `[0,1]` has ~0.04 dB of boost left — reporting that as "full"
/// headroom would be a lie the user only discovers from a clamped row.
const HANDLE_TOP_EPS_FRACTION: f32 = 0.01;

/// Finish one raw [`footswitch::LevelParamCandidate`] into a wire [`SceneHandleCandidate`]:
/// class + range + headroom off [`leveller::FsParamTarget`], `scope` passed in verbatim (the
/// two callers — [`scene_handle_rows`] and [`base_handle_candidates_scanned`] — derive it differently,
/// but the class/range/headroom math is identical, so it lives here once).
fn finish_handle_candidate(
    c: footswitch::LevelParamCandidate,
    fender_id: &str,
    scope: &str,
    enables_block: bool,
) -> SceneHandleCandidate {
    let current = c.current as f32;
    let target = leveller::FsParamTarget::new(fender_id, &c.parameter_id, current);
    let (lo, hi) = target.bounds();
    let headroom = if current >= hi - (hi - lo).abs() * HANDLE_TOP_EPS_FRACTION {
        "lowers_only"
    } else {
        "full"
    };
    SceneHandleCandidate {
        group_id: c.group_id,
        node_id: c.node_id,
        fender_id: c.fender_id,
        parameter_id: c.parameter_id,
        class: target.info.class,
        range: [lo, hi],
        current,
        scope: scope.to_string(),
        headroom: headroom.to_string(),
        enables_block,
    }
}

/// One preset's node param map plus its two rosters — the graph walk [`base_handle_candidates_scanned`]
/// and [`scene_handle_rows`] used to each pay for INDEPENDENTLY (a full `audiograph::for_each_node`
/// pass for the param map, a full `audiograph::roster` pass for the block list) on the SAME
/// preset, back-to-back, in `backup_read::read_backup_archive`'s per-preset loop. Run this
/// ONCE per preset via [`scan_node_graph`] and thread the result BY REFERENCE into
/// [`base_handle_candidates_scanned`]/[`scene_handle_rows_scanned`] instead. [`scene_handle_rows`]
/// (unlike Base, which has no such command) stays as a plain (preset-only) convenience wrapper
/// for `list_scene_level_handles` and this module's own tests, where a second walk costs
/// nothing; it calls [`scan_node_graph`] internally and delegates to [`scene_handle_rows_scanned`].
pub(crate) struct NodeGraphScan {
    /// nodeId → `dspUnitParameters`, over BOTH graphs. A node missing a literal `nodeId`
    /// field contributes no entry — matching `audiograph::for_each_node`'s callers before
    /// this extraction, which never fell back to `FenderId` for the map key (only the
    /// roster below does).
    pub(crate) params:
        std::collections::HashMap<String, serde_json::Map<String, serde_json::Value>>,
    /// `(group, nodeId, fenderId)` for `/audioGraph/guitarNodes` ONLY — [`base_handle_candidates_scanned`]'s
    /// source. Only the guitar chain reaches the USB-Out the leveler measures — the same
    /// restriction `session::extract_level_candidates` applies for Base's device-fallback
    /// picker (`list_level_blocks`) — so a mic-node candidate must never surface in the Base
    /// picker the way it could when this walked `audiograph::roster`'s both-graphs list.
    pub(crate) guitar_roster: Vec<(String, String, String)>,
    /// Both graphs, guitar entries first then mic — the same order `audiograph::roster`
    /// returns. [`scene_handle_rows_scanned`]'s source, UNCHANGED from before this
    /// extraction: a scene's picker still offers either graph's blocks.
    pub(crate) full_roster: Vec<(String, String, String)>,
}

/// Build a [`NodeGraphScan`] in ONE pass over `preset`'s `audioGraph` — guitar nodes first
/// (populating `guitar_roster` and the shared start of `full_roster`), then mic nodes
/// (extending only `full_roster`), both in the same sorted-group order
/// `audiograph::for_each_node`/`audiograph::roster` use.
pub(crate) fn scan_node_graph(preset: &serde_json::Value) -> NodeGraphScan {
    let mut params: std::collections::HashMap<String, serde_json::Map<String, serde_json::Value>> =
        std::collections::HashMap::new();
    let mut guitar_roster = Vec::new();
    let mut full_roster = Vec::new();
    let Some(ag) = preset.get("audioGraph").and_then(|a| a.as_object()) else {
        return NodeGraphScan {
            params,
            guitar_roster,
            full_roster,
        };
    };
    for graph in ["guitarNodes", "micNodes"] {
        let Some(groups) = ag.get(graph).and_then(|g| g.as_object()) else {
            continue;
        };
        let mut keys: Vec<&String> = groups.keys().collect();
        keys.sort();
        for k in keys {
            for node in groups[k].as_array().into_iter().flatten() {
                let Some(obj) = node.as_object() else {
                    continue;
                };
                if let Some(nid) = obj.get("nodeId").and_then(|v| v.as_str()) {
                    params.insert(
                        nid.to_string(),
                        obj.get("dspUnitParameters")
                            .and_then(|p| p.as_object())
                            .cloned()
                            .unwrap_or_default(),
                    );
                }
                let fid = obj.get("FenderId").and_then(|v| v.as_str());
                let nid = obj.get("nodeId").and_then(|v| v.as_str()).or(fid);
                if let (Some(nid), Some(fid)) = (nid, fid) {
                    let row = (k.clone(), nid.to_string(), fid.to_string());
                    if graph == "guitarNodes" {
                        guitar_roster.push(row.clone());
                    }
                    full_roster.push(row);
                }
            }
        }
    }
    NodeGraphScan {
        params,
        guitar_roster,
        full_roster,
    }
}

/// Base-row handle candidates for the picker: the SAME safe candidate source Base's
/// device-fallback picker offers (`footswitch::level_candidates_for_node`, the gate behind
/// `session::extract_level_candidates`/`list_level_blocks`), annotated like a scene candidate
/// (class/range/current/headroom) but with NO overlay/scope concept — a base write can never
/// leak into another sound's overlay the way a scene's shared bypass-only write can, so every
/// candidate is unconditionally `scope: "isolated"`. PURE: no device I/O — takes an
/// already-built [`NodeGraphScan`] (see [`scan_node_graph`]) rather than a preset document, so
/// `backup_read::read_backup_archive`'s per-preset loop pays the graph walk once, shared with
/// [`scene_handle_rows_scanned`], instead of re-walking here. GUITAR-ONLY, same rationale as
/// `session::extract_level_candidates`: only the guitar chain reaches the USB-Out the leveler
/// measures, so a mic-node candidate must never surface here — enforced by consuming
/// `scan.guitar_roster`, never `scan.full_roster`.
///
/// Deliberately NOT the numeric superset (`footswitch::all_numeric_candidates_for_node`) yet —
/// Base's combined picker doesn't consume it today (unlike Scene's `allCandidates`), and the
/// per-param annotation below is real CPU (see [`scene_handle_rows`]'s doc on scan cost); add
/// it if/when a Base "all candidates" consumer exists.
///
/// No single-preset (preset-in, walk-internally) convenience wrapper exists here, unlike
/// [`scene_handle_rows`]/[`scene_handle_rows_scanned`] — there is no live command that levels
/// ONE Base picker read the way `list_scene_level_handles` does for scenes, so a wrapper would
/// be dead code outside tests. Tests call `base_handle_candidates_scanned(&scan_node_graph(&preset))`.
pub(crate) fn base_handle_candidates_scanned(scan: &NodeGraphScan) -> Vec<SceneHandleCandidate> {
    let mut out = Vec::new();
    for (group_id, node_id, fender_id) in &scan.guitar_roster {
        let Some(base_params) = scan.params.get(node_id) else {
            continue;
        };
        out.extend(
            footswitch::level_candidates_for_node(group_id, node_id, fender_id, base_params)
                .into_iter()
                // `enables_block: false` — Base has no scene/overlay concept to enable
                // anything relative to.
                .map(|c| finish_handle_candidate(c, fender_id, "isolated", false)),
        );
    }
    out
}

/// [`list_scene_level_handles`]'s pure core — the whole annotation rule, unit-testable
/// against a preset document with no device in the loop.
///
/// SCAN-COST NOTE: the `BypassOnly` arm below calls [`scene_write_verdict_for_param`] PER
/// (scene, node, param) — its own `shared_write_is_scene_local` re-walks the whole preset
/// (`base_node_matches`, `max_referenced_scene`, a fresh `roster()`) EVERY call, so it is not
/// O(1). The other three overlay states are answered directly off the `overlay` this function
/// already resolved once per (scene, node) — see `overlay_kind_scope` below — so the expensive
/// path is paid only for genuinely BypassOnly nodes, never for `Full`/`Absent`/`Unknown`. On a
/// preset whose scenes mostly toggle bypass rather than reprogram knobs (the common "scenes as
/// pedal combos" usage) this can still dominate a large preset's derivation time; the caller
/// (`backup_read::read_backup_archive`) logs the total derivation time across the whole scan so
/// a regression here is visible rather than silently eating into startup latency.
pub(crate) fn scene_handle_rows(preset: &serde_json::Value) -> Vec<SceneHandleRow> {
    scene_handle_rows_scanned(preset, &scan_node_graph(preset))
}

/// [`scene_handle_rows`]'s core, off an already-built [`NodeGraphScan`] — the shape
/// `backup_read::read_backup_archive`'s per-preset loop uses so the walk is paid once, shared
/// with [`base_handle_candidates_scanned`]. `preset` is still needed here (unlike the base
/// variant): the per-scene overlay/write-verdict resolution below reads it directly.
pub(crate) fn scene_handle_rows_scanned(
    preset: &serde_json::Value,
    scan: &NodeGraphScan,
) -> Vec<SceneHandleRow> {
    let scene_count = preset
        .get("scenes")
        .and_then(|s| s.as_array())
        .map_or(0, |a| a.len()) as u32;
    (0..scene_count)
        .map(|scene| {
            let mut candidates = Vec::new();
            let mut all_candidates = Vec::new();
            for (group_id, node_id, fender_id) in &scan.full_roster {
                let Some(base_params) = scan.params.get(node_id) else {
                    continue;
                };
                // The overlay decides which values are the scene's own — one lookup per node,
                // reused for every candidate on it. (The scope annotation is a SEPARATE,
                // per-candidate lookup below, via `scene_write_verdict_for_param`.)
                // `scene_overlay_for`, not `scene_overlay`: the roster triple is already in
                // hand, so the string-keyed wrapper's whole-graph `roster_entry` walk would
                // be re-paid once per (scene, node) pair for an answer we resolved once.
                let overlay = scene_overlay_for(preset, scene, (group_id, node_id, fender_id));
                // Issue 5 (Boost preselect): does THIS scene turn the (base-bypassed) block
                // on? Reuses `overlay` — no second bypass reader. `Absent` (the scene shares
                // base's bypass state) and `Unknown` both answer "no" here, same as a
                // positive `bypass: true`/missing key in a `Full`/`BypassOnly` overlay — only
                // an overlay that POSITIVELY un-bypasses counts.
                let base_bypassed = base_params
                    .get("bypass")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let scene_unbypasses = overlay_bypass(&overlay) == Some(false);
                // The safe-default candidate list, computed ONCE and reused both for the
                // `enables_block` gate ("has ≥1 `is_levelable_param` candidate") and for
                // `candidates` below — avoids walking `base_params` twice per node.
                let node_candidates = footswitch::level_candidates_for_node(
                    group_id,
                    node_id,
                    fender_id,
                    base_params,
                );
                let enables_block =
                    base_bypassed && scene_unbypasses && !node_candidates.is_empty();
                // Only the Full overlay's own params ever override a candidate's `current` —
                // the scope annotation below is now derived per-candidate from the write-
                // landing verdict itself, not re-derived here.
                let overlay_params = match overlay {
                    SceneOverlay::Full(p) => Some(p),
                    _ => None,
                };
                // The three overlay states OTHER than `BypassOnly` answer `scope` exactly as
                // `scene_write_verdict_for_param` would (its own doc: "`Full`/`Absent` answer
                // exactly as the param-blind rule would"; `Unknown` overlay ⇒ `Unknown` refusal
                // unconditionally) — but the verdict fn re-resolves the overlay from scratch
                // (a fresh whole-graph `roster()` walk) on every call, and only the `BypassOnly`
                // arm is genuinely PARAM-sensitive (`shared_write_is_scene_local`'s per-scene
                // pin check). Deciding it here, off the `overlay` already resolved once per
                // (scene, node), skips that expensive re-derivation for every param on every
                // non-`BypassOnly` node — see `scene_handle_rows`'s SCAN-COST doc.
                let overlay_kind_scope: Option<&'static str> = match overlay {
                    SceneOverlay::Full(_) | SceneOverlay::Absent => Some("isolated"),
                    SceneOverlay::Unknown => Some("unknown"),
                    SceneOverlay::BypassOnly(_) => None,
                };
                // ONE annotation rule, applied to both lists — the safe-default candidates
                // and the combined picker's superset must describe the same control the same
                // way, which they cannot do if each list annotates separately.
                let annotate = |mut c: footswitch::LevelParamCandidate| {
                    if let Some(v) = overlay_params
                        .and_then(|p| p.get(&c.parameter_id))
                        .and_then(|v| v.as_f64())
                    {
                        c.current = v;
                    }
                    // ONE write-landing policy, consulted PER PARAM only when the overlay kind
                    // itself doesn't already answer it: another param on the same BypassOnly
                    // node may leak audibly elsewhere even when this one doesn't, so THAT case
                    // can't be a per-node default — the verdict answers per (node, param).
                    let scope = overlay_kind_scope.unwrap_or_else(|| {
                        match scene_write_verdict_for_param(preset, scene, node_id, &c.parameter_id)
                        {
                            SceneWriteVerdict::WriteDirect | SceneWriteVerdict::NeedsEnable => {
                                "isolated"
                            }
                            SceneWriteVerdict::Refuse {
                                scope: RefusedScope::SharedWithBase,
                                ..
                            } => "shared_with_base",
                            SceneWriteVerdict::Refuse {
                                scope: RefusedScope::Unknown,
                                ..
                            } => "unknown",
                        }
                    });
                    finish_handle_candidate(c, fender_id, scope, enables_block)
                };
                candidates.extend(node_candidates.into_iter().map(annotate));
                all_candidates.extend(
                    footswitch::all_numeric_candidates_for_node(
                        group_id,
                        node_id,
                        fender_id,
                        base_params,
                    )
                    .into_iter()
                    .map(annotate),
                );
            }
            SceneHandleRow {
                scene_slot: scene,
                candidates,
                all_candidates,
            }
        })
        .collect()
}

// `pub(crate)` so sibling test modules can reuse this module's fixture builders
// (`commands::doctor_tests` shares `with_scene0_overlay` — the shape `scene_overlay`
// itself is pinned against) instead of re-typing the preset JSON.
#[cfg(test)]
#[path = "scene_jobs_tests.rs"]
pub(crate) mod scene_jobs_tests;
