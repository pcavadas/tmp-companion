//! The scene lane: scene jobs and their prepass ([`prepass_scene_ceilings`]), the one-shot
//! joint-k solve, the user-handle solve, the bounded correction loop, and the batch runner
//! ([`level_scenes_oneshot`]) with its deferred save.

use super::*;

/// Per-scene outcome of [`level_scenes_live_batched`].
#[derive(Debug, Clone, Serialize, Default)]
pub struct BatchedSceneOutcome {
    pub scene_slot: u32,
    /// The effective (offset-adjusted) loudness target this scene was leveled to.
    /// Per-scene because one batch can carry a mix of targets; `outcome_to_level_result`
    /// reads it here rather than zipping outcomes against jobs by index (the failure
    /// filter misaligns positional zips).
    pub target_lufs: f64,
    pub final_lufs: Option<f64>,
    pub final_level: Option<f32>,
    pub clamped: bool,
    pub windows: u32,
    pub writes: u32,
    pub elapsed_ms: u128,
    /// Why this scene was not leveled (its reason text, for the log and bug report); `None`
    /// for a leveled scene.
    pub failure: Option<String>,
    /// The typed kind of `failure` ([`SkipKind`]) — what the frontend chooses the row's
    /// message by. Set exactly when `failure` is.
    pub failure_kind: Option<SkipKind>,
    /// Dynamics spread of the scene's measure capture (LU); `None` where the
    /// measuring path has no full-capture meter (the live-window runner) or the
    /// scene failed. See `LevelResult::dynamic_spread_lu`.
    pub dynamic_spread_lu: Option<f64>,
    /// Set with `clamped` when the scene clamped for a SPECIFIC reason the UI should
    /// show verbatim — currently "no authority": a big `outputLevel` change moved the
    /// USB 1/2 capture by ~nothing, so the amp is off-branch / off-USB (or hard-limited).
    /// `None` for an ordinary headroom clamp.
    pub clamp_reason: Option<String>,
    /// The clamp's CAUSE from the shared taxonomy ([`crate::headroom_trade::ClampKind`]);
    /// `None` when the scene is not clamped. Additive next to `clamp_reason` — see
    /// [`LevelResult::clamp_kind`].
    pub clamp_kind: Option<crate::headroom_trade::ClampKind>,
    /// Best-effort "verify by ear" flag from the rebalance flow: the lane-mute floor was
    /// close enough to a solo lane that bleed may have skewed the equal-solo balance (the
    /// overall target is still hit). `false` outside rebalance.
    pub verify_by_ear: bool,
    /// Post-save param-level verify (`verify_persisted_writes`): `Some(true)` = the saved
    /// preset does NOT hold the value this outcome reports, so the number above is
    /// pre-wipe and must not be trusted; `Some(false)` = re-read and confirmed. `None` =
    /// not checked (the scene wrote nothing, the run didn't save, or the re-read failed).
    pub persist_mismatch: Option<bool>,
    /// Whether `final_lufs` was measured after the write (see [`Verification`]).
    pub verification: Verification,
}

/// One amp knob to drive within a scene: the control, its bounds, and its current
/// value in THAT scene (from the pre-pass doc, so the first jump starts from truth).
#[derive(Debug, Clone)]
pub struct KnobTarget {
    pub knob: LevelKnob,
    pub lo: f32,
    pub hi: f32,
    pub current: f32,
}

/// A pre-resolved per-scene leveling job. A scene carries a **set** of amp knobs:
/// one for a series chain (the last active amp) and the split-output single-lane
/// case, but TWO+ for a parallel-merged scene where each lane has its own amp — those
/// are driven together by one factor `k` (joint-k), since scaling every amp in a sum
/// by the same `k` shifts the captured loudness by exactly `20·log10(k)` regardless of
/// inter-lane correlation. The probe-only bench runner (`level_scenes_live_batched`)
/// requires `knobs.len() == 1` (via `solo()`) and errors otherwise.
#[derive(Debug, Clone)]
pub struct SceneJob {
    pub scene_slot: u32,
    /// This scene's own (offset-adjusted) loudness target — the SINGLE source of truth.
    /// `build_scene_jobs` stamps it on every job; the app command overrides it per wire
    /// job so a mixed-target preset levels in ONE batch. The runners read it directly.
    pub target_lufs: f64,
    pub knobs: Vec<KnobTarget>,
    /// When `Some`, this scene can't be safely leveled (mic/split/no-active-amp/etc.);
    /// the runner reports it as a skipped (failed) outcome — with the refusal's kind — and
    /// moves on, never aborting the whole run. `knobs` is empty in that case.
    pub skip: Option<Refusal>,
    /// True only for a parallel scene whose lanes RE-MERGE (≥2 knobs feeding one summed
    /// output) — the rebalance flow may adjust the lanes' mix. False for series, single
    /// amp, and split-OUTPUT scenes (separate physical outs have no shared mix).
    pub rebalanceable: bool,
    /// The USER'S OWN leveling handle for this scene, when they chose one: the CLASSIFIED
    /// block param `knobs[0]` addresses. `None` = the amp-`outputLevel` joint-k path (every
    /// existing caller), which keeps joint-k, the amplitude-bounds requirement, and the
    /// rebalance flow. `Some` switches the scene onto the generic param-space secant
    /// ([`solve_footswitch`]'s loop) with THIS param's class, range and wet floor — and
    /// `knobs` then holds exactly one [`KnobTarget`] for it, so the persist-verify and
    /// deferred-save machinery need no special case.
    pub handle: Option<FsParamTarget>,
    /// This scene's PREPASS reading, when the run measured every sound's ceiling BEFORE any
    /// write (the reordered run — see [`prepass_scene_ceilings`]). `Some` makes
    /// [`scene_prologue`] consume the reading instead of taking its own capture, so the
    /// per-scene solve costs exactly what it always did and the batch simply pays its
    /// as-is captures up front. `None` (every legacy caller: the rebalance flow, the
    /// redistribution runner, the probe benches) keeps the measure-inside-the-solve order
    /// byte-for-byte.
    pub prepass: Option<ScenePrepass>,
    /// Isolation bypass writes `(group_id, node_id, forced_bypass)` this job's every capture
    /// and write must re-assert — "base means base": preset 28's TubeScreamer is saved ON, so
    /// an un-isolated base capture would measure it and silently under-isolate. Empty for every
    /// scene job (a scene rides its own overlay). Non-empty only for a BASE job
    /// (`scene_slot == session::BASE_SCENE_SLOT`), derived once by the app command
    /// (`doctor_force_bypass`) and threaded through `measure_scene_asis`/`apply_levels`/
    /// `apply_first_verified`/`correct_iter`'s trailing `force_bypass` parameter at every site
    /// this job reaches one. The device working copy the isolation writes land in must be
    /// clean again before the batch's terminal save — see `danger.md`'s PHASE-1/trade-hold
    /// cleanup choreography; this field only carries WHAT to force, not when to undo it.
    pub force_bypass: Vec<(String, String, bool)>,
}

/// One scene's PREPASS measurement: the reading every solve used to take as its own first
/// step, hoisted OUT of the solve so a batch can know every sound's ceiling before it writes
/// anything.
///
/// WHY THE ORDER MATTERS. The benefit-aware headroom trade
/// ([`crate::headroom_trade::plan_headroom_trade`]) has to compare EVERY sound's ceiling
/// against EVERY sound's target before it decides whether to move the base
/// `presetLevel`/`outputLevel` pair — a decision that changes what every later sound must be
/// solved to. Measuring inside each solve makes that decision impossible without either
/// re-measuring (double the captures) or guessing.
///
/// It carries no more than the old inline capture produced (`asis` + `spread`), so nothing
/// downstream had to change shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScenePrepass {
    /// AS-IS loudness at the scene's authored knob values (LUFS).
    pub asis: f64,
    /// Dynamics spread (LU) of that same capture.
    pub spread: f64,
}

/// THE REORDERED RUN'S PREPASS: measure every scene job's as-is reading BEFORE the batch
/// writes anything, stamping each one on its job ([`SceneJob::prepass`]) so the solve that
/// follows consumes it instead of taking its own capture. Total captures are UNCHANGED — the
/// same one-engage-per-scene as-is reading each solve always opened with, simply paid up
/// front.
///
/// WHY: [`crate::headroom_trade::plan_headroom_trade`] must see every sound's ceiling next to
/// every sound's target before it decides whether to move the base
/// `presetLevel`/`outputLevel` pair, because that decision changes what every LATER sound is
/// solved to. Measuring inside each solve makes the decision unavailable at the only moment
/// it can be acted on.
///
/// CALLER CONTRACT: the preset is already current (the caller ran `prepass_scene_docs`), and
/// this writes NOTHING — every reading is a pure `measure_scene_asis` (scene recall, engage,
/// capture, disengage), so there is no measurement dirt for the batch's deferred save to pick
/// up and no reload is owed before the write phase.
///
/// A row whose measurement FAILS is left with `prepass: None` and logged: its solve then
/// takes the capture itself and reports the failure through the batch's one reporting path
/// (`run_scene_jobs`), rather than inventing a second one here. A skip job is never measured.
///
/// Ends on a guaranteed fresh re-amp OFF — each capture already disengages, but an
/// interrupted one can strand the unit input-muted (`danger.md`).
///
/// `intended_preset_level` is the run's OWN `presetLevel` re-asserted on every reading (see
/// [`measure_scene_asis`]). The batched scene command passes the preset's own SAVED level —
/// not the headroom trade's raise, which is decided AFTER this prepass. It must match what
/// the solve captures render at: `correct_iter` takes a prepass reading as its `measured0`
/// and compares it to a post-apply capture, so two different renderings make the first
/// "response" include the level difference and can defeat the `no_authority` verdict outright.
/// The progress `message` a lane sends while a CEILING PREPASS capture is running.
///
/// A progress message on an ACTIVE row is that row's caption, and the wizard renders it two
/// ways depending on whether a capture is streaming (`RunBody`'s `rowStatus`):
///
/// - a capture IS streaming -> the caption is the VERB before the live number, `measuring · -18.9`
/// - nothing is streaming -> the caption is a NOTE, rendered verbatim
///
/// So send THIS one only from inside a measurement loop, wrapped around a `measure_*` call.
/// A message sent outside a capture is a note by construction and must read as a sentence.
pub const PREPASS_ACTIVE_MSG: &str = "measuring";

pub fn prepass_scene_ceilings(
    jobs: &mut [SceneJob],
    stimulus: &[f32],
    intended_preset_level: Option<f32>,
    mut on_scene: impl FnMut(u32),
    mut cancelled: impl FnMut() -> bool,
) -> Result<(), LevelError> {
    let mut stopped = false;
    for job in jobs.iter_mut() {
        if cancelled() {
            stopped = true;
            break;
        }
        if job.skip.is_some() {
            continue;
        }
        on_scene(job.scene_slot);
        match require_live(
            || {
                measure_scene_asis(
                    job.scene_slot,
                    stimulus,
                    intended_preset_level,
                    &job.force_bypass,
                )
            },
            stimulus,
        ) {
            Ok(l) => {
                job.prepass = Some(ScenePrepass {
                    asis: l.integrated_lufs,
                    spread: l.spread_lu(),
                });
                // Mirror the `fs prepass` line so a clamped SCENE row leaves the same
                // explainable trail its footswitch sibling already does. The absence of this
                // line is not evidence the prepass measured nothing — it was twice read that
                // way during this investigation.
                if let Some(ceiling) = scene_ceiling_lufs(job) {
                    log::info!(
                        "scene prepass scene={} ceiling={ceiling:.2} LUFS target={:.2} \
                         asis={:.2} spread={:.2} LU",
                        job.scene_slot,
                        job.target_lufs,
                        l.integrated_lufs,
                        l.spread_lu()
                    );
                }
            }
            Err(e) if e.is_cancelled() => {
                stopped = true;
                break;
            }
            // A SILENT scene (no signal on USB 1/2 — e.g. an amp saved at `outputLevel` 0,
            // deep digital silence on the unit) has no ceiling to give the trade, so it is
            // left unmeasured like any failed row: its solve re-captures, and a second silent
            // read becomes the routing clamp there (`scene_prologue_or_silent`) — one report
            // path, and a transiently silent inject gets a second chance before the verdict.
            Err(e) if e.is_no_signal() => log::info!(
                "prepass ceiling for scene {}: silent capture (no signal on USB 1/2); its \
                 solve will re-measure and report the routing clamp if it stays silent",
                job.scene_slot
            ),
            Err(e) => log::warn!(
                "prepass ceiling for scene {} failed ({e}); its solve will re-measure",
                job.scene_slot
            ),
        }
    }
    reamp_off_guaranteed("prepass_scene_ceilings");
    if stopped {
        return Err(LevelError::Cancelled);
    }
    Ok(())
}

