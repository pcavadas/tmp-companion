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

/// A continuous block parameter the leveler can solve on (numeric `dspUnitParameter` in
/// `[0,1]`), surfaced so the UI can offer a block+parameter picker per footswitch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LevelParamCandidate {
    pub group_id: String,
    pub node_id: String,
    pub fender_id: String,
    pub parameter_id: String,
    pub current: f64, // base value (the switch-OFF / `valueB` default)
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
}

/// `dspUnitParameter` keys that are never leveling targets (bool/list controls). Numeric
/// `[0,1]` continuous params are kept; bool/string params are already excluded by the
/// `as_f64` filter, so this only documents intent for the bypass family.
fn is_levelable_param(key: &str, val: &Value) -> bool {
    !matches!(
        key,
        "bypass" | "bypassType" | "clipState" | "muteInput" | "muteOutput"
    ) && val.as_f64().is_some_and(|v| (0.0..=1.0).contains(&v))
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
        // Level-param candidates: continuous [0,1] params of each acted-on block (deduped).
        let mut level_params: Vec<LevelParamCandidate> = Vec::new();
        let mut seen: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        for (g, nid) in &acted {
            if let Some((fid, params)) = nodes.get(nid) {
                let mut keys: Vec<&String> = params.keys().collect();
                keys.sort();
                for k in keys {
                    let v = &params[k];
                    if is_levelable_param(k, v) && seen.insert((nid.clone(), k.clone())) {
                        level_params.push(LevelParamCandidate {
                            group_id: g.clone(),
                            node_id: nid.clone(),
                            fender_id: fid.clone(),
                            parameter_id: k.clone(),
                            current: v.as_f64().unwrap_or(0.0),
                        });
                    }
                }
            }
        }
        out.push(FootswitchInfo {
            switch: sw_idx as u32,
            label,
            link_group,
            functions,
            level_params,
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

/// Decide bake-vs-assign for every job in a batch (PURE — no device I/O). `has_fs_scenes` is the
/// whole-preset gate (any/unknown scenes → never bake; conservative because a sparse scene
/// overlay can turn a base-off block ON and would then render the baked value, and the field-8
/// read can truncate scene bodies). Returns one plan per job, aligned to `jobs`.
pub fn plan_footswitch_jobs(
    ftsw: &Value,
    preset: &Value,
    jobs: &[FsJobKey],
    has_fs_scenes: bool,
) -> Vec<FsLevelPlan> {
    let mut plans = Vec::with_capacity(jobs.len());
    let mut bake_rep: std::collections::HashMap<(&str, &str, u64), usize> =
        std::collections::HashMap::new();
    for (idx, job) in jobs.iter().enumerate() {
        if !block_bypassed_in_base(preset, job.lev_node) {
            // Block ON in base → part of the base sound → assignment, its own block measured
            // as-saved; only force every OTHER footswitch's block off (isolation).
            // ponytail: accepted edge case — an always-on `param`-target block that some OTHER
            // switch's on-off also toggles gets forced off here while being the leveled block.
            // Exotic layout, not the reported bug; acknowledged not handled.
            plans.push(FsLevelPlan::Assign {
                engaged: siblings_off_excluding(ftsw, job.switch),
            });
            continue;
        }
        let activators = onoff_switches_for(ftsw, job.lev_node);
        if !activators.contains(&job.switch) {
            plans.push(FsLevelPlan::Clamp(
                "block is bypassed in the base preset and this footswitch doesn't enable it".into(),
            ));
            continue;
        }
        // Off in base, this switch enables it: every other switch's block off (siblings) PLUS
        // this switch's own flip. Disjoint by construction (siblings excludes own nodes).
        let mut engaged = siblings_off_excluding(ftsw, job.switch);
        engaged.extend(engaged_bypass_for_switch(ftsw, preset, job.switch));
        // Sole-/group-owner: every active on-off activator of N must be in this (N,P,T) group.
        let group: std::collections::HashSet<u32> = jobs
            .iter()
            .filter(|j| {
                j.lev_node == job.lev_node
                    && j.lev_param == job.lev_param
                    && j.target_bits == job.target_bits
            })
            .map(|j| j.switch)
            .collect();
        let sole_owner = activators.iter().all(|sw| group.contains(sw));
        if has_fs_scenes || !sole_owner {
            // Can't bake safely → engaged-measured param assignment (best-effort fallback).
            plans.push(FsLevelPlan::Assign { engaged });
            continue;
        }
        let key = (job.lev_node, job.lev_param, job.target_bits);
        if let Some(&rep) = bake_rep.get(&key) {
            plans.push(FsLevelPlan::BakeShared { rep });
        } else {
            bake_rep.insert(key, idx);
            plans.push(FsLevelPlan::Bake {
                engaged,
                clear_stale: existing_param_fn_index(ftsw, job.switch, job.lev_node, job.lev_param),
            });
        }
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

#[cfg(test)]
mod tests {
    use super::*;
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
    // level-param candidates resolved from the graph (continuous [0,1] params only).
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
        // gain + level are levelable; bypass/bypassType are not.
        let params: Vec<&str> = sw1
            .level_params
            .iter()
            .map(|p| p.parameter_id.as_str())
            .collect();
        assert_eq!(params, vec!["gain", "level"]);
        assert_eq!(sw1.level_params[0].current, 0.4);

        let sw2 = &infos[1];
        assert_eq!(sw2.switch, 2);
        assert_eq!(sw2.functions[0].func, "param");
        assert_eq!(sw2.functions[0].parameter_id.as_deref(), Some("gain"));
        assert_eq!(sw2.functions[0].value_a, Some(0.9));
        assert_eq!(sw2.functions[0].value_b, Some(0.4));
    }

    // ── Bake-vs-assign planning ──

    /// A preset graph with one guitar block `N` and an optional sibling `M`, each with a
    /// `bypass` flag; `ftsw` is supplied by the caller per case.
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
        })
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
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)], false);
        match &plans[0] {
            FsLevelPlan::Bake {
                engaged,
                clear_stale,
            } => {
                // tuple bool = the `bypass` to WRITE: base off (bypass=true) → engaged un-bypass (false).
                assert!(engaged.contains(&("G1".into(), "N".into(), false)));
                // sibling switch's block M forced off (bypass=true).
                assert!(engaged.contains(&("G1".into(), "M".into(), true)));
                assert_eq!(*clear_stale, None);
            }
            other => panic!("expected Bake, got {other:?}"),
        }
    }

    #[test]
    fn plan_assigns_when_block_on_in_base() {
        // N ON in base → assignment; N's own block stays as-saved, but a SIBLING switch's block
        // M is forced off (isolation) so N is measured against the clean base, not base + M.
        let p = preset_with(
            false,
            None,
            serde_json::json!([[onoff(&["N"], true)], [onoff(&["M"], true)]]),
        );
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)], false);
        match &plans[0] {
            FsLevelPlan::Assign { engaged } => {
                assert_eq!(engaged, &vec![("G1".into(), "M".into(), true)]);
            }
            other => panic!("expected Assign, got {other:?}"),
        }
    }

    #[test]
    fn plan_clamps_off_in_base_with_no_enabler() {
        // N off in base but switch 0 has no on-off for it → can never be heard → Clamp.
        let p = preset_with(true, None, serde_json::json!([[]]));
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)], false);
        assert!(matches!(plans[0], FsLevelPlan::Clamp(_)));
    }

    #[test]
    fn plan_onoff_enables_regardless_of_isactive() {
        // `isActive` on an on-off is the CURRENT engaged state, not enable/disable (HW: a base-off
        // block's switch reads isActive=false). So an `isActive:false` on-off is STILL an enabler →
        // off-in-base + sole owner + no scenes → Bake.
        let p = preset_with(true, None, serde_json::json!([[onoff(&["N"], false)]]));
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)], false);
        assert!(matches!(plans[0], FsLevelPlan::Bake { .. }));
    }

    #[test]
    fn plan_assigns_when_second_footswitch_also_enables_n() {
        // Switch 0 levels N, but switch 1 ALSO has an on-off for N → not sole owner →
        // engaged-measured Assign (baking would change N for switch 1 too). N off in
        // base ⇒ both switches saved inactive (the HW correlation).
        let p = preset_with(
            true,
            None,
            serde_json::json!([[onoff(&["N"], false)], [onoff(&["N"], false)]]),
        );
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)], false);
        match &plans[0] {
            FsLevelPlan::Assign { engaged } => {
                assert!(!engaged.is_empty());
                // switch 1 targets the SAME node N → excluded from switch 0's siblings, so N is
                // NOT force-bypassed; it's engaged (flipped on) by switch 0's own flip.
                assert!(engaged.contains(&("G1".into(), "N".into(), false)));
                assert!(!engaged.contains(&("G1".into(), "N".into(), true)));
            }
            other => panic!("expected engaged Assign, got {other:?}"),
        }
    }

    #[test]
    fn plan_assigns_when_preset_has_scenes() {
        // Off in base + sole owner BUT the preset has scenes → conservative: engaged Assign.
        let p = preset_with(true, None, serde_json::json!([[onoff(&["N"], true)]]));
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)], true);
        match &plans[0] {
            FsLevelPlan::Assign { engaged } => assert!(!engaged.is_empty()),
            other => panic!("expected engaged Assign, got {other:?}"),
        }
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
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &jobs, false);
        assert!(matches!(plans[0], FsLevelPlan::Bake { .. }));
        assert_eq!(plans[1], FsLevelPlan::BakeShared { rep: 0 });
    }

    #[test]
    fn plan_case2_different_targets_do_not_share() {
        // Same block, DIFFERENT targets → a single block value can't satisfy both → both Assign
        // (neither is the sole owner of N for its own target group).
        let p = preset_with(
            true,
            None,
            serde_json::json!([[onoff(&["N"], true)], [onoff(&["N"], true)]]),
        );
        let jobs = [key(0, -23.0), key(1, -18.0)];
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &jobs, false);
        assert!(matches!(plans[0], FsLevelPlan::Assign { .. }));
        assert!(matches!(plans[1], FsLevelPlan::Assign { .. }));
    }

    #[test]
    fn plan_clears_a_stale_param_fn_on_bake() {
        // Switch 0 already carries a redundant param fn on (N, gain) → bake clears it.
        let ftsw = serde_json::json!([[
            onoff(&["N"], true),
            { "func": "param", "groupId": "G1", "nodeId": "N", "parameterId": "gain",
              "valueA": 0.9, "valueB": 0.4 }
        ]]);
        let p = preset_with(true, None, ftsw);
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)], false);
        match &plans[0] {
            FsLevelPlan::Bake { clear_stale, .. } => assert_eq!(*clear_stale, Some(1)),
            other => panic!("expected Bake with clear_stale, got {other:?}"),
        }
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
        let plans = plan_footswitch_jobs(&p["ftsw"], &p, &[key(0, -23.0)], false);
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
