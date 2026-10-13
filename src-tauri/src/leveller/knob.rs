//! Level-control writes: the scene-aware [`set_knob`] / [`set_knobs`] family (scene recall,
//! Scene Edit arming, the re-asserted `presetLevel`, the recall-and-save tail) and the
//! knob-search coordinate maps.

use super::*;

/// The field-8 saved doc `set_knobs` needs for its Scene Edit decision, for the ENTRY
/// POINTS that can't be handed one: the probe/legacy seams whose callers live
/// outside the leveling run that already read it (`level_preset_block`,
/// `mute_floor_report`, the bench runner). Reads ONLY when a
/// scene target is actually present, so every base/`presetLevel` path pays nothing, and
/// ONCE per run — the batched scene runners take the command's single read instead
/// (`read_saved_preset`'s read-once contract). `None` (read failed) makes `set_knobs`
/// refuse the scene write rather than guess.
pub(super) fn saved_for_scene_knobs<'a>(
    slot: ListIndex,
    knobs: impl IntoIterator<Item = &'a LevelKnob>,
) -> Option<serde_json::Value> {
    knobs
        .into_iter()
        .any(|k| {
            matches!(
                k,
                LevelKnob::Block {
                    scene_slot: Some(_),
                    ..
                }
            )
        })
        .then(|| read_saved_preset(slot))
        .flatten()
}

/// Recall base explicitly (`BASE_SCENE_SLOT`) + the standard post-recall settle —
/// shared by every base-context write/save seam that isn't already inside a
/// per-scene Scene Edit dance (which pays its own `SETTLE_AFTER_SCENE_RECALL_MS`
/// / `SETTLE_AFTER_SCENE_EDIT_MS` pair instead, in `set_knobs`).
pub(super) fn recall_base(s: &mut Session) -> Result<(), LevelError> {
    s.load_scene(crate::session::BASE_SCENE_SLOT)?;
    crate::settle(Duration::from_millis(SETTLE_AFTER_SET_MS));
    Ok(())
}

/// Recall the preset's ORIGINAL `lastLoadedScene` right before a save, so the save
/// re-stamps it instead of whatever scene the run left active (HW: a base-context
/// leveling save silently rewrote the preset's on-load scene, 3 → 8). Unsaved writes
/// survive the recall (HW, `probe --defer-scenes`). `None` = no recall (old behavior).
/// The ONE pre-save recall shared by `apply_levels`, `save_deferred_scene_writes`, and
/// `write_fs_values_on_session` — a settle-timing fix here fixes all three.
pub(super) fn recall_original_scene(
    s: &mut Session,
    restore_scene: Option<u32>,
) -> Result<(), LevelError> {
    if let Some(scene) = restore_scene {
        s.load_scene(scene)?;
        crate::settle(Duration::from_millis(SETTLE_AFTER_SET_MS));
    }
    Ok(())
}

