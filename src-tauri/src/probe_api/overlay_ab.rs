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

use super::scene_jobs::{is_amp_model_id, prepass_scene_docs};
use crate::leveller;
use crate::read_slot_preset_parsed;
use crate::scenes;
use crate::session;
use crate::session::Session;
use serde_json::Value;

/// Overlay-effective amp state for one scene: the value the SAVED preset would produce if
/// the sparse scene overlay is applied over the base node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AmpEffective {
    pub(crate) bypass: Option<bool>,
    pub(crate) output_level: Option<f64>,
}

/// One amp node's identity in the guitar graph (enumerated from the SAVED base array).
#[derive(Debug, Clone)]
pub(crate) struct AmpNode {
    pub(crate) group: String,
    pub(crate) node_id: String,
    pub(crate) fender_id: String,
}

/// Base-node `{bypass, outputLevel}` with NO overlay (the base scene, `BASE_SCENE_SLOT`, has
/// no `scenes[]` entry — every amp plays its base params).
pub(crate) fn base_effective_amp(base_node: &Value) -> AmpEffective {
    let params = base_node
        .get("dspUnitParameters")
        .and_then(|p| p.as_object());
    AmpEffective {
        bypass: params
            .and_then(|p| p.get("bypass"))
            .and_then(Value::as_bool),
        output_level: params
            .and_then(|p| p.get("outputLevel"))
            .and_then(Value::as_f64),
    }
}

/// Overlay-effective `{bypass, outputLevel}` for ONE amp in ONE FS scene of a SAVED preset.
///
/// `base_node` = the node object under `audioGraph.guitarNodes.<group>[]`. `scene` =
/// `scenes[i]` (the caller passes `scenes.get(i)`, so `None` = the index is absent → skip).
/// A `scene` that isn't a JSON object (malformed / truncated to a non-object) → `None` too.
/// The amp is keyed in the sparse overlay by FenderId (nodeId fallback); a param the overlay
/// lacks falls through to the base-node value.
pub(crate) fn overlay_effective_amp(
    base_node: &Value,
    scene: Option<&Value>,
    group: &str,
    fender_id: &str,
    node_id: &str,
) -> Option<AmpEffective> {
    let scene = scene?; // scene index missing from scenes[] → skip
    let scene = scene.as_object()?; // malformed/truncated scene entry → skip
    let overlay_params = scene
        .get("guitarNodes")
        .and_then(|g| g.get(group))
        .and_then(|grp| grp.get(fender_id).or_else(|| grp.get(node_id)))
        .and_then(|n| n.get("dspUnitParameters"))
        .and_then(|p| p.as_object());
    let base_params = base_node
        .get("dspUnitParameters")
        .and_then(|p| p.as_object());
    // Overlay value if the overlay carries this param for this amp, else the base value.
    let pick = |key: &str| -> Option<Value> {
        overlay_params
            .and_then(|o| o.get(key))
            .or_else(|| base_params.and_then(|b| b.get(key)))
            .cloned()
    };
    Some(AmpEffective {
        bypass: pick("bypass").as_ref().and_then(Value::as_bool),
        output_level: pick("outputLevel").as_ref().and_then(Value::as_f64),
    })
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
            let (Some(node_id), Some(fender_id)) = (nid.or(fender), fender.or(nid)) else {
                continue;
            };
            let model = fender.unwrap_or(node_id);
            if is_amp_model_id(model) {
                out.push(AmpNode {
                    group: group.clone(),
                    node_id: node_id.to_string(),
                    fender_id: fender_id.to_string(),
                });
            }
        }
    }
    out
}

/// Find an amp node's base object in a (possibly re-read) preset by group + node/FenderId.
fn find_base_node<'a>(preset: &'a Value, amp: &AmpNode) -> Option<&'a Value> {
    preset
        .pointer(&format!("/audioGraph/guitarNodes/{}", amp.group))?
        .as_array()?
        .iter()
        .find(|n| {
            let id = |k: &str| n.get(k).and_then(Value::as_str);
            id("nodeId") == Some(amp.node_id.as_str())
                || id("FenderId") == Some(amp.fender_id.as_str())
        })
}

/// The LIVE amp `outputLevel` in a prepass field-3 doc (via the production `extract_level_blocks`).
fn live_output_level(doc: &Value, group: &str, node_id: &str) -> Option<f64> {
    session::extract_level_blocks(doc)
        .into_iter()
        .find(|b| b.group_id == group && b.node_id == node_id && b.parameter_id == "outputLevel")
        .map(|b| b.value as f64)
}

/// Overlay-effective for a scene slot in one context preset: base params for the base scene,
/// else the FS-scene overlay applied over the base node.
fn effective_for(preset: &Value, amp: &AmpNode, scene_slot: u32) -> Option<AmpEffective> {
    let base_node = find_base_node(preset, amp)?;
    if scene_slot >= session::BASE_SCENE_SLOT {
        return Some(base_effective_amp(base_node));
    }
    let scene = preset
        .get("scenes")
        .and_then(Value::as_array)
        .and_then(|a| a.get(scene_slot as usize));
    overlay_effective_amp(base_node, scene, &amp.group, &amp.fender_id, &amp.node_id)
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
        return Ok(Tally {
            pairs: 0,
            agree: 0,
            bypass_mismatch: 0,
            xctx_mismatch: 0,
        });
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

    let mut t = Tally {
        pairs: 0,
        agree: 0,
        bypass_mismatch: 0,
        xctx_mismatch: 0,
    };
    for (scene_slot, doc) in &docs {
        let scene_label = if *scene_slot >= session::BASE_SCENE_SLOT {
            "base".to_string()
        } else {
            scene_slot.to_string()
        };
        for amp in &amps {
            // Overlay-effective per context; ctx0 is the primary for the live compare.
            let effs: Vec<Option<AmpEffective>> = ctx_presets
                .iter()
                .map(|p| effective_for(p, amp, *scene_slot))
                .collect();
            let ov = effs[0];
            let ov_bypass = ov.and_then(|e| e.bypass);
            let ov_ol = ov.and_then(|e| e.output_level);

            // Live side from the prepass doc.
            let live_bypass = doc
                .as_ref()
                .and_then(|d| scenes::block_bypass_in_live_graph(d, &amp.group, &amp.node_id));
            let live_ol = doc
                .as_ref()
                .and_then(|d| live_output_level(d, &amp.group, &amp.node_id));

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
            let xctx = effs.iter().skip(1).any(|e| match (e, &ov) {
                (Some(later), Some(first)) => !eff_eq(later, first),
                (None, None) => false,
                _ => true,
            });
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
    let mut total = Tally {
        pairs: 0,
        agree: 0,
        bypass_mismatch: 0,
        xctx_mismatch: 0,
    };
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
