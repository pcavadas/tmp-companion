//! Footswitch (engaged-state) leveling command + job resolution.
#![allow(clippy::too_many_arguments)]
use crate::*;

// ───────────────────────── Footswitch (engaged-state) leveling ─────────────────────────

/// One footswitch-leveling request: level switch `switch`'s engaged state by solving the
/// `(lev_group_id, lev_node_id, lev_parameter_id)` param to hit `target_lufs`.
#[derive(serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FootswitchLevelJob {
    pub(crate) switch: u32,
    pub(crate) lev_group_id: String,
    pub(crate) lev_node_id: String,
    pub(crate) lev_parameter_id: String,
    pub(crate) target_lufs: f64,
    /// The switch's CURRENT display label as the UI shows it (the Level list's footswitch row
    /// name: the player's `customLabel`, else the toggled block's friendly name). Used ONLY
    /// when the assign path appends a second function to a switch whose `customLabel` is empty
    /// — the unit displays "MULTI" for a multi-function switch with no label, so writing the
    /// prior display name keeps the pedalboard reading the same. Absent → today's behavior.
    #[serde(default)]
    pub(crate) display_label: Option<String>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FootswitchLevelProgressItem {
    switch: u32,
    status: String, // active | done | error | cancelled
    result: Option<leveller::FootswitchLevelResult>,
    message: Option<String>,
}

static FOOTSWITCH_LEVEL_CANCEL: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub(crate) fn cancel_footswitch_leveling() {
    FOOTSWITCH_LEVEL_CANCEL.store(true, SeqCst);
    // Also wake the in-flight capture/settle waits (see `device_gate::OP_ABORT`).
    crate::request_op_abort();
}

/// Read a slot's field-8 preset JSON on a fresh quiet session and return the parsed preset, a
/// DIAGNOSTIC "has FS scenes" flag (`Some(empty)` = definitely no FS scenes; truncated/unknown or
/// non-empty → conservative `true`) and the raw byte length. Shared by the footswitch leveling
/// command + probes (the connect→drain→read→parse→scene-check boilerplate). The flag is NO LONGER
/// a gate: `footswitch::plan_footswitch_jobs` decides bake-vs-assign PER NODE off this same parsed
/// document (`scene_jobs::scene_overlays_change_param`) — only `probe --fs-list` still prints it.
pub(crate) fn read_slot_preset_parsed(
    slot: u32,
) -> Result<(serde_json::Value, bool, usize), String> {
    let mut s = Session::connect()?;
    s.drain_until_quiet(250, 20)?;
    let json = s
        .read_slot_preset_json(slot + 1)?
        .ok_or_else(|| format!("no preset data for slot {}", slot + 1))?;
    let preset = session::tolerant_parse_json(&String::from_utf8_lossy(&json))
        .ok_or_else(|| "preset JSON did not parse".to_string())?;
    let has_fs_scenes = session::scene_names_from_slot_json(&json).is_none_or(|n| !n.is_empty());
    Ok((preset, has_fs_scenes, json.len()))
}

/// A numeric `dspUnitParameter` of `node_id` (e.g. the lev param's current value = `valueB`).
pub(crate) fn node_param_f64(
    preset: &serde_json::Value,
    node_id: &str,
    param: &str,
) -> Option<f64> {
    let mut found = None;
    audiograph::for_each_node(preset, |obj| {
        if obj.get("nodeId").and_then(|v| v.as_str()) == Some(node_id) {
            found = obj
                .get("dspUnitParameters")
                .and_then(|p| p.get(param))
                .and_then(|v| v.as_f64());
        }
    });
    found
}

/// Resolved inputs to `leveller::level_footswitch`: the switch-OFF value (`valueB` = the
/// param's current value) and the write spec.
type FootswitchJobResolution = (f32, leveller::FootswitchWriteSpec);