/// The ONE pre-save sequence every batch/deferred save shares: recall the preset's original
/// scene, re-assert any unsaved `presetLevel` the recall just reverted, then save. The
/// recall's `loadScene` — base included — runs the device's own level-apply (danger.md's
/// "`load_preset` + `set_preset_level` in the same connection → the set is overridden"), so
/// an unsaved working-copy `presetLevel` reverts to the SAVED value right before the save
/// persists it. HW: `probe --levelpreset 400 -24 save` solved 0.3096 and the saved doc still
/// read the prior 0.32.
///
/// THE SHAPE SPLIT. That revert is LEAN-session behaviour — every caller of this seam saves
/// on a connection that did NOT `load_preset` in-session. A LIVE-edit session that loaded the
/// preset itself carries its unsaved `presetLevel` through scene recalls and into the save
/// (HW, fw 1.8.45-era `probe_redistribute_persist_check`: pl 0.42 read back against a saved
/// 0.53 after `loadScene(1)` + `loadScene(BASE)` + save), which is why
/// `restore_redistribution` does not route through here. Node/overlay writes are immune in
/// either shape, so only `Reassert::PresetLevel` needs re-writing.
///
/// The re-assert does NOT defeat the restore: `setPresetLevel` emits no `loadScene`, so the
/// scene the save stamps is still the recalled one. `reasserts` are written in the ORDER
/// GIVEN (every caller lists `PresetLevel` before a `Param`), each followed by
/// `SETTLE_AFTER_SET_MS` so the idle gap stays under the cliff.
pub(super) fn recall_reassert_save(
    s: &mut Session,
    slot: ListIndex,
    restore_scene: Option<u32>,
    reasserts: &[Reassert],
) -> Result<(), LevelError> {
    recall_original_scene(s, restore_scene)?;
    // Only a recall reverts the working copy, so only a recall needs undoing. Without one the
    // values are already in the device's working copy from this same session.
    if restore_scene.is_some() {
        for r in reasserts {
            match r {
                Reassert::PresetLevel(pl) => {
                    s.set_preset_level(*pl)?;
                }
                Reassert::Param {
                    group,
                    node,
                    param,
                    value,
                } => {
                    s.change_parameter(group, node, param, *value)?;
                }
            }
            crate::settle(Duration::from_millis(SETTLE_AFTER_SET_MS));
        }
    }
    s.save_current_preset(slot)?;
    Ok(())
}

/// Set the chosen knob to `value` on an open session (before re-amp engage). The
/// single-knob case of `set_knobs` — see its doc for the recall/write ordering
/// rules and what `saved` is for. Must be the FIRST write on the connection when
/// `knob` is a `Block`: its own scene recall, base or otherwise, reverts an
/// earlier unsaved `presetLevel` on this session (the lean-session level-apply —
/// callers here connect without an in-session `load_preset`; see
/// `recall_reassert_save`'s shape split) AND re-asserts that scene's own bypass
/// state (see `capture_on_session`'s doc), so a caller doing force-bypass
/// isolation must write the bypasses AFTER this call, never before
/// (`measure_knob_at`/`measure_fs_at` follow that order).
pub(crate) fn set_knob(
    s: &mut Session,
    knob: &LevelKnob,
    value: f32,
    saved: Option<&serde_json::Value>,
) -> Result<(), LevelError> {
    set_knobs(s, &[(knob, value)], saved)
}

/// Write ONLY the knob value (no scene re-activation) — the live loop's
/// mid-stream adjustment. The scene + per-block Scene Edit were already
/// activated by the initial `set_knob` on the same connection; re-loading the
/// scene while re-amp is engaged and audio is streaming is slow and risks
/// disturbing the engaged state.
pub(crate) fn set_knob_value_only(
    s: &mut Session,
    knob: &LevelKnob,
    value: f32,
) -> Result<(), String> {
    match knob {
        LevelKnob::PresetLevel => {
            s.set_preset_level(value)?;
            Ok(())
        }
        // `bypass` is a distinct wire message (`ChangeParameter.boolVal`, field 7) from
        // every other block param (a plain float field) — see `Session::change_parameter`
        // vs `change_parameter_bool`. A bypass value rides the same `(LevelKnob, f32)` shape
        // as every other knob (0.0/1.0 encoding), so the dispatch lives here rather than
        // widening `LevelKnob`. NOT the same shape as
        // `capture_on_session`/`measure_knob_at`/`measure_fs_at`'s typed
        // `&[(String, String, bool)]` force-bypass side channels — those write BEFORE a
        // separate connect/engage sequence, outside `set_knobs` entirely.
        LevelKnob::Block {
            group_id,
            node_id,
            parameter_id,
            ..
        } if parameter_id == "bypass" => {
            s.change_parameter_bool(group_id, node_id, parameter_id, value != 0.0)
        }
        LevelKnob::Block {
            group_id,
            node_id,
            parameter_id,
            ..
        } => s.change_parameter(group_id, node_id, parameter_id, value),
    }
}