/// The maximum loudness this scene can reach AT THE CURRENT base `presetLevel` — the ceiling
/// the headroom trade compares against the row's target.
///
/// ONLY the amp-`outputLevel` (joint-k) lane can answer. That control is LINEAR IN dB with
/// full authority (HW, ~25 LU range), so the top of its range is an exact extrapolation from
/// the as-is reading: `asis + 20·log10(LEVEL_MAX / max_i current_i)` — the same ratio-
/// preserving `k_cap` [`solve_joint_k_at`] clamps to, so the two can never disagree about
/// where the top is.
///
/// A USER-HANDLE row answers `None` ON PURPOSE. An arbitrary block param (a taper `volume`, a
/// raw-dB `gain`, a wet `mix`) has no algebraically predictable response (`headroom_trade`
/// module header), so extrapolating one would be exactly the taper model this codebase refuses
/// to build. Such
/// a row's clamp is discovered by its own bounded secant and reported honestly; it simply
/// does not get a vote in the trade.
///
/// `None` also for a skip job, a knob-less job, and a job with no prepass reading.
pub fn scene_ceiling_lufs(job: &SceneJob) -> Option<f64> {
    if job.skip.is_some() || job.handle.is_some() {
        return None;
    }
    let asis = job.prepass?.asis;
    let max_cur = job
        .knobs
        .iter()
        .map(|kt| kt.current as f64)
        .fold(0.0f64, f64::max);
    if max_cur <= 0.0 {
        return None;
    }
    Some(asis + 20.0 * (LEVEL_MAX as f64 / max_cur).log10())
}

/// ONE-SHOT open-loop per-scene leveling — the validated replacement for
/// [`level_scenes_live_batched`] (HW). The active amp's `outputLevel` is
/// LINEAR in dB (`captured_LUFS = 20·log10(outputLevel) + C`, ~25 LU authority), so —
/// exactly like `presetLevel` — there is no need for a closed loop: measure ONCE at a
/// reference level via an ISOLATED fresh re-amp capture, solve `C`, set the exact
/// level. The BatchedLive runner's shared continuous stream MIS-MEASURED scenes
/// (returning impossible loudness, e.g. -6.96 LUFS on a knob whose true range is
/// -40..-14), which made the trust-region loop clamp on garbage; the isolated
/// measurement (`measure_knob_at`) reads correctly. Same signature + outcome shape as
/// `level_scenes_live_batched` so the command path is a drop-in swap. Per-scene
/// isolation rides `set_knob`'s Scene Edit; `presetLevel` (the Base) must be leveled
/// FIRST (it is a global multiplier over every scene). With `save`, every scene's
/// write accumulates UNSAVED in the working copy and ONE `saveCurrentPreset` at
/// batch end persists them all (`save_deferred_scene_writes` — also fired on
/// cancel, so already-reported scenes are never silently lost); `restore_scene`
/// is recalled first so the save stamps the preset's original active scene. A
/// per-scene failure becomes a failed outcome, never aborting the run.
///
/// ONE per-row variation rides this runner, defaulted OFF so an existing caller's batch is
/// byte-identical: [`SceneJob::handle`] — the user named their OWN control for that scene, so
/// it is solved by [`handle_one_scene`] (the generic param secant) instead of the amp
/// joint-k. The amp `outputLevel` remains the ONLY control this lane touches when no handle
/// is given; a handle is an explicit, per-row user choice.
/// THE LEVEL EVERY CAPTURE IN A SCENE BATCH MUST RENDER AT. Each per-scene capture recalls
/// its scene first, and the recall runs the device's own level-apply — so a capture renders
/// at whatever level that apply serves, not at the one this run means.
///
/// Two ways that diverges, and BOTH need the same re-assert:
///  · a landed headroom trade holds a RAISED `presetLevel` UNSAVED in the working copy until
///    this batch's one save — without the re-assert every scene is solved against the
///    pre-raise sound;
///  · with no trade, the recall serves the SAVED level; asserting it is defensive on fw
///    1.8.58 (fw 1.8.45 served a pre-save level for a while after a save: HW 2026-08-19, a
///    whole footswitch batch measured 5.53 dB quiet).
///
/// The trade's unsaved raise wins when there is one; otherwise the preset's own SAVED level,
/// read from the complete field-8 doc the caller already holds (the fresher of the device's
/// two stores). Rendering is then independent of commit timing.
///
/// COUPLED WITH THE PREPASS. `prepass_scene_ceilings` must be given the SAME level, or the
/// as-is reading it produces and the post-apply captures compared against it render
/// differently and the difference is read as knob response — see `commands/level_scenes.rs`.
/// Split out as a named function so this decision is unit-gated: it is not otherwise
/// observable offline, because SimDevice only reverts a recall's level for a slot this run
/// has already saved.
pub(crate) fn scene_capture_level(
    hold: Option<&TradeHold>,
    saved: Option<&serde_json::Value>,
) -> Option<f32> {
    hold.map(|h| h.preset_level).or_else(|| {
        saved
            .and_then(crate::audiograph::preset_level)
            .map(|v| v as f32)
    })
}

#[allow(clippy::too_many_arguments)]
pub fn level_scenes_oneshot(
    slot: ListIndex,
    jobs: &[SceneJob],
    stimulus: &[f32],
    save: bool,
    restore_scene: Option<u32>,
    saved: Option<&serde_json::Value>,
    hold: Option<&TradeHold>,
    // A5/F2: `(group, node, ORIGINAL saved bypass)` for a base-requested job's own isolation
    // (empty on every run that never isolated anything) — see `run_scene_jobs`'s param doc.
    isolation_restore: &[(String, String, bool)],
    on_scene: impl FnMut(u32, Option<&BatchedSceneOutcome>),
    // B6: forwarded to `run_scene_jobs` verbatim — see its own doc.
    on_tail: impl FnMut(&str),
    cancelled: impl FnMut() -> bool,
) -> Result<Vec<BatchedSceneOutcome>, LevelError> {
    let intended_preset_level = scene_capture_level(hold, saved);
    run_scene_jobs(
        slot,
        jobs,
        save,
        restore_scene,
        saved,
        hold,
        isolation_restore,
        on_scene,
        on_tail,
        cancelled,
        move |job: &SceneJob| match &job.handle {
            Some(handle) => handle_one_scene(
                slot,
                job,
                handle,
                stimulus,
                job.target_lufs,
                save,
                saved,
                intended_preset_level,
                asis_reuse_batch_ok(jobs, job, hold),
            ),
            None => jointk_one_scene(
                slot,
                job,
                stimulus,
                job.target_lufs,
                save,
                true,
                saved,
                // A scene amp may legitimately be solved all the way to silence; only
                // the trade's BASE hold carries the fader floor.
                LEVEL_MIN,
                intended_preset_level,
            ),
        },
    )
}

/// Write every knob of `job` back to its pre-run value (`KnobTarget::current`), unsaved and
/// unverified — the deferred batch save then persists the scene as the player left it. Used
/// when a scene's solve fails or is stopped part-way, since its measure helpers write a
/// probe value before each capture.
fn rollback_scene_knobs(
    slot: ListIndex,
    job: &SceneJob,
    saved: Option<&serde_json::Value>,
) -> Result<(), LevelError> {
    let targets: Vec<(&LevelKnob, f32)> = job.knobs.iter().map(|k| (&k.knob, k.current)).collect();
    let opts = LevelOptions {
        verify: false,
        defer: true,
        ..Default::default()
    };
    apply_levels(slot, &[], &targets, opts, false, saved, &[]).map(|_| ())
}

