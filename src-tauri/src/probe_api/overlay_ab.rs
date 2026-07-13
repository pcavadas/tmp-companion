//! `probe --overlay-ab <slot|all> [--contexts N]` — evidence arm: does the SAVED
//! preset's per-scene overlay (`scenes[i].guitarNodes.<group>.<FenderId>.dspUnitParameters`)
//! agree with the LIVE prepass state (`prepass_scene_docs` field-3 recalls) for each amp's
//! `{bypass, outputLevel}`? `scenes.rs` documents the overlay bypass flags as HW-unstable
//! ("the same scene read opposite amp states in different read contexts"); this arm produces
//! the numbers to accept or kill replacing the live walk with the cheap overlay read.
//!
//! Two comparisons: (1) overlay-derived vs live per scene-amp (bypass mismatch = the critical
//! signal), and (2) overlay-derived across READ CONTEXTS (a field-8 read BEFORE vs AFTER the
//! prepass) — a saved read that differs between contexts is itself proof of instability,
//! independent of the live side.
//!
//! Probe-only, NON-DESTRUCTIVE: zero writes/saves, NO re-amp. Recalls (load_preset/load_scene
//! via the prepass) are the only live-state changes.

use super::scene_jobs::{is_amp_model_id, prepass_scene_docs, scene_docs_from_saved};
use crate::leveller;
use crate::read_slot_preset_parsed;
use crate::scenes;
use crate::session;
use crate::session::Session;
use serde_json::Value;

/// One amp's `{bypass, outputLevel}` extracted from a scene doc (saved-derived or live) —
/// both sides read it with the SAME production extractors (`block_bypass_in_live_graph` +
/// `extract_level_blocks`), so the compare is apples-to-apples. `None` fields = the doc
/// didn't carry that amp/param (missing/truncated read).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AmpEffective {
    pub(crate) bypass: Option<bool>,
    pub(crate) output_level: Option<f64>,
}

/// One amp node's identity in the guitar graph (enumerated from the SAVED base array).
/// `node_id` keys both production extractors (`block_bypass_in_live_graph` /
/// `extract_level_blocks`), so it is all the compare needs.
#[derive(Debug, Clone)]
pub(crate) struct AmpNode {
    pub(crate) group: String,
    pub(crate) node_id: String,
}

/// Every guitar-graph amp node in the SAVED preset's base array, in group/array order.
pub(crate) fn amp_nodes(preset: &Value) -> Vec<AmpNode> {
    let mut out = Vec::new();
    let Some(groups) = preset
        .pointer("/audioGraph/guitarNodes")
        .and_then(|g| g.as_object())
    else {
        return out;
    };
    for (group, nodes) in groups {
        let Some(nodes) = nodes.as_array() else {
            continue;
        };
        for node in nodes {
            let fender = node.get("FenderId").and_then(Value::as_str);
            let nid = node.get("nodeId").and_then(Value::as_str);
            let Some(node_id) = nid.or(fender) else {
                continue;
            };
            let model = fender.unwrap_or(node_id);
            if is_amp_model_id(model) {
                out.push(AmpNode {
                    group: group.clone(),
                    node_id: node_id.to_string(),
                });
            }
        }
    }
    out
}

/// One amp's `{bypass, outputLevel}` read from a scene doc via the production extractors
/// (`scenes::block_bypass_in_live_graph` + `session::extract_level_blocks`). Used for BOTH
/// the saved-derived docs (from `scene_docs_from_saved`) and the live prepass docs, so the
/// A/B compares like with like. A `None` doc (scene absent from the read) → all-`None`.
fn amp_state(doc: Option<&Value>, group: &str, node_id: &str) -> AmpEffective {
    let Some(doc) = doc else {
        return AmpEffective {
            bypass: None,
            output_level: None,
        };
    };
    AmpEffective {
        bypass: scenes::block_bypass_in_live_graph(doc, group, node_id),
        output_level: session::extract_level_blocks(doc)
            .into_iter()
            .find(|b| {
                b.group_id == group && b.node_id == node_id && b.parameter_id == "outputLevel"
            })
            .map(|b| b.value as f64),
    }
}