/// Write a SET of block knobs that all belong to the SAME scene (or all to base),
/// doing the scene recall ONCE up front (NOT per knob — calling this per knob
/// individually re-`load_scene`s between writes, which reverts the prior knob's
/// unsaved value; a multi-knob caller must group its knobs by scene for exactly
/// this reason). Ordering: load scene → (per-scene only) enable Scene Edit on
/// every per-scene block → ONE settle → write every value.
///
/// * A target set with a `Block { scene_slot: Some(n), .. }` recalls scene `n`,
///   then enables Scene Edit on every per-scene block whose node has NO overlay in
///   that scene — and ONLY those. The enable RESEEDS the node's whole scene overlay
///   from base (HW 3-cell matrix, `probe_api/slot_write.rs`), so it is required
///   where the overlay is absent (it is what materialises it; without it the write
///   LEAKS TO BASE) and actively destructive where the overlay exists (the write
///   lands on the overlay anyway, and the enable wipes the scene's other stored
///   params). `saved` — the slot's field-8 preset JSON, read ONCE per run and
///   threaded (`scene_jobs::read_saved_preset`) — is the only source
///   that can tell the two apart: after `overlay_scene_onto_graph` a base value is
///   indistinguishable from an overlaid one, so the live docs can't answer it.
///   The decision itself is NOT taken here: it is
///   `scene_jobs::scene_write_verdict_for_param`, the ONE write-landing policy this
///   lane shares with the Doctor's prescription apply, so a `BypassOnly` node can't
///   be refused in one lane and written in the other.
///   `SceneOverlay::Unknown` (truncated read) or no `saved` at all ⇒ REFUSE the
///   batch before any device write; both write shapes corrupt from there. Mostly so
///   does `SceneOverlay::BypassOnly` — an overlay carrying only the bypass family
///   means that block's Scene Edit flag is DISABLED and the scene SHARES its knobs
///   with base, so the enable-dropped write lands on BASE and changes every sharing
///   scene (HW-verified fw 1.8.45) — UNLESS `scene_jobs::shared_write_is_scene_local`
///   confirms the leak is audible ONLY in this scene (every other sharing scene stays
///   bypassed, or pins the param with its own overlay), in which case the verdict
///   allows the base-landing write through as `WriteDirect` too. The refusal is a
///   per-scene skip, never a batch abort: `run_scene_jobs` turns a solve `Err` into a
///   `failed_scene_outcome` and continues, exactly like a `build_scene_jobs`
///   "no active guitar amp" skip.
///   Re-asserted on EVERY connection — including the enable. On fw 1.8.45 the scene
///   and the scene-edit enables read as not surviving the leveller's reconnects; on
///   fw 1.8.58 both survive (static RE: only the HID open flag and the send queue
///   reset), and the 1.8.45 symptom fits a same-slot `loadPreset` being a real reload
///   into `lastLoadedScene`, or a lapse-dropped `loadScene` (INFERRED). Re-asserting
///   stays safe, but a repeated enable on an overlay that is already Full reseeds it
///   from base. The enable repeats because `saved` is a pre-run
///   snapshot: an overlay this run just materialised still reads Absent, so the
///   enable repeats and reseeds again. Harmless, because the leveled write always
///   follows it inside the same batch — do NOT "optimise" it into a cached
///   per-node enabled set, which would drop the enable on a later connection where
///   the device no longer has scene edit armed.
///   The settles must stay SHORT: the device silently drops a write after a
///   ~400–450 ms pre-write IDLE gap (see `SETTLE_AFTER_SCENE_EDIT_MS` — measured as a
///   per-command cliff on fw 1.8.45, so a settle is slept ONLY when its command was sent);
///   the rare too-fast leak-to-base race is covered by the verify + correction
///   pass.
/// * A base-only target set (`Block { scene_slot: None, .. }`, no `Some`) gets
///   an explicit base recall — a bare write with no preceding `loadScene` lands
///   in whatever scene the connection defaults to (the preset's saved
///   `lastLoadedScene`), not necessarily base (HW). No Scene Edit: base params
///   are never scene-specific.
/// * A `PresetLevel`-only target set gets NO recall at all: `setPresetLevel` is
///   a different wire message and a global multiplier, not a scene-scoped
///   `changeParameter`, so a recall there would only risk reverting an unsaved
///   presetLevel write for no benefit.
pub(super) fn set_knobs(
    s: &mut Session,
    targets: &[(&LevelKnob, f32)],
    saved: Option<&serde_json::Value>,
) -> Result<(), LevelError> {
    // ONE scene per batch, ENFORCED not just documented: only the first scene found is
    // recalled below, so a batch mixing two scenes would land every write in that first
    // scene's overlay — silently, with each write confirming normally. Refuse instead.
    let mut scenes: Vec<u32> = targets
        .iter()
        .filter_map(|(k, _)| match k {
            LevelKnob::Block {
                scene_slot: Some(slot),
                ..
            } => Some(*slot),
            _ => None,
        })
        .collect();
    scenes.sort_unstable();
    scenes.dedup();
    if scenes.len() > 1 {
        return Err(LevelError::Device(format!(
            "set_knobs: batch mixes scenes {scenes:?}; only one scene may be recalled per batch, \
             so the others would silently write into scene {}",
            scenes[0]
        )));
    }
    let scene = scenes.first().copied();
    let has_base_block = targets.iter().any(|(k, _)| {
        matches!(
            k,
            LevelKnob::Block {
                scene_slot: None,
                ..
            }
        )
    });
    // ...and no BASE block may ride along with a scene block, for the same reason one step
    // removed: the branch below recalls the scene whenever `scene` is Some, so a base-scoped
    // knob (`scene_slot: None`) in that batch would be written under the scene overlay
    // instead of base — silently, confirming normally, exactly like the two-scene case.
    // One connection can hold one scene context, so base and scene targets cannot share a
    // batch; split them into two calls.
    if scene.is_some() && has_base_block {
        return Err(LevelError::Device(format!(
            "set_knobs: batch mixes base and scene {} targets; the scene recall would capture \
             the base write too — split them into separate calls",
            scene.unwrap_or_default()
        )));
    }
    // Which nodes need the Scene Edit enable — decided BEFORE any device write, so an
    // unanswerable overlay state refuses with the preset untouched. The verdict itself is
    // checked per (node, PARAM) — `scene_write_verdict_for_param`'s BypassOnly arm can answer
    // two params on the same node differently (one audibility-guarded, one not) — but the
    // enable send stays deduped by (group_id, node_id): two targets on one node share one
    // enable, and re-enabling an already-enabled node re-triggers the reseed (HW 3-cell
    // matrix) — not cosmetic. Neither `WriteDirect` arm (Full overlay, or an audibility-
    // guarded BypassOnly leak) ever needs the enable, so this dedup can't hide a missed one.
    let mut needs_enable: Vec<(&str, &str)> = Vec::new();
    if let Some(scene) = scene {
        let mut checked: Vec<(&str, &str, &str)> = Vec::new();
        for (k, _) in targets {
            if let LevelKnob::Block {
                group_id,
                node_id,
                parameter_id,
                scene_slot: Some(_),
            } = k
            {
                let key = (group_id.as_str(), node_id.as_str(), parameter_id.as_str());
                if checked.contains(&key) {
                    continue;
                }
                checked.push(key);
                // ONE write-landing policy for every scene-writing lane
                // (`scene_jobs::scene_write_verdict_for_param`, shared with the Doctor's
                // apply) — the four overlay states can never be answered two ways. The
                // no-saved-read case stays local: the verdict is a function OF a saved
                // document, so having none is this caller's own gap, and it keeps its own
                // wording.
                let Some(sv) = saved else {
                    return Err(LevelError::Device(format!(
                        "set_knobs: refusing to write {group_id}/{node_id} in scene {scene} — \
                         the saved preset does not say whether that node already has a scene \
                         overlay (no saved-preset read), and both write shapes corrupt it \
                         (enable reseeds an existing overlay from base; omitting it leaks the \
                         write to base)"
                    )));
                };
                match scene_write_verdict_for_param(sv, scene, node_id, parameter_id) {
                    SceneWriteVerdict::WriteDirect { .. } => {}
                    SceneWriteVerdict::NeedsEnable => {
                        let node_key = (group_id.as_str(), node_id.as_str());
                        if !needs_enable.contains(&node_key) {
                            needs_enable.push(node_key);
                        }
                    }
                    SceneWriteVerdict::Refuse { reason, .. } => {
                        return Err(LevelError::Device(format!("set_knobs: {reason}")))
                    }
                }
            }
        }
    }
    // Repro instrumentation: wall-clock every step relative to the loadScene recall,
    // to observe idle gaps crossing the ~400-450 ms silent-drop cliff (see
    // `SETTLE_AFTER_SCENE_EDIT_MS`).
    let t0 = std::time::Instant::now();
    if let Some(scene) = scene {
        s.load_scene(scene)?;
        log::info!(
            "set_knobs[t] scene {scene}: loadScene returned at {} ms",
            t0.elapsed().as_millis()
        );
        crate::settle(Duration::from_millis(SETTLE_AFTER_SCENE_RECALL_MS));
        for (group_id, node_id) in &needs_enable {
            s.set_node_scene_edit(group_id, node_id, true)?;
            log::info!(
                "set_knobs[t]: SceneEdit({node_id}) returned at {} ms",
                t0.elapsed().as_millis()
            );
        }
        // Settle ONLY when an enable was actually sent — a settle for a command that
        // wasn't is pure idle and rides the ~400-450 ms silent-drop cliff (see
        // `SETTLE_AFTER_SCENE_EDIT_MS`'s doc for the HW evidence).
        if !needs_enable.is_empty() {
            crate::settle(Duration::from_millis(SETTLE_AFTER_SCENE_EDIT_MS));
        }
    } else if has_base_block {
        recall_base(s)?;
    }
    for (k, v) in targets {
        set_knob_value_only(s, k, *v)?;
        log::info!(
            "set_knobs[t]: write {}={v:.4} returned at {} ms",
            k.label(),
            t0.elapsed().as_millis()
        );
    }
    Ok(())
}

