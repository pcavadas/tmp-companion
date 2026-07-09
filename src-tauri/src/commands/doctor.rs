//! Doctor (tone diagnosis) commands — capture · diagnose · live apply/save/discard.
use crate::*;

// ───────────────────────── Doctor (tone diagnosis) ─────────────────────────

/// One sound to check: a preset's base (`scene: None`) or one scene
/// (`scene: Some(wire index)` — 0-based `scenes[]`, base slot 8 excluded).
/// `nodes` is the preset's chain from the backup scan's graph (may be empty —
/// diagnosis still runs, graph-dependent prescriptions are absent).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorInput {
    pub key: String,
    pub list_index: u32,
    pub scene: Option<u32>,
    /// Block-acting footswitch index (0-based `switch`) when this sound is a
    /// footswitch state; `None` for base/scene sounds.
    pub footswitch: Option<u32>,
    pub label: String,
    pub tag: Option<String>,
    pub topology_id: Option<String>,
    pub calibration_lufs: Option<f32>,
    #[serde(default)]
    pub nodes: Vec<doctor::DoctorNode>,
}

/// The force-bypass isolation list for capturing one sound cleanly (mirrors the
/// leveller's base/footswitch isolation, `footswitch.rs`): Base forces EVERY
/// footswitch on/off block OFF; a footswitch flips its OWN blocks engaged while
/// forcing every other switch's block off; a scene contributes NOTHING (its own
/// bypass overrides define it). `(group_id, node_id, bypass_to_write)`.
fn doctor_force_bypass(
    ftsw: &serde_json::Value,
    preset: &serde_json::Value,
    footswitch: Option<u32>,
) -> Vec<(String, String, bool)> {
    match footswitch {
        Some(s) => {
            let mut out = footswitch::siblings_off_excluding(ftsw, s);
            out.extend(footswitch::engaged_bypass_for_switch(ftsw, preset, s));
            out
        }
        None => footswitch::all_onoff_blocks(ftsw)
            .into_iter()
            .map(|(g, n)| (g, n, true))
            .collect(),
    }
}

/// The instrument a sound is judged as — from its topology, guitar by default.
fn instrument_of(item: &DoctorInput) -> doctor::Instrument {
    doctor::Instrument::from_topology(
        item.topology_id
            .as_deref()
            .and_then(topologies::by_id)
            .map(|t| t.instrument)
            .unwrap_or("guitar"),
    )
}

