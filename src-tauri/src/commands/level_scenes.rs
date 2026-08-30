//! Per-scene leveling commands + setlist common-target leveling.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// One resolved amp knob: `(group_id, node_id, current_outputLevel)`.
pub(crate) type AmpKnobSpec = (String, String, f32);
/// A candidate leveling knob for `level_scenes_apply` — the frontend passes EVERY
/// amp-level candidate (it owns amp-ness via the models catalog); the backend picks
/// PER SCENE the one whose block is actually ON in that scene.
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LevelBlockArg {
    pub(crate) group_id: String,
    pub(crate) node_id: String,
    pub(crate) parameter_id: String,
    pub(crate) value: f32,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SceneLevelProgressItem {
    scene_slot: u32,
    status: String,
    result: Option<leveller::LevelResult>,
    message: Option<String>,
    /// B6 (issue 6b): a batch-wide caption ("Saving preset…" / "Verifying…") for the
    /// deferred-save/persist-verify phases, which have no single scene to report progress
    /// against. Rides on its OWN item — see [`tail_progress_item`] — never alongside a real
    /// per-scene `result`/`message`, so `message` (the active row's own caption) never has to
    /// double as this and start lying about which row it describes.
    tail: Option<String>,
}

/// One scene-leveling request from the wizard: a wire scene slot + its OWN loudness
/// target. Per-job targets (mirroring `FootswitchLevelJob`) let a preset with a mix of
/// targets level in ONE batch — one prepass, one runner, one deferred save.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SceneLevelJobArg {
    scene_slot: u32,
    target_lufs: f64,
    /// The user's OWN control for this scene. Absent = the amp-`outputLevel` path (joint-k,
    /// rebalance, every existing caller).
    #[serde(default)]
    handle: Option<SceneHandleArg>,
}

/// A user-chosen scene leveling control: the block param the solve should sweep INSTEAD of
/// the active amp's `outputLevel`.
#[derive(serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SceneHandleArg {
    group_id: String,
    node_id: String,
    parameter_id: String,
}

/// A1 — "the base anchor" (PR A design doc): keeps the batch's force-appended base job alive
/// through PHASE 1 (prepass) and PHASE 2 (trade) with NO wire scene job of its own, so the
/// headroom trade can be planned/executed even though the wizard levels base itself through
/// the separate `level_preset` lane. Stripped before PHASE 3 — base's own `outputLevel` is
/// never solved by this batch when it arrived only as an anchor.
#[derive(serde::Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BaseAnchorArg {
    target_lufs: f32,
}

/// A1, PURE: derive `(anchor_only_base, base_in_plan)` from the wire inputs — the whole anchor
/// decision, exercised without any device/session in the loop.
///
/// `anchor_only_base` — the anchor is the ONLY reason base is in this batch (no wire job named
/// it): its PHASE 1 progress items have no frontend row to route to, and it must be stripped
/// before PHASE 3 (base's own `outputLevel` is never solved by this batch when it arrived only
/// as an anchor — the wizard's separate `level_preset` lane owns that write).
///
/// `base_in_plan` — base survives the PHASE 1/2 strip at all, either because the caller
/// explicitly requested it (`base_requested`) or because a VALID anchor delivered it.
///
/// A non-finite anchor target degrades to `(false, base_requested)` — exactly today's
/// behavior — rather than a failed run: the anchor is advisory infrastructure for the trade,
/// never a hard requirement of the batch it rides on.
fn base_anchor_plan(base_requested: bool, base_anchor: Option<BaseAnchorArg>) -> (bool, bool) {
    let anchor_valid = base_anchor.is_some_and(|a| a.target_lufs.is_finite());
    let anchor_only_base = anchor_valid && !base_requested;
    let base_in_plan = base_requested || anchor_valid;
    (anchor_only_base, base_in_plan)
}

#[cfg(test)]
mod base_anchor_plan_tests {
    use super::*;

    fn anchor(target: f32) -> BaseAnchorArg {
        BaseAnchorArg {
            target_lufs: target,
        }
    }

    // The wizard-shaped run: base arrives ONLY via the anchor. It must survive into PHASE 1/2
    // (`base_in_plan`) but is flagged `anchor_only_base` — stripped before PHASE 3, and its
    // progress items have no frontend row to route to (suppressed at the two send sites).
    #[test]
    fn a_valid_anchor_with_no_wire_base_job_is_anchor_only_and_in_plan() {
        assert_eq!(base_anchor_plan(false, Some(anchor(-23.0))), (true, true));
    }

    // A `base_requested` caller (the wire jobs literally name base) is never "anchor only" —
    // base already has a wire target of its own, anchor or not.
    #[test]
    fn base_requested_is_never_anchor_only_even_with_a_valid_anchor() {
        assert_eq!(base_anchor_plan(true, Some(anchor(-23.0))), (false, true));
        assert_eq!(base_anchor_plan(true, None), (false, true));
    }

    // No anchor, no request: today's behavior, byte-identical.
    #[test]
    fn no_anchor_and_no_request_is_out_of_the_batch_entirely() {
        assert_eq!(base_anchor_plan(false, None), (false, false));
    }

    // A9/F3: anchor validation failure (a non-finite target) degrades to NO trade — exactly
    // `base_requested`'s own plan — never a failed run. `plan_trade_for_batch` then simply
    // finds no base row in the batch (gate #2 in its own doc) and answers `TradeDecision::None`.
    #[test]
    fn a_non_finite_anchor_target_degrades_to_no_anchor_never_a_failure() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(
                base_anchor_plan(false, Some(anchor(bad))),
                (false, false),
                "non-finite anchor target {bad} must not put base in the plan"
            );
            // And it must not silently strip a GENUINELY requested base either.
            assert_eq!(base_anchor_plan(true, Some(anchor(bad))), (false, true));
        }
    }
}

/// A5/F2 (TRADE-PATH GAP): the restore list for a BASE-REQUESTED job's own isolation — the base
/// job survives PHASE 3 whenever it arrived as a real wire job (not stripped as anchor-only),
/// and its own solve there routes through `apply_levels(defer: true)`, which re-asserts
/// `force_bypass` on every capture and never undoes it (that seam exists to keep the isolation
/// alive across a multi-capture secant, not to clean it up). This is a DIFFERENT seam from
/// `apply_headroom_trade`'s own `undo_base_isolation`, which — if it ran at all — ran BEFORE
/// this job solves again, so the two are independent: this fires whether or not a trade landed
/// (a no-trade `base_requested` run isolates and solves base exactly the same way). The
/// derivation itself (including the `saved == None` policy) is `leveller::isolation_restore_
/// list`'s doc, shared by every restore-list owner in the leveling path.
fn isolation_restore_for_batch(
    scene_jobs: &[leveller::SceneJob],
    saved: Option<&serde_json::Value>,
) -> Vec<(String, String, bool)> {
    scene_jobs
        .iter()
        .find(|sj| sj.scene_slot == session::BASE_SCENE_SLOT)
        .map(|base| leveller::isolation_restore_list(&base.force_bypass, saved))
        .unwrap_or_default()
}

#[cfg(test)]
mod isolation_restore_for_batch_tests {
    use super::*;

    fn base_job(force_bypass: Vec<(String, String, bool)>) -> leveller::SceneJob {
        leveller::SceneJob {
            scene_slot: session::BASE_SCENE_SLOT,
            target_lufs: -23.0,
            knobs: Vec::new(),
            skip: None,
            rebalanceable: false,
            handle: None,
            prepass: None,
            force_bypass,
        }
    }

    fn saved_with_bypass(node: &str, bypassed: bool) -> serde_json::Value {
        serde_json::json!({
            "audioGraph": { "guitarNodes": {
                "G1": [ { "nodeId": node, "FenderId": node,
                          "dspUnitParameters": { "bypass": bypassed } } ]
            } }
        })
    }

    // THE base_requested SHAPE: base is IN the PHASE-3 jobs (not stripped as anchor-only) and
    // carries a non-empty `force_bypass` — the run must derive the exact inverse (the node's
    // ORIGINAL saved bypass, not a blind `!forced`), independent of whether a trade landed.
    #[test]
    fn a_base_requested_job_with_isolation_derives_its_inverse_from_the_saved_doc() {
        let jobs = vec![base_job(vec![(
            "G1".to_string(),
            "pedal".to_string(),
            true,
        )])];
        let saved = saved_with_bypass("pedal", false);
        let restore = isolation_restore_for_batch(&jobs, Some(&saved));
        assert_eq!(
            restore,
            vec![("G1".to_string(), "pedal".to_string(), false)],
            "must restore the SAVED (pre-isolation) value, not invert the forced flag"
        );
    }

    // NO-TRADE CASE: this derivation reads only `scene_jobs`/`saved` — nothing about a trade
    // having landed — so a base-requested run with no trade at all still gets its isolation
    // cleaned up. (The trade's OWN `force_bypass_restore` is a separate, independent source —
    // see `run_scene_jobs`'s union.)
    #[test]
    fn the_derivation_does_not_depend_on_a_trade_having_landed() {
        let jobs = vec![base_job(vec![(
            "G1".to_string(),
            "pedal".to_string(),
            true,
        )])];
        let saved = saved_with_bypass("pedal", true);
        // No `TradeHold` anywhere in this call — the function doesn't even take one.
        let restore = isolation_restore_for_batch(&jobs, Some(&saved));
        assert_eq!(restore, vec![("G1".to_string(), "pedal".to_string(), true)]);
    }

    #[test]
    fn no_base_job_in_the_batch_yields_no_restore_list() {
        let jobs = vec![leveller::SceneJob {
            scene_slot: 0,
            target_lufs: -23.0,
            knobs: Vec::new(),
            skip: None,
            rebalanceable: false,
            handle: None,
            prepass: None,
            force_bypass: Vec::new(),
        }];
        assert!(isolation_restore_for_batch(&jobs, None).is_empty());
    }

    #[test]
    fn a_base_job_with_no_isolation_yields_an_empty_list() {
        let jobs = vec![base_job(Vec::new())];
        assert!(isolation_restore_for_batch(&jobs, None).is_empty());
    }

    // UNIFIED None-POLICY (post-review dedup): a base job WITH isolation but no saved
    // document to restore from yields EMPTY, never a guessed `false` for every forced node —
    // see `leveller::isolation_restore_list`'s doc for why a blind `false` would risk actively
    // un-bypassing a pedal the player had deliberately engaged.
    #[test]
    fn a_base_job_with_isolation_and_no_saved_doc_yields_no_restore_never_a_guessed_false() {
        let jobs = vec![base_job(vec![(
            "G1".to_string(),
            "pedal".to_string(),
            true,
        )])];
        assert!(isolation_restore_for_batch(&jobs, None).is_empty());
    }
}

/// Wire payload for `tmp://leveling-lufs` — the advisory live measured loudness streamed
/// while a leveling capture runs, so the UI can show a "measuring…" readout. ADVISORY: this
/// is the loudness at the reference level, NOT the final preset level (the result row is the
/// confirm). `momentary` is the current hop's plain RMS in dB (decorative fuel for the live
/// VU bars, not the solve). Mirrored in `src/lib/types.ts`.
#[derive(Clone, serde::Serialize)]
pub(crate) struct LiveLufsEvent {
    lufs: f64,
    momentary: f64,
}

/// RAII guard: installs an advisory live-LUFS sink that emits `tmp://leveling-lufs` for the
/// lifetime of a leveling run, clearing it on drop (incl. unwind). Every leveling command
/// runs serialized under the device-op lock, so only one guard is ever live at a time.
pub(crate) struct LiveLufsGuard;

impl LiveLufsGuard {
    pub(crate) fn install<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Self {
        use tauri::Emitter;
        audio::set_live_lufs_sink(Box::new(move |lufs, momentary| {
            let _ = app.emit("tmp://leveling-lufs", LiveLufsEvent { lufs, momentary });
        }));
        LiveLufsGuard
    }
}

impl Drop for LiveLufsGuard {
    fn drop(&mut self) {
        audio::clear_live_lufs_sink();
    }
}