/// Resolve a footswitch-leveling job against the preset: the lev param's current value
/// (`valueB`) and the write spec (edit an existing matching `param` function, else add at
/// the next free index; enforce the firmware's 5-function cap). The leveler only ever
/// creates/edits a parameter-change assignment — it does not touch on/off.
pub(crate) fn resolve_footswitch_job(
    ftsw: &serde_json::Value,
    preset: &serde_json::Value,
    job: &FootswitchLevelJob,
) -> Result<FootswitchJobResolution, String> {
    let switches = ftsw.as_array().ok_or("preset has no ftsw")?;
    let sw = switches
        .get(job.switch as usize)
        .and_then(|s| s.as_array())
        .ok_or_else(|| format!("footswitch {} not found", job.switch))?;

    let value_b =
        node_param_f64(preset, &job.lev_node_id, &job.lev_parameter_id).ok_or_else(|| {
            format!(
                "parameter {} not found on {}",
                job.lev_parameter_id, job.lev_node_id
            )
        })? as f32;

    // Edit an existing param fn on (lev_node, lev_param), else add (≤5 cap).
    let existing = footswitch::existing_param_fn_index(
        ftsw,
        job.switch,
        &job.lev_node_id,
        &job.lev_parameter_id,
    )
    .and_then(|i| sw.get(i as usize).map(|a| (i, a)));
    // colorA/colorB/customLabel/linkGroup/switchType are SWITCH-level — the manual:
    // "common to all five footswitch assignments" — so both an edited existing
    // function AND a brand-new one read them the same way (an absent field on an
    // existing function falls back to the historical constants, same as a switch
    // with no sibling to inherit from). isActive is NOT switch-level (it's the
    // per-function ENGAGED state — HW round-trip: engaging an unlinked switch and
    // re-saving flipped it false→true on that one function only) — a firmware-
    // authored disengaged assignment reads false, so an absent field means
    // disengaged, never inherited/defaulted to engaged.
    let field_u64 = |v: Option<&serde_json::Value>, field: &str, default: u64| -> u32 {
        v.and_then(|a| a.get(field))
            .and_then(|v| v.as_u64())
            .unwrap_or(default) as u32
    };
    let field_str = |v: Option<&serde_json::Value>, field: &str| -> String {
        v.and_then(|a| a.get(field))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let spec = match existing {
        Some((i, a)) => leveller::FootswitchWriteSpec {
            function_index: i,
            color_a: field_u64(Some(a), "colorA", 3),
            color_b: field_u64(Some(a), "colorB", 0),
            custom_label: field_str(Some(a), "customLabel"),
            link_group: field_u64(Some(a), "linkGroup", 0),
            is_active: a.get("isActive").and_then(|v| v.as_bool()).unwrap_or(false),
            switch_type: field_u64(Some(a), "switchType", 0),
        },
        None => {
            if sw.len() >= 5 {
                return Err(format!(
                    "footswitch {} is full (5 functions) — no room to add a leveling param",
                    job.switch
                ));
            }
            // A new assignment must INHERIT the switch-level fields from an existing
            // sibling function (`sw.first()`), not hardcode defaults: a linked
            // on-off's linkGroup would otherwise drop the switch out of its Switch
            // Link, and colour/label would silently diverge from the switch's other
            // assignments (Fender's own CloudPresets multi-function switches keep
            // all five fields identical across every assignment). Falls back to the
            // historical constants only when the switch has no existing function.
            let sibling = sw.first();
            // A second function on an UNLABELLED switch makes the unit display "MULTI" instead
            // of the single function's implied name. Nothing else changes the label: an already
            // labelled switch keeps the player's own text (inherited), and the first function on
            // an empty switch shows its own name, so there is nothing to preserve there.
            let inherited = field_str(sibling, "customLabel");
            let custom_label = match (inherited.is_empty(), sibling, &job.display_label) {
                (true, Some(_), Some(shown)) => shown.clone(),
                _ => inherited,
            };
            leveller::FootswitchWriteSpec {
                function_index: sw.len() as u32,
                color_a: field_u64(sibling, "colorA", 3),
                color_b: field_u64(sibling, "colorB", 0),
                custom_label,
                link_group: field_u64(sibling, "linkGroup", 0),
                is_active: false,
                switch_type: field_u64(sibling, "switchType", 0),
            }
        }
    };
    Ok((value_b, spec))
}

/// Level one or more block-acting footswitches of preset `slot`, streaming a progress item
/// per switch. Each switch's engaged state is measured/solved independently against the
/// base preset; jobs run sequentially. Mirrors `level_scenes_apply_batched`.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn level_footswitches_apply<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    slot: u32,
    jobs: Vec<FootswitchLevelJob>,
    save: bool,
    topology_id: Option<String>,
    calibration_lufs: Option<f32>,
    profile_id: Option<String>,
    on_result: tauri::ipc::Channel<FootswitchLevelProgressItem>,
) -> Result<Vec<leveller::FootswitchLevelResult>, String> {
    let (stim_path, calibration_lufs) = resolve_stimulus_for_leveling(
        &app,
        None,
        topology_id.clone(),
        profile_id.as_deref(),
        calibration_lufs,
    )?;
    let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;
    let offset = playback_offset_for(&app, topology_id.as_deref());
    FOOTSWITCH_LEVEL_CANCEL.store(false, SeqCst);
    let app_evt = app.clone();

    with_released_seize(state.session.clone(), move || {
        // Stream advisory live LUFS while each capture runs (dropped at closure end).
        let _lufs = LiveLufsGuard::install(app_evt);
        // THE single field-8 read: it resolves every job AND is the planner's scene-overlay
        // source (per-node bake gate) — never add a second read here.
        let (preset, _, _) = read_slot_preset_parsed(slot)?;
        crate::settle(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
        let ftsw = preset
            .get("ftsw")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        // Plan bake-vs-assign for the whole batch (pure) — block-off-in-base + sole-owner + no
        // scene overlay on THAT node's bypass ⇒ bake straight onto the block (no `ftsw` write, so
        // the switch keeps its single function and its label); otherwise the param assignment.
        let keys: Vec<footswitch::FsJobKey> = jobs
            .iter()
            .map(|j| footswitch::FsJobKey {
                switch: j.switch,
                lev_node: &j.lev_node_id,
                lev_param: &j.lev_parameter_id,
                target_bits: j.target_lufs.to_bits(),
            })
            .collect();
        let plans = footswitch::plan_footswitch_jobs(&ftsw, &preset, &keys);

        // Load the preset ONCE for the whole batch — `measure_footswitch`'s caller
        // contract. Every job's sweep runs against this load (its pollution is
        // self-correcting: each job's force list explicitly sets every sibling
        // block's bypass, and swept params live on blocks the next job forces off);
        // the ONE write session's reload discards it all at the end.
        {
            let mut s = Session::connect_lean()?;
            s.load_preset(slot)?;
            crate::settle(std::time::Duration::from_millis(
                leveller::settle_after_load_ms(),
            ));
        }
        crate::settle(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));

        let mut results: Vec<Option<leveller::FootswitchLevelResult>> = vec![None; jobs.len()];
        // The solved writes pending the batch's single write+save session, each
        // carrying its job index (one vec — no hand-aligned parallel arrays).
        let mut pending: Vec<(usize, leveller::FsPendingWrite)> = Vec::new();
        for (idx, job) in jobs.iter().enumerate() {
            if FOOTSWITCH_LEVEL_CANCEL.load(SeqCst) {
                let _ = on_result.send(FootswitchLevelProgressItem {
                    switch: job.switch,
                    status: "cancelled".into(),
                    result: None,
                    message: None,
                });
                break;
            }
            let _ = on_result.send(FootswitchLevelProgressItem {
                switch: job.switch,
                status: "active".into(),
                result: None,
                message: None,
            });
            let lev = (
                job.lev_group_id.as_str(),
                job.lev_node_id.as_str(),
                job.lev_parameter_id.as_str(),
            );
            let lev_owned = || {
                (
                    job.lev_group_id.clone(),
                    job.lev_node_id.clone(),
                    job.lev_parameter_id.clone(),
                )
            };
            let outcome: Result<leveller::FootswitchLevelResult, String> = match &plans[idx] {
                footswitch::FsLevelPlan::Clamp(msg) => Err(msg.clone()),
                // A sibling switch already baked this (node, param, target) — reuse its result.
                footswitch::FsLevelPlan::BakeShared { rep } => results[*rep]
                    .clone()
                    .map(|mut r| {
                        r.switch = job.switch;
                        r
                    })
                    .ok_or_else(|| "shared bake produced no result".to_string()),
                // Bake has no cheap re-run marker (the block's param value is `Some` from the
                // factory too), so it always solves — no idempotency probe (`current` = None).
                footswitch::FsLevelPlan::Bake {
                    engaged,
                    clear_stale,
                    mirror_scenes,
                } => leveller::measure_footswitch(
                    job.switch,
                    lev,
                    engaged,
                    &stim,
                    job.target_lufs + offset,
                    "baked",
                    None,
                )
                .inspect(|r| {
                    if save && r.clamp_reason.is_none() {
                        pending.push((
                            idx,
                            leveller::FsPendingWrite {
                                switch: job.switch,
                                lev: lev_owned(),
                                write: leveller::FsWrite::Bake {
                                    clear_stale: *clear_stale,
                                    mirror_scenes: mirror_scenes.clone(),
                                },
                                value: r.final_value,
                            },
                        ));
                    }
                }),
                footswitch::FsLevelPlan::Assign { engaged } => {
                    match resolve_footswitch_job(&ftsw, &preset, job) {
                        Err(e) => Err(e),
                        Ok((value_b, spec)) => {
                            // Re-run anchor: a prior assign's stored valueA (None = fresh).
                            let current = footswitch::existing_param_fn_value_a(
                                &ftsw,
                                job.switch,
                                &job.lev_node_id,
                                &job.lev_parameter_id,
                            )
                            .map(|v| v as f32);
                            leveller::measure_footswitch(
                                job.switch,
                                lev,
                                engaged,
                                &stim,
                                job.target_lufs + offset,
                                "assigned",
                                current,
                            )
                            // Skip the write when the leveler left the value unchanged — its
                            // `final_value == current` is the idempotency signal (no wire field).
                            .inspect(|r| {
                                if save
                                    && r.clamp_reason.is_none()
                                    && Some(r.final_value) != current
                                {
                                    pending.push((
                                        idx,
                                        leveller::FsPendingWrite {
                                            switch: job.switch,
                                            lev: lev_owned(),
                                            write: leveller::FsWrite::Assign { value_b, spec },
                                            value: r.final_value,
                                        },
                                    ));
                                }
                            })
                        }
                    }
                }
            };
            // A Stop mid-sweep surfaces as the CANCELLED sentinel, not a failure — report
            // it like the top-of-loop check would rather than as an errored switch.
            if let Err(e) = &outcome {
                if *e == leveller::CANCELLED {
                    let _ = on_result.send(FootswitchLevelProgressItem {
                        switch: job.switch,
                        status: "cancelled".into(),
                        result: None,
                        message: None,
                    });
                    break;
                }
            }
            let item = match outcome {
                Ok(r) => {
                    results[idx] = Some(r.clone());
                    FootswitchLevelProgressItem {
                        switch: job.switch,
                        status: "done".into(),
                        result: Some(r),
                        message: None,
                    }
                }
                Err(e) => FootswitchLevelProgressItem {
                    switch: job.switch,
                    status: "error".into(),
                    result: None,
                    message: Some(e),
                },
            };
            let _ = on_result.send(item);
        }
        // ── ONE write session + ONE save for every solved switch (also fired after a
        // cancel, so already-reported switches persist), then a reload to leave the
        // working copy clean. No post-save verify capture: `predicted_lufs` is already
        // a REAL measurement at `final_value` (the sweep's best point), not a model
        // prediction — re-measuring it bought nothing but ~10 s per switch.
        let write_result = if save && !pending.is_empty() {
            let (idxs, writes): (Vec<usize>, Vec<leveller::FsPendingWrite>) =
                pending.into_iter().unzip();
            // Re-stamp the preset's original `lastLoadedScene`: the write session's base
            // recall (and any bake-mirror scene recalls) leave the wrong scene active, and
            // the save records the active one (HW: the FS save stamped 8 over scene 3).
            let restore = crate::last_loaded_scene(&preset);
            crate::warn_missing_restore_scene("level_footswitches", slot, &preset, restore);
            leveller::write_footswitch_values(slot, &writes, restore).map(|()| {
                let written: std::collections::HashSet<usize> = idxs.iter().copied().collect();
                for &idx in &idxs {
                    if let Some(r) = &mut results[idx] {
                        r.saved = true;
                    }
                }
                // Propagate the persisted state to BakeShared siblings that reused a
                // now-saved representative's result (they share the same written write).
                for (idx, plan) in plans.iter().enumerate() {
                    if let footswitch::FsLevelPlan::BakeShared { rep } = plan {
                        if written.contains(rep) {
                            if let Some(r) = &mut results[idx] {
                                r.saved = true;
                            }
                        }
                    }
                }
            })
        } else {
            // Dry run / nothing solved: discard the sweep pollution.
            let _ = Session::connect_lean().map(|mut s| s.load_preset(slot));
            Ok(())
        };
        // Guarantee re-amp OFF on a fresh connection.
        if let Ok(mut s) = Session::connect_lean() {
            let _ = s.set_reamp_mode(false);
        }
        write_result?;
        Ok(results.into_iter().flatten().collect())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset_with_lev_param(value: f64) -> serde_json::Value {
        serde_json::json!({
            "audioGraph": {
                "template": "gtrSeries",
                "guitarNodes": {
                    "G1": [
                        { "FenderId": "amp", "nodeId": "amp",
                          "dspUnitParameters": { "drive": value } }
                    ]
                }
            }
        })
    }

    fn job() -> FootswitchLevelJob {
        FootswitchLevelJob {
            switch: 0,
            lev_group_id: "G1".into(),
            lev_node_id: "amp".into(),
            lev_parameter_id: "drive".into(),
            target_lufs: -20.0,
            display_label: None,
        }
    }

    // (A4) A NEW assignment on a switch that already has a function must inherit
    // that sibling's switch-level fields (colour/label/link/switchType) rather
    // than hardcode defaults — a linked on-off's linkGroup would otherwise drop
    // the switch out of its Switch Link, and colour/label would silently diverge.
    #[test]
    fn new_assignment_inherits_sibling_switch_level_fields() {
        let ftsw = serde_json::json!([
            [
                {
                    "func": "on-off",
                    "colorA": 1, "colorB": 2,
                    "customLabel": "BOOST",
                    "linkGroup": 1,
                    "isActive": true,
                    "switchType": 1
                }
            ]
        ]);
        let preset = preset_with_lev_param(0.5);
        let (_, spec) = resolve_footswitch_job(&ftsw, &preset, &job()).expect("resolve");
        assert_eq!(
            spec.function_index, 1,
            "appended after the existing on-off fn"
        );
        assert_eq!(spec.color_a, 1, "colorA inherited from the sibling");
        assert_eq!(spec.color_b, 2, "colorB inherited from the sibling");
        assert_eq!(
            spec.custom_label, "BOOST",
            "customLabel inherited from the sibling"
        );
        assert_eq!(
            spec.link_group, 1,
            "linkGroup inherited — else the switch drops out of its Switch Link"
        );
        assert_eq!(spec.switch_type, 1, "switchType inherited from the sibling");
        assert!(
            !spec.is_active,
            "isActive is per-function engaged state, NOT inherited — a fresh assignment starts disengaged"
        );
    }

    // An empty switch (no existing function to inherit from) falls back to the
    // historical constants — there is nothing to inherit.
    #[test]
    fn new_assignment_on_empty_switch_falls_back_to_defaults() {
        let ftsw = serde_json::json!([[]]);
        let preset = preset_with_lev_param(0.5);
        let (_, spec) = resolve_footswitch_job(&ftsw, &preset, &job()).expect("resolve");
        assert_eq!(spec.function_index, 0);
        assert_eq!(spec.color_a, 3);
        assert_eq!(spec.color_b, 0);
        assert_eq!(spec.custom_label, "");
        assert_eq!(spec.link_group, 0);
        assert_eq!(spec.switch_type, 0);
        assert!(!spec.is_active);
    }

    // BUG→GATE (the "MULTI" class): adding a SECOND function to a switch whose `customLabel`
    // is empty makes the unit stop showing the single function's implied name and display
    // "MULTI" instead. The UI's own row label rides along on the job, so write it as the
    // switch's `customLabel` and the pedalboard display doesn't change.
    #[test]
    fn new_assignment_labels_an_unlabelled_switch_with_the_ui_display_label() {
        let ftsw = serde_json::json!([[{ "func": "on-off", "customLabel": "" }]]);
        let preset = preset_with_lev_param(0.5);
        let job = FootswitchLevelJob {
            display_label: Some("Mythic Drive".into()),
            ..job()
        };
        let (_, spec) = resolve_footswitch_job(&ftsw, &preset, &job).expect("resolve");
        assert_eq!(spec.function_index, 1, "a SECOND function is being added");
        assert_eq!(
            spec.custom_label, "Mythic Drive",
            "the switch's prior display label is preserved as its customLabel"
        );
    }

    // The player's own label is never touched — inheritance still wins.
    #[test]
    fn new_assignment_keeps_a_non_empty_sibling_label() {
        let ftsw = serde_json::json!([[{ "func": "on-off", "customLabel": "BOOST" }]]);
        let preset = preset_with_lev_param(0.5);
        let job = FootswitchLevelJob {
            display_label: Some("Mythic Drive".into()),
            ..job()
        };
        let (_, spec) = resolve_footswitch_job(&ftsw, &preset, &job).expect("resolve");
        assert_eq!(spec.custom_label, "BOOST");
    }

    // A switch with NO existing function gets a single function — the unit shows that
    // function's own name, never "MULTI", so there is nothing to preserve.
    #[test]
    fn first_assignment_on_an_empty_switch_is_left_unlabelled() {
        let ftsw = serde_json::json!([[]]);
        let preset = preset_with_lev_param(0.5);
        let job = FootswitchLevelJob {
            display_label: Some("Mythic Drive".into()),
            ..job()
        };
        let (_, spec) = resolve_footswitch_job(&ftsw, &preset, &job).expect("resolve");
        assert_eq!(spec.function_index, 0);
        assert_eq!(spec.custom_label, "");
    }

    // An EXISTING param assignment's isActive reads its own field verbatim — an
    // absent field means disengaged (false), not the old unwrap_or(true) default.
    #[test]
    fn existing_assignment_missing_is_active_defaults_to_disengaged() {
        let ftsw = serde_json::json!([
            [
                {
                    "func": "param", "groupId": "G1", "nodeId": "amp", "parameterId": "drive",
                    "colorA": 5, "colorB": 9, "customLabel": "X", "linkGroup": 2
                }
            ]
        ]);
        let preset = preset_with_lev_param(0.5);
        let (_, spec) = resolve_footswitch_job(&ftsw, &preset, &job()).expect("resolve");
        assert_eq!(
            spec.function_index, 0,
            "edits the existing param fn, not a new one"
        );
        assert!(
            !spec.is_active,
            "an absent isActive must default to disengaged, not engaged"
        );
    }
}