/// Streamed per-sound progress row (`active` → `done`/`error`). Diagnoses ride
/// the command's RETURN value, not this channel — they're cohort-relative, so
/// they can only be computed once every sound is measured.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorProgressItem {
    pub key: String,
    pub status: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorSoundResult {
    pub key: String,
    pub list_index: u32,
    pub scene: Option<u32>,
    pub footswitch: Option<u32>,
    pub label: String,
    pub tag: Option<String>,
    pub diags: Vec<doctor::Diag>,
    pub integrated_lufs: f64,
    pub tail_ratio_db: f64,
    pub balance_db: Vec<f64>,
    /// Set when this sound's capture failed (no diags then); the run continues.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorPresetResult {
    pub list_index: u32,
    pub sounds: Vec<DoctorSoundResult>,
    pub scene_consistency: Option<doctor::SceneConsistency>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCheckResult {
    pub presets: Vec<DoctorPresetResult>,
    pub stopped: bool,
    /// "median" (≥ MIN_COHORT sounds measured) or "absolute".
    pub cohort: String,
}

/// Cooperative cancel for [`doctor_check`] — stops before the next sound;
/// already-measured sounds keep their results (they're diagnosed and returned).
static DOCTOR_CANCEL: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub(crate) fn cancel_doctor_check() {
    DOCTOR_CANCEL.store(true, SeqCst);
}

/// The Doctor RUN: capture every selected sound (Doctor tail), then diagnose
/// the whole cohort (median-relative when ≥ `doctor::MIN_COHORT` measured) and
/// derive per-preset scene consistency from the same captures. READ-ONLY on
/// the unit: loads + captures, never a save; every capture ends re-amp OFF.
/// One command per run (the `copy_apply`/`level_scenes_apply_batched` shape):
/// per-sound progress streams over `on_result`, structured results return.
/// `restore_list_index` is the pre-run ACTIVE preset (the frontend's
/// `activeListIndex`): the run ends by reloading it — falling back to the
/// last-scanned slot — so the unit is back where the player left it and the
/// 0.5 reference `presetLevel` never lingers in the edit buffer.
#[tauri::command]
pub(crate) async fn doctor_check<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    items: Vec<DoctorInput>,
    restore_list_index: Option<u32>,
    on_result: tauri::ipc::Channel<DoctorProgressItem>,
) -> Result<DoctorCheckResult, String> {
    if items.is_empty() {
        return Err("no sounds selected".to_string());
    }
    DOCTOR_CANCEL.store(false, SeqCst);
    // Resolve stimulus paths up front (needs the app handle; the closure below
    // runs on the blocking pool).
    let resolved: Vec<(DoctorInput, String)> = items
        .into_iter()
        .map(|it| {
            let path = resolve_stimulus(&app, None, it.topology_id.clone())?;
            Ok((it, path))
        })
        .collect::<Result<_, String>>()?;
    with_released_seize(state.session.clone(), move || {
        // One stimulus decode per (path, calibration) pair — items share them.
        let mut stims: std::collections::HashMap<(String, Option<u32>), Vec<f32>> =
            std::collections::HashMap::new();
        let mut measured: Vec<(usize, doctor::SoundProfile)> = Vec::new();
        let mut errors: Vec<(usize, String)> = Vec::new();
        // One field-8 preset read per list index, reused across that preset's base
        // + footswitch sounds — the source for each sound's force-bypass isolation.
        let mut preset_cache: std::collections::HashMap<u32, serde_json::Value> =
            std::collections::HashMap::new();
        let mut stopped = false;
        let mut last_scanned: Option<u32> = None;
        for (i, (item, path)) in resolved.iter().enumerate() {
            if DOCTOR_CANCEL.load(SeqCst) {
                stopped = true;
                break;
            }
            // Marketing-screenshot showcase: the offline fake re-amp is a stimulus
            // passthrough (every sound measures identically → "All clear"), so inject
            // curated profiles instead and let the real `diagnose` engine render cards.
            // Skips the device reads + reconnect sleeps for this item entirely.
            #[cfg(feature = "e2e")]
            if crate::e2e_showcase() {
                measured.push((i, doctor::showcase_profile(item.list_index)));
                let _ = on_result.send(DoctorProgressItem {
                    key: item.key.clone(),
                    status: "done".to_string(),
                    message: None,
                });
                continue;
            }
            last_scanned = Some(item.list_index);
            let _ = on_result.send(DoctorProgressItem {
                key: item.key.clone(),
                status: "active".to_string(),
                message: None,
            });
            let stim_key = (path.clone(), item.calibration_lufs.map(f32::to_bits));
            let stim = match stims.entry(stim_key) {
                std::collections::hash_map::Entry::Occupied(e) => Ok(&*e.into_mut()),
                std::collections::hash_map::Entry::Vacant(e) => {
                    read_stimulus_calibrated(path, item.calibration_lufs).map(|s| &*e.insert(s))
                }
            };
            // Force-bypass isolation for this sound. A scene sound contributes
            // nothing (its own bypass overrides define it), so skip the preset read
            // entirely for it (the optimization guard) — base + footswitch sounds
            // share one cached read per list index. A read hiccup degrades to no
            // isolation (best-effort, like `level_preset`), never fails the run.
            let fb = if item.scene.is_some() {
                Vec::new()
            } else {
                if let std::collections::hash_map::Entry::Vacant(e) =
                    preset_cache.entry(item.list_index)
                {
                    let preset = match read_slot_preset_parsed(item.list_index) {
                        Ok((p, _, _)) => p,
                        Err(err) => {
                            log::warn!(
                                "doctor_check slot={}: isolation preset read failed ({err}), capturing without isolation",
                                item.list_index
                            );
                            serde_json::Value::Null
                        }
                    };
                    e.insert(preset);
                    // The read opened (or tried to open) its own session — gap before
                    // the capture reconnects, else the quick reopen risks the HID
                    // open-lockout (0xe00002c5).
                    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
                }
                let preset = &preset_cache[&item.list_index];
                doctor_force_bypass(
                    preset.get("ftsw").unwrap_or(&serde_json::Value::Null),
                    preset,
                    item.footswitch,
                )
            };
            let result = stim.and_then(|stim| {
                leveller::doctor_capture(item.list_index, item.scene, &fb, stim, Some(0.5)).and_then(
                    |(samples, rate)| {
                        doctor::SoundProfile::from_capture(&samples, rate, stim.len())
                    },
                )
            });
            match result {
                Ok(profile) => {
                    measured.push((i, profile));
                    let _ = on_result.send(DoctorProgressItem {
                        key: item.key.clone(),
                        status: "done".to_string(),
                        message: None,
                    });
                }
                Err(e) => {
                    errors.push((i, e.clone()));
                    let _ = on_result.send(DoctorProgressItem {
                        key: item.key.clone(),
                        status: "error".to_string(),
                        message: Some(e),
                    });
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
        }

        // Cohorts are PER INSTRUMENT — a bass preset judged against a guitar
        // median reads falsely boomy. An under-minimum group gets None
        // (absolute fallback), independently of the other group.
        let by_inst: Vec<(doctor::Instrument, &doctor::SoundProfile)> = measured
            .iter()
            .map(|(i, p)| (instrument_of(&resolved[*i].0), p))
            .collect();
        let (guitar_cohort, bass_cohort) = doctor::cohorts_by_instrument(&by_inst);

        // Group results per preset, in first-seen item order.
        let mut presets: Vec<DoctorPresetResult> = Vec::new();
        let sound_of = |i: usize, profile: Option<&doctor::SoundProfile>, err: Option<&String>| {
            let (item, _) = &resolved[i];
            let instrument = instrument_of(item);
            let cohort = match instrument {
                doctor::Instrument::Guitar => guitar_cohort.as_ref(),
                doctor::Instrument::Bass => bass_cohort.as_ref(),
            };
            let (diags, lufs_v, tail, bal) = match profile {
                Some(p) => (
                    doctor::diagnose(
                        p,
                        (!item.nodes.is_empty()).then_some(item.nodes.as_slice()),
                        instrument,
                        cohort,
                    ),
                    p.integrated_lufs,
                    p.tail_ratio_db,
                    doctor::balance(&p.bands).to_vec(),
                ),
                None => (Vec::new(), 0.0, 0.0, Vec::new()),
            };
            DoctorSoundResult {
                key: item.key.clone(),
                list_index: item.list_index,
                scene: item.scene,
                footswitch: item.footswitch,
                label: item.label.clone(),
                tag: item.tag.clone(),
                diags,
                integrated_lufs: lufs_v,
                tail_ratio_db: tail,
                balance_db: bal,
                error: err.cloned(),
            }
        };
        // Measured sounds first (in run order), then the errored ones — one pass
        // groups both into their presets.
        let all = measured
            .iter()
            .map(|(i, p)| sound_of(*i, Some(p), None))
            .chain(errors.iter().map(|(i, e)| sound_of(*i, None, Some(e))));
        for sound in all {
            match presets
                .iter_mut()
                .find(|p| p.list_index == sound.list_index)
            {
                Some(p) => p.sounds.push(sound),
                None => presets.push(DoctorPresetResult {
                    list_index: sound.list_index,
                    sounds: vec![sound],
                    scene_consistency: None,
                }),
            }
        }
        // Sound consistency per preset — needs the base sound as the reference.
        for p in &mut presets {
            let base = p
                .sounds
                .iter()
                .find(|s| s.scene.is_none() && s.footswitch.is_none() && s.error.is_none());
            // Scene sounds carry Some(wire index); footswitch sounds carry
            // None — both are stomp destinations, so both enter the table.
            let others: Vec<(String, Option<String>, f64, Option<u32>)> = p
                .sounds
                .iter()
                .filter(|s| s.error.is_none() && (s.scene.is_some() || s.footswitch.is_some()))
                .map(|s| (s.label.clone(), s.tag.clone(), s.integrated_lufs, s.scene))
                .collect();
            if let Some(base) = base {
                let instrument = resolved
                    .iter()
                    .find(|(it, _)| it.key == base.key)
                    .map(|(it, _)| instrument_of(it))
                    .unwrap_or(doctor::Instrument::Guitar);
                p.scene_consistency = doctor::scene_consistency(
                    &base.label,
                    base.integrated_lufs,
                    &others,
                    instrument,
                );
            }
        }
        // Belt-and-braces: never leave the unit in re-amp after a run, and put
        // it back on the pre-run active preset (fallback: the last-scanned
        // slot) — the reload also clears the 0.5 reference presetLevel from
        // the edit buffer.
        if let Err(e) = Session::connect().and_then(|mut s| {
            if let Err(re) = s.set_reamp_mode(false) {
                log::warn!("doctor_check: failed to disable re-amp mode after run: {re}");
            }
            match restore_list_index.or(last_scanned) {
                Some(slot) => s.load_preset(slot),
                None => Ok(()),
            }
        }) {
            log::warn!("doctor_check: failed to restore active preset after run: {e}");
        }
        Ok(DoctorCheckResult {
            presets,
            stopped,
            cohort: if guitar_cohort.is_some() || bass_cohort.is_some() {
                "median".to_string()
            } else {
                "absolute".to_string()
            },
        })
    })
    .await
}

/// One prescription's apply job (camelCase wire — the frontend echoes the ops it
/// got from `doctor_check`). `name` is the identity guard (apply refuses if the
/// loaded slot's name differs). Scene-trim ops are rejected here — they route
/// through the scene-leveling command frontend-side.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorApplyJob {
    pub list_index: u32,
    pub name: String,
    pub ops: Vec<doctor::DoctorOp>,
    pub topology_id: Option<String>,
    pub calibration_lufs: Option<f32>,
}

/// Result of a live (unsaved) prescription apply: the before/after audition clips
/// as `data:audio/wav;base64,…` URLs, so the A/B compares the stored state against
/// the applied-but-unsaved edit buffer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorApplyResult {
    pub before_clip: String,
    pub after_clip: String,
}

