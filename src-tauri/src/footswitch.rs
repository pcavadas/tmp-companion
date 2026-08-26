//! Footswitch / EXP / MIDI assignment batch editor.
//!
//! OFFLINE: the assignments live at preset top-level — `ftsw` (a list of switches,
//! each a list of assignment objects: `func`, `sceneSlot`, `customLabel`, …) and
//! `exp` (a dict of jacks: `exp1`/`exp2`/`midiExp1`/`midiExp2`/`toe`). A full-overwrite
//! apply of a layout across selected presets, with firmware-defined fields only.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One block-acting function on a footswitch (a `func:"on-off"` node toggle or a
/// `func:"param"` parameter change). MIDI / amp-control / scene / looper functions are
/// excluded by the enumerator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FootswitchFn {
    pub func: String, // "on-off" | "param"
    pub group_id: String,
    pub node_id: String,
    pub fender_id: String,
    pub parameter_id: Option<String>, // param functions only
    pub value_a: Option<f64>,         // param: switch-ON value
    pub value_b: Option<f64>,         // param: switch-OFF value
    /// The assignment's own `isActive` (default false when absent) — for an
    /// on-off function, the CURRENT engaged state at save time (see
    /// [`engaged_bypass_for_switch`]'s note); carried here so a Doctor
    /// isolation derivation working from the backup scan's already-enumerated
    /// `FootswitchInfo` (no live `ftsw` JSON in hand) can replicate it.
    #[serde(default)]
    pub is_active: bool,
}

/// A continuous block parameter the leveler can solve on (a numeric `dspUnitParameter` that
/// [`crate::param_class::classify`] recognises as a LEVEL or WET/MIX control), surfaced so
/// the UI can offer a block+parameter picker per footswitch. `current` and the solve run in
/// the param's OWN units and range — no longer assumed `[0,1]`: `ACD_Boost.gain` is raw dB
/// over `[0, 12]` (HW-verified fw 1.8.45).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LevelParamCandidate {
    pub group_id: String,
    pub node_id: String,
    pub fender_id: String,
    pub parameter_id: String,
    pub current: f64, // base value (the switch-OFF / `valueB` default)
    /// The classifier's verdict for this param on this block — the wire-carried single
    /// source of truth for "how should the picker rank/label this control?". Never
    /// [`crate::param_class::ParamClass::Other`]: [`level_candidates_for_node`] admits only
    /// params the classifier recognises, so an unrecognised one is not a candidate at all.
    pub class: crate::param_class::ParamClass,
}

/// A footswitch that acts on at least one block (on/off or parameter change), with its
/// block-acting functions and the continuous parameters of those blocks the leveler can
/// target. `switch` is the `ftsw` array index (== the wire `footswitchAddress`, HW-verified).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FootswitchInfo {
    pub switch: u32,
    pub label: String,
    pub link_group: Option<u32>,
    pub functions: Vec<FootswitchFn>,
    pub level_params: Vec<LevelParamCandidate>,
    /// EVERY numeric param of the acted-on blocks, class-annotated and level-class first —
    /// the combined picker's source ([`all_numeric_candidates_for_node`]). A SUPERSET of
    /// `level_params`, which stays the safe-default/pre-selection source and keeps its
    /// "never `Other`" contract. Additive on the wire.
    pub all_params: Vec<LevelParamCandidate>,
}

/// Is `key` on block `fender_id` a leveling candidate? The gate is
/// [`crate::param_class::classify`]: a param is a candidate iff it classifies as something
/// OTHER than [`crate::param_class::ParamClass::Other`] — i.e. the table recognises it as a
/// level or wet/mix control. That table ANNOTATES rather than guesses, so an unlisted param
/// is silently excluded, which is the safe default.
///
/// No belt over the classifier: the per-scene state keys (`bypass`, `clipState`, …) are
/// absent from the table (⇒ `Other`) AND carry bool/string values (`as_f64` fails) — a
/// name list here could never change the answer. What is GONE is the old `(0.0..=1.0)`
/// value filter: it silently dropped `ACD_Boost.gain`, whose base value 2.5 is raw dB
/// (HW-verified fw 1.8.45, accepted by `changeParameter`, ~1:1 dB→LUFS) — params are no
/// longer all `[0,1]`, so the RANGE now comes from `ParamInfo.range`, never from the
/// observed value.
///
/// Returns the classifier's verdict on a hit (`Some(class)`, never `Other`) rather than a
/// bare bool, so [`level_candidates_for_node`] classifies each key exactly ONCE and reuses
/// the verdict for the candidate's `class` field instead of calling `classify` again.
fn is_levelable_param(
    fender_id: &str,
    key: &str,
    val: &Value,
) -> Option<crate::param_class::ParamClass> {
    val.as_f64()?;
    let class = crate::param_class::classify(fender_id, key).class;
    (class != crate::param_class::ParamClass::Other).then_some(class)
}

/// ONE node's leveling-candidate params, in stable (sorted-key) order — the candidate
/// builder shared by the footswitch picker ([`enumerate_block_footswitches`]) and the SCENE
/// handle picker (`commands::level_scenes::list_scene_level_handles`), so the two pickers
/// can never offer different controls for the same block. The gate is
/// [`is_levelable_param`]: the classifier table, nothing else.
pub fn level_candidates_for_node(
    group_id: &str,
    node_id: &str,
    fender_id: &str,
    params: &serde_json::Map<String, Value>,
) -> Vec<LevelParamCandidate> {
    let mut keys: Vec<&String> = params.keys().collect();
    keys.sort();
    keys.into_iter()
        .filter_map(|k| {
            let class = is_levelable_param(fender_id, k, &params[k])?;
            Some(LevelParamCandidate {
                group_id: group_id.to_string(),
                node_id: node_id.to_string(),
                fender_id: fender_id.to_string(),
                parameter_id: k.clone(),
                current: params[k].as_f64().unwrap_or(0.0),
                class,
            })
        })
        .collect()
}

/// ONE node's params for the COMBINED PICKER: **every numeric `dspUnitParameter`**, each
/// annotated with the classifier's verdict, level-class controls first and alphabetical
/// within a rank.
///
/// WHY A PARALLEL FUNCTION RATHER THAN A RELAXED [`level_candidates_for_node`]. That one is
/// the SAFE-DEFAULT source: everything that pre-selects a handle, refuses a request, or picks
/// an amp knob reads it, and it admits only controls the classifier recognises. Widening it
/// would silently let an `Other` param become a *default* pick — sweeping a control that
/// changes the effect, not the volume. This function feeds the PICKER ONLY: the user sees
/// every control the block has, annotated, and their explicit pick is authoritative — while
/// an unrecognised pick still meets [`crate::leveller::FsParamTarget::refuse_if_not_a_level_control`]
/// at solve time, which is the one gate that must not move.
///
/// `class` on a returned candidate CAN be [`crate::param_class::ParamClass::Other`] (unlike
/// [`level_candidates_for_node`]'s, which never is) — that is the annotation the picker sorts
/// and warns on. `ParamInfo.range` is meaningless for `Other`, so a consumer must read the
/// class before trusting a range.
pub fn all_numeric_candidates_for_node(
    group_id: &str,
    node_id: &str,
    fender_id: &str,
    params: &serde_json::Map<String, Value>,
) -> Vec<LevelParamCandidate> {
    let mut out: Vec<LevelParamCandidate> = params
        .iter()
        .filter_map(|(k, v)| {
            let current = v.as_f64()?;
            Some(LevelParamCandidate {
                group_id: group_id.to_string(),
                node_id: node_id.to_string(),
                fender_id: fender_id.to_string(),
                parameter_id: k.clone(),
                current,
                class: crate::param_class::classify(fender_id, k).class,
            })
        })
        .collect();
    // Stable, deterministic order: rank, then key. `sort_by` (not `sort_unstable_by`) so an
    // equal-rank pair keeps the map's own iteration order as the tiebreak fallback.
    let rank = |class: crate::param_class::ParamClass| -> u8 {
        use crate::param_class::ParamClass::*;
        match class {
            LevelLinear | LevelDb => 0,
            WetMix => 1,
            Other => 2,
        }
    };
    out.sort_by(|a, b| {
        rank(a.class)
            .cmp(&rank(b.class))
            .then_with(|| a.parameter_id.cmp(&b.parameter_id))
    });
    out
}

/// Enumerate the preset's BLOCK-ACTING footswitches (`func:"on-off"` / `func:"param"`),
/// resolving each acted-on block's `FenderId` + its leveling-candidate parameters from the
/// `audioGraph`. Switches with only scene/MIDI/amp-control/looper functions are skipped.
/// `preset` is the decoded preset JSON (carries `audioGraph` with `dspUnitParameters`).
pub fn enumerate_block_footswitches(ftsw: &Value, preset: &Value) -> Vec<FootswitchInfo> {
    // nodeId → (FenderId, &dspUnitParameters)
    let mut nodes: std::collections::HashMap<String, (String, serde_json::Map<String, Value>)> =
        std::collections::HashMap::new();
    crate::audiograph::for_each_node(preset, |obj| {
        let Some(nid) = obj.get("nodeId").and_then(Value::as_str) else {
            return;
        };
        let fid = obj
            .get("FenderId")
            .and_then(Value::as_str)
            .unwrap_or(nid)
            .to_string();
        let params = obj
            .get("dspUnitParameters")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        nodes.insert(nid.to_string(), (fid, params));
    });
    let fender_of = |nid: &str| nodes.get(nid).map(|(f, _)| f.clone()).unwrap_or_default();

    let Some(switches) = ftsw.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (sw_idx, sw) in switches.iter().enumerate() {
        let Some(assigns) = sw.as_array() else {
            continue;
        };
        let mut functions = Vec::new();
        let mut label = String::new();
        let mut link_group = None;
        // The (group, node) blocks this switch acts on — drives the level-param candidates.
        let mut acted: Vec<(String, String)> = Vec::new();
        for a in assigns {
            let func = a.get("func").and_then(Value::as_str).unwrap_or_default();
            if label.is_empty() {
                if let Some(l) = a.get("customLabel").and_then(Value::as_str) {
                    if !l.is_empty() {
                        label = l.to_string();
                    }
                }
            }
            if link_group.is_none() {
                link_group = a
                    .get("linkGroup")
                    .and_then(Value::as_u64)
                    .filter(|&g| g != 0)
                    .map(|g| g as u32);
            }
            let is_active = a.get("isActive").and_then(Value::as_bool).unwrap_or(false);
            match func {
                "on-off" => {
                    for n in a
                        .get("nodes")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        let g = n.get("groupId").and_then(Value::as_str).unwrap_or_default();
                        let nid = n.get("nodeId").and_then(Value::as_str).unwrap_or_default();
                        if nid.is_empty() {
                            continue;
                        }
                        functions.push(FootswitchFn {
                            func: "on-off".into(),
                            group_id: g.into(),
                            node_id: nid.into(),
                            fender_id: fender_of(nid),
                            parameter_id: None,
                            value_a: None,
                            value_b: None,
                            is_active,
                        });
                        acted.push((g.into(), nid.into()));
                    }
                }
                "param" => {
                    let g = a.get("groupId").and_then(Value::as_str).unwrap_or_default();
                    let nid = a.get("nodeId").and_then(Value::as_str).unwrap_or_default();
                    if nid.is_empty() {
                        continue;
                    }
                    functions.push(FootswitchFn {
                        func: "param".into(),
                        group_id: g.into(),
                        node_id: nid.into(),
                        fender_id: fender_of(nid),
                        parameter_id: a
                            .get("parameterId")
                            .and_then(Value::as_str)
                            .map(String::from),
                        value_a: a.get("valueA").and_then(Value::as_f64),
                        value_b: a.get("valueB").and_then(Value::as_f64),
                        is_active,
                    });
                    acted.push((g.into(), nid.into()));
                }
                _ => {} // scene / midi / ampcontrol / tap / tuner / mode / looper — skip
            }
        }
        if functions.is_empty() {
            continue; // not a block-acting switch
        }
        // Level-param candidates: the classifier-recognised level/wet params of each
        // acted-on block (deduped). The block's FenderId drives the classification — the
        // table's block-scoped overrides are keyed on it (`ACD_TMRumbleV3.level` is barred,
        // `ACD_Boost.gain` is raw dB), so the id must be threaded, not the param name alone.
        let mut level_params: Vec<LevelParamCandidate> = Vec::new();
        let mut seen: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        // The combined picker's superset, deduped on its own key set (a param can be a
        // candidate in both lists and must appear exactly once in each).
        let mut all_params: Vec<LevelParamCandidate> = Vec::new();
        let mut seen_all: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        for (g, nid) in &acted {
            if let Some((fid, params)) = nodes.get(nid) {
                level_params.extend(
                    level_candidates_for_node(g, nid, fid, params)
                        .into_iter()
                        .filter(|c| seen.insert((nid.clone(), c.parameter_id.clone()))),
                );
                all_params.extend(
                    all_numeric_candidates_for_node(g, nid, fid, params)
                        .into_iter()
                        .filter(|c| seen_all.insert((nid.clone(), c.parameter_id.clone()))),
                );
            }
        }
        out.push(FootswitchInfo {
            switch: sw_idx as u32,
            label,
            link_group,
            functions,
            level_params,
            all_params,
        });
    }
    out
}

