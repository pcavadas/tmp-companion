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
}

/// Read a slot's field-8 preset JSON on a fresh quiet session and return the parsed preset, the
/// scene gate (`Some(empty)` = definitely no FS scenes; truncated/unknown or non-empty →
/// conservative `true`), and the raw byte length. Shared by the footswitch leveling command +
/// probes (the connect→drain→read→parse→scene-check boilerplate).
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
    let spec = match existing {
        Some((i, a)) => leveller::FootswitchWriteSpec {
            function_index: i,
            color_a: a.get("colorA").and_then(|v| v.as_u64()).unwrap_or(3) as u32,
            color_b: a.get("colorB").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            custom_label: a
                .get("customLabel")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            link_group: a.get("linkGroup").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            is_active: a.get("isActive").and_then(|v| v.as_bool()).unwrap_or(true),
        },
        None => {
            if sw.len() >= 5 {
                return Err(format!(
                    "footswitch {} is full (5 functions) — no room to add a leveling param",
                    job.switch
                ));
            }
            leveller::FootswitchWriteSpec {
                function_index: sw.len() as u32,
                color_a: 3,
                color_b: 0,
                custom_label: String::new(),
                link_group: 0,
                is_active: true,
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
pub(crate) async fn level_footswitches_apply(
    app: tauri::AppHandle,
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
        // Read the preset once (resolve every job) + whether it has FS scenes (the bake gate).
        let (preset, has_fs_scenes, _) = read_slot_preset_parsed(slot)?;
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
        let ftsw = preset
            .get("ftsw")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        // Plan bake-vs-assign for the whole batch (pure) — block-off-in-base + sole-owner +
        // no-scenes ⇒ bake straight onto the block; otherwise the (engaged-measured) param fn.
        let keys: Vec<footswitch::FsJobKey> = jobs
            .iter()
            .map(|j| footswitch::FsJobKey {
                switch: j.switch,
                lev_node: &j.lev_node_id,
                lev_param: &j.lev_parameter_id,
                target_bits: j.target_lufs.to_bits(),
            })
            .collect();
        let plans = footswitch::plan_footswitch_jobs(&ftsw, &preset, &keys, has_fs_scenes);

        // Load the preset ONCE for the whole batch — `measure_footswitch`'s caller
        // contract. Every job's sweep runs against this load (its pollution is
        // self-correcting: each job's force list explicitly sets every sibling
        // block's bypass, and swept params live on blocks the next job forces off);
        // the ONE write session's reload discards it all at the end.
        {
            let mut s = Session::connect_lean()?;
            s.load_preset(slot)?;
            std::thread::sleep(std::time::Duration::from_millis(
                leveller::settle_after_load_ms(),
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));

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
                footswitch::FsLevelPlan::Bake {
                    engaged,
                    clear_stale,
                } => leveller::measure_footswitch(
                    job.switch,
                    lev,
                    engaged,
                    &stim,
                    job.target_lufs + offset,
                    "baked",
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
                                },
                                value: r.final_value,
                            },
                        ));
                    }
                }),
                footswitch::FsLevelPlan::Assign { engaged } => {
                    match resolve_footswitch_job(&ftsw, &preset, job) {
                        Err(e) => Err(e),
                        Ok((value_b, spec)) => leveller::measure_footswitch(
                            job.switch,
                            lev,
                            engaged,
                            &stim,
                            job.target_lufs + offset,
                            "assigned",
                        )
                        .inspect(|r| {
                            if save && r.clamp_reason.is_none() {
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
                        }),
                    }
                }
            };
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
            leveller::write_footswitch_values(slot, &writes).map(|()| {
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
