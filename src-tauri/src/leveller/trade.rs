//! The headroom trade and redistribution: the base `presetLevel`/`outputLevel` trade
//! ([`apply_headroom_trade`]), its isolation undo, and the clamped-headroom redistribution
//! runner ([`redistribute_clamped_headroom`]).

use super::*;

/// The common leveling target for a set of preset ceilings `cs`: the
/// loudest level *every* preset can still reach, with `headroom_lu` of margin —
/// `min(C) − headroom`. `None` if `cs` is empty. Leveling all presets to this target
/// means no loudness jump when switching presets/instruments on stage.
pub fn common_target(cs: &[f64], headroom_lu: f64) -> Option<f64> {
    // Ignore non-finite ceilings (a failed/silent capture yields NaN); an all-NaN
    // slice returns None so the caller errors out rather than solving against NaN and
    // writing a garbage presetLevel.
    cs.iter()
        .copied()
        .filter(|c| c.is_finite())
        .reduce(f64::min)
        .map(|min_c| min_c - headroom_lu)
}

/// Amp `outputLevel` a redistribution-compensated knob must stay above — never write a
/// compensating value toward deep digital silence (`outputLevel = 0` reads as silence).
pub const REDIST_MIN_KNOB: f32 = 0.05;

/// A hair of extra headroom (dB) added to the redistribution delta beyond the worst clamped
/// scene's deficit, so that scene reaches target with its `outputLevel` a touch BELOW max
/// (genuinely re-solvable → "done") instead of pinned exactly at 1.0 (an edge clamp any
/// measurement jitter tips back over). Capped by the presetLevel headroom / down-room like
/// the deficit itself, so it never over-raises past what the budget allows.
pub const REDIST_HEADROOM_MARGIN_DB: f64 = 1.0;

/// The gain-budget redistribution delta (dB, ≥ 0): raise `presetLevel` by this and
/// compensate the base amp + non-clamped scene overlays DOWN by this, so clamped scenes
/// gain headroom while non-clamped sounds stay on target (net-neutral). = the min of:
///  - `worst_clamped_deficit_db` — the loudest-short clamped scene's `target − achieved`
///    (enough to rescue the worst; lesser-clamped scenes were shorter, so all are rescued);
///  - `presetLevel` headroom `−20·log10(pl)` — can't push `pl` past 1.0;
///  - the down-room before the LOWEST compensated knob would hit [`REDIST_MIN_KNOB`]
///    (`20·log10(min_knob / REDIST_MIN_KNOB)`), so no compensation writes toward silence.
///
/// Returns 0 (⇒ don't offer / no-op) when there's no clamp, no `pl` headroom, or a
/// compensated knob already sits at/below the floor.
pub fn redistribute_delta_db(
    preset_level: f32,
    worst_clamped_deficit_db: f64,
    min_compensated_knob: f32,
) -> f64 {
    if worst_clamped_deficit_db <= 0.0 {
        return 0.0; // no clamp → nothing to redistribute
    }
    let pl_headroom = -20.0 * (preset_level.clamp(1e-6, 1.0) as f64).log10();
    let down_room = if min_compensated_knob > REDIST_MIN_KNOB {
        20.0 * (min_compensated_knob as f64 / REDIST_MIN_KNOB as f64).log10()
    } else {
        0.0
    };
    (worst_clamped_deficit_db + REDIST_HEADROOM_MARGIN_DB)
        .min(pl_headroom)
        .min(down_room)
        .max(0.0)
}