// ───────────────────── Bake-vs-assign planning (preset-simplification) ─────────────────────
//
// When a footswitch turns a block ON (the block is OFF in the base preset), the leveled value
// can be written STRAIGHT onto the block (`change_parameter`) instead of as a footswitch
// `param` assignment — it's inert in the base (block bypassed) and hits target when the switch
// turns the block on, so the footswitch stays a clean on/off. A block that's ON in the base is
// part of the base sound: baking would shift "preset level", so it keeps the assignment path.

/// `bypass` state of `node_id` in the BASE graph (`dspUnitParameters.bypass`). A missing node
/// or absent `bypass` key → `false` (conservatively "not bypassed" → not bake-eligible).
pub fn block_bypassed_in_base(preset: &Value, node_id: &str) -> bool {
    let mut bypassed = false;
    crate::audiograph::for_each_node(preset, |obj| {
        if obj.get("nodeId").and_then(Value::as_str) == Some(node_id) {
            bypassed = obj
                .get("dspUnitParameters")
                .and_then(|p| p.get("bypass"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
        }
    });
    bypassed
}

/// Switch indices with an `on-off` function referencing `node_id` (by `nodes[].nodeId`). Drives
/// both "does switch S enable N" and the sole-/group-owner check. NOTE: `isActive` on an on-off
/// is the CURRENT engaged state (HW: a base-off block's switch reads `isActive=false`, a base-on
/// block's reads `true`), NOT enabled/disabled — so an on-off is an enabler regardless of it.
pub fn onoff_switches_for(ftsw: &Value, node_id: &str) -> Vec<u32> {
    let mut out = Vec::new();
    let Some(switches) = ftsw.as_array() else {
        return out;
    };
    for (i, sw) in switches.iter().enumerate() {
        let Some(assigns) = sw.as_array() else {
            continue;
        };
        let hit = assigns.iter().any(|a| {
            a.get("func").and_then(Value::as_str) == Some("on-off")
                && a.get("nodes")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .any(|n| n.get("nodeId").and_then(Value::as_str) == Some(node_id))
        });
        if hit {
            out.push(i as u32);
        }
    }
    out
}

/// The force-list replicating `switch`'s engaged state for measurement: every block its on-off
/// functions reference, in the state the block holds WHILE THE SWITCH IS ACTIVE. An on-off is
/// a latching toggle, so the saved base bypass is the engaged state only when the preset was
/// saved WITH the switch active — that's exactly what the assignment's `isActive` records
/// (HW: preset "TR+BD2+BMP" saved with its BD2 switch engaged stores BD2 ON + `isActive:true`).
/// So per assignment: engaged bypass = saved bypass when `isActive`, else the flip. The old
/// unconditional flip inverted saved-engaged switches — the Doctor forced BD2 OFF during its
/// own switch's capture and diagnosed the base sound instead. Empty when the switch has no
/// on-off — then measurement uses the base state.
pub fn engaged_bypass_for_switch(
    ftsw: &Value,
    preset: &Value,
    switch: u32,
) -> Vec<(String, String, bool)> {
    let mut out = Vec::new();
    let Some(assigns) = ftsw
        .as_array()
        .and_then(|a| a.get(switch as usize))
        .and_then(Value::as_array)
    else {
        return out;
    };
    for a in assigns {
        if a.get("func").and_then(Value::as_str) != Some("on-off") {
            continue;
        }
        // Saved-while-active ⇒ the saved state IS the engaged state; otherwise the
        // engaged state is one toggle away from saved.
        let is_active = a.get("isActive").and_then(Value::as_bool).unwrap_or(false);
        for n in a
            .get("nodes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let g = n.get("groupId").and_then(Value::as_str).unwrap_or_default();
            let nid = n.get("nodeId").and_then(Value::as_str).unwrap_or_default();
            if nid.is_empty() {
                continue;
            }
            let saved = block_bypassed_in_base(preset, nid);
            out.push((g.into(), nid.into(), if is_active { saved } else { !saved }));
        }
    }
    out
}

/// The two device states an ON/OFF delta measurement compares — what the preset sounds like with
/// `switch` ENGAGED versus DISENGAGED, as the writes each state needs. Symmetric BY
/// CONSTRUCTION: both force-lists start from the same sibling isolation
/// ([`siblings_off_excluding`]), so the ONLY difference between the two captures is this
/// switch's own effect and the measured delta is that effect alone — a "natural state"
/// disengaged capture would instead fold in whatever the other switches happened to be
/// doing. `params` carries the switch's `param` functions so a PURE-param switch (no
/// on-off) still has a measurable engaged state: engaging one jumps its param to `valueA`,
/// which no bypass list can express.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SwitchStates {
    /// `(group, node, bypass)` forcing the ENGAGED state: siblings off + this switch's own
    /// on-off nodes at their engaged bypass ([`engaged_bypass_for_switch`]).
    pub engaged_bypass: Vec<(String, String, bool)>,
    /// The same list with this switch's own nodes FLIPPED — the disengaged state.
    pub disengaged_bypass: Vec<(String, String, bool)>,
    /// One `param` function each: `(group, node, param, valueA, valueB)` — the engaged and
    /// disengaged values. A function missing either value is skipped (nothing to write).
    pub params: Vec<(String, String, String, f32, f32)>,
}

/// Derive [`SwitchStates`] for `switch` from the SAVED (field-8) preset. PURE — no device
/// I/O. `preset` supplies each on-off block's saved bypass (the `isActive`-aware engaged
/// state, see [`engaged_bypass_for_switch`]).
pub fn switch_states(ftsw: &Value, preset: &Value, switch: u32) -> SwitchStates {
    let siblings = siblings_off_excluding(ftsw, switch);
    let own = engaged_bypass_for_switch(ftsw, preset, switch);
    let mut engaged_bypass = siblings.clone();
    engaged_bypass.extend(own.iter().cloned());
    let mut disengaged_bypass = siblings;
    disengaged_bypass.extend(own.iter().map(|(g, n, b)| (g.clone(), n.clone(), !b)));
    SwitchStates {
        engaged_bypass,
        disengaged_bypass,
        params: param_fn_values(ftsw, switch),
    }
}

/// One switch's `param` functions as `(group, node, param, valueA, valueB)`. A function
/// missing either value is skipped (nothing to write) — the same rule
/// [`SwitchStates::params`] has always applied. Shared by [`switch_states`] and the
/// Doctor's FS-sound capture (which writes each `valueA` before engaging, so the
/// as-played footswitch sound includes its param-function jumps, not just on-off flips).
pub(crate) fn param_fn_values(
    ftsw: &Value,
    switch: u32,
) -> Vec<(String, String, String, f32, f32)> {
    ftsw.as_array()
        .and_then(|a| a.get(switch as usize))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|a| a.get("func").and_then(Value::as_str) == Some("param"))
        .filter_map(|a| {
            let nid = a
                .get("nodeId")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())?;
            let param = a
                .get("parameterId")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())?;
            let g = a.get("groupId").and_then(Value::as_str).unwrap_or_default();
            Some((
                g.to_string(),
                nid.to_string(),
                param.to_string(),
                a.get("valueA").and_then(Value::as_f64)? as f32,
                a.get("valueB").and_then(Value::as_f64)? as f32,
            ))
        })
        .collect()
}

/// One switch's `func:"on-off"` `(groupId, nodeId)` pairs (empty nodeIds skipped).
fn onoff_nodes(sw: &Value) -> impl Iterator<Item = (String, String)> + '_ {
    sw.as_array()
        .into_iter()
        .flatten()
        .filter(|a| a.get("func").and_then(Value::as_str) == Some("on-off"))
        .filter_map(|a| a.get("nodes").and_then(Value::as_array))
        .flatten()
        .filter_map(|n| {
            let nid = n
                .get("nodeId")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())?;
            let g = n.get("groupId").and_then(Value::as_str).unwrap_or_default();
            Some((g.to_string(), nid.to_string()))
        })
}

/// Every `(groupId, nodeId)` referenced by ANY switch's `func:"on-off"` assignments, deduped
/// (order-preserving). Drives off the raw `nodes[]` lists — NOT `isActive` (a snapshot, not
/// enable/disable; see the `onoff_switches_for` note). This is the full set of footswitch-owned
/// on/off blocks — used to force every footswitch's block OFF while isolating one switch.
pub fn all_onoff_blocks(ftsw: &Value) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let Some(switches) = ftsw.as_array() else {
        return out;
    };
    for sw in switches {
        for pair in onoff_nodes(sw) {
            if !out.contains(&pair) {
                out.push(pair);
            }
        }
    }
    out
}

/// The EXP/MIDI-EXP jack set the device carries under `exp` — the module header's documented
/// list, named once so [`node_targeted_by_assign`] can't drift from it.
const EXP_JACKS: [&str; 5] = ["exp1", "exp2", "midiExp1", "midiExp2", "toe"];

/// Every entry under `exp[jack]`, normalizing the two wire shapes a jack body takes: an array
/// of assignment objects (the real-device shape [`crate::backup_read::silence_hint`] reads), or
/// a single assignment object with no array wrapper.
fn exp_jack_entries<'a>(exp: &'a Value, jack: &str) -> Vec<&'a Value> {
    match exp.get(jack) {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(v @ Value::Object(_)) => vec![v],
        _ => Vec::new(),
    }
}

/// Does ANY footswitch (`"on-off"` node list or `"param"`-function `nodeId`) or EXP/MIDI-EXP
/// jack binding target `node`? Those are separate wire paths a knob-overlay scan (e.g.
/// `probe_api::scene_jobs::shared_write_is_scene_local`) does not model, so a node either one
/// touches must refuse an audibility guard outright rather than risk missing an audible path.
///
/// On-off rows and param-func assigns reuse the module's own enumeration seams
/// ([`all_onoff_blocks`], [`param_fn_values`]) rather than re-walking `ftsw` here. EXP/MIDI-EXP
/// jacks ([`EXP_JACKS`]: `exp1`/`exp2`/`midiExp1`/`midiExp2`/`toe`) are walked directly, since
/// nothing else in the module enumerates them: every entry (array- or object-shaped body, see
/// [`exp_jack_entries`]) counts if it names `node` via `nodeId`, regardless of its `func` — a
/// `"volume"` or `"wah"` binding is as audible a path as a `"param"` one.
pub fn node_targeted_by_assign(preset: &Value, node: &str) -> bool {
    if let Some(ftsw) = preset.get("ftsw") {
        if all_onoff_blocks(ftsw).iter().any(|(_, n)| n == node) {
            return true;
        }
        let switch_count = ftsw.as_array().map_or(0, |a| a.len()) as u32;
        if (0..switch_count).any(|sw| param_fn_values(ftsw, sw).iter().any(|(_, n, ..)| n == node))
        {
            return true;
        }
    }
    let Some(exp) = preset.get("exp") else {
        return false;
    };
    EXP_JACKS.iter().any(|jack| {
        exp_jack_entries(exp, jack)
            .iter()
            .any(|e| e.get("nodeId").and_then(Value::as_str) == Some(node))
    })
}

/// `all_onoff_blocks` minus `switch`'s OWN on-off targets, each forced to `bypass=true` (off) —
/// "every OTHER footswitch's block off", the isolation force-list for leveling one switch. The
/// excluded nodes are `switch`'s own (the caller owns them: `engaged_bypass_for_switch` forces
/// them ON, or an on-in-base block keeps its saved state), so this list is disjoint from it.
pub fn siblings_off_excluding(ftsw: &Value, switch: u32) -> Vec<(String, String, bool)> {
    let own: std::collections::HashSet<String> = ftsw
        .as_array()
        .and_then(|a| a.get(switch as usize))
        .map(|sw| onoff_nodes(sw).map(|(_, n)| n).collect())
        .unwrap_or_default();
    all_onoff_blocks(ftsw)
        .into_iter()
        .filter(|(_, nid)| !own.contains(nid))
        .map(|(g, n)| (g, n, true))
        .collect()
}

