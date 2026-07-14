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
    /// Instrument profile id: when it has a stored Tier-2 DI capture, that WAV is
    /// the stimulus (read VERBATIM, calibration scaling off) and the sound is
    /// diagnosed against the CAPTURE threshold table. `None`/capture-less → the
    /// synthetic topology sample + synthetic threshold table (the pinned default).
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub nodes: Vec<doctor::DoctorNode>,
}

/// The force-bypass isolation list for capturing one sound cleanly (mirrors the
/// leveller's base/footswitch isolation, `footswitch.rs`): Base forces EVERY
/// footswitch on/off block OFF; a footswitch forces its OWN blocks into their
/// switch-active state (isActive-aware — a plain flip of the saved bypass inverts
/// on a preset saved with the switch engaged, HW preset 024's BD2) while forcing
/// every other switch's block off; a scene contributes NOTHING (its own bypass
/// overrides define it). `(group_id, node_id, bypass_to_write)`.
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

/// The previous sound's run outcome, for the consecutive-scene load skip.
/// Only a SUCCESSFUL sound is recorded — after an error the unit may be on any
/// preset, so the loop resets this to `None` (same effect as the run's start).
struct PrevSound {
    list_index: u32,
    /// It sent force-bypass writes (its `fb` was non-empty) — the working copy is
    /// polluted and only a reload clears it (a scene recall re-asserts ONLY the
    /// scene's own overrides, not the forced bypasses).
    wrote: bool,
}

/// Whether the current sound can skip the per-sound preset load connection: the
/// previous sound already loaded the SAME preset, made no working-copy writes, and
/// succeeded — and the current sound is a SCENE (its capture connection recalls the
/// scene anyway, the production scene-leveling mechanism; HW-validated 0.001 LU
/// identical to a fresh reload). Base and footswitch sounds always reload (base
/// needs the base-scene state; footswitch sounds write isolation either way).
fn doctor_skip_load(prev: Option<&PrevSound>, list_index: u32, is_scene: bool) -> bool {
    is_scene
        && prev
            .map(|p| p.list_index == list_index && !p.wrote)
            .unwrap_or(false)
}

/// Should a just-captured sound be treated as a silent-inject floor read (mirrors
/// the leveller's `leveller::floor_suspect` guard, applied to the Doctor's capture
/// spread instead of a leveling measurement)? `Some` carries the honest error to
/// surface; `None` means the capture looks live.
fn floor_error_for(profile_spread_lu: f64, stimulus_spread_lu: f64) -> Option<&'static str> {
    leveller::floor_suspect(profile_spread_lu, stimulus_spread_lu)
        .then_some(leveller::FLOOR_READ_ERR)
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
/// the command's RETURN value, not this channel — the structured per-preset
/// results (incl. scene consistency) are assembled once every sound is measured.
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
    /// Findings for this sound, each tagged with the quietest playback level it
    /// fires at (`doctor::diagnose_levels` — one capture, diagnosed at all three
    /// levels). `fromLevel: "quiet"` = a problem at any volume; `rehearsal`/
    /// `stage` = only appears at that volume and louder.
    pub diags: Vec<doctor::LeveledDiag>,
    pub integrated_lufs: f64,
    pub tail_ratio_db: f64,
    pub balance_db: Vec<f64>,
    /// The display labels of THIS sound's family band layout, in lockstep with
    /// `balance_db` and the `Diag.bands` indices (6 for guitar/bass, 7 for
    /// Bass VI's Sub-first layout). The frontend renders bars/labels from this.
    pub band_labels: Vec<String>,
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
}

/// Cooperative cancel for [`doctor_check`] — stops before the next sound;
/// already-measured sounds keep their results (they're diagnosed and returned).
static DOCTOR_CANCEL: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub(crate) fn cancel_doctor_check() {
    DOCTOR_CANCEL.store(true, SeqCst);
}