/// The ONE scene-batch scaffold shared by [`level_scenes_oneshot`] and
/// [`level_scenes_rebalance`] — only the per-job `solve` differs. Owning the loop in
/// one place matters beyond dedup: the loop has a SINGLE EXIT so the deferred-save
/// guard (persist accumulated unsaved writes on EVERY exit, incl. cancel — the
/// silent-data-loss class this design exists to prevent) exists exactly once.
///
/// CALLER CONTRACT: the preset must already be current — every caller runs
/// `prepass_scene_docs` (which loads it) right before this. Re-loading here was
/// pure churn the user SAW: the unit flashing back to the preset (base scene)
/// between the prepass and the first scene measure, once per dispatched scene.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_scene_jobs(
    slot: ListIndex,
    jobs: &[SceneJob],
    save: bool,
    restore_scene: Option<u32>,
    // The run's ONE pre-run field-8 read — the same doc every solve's `set_knobs` decided its
    // write landing from, so the post-save readers look for each value where it landed
    // (`write_overlay`).
    saved: Option<&serde_json::Value>,
    // `hold`: the UNSAVED base pair a headroom trade raised, if one ran. The batch's ONE save
    // recalls the preset's original scene first, and that recall runs the device's own
    // level-apply — silently reverting an unsaved `presetLevel` right before the save persists
    // it (HW). Re-asserting it is what makes the trade's two halves land TOGETHER; `None`
    // (every untraded run) is byte-identical to the previous behaviour.
    hold: Option<&TradeHold>,
    // A5/F2 (TRADE-PATH GAP): when the BASE job itself is one of `jobs` (base_requested — not
    // stripped as anchor-only upstream), its own solve routes through `jointk_one_scene` →
    // `apply_levels(defer: true)`, which re-asserts `job.force_bypass` on EVERY capture and
    // never undoes it — that seam exists to keep the isolation alive across a multi-capture
    // secant, not to clean it up, so a trade's own `undo_base_isolation` (which ran BEFORE this
    // job even solved) is left stale the moment base solves again. `(group, node, ORIGINAL
    // saved bypass)`, same derivation `undo_base_isolation` uses — empty on every run that
    // never isolated anything.
    isolation_restore: &[(String, String, bool)],
    mut on_scene: impl FnMut(u32, Option<&BatchedSceneOutcome>),
    // A batch-wide caption for the two tail phases that have no single scene to report
    // progress against — "Saving preset…" and "Verifying…". `|_| {}` for callers that want
    // no captions.
    mut on_tail: impl FnMut(&str),
    mut cancelled: impl FnMut() -> bool,
    mut solve: impl FnMut(&SceneJob) -> Result<SceneSolve, LevelError>,
) -> Result<Vec<BatchedSceneOutcome>, LevelError> {
    let reasserts = Reassert::preset_level_only(hold.map(|h| h.preset_level));
    let mut outcomes = Vec::with_capacity(jobs.len());
    let mut attempted = false;
    let mut stopped = false;
    // Every value this batch actually wrote, for the post-save re-read (`verify_persisted_writes`).
    // SEEDED with the trade's own base pair when one landed: those `outputLevel` writes are
    // just as unsaved and just as load-bearing as the scene overlays, so the post-save re-read
    // has to cover them too.
    let mut written: Vec<PersistedWrite> = hold.map(|h| h.writes.clone()).unwrap_or_default();
    // A5/F2 DETECTION: the union of both isolation-restore sources — the trade hold's own
    // expectations (see `TradeHold::force_bypass_restore`'s doc) AND this run's own
    // `isolation_restore` (the base-requested path, cleaned up below).
    let force_bypass_restore = union_isolation_restore(
        hold.map(|h| h.force_bypass_restore.as_slice())
            .unwrap_or(&[]),
        isolation_restore,
    );
    // Every writing scene verifies + self-corrects (see `jointk_one_scene`): a downstream
    // compressor undershoots the open-loop solve per scene, so the canary-only model isn't
    // enough. Cost is one verify capture per off-target scene (none when already at target).
    for job in jobs {
        if cancelled() {
            stopped = true;
            break;
        }
        on_scene(job.scene_slot, None);
        let t0 = std::time::Instant::now();

        // A skip job (unclassifiable scene: mic/split lane/no active amp/…) is reported
        // as a failed outcome and the run continues — never aborts the whole pass.
        if let Some(reason) = &job.skip {
            // The reason reaches the wizard over the progress Channel, and the batch FILTERS
            // skipped rows out of the array it returns — so without this line a skipped scene
            // leaves no trace a later reader can find, and a preset that levels 2 of its 4
            // scenes looks like a clean run in the log. HW (2026-09-01, real "Friedman HBE"):
            // two of four scenes skipped and the log recorded only the two that ran.
            log::warn!("scene {} SKIPPED (not leveled): {reason}", job.scene_slot);
            let outcome = failed_scene_outcome(
                job.scene_slot,
                job.target_lufs,
                reason.clone(),
                t0.elapsed().as_millis(),
            );
            on_scene(job.scene_slot, Some(&outcome));
            outcomes.push(outcome);
            continue;
        }

        attempted = true;
        let result = solve(job);

        // A failed or stopped solve can leave a probe value (or a rebalance solo mute) in
        // this scene's knobs: the measure helpers write before they capture, and the batch
        // save below persists the whole working copy. Put every knob back to its pre-run
        // value first. If that rollback fails, abandon the batch exactly like a failed
        // isolation cleanup below: a lost batch beats saving a value the run never chose.
        if let Err(solve_err) = &result {
            if save && !job.knobs.is_empty() {
                if let Err(e) = rollback_scene_knobs(slot, job, saved) {
                    if let Err(e2) = restore_saved_preset(slot) {
                        log::warn!(
                            "restore_saved_preset failed backing the batch out after a failed \
                             scene {} rollback (slot {slot}): {e2}",
                            job.scene_slot
                        );
                    }
                    reamp_off_fresh("run_scene_jobs rollback");
                    let msg = format!(
                        "scene {} failed ({solve_err}) and its knobs could not be put back \
                         ({e}) — abandoning this batch's unsaved writes rather than risk \
                         saving a probe value",
                        job.scene_slot
                    );
                    return Err(if solve_err.is_cancelled() || cancelled() {
                        LevelError::Cancelled.with_restore(Err(msg))
                    } else {
                        LevelError::Device(msg)
                    });
                }
            }
        }

        let outcome = match result {
            Ok(s) => {
                // Harvested BEFORE `solved_scene_outcome` consumes the solve: the outcome keeps
                // only the loudest lane's level, but every lane amp was written and each one has
                // to be re-read. `writes == 0` wrote nothing, so there is nothing to confirm.
                if s.writes > 0 {
                    written.extend(job.knobs.iter().zip(&s.levels).filter_map(|(k, &v)| {
                        match &k.knob {
                            LevelKnob::Block {
                                node_id,
                                parameter_id,
                                ..
                            } => Some(PersistedWrite {
                                scene_slot: job.scene_slot,
                                overlay: write_overlay(
                                    saved,
                                    job.scene_slot,
                                    node_id,
                                    parameter_id,
                                ),
                                node_id: node_id.clone(),
                                parameter_id: parameter_id.clone(),
                                value: v,
                            }),
                            // Not written by a scene job, and not scene-scoped if it were.
                            LevelKnob::PresetLevel => None,
                        }
                    }));
                }
                solved_scene_outcome(job.scene_slot, job.target_lufs, s, t0.elapsed().as_millis())
            }
            Err(e) if e.is_cancelled() => {
                stopped = true;
                break;
            }
            Err(e) => {
                log::warn!("scene {} failed: {e}", job.scene_slot);
                failed_scene_outcome(
                    job.scene_slot,
                    job.target_lufs,
                    e.refusal(),
                    t0.elapsed().as_millis(),
                )
            }
        };
        on_scene(job.scene_slot, Some(&outcome));
        outcomes.push(outcome);
    }
    // Guaranteed fresh re-amp OFF (each `measure_knob_at`/`apply_level` already
    // disengages, but an interrupted capture can strand it).
    reamp_off_fresh("run_scene_jobs");
    // A5/F2 (TRADE-PATH GAP): undo the base-requested isolation HERE, straight after the last
    // capture and BEFORE the one save below — this is the only place left that can, since every
    // capture in the loop above re-asserted `force_bypass` and dropped its own session without
    // ever clearing it (see the param doc). FAILURE here is not survivable: the working copy
    // still carries every isolated pedal forced off, and this batch's OWN deferred writes are
    // just as unsaved as that isolation — so a failed cleanup abandons the whole batch (reload,
    // exactly like `apply_headroom_trade`'s own back-out) rather than let the save below persist
    // a pedal in a state the player never authored. A lost batch beats a corrupted save.
    if attempted && !isolation_restore.is_empty() {
        if let Err(e) = write_isolation_restore(isolation_restore) {
            if let Err(e2) = restore_saved_preset(slot) {
                log::warn!(
                    "restore_saved_preset failed backing the batch out after a failed base \
                     isolation cleanup (slot {slot}): {e2}"
                );
            }
            let msg = format!(
                "the base isolation could not be undone before the save ({e}) — abandoning \
                 this batch's unsaved writes rather than risk saving a pedal forced off"
            );
            // A stopped run stays a cancel, with the failed cleanup attached.
            return Err(if stopped {
                LevelError::Cancelled.with_restore(Err(msg))
            } else {
                LevelError::Device(msg)
            });
        }
    }
    // Did the run PERSIST a headroom trade? Only then is there anything to disclose on a
    // cancel: with nothing attempted the save below never fires and the reload two blocks down
    // discards the pair whole, so reporting a landed trade there would be a lie.
    let trade_persisted = save && attempted && hold.is_some();
    // The batch's ONE persist — after the re-amp OFF, on its own clean connection.
    // Fired on the stopped path too, so already-reported scenes are never lost. Its verdict
    // is REPORTED on every path, the stopped one included: a save that fails (its retry
    // too) marks every written scene `persist_mismatch`, and a save that lands is re-read
    // so a value it did not keep is marked the same way. Nothing here sends a load: the
    // save is the first device command after the re-amp OFF, as it always was.
    if save && attempted {
        on_tail("Saving preset…");
        let saved_result = save_deferred_scene_writes(slot, restore_scene, &reasserts);
        report_batch_save(
            slot,
            stopped,
            saved_result,
            &written,
            &mut outcomes,
            |outcomes| {
                on_tail("Verifying…");
                verify_persisted_writes(slot, &written, &force_bypass_restore, outcomes);
            },
        );
    }
    // ⟦3b⟧ CANCELLED BEFORE THE FIRST SOLVE. Nothing was deferred, so `save && attempted` above
    // is false and NO `save_deferred_scene_writes` ran — which makes this reload safe under
    // danger.md's "never a load in front of the deferred save": there are no deferred writes
    // to wipe. What there CAN be is a landed headroom trade's raised `presetLevel` + solved
    // base fader sitting dirty in the working copy with nothing to pay for them; the reload
    // discards the pair whole, exactly as `apply_headroom_trade`'s own back-out does.
    if stopped && !attempted {
        if let Err(e) = restore_saved_preset(slot) {
            log::warn!("restore_saved_preset failed after a pre-solve cancel (slot {slot}): {e}");
        }
    }
    if stopped {
        // ⟦3a⟧ A STOPPED RUN THAT SAVED RETURNS ITS OUTCOMES — never a bare cancel, whose
        // results the command drops. The save above persisted every scene this run already
        // reported, and its verdict (`persist_mismatch`) is on those outcomes: dropping them
        // would hide a save that did not hold behind "Everything matched so far stays saved."
        //
        // With a LANDED TRADE it is also the disclosure. The save persisted the raised
        // `presetLevel` + the base fader that holds base on target. Backing the trade out HERE
        // was the alternative and it is WRONG: every scene this run already solved was solved
        // AT the raised level, so lowering `presetLevel` again would leave those persisted
        // values off target by exactly `raise_db` while the run reported them on target —
        // silently wrong numbers, the one outcome this codebase refuses. Skipped benefiting
        // scenes DO end up `raise_db` louder than authored, which is a real cost — so the
        // trade summary rides along, giving the UI both the disclosure and the pre-trade
        // values to restore from. danger.md's cancel contract is intact either way: nothing
        // early-returns past the re-amp engage and the cleanup above ran whole.
        if save && attempted {
            if trade_persisted {
                log::warn!(
                    "slot {slot}: cancelled AFTER a headroom trade landed — the raised base pair \
                     is persisted and reported, not silently backed out"
                );
            }
            return Ok(outcomes);
        }
        return Err(LevelError::Cancelled);
    }
    Ok(outcomes)
}

/// The batch save's verdict, onto the outcomes it covers: a save that landed is re-read
/// (`verify`, which stamps `persist_mismatch` per written scene), and one that failed — its
/// retry included — marks every scene this batch WROTE as not held, since the stored preset
/// cannot be holding a value the save never wrote. A scene that wrote nothing is left alone
/// (`None`, nothing to hold). The failure is only logged beyond that: the outcomes are the
/// report, so the Summary says "the save didn't hold" on exactly those rows, on a stopped
/// run too. Pure apart from `verify`, so the policy is unit-gated.
pub(super) fn report_batch_save(
    slot: ListIndex,
    stopped: bool,
    saved: Result<(), LevelError>,
    written: &[PersistedWrite],
    outcomes: &mut [BatchedSceneOutcome],
    verify: impl FnOnce(&mut [BatchedSceneOutcome]),
) {
    match saved {
        Ok(()) => verify(outcomes),
        Err(e) => {
            log::warn!(
                "save_deferred_scene_writes failed{} (slot {slot}): {e} — every written scene \
                 is reported as not held",
                if stopped { " on cancel" } else { "" }
            );
            for o in outcomes.iter_mut() {
                if written.iter().any(|w| w.scene_slot == o.scene_slot) {
                    o.persist_mismatch = Some(true);
                }
            }
        }
    }
}

/// The scene batch's ONE persist: recall the preset's original active scene (a save stamps
/// `lastLoadedScene` + switch states from the working state), then ONE `saveCurrentPreset`
/// persisting every accumulated unsaved scene overlay. HW (`probe --defer-scenes`, fw
/// 1.8.45): unsaved scene-edit writes survive scene recalls and reconnects; re-recalling a
/// written scene does NOT revert it; base recall = wire slot 8; the single save persists ALL
/// accumulated overlays. One retry on a fresh connection (the realistic failure is the
/// open, not the save itself). The connection never toggles re-amp, so the post-re-amp
/// save-drop cannot bite.
pub(super) fn save_deferred_scene_writes(
    slot: ListIndex,
    restore_scene: Option<u32>,
    reasserts: &[Reassert],
) -> Result<(), LevelError> {
    // NOT `sleep_or_cancel`: this is ALSO fired on cancel, to persist the scene overlays
    // already written. Bailing here would throw away the run's completed work.
    let attempt = || -> Result<(), LevelError> {
        let mut s = Session::connect()?;
        recall_reassert_save(&mut s, slot, restore_scene, reasserts)
    };
    attempt().or_else(|e| {
        log::warn!("deferred scene save failed ({e}); retrying on a fresh connection");
        attempt()
    })
}

/// Result of the joint-k solve for a scene's amp-knob set.
pub(super) struct JointK {
    /// Per-knob final levels (aligned with the input `knobs`), each `current_i · k_eff`.
    pub(super) levels: Vec<f32>,
    /// The target needed more boost than the hottest amp's headroom allowed.
    pub(super) clamped: bool,
    /// Predicted captured loudness at the applied levels (= target unless clamped).
    pub(super) achieved: f64,
    /// The applied scale factor (< the ideal `k` when clamped).
    pub(super) k_eff: f64,
}

/// The smallest joint factor a scale-DOWN may use before the QUIETEST AUDIBLE lane reaches
/// `floor`. The mirror image of `k_cap` (where the LOUDEST lane binds the scale-up): every
/// lane moves by ONE `k`, so on the way down the quietest one runs out first.
///
/// TWO RULES that must not be lost (danger.md: `outputLevel = 0` is deep digital silence):
/// * the bound is on **k**, never on the individual levels — clamping a level UP to the floor
///   would UN-MUTE an author-muted `0 · k` lane, i.e. change the preset's tone.
/// * a lane already at or below `floor` is author-parked, so it does NOT bind: it would
///   otherwise forbid every scale-down.
///
/// `floor = LEVEL_MIN` (0.0) — every caller but the headroom trade's base hold — yields 0 and
/// leaves the solve byte-identical to the pre-floor behaviour.
pub(super) fn joint_k_floor(currents: impl Iterator<Item = f32>, floor: f32) -> f64 {
    if floor <= 0.0 {
        return 0.0;
    }
    // The same "quietest AUDIBLE lane" fold the trade planner's own fader-room estimate uses
    // — one helper, so the bound the solver enforces and the room the planner promises can
    // never be computed off two different lanes.
    match crate::headroom_trade::min_audible_above(currents, floor) {
        Some(min_audible) => (floor as f64) / f64::from(min_audible),
        None => 0.0,
    }
}

/// Solve joint-k for a scene's amp-knob set, given the as-is `measured` loudness at the
/// knobs' current values. Scaling every amp's `outputLevel` by one `k` shifts the
/// summed output by exactly `20·log10(k)` (correlation-invariant), so:
///   `k = 10^((target − measured)/20)`, clamped to keep the hottest amp ≤ `LEVEL_MAX`
///   (`k_eff = min(k, LEVEL_MAX / max_i current_i)` — ratio-preserving), then
///   `level_i = current_i · k_eff`.
/// Requires amplitude (0..1) knobs — a dB-unit knob can't be scaled multiplicatively
/// (`−12 dB · 1.5` is nonsense) and errors. The per-knob `current` is floored at 1e-3
/// so an author-muted lane (current 0) doesn't divide-by-zero (it stays muted: `0·k`).
///
/// `floor` bounds the solve from BELOW as well ([`joint_k_floor`]). `LEVEL_MIN` for every
/// lane but the headroom trade's base hold, which may not solve the base amp into digital
/// silence just to pay for a `presetLevel` raise.
pub(super) fn solve_joint_k_at(
    knobs: &[KnobTarget],
    target_lufs: f64,
    measured: f64,
    floor: f32,
) -> Result<JointK, LevelError> {
    if knobs.is_empty() {
        return Err(LevelError::from("joint-k: no amp knobs in scene"));
    }
    for kt in knobs {
        if kt.lo < -1e-6 || kt.hi > 1.0 + 1e-6 {
            return Err(LevelError::Device(format!(
                "joint-k requires amplitude (0..1) knobs; got bounds [{}, {}] for {}",
                kt.lo,
                kt.hi,
                kt.knob.label()
            )));
        }
    }
    let max_cur = knobs
        .iter()
        .map(|kt| kt.current.clamp(1e-3, 1.0) as f64)
        .fold(0.0_f64, f64::max);
    let k = 10f64.powf((target_lufs - measured) / 20.0);
    let k_cap = (LEVEL_MAX as f64) / max_cur;
    let k_floor = joint_k_floor(knobs.iter().map(|kt| kt.current), floor);
    let k_eff = k.min(k_cap).max(k_floor).max(0.0);
    // Clamped in EITHER direction now: out of boost at the top, out of fader at the bottom.
    let clamped = (k_eff - k).abs() > 1e-9;
    let levels = knobs
        .iter()
        .map(|kt| (kt.current as f64 * k_eff).clamp(LEVEL_MIN as f64, LEVEL_MAX as f64) as f32)
        .collect();
    let achieved = measured + 20.0 * k_eff.max(1e-9).log10();
    Ok(JointK {
        levels,
        clamped,
        achieved,
        k_eff,
    })
}