/// Derive the force-bypass isolation list OFFLINE from the backup scan's data
/// (footswitch assignments + each block's saved bypass state) — the same list
/// [`crate::doctor_force_bypass`] computes from a live field-8 preset read, but
/// walking the already-enumerated `FootswitchInfo` + a `node_id → saved bypass`
/// map (both sourced from the SAME backup-scan `presetJson`
/// [`crate::doctor_force_bypass`] would otherwise re-fetch live) instead of
/// `ftsw`/`preset` JSON — decoupled from the Doctor's own node type so this
/// lives next to its live twins (`all_onoff_blocks`/`siblings_off_excluding`/
/// `engaged_bypass_for_switch`). Mirrors `all_onoff_blocks` (base: every
/// distinct on-off `(group,node)` across all switches, dedup on first occurrence
/// in switch/function order) and `siblings_off_excluding` +
/// `engaged_bypass_for_switch` (one footswitch: every OTHER switch's on-off block
/// off, then this switch's own on-off nodes flipped to their engaged state —
/// `isActive`-aware, see `engaged_bypass_for_switch`'s doc). Order differences
/// from the live path are not a defect (the caller only ever needs the set). A
/// `node_id` missing from `saved_bypass` keeps today's `unwrap_or(false)` +
/// one-shot warn.
/// The OFFLINE twin of [`param_fn_values`], off the backup scan's already-enumerated
/// [`FootswitchInfo`] (no live `ftsw` JSON in hand): one switch's `param`-function
/// engaged writes as `(group, node, param, valueA)`. `None` (a base sound) writes
/// nothing. Same both-values rule as [`SwitchStates::params`] — a function missing
/// either value is skipped. Lives next to [`derived_force_bypass`] because the two
/// together define a footswitch SOUND: on-off flips plus param jumps.
pub fn derived_param_writes(
    footswitches: &[FootswitchInfo],
    footswitch: Option<u32>,
) -> Vec<(String, String, String, f32)> {
    let Some(sw) = footswitch else {
        return Vec::new();
    };
    footswitches
        .iter()
        .filter(|fi| fi.switch == sw)
        .flat_map(|fi| &fi.functions)
        .filter(|f| f.func == "param")
        .filter_map(|f| {
            let a = f.value_a? as f32;
            f.value_b?;
            Some((
                f.group_id.clone(),
                f.node_id.clone(),
                f.parameter_id.clone()?,
                a,
            ))
        })
        .collect()
}

pub fn derived_force_bypass(
    footswitches: &[FootswitchInfo],
    saved_bypass: &std::collections::HashMap<String, bool>,
    footswitch: Option<u32>,
) -> Vec<(String, String, bool)> {
    // Every switch's on-off (group_id, node_id), deduped on first occurrence —
    // mirrors `all_onoff_blocks`'s walk of `ftsw` in array order.
    let mut all_onoff: Vec<(String, String)> = Vec::new();
    for fi in footswitches {
        for f in fi.functions.iter().filter(|f| f.func == "on-off") {
            let pair = (f.group_id.clone(), f.node_id.clone());
            if !all_onoff.contains(&pair) {
                all_onoff.push(pair);
            }
        }
    }
    let Some(s) = footswitch else {
        return all_onoff.into_iter().map(|(g, n)| (g, n, true)).collect();
    };
    let switch_info = footswitches.iter().find(|fi| fi.switch == s);
    // Siblings: every other switch's on-off block off — excludes THIS switch's
    // own node_ids (mirrors `siblings_off_excluding`'s node_id-only exclusion set,
    // so a node shared with another switch stays excluded too).
    let own: std::collections::HashSet<&str> = switch_info
        .map(|fi| {
            fi.functions
                .iter()
                .filter(|f| f.func == "on-off")
                .map(|f| f.node_id.as_str())
                .collect()
        })
        .unwrap_or_default();
    let mut out: Vec<(String, String, bool)> = all_onoff
        .into_iter()
        .filter(|(_, n)| !own.contains(n.as_str()))
        .map(|(g, n)| (g, n, true))
        .collect();
    // This switch's own on-off nodes, flipped to their engaged state.
    let mut warned: std::collections::HashSet<&str> = std::collections::HashSet::new();
    if let Some(fi) = switch_info {
        for f in fi.functions.iter().filter(|f| f.func == "on-off") {
            let saved = saved_bypass
                .get(&f.node_id)
                .copied()
                .unwrap_or_else(|| {
                    if warned.insert(f.node_id.as_str()) {
                        log::warn!(
                            "derived_force_bypass: node {} (switch {s}) missing from the backup graph — assuming not bypassed",
                            f.node_id
                        );
                    }
                    false
                });
            out.push((
                f.group_id.clone(),
                f.node_id.clone(),
                if f.is_active { saved } else { !saved },
            ));
        }
    }
    out
}

/// Index of an existing `param` function on `switch` targeting `(node_id, param)`, if any —
/// the assignment a bake makes redundant (cleared so the bake is the single source).
pub fn existing_param_fn_index(
    ftsw: &Value,
    switch: u32,
    node_id: &str,
    param: &str,
) -> Option<u32> {
    ftsw.as_array()?
        .get(switch as usize)?
        .as_array()?
        .iter()
        .enumerate()
        .find(|(_, a)| {
            a.get("func").and_then(Value::as_str) == Some("param")
                && a.get("nodeId").and_then(Value::as_str) == Some(node_id)
                && a.get("parameterId").and_then(Value::as_str) == Some(param)
        })
        .map(|(i, _)| i as u32)
}

/// The stored engaged value (`valueA`) of switch `switch`'s existing `param` function on
/// `(node_id, param)`, if one exists — the re-run idempotency anchor (the value a prior
/// leveling run wrote). `None` for a fresh assign (no such function yet).
pub fn existing_param_fn_value_a(
    ftsw: &Value,
    switch: u32,
    node_id: &str,
    param: &str,
) -> Option<f64> {
    let i = existing_param_fn_index(ftsw, switch, node_id, param)?;
    ftsw.as_array()?
        .get(switch as usize)?
        .as_array()?
        .get(i as usize)?
        .get("valueA")
        .and_then(Value::as_f64)
}

/// What a switch's `ftsw` entry can tell a RE-MEASURE about replaying an assign's engaged
/// state — [`existing_param_fn_value_a`]'s three outcomes, kept apart.
///
/// The plain `Option` collapses two very different situations into `None`, and a re-measure
/// caller cannot act correctly on the collapsed answer:
/// * "no `param` function for this node at all" is the BAKED (or never-assigned) switch —
///   the solved value lives in the block, the engaged sound IS the saved state, and writing
///   nothing is exactly right. Every `on-off`-only switch in the Hiwatt fixture is this.
/// * "there ARE `param` functions on this node, just not the one you named" — or "the right
///   one is there but its `valueA` is unusable" — means the caller asked for a sound this
///   switch cannot produce. Replaying nothing then measures the BASE sound while the row is
///   still labelled with the switch's identity: the wrong sound under the right name, which
///   an external validator has no way to catch.
#[derive(Debug, Clone, PartialEq)]
#[cfg(any(test, feature = "e2e"))]
pub enum FsAssignAnchor {
    /// A matching `param` function with a usable `valueA` — replay this value.
    Value(f64),
    /// No `param` function targets `node_id` on this switch. Baked or never assigned:
    /// nothing to replay, and that is correct.
    NoAssignment,
    /// The switch is assign-shaped on this node but cannot produce the named sound. Carries
    /// a human-readable reason for the caller's error message.
    Mismatch(String),
}

/// Resolve `switch`'s assign anchor for `(node_id, param)` — [`existing_param_fn_value_a`]
/// with its `None` split into "nothing assigned here" vs "assigned, but not what you asked
/// for". See [`FsAssignAnchor`] for why the distinction matters.
#[cfg(any(test, feature = "e2e"))]
pub fn resolve_assign_anchor(
    ftsw: &Value,
    switch: u32,
    node_id: &str,
    param: &str,
) -> FsAssignAnchor {
    let fns: Vec<&Value> = ftsw
        .as_array()
        .and_then(|a| a.get(switch as usize))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|f| {
                    f.get("func").and_then(Value::as_str) == Some("param")
                        && f.get("nodeId").and_then(Value::as_str) == Some(node_id)
                })
                .collect()
        })
        .unwrap_or_default();
    if fns.is_empty() {
        return FsAssignAnchor::NoAssignment;
    }
    match fns
        .iter()
        .find(|f| f.get("parameterId").and_then(Value::as_str) == Some(param))
    {
        Some(f) => match f.get("valueA").and_then(Value::as_f64) {
            Some(v) => FsAssignAnchor::Value(v),
            None => FsAssignAnchor::Mismatch(format!(
                "switch {switch}'s `param` function on {node_id}.{param} has no usable \
                 numeric `valueA` (got {:?}) — its engaged value cannot be replayed",
                f.get("valueA")
            )),
        },
        None => {
            let have: Vec<&str> = fns
                .iter()
                .filter_map(|f| f.get("parameterId").and_then(Value::as_str))
                .collect();
            FsAssignAnchor::Mismatch(format!(
                "switch {switch} assigns {node_id} on {have:?}, not {param:?} — replaying \
                 nothing would measure the BASE sound under this switch's identity"
            ))
        }
    }
}

/// One leveling job's key for planning (the device-independent fields the decision needs).
pub struct FsJobKey<'a> {
    pub switch: u32,
    pub lev_node: &'a str,
    pub lev_param: &'a str,
    /// `target_lufs.to_bits()` — groups jobs that share an exact target (Case 2).
    pub target_bits: u64,
}

/// How to level one job (decided purely; the command executes it).
#[derive(Debug, Clone, PartialEq)]
pub enum FsLevelPlan {
    /// Bake the solved value into the block: force `engaged` during measurement, write
    /// `change_parameter`, and clear `clear_stale` (a now-redundant `param` fn on the switch).
    Bake {
        engaged: Vec<(String, String, bool)>,
        clear_stale: Option<u32>,
        /// Scenes whose overlay restates the base value of the leveled param
        /// ([`crate::scenes_restating_base`]) — the solved value is ALSO written into these
        /// overlays, because a full-param overlay MASKS base (the bake would otherwise be
        /// inert whenever a scene is active). Scenes that authored their OWN value are never
        /// listed — their divergence is intent and stays untouched.
        mirror_scenes: Vec<u32>,
    },
    /// Same `(node, param, target)` as job index `rep`, which bakes — no extra device work.
    BakeShared { rep: usize },
    /// Write a `param`-change assignment. `engaged` empty = measure the base state (block ON in
    /// base, today's path); non-empty = engaged measurement (the off-in-base fallback, when a
    /// scene or a second footswitch also activates the block so baking would be unsafe).
    Assign {
        engaged: Vec<(String, String, bool)>,
    },
    /// Not levelable — `String` is the progress-item message.
    Clamp(String),
}

/// Push one [`FsLevelPlan::Bake`] — or a [`FsLevelPlan::BakeShared`] pointing at the job that
/// already bakes the same `(node, param, target)`, so co-owners produce ONE device write.
///
/// `clear_stale` is always `None` here BY CONSTRUCTION and that is load-bearing, not an
/// oversight: a switch that already carries a param function for this `(node, param)` took the
/// `Assign` branch before reaching this helper (see the assign gate in
/// [`plan_footswitch_jobs`]), so there is never a stale function left to remove. Leveling only
/// ever EDITS an existing function or writes the block — it neither creates nor deletes one.
#[allow(clippy::too_many_arguments)]
fn push_bake<'j>(
    plans: &mut Vec<FsLevelPlan>,
    bake_rep: &mut std::collections::HashMap<(&'j str, &'j str, u64), usize>,
    idx: usize,
    job: &'j FsJobKey,
    _ftsw: &Value,
    preset: &Value,
    engaged: Vec<(String, String, bool)>,
) {
    let key = (job.lev_node, job.lev_param, job.target_bits);
    if let Some(&rep) = bake_rep.get(&key) {
        plans.push(FsLevelPlan::BakeShared { rep });
        return;
    }
    bake_rep.insert(key, idx);
    plans.push(FsLevelPlan::Bake {
        engaged,
        clear_stale: None,
        mirror_scenes: crate::scenes_restating_base(preset, job.lev_node, job.lev_param),
    });
}