/// `(log_space, c_lo, c_hi)` for a knob-search coordinate map — the SAME log-vs-identity
/// shape as [`FsParamTarget::to_coord`]/[`FsParamTarget::coord_to_value`], but a DIFFERENT
/// discriminator: this seam has no `ParamClass` to consult, so it infers log-space from the
/// BOUNDS SHAPE (`[0, 1]`-like) instead of the param's own classification. Deliberately not
/// merged with `FsParamTarget`'s class-based map — see that type's doc for why.
pub(crate) fn knob_search_space(lo: f32, hi: f32) -> (bool, f32, f32) {
    let log_space = lo >= 0.0 && hi <= 1.0 + 1e-6;
    let eps = 1e-3f32;
    let to_c = |x: f32| {
        if log_space {
            20.0 * x.max(eps).log10()
        } else {
            x
        }
    };
    let c_lo = to_c(if log_space { lo.max(eps) } else { lo });
    let c_hi = to_c(hi);
    (log_space, c_lo, c_hi)
}

/// See [`knob_search_space`]'s doc for the discriminator this pairs with.
pub(crate) fn knob_to_coord(value: f32, log_space: bool) -> f32 {
    if log_space {
        20.0 * value.max(1e-3).log10()
    } else {
        value
    }
}

/// See [`knob_search_space`]'s doc for the discriminator this pairs with.
pub(crate) fn coord_to_knob(coord: f32, log_space: bool, lo: f32, hi: f32) -> f32 {
    if log_space {
        10f32.powf(coord / 20.0).clamp(lo, hi)
    } else {
        coord.clamp(lo, hi)
    }
}