/// What [`apply_headroom_trade`] actually did to the base pair — the caller needs all of it:
/// the raised (UNSAVED) `presetLevel` has to be re-asserted at the batch's one save (the
/// pre-save scene recall reverts it otherwise, see `recall_reassert_save`), and the raise in
/// dB is what re-targets every benefiting sound's prepass reading.
#[derive(Debug, Clone, PartialEq)]
pub struct TradeApplied {
    /// dB the base `presetLevel` went UP by. Exact (module header).
    pub raise_db: f64,
    /// The raised `presetLevel`, sitting UNSAVED in the working copy. The batch runner wraps
    /// it as `Reassert::preset_level_only(Some(preset_level))` for its own save.
    pub preset_level: f32,
    /// What the base `presetLevel` was before the trade — the value the back-out restores
    /// and the anchor the Summary's "Restore original" needs.
    pub previous_preset_level: f32,
    /// The solved base amp `outputLevel`(s) holding base at its target. On a parallel merge
    /// BOTH lanes are here, scaled by the same joint factor.
    pub base_levels: Vec<f32>,
}

/// Why [`apply_headroom_trade`] backed the pair out. Carries the ONE measurement the caller
/// cannot re-derive: how far base overshot its target when the hold pinned at the fader floor,
/// which is what the single bounded re-plan
/// ([`crate::headroom_trade::replan_after_floor_pin`]) is computed from.
#[derive(Debug, Clone, PartialEq)]
pub struct TradeFailure {
    pub kind: crate::headroom_trade::ClampKind,
    pub why: String,
    /// LU base ended ABOVE its target with the base fader on [`crate::headroom_trade::
    /// BASE_FADER_FLOOR`]. `None` for every other failure — a cancel, a dropped write, a
    /// no-authority amp, or a mid-range unconverged hold affords no smaller retry.
    pub base_overshoot_lu: Option<f64>,
}

/// The UNSAVED base pair a landed headroom trade left in the working copy, handed to the
/// batch runner so its ONE save persists BOTH halves and the post-save re-read covers them.
///
/// The compensating base-amp `outputLevel`(s) are just as unsaved as the raise and just as
/// much part of the trade: leaving them out of the run's `written` list would mean
/// `verify_persisted_writes` confirmed the scenes' values while saying nothing about the pair
/// that moved every one of them.
#[derive(Debug, Clone)]
pub struct TradeHold {
    /// The raised `presetLevel` to re-assert at the save (the pre-save scene recall runs the
    /// device's own level-apply and would otherwise revert it — see `recall_reassert_save`).
    pub preset_level: f32,
    /// The solved base amp `outputLevel` writes, in `PersistedWrite` form (scene slot
    /// `BASE_SCENE_SLOT` — `persisted_value` reads those straight off the base graph).
    pub writes: Vec<PersistedWrite>,
    /// A5/F2 DETECTION: `(group_id, node_id, expected_bypass)` for every node the hold's own
    /// base job isolated — `expected_bypass` is what the SAVED document held for that node
    /// BEFORE isolation (`footswitch::block_bypassed_in_base`), i.e. what `undo_base_isolation`
    /// already wrote back. Empty when the base job carried no isolation. Threaded to
    /// `verify_persisted_writes` so a silently dropped inverse write is visible at the
    /// post-save re-read — that function checks only `node_param_f64` numeric values otherwise
    /// and a bypass bool is invisible to it.
    pub force_bypass_restore: Vec<(String, String, bool)>,
}