/// One scene's solve outcome.
pub(super) struct SceneSolve {
    /// Achieved (verified, or predicted when unverified) loudness.
    pub(super) lufs: f64,
    /// Final per-knob levels, aligned with the job's knobs.
    pub(super) levels: Vec<f32>,
    /// Target unreachable — ran out of headroom, OR a specific `clamp_reason` applies.
    pub(super) clamped: bool,
    /// Dynamics spread (LU) of the as-is measure capture.
    pub(super) spread: f64,
    /// Device writes this scene took (0 = already at target, nothing written).
    pub(super) writes: u32,
    /// Set with `clamped` for the "no authority" case (off-branch / off-USB amp).
    pub(super) clamp_reason: Option<String>,
    /// The clamp's cause from the shared taxonomy —
    /// [`crate::headroom_trade::ClampKind::from_flags`]'s answer, so the wet floor of a
    /// user-HANDLE row is reported as [`crate::headroom_trade::ClampKind::WetFloor`] rather
    /// than collapsing into a generic headroom clamp.
    pub(super) clamp_kind: Option<crate::headroom_trade::ClampKind>,
    /// WHICH bound the joint-k solve pinned at, when one did — the solve's own direction-aware
    /// answer ([`joint_levels_pinned`]), carried so no consumer re-derives it from `levels`.
    /// `None` on every lane that cannot pin one: the already-at-target skip, the user-HANDLE
    /// lane (its bound is the param's own, reported through `clamp_kind`), and any solve no
    /// bound stopped. Read by the headroom trade's base hold ([`trade_hold_failure_kind`]).
    pub(super) pinned: Option<PinnedBound>,
    /// Rebalance "verify by ear" flag; `false` for the plain joint-k path.
    pub(super) verify_by_ear: bool,
    /// Whether `lufs` was measured after the write (see [`Verification`]).
    pub(super) verification: Verification,
}

/// Is this scene already at target? Matches the correction loop's `ACCEPT_TOL_LU`
/// acceptance band (rather than the tighter ~0.1 dB knob-ratio check this replaces)
/// so a re-run doesn't rewrite an already-in-tolerance scene and re-randomize it.
/// Deliberately skips the corrective pass (`correct_iter`) for a within-tolerance
/// COMPRESSED scene (the UA1176 case below, see `jointk_one_scene`'s doc) — within
/// tolerance is good enough, and a `clamped` solve must still fall through and
/// report clamped even when the measured value happens to sit on target.
///
/// That `!clamped` requirement is this lane's OWN rule, not a shared one: the base lane's
/// `already_on_target` drops it, and may, because its skip is provably disjoint from a boost.
/// Nothing of the sort holds here — keep the flag.
pub(super) fn scene_at_target(measured: f64, target: f64, clamped: bool) -> bool {
    !clamped && (measured - target).abs() <= ACCEPT_TOL_LU
}

/// What every per-scene solve opens with: the scene's as-is reading.
pub(super) struct Prologue {
    /// AS-IS loudness at the scene's authored knob values — the solve's starting point.
    pub(super) asis: f64,
    /// Dynamics spread (LU) of that same capture.
    pub(super) spread: f64,
}

/// The prologue BOTH per-scene lanes share (`jointk_one_scene` on the amp's `outputLevel`,
/// `handle_one_scene` on the user's own control): ONE isolated fresh re-amp capture of the
/// scene as-is — no write, no Scene Edit, nothing to undo.
///
/// What is deliberately NOT here is the already-at-target short-circuit, because the two
/// lanes' skip TESTS genuinely differ (same `KNOB_TOL_LU` band, different clamp input):
/// joint-k asks `scene_at_target(asis, target, clamped)` with the clamp flag from
/// `solve_joint_k_at` — which does not exist until after its own solver runs, and which
/// deliberately forces a clamped scene to fall through and be REPORTED even when the as-is
/// reading happens to sit on target. The handle lane has no closed-form solve and therefore
/// no clamp flag to pass (`false`). Each lane keeps its own one-line test and builds the
/// skip outcome through [`Prologue::already_there`], so only the test differs, not the shape.
///
/// REORDERED RUN: when the job carries a [`ScenePrepass`] the reading is CONSUMED rather than
/// taken — the batch already paid this capture up front so it could plan the headroom trade
/// against every ceiling at once. A job with no prepass measures here exactly as before, so
/// the redistribution/bench callers are untouched. The rebalance lane opens with it too
/// (`rebalance_one_scene`, through [`scene_prologue_or_silent`]), so a silent mergeable scene
/// clamps before any lane solo; with no prepass that costs it one as-is capture.
///
/// `intended_preset_level` rides through to [`measure_scene_asis`] — the run's own solved or
/// UNSAVED-held `presetLevel`, re-asserted after the scene recall so the reading describes
/// the level the run is actually working at rather than the one the device has saved. It has
/// no effect on a CONSUMED prepass reading (that capture was already taken).
pub(super) fn scene_prologue(
    job: &SceneJob,
    stimulus: &[f32],
    intended_preset_level: Option<f32>,
) -> Result<Prologue, LevelError> {
    // Hard-error on a persistent flat read (after the retry). Trade-off, made
    // consciously: a real scene crushed by a limiter (the UA1176 case) with spread ≤ the
    // trip gate would false-error — but the library's Base minimum is 0.12 and without the
    // guard a floor read lands on the no-authority clamp path, which mislabels a USB
    // failure as an off-branch amp.
    let (asis, spread) = match job.prepass {
        Some(p) => (p.asis, p.spread),
        None => {
            let loudness = require_live(
                || {
                    measure_scene_asis(
                        job.scene_slot,
                        stimulus,
                        intended_preset_level,
                        &job.force_bypass,
                    )
                },
                stimulus,
            )?;
            (loudness.integrated_lufs, loudness.spread_lu())
        }
    };
    Ok(Prologue { asis, spread })
}

/// The scene lanes' routing clamp: the as-is capture was SILENT (`LevelError::NoSignal`), so
/// the scene's output is not reaching USB 1/2 — the same conversion, sentinel and wording the
/// base lane (`level_preset_impl`) and the footswitch lane's first seed (`solve_param_secant`)
/// already make, so a silent scene is the honest
/// [`crate::headroom_trade::ClampKind::NoAuthority`] clamp ("no signal on USB 1/2", the UI's
/// off-branch outcome) rather than a failed row.
///
/// The commonest cause is an authored one: a scene saved with its amp `outputLevel` at 0,
/// which is DEEP DIGITAL SILENCE on the real unit (`danger.md`), so no knob value read from
/// the as-is capture exists to solve from. It is also what a scene routed off USB 1/2 reads
/// as. Either way nothing is written (`writes: 0`, every knob keeps its authored `current`),
/// so the batch's deferred save has nothing of this scene's to persist and no restore is
/// owed: the as-is capture is a pure measurement.
///
/// Only the AS-IS capture converts. A silence later in a solve (a verify or correction
/// capture) keeps its own handling — that reading follows a write the solve chose.
pub(super) fn no_signal_scene_solve(levels: Vec<f32>) -> SceneSolve {
    let clamp_reason = Some(NO_SIGNAL_REASON.to_string());
    SceneSolve {
        lufs: MUTE_FLOOR_SILENT_LUFS,
        levels,
        clamped: true,
        clamp_kind: crate::headroom_trade::ClampKind::from_flags(
            true,
            false,
            clamp_reason.as_deref(),
        ),
        pinned: None,
        // A silent capture has no dynamics to report.
        spread: 0.0,
        writes: 0,
        clamp_reason,
        verify_by_ear: false,
        // The sentinel is not a reading of a level the scene can hold: say so, exactly as the
        // handle lane's own routing clamp does.
        verification: Verification::Predicted,
    }
}

/// [`scene_prologue`] with the silent as-is capture converted to the routing clamp
/// ([`no_signal_scene_solve`]): `Ok(Err(solve))` is the scene's finished outcome, `Ok(Ok(_))`
/// the reading to solve from. Shared by EVERY per-scene lane (joint-k, user handle, and
/// rebalance's `rebalance_one_scene`) so they cannot word or shape the clamp two ways. `levels` is what the device holds (each knob's authored `current`).
pub(super) fn scene_prologue_or_silent(
    job: &SceneJob,
    stimulus: &[f32],
    intended_preset_level: Option<f32>,
    levels: impl FnOnce() -> Vec<f32>,
) -> Result<Result<Prologue, SceneSolve>, LevelError> {
    match scene_prologue(job, stimulus, intended_preset_level) {
        Ok(p) => Ok(Ok(p)),
        Err(e) if e.is_no_signal() => {
            log::info!(
                "scene {}: as-is capture is silent — no signal on USB 1/2, reporting the \
                 routing clamp (nothing written)",
                job.scene_slot
            );
            Ok(Err(no_signal_scene_solve(levels())))
        }
        Err(e) => Err(e),
    }
}

impl Prologue {
    /// The scene is already at its target: report the as-is reading and leave every knob
    /// untouched (`writes: 0`). `levels` is what the device already holds — each knob's
    /// authored `current`.
    pub(super) fn already_there(&self, levels: Vec<f32>) -> SceneSolve {
        SceneSolve {
            lufs: self.asis,
            levels,
            clamped: false,
            clamp_kind: None,
            pinned: None,
            spread: self.spread,
            writes: 0,
            clamp_reason: None,
            verify_by_ear: false,
            // Nothing was written: the as-is capture IS the measurement of what stays.
            verification: Verification::Verified,
        }
    }
}