/// Decide bake-vs-assign for every job in a batch (PURE — no device I/O). `preset` is the SAVED
/// (field-8) document — the scene gate reads its per-node overlays, so pass the read straight
/// through, never a live field-3 graph. Returns one plan per job, aligned to `jobs`.
pub fn plan_footswitch_jobs(ftsw: &Value, preset: &Value, jobs: &[FsJobKey]) -> Vec<FsLevelPlan> {
    let mut plans = Vec::with_capacity(jobs.len());
    let mut bake_rep: std::collections::HashMap<(&str, &str, u64), usize> =
        std::collections::HashMap::new();
    for (idx, job) in jobs.iter().enumerate() {
        // THE ASSIGN GATE (user directive, 2026-08-19) — the ONE thing that decides Assign vs
        // Bake, checked before every other consideration:
        //
        //   · the switch ALREADY changes the user-selected control → edit THAT function's
        //     active value (`Assign`);
        //   · it does not → NEVER add a new function to the switch. Write the user-selected
        //     control's value directly (`Bake`).
        //
        // Leveling therefore never CREATES a footswitch function. It used to, whenever baking
        // looked unsafe, and that appended a second entry to a row that already carried an
        // on-off — the shape `danger.md` forbids outright: HW bisect (fw 1.8.45, 2026-08-18)
        // proved a two-entry row makes the firmware silently replace an IMPORTED preset with
        // an EMPTY body, under the imported display name, so the list looks right indefinitely.
        // It also relabels the switch "MULTI" on the unit. Observed in the wild on the reported
        // run: switch 5 of "Plumes+BD2+OCD" gained a `param` entry (valueA 0.25) it never had.
        //
        // The gate is `existing_param_fn_index`, i.e. exactly "is there already a param function
        // on this switch for this (node, param)" — the same lookup `resolve_footswitch_job` then
        // uses to pick the function to edit, so the two cannot disagree.
        let edits_selected_control =
            existing_param_fn_index(ftsw, job.switch, job.lev_node, job.lev_param).is_some();

        if block_bypassed_in_base(preset, job.lev_node) {
            let activators = onoff_switches_for(ftsw, job.lev_node);
            if !activators.contains(&job.switch) {
                plans.push(FsLevelPlan::Clamp(
                    "block is bypassed in the base preset and this footswitch doesn't enable it"
                        .into(),
                ));
                continue;
            }
        }
        // THE ROW'S ENGAGED ISOLATION: every OTHER switch's block off (siblings) PLUS this
        // switch's own nodes at their engaged state. Disjoint by construction (siblings
        // excludes own nodes), and IDENTICAL to what `switch_states` hands the ceiling
        // prepass — pinned by `plan_on_in_base_still_asserts_its_own_block_bypass`, because
        // a plan that describes a different sound than the prepass is a solver that
        // contradicts its own feasibility check.
        //
        // A block ON in the base used to skip the second half here, on the reasoning that it
        // is part of the base sound and so needs no assertion. That is only true in a
        // one-row batch. A SIBLING row's isolation writes this block's `bypass = true` into
        // the device's working copy, and every capture's `recall_base` re-asserts that
        // mutated copy — so the row inherited the sibling's write and measured with the very
        // block it was levelling switched OFF, where its handle has no authority over
        // loudness. HW, 2026-08-19, slot 26 "Plumes+BD2+OCD": `ACD_Plumes` (the only block on
        // in base) converged when solved alone (level 0.3634 → -22.95 LUFS) and clamped in
        // every batch containing any other row, in either order, with all captures pinned at
        // -26.96 LUFS. Its prepass ceiling read -21.13 LUFS throughout.
        //
        // ponytail: accepted edge case — an always-on `param`-target block that some OTHER
        // switch's on-off also toggles gets forced off by that sibling entry while being the
        // leveled block. Exotic layout, not the reported bug; acknowledged not handled.
        let mut engaged = siblings_off_excluding(ftsw, job.switch);
        engaged.extend(engaged_bypass_for_switch(ftsw, preset, job.switch));
        if edits_selected_control {
            plans.push(FsLevelPlan::Assign { engaged });
            continue;
        }
        // Everything else BAKES — writes the user-selected control directly.
        //
        // This lane used to consult two extra gates and fall back to `Assign` when either
        // tripped: sole-ownership (another switch also activates this node) and a scene that
        // FLIPS the node's bypass (which renders the baked value in a state the leveler never
        // measured). Both are real caveats about the RESULT, but neither justifies the cure —
        // that fallback is precisely what created the forbidden second function, and it fired
        // broadly (it once sent every switch of every scened preset down the Assign path).
        // Per the directive above the value now goes to the control itself in these cases too.
        //
        // What still protects the result: `BakeShared` collapses jobs that share
        // `(node, param, target)` so co-owners resolve to ONE write rather than fighting, and
        // `mirror_scenes` carries the solved value into the scenes whose overlay RESTATES base
        // — a scene that overlays the leveled param MASKS base (HW, Hiwatt slot 31), so a bake
        // never leaks into an authored per-scene divergence.
        push_bake(&mut plans, &mut bake_rep, idx, job, ftsw, preset, engaged);
    }
    plans
}

/// Build `sceneSlot` (0-based) → footswitch index (0-based) from a preset's `ftsw`,
/// for the live-sync scene rows' data-driven FS tags. `ftsw` is the array of switches
/// (the enumerate index IS the switch number, as in [`flag_unbindable`]); a scene
/// assignment is `{func:"scene", sceneSlot, isActive}`. Only `isActive` assignments
/// map (a disabled assignment → no tag, the row shows an em-dash); first switch wins
/// on a `sceneSlot` collision (deterministic). The caller displays the human
/// footswitch number as `index + 1`.
pub fn scene_fs_map(ftsw: &Value) -> std::collections::HashMap<u32, u32> {
    let mut map = std::collections::HashMap::new();
    let Some(switches) = ftsw.as_array() else {
        return map;
    };
    // A scene stays BOUND to its footswitch even when the assignment is
    // `isActive: false` (the switch is disabled in the current layout) — the
    // device still numbers the scene, so the tag must show `FS{n}`, not "—".
    // Two passes so an ACTIVE binding always wins the slot; an inactive one
    // only fills a scene that has no active binding at all. First-wins within
    // each pass (switch order) preserves the original collision rule.
    for want_active in [true, false] {
        for (sw_idx, sw) in switches.iter().enumerate() {
            let Some(assigns) = sw.as_array() else {
                continue;
            };
            for a in assigns {
                if a.get("func").and_then(Value::as_str) != Some("scene") {
                    continue;
                }
                let is_active = a.get("isActive").and_then(Value::as_bool).unwrap_or(true);
                if is_active != want_active {
                    continue;
                }
                if let Some(slot) = a.get("sceneSlot").and_then(Value::as_u64) {
                    map.entry(slot as u32).or_insert(sw_idx as u32);
                }
            }
        }
    }
    map
}

/// The highest `scenes[]` index the document REFERENCES elsewhere: `lastLoadedScene` plus
/// every footswitch scene assignment (base — `session::BASE_SCENE_SLOT` — is not an index and
/// is excluded). A `scenes` array shorter than this is a TRUNCATED read, not a preset with
/// fewer scenes: the tolerant parse drops the cut entries and the array length alone can't
/// tell the two apart. Both references sit BEFORE `scenes` in the document (HW field-8 order:
/// `ftsw`, `lastLoadedScene`, `scenes`), so a tail cut that shortens `scenes` always leaves
/// this evidence intact.
///
/// ponytail: a scene bound to NO footswitch and not the last-loaded one is unreferenced, so a
/// cut that takes only such scenes is undetectable. Upgrade path if that matters: thread
/// `session::scene_names_from_slot_json`'s count (it recovers names from a cut document) out
/// of `read_slot_preset_parsed`, which already computes and discards it.
pub(crate) fn max_referenced_scene(preset: &Value) -> Option<u32> {
    let switch_scenes = preset.get("ftsw").map(scene_fs_map).unwrap_or_default();
    switch_scenes
        .into_keys()
        .chain(
            preset
                .get("lastLoadedScene")
                .and_then(Value::as_u64)
                .map(|v| v as u32),
        )
        .filter(|s| *s < crate::session::BASE_SCENE_SLOT)
        .max()
}

/// Overwrite the preset's footswitch layout (`ftsw`). Preset metadata untouched.
pub fn apply_ftsw(preset: &mut Value, ftsw: Value) -> Result<(), String> {
    let obj = preset.as_object_mut().ok_or("preset is not an object")?;
    obj.insert("ftsw".into(), ftsw);
    Ok(())
}

/// Overwrite the preset's expression-pedal assignments (`exp`).
pub fn apply_exp(preset: &mut Value, exp: Value) -> Result<(), String> {
    let obj = preset.as_object_mut().ok_or("preset is not an object")?;
    obj.insert("exp".into(), exp);
    Ok(())
}

/// Bulk-run operation: apply a footswitch (and optional EXP) layout.
pub struct FootswitchLayoutOp {
    pub ftsw: Value,
    pub exp: Option<Value>,
}
impl crate::bulkrun::Operation for FootswitchLayoutOp {
    fn label(&self) -> String {
        "apply footswitch layout".into()
    }
    fn transform(&self, t: &crate::bulkrun::PresetTarget) -> Result<Option<String>, String> {
        let mut v: Value =
            serde_json::from_str(&t.before_json).map_err(|e| format!("parse: {e}"))?;
        apply_ftsw(&mut v, self.ftsw.clone())?;
        if let Some(exp) = &self.exp {
            apply_exp(&mut v, exp.clone())?;
        }
        Ok(Some(serde_json::to_string(&v).map_err(|e| e.to_string())?))
    }
}

// ───────────────────── Scene context for a footswitch sound (D3) ─────────────────────

/// One switch's SCENE-CONTEXT answer for the leveling picker: which FS scenes turn this switch
/// on, and which one (if any) the UI should preselect.
///
/// PICKER-PRESELECT ONLY — the caveat travels with the shape that crosses the wire, because
/// this is what a consumer reads: both fields are derived from a DERIVED CACHE the real device
/// ignores on recall, so neither may ever decide what to WRITE (full rationale:
/// [`scene_contexts_for_switches`]'s doc).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsSceneContext {
    pub switch: u32,
    /// The 0-based `scenes[]` wire slots whose overlay ENABLES this switch, in scene order.
    pub enabling_scenes: Vec<u32>,
    /// What to preselect: `Some(i)` iff EXACTLY ONE scene enables the switch — then that scene
    /// is unambiguously the sound the player reaches by tapping it. `None` = level it in the
    /// BASE context, which is both the historical behaviour and the only defensible answer when
    /// zero scenes (nothing to infer) or several (no single right one) enable it. The user may
    /// still override to any scene, including a non-enabling one; the picker flags that.
    pub suggested: Option<u32>,
}