/// Reject the ops `doctor_apply` can't route live. Only `SceneTrim` is rejected
/// (scene trims go through the scene-leveling command). Pure + device-free so the
/// rejection is unit-testable.
fn doctor_validate_ops(ops: &[doctor::DoctorOp]) -> Result<(), String> {
    if ops
        .iter()
        .any(|op| matches!(op, doctor::DoctorOp::SceneTrim { .. }))
    {
        return Err(
            "scene-trim prescriptions aren't applied here — use scene leveling instead".to_string(),
        );
    }
    Ok(())
}

/// Apply a prescription LIVE onto the edit buffer (never saved) and return the
/// before/after A/B clips. Flow (honoring one-engage-per-connection + set-then-
/// engage): (a) capture the STORED preset — this also loads it, so (b) can
/// `confirm_active` without a second load — (b) apply each op on a fresh session's
/// live edit buffer, restoring the stored preset on ANY failure, and (c) capture
/// the edit buffer WITHOUT reloading (a load would discard the unsaved edit).
/// Persist with `doctor_save`; revert with `doctor_discard`.
#[tauri::command]
pub(crate) async fn doctor_apply<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    job: DoctorApplyJob,
) -> Result<DoctorApplyResult, String> {
    doctor_validate_ops(&job.ops)?;
    let stim_path = resolve_stimulus(&app, None, job.topology_id.clone())?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, job.calibration_lufs)?;

        // (a) BEFORE: capture the stored preset (reamp off at the end). This LOADS
        //     the slot, so (b) below confirms the already-current preset — no reload.
        //     ref_level None: capture at the preset's OWN level — never write a
        //     reference presetLevel a later doctor_save would PERSIST (#1).
        let (before, rate) = leveller::doctor_capture(job.list_index, None, &[], &stim, None)?;
        let before_clip = format!(
            "data:audio/wav;base64,{}",
            base64_encode(&wav_bytes(&before, rate)?)
        );

        // (b) APPLY live onto the edit buffer. NEVER saves. On ANY op failure the
        //     stored preset is reloaded (the partial edit is discarded).
        {
            let mut s = Session::connect()?;
            s.confirm_active(job.list_index, Some(&job.name))?;
            s.begin_live_edit()?;
            for op in &job.ops {
                let outcome: Result<bool, String> = match op {
                    doctor::DoctorOp::Param {
                        group_id,
                        node_id,
                        param,
                        value,
                    } => s
                        .change_parameter(group_id, node_id, param, *value as f32)
                        .map(|_| true),
                    doctor::DoctorOp::InsertNode {
                        group_id,
                        before_fender_id,
                        fender_id,
                        params,
                    } => match s.insert_node(group_id, before_fender_id.as_deref(), fender_id) {
                        Ok(true) => {
                            let mut r = Ok(true);
                            for (p, v) in params {
                                // The fresh node's id == its fender id — Doctor only
                                // inserts models ABSENT from the chain (guaranteed at
                                // GENERATION: comp/EQ/cut inserts only fire when
                                // graph_facts found none), so no collision.
                                if let Err(e) =
                                    s.change_parameter(group_id, fender_id, p, *v as f32)
                                {
                                    r = Err(e);
                                    break;
                                }
                            }
                            r
                        }
                        other => other,
                    },
                    // Rejected up front by doctor_validate_ops.
                    doctor::DoctorOp::SceneTrim { .. } => Ok(true),
                };
                let detail = match outcome {
                    Ok(true) => continue,
                    Ok(false) => "the device rejected the edit".to_string(),
                    Err(e) => e,
                };
                return Err(match leveller::restore_saved_preset(job.list_index) {
                    Ok(()) => {
                        format!("couldn't apply — the preset was restored unchanged: {detail}")
                    }
                    Err(restore_err) => format!(
                        "couldn't apply ({detail}) AND the restore also failed ({restore_err}) — verify the preset on the unit"
                    ),
                });
            }
        }

        // (c) AFTER: capture the live edit buffer WITHOUT reloading. ref_level
        //     None like (a): nothing touched the level between the captures, so
        //     the A/B is inherently level-fair at the preset's own level.
        let (after, rate) = leveller::doctor_capture_current(&stim, None)?;
        let after_clip = format!(
            "data:audio/wav;base64,{}",
            base64_encode(&wav_bytes(&after, rate)?)
        );

        Ok(DoctorApplyResult {
            before_clip,
            after_clip,
        })
    })
    .await
}

/// Persist the applied (still-live) edit buffer to `list_index`. Fresh session,
/// identity-guarded — `confirm_active` never loads, so the edit buffer survives.
#[tauri::command]
pub(crate) async fn doctor_save(
    state: State<'_, AppState>,
    list_index: u32,
    expect_name: String,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?;
        s.confirm_active(list_index, Some(&expect_name))?;
        s.save_current_preset(list_index)?;
        Ok(())
    })
    .await
}

/// Discard the applied edit buffer by reloading the stored preset.
#[tauri::command]
pub(crate) async fn doctor_discard(
    state: State<'_, AppState>,
    list_index: u32,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        leveller::restore_saved_preset(list_index)
    })
    .await
}

#[cfg(test)]
#[path = "doctor_tests.rs"]
mod tests;