/// Per-scene joint-k: measure the scene AS-IS once, solve one factor `k`, apply it to every
/// lane amp (preserving their mix), VERIFY, then `correct_iter` (bounded secant) to converge
/// through a downstream compressor. The open-loop `20·log10(k)` model is exact for pure gain
/// (±0.07 LU) but UNDERSHOOTS through a compressor/limiter (preset 027's UA1176 → −22.93 vs
/// −22). On a linear chain the first verify is within tol and no correction runs. Shared by
/// `level_scenes_oneshot` and the rebalance flow's non-mergeable scenes. `verify=false` skips
/// both verify and correction.
#[allow(clippy::too_many_arguments)]
pub(super) fn jointk_one_scene(
    slot: ListIndex,
    job: &SceneJob,
    stimulus: &[f32],
    target_lufs: f64,
    defer: bool,
    verify: bool,
    saved: Option<&serde_json::Value>,
    // The knob-set's DOWNWARD bound (see [`joint_k_floor`]): `LEVEL_MIN` for every scene lane,
    // [`crate::headroom_trade::BASE_FADER_FLOOR`] for the headroom trade's base hold, which
    // may not solve the base amp into digital silence.
    floor: f32,
    // The run's own `presetLevel` — its solved value, or the UNSAVED raise a headroom trade is
    // holding — re-asserted on the as-is capture below (`measure_scene_asis`). `None` = assert
    // nothing. STALE CLAIM CORRECTED (2026-08-29): `apply_first_verified`/`correct_iter` do NOT
    // route through `capture_on_session` (that seam is Doctor's + the isolated-block-knob
    // paths) — they capture through `apply_levels`' own `engage_measure_disengage`, and DO
    // thread `intended_preset_level` through it (`LevelOptions::intended_preset_level`, set
    // below). What is NOT threaded into those two is `force_bypass` from a caller-supplied
    // literal here — they take it from `job.force_bypass` instead (A4: base's isolation lives
    // on the job, not a parallel parameter chain).
    intended_preset_level: Option<f32>,
) -> Result<SceneSolve, LevelError> {
    let prologue = match scene_prologue_or_silent(job, stimulus, intended_preset_level, || {
        job.knobs.iter().map(|kt| kt.current).collect()
    })? {
        Ok(p) => p,
        Err(silent) => return Ok(silent),
    };
    let (measured, spread) = (prologue.asis, prologue.spread);
    let JointK {
        levels,
        clamped,
        achieved,
        k_eff,
    } = solve_joint_k_at(&job.knobs, target_lufs, measured, floor)?;
    // Already at target (within the ACCEPT_TOL_LU acceptance band) and not clamped →
    // leave every knob untouched (a clamp must still be REPORTED even if nothing moves,
    // which is why this lane's test takes the joint-k clamp flag and the handle lane's
    // cannot).
    if scene_at_target(measured, target_lufs, clamped) {
        return Ok(prologue.already_there(job.knobs.iter().map(|kt| kt.current).collect()));
    }
    // Scene writes are NEVER saved per apply: `defer` accumulates them unsaved in the
    // working copy (the runner saves ONCE at batch end); `!defer` is the dry-run shape
    // (each apply restores). See `save_deferred_scene_writes`.
    let opts = LevelOptions {
        verify,
        defer,
        intended_preset_level,
        ..Default::default()
    };
    let base: Vec<f32> = job.knobs.iter().map(|kt| kt.current).collect();
    let knob_refs: Vec<&LevelKnob> = job.knobs.iter().map(|kt| &kt.knob).collect();
    let expected_db = 20.0 * k_eff.max(1e-9).log10();
    let (v0, retry_writes) = apply_first_verified(
        slot,
        stimulus,
        &knob_refs,
        &levels,
        opts,
        expected_db,
        measured,
        saved,
        &job.force_bypass,
    )?;
    let (best_lufs, best_levels, clamp_reason, writes) = match v0 {
        Some(v0) if verify => {
            let c = correct_iter(
                slot,
                stimulus,
                &knob_refs,
                &base,
                levels,
                measured,
                v0,
                target_lufs,
                defer,
                saved,
                floor,
                intended_preset_level,
                &job.force_bypass,
            )?;
            (
                c.lufs,
                c.levels,
                c.clamp_reason,
                1 + retry_writes + c.writes,
            )
        }
        _ => (v0.unwrap_or(achieved), levels, None, 1 + retry_writes),
    };
    // A planned verify that failed twice leaves `best_lufs` = the PREDICTION: say so.
    let verification = Verification::of(verify, v0);
    // Report clamped from the FINAL point, NOT the open-loop's initial want: a specific reason
    // fired, or the verified best still can't reach target (knob out of headroom / chain limits
    // below target). The open-loop `clamped` flag is deliberately DROPPED here — a scene whose
    // first solve wanted `outputLevel > 1.0` but whose verify+correct then landed a valid point
    // within `ACCEPT_TOL_LU` HAS reached target (it just started far below), so it is "done", not
    // clamped. Keying the flag on `clamped ||` over-reported those as clamped (a stale edge flag,
    // exactly the redistribution's once-clamped-now-rescued scenes).
    let clamped = clamp_reason.is_some() || (best_lufs - target_lufs).abs() > ACCEPT_TOL_LU;
    let (pinned, clamp_kind) = joint_clamp_report(
        &best_levels,
        &base,
        floor,
        best_lufs,
        target_lufs,
        clamped,
        clamp_reason.as_deref(),
    );
    Ok(SceneSolve {
        lufs: best_lufs,
        levels: best_levels,
        clamped,
        clamp_kind,
        pinned,
        spread,
        writes,
        clamp_reason,
        verify_by_ear: false,
        verification,
    })
}

/// Did the joint-k solve actually PIN at the bound that blocks the direction the target needs?
/// The scene lane's mirror of [`classify_fs_outcome`]'s direction-aware test, and the gate on
/// stamping [`crate::headroom_trade::ClampKind::SceneCeiling`].
///
/// DIRECTION MATTERS, both ends:
/// * a lane at `LEVEL_MAX` with the target still LOUDER → out of headroom, a real ceiling;
/// * a lane at `floor` with the target still QUIETER → out of fader (the base hold's
///   [`crate::headroom_trade::BASE_FADER_FLOOR`] case);
/// * anything else — including a lane sitting AT a bound whose miss points back INTO its range
///   — is the search stopping early, not the sound being unable to get there.
///
/// `base` is the pre-solve value set, so a lane the author already parked at a bound (and the
/// solve never moved) is not read as the solve pinning there.
pub(super) fn joint_levels_pinned(
    levels: &[f32],
    base: &[f32],
    floor: f32,
    achieved: f64,
    target: f64,
) -> Option<PinnedBound> {
    let moved_or_not_authored = |i: usize, v: f32| base.get(i).is_none_or(|b| (b - v).abs() > 1e-4);
    levels.iter().enumerate().find_map(|(i, &v)| {
        if v >= LEVEL_MAX - 1e-3 && target > achieved {
            Some(PinnedBound::Max)
        } else if v <= floor + 1e-6 && target < achieved && moved_or_not_authored(i, v) {
            Some(PinnedBound::Floor)
        } else {
            None
        }
    })
}

/// WHICH bound a joint-k solve pinned at. The two are mutually exclusive by construction —
/// they gate on opposite signs of `target − achieved` — so one answer per solve.
///
/// Carried on [`SceneSolve`] rather than re-derived downstream: the headroom trade's base hold
/// is the one consumer that must tell "the base fader genuinely ran out" apart from "the
/// bounded secant stalled mid-range", and a levels scan cannot (an author-MUTED lane sits at
/// the floor in every solve of that preset). See [`trade_hold_failure_kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinnedBound {
    /// A lane is at [`LEVEL_MAX`] and the target is still LOUDER — out of headroom.
    Max,
    /// A lane the solve MOVED is at `floor` and the target is still QUIETER — out of fader.
    Floor,
}

/// The joint-k lanes' whole clamp REPORT in one place: which bound the solve pinned at, and
/// the taxonomy cause that follows. Both joint-k producers ([`jointk_one_scene`] and
/// [`rebalance_one_scene`]) need exactly this pair, and forking the four lines twice is how
/// the two lanes drift about when a cause may be named at all.
///
/// ⟦8⟧ Only name a cause when a bound ACTUALLY pinned the solve (or a routing failure fired).
/// An amp-`outputLevel` lane has no wet floor, so the only causes here are routing and the
/// ordinary headroom clamp — but a row that merely ran out of secant captures MID-RANGE has
/// neither. Telling the user "its level control is already maxed out" about a fader sitting
/// at 0.4 is a false cause: a re-run can improve that row. `clamp_reason` is deliberately NOT
/// filled in instead — `.claude/rules/leveling-dsp.md` pins that field to "no signal on
/// USB 1/2" and the UI maps ANY non-null reason to the off-branch outcome.
pub(super) fn joint_clamp_report(
    levels: &[f32],
    base: &[f32],
    floor: f32,
    achieved: f64,
    target: f64,
    clamped: bool,
    clamp_reason: Option<&str>,
) -> (
    Option<PinnedBound>,
    Option<crate::headroom_trade::ClampKind>,
) {
    let pinned = joint_levels_pinned(levels, base, floor, achieved, target);
    let kind = if clamp_reason.is_some() || pinned.is_some() {
        crate::headroom_trade::ClampKind::from_flags(clamped, false, clamp_reason)
    } else {
        None
    };
    (pinned, kind)
}

/// The value `saved` (the stored row) holds for `param` on `node` as SCENE `overlay_scene`
/// hears it: the scene's own overlay value when it carries one, else base's (a `Full`
/// overlay without the key, a `BypassOnly` one and an `Absent` one all inherit base —
/// `overlay_scene_onto_graph`'s merge rule). `None` for base when the document lost its
/// `audioGraph`, and for a scene whose overlay presence is unanswerable (`Unknown`).
fn stored_effective_value(
    saved: &serde_json::Value,
    overlay_scene: Option<u32>,
    node: &str,
    param: &str,
) -> Option<f64> {
    let base = || crate::audiograph::node_param_f64(saved, node, param);
    match overlay_scene {
        None => base(),
        Some(scene) => match overlay_param(saved, scene, node, param) {
            SceneParamRead::Value(v) => v.as_f64(),
            SceneParamRead::Absent => base(),
            SceneParamRead::Unknown => None,
        },
    }
}

/// Band within which the job's `current` and the stored row's value are the SAME knob
/// value — JSON float formatting only, far below any audible step.
const ASIS_SEED_VALUE_TOL: f64 = 1e-6;

/// The JOB-local half of the scene handle lane's as-is seed gate (#196): is the prologue's
/// `measure_scene_asis` reading exactly what `measure_knob_at(stimulus, knob, kt.current,
/// &job.force_bypass, saved, intended_preset_level)` would capture? The reading is either the
/// prologue's own capture or the batch's PHASE-1 prepass capture of the same seam
/// (`prepass_scene_ceilings`), taken at the same `presetLevel` when no trade landed
/// (`scene_capture_level(None, ..)` both times). Element by element:
///
/// * STIMULUS, `presetLevel`, ISOLATION — identical by construction: both seams take the
///   same `stimulus`, re-assert the same `intended_preset_level` right after their recall,
///   and send the same `job.force_bypass` last, each on its own fresh connection and engage
///   (danger.md: only the first capture after an engage is accurate — both are firsts).
/// * SCENE — the as-is recalls `job.scene_slot`; `set_knobs` recalls the KNOB's scope. They
///   must name the same scene (a base knob ⇔ the base slot).
/// * KNOB VALUE — the as-is writes nothing, so it hears whatever the working copy holds,
///   while `measure_knob_at` writes `kt.current`. The scene-doc prepass `load_preset` leaves
///   the working copy equal to the stored row (danger.md: a same-slot load reloads a dirty
///   copy and keeps a clean one, which already equals it), so the two agree exactly when
///   `kt.current` IS the stored row's effective value for this scene — which is NOT a given:
///   `handle_scene_job` falls back to BASE's value (or 0.0) when the live scene doc lacks the
///   node (a lean field-3 push truncates large graphs), and the scene may overlay something
///   else. Writes this run made since are the batch half's job ([`asis_reuse_batch_ok`]).
/// * BOUNDS — `kt.current` must lie inside the handle's own bounds, so seed 1 can never
///   become a best point the solve could not have probed itself.
///
/// The Scene Edit enable `measure_knob_at` may send on a node with no overlay reseeds that
/// overlay FROM BASE, i.e. with the values the scene already inherits, so it is
/// audio-neutral here. Any doubt answers `false` — the fixed-seed path is always safe.
fn asis_seed_context_matches(
    job: &SceneJob,
    kt: &KnobTarget,
    handle: &FsParamTarget,
    saved: Option<&serde_json::Value>,
) -> bool {
    let LevelKnob::Block {
        node_id,
        parameter_id,
        scene_slot,
        ..
    } = &kt.knob
    else {
        return false;
    };
    let same_scene = match scene_slot {
        Some(n) => *n == job.scene_slot && *n < crate::session::BASE_SCENE_SLOT,
        None => job.scene_slot == crate::session::BASE_SCENE_SLOT,
    };
    let (lo, hi) = handle.bounds();
    let in_bounds = kt.current.is_finite() && kt.current >= lo && kt.current <= hi;
    let stored_matches = saved
        .and_then(|sv| stored_effective_value(sv, *scene_slot, node_id, parameter_id))
        .is_some_and(|v| (v - f64::from(kt.current)).abs() <= ASIS_SEED_VALUE_TOL);
    same_scene && in_bounds && stored_matches
}

/// The BATCH half of the as-is seed gate: nothing this run writes between the as-is reading
/// and `job`'s solve may change what the solve's captures hear. Refused when:
///
/// * a headroom trade landed (`hold`): it wrote base `outputLevel`(s) and a raised
///   `presetLevel` unsaved AFTER the PHASE-1 readings, and re-targets them by arithmetic;
/// * any OTHER row of the batch touches the same node: a base write is inherited by every
///   scene without its own overlay, and a `BypassOnly` scene write may land on base;
/// * an EARLIER row (run order — `run_scene_jobs` walks `jobs` in order) carries an isolation
///   list: its forced bypasses sit in the working copy while this row's solve runs, but were
///   not there when the PHASE-1 reading was taken.
///
/// `job` must be an element of `jobs` (it is, inside `run_scene_jobs`); anything else is
/// refused.
fn asis_reuse_batch_ok(jobs: &[SceneJob], job: &SceneJob, hold: Option<&TradeHold>) -> bool {
    let Some(pos) = jobs.iter().position(|o| std::ptr::eq(o, job)) else {
        return false;
    };
    let node_of = |k: &LevelKnob| match k {
        LevelKnob::Block { node_id, .. } => Some(node_id.clone()),
        _ => None,
    };
    let mine: Vec<String> = job
        .knobs
        .iter()
        .filter_map(|kt| node_of(&kt.knob))
        .collect();
    hold.is_none()
        && !jobs[..pos].iter().any(|o| !o.force_bypass.is_empty())
        && !jobs
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != pos)
            .flat_map(|(_, o)| o.knobs.iter())
            .filter_map(|kt| node_of(&kt.knob))
            .any(|n| mine.contains(&n))
}