/// Pre-run guard for every leveling lane that accumulates UNSAVED scene writes
/// across scene recalls and saves once at batch end (gap 2 of
/// `notes/device-manual-gaps.md`): refuse before ANY device write when the device's
/// `Scene Change Behavior` snapshot reads `DISCARD CHANGES`. Under `DISCARD` every
/// recall silently reverts the recalled scene's unsaved edits (HW-confirmed on the
/// touchscreen) — the batch-end save would then persist the reverted state, and a
/// save has no revert (danger.md). Call sites: `level_scenes_apply_batched`,
/// `level_footswitches_apply`, `redistribute_headroom`, `restore_redistribution`
/// (the deferred-write lanes; the legacy per-scene lane and `level_setlist` save
/// immediately / re-assert through `recall_reassert_save`, so they are safe under
/// `DISCARD` and deliberately unguarded).
///
/// `settings_path` is the settings snapshot the startup backup read persisted
/// (`support/device-settings.json`). It can be STALE — the touchscreen may have been
/// edited since connecting — and the asymmetry mirrors `calibrate_profile`'s #124
/// fader handling: only a positively-read `Discard` refuses (the failure it causes is
/// silent corruption of a destructive save, and the wrongly-refused stale case is
/// recovered by the same replug the message names, since a detach fires
/// `resetLibraryScan` and the next connection re-reads the settings). An absent or
/// unreadable snapshot, or an unknown ordinal, PROCEEDS — the factory default is
/// `MAINTAIN CHANGES`, and blocking every fresh install on a missing snapshot would
/// punish the common case on no evidence.
///
/// Refuse, not warn: the app is click-only for non-technical users, and a warning
/// that can be clicked through recreates the corruption it exists to prevent. The
/// guard fires regardless of `save` — even a no-save run's solves read state that
/// `DISCARD` reverts mid-run, so its numbers would be garbage.
pub(crate) fn scene_discard_guard(settings_path: Option<&std::path::Path>) -> Result<(), String> {
    let behavior = crate::backup_read::read_settings_snapshot(settings_path)
        .and_then(|json| crate::backup_read::scene_change_behavior(&json));
    if behavior == Some(crate::backup_read::SceneChangeBehavior::Discard) {
        return Err(
            "this device's Scene Change Behavior is set to DISCARD CHANGES, which silently \
             reverts the unsaved scene changes a leveling run accumulates before its save. \
             On the unit's touchscreen, set SETTINGS \u{2192} Scene Change Behavior to \
             MAINTAIN CHANGES, then unplug and replug the USB cable so the app re-reads the \
             setting, and run leveling again."
                .to_string(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod scene_discard_guard_tests {
    use super::*;

    /// A settings snapshot on disk, exactly as `persist_device_settings` leaves one
    /// (`support/device-settings.json` — the guard's real input). Removed on drop.
    struct Snapshot(std::path::PathBuf);
    impl Drop for Snapshot {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn snapshot(tag: &str, contents: &str) -> Snapshot {
        let path = std::env::temp_dir().join(format!(
            "tmp-companion-scene-discard-{}-{tag}.json",
            std::process::id()
        ));
        std::fs::write(&path, contents).expect("write snapshot");
        Snapshot(path)
    }

    // THE gate for gap 2: a snapshot that positively reads DISCARD refuses, and the
    // message carries the two things the user needs — the touchscreen setting to
    // change (by its on-unit name) and the replug that refreshes the stale snapshot.
    #[test]
    fn a_discard_snapshot_refuses_with_the_touchscreen_setting_named() {
        let f = snapshot("discard", r#"{"sceneChangeBehavior":1,"mixerSaveData":{}}"#);
        let err = scene_discard_guard(Some(&f.0)).expect_err("DISCARD must refuse");
        assert!(err.contains("Scene Change Behavior"), "{err}");
        assert!(err.contains("MAINTAIN CHANGES"), "{err}");
        assert!(err.contains("replug"), "{err}");
    }

    #[test]
    fn the_retain_default_proceeds() {
        let f = snapshot("retain", r#"{"sceneChangeBehavior":0}"#);
        assert!(scene_discard_guard(Some(&f.0)).is_ok());
    }

    // The deliberate asymmetry: no snapshot, no key, garbage JSON, or an ordinal the
    // enum does not carry all PROCEED — the factory default is MAINTAIN, and only a
    // positively-read DISCARD is evidence of the silent-revert mechanism.
    #[test]
    fn an_absent_or_unreadable_snapshot_proceeds() {
        assert!(scene_discard_guard(None).is_ok());
        assert!(
            scene_discard_guard(Some(std::path::Path::new("/nonexistent/settings.json"))).is_ok()
        );
        for (tag, contents) in [
            ("no-key", r#"{"mixerSaveData":{}}"#),
            ("garbage", "not json"),
            ("unknown-ordinal", r#"{"sceneChangeBehavior":7}"#),
        ] {
            let f = snapshot(tag, contents);
            assert!(
                scene_discard_guard(Some(&f.0)).is_ok(),
                "must proceed on {contents:?}"
            );
        }
    }
}

pub(crate) static SCENE_LEVEL_CANCEL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
pub(crate) fn cancel_scene_leveling() {
    SCENE_LEVEL_CANCEL.store(true, SeqCst);
    // Also wake the in-flight capture/settle waits (see `device_gate::OP_ABORT`).
    crate::request_op_abort();
}

fn pick_scene_level_knob(
    slot: u32,
    scene: u32,
    candidates: &[LevelBlockArg],
) -> Result<(leveller::LevelKnob, f32, f32, f32), String> {
    let scene_slot = if scene >= session::BASE_SCENE_SLOT {
        None
    } else {
        Some(scene)
    };
    // ONE rich session (HW-rearchitected): heartbeat warmup → loads
    // via send_and_collect → live doc from the accumulated field-3 pushes. The
    // old connect → load → drop → connect_for_discovery chain is broken on fw
    // 1.8.45 twice over: a close chased by a re-open wedges the device's next
    // exclusive open (0xe00002c5 lockout), and field-78 kills field-3 delivery
    // for its whole session anyway. After each load the raw accumulator is
    // cleared so the doc reflects the POST-scene live state (the pick must read
    // the sounding graph, never stale pre-scene pushes).
    let live_doc = {
        let mut s = Session::connect()?;
        for _ in 0..16 {
            s.heartbeat()?;
            s.pump_collect(120)?;
        }
        s.raw.clear();
        s.send_and_collect(&proto::load_preset((slot + 1) as u64, 1), 300)?;
        for _ in 0..8 {
            s.heartbeat()?;
            s.pump_collect(200)?;
        }
        if let Some(sl) = scene_slot {
            s.raw.clear();
            s.send_and_collect(&proto::load_scene(sl as u64), 300)?;
            for _ in 0..8 {
                s.heartbeat()?;
                s.pump_collect(200)?;
            }
        }
        s.current_preset_value()?
    };
    for c in candidates {
        log::info!(
            "pick_scene_level_knob scene={scene} candidate {}/{}/{} live_bypass={:?}",
            c.group_id,
            c.node_id,
            c.parameter_id,
            scenes::block_bypass_in_live_graph(&live_doc, &c.group_id, &c.node_id),
        );
    }
    let picked = candidates
        .iter()
        .filter(|c| is_amp_output_level_param(&c.parameter_id))
        .find(|c| {
            scenes::block_bypass_in_live_graph(&live_doc, &c.group_id, &c.node_id) == Some(false)
        })
        .ok_or_else(|| format!("no active amp outputLevel control found for scene slot {scene}"))?;
    let (lo, hi) = knob_bounds(picked.value);
    Ok((
        leveller::LevelKnob::Block {
            group_id: picked.group_id.clone(),
            node_id: picked.node_id.clone(),
            parameter_id: picked.parameter_id.clone(),
            scene_slot,
        },
        lo,
        hi,
        picked.value,
    ))
}

/// Level ONE scene the capture-per-connection way (`level_preset_block`): pick
/// the scene's knob from its live graph, then closed-loop with fresh re-amp
/// captures. The legacy `level_scenes_apply` path; the shipped batched flow is
/// `level_scenes_apply_batched` → `leveller::level_scenes_oneshot` (or
/// `level_scenes_rebalance` for the parallel-amp option) — NOT the retired
/// bench-only `level_scenes_live_batched` (see notes/leveling.md).
fn level_one_scene_legacy(
    slot: u32,
    scene: u32,
    candidates: &[LevelBlockArg],
    stimulus: &[f32],
    target_lufs: f64,
    save: bool,
) -> Result<leveller::LevelResult, String> {
    let (knob, lo, hi, _current) = pick_scene_level_knob(slot, scene, candidates)?;
    // 800 ms before the leveller's first fresh connect — the empirical safe gap
    // after a rich-session close (shorter chases trip the device's open lockout).
    crate::settle(std::time::Duration::from_millis(800));
    let opts = leveller::LevelOptions {
        save,
        verify: true,
        ..Default::default()
    };
    leveller::level_preset_block(slot, stimulus, &knob, lo, hi, target_lufs, opts, || false)
}

/// Per-scene leveling APPLY (chosen mechanism: enable scene mode on the amp
/// block, level only the amp `outputLevel` control). For each selected scene, drive
/// the scene's ACTIVE amp's `outputLevel` knob closed-loop to `target_lufs` with
/// per-block Scene Edit enabled —
/// so the level lands on that scene's overlay, not the base. The knob is resolved
/// PER SCENE from `candidates` by the scene overlay's `bypass` (HW-found:
/// a preset can carry several amps with scenes swapping which is live — leveling a
/// bypassed amp's knob measures flat and clamps).
/// `scene_slots` are the WIRE slots: 0-based `scenes[]` indices for FS scenes;
/// `session::BASE_SCENE_SLOT` (8) = the base/preset value (levelled WITHOUT scene-edit
/// — a preset load activates base, so no scene recall is needed).
/// DEVICE WRITE when `save` — opt-in, gated by the read-only HW policy + the leveling
/// overlay confirm. Reuses `level_preset_block` (the scene context rides the knob and
/// is re-asserted on every connection). Each scene is a self-contained leveling pass.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn level_scenes_apply(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    slot: u32,
    scene_slots: Vec<u32>,
    candidates: Vec<LevelBlockArg>,
    target_lufs: f64,
    save: bool,
    topology_id: Option<String>,
    calibration_lufs: Option<f32>,
) -> Result<Vec<leveller::LevelResult>, String> {
    if !candidates
        .iter()
        .any(|c| is_amp_output_level_param(&c.parameter_id))
    {
        return Err("per-scene leveling needs at least one amp outputLevel candidate".to_string());
    }
    if scene_slots.is_empty() {
        return Err("no scenes selected".to_string());
    }
    let target_lufs = target_lufs + playback_offset_for(&app, topology_id.as_deref());
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;
        let run = || -> Result<Vec<leveller::LevelResult>, String> {
            let mut results = Vec::with_capacity(scene_slots.len());
            for scene in &scene_slots {
                let r = level_one_scene_legacy(
                    slot,
                    *scene,
                    &candidates,
                    &stim,
                    target_lufs,
                    save,
                )?;
                log::info!(
                    "level_scenes_apply slot={slot} scene={scene} save={save} final_level={:.4} measured={:.2} clamped={}",
                    r.final_level, r.measured_lufs, r.clamped,
                );
                results.push(r);
            }
            Ok(results)
        };
        let result = run();
        // Run-end backstop, success or failure (see `reamp_off_guaranteed`: the
        // device drops an in-session OFF sent after ~1 s of idle — every capture).
        leveller::reamp_off_guaranteed("level_scenes_apply");
        result
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn level_scenes_apply_batched<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    slot: u32,
    jobs: Vec<SceneLevelJobArg>,
    candidates: Vec<LevelBlockArg>,
    save: bool,
    rebalance: bool,
    topology_id: Option<String>,
    calibration_lufs: Option<f32>,
    profile_id: Option<String>,
    // A1: the wizard's Base row levels through the separate `level_preset` lane and hands its
    // OWN target here so PHASE 1/2 can plan (and PHASE 2 execute) the headroom trade against
    // it. `None` = today's behavior verbatim.
    base_anchor: Option<BaseAnchorArg>,
    on_result: tauri::ipc::Channel<SceneLevelProgressItem>,
) -> Result<Vec<leveller::LevelResult>, String> {
    if jobs.is_empty() {
        return Err("no scenes selected".to_string());
    }
    // Gap-2 pre-run guard: refuse under a DISCARD `Scene Change Behavior` snapshot
    // before anything touches the device — see `scene_discard_guard`'s doc.
    scene_discard_guard(crate::commands::presets::device_settings_path(&app).as_deref())?;
    // A row that names its own control needs no amp candidate and no routing classification —
    // the user picked the knob. So this pre-device guard fires only for a batch where NOBODY
    // named one (every row is an amp-`outputLevel` joint-k row and the whole run is doomed);
    // a MIXED batch proceeds and `build_scene_jobs_with_handles` skips just the amp rows.
    if jobs.iter().all(|j| j.handle.is_none())
        && !candidates
            .iter()
            .any(|c| is_amp_output_level_param(&c.parameter_id))
    {
        return Err("per-scene leveling needs at least one amp outputLevel candidate".to_string());
    }
    SCENE_LEVEL_CANCEL.store(false, SeqCst);
    // Playback compensation is one offset for the whole batch; each job's own target
    // gets it added below (the per-scene targets differ, the offset does not).
    let offset = playback_offset_for(&app, topology_id.as_deref());
    let (stim_path, calibration_lufs) = resolve_stimulus_for_leveling(
        &app,
        None,
        topology_id,
        profile_id.as_deref(),
        calibration_lufs,
    )?;
    let app_evt = app.clone();
    with_released_seize(state.session.clone(), move || {
        // Stream advisory live LUFS while each capture runs (dropped at closure end).
        let _lufs = LiveLufsGuard::install(app_evt);
        let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;
        let mut scene_slots: Vec<u32> = jobs.iter().map(|j| j.scene_slot).collect();
        // Force-append base so the prepass always harvests a base doc — one more chance at a
        // complete `audioGraph.template` for `build_scene_jobs`' routing classification (the
        // scene-vs-base repair diff it was originally added for is gone: `set_knobs` now
        // enables Scene Edit only where the node has no overlay, so nothing gets reseeded
        // away). Stripped back out below (before the wire-job match) when the user never
        // asked to level base itself.
        let base_requested = scene_slots.contains(&session::BASE_SCENE_SLOT);
        if !base_requested {
            scene_slots.push(session::BASE_SCENE_SLOT);
        }
        // A1: see `base_anchor_plan`'s doc — anchor validation degrades to no-trade, never a
        // failed run.
        if base_anchor.is_some_and(|a| !a.target_lufs.is_finite()) {
            log::warn!(
                "slot {slot}: base_anchor target is not finite — ignoring it (no headroom trade \
                 this run)"
            );
        }
        let (anchor_only_base, base_in_plan) = base_anchor_plan(base_requested, base_anchor);

        // A2: the freshness barrier belongs BEFORE the saved read, unconditionally (not
        // anchor-gated) — the wizard's OWN `level_preset` lane may have saved this exact slot
        // seconds ago (lazy commit T+45-100s, danger.md), and a scenes-only batch run right
        // after it pays the identical race. Tell the wizard why nothing moves for up to ~2 min
        // before paying the wait (mirrors `level_footswitch.rs`'s barrier caption verbatim).
        if leveller::slot_save_pending_commit(slot) {
            let _ = on_result.send(SceneLevelProgressItem {
                scene_slot: jobs[0].scene_slot,
                status: "active".to_string(),
                result: None,
                message: Some(leveller::WAITING_FOR_COMMIT_MSG.to_string()),
                tail: None,
            });
        }
        leveller::ensure_fresh_load(slot, &mut || SCENE_LEVEL_CANCEL.load(SeqCst))?;
        // The slot's registered `presetLevel` witness — set by the run's OWN preceding
        // `level_preset` base save — in PREFERENCE to the doc's parsed value: field-8 is
        // read-your-writes and the barrier above already gates the wait, but inside the lazy
        // commit window the device's LOAD STORE (what a later recall serves) can still lag the
        // registry for a beat longer than the harvest above can observe.
        let intended_preset_level_seed = leveller::registered_preset_level(slot);

        // THE field-8 read for this preset (one per run, before any other session — nothing
        // has just closed one here, and it leaves the validated prepass→runner boundary
        // below untouched). Feeds the raw per-node scene overlays (`scene_jobs::
        // scene_overlay`, the Scene Edit enable + bake gates) AND `build_scene_jobs`'
        // routing-structure fallback — which still only fills in for a live doc set that
        // lacks `audioGraph.template`, so an unconditional `Some` changes no classification.
        //
        // COMPLETE-OR-FAIL, not the tolerant read: a truncated `scenes` tail makes the
        // planner blind to the scenes it cuts, and the run then levels the visible ones and
        // leaves the rest silently untouched (`read_saved_preset_complete`'s doc carries the
        // HW evidence). Refusing with a readable error beats half-leveling a preset.
        //
        // A3: widened to `["scenes", "ftsw"]` when base is in this batch — `ftsw` feeds the
        // base isolation derivation below, and a PARTIAL `ftsw` silently under-isolates (some
        // footswitch-owned blocks never forced off), so it must be required, not optional.
        let saved = Some(if base_in_plan {
            crate::probe_api::scene_jobs::read_saved_preset_complete_sections(
                slot,
                &["scenes", "ftsw"],
            )?
        } else {
            crate::read_saved_preset_complete(slot)?
        });
        type BatchOutcome = (
            Vec<leveller::BatchedSceneOutcome>,
            Option<crate::headroom_trade::TradeSummary>,
        );
        let run_batched = |save_run: bool| -> Result<BatchOutcome, String> {
            // Un-engaged pre-pass (scene docs → jobs), then the ONE-SHOT runner:
            // amp `outputLevel` is linear in dB, so each scene is measured once at a
            // reference level (ISOLATED fresh re-amp capture) and solved exactly — the
            // BatchedLive shared-stream loop mis-measured scenes (HW).
            // `restore_scene` = the preset's original active scene: the batch-end
            // single save recalls it first so the preset persists in the same
            // base/scene/footswitch state it was loaded in.
            // DARK: overlay path validated by `probe --overlay-ab` (76/76 scene-amp pairs,
            // 0 bypass mismatches) but adoption is a gated follow-up — flip to `true` then
            // (see prepass_scene_docs_via's adoption-time TODO). `false` = live prepass today.
            let (docs, restore_scene) = prepass_scene_docs_via(slot, &scene_slots, false)?;
            // Inter-session HID gap: the prepass session has just closed; the one-shot
            // runner opens a fresh one. Reuse the leveller's HW-proven open-after-close
            // gap (was a hard-coded 800, copied from the bench). build_scene_jobs below
            // is pure CPU, so this is the only wait here.
            crate::settle(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
            // `build_scene_jobs` stamps a base target on every job; override each with its
            // OWN wire job's offset-adjusted target (match by scene slot) so a mixed-target
            // preset levels in this ONE batch. `jobs` is non-empty (guarded above).
            let base_target = jobs[0].target_lufs + offset;
            // Each row's own control, threaded INTO the builder (sparse, keyed by wire scene
            // slot): a handle row is built from that param and never consults the amp
            // classifier, so an unreadable routing template can only skip the rows that
            // actually need the amp.
            let handles: Vec<(u32, SceneHandleSpec)> = jobs
                .iter()
                .filter_map(|j| {
                    j.handle.as_ref().map(|h| {
                        (
                            j.scene_slot,
                            SceneHandleSpec {
                                group_id: &h.group_id,
                                node_id: &h.node_id,
                                parameter_id: &h.parameter_id,
                            },
                        )
                    })
                })
                .collect();
            let mut scene_jobs = build_scene_jobs_with_handles(
                &scene_slots,
                &candidates,
                &docs,
                base_target,
                saved.as_ref(),
                &handles,
            )?;
            // A1: `base_in_plan` replaces `base_requested` in the strip — an anchor-only base
            // survives past PHASE 1/2 exactly like a `base_requested` one, and is stripped
            // separately, right before PHASE 3, below.
            if !base_in_plan {
                scene_jobs.retain(|sj| sj.scene_slot != session::BASE_SCENE_SLOT);
            } else if let Some(base_job) = scene_jobs
                .iter_mut()
                .find(|sj| sj.scene_slot == session::BASE_SCENE_SLOT)
            {
                // A3: "base means base" — isolate every footswitch-owned on/off block exactly
                // like `level_preset`'s own base lane (preset 28's TubeScreamer is saved ON; an
                // un-isolated base capture would measure it and silently under-isolate).
                // Derived ONCE from the (now ftsw-widened) saved doc via the shared seam; an
                // absent/empty `ftsw` degrades to no isolation, never an error.
                let empty_ftsw = serde_json::Value::Null;
                let ftsw = saved
                    .as_ref()
                    .and_then(|s| s.get("ftsw"))
                    .unwrap_or(&empty_ftsw);
                base_job.force_bypass = saved
                    .as_ref()
                    .map(|s| crate::commands::doctor::doctor_force_bypass(ftsw, s, None))
                    .unwrap_or_default();
            }
            // A1/F4: base's isolated prepass capture must run LAST in PHASE 1, or its forced
            // bypasses would render every LATER scene's as-is capture through a mutated
            // (pedals-off) working copy. The force-append happens to put it last already, but a
            // `base_requested` caller can name base anywhere among the wire jobs — enforce it
            // explicitly with a stable sort rather than inherit append order.
            if base_in_plan {
                scene_jobs.sort_by_key(|sj| sj.scene_slot == session::BASE_SCENE_SLOT);
            }
            // Error on ANY slot mismatch between the built jobs and the wire jobs — a silent
            // default (especially NaN, which `.min(k_cap)` would collapse to the cap and slam
            // the amp) must never reach a solve.
            for sj in scene_jobs.iter_mut() {
                // A1: the anchored base job has NO wire job of its own by construction (that is
                // the whole point of the anchor) — special-cased here so the "no wire target"
                // hard-error below never fires on it. Offset-adjusted exactly like every other
                // job's wire target (the `base_target` above), or the trade would solve against
                // the wrong number.
                if sj.scene_slot == session::BASE_SCENE_SLOT && anchor_only_base {
                    let anchor = base_anchor.expect("anchor_only_base implies base_anchor");
                    sj.target_lufs = anchor.target_lufs as f64 + offset;
                    continue;
                }
                let arg = jobs
                    .iter()
                    .find(|j| j.scene_slot == sj.scene_slot)
                    .ok_or_else(|| {
                        format!("built scene job slot {} has no wire target", sj.scene_slot)
                    })?;
                if !arg.target_lufs.is_finite() {
                    return Err(format!(
                        "scene slot {} has a non-finite target ({})",
                        arg.scene_slot, arg.target_lufs
                    ));
                }
                sj.target_lufs = arg.target_lufs + offset;
            }
            if let Some(j) = jobs
                .iter()
                .find(|j| !scene_jobs.iter().any(|sj| sj.scene_slot == j.scene_slot))
            {
                return Err(format!(
                    "requested scene slot {} produced no scene job",
                    j.scene_slot
                ));
            }
            let cancelled = || SCENE_LEVEL_CANCEL.load(SeqCst);
            // ── THE REORDERED RUN ────────────────────────────────────────────────────
            // PHASE 1 — measure EVERY sound's ceiling before anything is written. Same
            // captures the solves used to take themselves, simply paid up front, which is
            // the only ordering in which the headroom trade can be decided (it needs every
            // ceiling next to every target, and its verdict changes what every later sound
            // is solved to).
            {
                // The prepass tick names its OWN phase — see `leveller::PREPASS_ACTIVE_MSG`.
                // A capture is streaming when it lands, so the wizard reads it as the verb.
                // A1: suppressed for the anchor-only base row — the frontend has no row to
                // route a slot-8 item to when base arrived with no wire job of its own.
                let mut started = |scene| {
                    if anchor_only_base && scene == session::BASE_SCENE_SLOT {
                        return;
                    }
                    let _ = on_result.send(scene_progress_item(
                        slot,
                        save_run,
                        scene,
                        None,
                        Some(leveller::PREPASS_ACTIVE_MSG),
                        // PHASE 2 has not run yet — no trade to disclose on a prepass tick.
                        None,
                    ));
                };
                // The preset's own SAVED `presetLevel` — NOT the trade's raise, which PHASE 2
                // has not decided yet. The prepass must render at the SAME level every later
                // solve capture does, or the first solve step reads a "response" that is
                // really the level difference between the two renderings: `correct_iter`
                // takes this reading as its `measured0` and compares it against a post-apply
                // capture, so a mismatch of 9.9 dB swamped `no_authority`'s KNOB_TOL_LU and
                // turned an amps-at-zero scene's actionable routing clamp into a reason-less
                // headroom one (offline fixture 403 "Clean"). "The level the preset currently
                // holds" is the intent, and inside a save's lazy-commit window the device's
                // load store does not hold it — hence asserting it rather than assuming it.
                // Through the SAME seam the solve captures use, with no trade yet (`None`),
                // so the two renderings cannot drift apart by editing one side.
                //
                // A2: prefer the run's OWN registered `presetLevel` witness (the preceding
                // `level_preset` base save) over the parsed doc — the barrier above already
                // waited out the commit window, but the device's load store can still lag the
                // registry for a beat longer than the harvest can observe.
                let saved_pl = intended_preset_level_seed
                    .or_else(|| leveller::scene_capture_level(None, saved.as_ref()));
                let prepass_result = leveller::prepass_scene_ceilings(
                    &mut scene_jobs,
                    &stim,
                    saved_pl,
                    &mut started,
                    cancelled,
                );
                // A5/F1 (BLOCKER): the PHASE-1 restore must run on EVERY exit from this block —
                // Ok, Err, AND CANCELLED — from the moment base's isolated capture (LAST in the
                // loop above) may have landed its forced bypasses. Nothing has been written or
                // deferred yet at this point (PHASE 1 is read-only measurement), so the cheapest
                // correct cleanup is a full reload, unconditionally, mirroring the unbatched
                // path's own dirt handling. A cancel must not skip it either (danger.md) — the
                // never-cancel closure below guarantees the barrier/reload run to completion.
                //
                // A FAILED cleanup HARD-FAILS the whole command rather than warn-and-continue:
                // on the anchor-only path base carries no wire job of its own, so nothing later
                // in this run re-solves or re-verifies it, and PHASE 3's isolation-restore list
                // for such a run is empty by construction — a leaked forced bypass here would
                // ride straight into the batch's terminal save with no verify catching it. This
                // is PHASE 1, before anything is written or deferred, so it is the cheapest of
                // the run's three abort points to fail at (mirrors `apply_headroom_trade`'s own
                // back-out and `run_scene_jobs`' pre-save guard — same failure policy, applied at
                // the point where it is still free).
                let base_isolated = base_in_plan
                    && scene_jobs
                        .iter()
                        .find(|sj| sj.scene_slot == session::BASE_SCENE_SLOT)
                        .is_some_and(|sj| !sj.force_bypass.is_empty());
                if base_isolated {
                    let cleanup = leveller::ensure_fresh_load(slot, &mut || false)
                        .and_then(|_| leveller::restore_saved_preset(slot));
                    if let Err(e) = cleanup {
                        return Err(format!(
                            "slot {slot}: could not clear the base isolation prepass write \
                             ({e}) — aborting the run rather than risk a later save persisting \
                             every pedal forced off with an empty verify list"
                        ));
                    }
                }
                prepass_result?;
            }
            // PHASE 2 — plan and (only on a SAVE run, and only if a BENEFITING sound clamps)
            // execute the trade. A no-save run plans it and reports it as ADVISORY.
            let trade = trade_for_batch(
                slot,
                &mut scene_jobs,
                saved.as_ref(),
                &stim,
                save_run,
                cancelled,
            );
            // A1: NOW strip the anchor-only base job — it had to survive PHASE 1 (prepass) and
            // PHASE 2 (the trade, which needed its ceiling/target/fader) with no wire job of its
            // own, but PHASE 3 solves the batch's WIRE jobs and base's own `outputLevel` is
            // never one of this batch's writes when it arrived only as an anchor (the wizard's
            // separate `level_preset` lane owns that write). Leaving it in would re-solve base
            // here too, on top of `level_preset`'s own apply.
            if anchor_only_base {
                scene_jobs.retain(|sj| sj.scene_slot != session::BASE_SCENE_SLOT);
            }
            // A5/F2 (TRADE-PATH GAP): the base job SURVIVES the anchor strip above whenever it
            // arrived `base_requested` (a real wire job, not just an anchor) — see
            // `isolation_restore_for_batch`'s doc for why `run_scene_jobs` needs this
            // independently of whatever the trade's own restore list carries.
            let isolation_restore = isolation_restore_for_batch(&scene_jobs, saved.as_ref());
            // No message: the solve is the wizard's DEFAULT phase, and its bare `active` is
            // what flips the row's verb back from the prepass's `measuring`.
            // F5: `trade.summary` is known now (PHASE 2 already ran) — stamped on every `done`
            // item so the channel carries the same disclosure the return vec gets at :521-531.
            let on_scene = |scene, done: Option<&leveller::BatchedSceneOutcome>| {
                let _ = on_result.send(scene_progress_item(
                    slot,
                    save_run,
                    scene,
                    done,
                    None,
                    trade.summary.as_ref(),
                ));
            };
            // B6: the deferred-save/persist-verify batch-wide captions — see
            // `tail_progress_item`'s doc for why the row key is safe to ignore.
            let on_tail = |tail: &str| {
                let _ = on_result.send(tail_progress_item(tail));
            };
            // PHASE 3 — the writes, on the existing batch runner, unchanged.
            // `rebalance` (opt-in) equalizes a path-MERGE scene's two lanes before joint-k;
            // non-mergeable scenes fall through to the same joint-k either way.
            let mut outcomes = if rebalance {
                leveller::level_scenes_rebalance(
                    slot,
                    &scene_jobs,
                    &stim,
                    save_run,
                    restore_scene,
                    saved.as_ref(),
                    trade.hold.as_ref(),
                    &isolation_restore,
                    on_scene,
                    on_tail,
                    cancelled,
                )
            } else {
                leveller::level_scenes_oneshot(
                    slot,
                    &scene_jobs,
                    &stim,
                    save_run,
                    restore_scene,
                    saved.as_ref(),
                    trade.hold.as_ref(),
                    &isolation_restore,
                    on_scene,
                    on_tail,
                    cancelled,
                )
            }?;
            trade.stamp_failure(&mut outcomes);
            Ok((outcomes, trade.summary))
        };
        // Per-scene leveling drives ONLY the active amp's `outputLevel`. When a scene
        // can't reach target even at the knob's limit it CLAMPS and reports the achieved
        // loudness — we do NOT raise the global `presetLevel` to compensate. Raising it
        // lifts EVERY other scene off-target (presetLevel is the Base's job, settled once
        // before the scene pass), and HW the old boost-and-rerun drove
        // presetLevel to 1.0 and blew preset 001's loud scenes 5–7 LU over target.
        //
        // The headroom trade above is NOT that boost: it raises `presetLevel` only while
        // scaling the base amp's fader DOWN by the same dB, so base stays ON target and
        // the raise is pure headroom for the scenes that pin their own knob. It runs ONCE
        // before the pass (never as a boost-and-rerun), and when the fader hits its floor
        // the still-short sounds get a CLAMPING ERROR instead of a silent overshoot.
        let outcome = run_batched(save);
        let result = match outcome {
            Ok((outcomes, summary)) => {
                // A run CANCELLED after a headroom trade landed comes back Ok, carrying its
                // partial outcomes on purpose (see `run_scene_jobs`' cancel site): the raised
                // base pair is persisted, so the run must disclose rather than hand back an
                // empty vec. Report the cancel here so the wizard still closes its row.
                if SCENE_LEVEL_CANCEL.load(SeqCst) {
                    let _ = on_result.send(SceneLevelProgressItem {
                        scene_slot: session::BASE_SCENE_SLOT,
                        status: "cancelled".to_string(),
                        result: None,
                        message: Some(leveller::CANCELLED.to_string()),
                        tail: None,
                    });
                }
                Ok(outcomes
                    .iter()
                    .filter(|o| o.failure.is_none())
                    // The trade moved the whole preset's gain structure, so EVERY row it
                    // touched carries it (disclosure rationale: `TradeSummary`'s doc).
                    .map(|o| outcome_to_level_result(slot, save, o, summary.as_ref()))
                    .collect())
            }
            Err(e) if e == leveller::CANCELLED => {
                let _ = on_result.send(SceneLevelProgressItem {
                    scene_slot: session::BASE_SCENE_SLOT,
                    status: "cancelled".to_string(),
                    result: None,
                    message: Some(e),
                    tail: None,
                });
                Ok(Vec::new())
            }
            Err(e) => Err(e),
        };
        leveller::reamp_off_guaranteed("level_scenes_apply_batched");
        result
    })
    .await
}

/// The batch's headroom-trade result: what landed (if anything), what to disclose, and — when
/// the trade was attempted and FAILED — which rows deserve the trade's own clamp kind rather
/// than a generic headroom clamp.
pub(crate) struct BatchTrade {
    /// The UNSAVED base pair to hand the batch runner: its `presetLevel` must be re-asserted at
    /// the one save, and its fader writes belong in the run's post-save re-read. `None` on
    /// every run that did not MOVE the pair (an advisory one included).
    pub(crate) hold: Option<leveller::TradeHold>,
    /// What to tell the user — a landed trade OR a no-save run's advisory plan. `None` when
    /// there was nothing to trade at all.
    pub(crate) summary: Option<crate::headroom_trade::TradeSummary>,
    /// The trade's failure cause, when one was attempted and backed out.
    failure: Option<crate::headroom_trade::ClampKind>,
    /// The scene slots the trade would have RESCUED — the only rows whose clamp the trade's
    /// failure explains. A row that was never going to benefit keeps its honest
    /// `scene_ceiling` clamp, because the trade is not why it can't reach target.
    benefiting: Vec<u32>,
}

impl BatchTrade {
    fn none() -> Self {
        Self {
            hold: None,
            summary: None,
            failure: None,
            benefiting: Vec::new(),
        }
    }

    /// Re-stamp the clamp taxonomy on the rows a FAILED trade was supposed to rescue, so the
    /// UI says "the trade ran out of base fader" / "the trade was backed out" instead of the
    /// generic "this sound can’t reach the target". Only CLAMPED benefiting rows are touched;
    /// a row that reached its target is untouched whatever the trade did.
    ///
    /// An ADVISORY (no-save) run leaves every kind alone: those rows clamp for their own
    /// honest reason — the trade never ran, so it is not why they missed.
    fn stamp_failure(&self, outcomes: &mut [leveller::BatchedSceneOutcome]) {
        let Some(kind) = self.failure else { return };
        for o in outcomes.iter_mut() {
            if o.clamped && o.failure.is_none() && self.benefiting.contains(&o.scene_slot) {
                o.clamp_kind = Some(kind);
            }
        }
    }
}

/// PHASE 2's decision, taken from the prepass ceilings BEFORE any device work touches the base
/// pair. Made by [`plan_trade_for_batch`], which is PURE — the whole benefit / room / handle /
/// save rule set is unit-testable with no device in the loop.
#[derive(Debug, PartialEq)]
pub(crate) enum TradeDecision {
    /// Nothing to trade — see [`plan_trade_for_batch`] for the five ways this happens.
    None,
    /// A SAVE run with a benefiting clamp: execute the plan.
    Apply(TradeIntent),
    /// A NO-SAVE run with a benefiting clamp: report what WOULD be traded, write nothing.
    Advisory(TradeIntent),
}

/// Everything the executor needs from the pure plan.
#[derive(Debug, PartialEq)]
pub(crate) struct TradeIntent {
    pub(crate) plan: crate::headroom_trade::TradePlan,
    /// Index of the BASE row in the job list.
    pub(crate) base_idx: usize,
    pub(crate) preset_level: f32,
    /// The QUIETEST audible base amp `outputLevel` (see `min_audible_above`'s doc).
    pub(crate) base_fader: f32,
    /// The wire scene slots a base raise actually helps.
    pub(crate) benefiting: Vec<u32>,
    /// A6: the STRICT SUBSET of `benefiting` whose ALREADY-TAKEN prepass reading can be
    /// shifted `+raise_db` and reused rather than dropped for a re-measure — see
    /// `headroom_trade::retains_prepass_after_raise`'s doc. Every `Full`-overlay beneficiary
    /// qualifies; an `Absent` one benefits from the eventual raise but its prepass rendered
    /// through base's PRE-raise fader and must be dropped instead.
    pub(crate) retains_prepass: Vec<u32>,
}

/// PHASE 2, PURE HALF: decide the benefit-aware headroom trade from the prepass ceilings.
///
/// BENEFIT comes from the OVERLAY DEPENDENCY STRUCTURE, never from a guess — see
/// `benefits_from_base_raise`'s doc (`headroom_trade.rs`) for the table.
///
/// FIVE WAYS THIS ANSWERS `None`:
/// 1. no saved document (nothing to read overlays or `presetLevel` from);
/// 2. no BASE row in the batch. The trade's whole shape is "raise `presetLevel`, hold BASE at
///    its target with the base fader" — without base's own job there is no target to hold it
///    to and no fader to hold it with;
/// 3. the base row is HANDLE-DRIVEN. The user picked base's own leveling control, and the hold
///    would solve THAT control down instead of a fader — a wet `mix` carries a preservation
///    floor precisely so a run never guts an effect to make a number (D5), and the only knob
///    the trade may lower is the base amp fader (`outputLevel` is pure digital gain; a
///    wet/tone param is never a trade lever, D6). So: no trade. The base row still levels on
///    its handle exactly as asked and every clamp is reported honestly. Checked BEFORE the
///    fader fold, because a handle row's `current` is the handle param's value — folding it
///    would fabricate fader room out of a mix control;
/// 4. no `presetLevel` in the saved document;
/// 5. the plan itself asks for no raise (nothing clamps, or nothing that clamps benefits).
pub(crate) fn plan_trade_for_batch(
    scene_jobs: &[leveller::SceneJob],
    saved: Option<&serde_json::Value>,
    save: bool,
) -> TradeDecision {
    use crate::headroom_trade::{
        benefits_from_base_raise, min_audible_above, plan_headroom_trade,
        retains_prepass_after_raise, SoundId, TradeSound, BASE_FADER_FLOOR,
    };
    let Some(saved_doc) = saved else {
        return TradeDecision::None;
    };
    let Some(base_idx) = scene_jobs
        .iter()
        .position(|j| j.scene_slot == session::BASE_SCENE_SLOT && j.skip.is_none())
    else {
        return TradeDecision::None;
    };
    if scene_jobs[base_idx].handle.is_some() {
        return TradeDecision::None;
    }
    let Some(preset_level) = audiograph::preset_level(saved_doc) else {
        return TradeDecision::None;
    };
    let preset_level = preset_level as f32;
    // The base fader the trade has to pay with: the QUIETEST AUDIBLE base amp's `outputLevel`
    // (see `min_audible_above`'s doc). A base row with no audible lane at all reports zero
    // room, which the planner reads as no trade.
    let base_fader = min_audible_above(
        scene_jobs[base_idx].knobs.iter().map(|kt| kt.current),
        BASE_FADER_FLOOR,
    )
    .unwrap_or(0.0);

    let mut benefiting: Vec<u32> = Vec::new();
    let mut retains_prepass: Vec<u32> = Vec::new();
    let mut sounds: Vec<TradeSound> = Vec::new();
    for (i, job) in scene_jobs.iter().enumerate() {
        let Some(ceiling) = leveller::scene_ceiling_lufs(job) else {
            continue;
        };
        if i == base_idx {
            sounds.push(TradeSound {
                id: SoundId::Base,
                ceiling_lufs: ceiling,
                target_lufs: job.target_lufs,
                // Base is HELD at its target by the trade; it never benefits from it.
                benefits: false,
            });
            continue;
        }
        // Every knob of the scene must be overlay-pinned for the scene to keep the whole
        // rise: a lane still reading base's fader takes the drop with it.
        let benefits = !job.knobs.is_empty()
            && job.knobs.iter().all(|kt| match &kt.knob {
                leveller::LevelKnob::Block { node_id, .. } => {
                    benefits_from_base_raise(&scene_overlay(saved_doc, job.scene_slot, node_id))
                }
                leveller::LevelKnob::PresetLevel => false,
            });
        if benefits {
            benefiting.push(job.scene_slot);
            // A6: the NARROWER Full-only predicate — see `retains_prepass_after_raise`'s doc.
            let retains = job.knobs.iter().all(|kt| match &kt.knob {
                leveller::LevelKnob::Block { node_id, .. } => {
                    retains_prepass_after_raise(&scene_overlay(saved_doc, job.scene_slot, node_id))
                }
                leveller::LevelKnob::PresetLevel => false,
            });
            if retains {
                retains_prepass.push(job.scene_slot);
            }
        }
        sounds.push(TradeSound {
            id: SoundId::Scene {
                scene_slot: job.scene_slot,
            },
            ceiling_lufs: ceiling,
            target_lufs: job.target_lufs,
            benefits,
        });
    }
    let plan = plan_headroom_trade(&sounds, preset_level, base_fader);
    if !plan.is_trade() {
        return TradeDecision::None;
    }
    let intent = TradeIntent {
        plan,
        base_idx,
        preset_level,
        base_fader,
        benefiting,
        retains_prepass,
    };
    if save {
        TradeDecision::Apply(intent)
    } else {
        TradeDecision::Advisory(intent)
    }
}

/// The per-lane `outputLevel` moves of a trade. `solved` aligns with `base_job.knobs` (the
/// hold's own output); `None` = an advisory, which solved nothing (module header: the fader
/// response is not algebraically predictable), so a run that never solved it must not state a
/// value.
fn base_amp_moves(
    base_job: &leveller::SceneJob,
    solved: Option<&[f32]>,
) -> Vec<crate::headroom_trade::TradeAmpMove> {
    base_job
        .knobs
        .iter()
        .enumerate()
        .filter_map(|(i, kt)| match &kt.knob {
            leveller::LevelKnob::Block {
                group_id,
                node_id,
                parameter_id,
                ..
            } => Some(crate::headroom_trade::TradeAmpMove {
                group_id: group_id.clone(),
                node_id: node_id.clone(),
                parameter_id: parameter_id.clone(),
                previous_value: kt.current,
                value: solved.and_then(|s| s.get(i)).copied(),
            }),
            leveller::LevelKnob::PresetLevel => None,
        })
        .collect()
}

/// The advisory summary for a no-save run: what the trade WOULD do.
fn advisory_summary(
    intent: &TradeIntent,
    base_job: &leveller::SceneJob,
) -> crate::headroom_trade::TradeSummary {
    crate::headroom_trade::TradeSummary {
        applied: false,
        raise_db: intent.plan.raise_db,
        previous_preset_level: intent.preset_level,
        preset_level: crate::headroom_trade::raised_preset_level(
            intent.preset_level,
            intent.plan.raise_db,
        ),
        base_amps: base_amp_moves(base_job, None),
        cap: intent.plan.capped,
        benefiting: intent
            .benefiting
            .iter()
            .map(|&scene_slot| crate::headroom_trade::SoundId::Scene { scene_slot })
            .collect(),
    }
}

/// The trade's own writes, as the batch runner's post-save re-read wants them: the base pair
/// lives in the BASE graph, so the scene slot is the base sentinel (`persisted_value` reads
/// those straight off `dspUnitParameters`).
fn trade_hold_writes(
    base_job: &leveller::SceneJob,
    levels: &[f32],
) -> Vec<leveller::PersistedWrite> {
    base_job
        .knobs
        .iter()
        .zip(levels)
        .filter_map(|(kt, &v)| match &kt.knob {
            leveller::LevelKnob::Block {
                node_id,
                parameter_id,
                ..
            } => Some(leveller::PersistedWrite {
                scene_slot: session::BASE_SCENE_SLOT,
                node_id: node_id.clone(),
                parameter_id: parameter_id.clone(),
                value: v,
            }),
            leveller::LevelKnob::PresetLevel => None,
        })
        .collect()
}

/// Adopt the trade's solved base faders as the jobs' new knob ANCHORS. `KnobTarget::current` is
/// what every later solve treats as "what the device holds", so leaving pre-trade values in
/// place makes base's own already-at-target path report the OLD fader as final and makes an
/// inheriting scene's first write overshoot by the whole fader drop.
///
/// WHO INHERITS: see `benefits_from_base_raise`'s doc for the overlay table (`Absent` /
/// `BypassOnly` inherit base's value, `Full` pins its own). An `Unknown` overlay is left alone:
/// its solve re-measures and the verify+correct loop absorbs a stale first write, so the
/// conservative side costs nothing.
fn adopt_trade_levels(
    scene_jobs: &mut [leveller::SceneJob],
    base_idx: usize,
    saved: &serde_json::Value,
    base_levels: &[f32],
) {
    let by_node: Vec<(String, f32)> = scene_jobs[base_idx]
        .knobs
        .iter()
        .zip(base_levels)
        .filter_map(|(kt, &v)| match &kt.knob {
            leveller::LevelKnob::Block { node_id, .. } => Some((node_id.clone(), v)),
            leveller::LevelKnob::PresetLevel => None,
        })
        .collect();
    for (i, job) in scene_jobs.iter_mut().enumerate() {
        for kt in job.knobs.iter_mut() {
            let leveller::LevelKnob::Block { node_id, .. } = &kt.knob else {
                continue;
            };
            let Some((_, v)) = by_node.iter().find(|(n, _)| n == node_id) else {
                continue;
            };
            let inherits = i == base_idx
                || matches!(
                    scene_overlay(saved, job.scene_slot, node_id),
                    SceneOverlay::Absent | SceneOverlay::BypassOnly(_)
                );
            if inherits {
                kt.current = *v;
            }
        }
    }
}

/// PHASE 2 of the reordered run: take [`plan_trade_for_batch`]'s decision and, on a SAVE run
/// with a benefiting clamp, execute it.
///
/// WHY A NO-SAVE RUN NEVER EXECUTES. The hold is written with `defer = true`, but phase 3 runs
/// with `defer = save` — and with `save = false` every scene apply ends in
/// `restore_saved_preset`, a same-slot load that reloads the SAVED preset and destroys the
/// unsaved raise + hold. `retarget_prepass_after_trade` would meanwhile have shifted the
/// benefiting scenes' readings by `+raise_db`, so they would describe a device state that no
/// longer exists and every one of those rows would report a number the preview cannot produce.
/// So a preview PLANS the trade and says so, and the rows it would have rescued keep their
/// honest clamp: without the trade, they really do clamp.
fn trade_for_batch(
    slot: u32,
    scene_jobs: &mut [leveller::SceneJob],
    saved: Option<&serde_json::Value>,
    stim: &[f32],
    save: bool,
    cancelled: impl Fn() -> bool + Copy,
) -> BatchTrade {
    use crate::headroom_trade::{ClampKind, TradeSummary};
    let intent = match plan_trade_for_batch(scene_jobs, saved, save) {
        TradeDecision::None => return BatchTrade::none(),
        TradeDecision::Advisory(intent) => {
            log::info!(
                "headroom trade slot={slot}: ADVISORY only (no-save run) — would raise \
                 presetLevel by {:.2} dB for scenes {:?}",
                intent.plan.raise_db,
                intent.benefiting,
            );
            let summary = advisory_summary(&intent, &scene_jobs[intent.base_idx]);
            return BatchTrade {
                hold: None,
                summary: Some(summary),
                // NOT a failure: nothing was attempted, so there is nothing to re-stamp. Those
                // rows clamp for their own honest reason.
                failure: None,
                benefiting: intent.benefiting,
            };
        }
        TradeDecision::Apply(intent) => intent,
    };
    // `saved` is `Some` by construction (the planner answers `None` without it).
    let Some(saved_doc) = saved else {
        return BatchTrade::none();
    };
    log::info!(
        "headroom trade slot={slot}: raising presetLevel by {:.2} dB (from {:.4}, quietest base \
         fader {:.4}, capped: {:?}) for scenes {:?}",
        intent.plan.raise_db,
        intent.preset_level,
        intent.base_fader,
        intent.plan.capped,
        intent.benefiting,
    );
    let base_job = scene_jobs[intent.base_idx].clone();
    let attempt = |plan: &crate::headroom_trade::TradePlan| {
        leveller::apply_headroom_trade(
            slot,
            plan,
            intent.preset_level,
            &base_job,
            stim,
            saved,
            cancelled,
        )
    };
    let mut plan = intent.plan.clone();
    let mut outcome = attempt(&plan);
    // THE ONE BOUNDED RE-PLAN (see `replan_after_floor_pin`'s doc for the arithmetic and the
    // worth-retrying rule). If the smaller raise still pins, the pair is backed out and the
    // rows it would have rescued are stamped `TradeFloor`.
    if let Err(f) = &outcome {
        if f.kind == ClampKind::TradeFloor && !cancelled() {
            let retry = f
                .base_overshoot_lu
                .and_then(|o| crate::headroom_trade::replan_after_floor_pin(plan.raise_db, o));
            if let Some(retry) = retry {
                log::info!(
                    "headroom trade slot={slot}: hold pinned at the fader floor ({:.2} LU over \
                     target) — ONE re-plan at {:.2} dB",
                    f.base_overshoot_lu.unwrap_or_default(),
                    retry.raise_db
                );
                plan = retry;
                outcome = attempt(&plan);
            }
        }
    }
    match outcome {
        Ok(applied) => {
            // A6: only the NARROWER `retains_prepass` set shifts its already-taken reading —
            // an `Absent` beneficiary's prepass is dropped instead (`None` → its own PHASE-3
            // solve re-measures, AFTER its overlay exists). See `retarget_prepass_after_trade`'s
            // doc for why the split is the physics, not a refactor choice.
            let rset = intent.retains_prepass.clone();
            leveller::retarget_prepass_after_trade(scene_jobs, applied.raise_db, move |sc| {
                rset.contains(&sc)
            });
            adopt_trade_levels(scene_jobs, intent.base_idx, saved_doc, &applied.base_levels);
            let summary = TradeSummary {
                applied: true,
                raise_db: applied.raise_db,
                previous_preset_level: applied.previous_preset_level,
                preset_level: applied.preset_level,
                base_amps: base_amp_moves(&base_job, Some(&applied.base_levels)),
                cap: plan.capped,
                benefiting: intent
                    .benefiting
                    .iter()
                    .map(|&scene_slot| crate::headroom_trade::SoundId::Scene { scene_slot })
                    .collect(),
            };
            // A5/F2 DETECTION: what the base isolation should read back as, per forced node —
            // the same shared derivation `apply_headroom_trade`'s own `undo_base_isolation`
            // uses (`leveller::isolation_restore_list`'s doc).
            let force_bypass_restore =
                leveller::isolation_restore_list(&base_job.force_bypass, Some(saved_doc));
            BatchTrade {
                hold: Some(leveller::TradeHold {
                    preset_level: applied.preset_level,
                    writes: trade_hold_writes(&base_job, &applied.base_levels),
                    force_bypass_restore,
                }),
                summary: Some(summary),
                failure: None,
                benefiting: intent.benefiting,
            }
        }
        Err(f) => {
            // BACKED OUT — the base pair is whole again and NOTHING was persisted. Every
            // prepass reading still describes the pre-raise device, so they all stand.
            log::warn!(
                "headroom trade slot={slot} did not land ({:?}): {}",
                f.kind,
                f.why
            );
            BatchTrade {
                hold: None,
                summary: None,
                failure: Some(f.kind),
                benefiting: intent.benefiting,
            }
        }
    }
}

/// Build the streamed progress row for one scene step — `None` = the step just STARTED
/// (spinner), `Some(outcome)` = it finished (a `done` result or an `error` message).
///
/// `active_message` is the started row's caption and is consumed ONLY by the `None` arm. The
/// ceiling prepass passes [`leveller::PREPASS_ACTIVE_MSG`] so the wizard can tell that phase
/// apart from the solve, which passes `None`; without it the two ticks are byte-identical and
/// the run reads as though it were already solving.
///
/// F5: `trade` is the batch's own [`crate::headroom_trade::TradeSummary`], known (PHASE 2 runs
/// before PHASE 3 ever emits a `done` item) and stamped on every `done` row alongside the
/// same stamping `level_scenes_apply_batched` passes through `outcome_to_level_result`'s own
/// `trade` parameter for its awaited return — the wizard consumes the CHANNEL's items, not the
/// awaited return, so leaving this `None` would land a permanent un-revertable `presetLevel`
/// raise with zero UI disclosure.
fn scene_progress_item(
    slot: u32,
    save: bool,
    scene: u32,
    done: Option<&leveller::BatchedSceneOutcome>,
    active_message: Option<&str>,
    trade: Option<&crate::headroom_trade::TradeSummary>,
) -> SceneLevelProgressItem {
    match done {
        None => SceneLevelProgressItem {
            scene_slot: scene,
            status: "active".to_string(),
            result: None,
            message: active_message.map(str::to_string),
            tail: None,
        },
        Some(o) => match &o.failure {
            None => SceneLevelProgressItem {
                scene_slot: scene,
                status: "done".to_string(),
                result: Some(outcome_to_level_result(slot, save, o, trade)),
                message: None,
                tail: None,
            },
            Some(e) => SceneLevelProgressItem {
                scene_slot: scene,
                status: "error".to_string(),
                result: None,
                message: Some(e.clone()),
                tail: None,
            },
        },
    }
}

/// B6 (issue 6b): a batch-wide caption for a phase with no single scene to attach to — the
/// deferred-save start ("Saving preset…") and the post-save persist-verify start
/// ("Verifying…"). Rides its OWN [`SceneLevelProgressItem`].
///
/// `scene_slot: u32::MAX` — a DEDICATED sentinel, deliberately NOT `session::BASE_SCENE_SLOT`
/// (8): the frontend's `byScene` map (`useLevelingFlow.ts`) is built only from the group's own
/// scene rows, so `BASE_SCENE_SLOT` would in fact miss there. `u32::MAX` is outside every real
/// scene slot (0..7), `BASE_SCENE_SLOT` included, so `batchResolve`'s entry lookup always misses
/// on it and silently drops the synthetic row — the same "unknown key is ignored" path that
/// already protects a `cancelled`-status item today.
///
/// `status`/`result`/`message` are dummy values here: `batchResolve` reads `tail` BEFORE the
/// entry lookup that would ever look at them (see the frontend's own doc on that ordering), so
/// they never surface.
fn tail_progress_item(tail: &str) -> SceneLevelProgressItem {
    SceneLevelProgressItem {
        scene_slot: u32::MAX,
        status: "active".to_string(),
        result: None,
        message: None,
        tail: Some(tail.to_string()),
    }
}

/// Map a [`leveller::BatchedSceneOutcome`] onto the frontend's `LevelResult`
/// contract (the batched runner's outcome is per-scene; `verify_lufs` carries
/// the final measured window).
fn outcome_to_level_result(
    slot: u32,
    save: bool,
    o: &leveller::BatchedSceneOutcome,
    trade: Option<&crate::headroom_trade::TradeSummary>,
) -> leveller::LevelResult {
    let lufs = o.final_lufs.unwrap_or(f64::NAN);
    leveller::LevelResult {
        slot,
        // IDENTITY, straight off the outcome — never the row's position. The caller
        // FILTERS failed outcomes out of the vec it returns, so a positional read
        // mislabels every row after a mid-batch failure (see `LevelResult::scene_slot`).
        //
        // BASE MAPS TO `None`, at the wire. A batch carries the whole preset's sounds, base
        // included, and base rides the runner as the `BASE_SCENE_SLOT` (8) SENTINEL — which
        // is not a `scenes[]` index at all. Forwarding it verbatim contradicted the field's
        // own contract ("None on every base row"), the TS mirror, and `validate_log`'s
        // base/scene labelling, and would have let a consumer look up `scenes[8]`.
        scene_slot: (o.scene_slot != session::BASE_SCENE_SLOT).then_some(o.scene_slot),
        ref_level: o.final_level.unwrap_or(0.0),
        measured_lufs: lufs,
        constant_c: f64::NAN,
        final_level: o.final_level.unwrap_or(0.0),
        // Per-scene target lives on the outcome (a batch can mix targets).
        target_lufs: o.target_lufs,
        predicted_lufs: lufs,
        clamped: o.clamped,
        saved: save,
        verify_lufs: o.final_lufs,
        iterations: o.windows.max(o.writes),
        dynamic_spread_lu: o.dynamic_spread_lu,
        // Forwarded verbatim from the runner's outcome — the taxonomy is decided once, in
        // the leveller, so a scene row and a footswitch row can never name the same cause
        // differently.
        clamp_kind: o.clamp_kind,
        clamp_reason: o.clamp_reason.clone(),
        verify_by_ear: o.verify_by_ear,
        // Scene rows write amp outputLevel, not presetLevel — nothing to revert here.
        previous_level: None,
        // Scene path: no predicted true peak this cycle (only the one-shot presetLevel
        // path in `level_preset` estimates it).
        true_peak_dbtp: None,
        persist_mismatch: o.persist_mismatch,
        // F5: the batch's own trade summary, known before PHASE 3 ever emits this — see
        // `scene_progress_item`'s doc for why the channel items need it too, not just the
        // awaited return vec.
        trade: trade.cloned(),
    }
}

// ───────────────────── Scene handle picker (enumeration) ─────────────────────
// The wire DTOs (`SceneHandleCandidate`/`SceneHandleRow`) and the whole pure derivation this
// command wraps (`scene_handle_rows`, `base_handle_candidates_scanned`, `scan_node_graph`,
// `finish_handle_candidate`) live in `probe_api::scene_jobs` — their entire dependency mass
// (`scene_overlay`, `scene_write_verdict_for_param`, `SceneOverlay`) already lived there, and
// `backup_read` (which calls the shared-walk `*_scanned` cores straight off the backup scan,
// with no command layer involved) has no business importing an algorithm from the IPC
// command layer to reach them. All are re-exported at the crate root, so this command — and
// every other caller, `backup_read` included — keeps addressing them unqualified
// (`scene_handle_rows`, `SceneHandleRow`, …) exactly as before.

/// Per-scene handle candidates for the picker. PURE apart from ONE field-8 read: every
/// annotation (class, range, per-scene current value, overlay scope) comes out of the saved
/// document, so no scene is recalled on the unit and nothing is measured.
#[tauri::command]
pub(crate) async fn list_scene_level_handles(
    state: State<'_, AppState>,
    slot: u32,
) -> Result<Vec<SceneHandleRow>, String> {
    with_released_seize(state.session.clone(), move || {
        // The row set is `scenes.len()` — the tail section a large preset's field-8 read
        // cuts first — so a truncated body would silently offer the player a handle
        // picker missing its last scenes.
        let (preset, _, _) = read_slot_preset_complete(slot, &["scenes"])?;
        Ok(scene_handle_rows(&preset))
    })
    .await
}

/// Headroom (LU) below the quietest-capable preset's ceiling when auto-picking
/// the setlist common target. Small margin so the floor preset isn't clamped.
const SETLIST_HEADROOM_LU: f64 = 1.0;

/// One preset in a setlist leveling job: its slot + the instrument profile's
/// topology (resolved to that instrument's stimulus).
#[derive(serde::Deserialize)]
pub(crate) struct SetlistJobEntry {
    slot: u32,
    topology_id: Option<String>,
    calibration_lufs: Option<f32>,
}

/// Level a whole setlist to one common loudness target so switching presets (and
/// instruments) on stage causes no jump. Measures every preset's ceiling, picks a
/// target just below the quietest, and applies it to all. Like `level_preset`, it
/// releases the app's seize, runs, then re-establishes the UI session.
#[tauri::command]
pub(crate) async fn level_setlist(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    entries: Vec<SetlistJobEntry>,
    save: bool,
) -> Result<leveller::SetlistResult, String> {
    if entries.is_empty() {
        return Err("no presets selected to level".to_string());
    }
    // Resolve each entry's stimulus path + playback compensation on the UI
    // thread (needs AppHandle; the store is read ONCE for the whole setlist).
    // The common target stays one loudness; a bass entry's offset rides its own
    // effective target inside the leveller.
    let playback = profiles::load(&app)
        .map(|s| s.playback_level)
        .unwrap_or_default();
    let resolved: Vec<(u32, String, Option<f32>, f64)> = entries
        .into_iter()
        .map(|e| {
            let offset_lu = profiles::playback_offset_lu(
                playback,
                stimulus_instrument(e.topology_id.as_deref()),
            );
            resolve_stimulus(&app, None, e.topology_id)
                .map(|p| (e.slot, p, e.calibration_lufs, offset_lu))
        })
        .collect::<Result<_, _>>()?;
    with_released_seize(state.session.clone(), move || {
        // Own each stimulus (calibrated if the profile has a real-output level),
        // then borrow into entries for the leveller.
        let stims: Vec<(u32, Vec<f32>, f64)> = resolved
            .into_iter()
            .map(|(slot, path, cal, off)| {
                read_stimulus_calibrated(&path, cal).map(|s| (slot, s, off))
            })
            .collect::<Result<_, _>>()?;
        let lvl_entries: Vec<leveller::SetlistEntry> = stims
            .iter()
            .map(|(slot, s, off)| leveller::SetlistEntry {
                slot: *slot,
                stimulus: s,
                offset_lu: *off,
            })
            .collect();
        let result = leveller::level_setlist(&lvl_entries, SETLIST_HEADROOM_LU, 0.5, save);
        leveller::reamp_off_guaranteed("level_setlist");
        result
    })
    .await
}

/// Measure each scene's ceiling loudness (re-amp + `loadScene` per scene)
/// and return the per-scene gain offsets to a common target (MEASURE — drives the
/// device; HW-pending). Supersedes hand-entered C values when hardware is present.
#[tauri::command]
pub(crate) async fn level_scenes(
    app: tauri::AppHandle,
    slot: u32,
    scene_count: u32,
    topology_id: Option<String>,
    headroom_lu: f64,
    state: State<'_, AppState>,
) -> Result<Vec<f64>, String> {
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        let cs = leveller::capture_scene_ceilings(slot, scene_count, &stim)?;
        scenes::normalize_scene_targets(&cs, headroom_lu)
            .ok_or_else(|| "no finite scene loudness measured".to_string())
    })
    .await
}

/// PHASE 2's pure decision seam, exercised with NO device in the loop. Items ⟦1⟧ (a
/// handle-carrying base row disables the trade), ⟦2⟧ (a no-save run plans but never applies)
/// and ⟦4b⟧ (the QUIETEST lane binds a joint scale-down) all land here.
#[cfg(test)]
mod trade_planner_tests {
    use super::*;
    use crate::headroom_trade::BASE_FADER_FLOOR;

    /// A preset whose amp carries a scene-0 knob overlay (`Full` — so scene 0 BENEFITS from a
    /// base raise) and a pedal for the handle rows to point at.
    fn preset(preset_level: f64) -> serde_json::Value {
        serde_json::json!({
            "audioGraph": {
                "presetLevel": preset_level,
                "guitarNodes": { "G1": [
                    { "nodeId": "amp", "FenderId": "ACD_TwinReverb65NoFx",
                      "dspUnitParameters": { "outputLevel": 0.8, "bypass": false } },
                    { "nodeId": "amp2", "FenderId": "ACD_TwinReverb65NoFx",
                      "dspUnitParameters": { "outputLevel": 0.02, "bypass": false } },
                    { "nodeId": "ped", "FenderId": "ACD_ChorusCE",
                      "dspUnitParameters": { "mix": 0.6, "bypass": false } }
                ] }
            },
            "scenes": [
                { "guitarNodes": { "G1": {
                    "ACD_TwinReverb65NoFx": { "dspUnitParameters": { "outputLevel": 0.5 } } } } },
                // Scene 1 mentions no amp at all (`Absent`): it READS base's fader, so a base
                // fader drop moves it too — exactly net-zero, and it never benefits.
                { "guitarNodes": { "G1": {
                    "ACD_ChorusCE": { "dspUnitParameters": { "mix": 0.4 } } } } }
            ]
        })
    }

    fn knob(node: &str, current: f32, scene_slot: Option<u32>) -> leveller::KnobTarget {
        leveller::KnobTarget {
            knob: leveller::LevelKnob::Block {
                group_id: "G1".into(),
                node_id: node.into(),
                parameter_id: "outputLevel".into(),
                scene_slot,
            },
            lo: 0.0,
            hi: 1.0,
            current,
        }
    }

    /// One prepass-measured job. `asis` is the ceiling seed: `scene_ceiling_lufs` extrapolates
    /// it to the top of the amp's range, so a job with a 1.0 knob has `ceiling == asis`.
    fn job(
        scene_slot: u32,
        target: f64,
        asis: f64,
        knobs: Vec<leveller::KnobTarget>,
    ) -> leveller::SceneJob {
        leveller::SceneJob {
            scene_slot,
            target_lufs: target,
            knobs,
            skip: None,
            rebalanceable: false,
            handle: None,
            force_bypass: Vec::new(),
            prepass: Some(leveller::ScenePrepass { asis, spread: 1.0 }),
        }
    }

    /// Base (on its amp fader, at target) + a benefiting scene 0 that is 4 LU short.
    fn base_and_clamped_scene(base_knobs: Vec<leveller::KnobTarget>) -> Vec<leveller::SceneJob> {
        vec![
            job(session::BASE_SCENE_SLOT, -23.0, -23.0, base_knobs),
            job(0, -15.0, -19.0, vec![knob("amp", 1.0, Some(0))]),
        ]
    }

    fn intent(d: &TradeDecision) -> &TradeIntent {
        match d {
            TradeDecision::Apply(i) | TradeDecision::Advisory(i) => i,
            TradeDecision::None => panic!("expected a planned trade, got None"),
        }
    }

    // The control case: a plain fader base row on a SAVE run plans and EXECUTES the trade.
    #[test]
    fn a_fader_base_row_on_a_save_run_arms_the_trade() {
        let p = preset(0.5);
        let jobs = base_and_clamped_scene(vec![knob("amp", 0.8, None)]);
        let d = plan_trade_for_batch(&jobs, Some(&p), true);
        assert!(matches!(d, TradeDecision::Apply(_)), "{d:?}");
        let i = intent(&d);
        assert!(
            (i.plan.raise_db - 4.0).abs() < 1e-9,
            "{:?}",
            i.plan.raise_db
        );
        assert_eq!(
            i.benefiting,
            vec![0],
            "scene 0's overlay pins its own fader"
        );
        assert_eq!(
            i.retains_prepass,
            vec![0],
            "a Full overlay's ALREADY-TAKEN prepass reading is exactly the one to shift"
        );
    }

    // A9/THE REGRESSION TEST (bug: preset 28 "Friedman HBE"). Before A6 `benefits_from_base_raise`
    // required a PRE-EXISTING `Full` overlay — wrong, because PHASE 3's own Scene Edit write
    // materializes the overlay the moment an Absent scene's own row is solved, so by the time
    // the raise matters that scene is exactly as independent of base as a Full one always was.
    // Shaped like the wizard's own anchor-delivered run: base is present with `skip: None` and
    // NO wire job of its own (`level_scenes.rs`'s anchor keeps it alive through PHASE 1/2 with
    // exactly this shape), and the SOLE beneficiary is Absent, not Full.
    #[test]
    fn plan_trade_for_batch_fires_for_an_absent_only_beneficiary_when_base_arrives_via_anchor() {
        let p = preset(0.5);
        let jobs = vec![
            job(
                session::BASE_SCENE_SLOT,
                -23.0,
                -23.0,
                vec![knob("amp", 0.8, None)],
            ),
            // Scene 1 mentions no amp overlay at all (Absent) — 4 LU short.
            job(1, -15.0, -19.0, vec![knob("amp", 1.0, Some(1))]),
        ];
        let d = plan_trade_for_batch(&jobs, Some(&p), true);
        assert!(
            matches!(d, TradeDecision::Apply(_)),
            "an Absent-only beneficiary must now arm the trade: {d:?}"
        );
        let i = intent(&d);
        assert_eq!(
            i.benefiting,
            vec![1],
            "Absent now benefits — its Scene Edit write will materialize the overlay"
        );
        assert!(
            i.retains_prepass.is_empty(),
            "but its prepass reading rendered through base's PRE-raise fader and must be \
             DROPPED, not shifted — retains_prepass is the narrower, Full-only set"
        );
    }

    // ⟦1⟧ THE BLOCKING ONE. When the user picked base's OWN control, the hold would solve THAT
    // control down to pay for the raise — a wet mix has a preservation floor precisely so a run
    // never guts an effect to make a number (D5), and the only knob the trade may lower is the
    // base amp fader (D6). So a handle-carrying base row disables the trade outright; the row
    // still levels on its handle and every clamp is reported honestly.
    #[test]
    fn a_handle_carrying_base_row_disables_the_trade() {
        let p = preset(0.5);
        let mut jobs = base_and_clamped_scene(vec![leveller::KnobTarget {
            knob: leveller::LevelKnob::Block {
                group_id: "G1".into(),
                node_id: "ped".into(),
                parameter_id: "mix".into(),
                scene_slot: None,
            },
            lo: 0.0,
            hi: 1.0,
            // A wet mix authored at 0.6 — folding THIS as "fader room" is exactly the bug.
            current: 0.6,
        }]);
        jobs[0].handle = Some(leveller::FsParamTarget::from_preset(&p, "ped", "mix"));
        assert_eq!(
            plan_trade_for_batch(&jobs, Some(&p), true),
            TradeDecision::None,
            "a user handle on base is never a trade lever"
        );
        // And it is the HANDLE that disabled it, not the batch shape: the same batch with a
        // fader base row does trade.
        assert!(matches!(
            plan_trade_for_batch(
                &base_and_clamped_scene(vec![knob("amp", 0.8, None)]),
                Some(&p),
                true
            ),
            TradeDecision::Apply(_)
        ));
    }

    // ⟦2⟧ A NO-SAVE run must plan the trade and write NOTHING: phase 3 runs `defer = save`, so
    // with `save = false` every scene apply reloads the stored preset and destroys an unsaved
    // raise + hold — while the benefiting rows' prepass readings would already have been
    // shifted `+raise_db` to describe a device state that no longer exists.
    #[test]
    fn a_no_save_run_plans_the_trade_as_advisory_and_never_applies_it() {
        let p = preset(0.5);
        let jobs = base_and_clamped_scene(vec![knob("amp", 0.8, None)]);
        let d = plan_trade_for_batch(&jobs, Some(&p), false);
        assert!(matches!(d, TradeDecision::Advisory(_)), "{d:?}");
        let i = intent(&d);
        assert!((i.plan.raise_db - 4.0).abs() < 1e-9);

        // The advisory the run reports: the raise and the WOULD-BE presetLevel are exact,
        // the fader values deliberately are not (module header).
        let s = advisory_summary(i, &jobs[i.base_idx]);
        assert!(!s.applied);
        assert!((s.previous_preset_level - 0.5).abs() < 1e-6);
        assert!((s.preset_level - 0.7924).abs() < 1e-3, "{}", s.preset_level);
        assert_eq!(
            s.benefiting,
            vec![crate::headroom_trade::SoundId::Scene { scene_slot: 0 }]
        );
        match &s.base_amps[..] {
            [a] => {
                assert!((a.previous_value - 0.8).abs() < 1e-6);
                assert_eq!(a.value, None, "an advisory solved no fader");
            }
            n => panic!("expected one base amp, got {}", n.len()),
        }
    }

    // ⟦4b⟧ see `min_audible_above`'s doc for why the QUIETEST lane binds the room.
    #[test]
    fn the_quietest_base_lane_binds_the_fader_room() {
        // presetLevel 0.05 leaves ~26 dB of its own room, so the FADER is what binds.
        let p = preset(0.05);
        let jobs = vec![
            job(
                session::BASE_SCENE_SLOT,
                -23.0,
                -23.0,
                vec![knob("amp", 0.8, None), knob("amp2", 0.02, None)],
            ),
            // 12 LU short — more than the quiet lane can ever pay for.
            job(0, -15.0, -27.0, vec![knob("amp", 1.0, Some(0))]),
        ];
        let d = plan_trade_for_batch(&jobs, Some(&p), true);
        let i = intent(&d);
        assert!(
            (i.base_fader - 0.02).abs() < 1e-6,
            "the QUIETEST audible lane, got {}",
            i.base_fader
        );
        assert!(
            (i.plan.raise_db - 6.0206).abs() < 1e-3,
            "0.02 → 0.01 is ~6 dB of room, not ~38; got {}",
            i.plan.raise_db
        );
        assert_eq!(
            i.plan.capped,
            Some(crate::headroom_trade::TradeCap::BaseFaderFloor)
        );
    }

    // A lane the author already parked at/below the floor is not a lane the trade can spend —
    // but it must not veto the trade either. The audible lane binds; the parked one rides.
    #[test]
    fn a_base_lane_parked_at_the_floor_does_not_bind_the_room() {
        let p = preset(0.5);
        let jobs = vec![
            job(
                session::BASE_SCENE_SLOT,
                -23.0,
                -23.0,
                vec![knob("amp", 0.8, None), knob("amp2", BASE_FADER_FLOOR, None)],
            ),
            job(0, -15.0, -19.0, vec![knob("amp", 1.0, Some(0))]),
        ];
        let d = plan_trade_for_batch(&jobs, Some(&p), true);
        let i = intent(&d);
        assert!((i.base_fader - 0.8).abs() < 1e-6, "{}", i.base_fader);
        assert!((i.plan.raise_db - 4.0).abs() < 1e-9);
    }

    // A scenes-only batch has no base target to hold and no base fader to hold it with.
    #[test]
    fn a_batch_without_a_base_row_never_trades() {
        let p = preset(0.5);
        let jobs = vec![job(0, -15.0, -19.0, vec![knob("amp", 1.0, Some(0))])];
        assert_eq!(
            plan_trade_for_batch(&jobs, Some(&p), true),
            TradeDecision::None
        );
    }

    // ⟦7⟧ The trade's solved base faders become the jobs' new anchors — but only where the
    // scene actually READS base's value. Scene 0 pins its own `outputLevel` overlay (that is
    // why it benefits), so its anchor must be left alone.
    #[test]
    fn the_solved_base_faders_become_the_inheriting_rows_anchors() {
        let p = preset(0.5);
        let mut jobs = vec![
            job(
                session::BASE_SCENE_SLOT,
                -23.0,
                -23.0,
                vec![knob("amp", 0.8, None)],
            ),
            // Scene 0: a `Full` overlay on the amp → pinned, untouched.
            job(0, -15.0, -19.0, vec![knob("amp", 0.5, Some(0))]),
            // Scene 1: no overlay at all (`Absent`) → it read base's fader, so it moved too.
            job(1, -15.0, -19.0, vec![knob("amp", 0.8, Some(1))]),
        ];
        adopt_trade_levels(&mut jobs, 0, &p, &[0.5044]);
        assert!(
            (jobs[0].knobs[0].current - 0.5044).abs() < 1e-6,
            "base adopts the solve"
        );
        assert!(
            (jobs[1].knobs[0].current - 0.5).abs() < 1e-6,
            "an overlay-pinned scene keeps its own value"
        );
        assert!(
            (jobs[2].knobs[0].current - 0.5044).abs() < 1e-6,
            "an inheriting scene follows base's fader down"
        );
    }
}

#[cfg(test)]
mod scene_handle_tests {
    use super::*;

    /// A 2-scene preset: an amp whose scene-0 overlay carries a KNOB (`Full`) and whose
    /// scene-1 overlay carries only `bypass` (`BypassOnly` — Scene Edit off, knobs shared
    /// with base), plus a pedal no scene mentions at all (`Absent`).
    fn preset() -> serde_json::Value {
        serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "nodeId": "amp", "FenderId": "ACD_TwinReverb65NoFx",
                  "dspUnitParameters": { "outputLevel": 0.5, "volume": 0.7, "bypass": false } },
                { "nodeId": "ped", "FenderId": "ACD_KingOfTone",
                  "dspUnitParameters": { "volume": 1.0, "overdrive": 0.4, "bypass": false } }
            ] } },
            "scenes": [
                { "guitarNodes": { "G1": {
                    "ACD_TwinReverb65NoFx": { "dspUnitParameters": { "outputLevel": 0.3 } } } } },
                { "guitarNodes": { "G1": {
                    "ACD_TwinReverb65NoFx": { "dspUnitParameters": { "bypass": true } } } } }
            ]
        })
    }

    /// The batch's amp candidate — an `outputLevel` on the fixture's amp, so a HANDLE-less
    /// row has something to classify (the fixture carries no `audioGraph.template`, so the
    /// amp path still fails its routing prerequisite; that is the mixed-batch gate below).
    fn amp_candidates() -> Vec<LevelBlockArg> {
        vec![LevelBlockArg {
            group_id: "G1".into(),
            node_id: "amp".into(),
            parameter_id: "outputLevel".into(),
            value: 0.5,
        }]
    }

    /// Build the batch's jobs the way `level_scenes_apply_batched` does: the handles threaded
    /// INTO the builder, sparse and keyed by wire scene slot.
    fn build(
        slots: &[u32],
        handles: &[(u32, (&str, &str, &str))],
        docs: &[(u32, Option<serde_json::Value>)],
        saved: Option<&serde_json::Value>,
    ) -> Result<Vec<leveller::SceneJob>, String> {
        let specs: Vec<(u32, SceneHandleSpec)> = handles
            .iter()
            .map(|(scene, (g, n, p))| {
                (
                    *scene,
                    SceneHandleSpec {
                        group_id: g,
                        node_id: n,
                        parameter_id: p,
                    },
                )
            })
            .collect();
        build_scene_jobs_with_handles(slots, &amp_candidates(), docs, -23.0, saved, &specs)
    }

    // A handle points the row at the user's control, and its starting value / wet-floor
    // anchor is the value authored IN THAT SCENE (the pedal has no scene-0 overlay, so it
    // inherits base — the amp would have taken its overlay's 0.3 instead).
    #[test]
    fn a_handle_repoints_the_row_and_takes_the_scenes_own_value() {
        let preset = preset();
        let docs = vec![(0u32, Some(preset.clone()))];
        let jobs = build(&[0], &[(0, ("G1", "ped", "volume"))], &docs, Some(&preset))
            .expect("an all-handles batch needs no amp prerequisite");
        let sj = &jobs[0];
        assert!(sj.skip.is_none());
        assert!(sj.handle.is_some(), "the row is handle-driven");
        assert!(
            !sj.rebalanceable,
            "one user-chosen control is not a rebalanceable lane pair"
        );
        match &sj.knobs[..] {
            [k] => {
                assert_eq!(k.current, 1.0, "the pedal's authored volume");
                assert_eq!((k.lo, k.hi), (0.0, 1.0));
                assert_eq!(
                    k.knob.label(),
                    "G1/ped/volume@scene0",
                    "the write is scene-scoped"
                );
            }
            n => panic!("expected exactly one handle knob, got {}", n.len()),
        }
    }

    // An unrecognised control is refused BEFORE any device work, as that ROW's skip — the
    // rest of the batch must still run (the lane's own per-scene-skip rule).
    #[test]
    fn an_unclassifiable_handle_skips_only_its_own_row() {
        let preset = preset();
        let docs = vec![(0u32, Some(preset.clone())), (1u32, Some(preset.clone()))];
        let jobs = build(
            &[0, 1],
            &[
                // `overdrive` is a drive control, not a level control.
                (0, ("G1", "ped", "overdrive")),
                (1, ("G1", "ped", "volume")),
            ],
            &docs,
            Some(&preset),
        )
        .expect("one bad handle is a row skip, never a batch abort");
        let reason = jobs[0].skip.as_deref().expect("row 0 refused");
        assert!(
            reason.contains("not a level control"),
            "the shared refusal wording: {reason}"
        );
        assert!(jobs[0].knobs.is_empty(), "a refused row drives nothing");
        assert!(jobs[1].skip.is_none(), "row 1 is unaffected");
    }

    // Without the saved document there is no FenderId to classify against — refuse the row
    // rather than sweep an unclassified control.
    #[test]
    fn a_handle_without_a_saved_read_is_refused() {
        let jobs = build(&[0], &[(0, ("G1", "ped", "volume"))], &[], None)
            .expect("still a row skip, not a batch abort");
        assert!(jobs[0].skip.is_some());
        assert!(jobs[0].knobs.is_empty());
    }

    // BUG→GATE (the mixed-batch class): the amp prerequisites — an `outputLevel` candidate
    // and a readable routing template — are inputs a HANDLE row does not need. The fixture
    // carries no `audioGraph.template`, so the amp classifier fails preset-wide; that must
    // skip only the row that needed the amp, never the row whose control the user named.
    #[test]
    fn an_amp_prerequisite_failure_skips_only_the_rows_that_need_the_amp() {
        let preset = preset();
        let docs = vec![(0u32, Some(preset.clone())), (1u32, Some(preset.clone()))];
        let jobs = build(
            &[0, 1],
            &[(1, ("G1", "ped", "volume"))],
            &docs,
            Some(&preset),
        )
        .expect("a mixed batch must not abort on the amp classifier");
        assert!(
            jobs[0].skip.as_deref().unwrap_or("").contains("routing"),
            "the amp row reports the routing read: {:?}",
            jobs[0].skip
        );
        assert!(jobs[1].skip.is_none(), "the handle row levels regardless");
        assert!(jobs[1].handle.is_some());
    }

    // The picker's two annotations, both read straight off the saved overlays.
    #[test]
    fn handle_rows_annotate_scope_and_headroom_per_scene() {
        let rows = scene_handle_rows(&preset());
        assert_eq!(
            rows.len(),
            2,
            "one row per FS scene (base is not enumerated)"
        );
        let find = |row: &SceneHandleRow, node: &str, param: &str| {
            row.candidates
                .iter()
                .find(|c| c.node_id == node && c.parameter_id == param)
                .cloned()
        };

        // Scene 0: the amp's overlay carries a knob → the write stays in this scene, and
        // the overlay's own 0.3 is the current value (not base's 0.5).
        let amp0 = find(&rows[0], "amp", "outputLevel").expect("amp outputLevel");
        assert_eq!(amp0.scope, "isolated");
        assert_eq!(amp0.current, 0.3);
        assert_eq!(amp0.class, param_class::ParamClass::LevelLinear);
        assert_eq!(
            serde_json::to_value(amp0.class).expect("class serializes"),
            "level_linear",
            "the wire spelling the frontend reads is the table's own"
        );
        assert_eq!(amp0.range, [0.0, 1.0]);
        assert_eq!(amp0.headroom, "full");

        // Scene 1: bypass-only overlay means Scene Edit is OFF, so the knobs are SHARED
        // with base (and `set_knobs` refuses the write) — the picker must say so.
        let amp1 = find(&rows[1], "amp", "outputLevel").expect("amp outputLevel");
        assert_eq!(amp1.scope, "shared_with_base");
        assert_eq!(amp1.current, 0.5, "shared, so it reads the BASE value");

        // No overlay at all: the Scene Edit enable materialises one, so still isolated; and
        // a control authored at the top of its range can only go DOWN.
        let ped = find(&rows[0], "ped", "volume").expect("pedal volume");
        assert_eq!(ped.scope, "isolated");
        assert_eq!(ped.headroom, "lowers_only");

        // The classifier's bars hold here too: an AMP's `volume` is the breakup knob, and a
        // pedal's `overdrive` is a drive control — neither is ever offered.
        assert!(find(&rows[0], "amp", "volume").is_none());
        assert!(find(&rows[0], "ped", "overdrive").is_none());
    }

    // ── Part A: base-row handle candidates (no overlay/scope concept — always isolated) ──

    /// Test-only convenience: `base_handle_candidates_scanned` takes an already-built
    /// [`NodeGraphScan`] (production callers — `backup_read` — build it once and share it
    /// with the scene derivation too), so there is no live preset-in wrapper to call the way
    /// `scene_handle_rows(&preset)` works. Every test below wants a single preset in, so
    /// build-and-scan here instead of repeating `base_handle_candidates_scanned(&scan_node_graph(&preset))`
    /// at every call site.
    fn base_handle_candidates(preset: &serde_json::Value) -> Vec<SceneHandleCandidate> {
        base_handle_candidates_scanned(&scan_node_graph(preset))
    }

    #[test]
    fn base_handle_candidates_matches_the_scene_zero_candidates_scope_isolated() {
        let out = base_handle_candidates(&preset());
        let find = |node: &str, param: &str| {
            out.iter()
                .find(|c| c.node_id == node && c.parameter_id == param)
                .cloned()
        };

        // Same classifier gate + range/current/headroom math as a scene candidate, but no
        // overlay concept at all: every base candidate is unconditionally "isolated".
        let amp = find("amp", "outputLevel").expect("amp outputLevel");
        assert_eq!(amp.scope, "isolated");
        assert_eq!(
            amp.current, 0.5,
            "the preset's BASE value, no overlay to prefer"
        );
        assert_eq!(amp.class, param_class::ParamClass::LevelLinear);
        assert_eq!(amp.range, [0.0, 1.0]);
        assert_eq!(amp.headroom, "full");

        // A control authored at the top of its range can only go down, same as a scene row.
        let ped = find("ped", "volume").expect("pedal volume");
        assert_eq!(ped.scope, "isolated");
        assert_eq!(ped.current, 1.0);
        assert_eq!(ped.headroom, "lowers_only");

        // The classifier's bars hold here too — an amp's `volume` (breakup) and a pedal's
        // `overdrive` (drive) are never offered, exactly like the scene picker.
        assert!(find("amp", "volume").is_none());
        assert!(find("ped", "overdrive").is_none());
        assert_eq!(out.len(), 2, "exactly the two level-classified candidates");
    }

    #[test]
    fn base_handle_candidates_is_empty_for_a_graph_with_no_numeric_level_params() {
        let preset = serde_json::json!({ "audioGraph": { "guitarNodes": {} } });
        assert!(base_handle_candidates(&preset).is_empty());
    }

    // BUG: `base_handle_candidates` used to walk BOTH graphs (`audiograph::roster`), so a
    // mic-node candidate could surface in the Base picker. Only the guitar chain reaches the
    // USB-Out the leveler measures (`session::extract_level_candidates`'s own rationale) —
    // Base must be guitar-only. `scene_handle_rows` keeps its pre-existing both-graphs walk.
    #[test]
    fn base_handle_candidates_is_guitar_only_but_scene_handle_rows_still_walks_both_graphs() {
        let preset = serde_json::json!({
            "audioGraph": {
                "guitarNodes": { "G1": [
                    { "nodeId": "amp", "FenderId": "ACD_TwinReverb65NoFx",
                      "dspUnitParameters": { "outputLevel": 0.5, "bypass": false } }
                ] },
                "micNodes": { "M1": [
                    { "nodeId": "micpre", "FenderId": "ACD_TwinReverb65NoFx",
                      "dspUnitParameters": { "outputLevel": 0.5, "bypass": false } }
                ] }
            },
            "scenes": [ {} ]
        });

        let base = base_handle_candidates(&preset);
        assert!(
            base.iter().any(|c| c.node_id == "amp"),
            "the guitar node is still offered"
        );
        assert!(
            base.iter().all(|c| c.node_id != "micpre"),
            "a mic-graph node must never surface in the Base picker"
        );

        let rows = scene_handle_rows(&preset);
        assert!(
            rows[0].candidates.iter().any(|c| c.node_id == "micpre"),
            "Scene's picker is unaffected — it still offers either graph's blocks"
        );
    }

    // ── Part B: audibility-guarded BypassOnly scope (bug: preset 28 "Friedman HBE",
    // `ACD_Boost`/`gain` — a Solo-only shared write was annotated "shared_with_base" and
    // disabled, even though the leak is inaudible everywhere else) ─────────────────────

    /// The bug's exact shape: `boost`/`ACD_Boost` (group G1) bypassed in base with `gain`
    /// 2.5; Dirt (0) and Crunch (3) carry Full overlays (own bypass + gain); Clean (1) is
    /// bypass-only and stays bypassed; Solo (2) is bypass-only and UN-bypassed — the only
    /// scene that can hear a plain leak-to-base write.
    fn hbe_boost_preset() -> serde_json::Value {
        serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "nodeId": "boost", "FenderId": "ACD_Boost",
                  "dspUnitParameters": { "bypass": true, "gain": 2.5 } }
            ] } },
            "scenes": [
                { "guitarNodes": { "G1": {
                    "ACD_Boost": { "dspUnitParameters": { "bypass": true, "gain": 5.0 } } } } },
                { "guitarNodes": { "G1": {
                    "ACD_Boost": { "dspUnitParameters": { "bypass": true } } } } },
                { "guitarNodes": { "G1": {
                    "ACD_Boost": { "dspUnitParameters": { "bypass": false } } } } },
                { "guitarNodes": { "G1": {
                    "ACD_Boost": { "dspUnitParameters": { "bypass": true, "gain": 6.0 } } } } }
            ]
        })
    }

    fn find_boost_gain(row: &SceneHandleRow) -> Option<SceneHandleCandidate> {
        row.candidates
            .iter()
            .find(|c| c.node_id == "boost" && c.parameter_id == "gain")
            .cloned()
    }

    // THE bug: Solo (scene 2) is the only scene the shared write is audible in, so the
    // picker must offer it — Dirt/Crunch stay isolated as genuine overlays, and Clean stays
    // shared (the leak there is silent, so writing it there would still change Solo too).
    #[test]
    fn bypass_only_boost_gain_scopes_isolated_only_on_the_solo_scene() {
        let rows = scene_handle_rows(&hbe_boost_preset());
        assert_eq!(rows.len(), 4);
        assert_eq!(
            find_boost_gain(&rows[0]).expect("Dirt gain").scope,
            "isolated",
            "Full overlay"
        );
        assert_eq!(
            find_boost_gain(&rows[1]).expect("Clean gain").scope,
            "shared_with_base",
            "still bypassed — a shared write here would also change Solo"
        );
        let solo = find_boost_gain(&rows[2]).expect("Solo gain");
        assert_eq!(solo.scope, "isolated", "THE bug fix");
        assert_eq!(
            solo.current, 2.5,
            "BypassOnly reads the base value, not a fabricated one"
        );
        assert_eq!(
            find_boost_gain(&rows[3]).expect("Crunch gain").scope,
            "isolated",
            "Full overlay"
        );
    }

    #[test]
    fn bypass_only_boost_gain_falls_back_to_shared_when_a_second_scene_is_audible() {
        let mut preset = hbe_boost_preset();
        // Clean (scene 1) un-bypassed too, with no Full overlay pinning `gain` there — now
        // BOTH Clean and Solo would hear the shared write, so neither can safely take it.
        preset["scenes"][1]["guitarNodes"]["G1"]["ACD_Boost"] =
            serde_json::json!({ "dspUnitParameters": { "bypass": false } });
        let rows = scene_handle_rows(&preset);
        assert_eq!(
            find_boost_gain(&rows[1]).expect("Clean gain").scope,
            "shared_with_base"
        );
        assert_eq!(
            find_boost_gain(&rows[2]).expect("Solo gain").scope,
            "shared_with_base"
        );
    }

    #[test]
    fn bypass_only_boost_gain_stays_shared_when_already_audible_in_base() {
        let mut preset = hbe_boost_preset();
        preset["audioGraph"]["guitarNodes"]["G1"][0]["dspUnitParameters"]["bypass"] =
            serde_json::json!(false);
        let rows = scene_handle_rows(&preset);
        assert_eq!(
            find_boost_gain(&rows[2]).expect("Solo gain").scope,
            "shared_with_base"
        );
    }

    // ── Part C: `enables_block` (issue 5 — Boost preselect) ─────────────────────────────

    // THE E2E-Edge shape: base-bypassed Boost, Solo (scene 2) is the ONLY scene whose own
    // overlay positively un-bypasses it (`bypass: false`) — that is exactly the signal the
    // frontend should preselect on.
    #[test]
    fn enables_block_is_true_only_on_the_scene_that_un_bypasses_a_base_bypassed_node() {
        let rows = scene_handle_rows(&hbe_boost_preset());
        assert_eq!(rows.len(), 4);
        assert!(
            !find_boost_gain(&rows[0]).expect("Dirt gain").enables_block,
            "Dirt's overlay keeps bypass:true — it doesn't enable the block"
        );
        assert!(
            !find_boost_gain(&rows[1]).expect("Clean gain").enables_block,
            "Clean's overlay ALSO keeps bypass:true"
        );
        assert!(
            find_boost_gain(&rows[2]).expect("Solo gain").enables_block,
            "Solo's overlay is the one that flips bypass:false — THE preselect signal"
        );
        assert!(
            !find_boost_gain(&rows[3])
                .expect("Crunch gain")
                .enables_block,
            "Crunch's overlay keeps bypass:true"
        );
        // The wire name B-FRONT reads: camelCase `enablesBlock`, not the Rust field name.
        let json =
            serde_json::to_value(find_boost_gain(&rows[2]).expect("Solo gain")).expect("serialize");
        assert_eq!(json["enablesBlock"], serde_json::json!(true));
    }

    // A node already ACTIVE in base has nothing for a scene to "enable" — even a scene whose
    // overlay carries `bypass: false` (redundantly restating base) must not preselect it.
    #[test]
    fn enables_block_is_false_when_the_node_is_already_active_in_base() {
        let mut preset = hbe_boost_preset();
        preset["audioGraph"]["guitarNodes"]["G1"][0]["dspUnitParameters"]["bypass"] =
            serde_json::json!(false);
        let rows = scene_handle_rows(&preset);
        for (i, row) in rows.iter().enumerate() {
            assert!(
                !find_boost_gain(row)
                    .unwrap_or_else(|| panic!("scene {i} gain"))
                    .enables_block,
                "scene {i}: base is already active, so no scene can \"enable\" it"
            );
        }
    }

    // A `BypassOnly` overlay that keeps `bypass: true` (Clean, scene 1) never enables the
    // block, matching the isolated assertion above but pinned as its own test per the plan.
    #[test]
    fn enables_block_is_false_for_a_bypass_only_overlay_that_stays_bypassed() {
        let rows = scene_handle_rows(&hbe_boost_preset());
        assert!(matches!(
            scene_overlay(&hbe_boost_preset(), 1, "boost"),
            SceneOverlay::BypassOnly(_)
        ));
        assert!(!find_boost_gain(&rows[1]).expect("Clean gain").enables_block);
    }

    // Base rows have no scene/overlay concept — `enables_block` is unconditionally false.
    #[test]
    fn enables_block_is_always_false_on_base_handle_candidates() {
        let base = base_handle_candidates(&hbe_boost_preset());
        assert!(
            base.iter().all(|c| !c.enables_block),
            "Base has nothing to enable relative to: {base:?}"
        );
    }
}