/// The scene doc for one slot out of a `scene_docs_from_saved`/prepass doc set.
fn doc_for(docs: &[(u32, Option<Value>)], scene_slot: u32) -> Option<&Value> {
    docs.iter()
        .find(|(s, _)| *s == scene_slot)
        .and_then(|(_, d)| d.as_ref())
}

const OL_EPS: f64 = 1e-3;

/// Two effective states equal within the outputLevel epsilon (both-`None` counts equal).
fn eff_eq(a: &AmpEffective, b: &AmpEffective) -> bool {
    let ol_eq = match (a.output_level, b.output_level) {
        (Some(x), Some(y)) => (x - y).abs() <= OL_EPS,
        (None, None) => true,
        _ => false,
    };
    a.bypass == b.bypass && ol_eq
}

fn fmt_bypass(b: Option<bool>) -> String {
    b.map_or_else(|| "?".to_string(), |v| v.to_string())
}
fn fmt_ol(v: Option<f64>) -> String {
    v.map_or_else(|| "?".to_string(), |v| format!("{v:.4}"))
}

/// Running tallies across the compared scene-amp pairs.
#[derive(Default)]
struct Tally {
    pairs: u32,
    agree: u32,
    bypass_mismatch: u32,
    xctx_mismatch: u32,
}

/// Compare one preset's SAVED overlay vs the LIVE prepass, plus saved-vs-saved across
/// contexts. Appends table lines to `out` and returns running tallies.
fn compare_preset(list_index: u32, contexts: u32, out: &mut String) -> Result<Tally, String> {
    // Context 0: the field-8 read BEFORE the live prepass.
    let (p0, _, len0) = read_slot_preset_parsed(list_index)?;
    let scene_count = p0
        .get("scenes")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    let amps = amp_nodes(&p0);
    let mut ctx_presets = vec![p0];

    let device_slot = list_index + 1;
    *out += &format!(
        "\n=== slot={device_slot} (list_index={list_index})  {scene_count} FS scene(s), {} amp(s), saved read {len0}B ===\n",
        amps.len(),
    );
    if amps.is_empty() {
        *out += "  (no guitar amps — nothing to compare)\n";
        return Ok(Tally::default());
    }

    // Scene slots to compare: every FS scene + the base scene.
    let mut scene_slots: Vec<u32> = (0..scene_count as u32).collect();
    scene_slots.push(session::BASE_SCENE_SLOT);

    // The live prepass (its own session; recalls every scene once).
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    let (docs, _) = prepass_scene_docs(list_index, &scene_slots)?;

    // Remaining contexts: field-8 reads AFTER the prepass (each its own session + gap).
    for _ in 1..contexts.max(1) {
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
        match read_slot_preset_parsed(list_index) {
            Ok((p, _, _)) => ctx_presets.push(p),
            Err(e) => *out += &format!("  [ctx read after prepass FAILED: {e}]\n"),
        }
    }

    // Saved-side per-scene docs per context, via the SAME production seam the runner uses
    // (`scene_docs_from_saved`) — no hand-rolled overlay merge. A context whose field-8 read
    // is truncated (missing scene/audioGraph) yields no docs → every amp reads `None` there
    // (surfaced as "?"), never a partial silent-wrong answer.
    let saved_docs: Vec<Vec<(u32, Option<Value>)>> = ctx_presets
        .iter()
        .map(|p| {
            scene_docs_from_saved(p, &scene_slots)
                .map(|(d, _)| d)
                .unwrap_or_default()
        })
        .collect();

    let mut t = Tally::default();
    for (scene_slot, live_doc) in &docs {
        let scene_label = if *scene_slot >= session::BASE_SCENE_SLOT {
            "base".to_string()
        } else {
            scene_slot.to_string()
        };
        for amp in &amps {
            // Saved-derived state per context; ctx0 is the primary for the live compare.
            let effs: Vec<AmpEffective> = saved_docs
                .iter()
                .map(|sd| amp_state(doc_for(sd, *scene_slot), &amp.group, &amp.node_id))
                .collect();
            let ov = effs[0];
            let ov_bypass = ov.bypass;
            let ov_ol = ov.output_level;

            // Live side from the prepass doc — same extractors as the saved side.
            let live = amp_state(live_doc.as_ref(), &amp.group, &amp.node_id);
            let live_bypass = live.bypass;
            let live_ol = live.output_level;

            // Bypass agreement (the critical signal): only decidable when both sides read.
            let (agree_str, agree) = match (ov_bypass, live_bypass) {
                (Some(a), Some(b)) if a == b => ("yes", Some(true)),
                (Some(_), Some(_)) => ("NO", Some(false)),
                _ => ("?", None),
            };
            t.pairs += 1;
            match agree {
                Some(true) => t.agree += 1,
                Some(false) => t.bypass_mismatch += 1,
                None => {}
            }

            // Cross-context saved mismatch: any later context whose effective differs from ctx0.
            let xctx = effs.iter().skip(1).any(|e| !eff_eq(e, &ov));
            if xctx {
                t.xctx_mismatch += 1;
            }

            *out += &format!(
                "  slot={device_slot} scene={scene_label} amp={}/{} overlay_bypass={} live_bypass={} agree={agree_str} overlay_ol={} live_ol={}{}\n",
                amp.group,
                amp.node_id,
                fmt_bypass(ov_bypass),
                fmt_bypass(live_bypass),
                fmt_ol(ov_ol),
                fmt_ol(live_ol),
                if xctx { "  XCTX-DIFF" } else { "" },
            );
        }
    }
    Ok(t)
}