/// Correction budget for the scene HANDLE lane's param secant — half [`FS_CORRECT_MAX`].
/// Each iterate is a fresh-connect re-amp capture (~10 s) and a scene batch pays it up to 9
/// times over, so the footswitch lane's 16 would put a whole-preset handle run past 20
/// minutes; 8 still covers the HW-observed worst case (the Hiwatt `volume` cliff converged
/// in ~4 Illinois-damped bracket iterates), and a well-behaved knob exits on its first
/// in-tolerance iterate either way. A row that runs out reports honestly — it lands off
/// target and `handle_one_scene` marks it clamped against [`ACCEPT_TOL_LU`], never
/// silently as done.
pub(super) const SCENE_HANDLE_CORRECT_MAX: u32 = 8;

/// One scene solved on the USER'S OWN handle instead of the amp's `outputLevel`.
///
/// SEAM CHOICE (`solve_param_secant`, i.e. the footswitch solve's loop, over the joint-k
/// one): joint-k is not a search at all — it is the closed-form `k = 10^((target−measured)/20)`
/// that only holds for an AMPLITUDE knob multiplying the whole summed output, and
/// `solve_joint_k_at` hard-errors on anything outside `0..1` for exactly that reason. A
/// user-chosen handle is an arbitrary control (a raw-dB `gain`, a wet `mix`, a pedal taper),
/// so the lane needs the generic param-space search that already carries the class gate,
/// the range-driven bounds, the wet floor, silence-as-data and the Illinois-damped bracket
/// — that is the footswitch solve, and duplicating it would fork every one of those HW
/// lessons. Only the capture budget differs ([`SCENE_HANDLE_CORRECT_MAX`]).
///
/// The measure closure is [`measure_knob_at`] with the knob's SCENE context, so every
/// probe writes through the Scene-Edit-aware `set_knobs`: its overlay gates apply unchanged
/// — a `BypassOnly` scene (knobs shared with base) refuses on the FIRST capture and the row
/// becomes a per-scene failed outcome, never a write that leaks to base.
///
/// AS-IS FIRST, exactly like `jointk_one_scene`: one `measure_scene_asis` capture (no write
/// at all) supplies the idempotency skip. That is why `current_value` is `None` below — the
/// solve's own idempotency probe would re-measure this very point. The same capture then
/// SEEDS the solve (`known_seed`, one capture saved) whenever [`asis_seed_context_matches`]
/// proves it is exactly what `measure_knob_at(kt.current)` would capture; otherwise the
/// solve seeds from the param's own range as before.
///
/// The write is DEFERRED like every other scene write (`defer`), so the batch's single
/// `save_deferred_scene_writes` persists it: ONE save per preset, unchanged.
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_one_scene(
    slot: ListIndex,
    job: &SceneJob,
    handle: &FsParamTarget,
    stimulus: &[f32],
    target_lufs: f64,
    defer: bool,
    saved: Option<&serde_json::Value>,
    // The run's own `presetLevel` (see `jointk_one_scene`'s parameter of the same name),
    // asserted on the as-is capture AND on every solve capture — each `measure_knob_at` here
    // recalls the knob's scene, which reverts the level exactly like the as-is recall does.
    intended_preset_level: Option<f32>,
    // The batch-level half of the as-is seed gate ([`asis_reuse_batch_ok`]): no trade hold
    // and no other row of this batch writes the handle's node.
    batch_allows_asis_seed: bool,
) -> Result<SceneSolve, LevelError> {
    // CLASS GATE, before any device work — the same refusal wording every lane shares.
    if let Some(refusal) = handle.refuse_if_not_a_level_control() {
        return Err(LevelError::refused(SkipKind::UnsupportedKnob, refusal));
    }
    let kt = match job.knobs.as_slice() {
        [one] => one,
        n => {
            return Err(LevelError::Device(format!(
                "a user-chosen scene handle addresses exactly one control, got {}",
                n.len()
            )))
        }
    };
    let prologue = match scene_prologue_or_silent(job, stimulus, intended_preset_level, || {
        vec![kt.current]
    })? {
        Ok(p) => p,
        Err(silent) => return Ok(silent),
    };
    let (asis, spread) = (prologue.asis, prologue.spread);
    // Already there → leave the handle untouched (the scene lane's `scene_at_target` rule,
    // on its own acceptance band). No closed-form solve runs here, so there is no clamp flag
    // to feed it — unlike `jointk_one_scene`, whose flag exists by this point.
    if scene_at_target(asis, target_lufs, false) {
        return Ok(prologue.already_there(vec![kt.current]));
    }
    let knob = &kt.knob;
    // The as-is capture as seed 1, ONLY when its whole context provably equals the solve's
    // own `measure_knob_at(kt.current)` capture — see `asis_seed_context_matches`.
    let known_seed = (batch_allows_asis_seed && asis_seed_context_matches(job, kt, handle, saved))
        .then_some(SeedPoint {
            value: kt.current,
            lufs: asis,
            spread,
        });
    let solved = solve_param_secant(
        stimulus,
        target_lufs,
        None,
        known_seed,
        handle,
        SCENE_HANDLE_CORRECT_MAX,
        || reamp_off_fresh("scene_handle_solve"),
        |v| {
            measure_knob_at(
                stimulus,
                knob,
                v,
                &job.force_bypass,
                saved,
                intended_preset_level,
            )
        },
    )?;
    // The sweep left the LAST probed value in the working copy, not necessarily the best
    // one — write the solved value explicitly (unsaved under `defer`; the batch-end save
    // persists it).
    let opts = LevelOptions {
        verify: false,
        defer,
        ..Default::default()
    };
    apply_levels(
        slot,
        stimulus,
        &[(knob, solved.final_value)],
        opts,
        false,
        saved,
        // A4: "base means base" applies to a handle-driven base row too — the user's own
        // control is still measured/written with base's isolation asserted.
        &job.force_bypass,
    )?;
    // Reported on the shared band (`ACCEPT_TOL_LU`): a handle row and an amp row in the same
    // batch must mean the same thing by "done". A routing clamp (`clamp_reason`) rides through
    // verbatim.
    let clamped = solved.clamp_reason.is_some()
        || solved.clamped
        || (solved.predicted_lufs - target_lufs).abs() > ACCEPT_TOL_LU;
    Ok(SceneSolve {
        lufs: solved.predicted_lufs,
        levels: vec![solved.final_value],
        clamped,
        // The handle lane CAN wet-floor: the user's control may be a mix knob, and the floor
        // that stopped the solve is the wet-preservation one, not the chain's ceiling.
        clamp_kind: crate::headroom_trade::ClampKind::from_flags(
            clamped,
            solved.wet_floor,
            solved.clamp_reason.as_deref(),
        ),
        // No joint-k bound here — the handle's own bound is what stopped it, and the trade
        // never holds base on a user handle (`plan_trade_for_batch` refuses that batch).
        pinned: None,
        spread,
        // The solve's own captures plus this final apply write.
        writes: solved.iterations + 1,
        // Every solve point is a real capture of the value it reports; only the routing
        // clamp's sentinel isn't one.
        verification: if solved.clamp_reason.is_some() {
            Verification::Predicted
        } else {
            Verification::Verified
        },
        clamp_reason: solved.clamp_reason,
        verify_by_ear: false,
    })
}

/// A ≥ this intended `outputLevel` move (dB) that reads back ~unchanged (< `KNOB_TOL_LU`)
/// is a suspected DROPPED WRITE, not compression: even 6:1 compression passes ~0.33 LU of a
/// 2 dB move. Below it a flat response is ambiguous with noise, so no retry fires.
pub(super) const SUSPECT_DROP_MIN_DB: f64 = 2.0;

/// One `set_knobs` batch's targets: each knob paired with its level. (Superseded
/// `merge_repair_targets`, which also appended per-scene writes re-asserting params the
/// Scene Edit reseed would wipe — dead, and HW-proven not to land (8/8 failed): `set_knobs`
/// now enables Scene Edit ONLY where the node has no overlay, the one case where the reseed
/// has nothing to lose, so there is nothing left to repair.)
pub(super) fn zip_targets<'a>(
    knobs: &[&'a LevelKnob],
    levels: &[f32],
) -> Vec<(&'a LevelKnob, f32)> {
    knobs
        .iter()
        .copied()
        .zip(levels)
        .map(|(k, &v)| (k, v))
        .collect()
}

/// First verified apply with a ONE-SHOT dropped-write retry (scene paths). The device can
/// silently drop a scene write (the ~700 ms post-`loadScene` acceptance window, HW
/// `probe --bisect-scene`); without the retry a single drop reads as a flat response →
/// `correct_iter` sees no slope → a false, non-deterministic "clamped at <as-is>" (HW: the
/// user's first-run Arpeges clamp that succeeded on re-run). One re-apply — a fresh scene
/// recall + write + verify — disambiguates: a drop lands on the retry; a genuine
/// no-authority amp stays flat and takes the honest clamp downstream. Returns
/// `(verify_lufs, retry_writes)`.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_first_verified(
    slot: ListIndex,
    stimulus: &[f32],
    knobs: &[&LevelKnob],
    levels: &[f32],
    opts: LevelOptions,
    expected_db: f64,
    baseline_lufs: f64,
    saved: Option<&serde_json::Value>,
    // A4: the job's isolation list — empty for every scene job, the base row's own list when
    // this apply is base's (both a plain base solve and the headroom trade's hold).
    force_bypass: &[(String, String, bool)],
) -> Result<(Option<f64>, u32), LevelError> {
    let targets = zip_targets(knobs, levels);
    let v0 = apply_levels(slot, stimulus, &targets, opts, false, saved, force_bypass)?.1;
    match v0 {
        // The verify capture itself failed: one fresh re-apply + verify, the same bounded
        // retry a dropped write gets. A second failure reaches the caller as `None`, which
        // it reports as `Verification::VerifyFailed` — never as the prediction verified.
        None if opts.verify => {
            log::warn!("scene verify capture failed on slot {slot} — retrying the apply once");
            Ok((
                apply_levels(slot, stimulus, &targets, opts, false, saved, force_bypass)?.1,
                1,
            ))
        }
        Some(v)
            if opts.verify
                && expected_db.abs() >= SUSPECT_DROP_MIN_DB
                && (v - baseline_lufs).abs() < KNOB_TOL_LU =>
        {
            Ok((
                apply_levels(slot, stimulus, &targets, opts, false, saved, force_bypass)?.1,
                1,
            ))
        }
        other => Ok((other, 0)),
    }
}

/// Result of the bounded correction loop.
pub(super) struct Correction {
    pub(super) lufs: f64,
    pub(super) levels: Vec<f32>,
    /// `Some` for the no-authority case (the amp doesn't reach the USB 1/2 capture).
    pub(super) clamp_reason: Option<String>,
    /// Device writes the correction itself performed (iterations + any land-on re-apply).
    pub(super) writes: u32,
}

/// Bounded secant correction after a first verified apply (shared by joint-k + rebalance).
/// The open-loop `20·log10(k)` solve undershoots through a downstream compressor; this
/// iterates a trust-region-clamped (±`BATCH_TRUST_DB`) secant from the real points until
/// within `ACCEPT_TOL_LU`, capped at `MEASURE_CORRECT_MAX`, and ALWAYS lands the device on the
/// best point seen — re-applying it if the last write wasn't the best. (Critical: the device
/// working copy holds whatever was LAST written and the batch-end save persists exactly that,
/// so a worse final step would otherwise persist while a better number is reported.) When a
/// large applied gain produced ~no response, it reports
/// NO-AUTHORITY (off-branch / off-USB) and restores `base` rather than leaving the amp slammed.
/// `levels0`/`v0` = the levels the caller already applied (the device holds them) and their
/// verified loudness; `measured0` = loudness at `base`.
#[allow(clippy::too_many_arguments)]
pub(super) fn correct_iter(
    slot: ListIndex,
    stimulus: &[f32],
    knobs: &[&LevelKnob],
    base: &[f32],
    levels0: Vec<f32>,
    measured0: f64,
    v0: f64,
    target: f64,
    defer: bool,
    saved: Option<&serde_json::Value>,
    // The same downward bound the open-loop solve took (`joint_k_floor`): a correction step
    // must not walk the quietest lane past the floor that solve refused to cross. `LEVEL_MIN`
    // for every caller but the headroom trade's base hold.
    floor: f32,
    // The run's own `presetLevel`, re-asserted on every corrective capture — see
    // [`LevelOptions::intended_preset_level`]. Each iterate re-applies through
    // `apply_levels`, whose `set_knobs` recalls the scene and reverts an unsaved level, so
    // omitting it here reverts the whole loop to measuring the SAVED level.
    intended_preset_level: Option<f32>,
    // A4: the job's isolation list, forwarded to every `apply` inside the loop below — same
    // contract as `apply_first_verified`'s.
    force_bypass: &[(String, String, bool)],
) -> Result<Correction, LevelError> {
    let apply = |levels: &[f32], verify: bool| -> Result<Option<f64>, LevelError> {
        let opts = LevelOptions {
            verify,
            defer,
            intended_preset_level,
            ..Default::default()
        };
        let targets = zip_targets(knobs, levels);
        Ok(apply_levels(slot, stimulus, &targets, opts, false, saved, force_bypass)?.1)
    };

    correct_iter_core(base, levels0, measured0, v0, target, floor, apply)
}