/// Which scenes enable each footswitch, read off `scenes[].ftswStates` — one bool per switch,
/// positionally aligned with `ftsw` (an invariant the fixture gates already assert).
///
/// PURE, and PICKER-PRESELECT ONLY. `ftswStates` is a DERIVED CACHE: the real device ignores it
/// on recall and re-derives the switch state from the scene's own block overlays
/// (`sim_device.rs` documents the same), so it must never be used to decide what to WRITE or
/// what a capture will sound like. As the source for "which scene did the player mean when they
/// wrote this switch", it is exactly right — it is what the authoring app recorded.
///
/// A scene with no readable `ftswStates` (a truncated field-8 tail takes `scenes` first)
/// contributes nothing, so a preset that cannot be read simply falls back to base — the
/// conservative side.
pub fn scene_contexts_for_switches(preset: &Value) -> Vec<FsSceneContext> {
    let switch_count = preset
        .get("ftsw")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let scenes = preset
        .get("scenes")
        .and_then(Value::as_array)
        .map_or(&[][..], |a| a.as_slice());
    (0..switch_count)
        .map(|switch| {
            let enabling_scenes: Vec<u32> = scenes
                .iter()
                .enumerate()
                .filter(|(_, sc)| {
                    sc.get("ftswStates")
                        .and_then(Value::as_array)
                        .and_then(|st| st.get(switch))
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                })
                .map(|(i, _)| i as u32)
                .collect();
            FsSceneContext {
                switch: switch as u32,
                suggested: match enabling_scenes.as_slice() {
                    [one] => Some(*one),
                    _ => None,
                },
                enabling_scenes,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3-scene / 3-switch preset: switch 0 enabled by scene 1 ALONE (the auto-detect case),
    /// switch 1 by scenes 0 and 2 (ambiguous), switch 2 by nobody.
    fn scene_context_preset() -> Value {
        serde_json::json!({
            "ftsw": [[], [], []],
            "scenes": [
                { "ftswStates": [false, true, false] },
                { "ftswStates": [true, false, false] },
                { "ftswStates": [false, true, false] }
            ]
        })
    }

    // (D3) EXACTLY ONE enabling scene auto-detects; zero or several fall back to base — the
    // only two answers that don't guess on the player's behalf.
    #[test]
    fn a_switch_enabled_by_exactly_one_scene_preselects_that_scene() {
        let rows = scene_contexts_for_switches(&scene_context_preset());
        assert_eq!(rows.len(), 3, "one row per switch");
        assert_eq!(rows[0].enabling_scenes, vec![1]);
        assert_eq!(rows[0].suggested, Some(1));
        assert_eq!(rows[1].enabling_scenes, vec![0, 2]);
        assert_eq!(
            rows[1].suggested, None,
            "two enabling scenes: no single right one"
        );
        assert!(rows[2].enabling_scenes.is_empty());
        assert_eq!(rows[2].suggested, None, "nothing enables it: base");
    }

    // A truncated / absent `scenes` tail must fall back to base, never invent a context.
    #[test]
    fn an_unreadable_scene_list_falls_back_to_base() {
        let rows = scene_contexts_for_switches(&serde_json::json!({ "ftsw": [[], []] }));
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.suggested.is_none()));
        // A scene whose `ftswStates` is missing or short contributes nothing rather than
        // shifting every later switch's answer by one.
        let short = serde_json::json!({
            "ftsw": [[], []],
            "scenes": [{ "ftswStates": [true] }, { "name": "no states" }]
        });
        let rows = scene_contexts_for_switches(&short);
        assert_eq!(rows[0].suggested, Some(0));
        assert!(rows[1].enabling_scenes.is_empty());
    }

    // The wire shape the picker reads.
    #[test]
    fn a_scene_context_row_serializes_camel_case() {
        let rows = scene_contexts_for_switches(&scene_context_preset());
        let json = serde_json::to_value(&rows[0]).expect("serialize");
        assert_eq!(json["switch"], 0);
        assert_eq!(json["enablingScenes"], serde_json::json!([1]));
        assert_eq!(json["suggested"], 1);
    }

    // ITEM 4 — THE COMBINED PICKER'S SOURCE. `all_numeric_candidates_for_node` lists EVERY
    // numeric param of the block, annotated with the classifier's verdict and with the
    // level-class controls first, while `level_candidates_for_node` — the SAFE-DEFAULT source
    // every pre-selection reads — is untouched and still admits only recognised controls.
    //
    // The split is the whole point: widening the default source would let an `Other` param
    // become a DEFAULT pick and silently sweep a control that changes the effect, not the
    // volume.
    #[test]
    fn the_combined_picker_lists_every_numeric_param_and_ranks_level_controls_first() {
        use crate::param_class::ParamClass;
        let params: serde_json::Map<String, Value> = serde_json::from_value(serde_json::json!({
            // Deliberately NOT in the answer's order, so a pass proves the sort ran.
            "tone": 0.4,
            "blend": 0.5,
            "level": 0.7,
            "bypass": false,
            "clipState": "hard",
        }))
        .expect("params");

        let all = all_numeric_candidates_for_node("G1", "n1", "ACD_TubeScreamer", &params);
        let names: Vec<&str> = all.iter().map(|c| c.parameter_id.as_str()).collect();
        assert_eq!(
            names,
            vec!["level", "blend", "tone"],
            "every NUMERIC param, level-class first then wet/mix then the rest: {all:?}"
        );
        assert!(
            all.iter().all(|c| c.node_id == "n1" && c.group_id == "G1"),
            "the node coordinates ride on every candidate: {all:?}"
        );
        // Non-numeric keys are not params a solve could ever sweep.
        assert!(
            !names.contains(&"bypass") && !names.contains(&"clipState"),
            "bool/string state keys are not candidates: {all:?}"
        );
        // The annotation is what the picker sorts and warns on — `Other` DOES appear here.
        let tone = all.iter().find(|c| c.parameter_id == "tone").expect("tone");
        assert_eq!(tone.class, ParamClass::Other);
        assert_eq!(tone.current, 0.4, "the authored value rides along");

        // THE SAFE DEFAULT IS UNCHANGED: still only classifier-recognised controls, and never
        // `Other`.
        let safe = level_candidates_for_node("G1", "n1", "ACD_TubeScreamer", &params);
        assert!(
            safe.iter().all(|c| c.class != ParamClass::Other),
            "the pre-selection source must never offer an unrecognised control: {safe:?}"
        );
        assert!(
            !safe.iter().any(|c| c.parameter_id == "tone"),
            "an `Other` param is not a default candidate: {safe:?}"
        );
        assert!(
            safe.len() < all.len(),
            "the combined list is a strict superset: {safe:?} vs {all:?}"
        );
    }
    use std::path::PathBuf;

    fn scene_switch(label: &str, slot: u64) -> Value {
        serde_json::json!({ "func": "scene", "sceneSlot": slot, "customLabel": label, "isActive": true })
    }

    // AC1 — build + apply ftsw/exp structures.
    #[test]
    fn build_ftsw_exp_structures() {
        let mut p =
            serde_json::json!({ "ftsw": [], "exp": serde_json::Value::Null, "scenes": [1, 2, 3] });
        let layout = serde_json::json!([[scene_switch("A", 0)], [scene_switch("B", 1)]]);
        apply_ftsw(&mut p, layout.clone()).unwrap();
        assert_eq!(p["ftsw"], layout);
        let exp =
            serde_json::json!({ "exp1": { "func": "volume" }, "toe": serde_json::Value::Null });
        apply_exp(&mut p, exp.clone()).unwrap();
        assert_eq!(p["exp"], exp);
    }

    // A VERIFY row's two states must differ ONLY by this switch's own effect: the sibling
    // isolation is identical on both sides, and switch 0's own node flips. Otherwise the
    // reported delta would fold in whatever the other switches happened to be doing.
    #[test]
    fn switch_states_differ_only_by_the_switchs_own_nodes() {
        let ftsw = serde_json::json!([
            [{ "func": "on-off", "isActive": false, "nodes": [{ "groupId": "G1", "nodeId": "drive" }] }],
            [{ "func": "on-off", "isActive": false, "nodes": [{ "groupId": "G1", "nodeId": "delay" }] }],
        ]);
        let preset = serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "nodeId": "drive", "dspUnitParameters": { "bypass": true } },
                { "nodeId": "delay", "dspUnitParameters": { "bypass": true } }
            ] } }
        });
        let st = switch_states(&ftsw, &preset, 0);
        // Sibling `delay` is forced OFF on BOTH sides…
        assert!(st
            .engaged_bypass
            .contains(&("G1".into(), "delay".into(), true)));
        assert!(st
            .disengaged_bypass
            .contains(&("G1".into(), "delay".into(), true)));
        // …and only `drive` flips: engaged ON (bypass false, it is off in base and this
        // switch enables it), disengaged back OFF.
        assert!(st
            .engaged_bypass
            .contains(&("G1".into(), "drive".into(), false)));
        assert!(st
            .disengaged_bypass
            .contains(&("G1".into(), "drive".into(), true)));
        assert!(st.params.is_empty());
    }

    // A PURE-param switch has no bypass difference at all — its engaged state is the param
    // jumping to `valueA`, which only `params` can express. A function missing either value
    // is dropped, and a switch left with nothing at all refuses before any capture.
    #[test]
    fn switch_states_carry_param_functions_and_refuse_a_no_op_switch() {
        let ftsw = serde_json::json!([
            [
                { "func": "param", "groupId": "G1", "nodeId": "amp", "parameterId": "outputLevel",
                  "valueA": 0.8, "valueB": 0.4 },
                { "func": "param", "groupId": "G1", "nodeId": "amp", "parameterId": "gain",
                  "valueA": 0.9 }
            ],
            [{ "func": "param", "groupId": "G1", "nodeId": "amp", "parameterId": "gain", "valueA": 0.9 }],
        ]);
        let preset = serde_json::json!({ "audioGraph": { "guitarNodes": { "G1": [
            { "nodeId": "amp", "dspUnitParameters": { "outputLevel": 0.4 } }
        ] } } });
        let st = switch_states(&ftsw, &preset, 0);
        assert_eq!(
            st.params,
            vec![(
                "G1".to_string(),
                "amp".to_string(),
                "outputLevel".to_string(),
                0.8,
                0.4
            )],
            "the valueB-less `gain` function has no disengaged value to write"
        );
        assert_eq!(
            st.engaged_bypass, st.disengaged_bypass,
            "no on-off function → no bypass difference"
        );
        assert!(!st.params.is_empty(), "the param jump IS the engaged state");
        // Switch 1's only function is the valueB-less one → nothing measurable at all.
        let none = switch_states(&ftsw, &preset, 1);
        assert!(
            none.params.is_empty() && none.engaged_bypass == none.disengaged_bypass,
            "a switch whose param functions carry no values changes nothing measurable"
        );
    }

    // The re-run idempotency anchor: the stored valueA of an existing param function.
    // (No-index cases collapse into one `?` branch — one probe suffices; the finder's
    // own match rules are `existing_param_fn_index`'s contract, not this one's.)
    #[test]
    fn existing_param_fn_value_a_anchor_contract() {
        let pf = |value_a: Value| serde_json::json!({ "func": "param", "nodeId": "amp1", "parameterId": "outputLevel", "valueA": value_a });
        let ftsw = serde_json::json!([
            [pf(0.42.into())],
            [{ "func": "param", "nodeId": "amp1", "parameterId": "outputLevel" }],
            [pf("loud".into())],
        ]);
        let anchor = |sw| existing_param_fn_value_a(&ftsw, sw, "amp1", "outputLevel");
        assert_eq!(anchor(0), Some(0.42), "found → the stored engaged value");
        assert_eq!(anchor(3), None, "no existing function (fresh assign)");
        assert_eq!(anchor(1), None, "function present but valueA missing");
        assert_eq!(anchor(2), None, "non-numeric valueA");
    }

    // The same four cases through `resolve_assign_anchor`, which exists to SPLIT that single
    // `None`. A re-measure caller writes nothing on all three of them, but only one of the
    // three is correct to write nothing on — the other two measure the BASE sound and file
    // it under the switch's identity, which reads as a plausible number on a correctly
    // named row and is exactly what an external validator cannot catch.
    #[test]
    fn resolve_assign_anchor_splits_no_assignment_from_a_mismatch() {
        let pf = |value_a: Value| serde_json::json!({ "func": "param", "nodeId": "amp1", "parameterId": "outputLevel", "valueA": value_a });
        let ftsw = serde_json::json!([
            [pf(0.42.into())],
            [{ "func": "param", "nodeId": "amp1", "parameterId": "outputLevel" }],
            [pf("loud".into())],
            // A BAKED switch: on-off only, no `param` function anywhere. Every switch in
            // the `E2E Hiwatt 3S` fixture is this shape, and its engaged sound IS the saved
            // block value — writing nothing is the right answer, not a swallowed failure.
            [{ "func": "on-off", "nodes": [{ "groupId": "G1", "nodeId": "amp1" }] }],
            // Assign-shaped on the SAME node, but on a different parameter.
            [serde_json::json!({ "func": "param", "nodeId": "amp1", "parameterId": "gain", "valueA": 0.9 })],
        ]);
        let anchor = |sw| resolve_assign_anchor(&ftsw, sw, "amp1", "outputLevel");
        assert_eq!(anchor(0), FsAssignAnchor::Value(0.42), "replay this value");
        assert_eq!(
            anchor(3),
            FsAssignAnchor::NoAssignment,
            "baked / never assigned → nothing to replay, and that is CORRECT"
        );
        assert_eq!(
            anchor(9),
            FsAssignAnchor::NoAssignment,
            "switch index past the end of ftsw → treated as unassigned, never a panic"
        );
        // The three that must NOT be silently `None`.
        for (sw, needle) in [
            (1, "no usable numeric `valueA`"),
            (2, "no usable numeric `valueA`"),
            (4, "not \"outputLevel\""),
        ] {
            match anchor(sw) {
                FsAssignAnchor::Mismatch(why) => assert!(
                    why.contains(needle),
                    "switch {sw}: {why:?} should name {needle:?}"
                ),
                other => panic!("switch {sw}: expected a Mismatch, got {other:?}"),
            }
        }
    }

    // AC — flag assignments that can't bind (scene out of range).

    // Live-sync FS tags: sceneSlot → switch index. Inactive bindings still tag
    // (the device numbers a scene bound to a disabled switch); active wins the
    // slot; first-wins within a pass.
    #[test]
    fn scene_fs_map_inactive_binds_active_wins() {
        let inactive = |label: &str, slot: u64| serde_json::json!({ "func": "scene", "sceneSlot": slot, "customLabel": label, "isActive": false });
        // switch 0 → scene 1 ; switch 1 → scene 2 (INACTIVE) ; switch 2 empty ;
        // switch 3 → a non-scene func ; switch 4 → scene 0 ; switch 5 → scene 0 (collision) ;
        // switch 6 → scene 3 ACTIVE while switch 7 → scene 3 INACTIVE (active must win).
        let ftsw = serde_json::json!([
            [scene_switch("R", 1)],
            [inactive("L", 2)],
            [],
            [{ "func": "bypass", "isActive": true }],
            [scene_switch("A", 0)],
            [scene_switch("dup", 0)],
            [scene_switch("C", 3)],
            [inactive("C-off", 3)],
        ]);
        let m = scene_fs_map(&ftsw);
        assert_eq!(m.get(&1), Some(&0), "scene 1 → switch 0");
        assert_eq!(
            m.get(&0),
            Some(&4),
            "scene 0 → switch 4 (first wins over switch 5)"
        );
        assert_eq!(
            m.get(&2),
            Some(&1),
            "scene 2 → switch 1 even though its binding is inactive (device still numbers it)"
        );
        assert_eq!(
            m.get(&3),
            Some(&6),
            "scene 3 → switch 6 (ACTIVE binding wins over the inactive one on switch 7)"
        );
        assert_eq!(m.len(), 4);
        // Empty / malformed ftsw → empty map (never panics).
        assert!(scene_fs_map(&serde_json::Value::Null).is_empty());
        assert!(scene_fs_map(&serde_json::json!([])).is_empty());
    }

    // Enumerate block-acting footswitches: on-off + param kept, scene/midi skipped;
    // level-param candidates resolved from the graph and gated by `param_class::classify`.
    #[test]
    fn enumerate_block_footswitches_filters_and_resolves() {
        let preset = serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "nodeId": "ACD_OD", "FenderId": "ACD_OD", "dspUnitParameters": {
                    "gain": 0.4, "level": 0.7, "bypass": false, "bypassType": "Post"
                }}
            ]}, "micNodes": {} },
            "ftsw": [
                [{ "func": "scene", "sceneSlot": 1, "isActive": true }],
                [{ "func": "on-off", "nodes": [{ "groupId": "G1", "nodeId": "ACD_OD" }],
                  "customLabel": "Boost", "linkGroup": 2, "isActive": false }],
                [{ "func": "param", "groupId": "G1", "nodeId": "ACD_OD", "parameterId": "gain",
                   "valueA": 0.9, "valueB": 0.4, "valueType": 2, "customLabel": "Lead" }],
                [{ "func": "midi", "channel": 0, "cc": 7 }],
                [],
            ]
        });
        let infos = enumerate_block_footswitches(&preset["ftsw"], &preset);
        // Only switches 1 (on-off) and 2 (param) are block-acting.
        assert_eq!(infos.len(), 2);
        let sw1 = &infos[0];
        assert_eq!(sw1.switch, 1);
        assert_eq!(sw1.label, "Boost");
        assert_eq!(sw1.link_group, Some(2));
        assert_eq!(sw1.functions.len(), 1);
        assert_eq!(sw1.functions[0].func, "on-off");
        assert_eq!(sw1.functions[0].fender_id, "ACD_OD");
        // `level` is levelable; bypass/bypassType are not — and neither is `gain`, which on
        // a generic drive block is DRIVE, not a level control (the classifier annotates only
        // what it knows: `gain` is `level_db` on `ACD_Boost` alone, everything else `Other`).
        // Before the param-class gate this list was `["gain", "level"]`, i.e. the picker
        // offered a distortion knob as a loudness control.
        let params: Vec<&str> = sw1
            .level_params
            .iter()
            .map(|p| p.parameter_id.as_str())
            .collect();
        assert_eq!(params, vec!["level"]);
        assert_eq!(sw1.level_params[0].current, 0.7);

        let sw2 = &infos[1];
        assert_eq!(sw2.switch, 2);
        assert_eq!(sw2.functions[0].func, "param");
        assert_eq!(sw2.functions[0].parameter_id.as_deref(), Some("gain"));
        assert_eq!(sw2.functions[0].value_a, Some(0.9));
        assert_eq!(sw2.functions[0].value_b, Some(0.4));
    }

    // Regression fixture for the reported bug: preset 28 (the e2e `E2E Hiwatt 3S` fixture)'s full
    // real 20-slot `ftsw` + real block params — 4 `func:"scene"` entries (one of them
    // literally named "Base Scene", at switch 1; the enumerator skips it by `func`, never by
    // name — no code path here reads scene names at all) interleaved with 4 block-acting
    // switches that act on nodes from TWO different `audioGraph` groups (G1 and G4). The
    // single-node fixture in `enumerate_block_footswitches_filters_and_resolves` above can't
    // exercise resolving multiple nodes across groups in one pass; this one can.
    #[test]
    fn enumerate_block_footswitches_on_preset28_real_ftsw_skips_scenes_incl_the_base_scene_name() {
        let onoff = |group: &str, node: &str| {
            serde_json::json!({ "func": "on-off", "nodes": [{ "groupId": group, "nodeId": node }],
                "colorA": 13, "colorB": 19, "customLabel": "", "linkGroup": 0, "isActive": false })
        };
        let ftsw = serde_json::json!([
            [],
            [scene_switch("Base Scene", 3)],
            [onoff("G1", "ACD_MythicDrive")],
            [onoff("G1", "ACD_Lightspeed")],
            [],
            [],
            [scene_switch("Clean", 0)],
            [scene_switch("Rhythm", 1)],
            [scene_switch("Dirty", 2)],
            [],
            [],
            [onoff("G1", "ACD_TremoloBias")],
            [onoff("G4", "ACD_UniVibe")],
            [],
            [],
            [],
            [],
            [],
            [],
            [],
        ]);
        let preset = serde_json::json!({ "audioGraph": { "guitarNodes": {
            "G1": [
                { "nodeId": "ACD_MythicDrive", "FenderId": "ACD_MythicDrive", "dspUnitParameters": {
                    "bypass": true, "bypassType": "Post", "gain": 0.5999999046325684,
                    "output": 0.5499999523162842, "treble": 0.5999999642372131 } },
                { "nodeId": "ACD_Lightspeed", "FenderId": "ACD_Lightspeed", "dspUnitParameters": {
                    "bypass": true, "bypassType": "Post", "drive": 0.6200000047683716,
                    "freq": 0.41999998688697815, "loudness": 0.4699999988079071 } },
                { "nodeId": "ACD_TremoloBias", "FenderId": "ACD_TremoloBias", "dspUnitParameters": {
                    "bypass": true, "bypassType": "Post", "intensity": 0.5, "level": 0.5,
                    "ratehz": 6.0 } },
            ],
            "G4": [
                { "nodeId": "ACD_UniVibe", "FenderId": "ACD_UniVibe", "dspUnitParameters": {
                    "bypass": true, "bypassType": "Post", "intensity": 0.7400000095367432,
                    "speed": 3.8463430404663086, "volume": 0.489999920129776 } },
            ],
        }, "micNodes": {} } });

        let infos = enumerate_block_footswitches(&ftsw, &preset);
        let switches: Vec<u32> = infos.iter().map(|i| i.switch).collect();
        assert_eq!(
            switches,
            vec![2, 3, 11, 12],
            "the 4 scene switches (incl. the \"Base Scene\"-named one at 1) must be skipped: {infos:?}"
        );

        // G1 side: switch 2 (ACD_MythicDrive) resolves its levelable params.
        let mythic = infos.iter().find(|i| i.switch == 2).expect("switch 2");
        assert_eq!(mythic.functions[0].fender_id, "ACD_MythicDrive");
        let mythic_params: Vec<&str> = mythic
            .level_params
            .iter()
            .map(|p| p.parameter_id.as_str())
            .collect();
        // `output` is a level_linear default; `gain` (drive) and `treble` (tone) are not
        // level controls and the param-class gate now drops them — the list was
        // `["gain", "output", "treble"]` when any numeric [0,1] param qualified.
        assert_eq!(mythic_params, vec!["output"]);

        // G1 side: switch 3 (ACD_Lightspeed) — the B3 bracketing-fix subject.
        let lightspeed = infos.iter().find(|i| i.switch == 3).expect("switch 3");
        assert_eq!(lightspeed.functions[0].fender_id, "ACD_Lightspeed");
        let lightspeed_params: Vec<&str> = lightspeed
            .level_params
            .iter()
            .map(|p| p.parameter_id.as_str())
            .collect();
        // `loudness` is a `level_linear` default (it's this pedal's output-level knob —
        // the B3 bracketing fix leveled it on hardware), so it survives the class gate.
        // `drive` and `freq` stay TABLE GAPS deliberately: the classifier answers `Other`
        // for both (they shaped tone, not loudness, and were only ever offered under the
        // old any-numeric-[0,1] rule, which listed `["drive", "freq", "loudness"]`).
        assert_eq!(lightspeed_params, vec!["loudness"]);

        // G1 side: switch 11 (ACD_TremoloBias) — `intensity` is a depth control, not a level
        // one, so only `level` survives (`ratehz=6.0` was already excluded before, by the old
        // [0,1] value filter; the classifier now excludes it on class alone).
        let tremolo = infos.iter().find(|i| i.switch == 11).expect("switch 11");
        assert_eq!(tremolo.functions[0].fender_id, "ACD_TremoloBias");
        let tremolo_params: Vec<&str> = tremolo
            .level_params
            .iter()
            .map(|p| p.parameter_id.as_str())
            .collect();
        assert_eq!(
            tremolo_params,
            vec!["level"],
            "only the level control is a candidate; intensity/ratehz are not"
        );

        // G4 side: switch 12 (ACD_UniVibe) resolves in the SAME pass as the G1 switches
        // above — the multi-group case the single-node fixture elsewhere can't exercise.
        let uni_vibe = infos.iter().find(|i| i.switch == 12).expect("switch 12");
        assert_eq!(uni_vibe.functions[0].fender_id, "ACD_UniVibe");
        let uni_vibe_params: Vec<&str> = uni_vibe
            .level_params
            .iter()
            .map(|p| p.parameter_id.as_str())
            .collect();
        assert_eq!(
            uni_vibe_params,
            vec!["volume"],
            "only the level control is a candidate; intensity/speed are not"
        );
    }

    // Candidate enumeration is now `param_class::classify`, block-scoped: the SAME param
    // name qualifies on one block and not another, and a value outside [0,1] no longer
    // disqualifies anything. Drives the picker, so each of the three interesting shapes is
    // pinned end-to-end through `enumerate_block_footswitches`, not just on the predicate.
    #[test]
    fn level_param_candidates_follow_the_param_class_table_not_a_0_1_value_range() {
        let preset = serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                // Raw dB, HW-verified fw 1.8.45: base 2.5 IS +2.5 dB, so the old
                // `(0.0..=1.0)` value filter silently dropped this block's only level
                // control. `gain` is `Other` on every OTHER block (it means drive).
                { "nodeId": "boost", "FenderId": "ACD_Boost", "dspUnitParameters": {
                    "gain": 2.5, "level": 0.5, "bypass": false }},
                // The documented trap: `level` is a level_linear DEFAULT everywhere, but on
                // the TM Rumble it is an amp knob that must never be swept for loudness.
                { "nodeId": "rumble", "FenderId": "ACD_TMRumbleV3", "dspUnitParameters": {
                    "level": 0.5, "bypass": false }},
                // A wet/mix control IS a candidate — it moves loudness — but it carries the
                // WetMix class, which is what arms the solver's floor.
                { "nodeId": "chorus", "FenderId": "ACD_Chorus", "dspUnitParameters": {
                    "mix": 0.8, "rate": 0.3, "bypass": false }}
            ]}, "micNodes": {} },
            "ftsw": [
                [{ "func": "on-off", "nodes": [{ "groupId": "G1", "nodeId": "boost" }] }],
                [{ "func": "on-off", "nodes": [{ "groupId": "G1", "nodeId": "rumble" }] }],
                [{ "func": "on-off", "nodes": [{ "groupId": "G1", "nodeId": "chorus" }] }],
            ]
        });
        let infos = enumerate_block_footswitches(&preset["ftsw"], &preset);
        let params_of = |sw: u32| -> Vec<&str> {
            infos
                .iter()
                .find(|i| i.switch == sw)
                .unwrap_or_else(|| panic!("switch {sw}"))
                .level_params
                .iter()
                .map(|p| p.parameter_id.as_str())
                .collect()
        };

        assert_eq!(
            params_of(0),
            vec!["gain", "level"],
            "ACD_Boost.gain is a candidate despite its 2.5 (raw dB) base value"
        );
        let info = crate::param_class::classify("ACD_Boost", "gain");
        assert_eq!(info.class, crate::param_class::ParamClass::LevelDb);
        assert_eq!(
            info.range,
            (0.0, 12.0),
            "the SOLVE's bounds come from the table, never from the observed value"
        );

        assert!(
            params_of(1).is_empty(),
            "ACD_TMRumbleV3.level is an amp knob, not a level control — never a candidate"
        );

        assert_eq!(
            params_of(2),
            vec!["mix"],
            "a wet/mix control IS a candidate ('rate' is not)"
        );
        assert_eq!(
            crate::param_class::classify("ACD_Chorus", "mix").class,
            crate::param_class::ParamClass::WetMix,
            "…and it carries WetMix, which is what arms the solver's wet floor"
        );
    }

    // ── Bake-vs-assign planning ──

    /// A preset graph with one guitar block `N` and an optional sibling `M`, each with a
    /// `bypass` flag; `ftsw` is supplied by the caller per case. `scenes: []` is the DEVICE
    /// shape for a scene-less preset (`session.rs`'s `{"scenes":[]}` case) — a MISSING key
    /// means a truncated read, which the bake gate must treat as unknown.
    fn preset_with(n_bypass: bool, m: Option<bool>, ftsw: Value) -> Value {
        let mut nodes = vec![serde_json::json!({
            "nodeId": "N", "FenderId": "N",
            "dspUnitParameters": { "gain": 0.4, "bypass": n_bypass }
        })];
        if let Some(mb) = m {
            nodes.push(serde_json::json!({
                "nodeId": "M", "FenderId": "M",
                "dspUnitParameters": { "gain": 0.5, "bypass": mb }
            }));
        }
        serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": nodes }, "micNodes": {} },
            "ftsw": ftsw,
            "scenes": [],
        })
    }

    /// Give the preset ONE scene whose overlay carries `params` for node `N` — the sparse
    /// per-node shape `scenes[i].guitarNodes.<group>.<id>.dspUnitParameters`.
    fn with_scene_overlay_on_n(mut p: Value, params: Value) -> Value {
        p["scenes"] = serde_json::json!([{
            "guitarNodes": { "G1": { "N": { "dspUnitParameters": params } } },
            "micNodes": {},
        }]);
        p
    }

    fn onoff(nodes: &[&str], active: bool) -> Value {
        let ns: Vec<Value> = nodes
            .iter()
            .map(|n| serde_json::json!({ "groupId": "G1", "nodeId": n }))
            .collect();
        serde_json::json!({ "func": "on-off", "nodes": ns, "isActive": active })
    }

    fn key(switch: u32, target: f64) -> FsJobKey<'static> {
        FsJobKey {
            switch,
            lev_node: "N",
            lev_param: "gain",
            target_bits: target.to_bits(),
        }
    }

    #[test]
    fn plan_bakes_single_owner_off_in_base() {
        // N off in base, switch 0 has an on-off for N, no other owner, no scenes → Bake.
        // A SIBLING switch owns M → M forced off (isolation) alongside N's own flip.
        // isActive:false matches the HW correlation (a base-off block's switch reads
        // inactive) — engaged is the flip of saved.
        let p = preset_with(
            true,
            None,
            serde_json::json!([[onoff(&["N"], false)], [onoff(&["M"], true)]]),
        );
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)]);
        match &plans[0] {
            FsLevelPlan::Bake {
                engaged,
                clear_stale,
                mirror_scenes,
            } => {
                // tuple bool = the `bypass` to WRITE: base off (bypass=true) → engaged un-bypass (false).
                assert!(engaged.contains(&("G1".into(), "N".into(), false)));
                // sibling switch's block M forced off (bypass=true).
                assert!(engaged.contains(&("G1".into(), "M".into(), true)));
                assert_eq!(*clear_stale, None);
                assert!(mirror_scenes.is_empty(), "no scenes → nothing to mirror");
            }
            other => panic!("expected Bake, got {other:?}"),
        }
    }

    #[test]
    fn plan_bakes_when_block_on_in_base() {
        // N ON in base, switch 0 has only an on-off for N (no `param` fn on it) → under the
        // assign gate (2026-08-19) this BAKES: the switch doesn't already change (N, gain), so
        // leveling never adds a function to it — it writes the block directly instead. A
        // SIBLING switch's block M is still forced off (isolation) so N is measured against the
        // clean base, not base + M. This used to Assign under an "always-assign a base-on
        // block" rule; that rule is gone, the gate is the sole decider now.
        //
        // The isolation also asserts N's OWN bypass. It used to stop at the sibling entry, on
        // the reasoning that a base-on block needs no assertion — hardware disproved that
        // (2026-08-19, slot 26): a sibling row's isolation writes N's `bypass = true` into the
        // working copy and `recall_base` re-asserts it, so the row measured with N switched
        // off. See `plan_on_in_base_still_asserts_its_own_block_bypass`.
        let p = preset_with(
            false,
            None,
            serde_json::json!([[onoff(&["N"], true)], [onoff(&["M"], true)]]),
        );
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)]);
        match &plans[0] {
            FsLevelPlan::Bake {
                engaged,
                clear_stale,
                mirror_scenes,
            } => {
                assert_eq!(
                    engaged,
                    &vec![
                        ("G1".into(), "M".into(), true),
                        ("G1".into(), "N".into(), false)
                    ]
                );
                assert_eq!(*clear_stale, None);
                assert!(mirror_scenes.is_empty(), "no scenes → nothing to mirror");
            }
            other => panic!("expected Bake, got {other:?}"),
        }
    }

    #[test]
    fn plan_clamps_off_in_base_with_no_enabler() {
        // N off in base but switch 0 has no on-off for it → can never be heard → Clamp.
        let p = preset_with(true, None, serde_json::json!([[]]));
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)]);
        assert!(matches!(plans[0], FsLevelPlan::Clamp(_)));
    }

    #[test]
    fn plan_onoff_enables_regardless_of_isactive() {
        // `isActive` on an on-off is the CURRENT engaged state, not enable/disable (HW: a base-off
        // block's switch reads isActive=false). So an `isActive:false` on-off is STILL an enabler →
        // off-in-base + sole owner + no scenes → Bake.
        let p = preset_with(true, None, serde_json::json!([[onoff(&["N"], false)]]));
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)]);
        assert!(matches!(plans[0], FsLevelPlan::Bake { .. }));
    }

    #[test]
    fn plan_bakes_when_second_footswitch_also_enables_n() {
        // Switch 0 levels N; switch 1 ALSO has an on-off for N, but neither switch already
        // carries a `param` fn on (N, gain) → the assign gate says Bake for both, sharing no
        // longer forces Assign. The old "not sole owner → Assign" fallback is gone — it was
        // exactly the mechanism that appended a second function to a switch's row (the shape
        // `danger.md` forbids), so co-ownership is no longer treated as an assign trigger.
        // N off in base ⇒ both switches saved inactive (the HW correlation).
        let p = preset_with(
            true,
            None,
            serde_json::json!([[onoff(&["N"], false)], [onoff(&["N"], false)]]),
        );
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)]);
        match &plans[0] {
            FsLevelPlan::Bake {
                engaged,
                clear_stale,
                ..
            } => {
                assert!(!engaged.is_empty());
                // switch 1 targets the SAME node N → excluded from switch 0's siblings, so N is
                // NOT force-bypassed; it's engaged (flipped on) by switch 0's own flip.
                assert!(engaged.contains(&("G1".into(), "N".into(), false)));
                assert!(!engaged.contains(&("G1".into(), "N".into(), true)));
                assert_eq!(*clear_stale, None);
            }
            other => panic!("expected Bake, got {other:?}"),
        }
    }

    /// The reported bug: a preset WITH scenes whose overlays never CHANGE the leveled node
    /// must still BAKE. The old whole-preset `has_fs_scenes` gate routed EVERY switch
    /// of ANY scened preset to Assign, which adds a second function to the switch — and a
    /// multi-function switch with an empty `customLabel` displays "MULTI" on the unit.
    #[test]
    fn plan_bakes_when_scenes_do_not_touch_the_node_bypass() {
        let p = with_scene_overlay_on_n(
            preset_with(true, None, serde_json::json!([[onoff(&["N"], false)]])),
            // A param that is neither `bypass` nor the leveled one (`gain`) — the overlay is
            // irrelevant to the bake either way.
            serde_json::json!({ "level": 0.7 }),
        );
        match &plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)])[0] {
            // `clear_stale: None` IS the "no `ftsw` write at all" invariant: `FsWrite::Bake`
            // only ever touches `ftsw` to clear a stale `param` fn, and this switch has none.
            FsLevelPlan::Bake {
                engaged,
                clear_stale,
                mirror_scenes,
            } => {
                assert_eq!(*clear_stale, None);
                assert!(engaged.contains(&("G1".into(), "N".into(), false)));
                // The overlay omits the leveled param (`gain`) → the scene inherits base,
                // so the bake propagates by itself — nothing to mirror.
                assert!(mirror_scenes.is_empty());
            }
            other => panic!("expected Bake, got {other:?}"),
        }
    }

    /// The DEVICE-AUTHORED preset shape, and why key-presence semantics were not enough: a
    /// preset the unit itself wrote carries the FULL param set for every node in every scene
    /// overlay, `bypass` included. Presence of the key therefore proves nothing — only a
    /// VALUE that differs from base changes what the scene renders, so an overlay that
    /// merely restates base must still BAKE (else every switch of every real scened preset
    /// takes the Assign path and the "MULTI" symptom survives).
    #[test]
    fn plan_bakes_when_a_full_scene_overlay_restates_the_base_values() {
        let p = with_scene_overlay_on_n(
            preset_with(true, None, serde_json::json!([[onoff(&["N"], false)]])),
            // base is `{ "gain": 0.4, "bypass": true }` — restated verbatim.
            serde_json::json!({ "bypass": true, "gain": 0.4 }),
        );
        match &plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)])[0] {
            FsLevelPlan::Bake {
                clear_stale,
                mirror_scenes,
                ..
            } => {
                assert_eq!(*clear_stale, None);
                // The overlay restates base's `gain` verbatim → mirror the solved value
                // there, else the full-param overlay masks the bake in that scene.
                assert_eq!(mirror_scenes, &vec![0]);
            }
            other => panic!("expected Bake, got {other:?}"),
        }
    }

    /// A scene that overlays the leveled param with its OWN value still BAKES — the overlay
    /// MASKS base (HW, Hiwatt slot 31), so the bake cannot leak into it, while an Assign's
    /// single `valueA` would trample exactly that authored per-scene mix (the user's Hiwatt
    /// mutes its trem in one scene with `level: 0.0`). The divergent scene is simply NOT
    /// mirrored: it keeps its authored value, unleveled by design.
    #[test]
    fn plan_bakes_but_never_mirrors_a_scene_that_authored_its_own_value() {
        let p = with_scene_overlay_on_n(
            preset_with(true, None, serde_json::json!([[onoff(&["N"], false)]])),
            serde_json::json!({ "bypass": true, "gain": 0.7 }),
        );
        match &plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)])[0] {
            FsLevelPlan::Bake { mirror_scenes, .. } => {
                assert!(
                    mirror_scenes.is_empty(),
                    "a diverging overlay is authored intent — never mirrored"
                );
            }
            other => panic!("expected Bake, got {other:?}"),
        }
    }

    #[test]
    fn plan_bakes_even_when_a_scene_overlay_touches_the_node_bypass() {
        // The old hazard this fixture pinned: a scene flips this block ON, so a bake would
        // render the solved value in a state the leveler never measured — that used to force
        // Assign. Under the assign gate that consideration is GONE: the switch has no `param`
        // fn on (N, gain), so it BAKES regardless of what any scene overlay does to the node's
        // bypass. The old fallback (consulting `scene_overlays_change_param`) was exactly the
        // mechanism that appended a second function to the row — danger.md's forbidden shape —
        // so it no longer has a say in bake-vs-assign at all.
        let p = with_scene_overlay_on_n(
            preset_with(true, None, serde_json::json!([[onoff(&["N"], false)]])),
            serde_json::json!({ "bypass": false, "gain": 0.7 }),
        );
        match &plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)])[0] {
            FsLevelPlan::Bake { engaged, .. } => assert!(!engaged.is_empty()),
            other => panic!("expected Bake, got {other:?}"),
        }
    }

    #[test]
    fn plan_bakes_a_shared_block_even_when_scenes_are_clean() {
        // Sharing no longer matters either: switch 1 also enables N, but since NEITHER switch
        // already carries a `param` fn on (N, gain), both bake — the old sole-ownership
        // fallback to Assign is gone along with the scene-overlay one.
        let p = with_scene_overlay_on_n(
            preset_with(
                true,
                None,
                serde_json::json!([[onoff(&["N"], false)], [onoff(&["N"], false)]]),
            ),
            serde_json::json!({ "gain": 0.7 }),
        );
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)]);
        assert!(matches!(plans[0], FsLevelPlan::Bake { .. }));
    }

    #[test]
    fn plan_bakes_even_when_the_saved_scene_data_is_unreadable() {
        // No `scenes` key = a truncated field-8 read (`scenes` sits at the document tail), NOT
        // a scene-less preset (which reads `"scenes":[]`). This used to be treated as "unknown,
        // so never bake" and forced Assign; the bake gate no longer reads scene data at all
        // (that consultation is gone with the old fallback), so an unreadable `scenes` no
        // longer changes the answer — the switch still has no `param` fn on (N, gain), so it
        // still bakes.
        let mut p = preset_with(true, None, serde_json::json!([[onoff(&["N"], false)]]));
        p.as_object_mut().expect("preset object").remove("scenes");
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)]);
        assert!(matches!(plans[0], FsLevelPlan::Bake { .. }));
    }

    #[test]
    fn plan_on_in_base_still_asserts_its_own_block_bypass() {
        // HW, 2026-08-19, slot 26 "Plumes+BD2+OCD". `ACD_Plumes` is the only block ON in the
        // base, and its row is the only one that failed: solved ALONE it converges
        // (level 0.3634 → -22.95 LUFS), but in ANY batch containing another row — either
        // order — all of its captures read the same -26.96 LUFS, the secant's flat-response
        // branch fires and the row reports a spurious `clamped scene_ceiling`.
        //
        // The cause is here, not in the solver. A SIBLING row's isolation
        // (`siblings_off_excluding`) writes THIS block's `bypass = true` into the device's
        // working copy, and every capture's `recall_base` re-asserts that mutated copy — so a
        // plan that omits its own block's bypass inherits the sibling's write and measures
        // with the block it is levelling switched OFF, where its handle has no authority.
        // The ceiling prepass was right throughout (-21.13 LUFS) precisely because it goes
        // through `switch_states`, which DOES assert it.
        //
        // So the plan's isolation must equal `switch_states(..).engaged_bypass` — asserted
        // against that function directly, so the two can never drift apart again.
        let p = preset_with(
            /* N on in base */ false,
            /* M off in base */ Some(true),
            serde_json::json!([[onoff(&["N"], true)], [onoff(&["M"], false)]]),
        );
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)]);
        let FsLevelPlan::Bake { engaged, .. } = &plans[0] else {
            panic!("expected Bake, got {:?}", plans[0]);
        };
        assert!(
            engaged.contains(&("G1".to_string(), "N".to_string(), false)),
            "the levelled block's own bypass must be asserted, not assumed — {engaged:?}"
        );
        assert_eq!(
            *engaged,
            switch_states(&p["ftsw"], &p, 0).engaged_bypass,
            "plan isolation must match the ceiling prepass's isolation exactly"
        );
    }

    #[test]
    fn plan_case2_shared_block_same_target_bakes_once() {
        // Two switches both enable N and level it to the SAME target → first bakes, second
        // shares (no second write). They are jointly the sole owners.
        let p = preset_with(
            true,
            None,
            serde_json::json!([[onoff(&["N"], true)], [onoff(&["N"], true)]]),
        );
        let jobs = [key(0, -23.0), key(1, -23.0)];
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &jobs);
        assert!(matches!(plans[0], FsLevelPlan::Bake { .. }));
        assert_eq!(plans[1], FsLevelPlan::BakeShared { rep: 0 });
    }

    #[test]
    fn plan_case2_different_targets_bake_independently_not_shared() {
        // NOT in the original failing-test list, but broken by the same rule change — the old
        // comment's reasoning ("a single block value can't satisfy both → both Assign") no
        // longer applies: the assign gate doesn't look at target sharing at all, only at
        // whether the switch already carries a `param` fn on (N, gain). Neither does here, so
        // BOTH bake. They are NOT `BakeShared` (that requires an identical `target_bits`, and
        // these differ), so this plans two independent writes to the SAME block N with two
        // DIFFERENT target values — whichever job's write lands last on the device wins, and
        // there is no assign-side differentiation left to keep them apart. This is a real
        // consequence of removing the sole-ownership fallback, flagged here rather than
        // "fixed": the task instructions were to pin new *observed* behaviour in
        // `plan_footswitch_jobs`, not to change it.
        let p = preset_with(
            true,
            None,
            serde_json::json!([[onoff(&["N"], true)], [onoff(&["N"], true)]]),
        );
        let jobs = [key(0, -23.0), key(1, -18.0)];
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &jobs);
        assert!(matches!(plans[0], FsLevelPlan::Bake { .. }));
        assert!(
            matches!(plans[1], FsLevelPlan::Bake { .. }),
            "different targets on a shared block: independently baked, not `BakeShared` — {:?}",
            plans[1]
        );
    }

    // NOT in the original failing-test list, but broken by the same rule change, and it is
    // exactly the POSITIVE half of the new rule (item 3): a switch that ALREADY carries a
    // `param` fn on the selected (node, param) must plan `Assign`, editing that function in
    // place — never bake-and-clear. This used to pin the opposite: a stale param fn got
    // cleared and the value baked onto the block instead ("consolidate to one write"). That
    // consolidation is gone — per the directive, an existing function on the selected control
    // is the ONE thing that routes a job to `Assign` at all, and it is edited, not removed.
    #[test]
    fn plan_assigns_when_the_switch_already_carries_a_param_fn_for_the_node() {
        // Switch 0 carries BOTH an on-off on N and a `param` fn on (N, gain) — the existing
        // function is exactly what the assign gate looks for.
        let ftsw = serde_json::json!([[
            onoff(&["N"], true),
            { "func": "param", "groupId": "G1", "nodeId": "N", "parameterId": "gain",
              "valueA": 0.9, "valueB": 0.4 }
        ]]);
        let p = preset_with(true, None, ftsw);
        assert_eq!(
            existing_param_fn_index(&p["ftsw"], 0, "N", "gain"),
            Some(1),
            "fixture precondition: the switch already changes (N, gain) at index 1"
        );
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)]);
        assert!(
            matches!(plans[0], FsLevelPlan::Assign { .. }),
            "expected Assign (edit the existing fn), got {:?}",
            plans[0]
        );
    }

    #[test]
    fn plan_engaged_list_flips_a_multi_block_switch() {
        // Switch enables N (off→on) AND M (on→off): engaged replicates BOTH flips so the target
        // is measured with the switch's full engaged state. A SIBLING switch owns P → P forced off.
        // Saved DISENGAGED (`isActive:false`) — engaged is one toggle away from saved; a preset
        // saved WITH the switch active (`isActive:true`) keeps its saved states instead (the
        // preset-024 BD2 regression, covered in `commands/doctor_tests.rs`).
        let p = preset_with(
            true,
            Some(false),
            serde_json::json!([[onoff(&["N", "M"], false)], [onoff(&["P"], true)]]),
        );
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)]);
        match &plans[0] {
            FsLevelPlan::Bake { engaged, .. } => {
                // bypass to write: N off→on = false ; M on→off = true.
                assert!(engaged.contains(&("G1".into(), "N".into(), false)));
                assert!(engaged.contains(&("G1".into(), "M".into(), true)));
                // sibling switch's block P forced off (isolation).
                assert!(engaged.contains(&("G1".into(), "P".into(), true)));
            }
            other => panic!("expected Bake, got {other:?}"),
        }
    }

    #[test]
    fn all_onoff_blocks_dedups_and_ignores_isactive_and_malformed() {
        // Two switches both on-off for N (active + inactive) + one for M → deduped {N, M},
        // order-preserving. scene/param funcs ignored; a nodes-less on-off contributes nothing.
        let ftsw = serde_json::json!([
            [onoff(&["N"], true)],
            [onoff(&["N"], false)],
            [onoff(&["M"], true)],
            [{ "func": "scene", "sceneSlot": 1, "isActive": true }],
            [{ "func": "param", "groupId": "G1", "nodeId": "P", "parameterId": "gain" }],
            [{ "func": "on-off" }],
        ]);
        assert_eq!(
            all_onoff_blocks(&ftsw),
            vec![
                ("G1".to_string(), "N".to_string()),
                ("G1".to_string(), "M".to_string()),
            ]
        );
        // Empty / missing / malformed → empty (never panics).
        assert!(all_onoff_blocks(&serde_json::Value::Null).is_empty());
        assert!(all_onoff_blocks(&serde_json::json!([])).is_empty());
        assert!(all_onoff_blocks(&serde_json::json!("garbage")).is_empty());
    }

    #[test]
    fn node_targeted_by_assign_covers_on_off_and_param_fn_rows() {
        let ftsw = serde_json::json!([
            [onoff(&["N"], true)],
            [{ "func": "param", "groupId": "G1", "nodeId": "P", "parameterId": "gain",
               "valueA": 0.0, "valueB": 1.0 }],
        ]);
        let preset = serde_json::json!({ "ftsw": ftsw });
        assert!(node_targeted_by_assign(&preset, "N"), "on-off row");
        assert!(node_targeted_by_assign(&preset, "P"), "param-func assign");
        assert!(!node_targeted_by_assign(&preset, "unrelated"));
        assert!(!node_targeted_by_assign(&serde_json::json!({}), "anything"));
    }

    // The old `scene_jobs::node_targeted_by_ftsw_or_exp` copy this function replaces only
    // scanned `exp1`/`exp2`, only accepted an array-shaped jack body, and only matched
    // `func:"param"` — missing every one of these three shapes. Each must be caught now.
    #[test]
    fn node_targeted_by_assign_catches_a_toe_jack_assign() {
        let preset = serde_json::json!({
            "exp": { "toe": [
                { "func": "param", "groupId": "G1", "nodeId": "T", "paramId": "gain",
                  "heel": 0.0, "toe": 1.0 }
            ] }
        });
        assert!(node_targeted_by_assign(&preset, "T"));
    }

    #[test]
    fn node_targeted_by_assign_catches_an_object_shaped_exp1_body() {
        let preset = serde_json::json!({
            "exp": { "exp1": { "func": "volume", "nodeId": "V" } }
        });
        assert!(node_targeted_by_assign(&preset, "V"));
    }

    #[test]
    fn node_targeted_by_assign_catches_a_wah_func_binding() {
        let preset = serde_json::json!({
            "exp": { "exp2": [
                { "func": "wah", "groupId": "G1", "nodeId": "W", "heel": 0.0, "toe": 1.0 }
            ] }
        });
        assert!(node_targeted_by_assign(&preset, "W"));
    }

    #[test]
    fn siblings_off_excludes_own_and_shared_nodes() {
        // Switch 0 owns N; switch 1 owns N (SHARED) + M; switch 2 owns P.
        let ftsw = serde_json::json!([
            [onoff(&["N"], true)],
            [onoff(&["N", "M"], true)],
            [onoff(&["P"], true)],
        ]);
        // For switch 0: own = {N}. Siblings = M, P — N is excluded even though switch 1 also
        // targets it (the shared-node case). Every entry forced OFF (bypass=true).
        let sibs = siblings_off_excluding(&ftsw, 0);
        assert!(sibs.iter().all(|(_, _, byp)| *byp));
        let ids: Vec<&str> = sibs.iter().map(|(_, n, _)| n.as_str()).collect();
        assert_eq!(ids, vec!["M", "P"]);
        // Missing / empty ftsw → empty.
        assert!(siblings_off_excluding(&serde_json::Value::Null, 0).is_empty());
    }

    // AC — applying a layout to the fixture re-encodes losslessly.
    #[test]
    fn reencode_roundtrips() {
        let xor = crate::backup::xor_jld;
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/Guitar.preset");
        let Ok(file) = std::fs::read(&path) else {
            eprintln!("skip: fixture absent");
            return;
        };
        let mut v: Value = serde_json::from_str(&String::from_utf8(xor(&file)).unwrap()).unwrap();
        let layout = serde_json::json!([[scene_switch("Custom", 0)]]);
        apply_ftsw(&mut v, layout).unwrap();
        let mutated = serde_json::to_string(&v).unwrap();
        let decoded_again = String::from_utf8(xor(&xor(mutated.as_bytes()))).unwrap();
        assert_eq!(decoded_again, mutated);
        let reparsed: Value = serde_json::from_str(&decoded_again).unwrap();
        assert_eq!(reparsed["ftsw"][0][0]["customLabel"], "Custom");
    }
}