/// EXECUTE the benefit-aware headroom trade: raise base `presetLevel` by exactly the planned
/// dB, then SOLVE the base amp `outputLevel` back down so the base sound stays on its target.
///
/// WHY THE SECOND HALF IS A SOLVE AND NOT ARITHMETIC. The raise is exact (`headroom_trade`
/// module header); the amp fader response is not (module header: "WHAT THIS MODULE
/// DELIBERATELY DOES NOT DO"). So the hold reuses [`jointk_one_scene`]'s measure →
/// closed-form → verify → bounded-secant correction, which is the only thing in this codebase
/// allowed to decide a fader value.
///
/// PARALLEL MERGE: nothing special is needed here — `base_job` carries BOTH lane amps as
/// knobs and joint-k scales them by ONE factor, which is exactly the required "scale both
/// amps by the same dB". A single-amp trade on a merge would shift the lane blend, i.e.
/// change the tone; the joint-k machinery already forbids that.
///
/// ATOMICITY (the `PartialTrade` state). The raised `presetLevel` and the compensating
/// `outputLevel` are ONE inseparable edit — half of it persisted leaves the preset either
/// uniformly loud or uniformly quiet. So any failure of the hold backs the WHOLE pair out by
/// reloading the stored preset (which discards every unsaved working-copy write, the raise
/// included) and returns a clamp kind, never a partial success. Nothing has been saved at
/// this point, so the reload cannot destroy a persisted value; it also runs BEFORE the
/// batch's write phase, so it is not a load in front of `save_deferred_scene_writes`.
///
/// CALLER CONTRACT: the preset is already current, `plan.raise_db > 0`, and `base_job` is the
/// BASE row's job with its amp knobs at their PRE-RAISE values. `base_job.prepass` is a reading
/// taken at the OLD `presetLevel`, so it is SHIFTED by `raise_db` rather than used as-is — see
/// the seed comment on `hold_job` below for why that shift is exact and what it trades.
pub fn apply_headroom_trade(
    slot: ListIndex,
    plan: &crate::headroom_trade::TradePlan,
    preset_level: f32,
    base_job: &SceneJob,
    stimulus: &[f32],
    saved: Option<&serde_json::Value>,
    mut cancelled: impl FnMut() -> bool,
) -> Result<TradeApplied, TradeFailure> {
    use crate::headroom_trade::ClampKind;
    // Back out the WHOLE pair on failure: the reload discards the unsaved raise and every
    // unsaved fader write together, so no half-trade can ever be persisted (shared bail body,
    // `backout_pair`).
    let bail = |kind: ClampKind, why: String| -> TradeFailure {
        backout_pair(slot, "headroom_trade_backout");
        TradeFailure {
            kind,
            why,
            base_overshoot_lu: None,
        }
    };
    if cancelled() {
        return Err(TradeFailure {
            kind: ClampKind::PartialTrade,
            why: LevelError::Cancelled.to_string(),
            base_overshoot_lu: None,
        });
    }
    let raised = crate::headroom_trade::raised_preset_level(preset_level, plan.raise_db);
    // A CONNECT failure does NOT bail: nothing is written yet. A failed `set_preset_level` is
    // NOT proof the set never landed (the write may have reached the device and only its ack
    // failed), so THAT one bails, rather than leave a raise with no compensating fader.
    raise_preset_level_unsaved(
        raised,
        |e| TradeFailure {
            kind: ClampKind::PartialTrade,
            why: e,
            base_overshoot_lu: None,
        },
        |e| bail(ClampKind::PartialTrade, e),
    )?;
    // SEED THE HOLD FROM BASE'S OWN PREPASS, SHIFTED BY THE RAISE. `presetLevel` is exact
    // (module header) — the same physics `retarget_prepass_after_trade` already applies to
    // every benefiting scene — so base's as-is reading at the new level is its old one plus
    // `raise_db`, exactly. Taking
    // a fresh as-is capture instead cost one engage per hold attempt and another on the re-plan
    // retry, to re-derive a number the arithmetic gives for free.
    //
    // WHAT THIS TRADES: if the `set_preset_level` above were silently DROPPED (its ack is not
    // proof, which is why the failure path backs out), the seeded reading would be `raise_db`
    // too loud and the open-loop solve would start that far off. The hold still runs
    // `verify: true` plus the bounded-secant correction below, so the miss surfaces at the
    // POST-WRITE verify — one capture later than the prologue would have caught it — and the
    // pair is backed out either way. A base row with NO prepass falls back to `None` and the
    // prologue measures exactly as it did before.
    let hold_job = SceneJob {
        prepass: base_job.prepass.map(|p| ScenePrepass {
            asis: p.asis + plan.raise_db,
            ..p
        }),
        ..base_job.clone()
    };
    let solved = match jointk_one_scene(
        slot,
        &hold_job,
        stimulus,
        hold_job.target_lufs,
        true, // defer: the batch's ONE save persists this with everything else
        true, // verify + bounded-secant correction — the fader is not predictable
        saved,
        // ⟦4a⟧ THE HOLD'S FLOOR (danger.md: `outputLevel = 0` is deep digital silence) — the
        // solve that pays for the raise stops at `BASE_FADER_FLOOR`, reported as `TradeFloor`
        // below.
        crate::headroom_trade::BASE_FADER_FLOOR,
        // The raise above is UNSAVED, and every capture the hold takes recalls base first —
        // which runs the device's own level-apply and would revert it. Re-assert it on each
        // one, or the hold solves the base fader against the PRE-raise sound.
        Some(raised),
    ) {
        Ok(s) => s,
        // A CANCEL backs the pair out exactly like a failure does. The raise is already in
        // the working copy and the hold did not complete, so returning early would strand a
        // half-trade there for the NEXT thing that saves this preset (a save on the unit, a
        // later non-leveling flow) to persist. `restore_saved_preset` is cancel-safe by
        // design and danger.md requires the post-cancel restore to run to completion.
        Err(e) => return Err(bail(ClampKind::PartialTrade, e.to_string())),
    };
    if solved.clamped {
        // The hold FAILED: base cannot be held at its target with the raise in place. See
        // `trade_hold_failure_kind`'s own doc for the three-cause mapping.
        let kind = trade_hold_failure_kind(solved.clamp_kind, solved.pinned);
        let why = solved
            .clamp_reason
            .clone()
            .unwrap_or_else(|| kind.message().to_string());
        let mut failure = bail(kind, why);
        if kind == ClampKind::TradeFloor {
            // How far ABOVE its target base ended with the fader on the floor — the dB the
            // raise has to give back for the retry (presetLevel exact — module header).
            failure.base_overshoot_lu = Some(solved.lufs - hold_job.target_lufs);
        }
        return Err(failure);
    }
    // A5/F2: the hold landed with base's isolation still forced live in the device's working
    // copy (every capture above re-asserted it via `base_job.force_bypass`). A preset load is
    // FORBIDDEN from here on — the raise and the solved fader are UNSAVED deferred writes, and
    // "never load before `save_deferred_scene_writes`" applies literally: a reload would wipe
    // them just as surely as it undoes the isolation. So the isolation is undone with INVERSE
    // writes instead, on their own fresh connection, and a failure to do so backs the WHOLE
    // trade out rather than let the batch's terminal save persist every pedal forced off.
    //
    // This owner is LOAD-BEARING on the anchor-only path: when base arrived only as an anchor
    // (no wire job of its own) it is stripped from PHASE 3 before `run_scene_jobs` ever runs,
    // so that function's OWN pre-save isolation-restore list sees an empty batch and this call
    // is the only place left in the whole run that can clean base's isolation up. On a
    // `base_requested` run base survives into PHASE 3, where `run_scene_jobs`' own pre-save
    // guard restores it too — so this call is merely redundant-but-cheap there, not required.
    undo_isolation_or_bail(&base_job.force_bypass, saved, "trade", |msg| {
        bail(ClampKind::PartialTrade, msg)
    })?;
    Ok(TradeApplied {
        raise_db: plan.raise_db,
        preset_level: raised,
        previous_preset_level: preset_level,
        base_levels: solved.levels,
    })
}