/// The Doctor RUN: capture every selected sound (Doctor tail), then diagnose
/// each sound on its OWN measurements (the deterministic tilt-residual metric)
/// and derive per-preset scene consistency from the same captures. READ-ONLY on
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
    // runs on the blocking pool). The `from_capture` bool → the sound's
    // `StimulusKind`: a real Tier-2 DI capture is diagnosed against its own
    // CAPTURE threshold table, a synthetic/topology WAV against the pinned
    // synthetic table.
    let resolved: Vec<(DoctorInput, String, doctor::StimulusKind)> = items
        .into_iter()
        .map(|it| {
            let (path, from_capture) = resolve_stimulus_with_capture(
                &app,
                None,
                it.topology_id.clone(),
                it.profile_id.as_deref(),
            )?;
            let kind = if from_capture {
                doctor::StimulusKind::Capture
            } else {
                doctor::StimulusKind::Synthetic
            };
            Ok((it, path, kind))
        })
        .collect::<Result<_, String>>()?;
    with_released_seize(state.session.clone(), move || {
        // One stimulus decode per (path, calibration) pair — items share them, along
        // with the decoded stimulus's OWN dynamics spread (arms the floor guard below;
        // computed once here rather than per capture).
        let mut stims: std::collections::HashMap<(String, Option<u32>), (Vec<f32>, f64)> =
            std::collections::HashMap::new();
        let mut measured: Vec<(usize, doctor::SoundProfile)> = Vec::new();
        // Per-measured-item band coverage of its stimulus (family layout) — the
        // Doctor skips a rule whose primary band the stimulus never excited.
        let mut coverage_by_item: std::collections::HashMap<usize, Vec<bool>> =
            std::collections::HashMap::new();
        let mut errors: Vec<(usize, String)> = Vec::new();
        // One field-8 preset read per list index, reused across that preset's base
        // + footswitch sounds — the source for each sound's force-bypass isolation.
        let mut preset_cache: std::collections::HashMap<u32, serde_json::Value> =
            std::collections::HashMap::new();
        let mut stopped = false;
        let mut last_scanned: Option<u32> = None;
        // The previous sound's outcome — drives the consecutive-scene load skip
        // (`doctor_skip_load`).
        let mut prev: Option<PrevSound> = None;
        for (i, (item, path, kind)) in resolved.iter().enumerate() {
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
                // No device load happened — the skip decision must not chain off this.
                prev = None;
                continue;
            }
            last_scanned = Some(item.list_index);
            let _ = on_result.send(DoctorProgressItem {
                key: item.key.clone(),
                status: "active".to_string(),
                message: None,
            });
            // A CAPTURE stimulus (real DI) is injected VERBATIM — calibration
            // scaling is None-by-definition (mirrors the leveling seam), so its
            // amplitude drives the chain as recorded. Synthetic keeps its scalar.
            let cal = match kind {
                doctor::StimulusKind::Capture => None,
                doctor::StimulusKind::Synthetic => item.calibration_lufs,
            };
            let stim_key = (path.clone(), cal.map(f32::to_bits));
            let stim = match stims.entry(stim_key) {
                std::collections::hash_map::Entry::Occupied(e) => Ok(&*e.into_mut()),
                std::collections::hash_map::Entry::Vacant(e) => {
                    read_stimulus_calibrated(path, cal).map(|s| {
                        let spread = leveller::stimulus_spread_lu(&s);
                        &*e.insert((s, spread))
                    })
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
            let family = instrument_of(item);
            let skip_load = doctor_skip_load(prev.as_ref(), item.list_index, item.scene.is_some());
            // One capture + profile attempt, `skip_load` threaded through (the retry
            // below forces a fresh preset recall — a floor read means the inject
            // failed, not that the working copy is stale).
            let capture_profile = |skip_load: bool,
                                    stim: &[f32]|
             -> Result<(doctor::SoundProfile, Vec<bool>), String> {
                let (samples, rate) = leveller::doctor_capture(
                    item.list_index,
                    item.scene,
                    &fb,
                    stim,
                    Some(0.5),
                    skip_load,
                )?;
                // Align the body/tail split to where the stimulus actually starts
                // (I/O latency); low confidence keeps the legacy un-aligned split.
                let (onset, confident) = audio::estimate_onset(stim, &samples, rate);
                if !confident {
                    log::warn!(
                        "doctor: onset not confidently found for {} — un-aligned tail split",
                        item.key
                    );
                }
                let profile =
                    doctor::SoundProfile::from_capture(&samples, rate, stim.len(), onset, family)?;
                // The STIMULUS's own band coverage (family layout) — a sparse
                // capture must not fire rules in bands it never excited.
                let cov = doctor::band_coverage(stim, family);
                Ok((profile, cov))
            };
            let result = stim.and_then(|(stim, stim_spread)| {
                let stim_spread = *stim_spread;
                capture_profile(skip_load, stim).and_then(|(profile, cov)| {
                    match floor_error_for(profile.spread_lu, stim_spread) {
                        None => Ok((profile, cov)),
                        Some(err) => {
                            // A silent-inject floor read (the leveller's guard, applied
                            // here to the Doctor's capture): the reading is finite but
                            // stationary. Wait out the quiet gap and force a fresh
                            // preset recall (skip_load=false) — a floor read means the
                            // inject failed, not that the cached working copy is dirty.
                            log::warn!(
                                "doctor_check: floor-suspect capture for {} (spread {:.2} LU ≤ stimulus {:.2} LU) — retrying once",
                                item.key, profile.spread_lu, stim_spread
                            );
                            std::thread::sleep(std::time::Duration::from_millis(
                                leveller::FLOOR_RETRY_GAP_MS,
                            ));
                            let (profile, cov) = capture_profile(false, stim)?;
                            match floor_error_for(profile.spread_lu, stim_spread) {
                                None => Ok((profile, cov)),
                                Some(_) => Err(err.to_string()),
                            }
                        }
                    }
                })
            });
            match result {
                Ok((profile, cov)) => {
                    measured.push((i, profile));
                    coverage_by_item.insert(i, cov);
                    prev = Some(PrevSound {
                        list_index: item.list_index,
                        wrote: !fb.is_empty(),
                    });
                    let _ = on_result.send(DoctorProgressItem {
                        key: item.key.clone(),
                        status: "done".to_string(),
                        message: None,
                    });
                }
                Err(e) => {
                    errors.push((i, e.clone()));
                    // A failed sound may have left the unit on ANY preset (e.g. its
                    // load connection never opened) — the next sound must reload.
                    prev = None;
                    let _ = on_result.send(DoctorProgressItem {
                        key: item.key.clone(),
                        status: "error".to_string(),
                        message: Some(e),
                    });
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
        }

        // Group results per preset, in first-seen item order. Each sound is
        // diagnosed on its OWN measurements (the deterministic tilt-residual
        // metric) — no run-cohort, so a verdict never depends on which other
        // sounds ran.
        let mut presets: Vec<DoctorPresetResult> = Vec::new();
        let sound_of = |i: usize, profile: Option<&doctor::SoundProfile>, err: Option<&String>| {
            let (item, _, kind) = &resolved[i];
            let instrument = instrument_of(item);
            let band_labels = instrument.labels_owned();
            let (diags, lufs_v, tail, bal) = match profile {
                Some(p) => (
                    // Diagnosed at ALL three playback levels (each finding tagged
                    // with its quietest firing level) — the capture is level-
                    // independent, so this is three pure passes over one profile.
                    doctor::diagnose_levels(
                        p,
                        (!item.nodes.is_empty()).then_some(item.nodes.as_slice()),
                        instrument,
                        *kind,
                        coverage_by_item.get(&i).map(Vec::as_slice),
                    ),
                    p.integrated_lufs,
                    p.tail_ratio_db,
                    doctor::balance(&p.bands),
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
                band_labels,
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
                    .find(|(it, _, _)| it.key == base.key)
                    .map(|(it, _, _)| instrument_of(it))
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
        if let Err(e) = Session::connect_lean().and_then(|mut s| {
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
        Ok(DoctorCheckResult { presets, stopped })
    })
    .await
}

/// One prescription's apply job (camelCase wire — the frontend echoes the ops it
/// got from `doctor_check`). `name` is the identity guard (apply refuses if the
/// loaded slot's name differs). Every `DoctorOp` variant (`Param` / `InsertNode`)
/// is applied live; scene-consistency findings are advisory-only and carry no ops.
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

/// Identity of a cached BEFORE clip: sound + stimulus. `name` catches rename/move,
/// the stimulus path + calibration catch an instrument-profile switch between applies.
type BeforeKey = (u32, String, String, Option<u32>);

/// Single-entry cache of `doctor_apply`'s BEFORE clip (the stored preset captured at
/// its own level — ~11 s to produce). The before-state is stable across consecutive
/// applies on the same sound: the frontend allows ONE unsaved prescription at a time
/// and `doctor_discard` reloads the stored preset. Invalidation is
/// correct-by-construction: `Session::{save_current_preset, clear_user_preset,
/// move_user_preset, import_preset}` — the choke points every stored-preset
/// mutation routes through — call [`clear_doctor_before_cache`], as does device
/// detach (`watcher.rs`, an offline unit can be edited elsewhere). Single-entry
/// bounds memory (one WAV).
static BEFORE_CACHE: std::sync::Mutex<Option<(BeforeKey, String)>> = std::sync::Mutex::new(None);

/// Drop the cached `doctor_apply` BEFORE clip — call after anything that changes a
/// stored preset (a save-bearing command) or on device detach.
pub(crate) fn clear_doctor_before_cache() {
    *crate::lock_ok(&BEFORE_CACHE) = None;
}

fn before_cache_get(key: &BeforeKey) -> Option<String> {
    crate::lock_ok(&BEFORE_CACHE)
        .as_ref()
        .filter(|(k, _)| k == key)
        .map(|(_, clip)| clip.clone())
}

fn before_cache_put(key: BeforeKey, clip: String) {
    *crate::lock_ok(&BEFORE_CACHE) = Some((key, clip));
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
    let stim_path = resolve_stimulus(&app, None, job.topology_id.clone())?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, job.calibration_lufs)?;

        // (a) BEFORE: capture the stored preset (reamp off at the end). This LOADS
        //     the slot, so (b) below confirms the already-current preset — no reload.
        //     ref_level None: capture at the preset's OWN level — never write a
        //     reference presetLevel a later doctor_save would PERSIST (#1).
        //     Cached across consecutive applies on the same sound (see BEFORE_CACHE);
        //     a cache hit MUST still load the slot — the load is what (b) relies on
        //     AND what discards a stale unsaved edit buffer (a failed earlier apply
        //     whose restore also failed would otherwise stack Rx on Rx).
        let key: BeforeKey = (
            job.list_index,
            job.name.clone(),
            stim_path.clone(),
            job.calibration_lufs.map(f32::to_bits),
        );
        let before_clip = match before_cache_get(&key) {
            Some(clip) => {
                leveller::restore_saved_preset(job.list_index)?;
                std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
                clip
            }
            None => {
                let (before, rate) =
                    leveller::doctor_capture(job.list_index, None, &[], &stim, None, false)?;
                let clip = format!(
                    "data:audio/wav;base64,{}",
                    base64_encode(&wav_bytes(&before, rate)?)
                );
                before_cache_put(key, clip.clone());
                clip
            }
        };

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