/// The device as `correct_iter_core` sees it: every write goes through [`Self::write`], so
/// `held` (what the device holds, and what the batch-end save persists) and `writes` can't
/// drift from the writes actually made.
pub(super) struct HeldLevels<F> {
    pub(super) apply: F,
    pub(super) held: Vec<f32>,
    pub(super) writes: u32,
}

impl<F: FnMut(&[f32], bool) -> Result<Option<f64>, LevelError>> HeldLevels<F> {
    pub(super) fn write(
        &mut self,
        levels: &[f32],
        verify: bool,
    ) -> Result<Option<f64>, LevelError> {
        let v = (self.apply)(levels, verify)?;
        self.held = levels.to_vec();
        self.writes += 1;
        Ok(v)
    }

    /// Relative tolerance (~0.001 dB): an absolute one spans several dB at low levels.
    pub(super) fn holds(&self, levels: &[f32]) -> bool {
        self.held
            .iter()
            .zip(levels)
            .all(|(a, b)| (a - b).abs() <= 1e-4 * a.abs().max(b.abs()))
    }
}

/// The correction loop proper, over an injected `apply(levels, verify) -> measured LUFS`
/// (the device write + optional capture) so the loop is unit-testable without hardware.
pub(super) fn correct_iter_core(
    base: &[f32],
    levels0: Vec<f32>,
    measured0: f64,
    v0: f64,
    target: f64,
    floor: f32,
    apply: impl FnMut(&[f32], bool) -> Result<Option<f64>, LevelError>,
) -> Result<Correction, LevelError> {
    let max_base = base
        .iter()
        .map(|&x| x.clamp(1e-3, 1.0) as f64)
        .fold(0.0_f64, f64::max);
    let k_cap = (LEVEL_MAX as f64) / max_base;
    let k_floor = joint_k_floor(base.iter().copied(), floor).max(1e-3);
    let levels_for = |applied_db: f64| -> Vec<f32> {
        let k = 10f64.powf(applied_db / 20.0).clamp(k_floor, k_cap);
        base.iter()
            .map(|&b| (b as f64 * k).clamp(LEVEL_MIN as f64, LEVEL_MAX as f64) as f32)
            .collect()
    };
    // Shared factor, read off the loudest base lane so a muted lane 0 can't zero it. Every
    // lane muted → k0 = 1 (no applied gain), which keeps levels0 below.
    let k0 = (0..base.len())
        .filter(|&i| base[i] > 1e-3)
        .max_by(|&a, &b| base[a].total_cmp(&base[b]))
        .map_or(1.0, |i| levels0[i] as f64 / base[i] as f64);
    let applied_db0 = 20.0 * k0.max(1e-9).log10();
    // The caller already applied levels0.
    let mut dev = HeldLevels {
        apply,
        held: levels0.clone(),
        writes: 0,
    };

    // No-authority: a big applied gain barely moved the capture → the amp isn't on the USB
    // 1/2 path. Restore `base` (don't leave it slammed) and report the distinct reason.
    if no_authority(applied_db0, v0 - measured0) {
        let reason = no_authority_reason(applied_db0 < 0.0);
        dev.write(base, false)?;
        return Ok(Correction {
            lufs: measured0,
            levels: base.to_vec(),
            clamp_reason: Some(reason),
            writes: dev.writes,
        });
    }

    // Already at target, or no applied gain to read a slope from → keep levels0.
    if applied_db0.abs() <= 1e-3 || (v0 - target).abs() <= ACCEPT_TOL_LU {
        return Ok(Correction {
            lufs: v0,
            levels: levels0,
            clamp_reason: None,
            writes: 0,
        });
    }

    // Best MEASURED point. `base@measured0` competes too, so an adverse first response can't
    // be kept over where the preset started (after a headroom trade `measured0` is MODELLED —
    // `retarget_prepass_after_trade` — the same value the secant seed already trusts). Strict
    // `<`: a tie keeps the incumbent, so levels0 wins ties and costs no write.
    let mut best = (levels0.clone(), v0);
    let consider = |best: &mut (Vec<f32>, f64), levels: &[f32], v: f64| {
        if (v - target).abs() < (best.1 - target).abs() {
            *best = (levels.to_vec(), v);
        }
    };
    consider(&mut best, base, measured0);

    // CONFIRMATION PROBE — the verdict above needs a move of at least `NO_AUTHORITY_MIN_DB`
    // to be conclusive, but the first step is sized by the SOLVE, not by what a verdict
    // needs: a scene sitting 4 dB from its target gets a 4 dB step. A FLAT response to that
    // is already suspicious, and left unresolved it falls through the secant (slope ~0 →
    // stop) and reports a REASON-LESS headroom clamp on a knob that in fact has no authority
    // at all — the user is told "couldn't get there" instead of "this amp doesn't reach USB
    // 1/2", which is the one thing they could act on. So resolve it: step DOWN by a full
    // `NO_AUTHORITY_MIN_DB` and look again.
    //
    // Downward only, deliberately: a raise can clip, and a saturating limiter makes an upward
    // non-response ambiguous anyway (`no_authority_reason`). Costs ONE extra capture, and only
    // on a sound whose first real move produced nothing — a knob with authority moves ~1 LU
    // per dB, so this never fires on a healthy solve.
    let mut prev = (0.0_f64, measured0); // (applied_db, lufs) — the secant's first seed
    if (v0 - measured0).abs() < KNOB_TOL_LU && applied_db0 > -NO_AUTHORITY_MIN_DB {
        let probe_db = applied_db0 - NO_AUTHORITY_MIN_DB;
        let probe_levels = levels_for(probe_db);
        // Only meaningful if the floor actually lets the knobs travel that far; if it does
        // not, the reading stays inconclusive and the ordinary clamp is the honest answer.
        if !dev.holds(&probe_levels) {
            match dev.write(&probe_levels, true)? {
                // Conclusive: a full `NO_AUTHORITY_MIN_DB` drop moved nothing.
                Some(vp) if (vp - measured0).abs() < KNOB_TOL_LU => {
                    dev.write(base, false)?;
                    return Ok(Correction {
                        lufs: measured0,
                        levels: base.to_vec(),
                        clamp_reason: Some(no_authority_reason(true)),
                        writes: dev.writes,
                    });
                }
                // It DID move, so the knob has authority and the flat first reading was the
                // unreliable one. The probe is a genuine second point — seed the secant with
                // it instead of `base@measured0`, and put the device back where the loop
                // below expects to find it.
                Some(vp) => {
                    prev = (probe_db, vp);
                    consider(&mut best, &probe_levels, vp);
                    dev.write(&levels0, false)?;
                }
                // Capture dropped — no verdict either way; the device holds the probe, so
                // the land-on-best below puts it back.
                None => {}
            }
        }
    }

    // Bounded secant. Seed points: `prev` (base, or the probe above) and levels0@v0.
    let mut last = (applied_db0, v0);
    for _ in 0..MEASURE_CORRECT_MAX {
        if (last.1 - target).abs() <= ACCEPT_TOL_LU {
            break;
        }
        // Trust-region-clamped secant step (None ⇒ slope too flat / non-finite → stop).
        let Some(next_db) = secant_step(
            prev,
            last,
            target,
            Flatness::AppliedDb,
            Some(BATCH_TRUST_DB as f64),
        ) else {
            break;
        };
        let next_levels = levels_for(next_db);
        if dev.holds(&next_levels) {
            break; // pinned — stepping changes nothing
        }
        let Some(vn) = dev.write(&next_levels, true)? else {
            break; // capture failed — land on best below
        };
        consider(&mut best, &next_levels, vn);
        prev = last;
        last = (next_db, vn);
    }

    // Land on best: persist the best point if the device isn't already there (the
    // apply_levels-saves-the-last-write fix). No verify needed — best.1 is known.
    if !dev.holds(&best.0) {
        dev.write(&best.0, false)?;
    }
    Ok(Correction {
        lufs: best.1,
        levels: best.0,
        clamp_reason: None,
        writes: dev.writes,
    })
}

/// Hedged message for a no-authority scene. A DOWNWARD move that gets no response is
/// near-conclusive off-branch (attenuating below any limiter still passes ~1:1); an UPWARD
/// one is ambiguous (a hard limiter saturates identically to an absent path).
pub(super) fn no_authority_reason(downward: bool) -> String {
    let cause = if downward {
        "it is routed to a different output"
    } else {
        "it is likely routed to a different output (or hard-limited downstream)"
    };
    format!(
        "changing this amp's outputLevel did not move the USB 1/2 capture — {cause}; \
         route it to USB 1/2 or level it manually"
    )
}

/// Pure: a no-authority verdict — a large applied gain (`|applied_db| ≥ NO_AUTHORITY_MIN_DB`)
/// produced almost no loudness `response` (`< KNOB_TOL_LU`), so the knob doesn't reach the
/// captured output. A small applied gain is inconclusive (a headroom clamp), so it's `false`.
pub(super) fn no_authority(applied_db: f64, response: f64) -> bool {
    applied_db.abs() >= NO_AUTHORITY_MIN_DB && response.abs() < KNOB_TOL_LU
}

/// Build a successful per-scene outcome from a [`SceneSolve`] (joint-k / rebalance share this).
pub(super) fn solved_scene_outcome(
    scene_slot: u32,
    target_lufs: f64,
    s: SceneSolve,
    elapsed_ms: u128,
) -> BatchedSceneOutcome {
    BatchedSceneOutcome {
        scene_slot,
        target_lufs,
        final_lufs: Some(s.lufs),
        // The loudest lane amp's solved value (representative for the single-knob case; the
        // meaningful number for a multi-knob scene is `final_lufs`). All lanes share `k_eff`.
        final_level: s
            .levels
            .iter()
            .copied()
            .fold(None, |m, v| Some(m.map_or(v, |mx: f32| mx.max(v)))),
        clamped: s.clamped,
        windows: 1,
        writes: s.writes,
        elapsed_ms,
        failure: None,
        failure_kind: None,
        dynamic_spread_lu: Some(s.spread),
        clamp_kind: s.clamp_kind,
        clamp_reason: s.clamp_reason,
        verify_by_ear: s.verify_by_ear,
        persist_mismatch: None,
        verification: s.verification,
    }
}

/// Build a failed/skipped per-scene outcome: the refusal's text and its kind.
pub(super) fn failed_scene_outcome(
    scene_slot: u32,
    target_lufs: f64,
    failure: Refusal,
    elapsed_ms: u128,
) -> BatchedSceneOutcome {
    BatchedSceneOutcome {
        scene_slot,
        target_lufs,
        final_lufs: None,
        final_level: None,
        clamped: false,
        windows: 0,
        writes: 0,
        elapsed_ms,
        failure_kind: Some(failure.kind),
        failure: Some(failure.reason),
        dynamic_spread_lu: None,
        // A FAILED row is not a clamped row: it never produced a verdict to name.
        ..Default::default()
    }
}

#[cfg(test)]
mod no_signal_scene_solve_tests {
    use super::*;

    /// A silent as-is scene capture is the SAME routing clamp the base and footswitch lanes
    /// report: the shared "no signal on USB 1/2" reason, kind `NoAuthority`, the silent
    /// sentinel, nothing written, and every knob left at its authored value.
    #[test]
    fn a_silent_scene_is_the_shared_routing_clamp_and_writes_nothing() {
        let s = no_signal_scene_solve(vec![0.0, 0.0]);
        assert!(s.clamped);
        assert_eq!(s.clamp_reason.as_deref(), Some("no signal on USB 1/2"));
        assert_eq!(s.clamp_reason.as_deref(), Some(NO_SIGNAL_REASON));
        assert_eq!(
            s.clamp_kind,
            Some(crate::headroom_trade::ClampKind::NoAuthority)
        );
        assert_eq!(s.lufs, MUTE_FLOOR_SILENT_LUFS);
        assert_eq!(s.levels, vec![0.0, 0.0]);
        assert_eq!(s.writes, 0);
        assert_eq!(s.verification, Verification::Predicted);
        assert!(s.pinned.is_none() && !s.verify_by_ear);
    }
}

#[cfg(test)]
mod scene_handle_asis_seed_tests {
    use super::*;
    use crate::session::BASE_SCENE_SLOT;