/// A5/F2's SHARED WRITER: push a precomputed `(group, node, ORIGINAL saved bypass)` list back
/// onto the device, on its OWN fresh connection, without ever loading the preset (either
/// caller may be holding UNSAVED deferred writes — the trade's raise/fader, or the batch's own
/// solved scene values — that a load would discard).
///
/// `recall_base` FIRST is load-bearing, not decorative: the scene-context rule says a bare
/// write with no preceding recall lands in whatever scene the connection currently holds, and
/// this fresh connection holds none — an un-recalled inverse write would risk creating (or
/// polluting) a SCENE overlay for the forced node instead of restoring its BASE value.
pub(super) fn write_isolation_restore(
    restore: &[(String, String, bool)],
) -> Result<(), LevelError> {
    if restore.is_empty() {
        return Ok(());
    }
    let mut s = Session::connect()?;
    recall_base(&mut s)?;
    for (g, n, original) in restore {
        s.change_parameter_bool(g, n, "bypass", *original)?;
    }
    Ok(())
}

/// A5/F2's SHARED DERIVATION: `(group, node, ORIGINAL saved bypass)` for a set of forced-bypass
/// nodes, read straight off the SAVED document (`footswitch::block_bypassed_in_base`) — the
/// value the batch found there before isolation ever touched it, so the pedal ends up exactly
/// where the user's preset already had it, never a guessed inverse of the forced flag. This is
/// the ONE derivation shared by `undo_base_isolation` (the trade hold's own cleanup),
/// `commands::level_scenes::isolation_restore_for_batch` (the base-requested job's PHASE-3
/// cleanup) and `trade_for_batch`'s landed-trade detection — previously three independent
/// copies that had drifted on the `saved == None` case.
///
/// `saved == None` returns EMPTY, never a guessed `false` for every node: there is nothing to
/// restore TO, and a blind `false` would risk actively un-bypassing a pedal the player had
/// deliberately engaged. Leaving the isolation in place is the conservative answer —
/// `verify_persisted_writes`' own forced-bypass check still catches a leak at the post-save
/// re-read. Every caller here only reaches `None` defensively (each planner refuses its trade
/// or batch without a saved doc), so this warns once rather than failing.
pub(crate) fn isolation_restore_list(
    force_bypass: &[(String, String, bool)],
    saved: Option<&serde_json::Value>,
) -> Vec<(String, String, bool)> {
    let Some(saved_doc) = saved else {
        if !force_bypass.is_empty() {
            log::warn!(
                "isolation_restore_list: no saved document to restore {} forced bypass(es) \
                 from — returning no restore rather than guess",
                force_bypass.len()
            );
        }
        return Vec::new();
    };
    force_bypass
        .iter()
        .map(|(g, n, _forced)| {
            (
                g.clone(),
                n.clone(),
                crate::footswitch::block_bypassed_in_base(saved_doc, n),
            )
        })
        .collect()
}