/// `probe --overlay-ab <slot|all> [--contexts N]`. `slot` is a 1-based device userSlot (as
/// `--slot-json` prints); `all` iterates every non-empty My Presets slot. Ends with a
/// guaranteed re-amp OFF for parity with the other device arms (this arm never engages it,
/// but a clean-up costs nothing and guards against a stranded prior run).
pub fn probe_overlay_ab(target: &str, contexts: u32) -> Result<String, String> {
    let list_indices: Vec<u32> = if target.eq_ignore_ascii_case("all") {
        Session::connect()?
            .list_my_presets()?
            .into_iter()
            .filter(|p| !session::is_empty_slot_name(&p.name))
            .map(|p| p.slot)
            .collect()
    } else {
        let slot: u32 = target.parse().map_err(|_| {
            format!("bad slot {target:?} — expected a 1-based device slot or 'all'")
        })?;
        if slot == 0 {
            return Err("slot is 1-based (device userSlot); use >= 1".to_string());
        }
        vec![slot - 1]
    };

    let contexts = contexts.max(1);
    let mut out = format!(
        "[overlay-ab] SAVED scene overlay vs LIVE prepass — {} preset(s), {contexts} context(s)\n",
        list_indices.len()
    );
    let mut total = Tally::default();
    for (i, &list_index) in list_indices.iter().enumerate() {
        if i > 0 {
            std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
        }
        match compare_preset(list_index, contexts, &mut out) {
            Ok(t) => {
                total.pairs += t.pairs;
                total.agree += t.agree;
                total.bypass_mismatch += t.bypass_mismatch;
                total.xctx_mismatch += t.xctx_mismatch;
            }
            Err(e) => out += &format!("\n=== list_index={list_index} FAILED: {e} ===\n"),
        }
    }

    // Guaranteed re-amp OFF (parity with the other device probe arms).
    let _ = Session::connect().and_then(|mut s| s.set_reamp_mode(false).map(|_| ()));

    out += &format!(
        "\n[overlay-ab] {}/{} scene-amp pairs agree; bypass mismatches: {}; cross-context saved mismatches: {}\n",
        total.agree, total.pairs, total.bypass_mismatch, total.xctx_mismatch,
    );
    Ok(out)
}

#[cfg(test)]
#[path = "overlay_ab_tests.rs"]
mod overlay_ab_tests;
