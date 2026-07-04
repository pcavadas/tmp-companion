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
    pub label: String,
    pub tag: Option<String>,
    pub topology_id: Option<String>,
    pub calibration_lufs: Option<f32>,
    #[serde(default)]
    pub nodes: Vec<doctor::DoctorNode>,
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
fn cancel_doctor_check() {
    DOCTOR_CANCEL.store(true, SeqCst);
}

/// The Doctor RUN: capture every selected sound (Doctor tail), then diagnose
/// the whole cohort (median-relative when ≥ `doctor::MIN_COHORT` measured) and
/// derive per-preset scene consistency from the same captures. READ-ONLY on
/// the unit: loads + captures, never a save; every capture ends re-amp OFF.
/// One command per run (the `copy_apply`/`level_scenes_apply_batched` shape):
/// per-sound progress streams over `on_result`, structured results return.
#[tauri::command]
async fn doctor_check(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    items: Vec<DoctorInput>,
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
        let mut stopped = false;
        for (i, (item, path)) in resolved.iter().enumerate() {
            if DOCTOR_CANCEL.load(SeqCst) {
                stopped = true;
                break;
            }
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
            let result = stim.and_then(|stim| {
                leveller::doctor_capture(item.list_index, item.scene, stim, 0.5).and_then(
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

        let cohort = (measured.len() >= doctor::MIN_COHORT).then(|| {
            let refs: Vec<&doctor::SoundProfile> = measured.iter().map(|(_, p)| p).collect();
            doctor::cohort_median(&refs)
        });

        // Group results per preset, in first-seen item order.
        let mut presets: Vec<DoctorPresetResult> = Vec::new();
        let sound_of = |i: usize, profile: Option<&doctor::SoundProfile>, err: Option<&String>| {
            let (item, _) = &resolved[i];
            let instrument = doctor::Instrument::from_topology(
                item.topology_id
                    .as_deref()
                    .and_then(topologies::by_id)
                    .map(|t| t.instrument)
                    .unwrap_or("guitar"),
            );
            let (diags, lufs_v, tail, bal) = match profile {
                Some(p) => (
                    doctor::diagnose(
                        p,
                        (!item.nodes.is_empty()).then_some(item.nodes.as_slice()),
                        instrument,
                        cohort.as_ref(),
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
        // Scene consistency per preset — needs the base sound as the reference.
        for p in &mut presets {
            let base = p
                .sounds
                .iter()
                .find(|s| s.scene.is_none() && s.error.is_none());
            let scenes: Vec<(String, Option<String>, f64, u32)> = p
                .sounds
                .iter()
                .filter(|s| s.error.is_none())
                .filter_map(|s| {
                    s.scene
                        .map(|sc| (s.label.clone(), s.tag.clone(), s.integrated_lufs, sc))
                })
                .collect();
            if let Some(base) = base {
                let instrument = doctor::Instrument::from_topology(
                    resolved
                        .iter()
                        .find(|(it, _)| it.key == base.key)
                        .and_then(|(it, _)| it.topology_id.as_deref())
                        .and_then(topologies::by_id)
                        .map(|t| t.instrument)
                        .unwrap_or("guitar"),
                );
                p.scene_consistency = doctor::scene_consistency(
                    &base.label,
                    base.integrated_lufs,
                    &scenes,
                    instrument,
                );
            }
        }
        // Belt-and-braces: never leave the unit in re-amp after a run.
        let _ = Session::connect().and_then(|mut s| s.set_reamp_mode(false).map(|_| ()));
        Ok(DoctorCheckResult {
            presets,
            stopped,
            cohort: if cohort.is_some() {
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
async fn doctor_apply(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    job: DoctorApplyJob,
) -> Result<DoctorApplyResult, String> {
    doctor_validate_ops(&job.ops)?;
    let stim_path = resolve_stimulus(&app, None, job.topology_id.clone())?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, job.calibration_lufs)?;

        // (a) BEFORE: capture the stored preset (reamp off at the end). This LOADS
        //     the slot, so (b) below confirms the already-current preset — no reload.
        let (before, rate) = leveller::doctor_capture(job.list_index, None, &stim, 0.5)?;
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
                                // inserts models ABSENT from the chain, so no collision.
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

        // (c) AFTER: capture the live edit buffer WITHOUT reloading, at the SAME
        //     reference level the before-capture used (level-fair A/B).
        let (after, rate) = leveller::doctor_capture_current(&stim, 0.5)?;
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
async fn doctor_save(
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
async fn doctor_discard(state: State<'_, AppState>, list_index: u32) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        leveller::restore_saved_preset(list_index)
    })
    .await
}

#[cfg(test)]
#[path = "doctor_tests.rs"]
mod tests;