/// A5/F2: reverse a base job's isolation bypass writes after a landed headroom trade hold. Thin
/// wrapper over `isolation_restore_list` (the derivation) + `write_isolation_restore` (the
/// writer) — see the former's doc for the `saved == None` policy.
pub(super) fn undo_base_isolation(
    force_bypass: &[(String, String, bool)],
    saved: Option<&serde_json::Value>,
) -> Result<(), LevelError> {
    write_isolation_restore(&isolation_restore_list(force_bypass, saved))
}

/// The base hold FAILED — name the cause. Pure, so the three-way mapping is unit-testable
/// without a device.
///
/// It reads the solve's OWN pinned-bound report ([`SceneSolve::pinned`]) rather than
/// re-deriving "is any lane on the floor?" from the levels: an author-MUTED lane sits at 0.0
/// by the player's choice, is deliberately excluded from binding the joint factor
/// ([`joint_k_floor`]) and is preserved untouched by the solve — so a levels scan calls every
/// mid-range stall on such a preset a floor pin, stamps the wrong wire cause AND burns the one
/// bounded re-plan on a raise the fader never refused. [`joint_levels_pinned`] already answers
/// the direction-aware question correctly (it ignores a bound the solve never moved a lane
/// to), so the answer is carried up rather than guessed at again here.
pub(crate) fn trade_hold_failure_kind(
    solved_kind: Option<crate::headroom_trade::ClampKind>,
    pinned: Option<PinnedBound>,
) -> crate::headroom_trade::ClampKind {
    use crate::headroom_trade::ClampKind;
    match solved_kind {
        // The amp never reached the USB 1/2 capture at all.
        Some(ClampKind::NoAuthority) => ClampKind::NoAuthority,
        // The fader genuinely ran out — the measured overshoot is what the ONE bounded
        // re-plan is computed from.
        _ if pinned == Some(PinnedBound::Floor) => ClampKind::TradeFloor,
        // Clamped MID-RANGE (or pinned at the TOP, which no base hold can pay for): the
        // bounded secant ran out of captures, which is NOT a floor. The pair IS backed out
        // and nothing is persisted, which is exactly what `PartialTrade` says.
        _ => ClampKind::PartialTrade,
    }
}