    /// Base: amp outputLevel 0.5, pedal volume 1.0. Scene 0 overlays the amp's outputLevel
    /// (0.3, `Full`); scene 1's amp overlay is bypass-only; no scene mentions the pedal.
    fn saved() -> serde_json::Value {
        serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "nodeId": "amp", "FenderId": "ACD_TwinReverb65NoFx",
                  "dspUnitParameters": { "outputLevel": 0.5, "bypass": false } },
                { "nodeId": "ped", "FenderId": "ACD_KingOfTone",
                  "dspUnitParameters": { "volume": 1.0, "bypass": false } }
            ] } },
            "scenes": [
                { "guitarNodes": { "G1": {
                    "amp": { "dspUnitParameters": { "outputLevel": 0.3 } } } } },
                { "guitarNodes": { "G1": {
                    "amp": { "dspUnitParameters": { "bypass": true } } } } }
            ]
        })
    }

    fn handle_job(scene_slot: u32, node: &str, param: &str, current: f32) -> SceneJob {
        let knob_scene = (scene_slot < BASE_SCENE_SLOT).then_some(scene_slot);
        SceneJob {
            scene_slot,
            target_lufs: -23.0,
            knobs: vec![KnobTarget {
                knob: LevelKnob::Block {
                    group_id: "G1".into(),
                    node_id: node.into(),
                    parameter_id: param.into(),
                    scene_slot: knob_scene,
                },
                lo: 0.0,
                hi: 1.0,
                current,
            }],
            skip: None,
            rebalanceable: false,
            handle: Some(FsParamTarget::from_preset(&saved(), node, param)),
            prepass: None,
            force_bypass: Vec::new(),
        }
    }

    fn matches(job: &SceneJob, saved: Option<&serde_json::Value>) -> bool {
        let handle = job.handle.as_ref().expect("handle job");
        asis_seed_context_matches(job, &job.knobs[0], handle, saved)
    }

    #[test]
    fn the_job_gate_accepts_only_the_stored_effective_value_in_the_same_scene() {
        let sv = saved();
        // Scene 0 overlays the amp: its own 0.3 matches, base's 0.5 (the doc-less fallback
        // `handle_scene_job` would take) does not.
        assert!(matches(
            &handle_job(0, "amp", "outputLevel", 0.3),
            Some(&sv)
        ));
        assert!(!matches(
            &handle_job(0, "amp", "outputLevel", 0.5),
            Some(&sv)
        ));
        // A bypass-only overlay and an absent one both inherit base.
        assert!(matches(
            &handle_job(1, "amp", "outputLevel", 0.5),
            Some(&sv)
        ));
        assert!(matches(&handle_job(0, "ped", "volume", 1.0), Some(&sv)));
        // Base row: the base value, recalled as base on both seams.
        assert!(matches(
            &handle_job(BASE_SCENE_SLOT, "ped", "volume", 1.0),
            Some(&sv)
        ));
        // A knob scoped to a different scene than the job's recall never matches.
        let mut crossed = handle_job(BASE_SCENE_SLOT, "ped", "volume", 1.0);
        if let LevelKnob::Block { scene_slot, .. } = &mut crossed.knobs[0].knob {
            *scene_slot = Some(0);
        }
        assert!(!matches(&crossed, Some(&sv)));
        // No saved read, or a read that lost `scenes` (overlay presence Unknown): no seed.
        assert!(!matches(&handle_job(0, "amp", "outputLevel", 0.3), None));
        let mut truncated = sv.clone();
        truncated.as_object_mut().unwrap().remove("scenes");
        assert!(!matches(
            &handle_job(0, "amp", "outputLevel", 0.3),
            Some(&truncated)
        ));
        // Outside the handle's own bounds: never a seed the solve couldn't have probed.
        let mut hot = saved();
        hot["audioGraph"]["guitarNodes"]["G1"][1]["dspUnitParameters"]["volume"] = 1.5.into();
        assert!(!matches(&handle_job(0, "ped", "volume", 1.5), Some(&hot)));
    }

    #[test]
    fn the_batch_gate_refuses_a_trade_hold_or_another_row_on_the_same_node() {
        let base = handle_job(BASE_SCENE_SLOT, "ped", "volume", 1.0);
        let scene = handle_job(0, "amp", "outputLevel", 0.3);
        let jobs = vec![base, scene];
        // Different nodes, no hold: both rows may seed.
        assert!(asis_reuse_batch_ok(&jobs, &jobs[0], None));
        assert!(asis_reuse_batch_ok(&jobs, &jobs[1], None));
        // Another row on the same node (a base write a scene inherits): neither may.
        let jobs = vec![
            handle_job(BASE_SCENE_SLOT, "ped", "volume", 1.0),
            handle_job(0, "ped", "volume", 1.0),
        ];
        assert!(!asis_reuse_batch_ok(&jobs, &jobs[0], None));
        assert!(!asis_reuse_batch_ok(&jobs, &jobs[1], None));
        // An EARLIER row's isolation list sits in the working copy during this row's solve:
        // the later row may not seed, the earlier one still may.
        let mut isolated = handle_job(BASE_SCENE_SLOT, "ped", "volume", 1.0);
        isolated.force_bypass = vec![("G1".into(), "fx".into(), true)];
        let jobs = vec![isolated, handle_job(0, "amp", "outputLevel", 0.3)];
        assert!(asis_reuse_batch_ok(&jobs, &jobs[0], None));
        assert!(!asis_reuse_batch_ok(&jobs, &jobs[1], None));
        // A job that is not an element of the batch is refused outright.
        let stray = handle_job(0, "amp", "outputLevel", 0.3);
        assert!(!asis_reuse_batch_ok(&jobs[..1], &stray, None));
        // A landed trade has written base unsaved: no row seeds.
        let hold = TradeHold {
            preset_level: 0.8,
            writes: Vec::new(),
            force_bypass_restore: Vec::new(),
        };
        let jobs = vec![handle_job(0, "amp", "outputLevel", 0.3)];
        assert!(!asis_reuse_batch_ok(&jobs, &jobs[0], Some(&hold)));
    }

    fn reading(integrated: f64) -> lufs::Loudness {
        lufs::Loudness {
            integrated_lufs: integrated,
            short_term_max_lufs: integrated + 0.3,
            true_peak_dbtp: -6.0,
            momentary_spread_lu: 6.0,
            clipped: false,
        }
    }

    /// A level knob whose true response is exactly linear in dB.
    fn law(v: f32) -> f64 {
        -14.0 + 20.0 * f64::from(v.max(1e-6)).log10()
    }

    /// One handle row the way `handle_one_scene` drives it, device-free: the as-is capture
    /// (the prologue — counted, it is a real engage), then the solve, seeded from that capture
    /// when `reuse`. Returns (every capture's knob value, the solve).
    fn handle_row(current: f32, target: f64, reuse: bool) -> (Vec<f32>, ParamSolve) {
        let param = FsParamTarget::new("ACD_SomeBlock", "level", current);
        let mut captured: Vec<f32> = Vec::new();
        captured.push(current); // the as-is capture hears the knob where it sits
        let asis = reading(law(current));
        let seed = reuse.then_some(SeedPoint {
            value: current,
            lufs: asis.integrated_lufs,
            spread: asis.spread_lu(),
        });
        let solved = solve_param_secant(
            &[],
            target,
            None,
            seed,
            &param,
            SCENE_HANDLE_CORRECT_MAX,
            || {},
            |v| {
                captured.push(v);
                Ok(reading(law(v)))
            },
        )
        .expect("solve");
        (captured, solved)
    }

    #[test]
    fn reuse_saves_exactly_one_capture_and_lands_the_same_value() {
        // Neither the as-is point nor the fixed 0.25 seed is already on target, so both paths
        // need the law-placed seed 2, which lands exactly on this linear response.
        let (target, current) = (-30.0, 0.8);
        let (with, solved_with) = handle_row(current, target, true);
        let (without, solved_without) = handle_row(current, target, false);
        assert_eq!(
            with.len() + 1,
            without.len(),
            "reuse: {with:?} vs no reuse: {without:?}"
        );
        // The as-is point is never captured twice.
        assert_eq!(with.iter().filter(|&&v| v == current).count(), 1);
        assert_eq!(solved_with.iterations + 1, solved_without.iterations);
        assert!((solved_with.final_value - solved_without.final_value).abs() < 1e-4);
        assert!((solved_with.predicted_lufs - target).abs() <= ACCEPT_TOL_LU);
        assert!((solved_without.predicted_lufs - target).abs() <= ACCEPT_TOL_LU);
        assert!(!solved_with.clamped && !solved_without.clamped);
    }

    #[test]
    fn without_reuse_the_solve_is_unchanged() {
        // No seed: the fixed `at_fraction(0.25)` seed 1, then seed 2 — two solve captures.
        let param = FsParamTarget::new("ACD_SomeBlock", "level", 0.8);
        let (captured, solved) = handle_row(0.8, -26.0, false);
        assert_eq!(captured[1], param.at_fraction(0.25));
        assert_eq!(solved.iterations, 2);
        // A known seed never displaces the idempotency probe: with `current_value` set the
        // capture sequence and the result are identical with or without one.
        let run = |seed: Option<SeedPoint>| {
            let mut seen = Vec::new();
            let s = solve_param_secant(
                &[],
                -26.0,
                Some(0.6),
                seed,
                &param,
                SCENE_HANDLE_CORRECT_MAX,
                || {},
                |v| {
                    seen.push(v);
                    Ok(reading(law(v)))
                },
            )
            .expect("solve");
            (seen, s.final_value, s.iterations)
        };
        let decoy = SeedPoint {
            value: 0.9,
            lufs: law(0.9),
            spread: 0.3,
        };
        assert_eq!(run(None), run(Some(decoy)));
    }

    /// A knob with NO authority (a flat response) must still land on a point the solve
    /// captured itself, never on the unchanged as-is value the caller handed in — that
    /// would make the run a silent no-op (the 402 Solo shared-base gate needs the write
    /// to move). Without reuse the landing is the fixed seed 1, as before.
    #[test]
    fn a_flat_response_never_lands_on_the_reused_as_is_value() {
        let param = FsParamTarget::new("ACD_SomeBlock", "level", 0.8);
        let flat = |seed: Option<SeedPoint>| {
            let mut seen = Vec::new();
            let s = solve_param_secant(
                &[],
                -30.0,
                None,
                seed,
                &param,
                SCENE_HANDLE_CORRECT_MAX,
                || {},
                |v| {
                    seen.push(v);
                    Ok(reading(-20.0))
                },
            )
            .expect("solve");
            (seen, s)
        };
        let current = 0.8;
        let (seen, with) = flat(Some(SeedPoint {
            value: current,
            lufs: -20.0,
            spread: 0.3,
        }));
        assert!(with.clamped, "a flat response is a clamp");
        assert_ne!(with.final_value, current, "landed on the as-is value");
        assert!(
            seen.contains(&with.final_value),
            "the landing {} is a point this solve captured: {seen:?}",
            with.final_value
        );
        let (_, without) = flat(None);
        assert!(without.clamped);
        assert_eq!(without.final_value, param.at_fraction(0.25));
    }
}

#[cfg(test)]
mod batch_save_report_tests {
    use super::*;

    fn written(scene_slot: u32) -> PersistedWrite {
        PersistedWrite {
            scene_slot,
            overlay: Some(scene_slot),
            node_id: "amp".into(),
            parameter_id: "outputLevel".into(),
            value: 0.5,
        }
    }

    fn outcome(scene_slot: u32) -> BatchedSceneOutcome {
        BatchedSceneOutcome {
            scene_slot,
            ..Default::default()
        }
    }

    /// "The save didn't hold" must reach the Summary on EVERY save,
    /// the one after a Stop included. A batch save that failed (its retry too) marks every
    /// scene the batch wrote as not held; a scene that wrote nothing keeps no verdict, and
    /// nothing is verified against a save that never landed. Before, the stopped path only
    /// logged the failure and the scenes read as leveled.
    #[test]
    fn a_failed_batch_save_marks_every_written_scene_as_not_held_on_a_stop_too() {
        for stopped in [true, false] {
            let mut outcomes = vec![outcome(0), outcome(1), outcome(2)];
            let mut verified = false;
            report_batch_save(
                ListIndex(3),
                stopped,
                Err(LevelError::Device("save dropped".into())),
                &[written(0), written(2)],
                &mut outcomes,
                |_| verified = true,
            );
            assert!(!verified, "nothing to re-read after a failed save");
            let verdicts: Vec<Option<bool>> = outcomes.iter().map(|o| o.persist_mismatch).collect();
            assert_eq!(
                verdicts,
                [Some(true), None, Some(true)],
                "stopped={stopped}"
            );
        }
    }

    /// A save that landed is re-read — on a stopped run as on a finished one — and the
    /// re-read's verdict is what the outcomes carry.
    #[test]
    fn a_landed_batch_save_is_verified_on_a_stop_too() {
        for stopped in [true, false] {
            let mut outcomes = vec![outcome(0)];
            report_batch_save(
                ListIndex(3),
                stopped,
                Ok(()),
                &[written(0)],
                &mut outcomes,
                |os| os[0].persist_mismatch = Some(false),
            );
            assert_eq!(
                outcomes[0].persist_mismatch,
                Some(false),
                "stopped={stopped}"
            );
        }
    }
}