/// Re-target every job's PREPASS reading after a trade landed, so the write phase solves
/// against truth instead of pre-raise numbers.
///
/// TWO ANSWERS, and the split is the physics (both facts: `headroom_trade` module header):
/// * a BENEFITING sound (its `outputLevel` is pinned by its own scene overlay, so the
///   compensating base-fader drop misses it) gained EXACTLY `raise_db` — presetLevel is
///   exact. Its reading is shifted, no capture needed.
/// * every OTHER sound's change routes through the base FADER, whose response is not
///   algebraically predictable. Guessing it is precisely the taper model this codebase
///   refuses to build, so the reading is DROPPED and that sound's solve re-measures. The base
///   row is always dropped for the same reason (and its own hold already verified it).
///
/// `benefits` answers per scene slot; a slot it does not know is treated as NOT benefiting,
/// the conservative side (an extra capture, never a wrong number).
pub fn retarget_prepass_after_trade(
    jobs: &mut [SceneJob],
    raise_db: f64,
    benefits: impl Fn(u32) -> bool,
) {
    for job in jobs.iter_mut() {
        let keeps = job.scene_slot < crate::session::BASE_SCENE_SLOT && benefits(job.scene_slot);
        match (&mut job.prepass, keeps) {
            (Some(p), true) => p.asis += raise_db,
            (slot @ Some(_), false) => *slot = None,
            (None, _) => {}
        }
    }
}

/// Post-save spot-verify tolerance (LU): a compensated sound re-measured at the PERSISTED
/// new presetLevel that lands more than this off target is the wrong-pl-solve tell (a
/// per-scene jointk solved against a stale pl — self-consistent at solve time, wrong after
/// the save re-establishes the real pl). Advisory: the save already happened, so this
/// warns + flags for the UI's Restore, it doesn't undo.
pub(crate) const REDIST_POST_VERIFY_TOL_LU: f64 = 1.5;

/// Gain-budget redistribution runner (loud-preset clamp class, single-amp v1). Raises
/// `presetLevel` to `new_preset_level` (UNSAVED) FIRST — a pure linear multiplier, so every
/// clamped scene inherits the rise as headroom — then re-levels EVERY sound in `jobs`
/// (base at slot 8 + all FS scenes) back to its target at the new pl via `jointk_one_scene`
/// (defer). A still-clamped scene stays at `outputLevel = 1.0` (jointk reports it clamped, no
/// write); every other sound drops its `outputLevel` to hold target (no overshoot — re-leveling
/// is uniform, so a lesser-clamped scene that now overshoots is compensated too). ATOMICITY:
/// if any sound's compensating write FAILS (error / no-authority off-branch), the redistribution
/// is partial → reload to discard, save NOTHING. Otherwise ONE `saveCurrentPreset` (base recall)
/// persists the new pl + every compensated `outputLevel` together, then a post-save AUDIO
/// spot-verify re-measures one compensated sound at the persisted pl (the only check at the real
/// pl — jointk's own verify is self-consistent at solve-time and misses a wrong-pl solve).
///
/// CALLER CONTRACT (mirrors `level_scenes_oneshot`): the preset is already current (the caller
/// ran `prepass_scene_docs`), and `jobs`' knobs carry each sound's pre-raise `current` value.
#[allow(clippy::too_many_arguments)]
pub fn redistribute_clamped_headroom(
    slot: ListIndex,
    new_preset_level: f32,
    jobs: &[SceneJob],
    stimulus: &[f32],
    restore_scene: Option<u32>,
    saved: Option<&serde_json::Value>,
    mut on_scene: impl FnMut(u32, Option<&BatchedSceneOutcome>),
    // B6 (F10): "the redistribute runner shares the seam" — this runner doesn't go through
    // `run_scene_jobs` (its own hand-rolled save + audio spot-verify below), so it takes the
    // same tail-emitter shape directly rather than inheriting it.
    mut on_tail: impl FnMut(&str),
    mut cancelled: impl FnMut() -> bool,
) -> Result<Vec<BatchedSceneOutcome>, LevelError> {
    if cancelled() {
        return Err(LevelError::Cancelled);
    }
    // Raise presetLevel FIRST, UNSAVED. `measure_scene_asis` (the jointk measure) connects
    // LEAN — no `load_preset` — so this working-copy value survives every scene's fresh
    // re-amp connect (HW: unsaved writes persist across reconnects).
    {
        let mut s = Session::connect()?;
        s.set_preset_level(new_preset_level)?;
        crate::settle(Duration::from_millis(SETTLE_AFTER_SET_MS));
    }

    let mut outcomes = Vec::with_capacity(jobs.len());
    let mut stopped = false;
    for job in jobs {
        if cancelled() {
            stopped = true;
            break;
        }
        on_scene(job.scene_slot, None);
        let t0 = std::time::Instant::now();
        if let Some(reason) = &job.skip {
            log::warn!(
                "scene {} skipped before any capture: {reason}",
                job.scene_slot
            );
            let o = failed_scene_outcome(
                job.scene_slot,
                job.target_lufs,
                reason.clone(),
                t0.elapsed().as_millis(),
            );
            on_scene(job.scene_slot, Some(&o));
            outcomes.push(o);
            continue;
        }
        let result = jointk_one_scene(
            slot,
            job,
            stimulus,
            job.target_lufs,
            true,
            true,
            saved,
            LEVEL_MIN,
            // The raise above is UNSAVED until this run's one save; each scene's capture
            // recalls its scene and the recall reverts it, so re-assert it per capture.
            Some(new_preset_level),
        );
        let o = match result {
            Ok(s) => {
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
                    e.to_string(),
                    t0.elapsed().as_millis(),
                )
            }
        };
        on_scene(job.scene_slot, Some(&o));
        outcomes.push(o);
    }
    // Guaranteed fresh re-amp OFF (an interrupted capture can strand it engaged).
    reamp_off_guaranteed("redistribute");

    // ATOMICITY: a cancel or ANY failed/off-branch compensating write leaves a PARTIAL
    // redistribution — reload the stored preset to discard the unsaved pl + writes, persist
    // nothing. (A jointk-reported headroom clamp on a still-clamped scene is EXPECTED and
    // not a failure; an ERROR or a no-authority `clamp_reason` is.)
    let partial = outcomes
        .iter()
        .any(|o| o.failure.is_some() || o.clamp_reason.is_some());
    if stopped || partial {
        if let Err(e) = restore_saved_preset(slot) {
            log::warn!(
                "restore_saved_preset failed after aborted redistribution (slot {slot}): {e}"
            );
        }
        return Err(if stopped {
            LevelError::Cancelled
        } else {
            "redistribution aborted: a compensating write did not land — nothing saved".into()
        });
    }

    // ONE save — new pl + every compensated outputLevel together, original scene recalled and
    // the UNSAVED raised pl re-asserted after it (see `recall_reassert_save`).
    on_tail("Saving preset…");
    let reasserts = Reassert::preset_level_only(Some(new_preset_level));
    save_deferred_scene_writes(slot, restore_scene, &reasserts)?;

    // Post-save AUDIO spot-verify at the PERSISTED pl (the wrong-pl-solve guard). Pick a
    // compensated sound that actually moved (writes > 0); re-measure it as-is. Advisory —
    // the save already landed, so a miss WARNS (the UI offers Restore), never re-writes.
    if let Some(check) = outcomes
        .iter()
        .find(|o| o.writes > 0 && o.final_lufs.is_some())
    {
        on_tail("Verifying…");
        // The save above persisted `new_preset_level`; re-asserting it is a belt-and-braces
        // no-op so the spot-verify reads the level this run intended.
        match require_live(
            || measure_scene_asis(check.scene_slot, stimulus, Some(new_preset_level), &[]),
            stimulus,
        ) {
            Ok(l) => {
                let err = (l.integrated_lufs - check.target_lufs).abs();
                if err > REDIST_POST_VERIFY_TOL_LU {
                    log::warn!(
                        "redistribute slot={slot}: post-save spot-verify scene {} read {:.2} LUFS \
                         vs target {:.2} (Δ{:.2} > {REDIST_POST_VERIFY_TOL_LU}) — possible wrong-pl \
                         solve; Restore is available",
                        check.scene_slot, l.integrated_lufs, check.target_lufs, err
                    );
                } else {
                    log::info!(
                        "redistribute slot={slot}: post-save spot-verify scene {} on target ({:.2} LUFS)",
                        check.scene_slot, l.integrated_lufs
                    );
                }
            }
            Err(e) => log::warn!("redistribute slot={slot}: post-save spot-verify skipped ({e})"),
        }
        // The post-verify capture disengages re-amp itself; the command's run-end
        // `reamp_off_guaranteed` is the fresh-connection backstop, so no extra OFF here.
    }
    Ok(outcomes)
}

/// A5/F2 DETECTION: union two `(group, node, expected_bypass)` isolation-restore lists, deduped
/// by node — `base_restore` (already-derived, e.g. the trade hold's own expectations) wins ties,
/// since a base row carries exactly one isolation set and both sources describe the SAME nodes
/// when they overlap. Pure so the union rule is unit-testable without a device: `verify_
/// persisted_writes` must check every node EITHER source isolated, whether a trade landed, a
/// base-requested job's own solve isolated it, or (rare) both.
pub(super) fn union_isolation_restore(
    base_restore: &[(String, String, bool)],
    other_restore: &[(String, String, bool)],
) -> Vec<(String, String, bool)> {
    let mut v = base_restore.to_vec();
    for (g, n, expected) in other_restore {
        if !v.iter().any(|(_, vn, _)| vn == n) {
            v.push((g.clone(), n.clone(), *expected));
        }
    }
    v
}

#[cfg(test)]
mod union_isolation_restore_tests {
    use super::*;

    fn entry(node: &str, expected: bool) -> (String, String, bool) {
        ("G1".to_string(), node.to_string(), expected)
    }

    #[test]
    fn both_empty_is_empty() {
        assert!(union_isolation_restore(&[], &[]).is_empty());
    }

    // NO-TRADE CASE: a base-requested run with no headroom trade has nothing in the hold's
    // own list, but its OWN isolation still needs to reach the verify — the union must not
    // require a non-empty first argument to carry the second through.
    #[test]
    fn an_empty_hold_list_still_carries_the_runs_own_isolation() {
        let other = vec![entry("pedal", false)];
        assert_eq!(union_isolation_restore(&[], &other), other);
    }

    #[test]
    fn an_empty_other_list_still_carries_the_holds_isolation() {
        let base = vec![entry("pedal", true)];
        assert_eq!(union_isolation_restore(&base, &[]), base);
    }

    // Both sources describe the SAME node (a trade landed AND the base row itself isolated
    // something) — the union must not double-count it, and the base list's own expectation
    // wins rather than being silently overwritten.
    #[test]
    fn a_shared_node_is_deduped_with_the_base_lists_value_winning() {
        let base = vec![entry("pedal", true)];
        let other = vec![entry("pedal", false)];
        assert_eq!(
            union_isolation_restore(&base, &other),
            vec![entry("pedal", true)]
        );
    }

    #[test]
    fn distinct_nodes_from_both_sources_are_all_kept() {
        let base = vec![entry("a", true)];
        let other = vec![entry("b", false)];
        let got = union_isolation_restore(&base, &other);
        assert_eq!(got.len(), 2);
        assert!(got.contains(&entry("a", true)));
        assert!(got.contains(&entry("b", false)));
    }
}
