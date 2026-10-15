//! The base (`presetLevel`) lane: [`solve_level`] / [`apply_level`], [`level_preset`] with
//! its boost and isolation handling, and [`level_setlist`].

use super::*;

/// `captured_LUFS = 20·log10(level) + C` — THE leveling physics identity (`CLAUDE.md`, "How
/// leveling works"). ONE definition so the solver, the boost planner, its executor and the
/// idempotency band cannot round it four different ways: the planner and executor recompute
/// the SAME base as-is loudness, and a drift between them would move a boost's split with no
/// lane noticing. `level` is floored at `1e-6`.
pub(crate) fn lufs_at_level(c: f64, level: f32) -> f64 {
    20.0 * (level.max(1e-6) as f64).log10() + c
}

/// Solve the `presetLevel` that hits `target_lufs` given `C`. Returns
/// `(final_level clamped 0..1, clamped, predicted_lufs)`.
///
/// The clamp test carries NO epsilon on purpose — the raw "did the ideal level land outside
/// `[0, 1]`" question; a lane that needs an acceptance band applies its own (see
/// [`already_on_target`]). `clamped` is a DECISION input, not a label: it buckets the
/// Summary's "Re-level clamped…" action and clears `allGood`, so banding it here would be
/// product-visible.
pub fn solve_level(c: f64, target_lufs: f64) -> (f32, bool, f64) {
    let ideal = 10f64.powf((target_lufs - c) / 20.0);
    let clamped = ideal > LEVEL_MAX as f64 || ideal < LEVEL_MIN as f64;
    let final_level = (ideal as f32).clamp(LEVEL_MIN, LEVEL_MAX);
    let predicted = lufs_at_level(c, final_level);
    (final_level, clamped, predicted)
}

/// Conn 3 seam: set `knob`=`final_level` on a fresh connection, optionally verify
/// (fresh re-amp capture) and save. With `save=false`, reloads the stored preset
/// after verification so the TMP edit buffer does not remain dirty. Returns
/// `(saved, verify_lufs)`.
///
/// `reload_preset` controls whether the preset is re-loaded first: the
/// single-preset and block paths leave it `false` (the preset is still current
/// from the prior load — exactly the validated 3-connection sequence); the setlist
/// path sets it `true` because measuring other presets has since changed which
/// preset is current. The scene runners (`jointk_one_scene`/rebalance) also leave
/// it `false` — their runner loads the preset once up front and nothing between
/// applies changes it; a reload per apply was pure churn the user SAW (the unit
/// flashing back to the preset between every scene write).
pub fn apply_level(
    slot: ListIndex,
    stimulus: &[f32],
    knob: &LevelKnob,
    final_level: f32,
    opts: LevelOptions,
    reload_preset: bool,
) -> Result<(bool, Option<f64>), LevelError> {
    apply_levels(
        slot,
        stimulus,
        &[(knob, final_level)],
        opts,
        reload_preset,
        None,
        &[],
    )
}

/// Multi-knob Conn-3 seam: set every `(knob, value)` in `targets` (all belonging to
/// the same scene) on a fresh connection — the joint-k apply for a parallel-merged
/// scene's lane amps — optionally verify (one fresh re-amp capture, latching the
/// whole set) and save. `apply_level` is the one-element case. See `apply_level`'s
/// notes on `reload_preset`.
pub fn apply_levels(
    slot: ListIndex,
    stimulus: &[f32],
    targets: &[(&LevelKnob, f32)],
    opts: LevelOptions,
    reload_preset: bool,
    saved: Option<&serde_json::Value>,
    // A4: a BASE job's isolation list — "base means base" (a scene job's is always empty; it
    // rides its own overlay instead). Written AFTER `set_knobs`/the intended-`presetLevel`
    // assert and BEFORE the settle/engage, mirroring `arm_measurement`'s "SCENE CONTEXT FIRST,
    // ISOLATION LAST" ordering (that seam is for a single `LevelKnob`; this one already has its
    // own recall via `set_knobs`, so the ordering is reproduced here rather than routed through
    // it).
    force_bypass: &[(String, String, bool)],
) -> Result<(bool, Option<f64>), LevelError> {
    if reload_preset {
        let mut s = Session::connect()?;
        s.load_preset(slot)?;
        // A verify capture needs the DSP audio fully settled; a pure write does not.
        let settle = if opts.verify {
            settle_after_load_ms()
        } else {
            SETTLE_BEFORE_WRITE_MS
        };
        crate::device_gate::settle(Duration::from_millis(settle));
    }

    let mut verify_lufs = None;
    let mut s = Session::connect()?;
    set_knobs(&mut s, targets, saved)?; // set before any re-amp engage (latched)
                                        // Re-assert the run's own `presetLevel` AFTER `set_knobs` — its scene recall runs the
                                        // device's level-apply and would otherwise leave the verify capture rendering at the
                                        // SAVED level (see `LevelOptions::intended_preset_level`). Skipped when a PresetLevel
                                        // target is in this batch: that value is the one under measurement and must win.
    if let Some(pl) = opts.intended_preset_level {
        if !targets
            .iter()
            .any(|(k, _)| matches!(k, LevelKnob::PresetLevel))
        {
            set_knob(&mut s, &LevelKnob::PresetLevel, pl, None)?;
        }
    }
    // ISOLATION LAST — after every knob/level write above, before the settle/engage below
    // (`capture_on_session`'s rule: a scene recall inside `set_knobs` would otherwise revert
    // bypasses written earlier on this connection).
    for (g, n, byp) in force_bypass {
        s.change_parameter_bool(g, n, "bypass", *byp)?;
    }
    crate::device_gate::settle(Duration::from_millis(SETTLE_AFTER_SET_MS));

    if opts.verify {
        // A failed verify is `None` (callers read it as `Verification::VerifyFailed`), never
        // a stand-in number. A Stop is NOT a failed verify: it propagates, so a cancelled
        // run can't carry on and report a prediction as this sound's result.
        verify_lufs = match engage_measure_disengage(&mut s, stimulus) {
            Ok(l) => Some(l.integrated_lufs),
            Err(e) if e.is_cancelled() => return Err(e),
            Err(e) => {
                log::warn!("apply_levels slot={slot}: verify capture failed: {e}");
                None
            }
        };
        // Re-assert after the re-amp toggle ONLY for PresetLevel targets (the
        // historical motivation). For scene knobs the re-assert is actively harmful:
        // its `load_scene` REVERTS the just-verified unsaved write, and the re-write
        // runs on a post-re-amp session that observably answers nothing (HW,
        // `probe --jointk-scenes` forensics: zero echoed fields) — a dropped re-write
        // would SAVE the reverted value. The verify capture already measured the
        // written state; save persists exactly that.
        if targets
            .iter()
            .all(|(k, _)| matches!(k, LevelKnob::PresetLevel))
        {
            let _ = set_knobs(&mut s, targets, saved);
            crate::device_gate::settle(Duration::from_millis(150));
        }
    }

    // Any unsaved `presetLevel` must ride through the pre-save recall via
    // `recall_reassert_save` (see its doc for the load-level-apply clobber this
    // guards). The re-assert value is the PresetLevel target when one is in the
    // batch; Block-knob targets need none (overlay writes persist through the
    // recall — the footswitch `switch_at_target` re-run spec proves it).
    let reassert_pl = targets
        .iter()
        .find(|(k, _)| matches!(k, LevelKnob::PresetLevel))
        .map(|(_, v)| *v);
    let reasserts = Reassert::preset_level_only(reassert_pl);
    if opts.save {
        if opts.verify {
            // A session that has toggled re-amp silently DROPS the save (HW: after the
            // verify engage/disengage, `saveCurrentPreset` on the same session persists
            // nothing — `probe --bisect-scene … save` with TMP_BISECT_SAVE_MODE=same vs
            // fresh). The written values survive in the device's working copy across
            // reconnects, so save on a FRESH connection.
            drop(s);
            let mut s2 = Session::connect()?;
            recall_reassert_save(&mut s2, slot, opts.restore_scene, &reasserts)?;
        } else {
            recall_reassert_save(&mut s, slot, opts.restore_scene, &reasserts)?;
        }
    } else if opts.defer {
        // Deferred mode: leave the write UNSAVED in the working copy — the scene
        // runner persists every accumulated overlay with ONE save at batch end
        // (`save_deferred_scene_writes`). No restore: a reload would discard it.
        drop(s);
    } else {
        drop(s);
        restore_saved_preset(slot)?;
    }
    Ok((opts.save, verify_lufs))
}

/// Is the solved `final_level` the same as the preset's already-saved `previous`
/// level, within the LU-space `KNOB_TOL_LU` band? Deliberately matches
/// `KNOB_TOL_LU` rather than a tighter ratio — a band under the ~0.12 LU measured
/// run-to-run noise would make the skip a coin flip. `previous <= 0.0` (unread or
/// nonsensical) never counts as unchanged.
pub(super) fn level_unchanged(final_level: f32, previous: f32) -> bool {
    previous > 0.0 && (20.0 * (final_level as f64 / previous as f64).log10()).abs() <= KNOB_TOL_LU
}

/// Is the preset's ALREADY-SAVED `p` already on `target_lufs`, given this run's measured `c`?
/// Banded at [`ACCEPT_TOL_LU`], the band every lane accepts on. Keep it under `KNOB_TOL_LU`:
/// evaluated at the REFERENCE level `p` it is bit-for-bit the boost planner's own `G`, which
/// fires only above `p_up + KNOB_TOL_LU`, so this band can never pre-empt a boost — keep
/// `previous_level`/`m.c` read identically to the planner's or that breaks silently. Since the
/// base reference IS `p` ([`base_ref_level`]), `lufs_at_level(c, p)` is the measured reading
/// itself, not an extrapolation.
pub(super) fn already_on_target(c: f64, p: f32, target_lufs: f64) -> bool {
    (target_lufs - lufs_at_level(c, p)).abs() <= ACCEPT_TOL_LU
}

/// Lowest saved `presetLevel` the base lane measures at: below it the capture sits near the
/// output floor, so it measures here and extrapolates instead.
pub(crate) const SAVED_REF_MIN: f32 = 0.1;

/// The base lane's reference level: the preset's SAVED `presetLevel` (clamped to
/// [`SAVED_REF_MIN`]..=1) when the caller read it, else `fallback` (`LevelOptions::ref_level`).
/// Measuring the preset as it is saved makes the capture the idempotency reading itself and,
/// on a re-run, a near-verify of the final value — with no 0.5 → final extrapolation through
/// the C-model's ~0.2 LU linearity error. A clipped reading at a hot saved level is
/// `measure_c`'s clip rescue's job.
pub(crate) fn base_ref_level(fallback: f32, previous_level: Option<f32>) -> f32 {
    previous_level
        .filter(|p| p.is_finite() && *p > 0.0)
        .map_or(fallback, |p| p.clamp(SAVED_REF_MIN, 1.0))
        .clamp(0.05, 1.0)
}

/// The boost-capable extras a caller may supply. `commands::level_preset`'s base
/// ("None"/presetLevel) arm is the only caller that fills it in — the only one that has
/// already read the saved preset and can derive a validated single-knob base amp candidate.
/// Everyone else calls [`level_preset`], which forwards a `Default` and takes the identical
/// path.
#[derive(Default)]
pub(crate) struct BoostContext<'a> {
    /// The candidate amp's job (its knob + bounds; `scene_slot: None`, isolation on
    /// `force_bypass` already resolved by the caller).
    pub base_amp: Option<SceneJob>,
    /// The same saved-preset doc the caller already read, threaded through to
    /// `jointk_one_scene`/`undo_base_isolation` exactly like every other scene-job seam takes
    /// it. v1 scopes the full continuation to scene-less presets, see `level_preset_impl`'s
    /// routing doc.
    pub saved: Option<&'a serde_json::Value>,
    /// The SAME read's own fail-safe "does this preset carry `scenes[]`?" flag — see
    /// [`scene_bearing_for_boost_gate`], which consumes it and states the hole it closes.
    pub has_fs_scenes: bool,
}

/// The v1 boost-routing gate's "is this preset scene-bearing?" signal (BUG→GATE). THE HOLE: a
/// field-8 read cut before the alphabetically-last `scenes` key yields a doc with no `scenes`
/// at all — indistinguishable, by a bare doc check, from a genuinely scene-less preset — and
/// the base arm only requires `ftsw` complete, so a scene-bearing preset would take the
/// APPLIED boost instead of the advisory. `has_fs_scenes` is the READ's own fail-safe flag
/// (`read_slot_preset_sections`: `true` on a truncated or unknown tail); the doc check is
/// OR'd in as defense in depth.
pub(crate) fn scene_bearing_for_boost_gate(
    has_fs_scenes: bool,
    saved: Option<&serde_json::Value>,
) -> bool {
    has_fs_scenes
        || saved.is_some_and(|s| {
            s.get("scenes")
                .and_then(|v| v.as_array())
                .is_some_and(|v| !v.is_empty())
        })
}

/// Level one preset to `target_lufs`. Self-contained: opens its own fresh
/// connections (load → measure → set), so the caller must NOT hold a competing
/// device seize while this runs. Composes the `measure_c` → `solve_level` →
/// `apply_level` seams. `previous_level` (the preset's currently-saved
/// `presetLevel`, when the caller already read it) enables the idempotency skip:
/// a re-run that solves the SAME level as last time AND whose saved level is itself on
/// target reloads the stored preset and returns without writing (see the
/// `level_unchanged` + [`already_on_target`] guard below — both conjuncts are load-bearing,
/// per the latter's doc), so repeat runs don't re-randomize an already-on-target preset. `None` (the probe/benchmark
/// call sites, and the setlist common-target pass) keeps the always-write behavior.
///
/// Thin compat wrapper over [`level_preset_impl`] for the legacy 6-arg (well, 7 with
/// `cancelled`) callers that have no boost context to supply — see [`BoostContext`]'s doc.
pub fn level_preset(
    slot: ListIndex,
    stimulus: &[f32],
    target_lufs: f64,
    opts: LevelOptions,
    force_bypass: &[(String, String, bool)],
    previous_level: Option<f32>,
    cancelled: impl FnMut() -> bool,
) -> Result<LevelResult, LevelError> {
    level_preset_impl(
        slot,
        stimulus,
        target_lufs,
        opts,
        force_bypass,
        previous_level,
        BoostContext::default(),
        cancelled,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn level_preset_impl(
    slot: ListIndex,
    stimulus: &[f32],
    target_lufs: f64,
    opts: LevelOptions,
    force_bypass: &[(String, String, bool)],
    previous_level: Option<f32>,
    ctx: BoostContext,
    mut cancelled: impl FnMut() -> bool,
) -> Result<LevelResult, LevelError> {
    // Pre-measure cancel: nothing has touched the device yet, so return WITHOUT the
    // restore wrapper (no needless reload).
    if cancelled() {
        return Err(LevelError::Cancelled);
    }
    let result = (|| {
        let ref_level = base_ref_level(opts.ref_level, previous_level);
        // A footswitch preset can't verify its base row (see `apply_opts` below), so its one
        // reference capture is cross-checked instead — nothing else would catch an outlier.
        let cross_check = opts.verify && !force_bypass.is_empty();
        let m = match measure_c_checked(slot, stimulus, ref_level, force_bypass, cross_check) {
            Ok(m) => m,
            // Silence == output not routed to USB 1/2 (a routing state, can happen on ANY preset):
            // report the honest "not on USB 1/2" clamp instead of a generic read failure.
            Err(e) if e.is_no_signal() => {
                // `measure_c` already set `presetLevel`/forced bypasses on the live device;
                // discard that before returning (this is an Ok result, so
                // `restore_after_unsaved_error` below never runs for it).
                if let Err(e) = restore_saved_preset(slot) {
                    log::warn!(
                        "restore_saved_preset failed after no-signal measure (slot {slot}): {e}"
                    );
                }
                return Ok(LevelResult {
                    slot,
                    scene_slot: None,
                    ref_level,
                    measured_lufs: MUTE_FLOOR_SILENT_LUFS,
                    constant_c: MUTE_FLOOR_SILENT_LUFS,
                    final_level: ref_level,
                    target_lufs,
                    predicted_lufs: MUTE_FLOOR_SILENT_LUFS,
                    clamped: true,
                    saved: false,
                    verify_lufs: None,
                    iterations: 1,
                    dynamic_spread_lu: None,
                    clamp_kind: Some(crate::headroom_trade::ClampKind::NoAuthority),
                    clamp_reason: Some(NO_SIGNAL_REASON.into()),
                    ..Default::default()
                });
            }
            Err(e) => return Err(e),
        };
        // Post-measure cancel: `measure_c` left `presetLevel` at `ref_level`; bail before
        // the apply+save. The restore wrapper reloads the stored preset (see
        // `restore_after_unsaved_error`).
        if cancelled() {
            return Err(LevelError::Cancelled);
        }
        // The level the reading was actually taken at (half the request after a clip rescue).
        let ref_level = m.ref_level;
        let (final_level, clamped, predicted) = solve_level(m.c, target_lufs);
        // Idempotency skip: the solved level matches what's already saved AND that saved level is
        // itself on target — reload to discard the measurement's ref-level edit and return without
        // writing. Both conjuncts are load-bearing: an unvalidated saved `p` above 1.0 bands to
        // zero and passes `already_on_target` alone, where `level_unchanged` correctly refuses.
        // `clamped` is deliberately NOT in the test — a re-measure flips it on float noise.
        if let Some(p) = previous_level {
            if level_unchanged(final_level, p) && already_on_target(m.c, p, target_lufs) {
                log::info!(
                    "level_preset slot={slot}: solved level within tolerance of saved ({final_level:.4} vs {p:.4}) and the saved level is already on target — skipping write"
                );
                restore_saved_preset(slot)?;
                return Ok(LevelResult {
                    slot,
                    scene_slot: None,
                    ref_level,
                    measured_lufs: m.measured_lufs,
                    constant_c: m.c,
                    final_level: p,
                    target_lufs,
                    // Reported at `p` — the level this skip KEEPS — not at `final_level`,
                    // which `solve_level` predicted for the value we are declining to write.
                    // The two coincide on a boosted preset (`p == final_level == LEVEL_MAX`)
                    // but diverge the moment the re-solve pins away from `p`, and the UI reads
                    // this field verbatim whenever `verify_lufs` is null (`useLevelingFlow`'s
                    // `valueOf`) — which is exactly this row.
                    predicted_lufs: lufs_at_level(m.c, p),
                    clamped: false,
                    saved: false,
                    verify_lufs: None,
                    iterations: 1,
                    dynamic_spread_lu: Some(m.dynamic_spread_lu),
                    // Idempotency skip: nothing was solved, so nothing clamped.
                    clamp_kind: None,
                    clamp_reason: None,
                    verify_by_ear: false,
                    previous_level: None,
                    true_peak_dbtp: Some(predicted_true_peak_dbtp(m.true_peak_dbtp, ref_level, p)),
                    ..Default::default()
                });
            }
        }
        // With forced footswitch bypasses, the device edit buffer is dirty (bypasses persist
        // across HID reconnects), so `apply_level` must reload FIRST to reset it before setting
        // only `presetLevel` and saving. And skip verify: its capture runs AFTER that reload, so
        // it would measure the un-isolated (Base + all FS blocks) state — a misleading number, and
        // re-forcing there would risk persisting the bypasses. The solve already used the
        // correctly-isolated measure_c, and the UI falls back to `predicted_lufs`.
        let mut apply_opts = opts;
        if !force_bypass.is_empty() {
            apply_opts.verify = false;
        }

        // ⟦BOOST⟧ Plan the base pair off the SAME measurement, before any write (physics in
        // `notes/leveling.md`'s BOOST section). `sounds` is empty: base has no OTHER sound's
        // clamp to redistribute against here (that is `redistribute_clamped_headroom`'s lane)
        // — this planner only asks "can base's OWN gap be closed by moving both its own
        // controls together?". `base_amp` is `Some` only from `commands::level_preset`'s base
        // arm; every other caller passes `None` and the `and_then` short-circuits.
        let boost_plan = ctx
            .base_amp
            .as_ref()
            .zip(previous_level)
            .and_then(|(job, p0)| {
                if job.skip.is_some() || job.knobs.len() != 1 {
                    return None;
                }
                let f0 = job.knobs[0].current;
                // Exact (module header): base's as-is loudness AT its CURRENTLY AUTHORED
                // `presetLevel` (p0) with the fader still at its authored value (f0) —
                // `measure_c`'s capture above moved neither.
                let a_base = lufs_at_level(m.c, p0);
                let plan = crate::headroom_trade::plan_level_pair(&[], a_base, target_lufs, p0, f0);
                (plan.boost && plan.fader_target.is_some()).then_some((job, p0, f0, plan))
            });

        // Only the ADVISORY outcome ever reaches the shared tail below — a run whose shape
        // allows the full continuation returns early from `apply_base_boost`.
        let mut base_boost: Option<crate::headroom_trade::BaseBoostSummary> = None;
        if let Some((job, p0, f0, plan)) = boost_plan {
            let LevelKnob::Block {
                group_id,
                node_id,
                parameter_id,
                ..
            } = &job.knobs[0].knob
            else {
                // Belt-and-suspenders, not an observed failure.
                return Err(LevelError::from(
                    "base boost candidate's own knob was not a Block knob (unreachable in \
                     practice — build_scene_jobs never emits this shape for an amp candidate)",
                ));
            };
            let ids = (group_id.clone(), node_id.clone(), parameter_id.clone());

            // v1 SCOPE: the full continuation only fires for a `save` run on a SCENE-LESS
            // preset; a scene preset gets the advisory below and today's honest clamp.
            // Widening needs its own plan — a boosted base's `presetLevel` raise also shifts
            // every SCENE row's captured loudness by `raise_db`, which is
            // `redistribute_clamped_headroom`'s territory, not this seam's.
            let scene_preset = scene_bearing_for_boost_gate(ctx.has_fs_scenes, ctx.saved);
            if opts.save && !scene_preset {
                return apply_base_boost(
                    slot,
                    stimulus,
                    target_lufs,
                    opts,
                    job,
                    p0,
                    &plan,
                    ids,
                    ctx.saved,
                    &m,
                    ref_level,
                    &mut cancelled,
                );
            }

            // ADVISORY: this run's shape can't apply the boost this cycle — either it has no
            // `save`, or the preset carries scenes (v1 scopes the full continuation to
            // scene-less presets). Either way the disclosure keys on `applied` alone.
            base_boost = Some(crate::headroom_trade::BaseBoostSummary::from_plan(
                false,
                &plan,
                crate::headroom_trade::TradeAmpMove {
                    group_id: ids.0,
                    node_id: ids.1,
                    parameter_id: ids.2,
                    previous_value: f0,
                    // The planner's SEED, never a prediction of what a closed-loop solve
                    // would actually land on (`LevelPairPlan::fader_target`'s own doc) — but
                    // it's exactly what the advisory disclosure needs: the plan gated
                    // `boost_plan` on `fader_target.is_some()`, so this is always populated
                    // whenever `applied:false` reaches the summary.
                    value: plan.fader_target,
                },
            ));
        }

        let (saved, mut verify_lufs) = apply_level(
            slot,
            stimulus,
            &LevelKnob::PresetLevel,
            final_level,
            apply_opts,
            !force_bypass.is_empty(),
        )?;
        // ONE bounded correction when the verify misses the acceptance band: the verify is a
        // second, independent reading of `C` at the written level, so a reference capture
        // that read low (the 2.9 LU outliers on record) no longer saves silently off target.
        // A healthy preset lands in band and pays nothing extra.
        let (mut final_level, mut clamped, mut predicted) = (final_level, clamped, predicted);
        if let Some(v) = verify_lufs.filter(|v| !clamped && (v - target_lufs).abs() > ACCEPT_TOL_LU)
        {
            if cancelled() {
                return Err(LevelError::Cancelled);
            }
            let c_verify = v - 20.0 * (final_level.max(1e-6) as f64).log10();
            let (level2, clamped2, predicted2) = solve_level(c_verify, target_lufs);
            log::warn!(
                "level_preset slot={slot}: verify read {v:.2} LUFS for target {target_lufs:.2} —                  correcting presetLevel {final_level:.4} → {level2:.4}"
            );
            let (_, v2) = apply_level(
                slot,
                stimulus,
                &LevelKnob::PresetLevel,
                level2,
                apply_opts,
                false,
            )?;
            match v2 {
                // Landed closer: keep it.
                Some(v2) if (v2 - target_lufs).abs() <= (v - target_lufs).abs() => {
                    (final_level, clamped, predicted) = (level2, clamped2, predicted2);
                    verify_lufs = Some(v2);
                }
                // Worse (the VERIFY was the outlier) or unreadable: put the first level back
                // — it was measured — and report its reading.
                _ => {
                    apply_level(
                        slot,
                        stimulus,
                        &LevelKnob::PresetLevel,
                        final_level,
                        LevelOptions {
                            verify: false,
                            ..apply_opts
                        },
                        false,
                    )?;
                }
            }
        }

        Ok(LevelResult {
            slot,
            scene_slot: None,
            ref_level,
            measured_lufs: m.measured_lufs,
            constant_c: m.c,
            final_level,
            target_lufs,
            predicted_lufs: predicted,
            clamped,
            saved,
            verify_lufs,
            verification: Verification::of(apply_opts.verify, verify_lufs),
            iterations: m.captures,
            dynamic_spread_lu: Some(m.dynamic_spread_lu),
            // The one-shot `presetLevel` lane has no wet floor and no routing clamp of its
            // own (that early-returns above), so a clamp here is the plain headroom case.
            clamp_kind: crate::headroom_trade::ClampKind::from_flags(clamped, false, None),
            clamp_reason: None,
            verify_by_ear: false,
            previous_level: None,
            true_peak_dbtp: Some(predicted_true_peak_dbtp(
                m.true_peak_dbtp,
                ref_level,
                final_level,
            )),
            persist_mismatch: None,
            trade: None,
            base_boost,
            reamp_warning: None,
        })
    })();
    restore_after_unsaved_error(slot, opts.save, result)
}

/// Back a half-landed raise-and-hold pair out: reload the stored preset (discarding the
/// unsaved raise and whatever the hold wrote, together) and guarantee re-amp OFF on a fresh
/// connection. `tag` names the executor in the log line.
pub(super) fn backout_pair(slot: ListIndex, tag: &str) {
    if let Err(e) = restore_saved_preset(slot) {
        log::warn!("restore_saved_preset failed backing out a {tag} (slot {slot}): {e}");
    }
    reamp_off_guaranteed(tag);
}

/// Undo a landed hold/boost's base isolation, backing the WHOLE pair out via `on_fail` if the
/// inverse writes themselves fail — half an undo is worse than none. No-op on an empty
/// `force_bypass`.
pub(super) fn undo_isolation_or_bail<E>(
    force_bypass: &[(String, String, bool)],
    saved: Option<&serde_json::Value>,
    what: &str,
    on_fail: impl FnOnce(String) -> E,
) -> Result<(), E> {
    if force_bypass.is_empty() {
        return Ok(());
    }
    undo_base_isolation(force_bypass, saved).map_err(|e| {
        on_fail(format!(
            "the base isolation could not be undone after the {what} landed ({e}) — backing \
             the whole {what} out rather than risk saving every isolated pedal forced off"
        ))
    })
}

/// The shared RAISE-FIRST, UNSAVED prologue — settle → connect → `set_preset_level(value)` →
/// settle. Every capture that follows connects lean-equivalent, so the working-copy value
/// survives each fresh re-amp connection (HW: unsaved writes persist across reconnects).
/// Callers map a CONNECT vs a SET failure differently, hence the two error closures.
pub(super) fn raise_preset_level_unsaved<E>(
    value: f32,
    on_connect_err: impl FnOnce(String) -> E,
    on_set_err: impl FnOnce(String) -> E,
) -> Result<(), E> {
    let mut s = Session::connect().map_err(on_connect_err)?;
    s.set_preset_level(value).map_err(on_set_err)?;
    crate::device_gate::settle(Duration::from_millis(SETTLE_AFTER_SET_MS));
    Ok(())
}

/// The BOOST full continuation: `presetLevel` to its ceiling, the base amp's fader
/// closed-loop solved for the remainder, both saved TOGETHER, then a hard-failing post-save
/// read-back of both halves. Called only from `level_preset_impl`'s routing, on a `save` run
/// over a scene-less preset. Mirrors [`apply_headroom_trade`]'s shape, differing where BOOST
/// is a single-preset move: it does its OWN save (a trade defers to a batch's terminal one),
/// and a CLAMPED solve here still PERSISTS — BOOST is the last resort, so a partial close
/// beats the honest clamp, where a trade's clamped hold backs out instead.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_base_boost(
    slot: ListIndex,
    stimulus: &[f32],
    target_lufs: f64,
    opts: LevelOptions,
    job: &SceneJob,
    previous_preset_level: f32,
    plan: &crate::headroom_trade::LevelPairPlan,
    (group_id, node_id, parameter_id): (String, String, String),
    saved: Option<&serde_json::Value>,
    m: &MeasuredC,
    ref_level: f32,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<LevelResult, LevelError> {
    let bail = |e: LevelError| -> LevelError {
        backout_pair(slot, "base_boost_backout");
        e
    };
    if cancelled() {
        return Err(bail(LevelError::Cancelled));
    }
    // A boost backs out on ANY failure here, connect included — unlike the trade's own
    // prologue below, which only bails on the SET (see that call site's comment).
    raise_preset_level_unsaved(plan.preset_level, |e| bail(e.into()), |e| bail(e.into()))?;
    // SEED THE HOLD FROM base's OWN as-is reading (already have it — `measure_c`'s capture,
    // taken before either control moved), SHIFTED by the raise: `presetLevel` is exact
    // (module header), so this costs no extra capture — the same seeding
    // `apply_headroom_trade` does for the downward trade.
    let a_base = lufs_at_level(m.c, previous_preset_level);
    let hold_job = SceneJob {
        prepass: Some(ScenePrepass {
            asis: a_base + plan.dp_db,
            spread: m.dynamic_spread_lu,
        }),
        ..job.clone()
    };
    let solved = match jointk_one_scene(
        slot,
        &hold_job,
        stimulus,
        target_lufs,
        true, // defer — this function's own save persists both halves together
        true, // verify + bounded-secant correction — the fader response isn't predictable
        saved,
        LEVEL_MIN, // an amp outputLevel lane has no wet floor; this is an UPWARD move
        Some(plan.preset_level),
    ) {
        Ok(s) => s,
        Err(e) => return Err(bail(e)),
    };
    // Undo base's isolation (if any) BEFORE the save — a preset load is forbidden from here
    // on (the raise and solved fader are unsaved deferred writes), so the isolation is undone
    // with inverse writes on their own fresh connection instead (mirrors
    // `apply_headroom_trade`'s A5/F2 cleanup).
    undo_isolation_or_bail(&job.force_bypass, saved, "boost", |e| bail(e.into()))?;
    let fader_value = solved
        .levels
        .first()
        .copied()
        .unwrap_or(job.knobs[0].current);
    // The ONLY two-control save: both halves ride through the pre-save recall.
    let reasserts = [
        Reassert::PresetLevel(plan.preset_level),
        Reassert::Param {
            group: group_id.clone(),
            node: node_id.clone(),
            param: parameter_id.clone(),
            value: fader_value,
        },
    ];
    // Unlike a plain single-knob run (whose outer `restore_after_unsaved_error` wrapper
    // skips the restore for a non-cancel `save: true` failure), this save's own failure is
    // wrapped in `bail` too: it is genuinely ambiguous whether `save_current_preset` reached
    // the device before failing, and a dirty unsaved-raise working copy left behind either
    // way is exactly the liability `apply_headroom_trade`'s own bail exists to close.
    save_deferred_scene_writes(slot, opts.restore_scene, &reasserts).map_err(bail)?;
    // POST-SAVE HARD READ-BACK: unlike `verify_persisted_writes` (advisory — a batch save
    // already landed there, so a miss only WARNS), a boost's two-control save is new and
    // untested on real hardware, so a mismatch here fails loudly instead of reporting a
    // persisted value that may not actually be on the device.
    let doc = crate::scene_jobs::read_saved_preset_complete(slot)?;
    let pl_doc = crate::audiograph::preset_level(&doc);
    let fader_doc = crate::audiograph::node_param_f64(&doc, &node_id, &parameter_id);
    let pl_ok = pl_doc.is_some_and(|v| (v - plan.preset_level as f64).abs() <= PERSIST_TOL);
    let fader_ok = fader_doc.is_some_and(|v| (v - fader_value as f64).abs() <= PERSIST_TOL);
    if !(pl_ok && fader_ok) {
        return Err(LevelError::Device(format!(
            "base boost save for slot {slot} did not persist as written (presetLevel wrote \
             {:.4} read {pl_doc:?}; {node_id}.{parameter_id} wrote {fader_value:.4} read \
             {fader_doc:?})",
            plan.preset_level
        )));
    }
    Ok(LevelResult {
        slot,
        scene_slot: None,
        ref_level,
        measured_lufs: m.measured_lufs,
        constant_c: m.c,
        final_level: plan.preset_level,
        target_lufs,
        predicted_lufs: solved.lufs,
        clamped: solved.clamped,
        saved: true,
        verify_lufs: (solved.verification == Verification::Verified).then_some(solved.lufs),
        verification: solved.verification,
        iterations: solved.writes,
        dynamic_spread_lu: Some(m.dynamic_spread_lu),
        clamp_kind: solved.clamp_kind,
        clamp_reason: None,
        verify_by_ear: solved.verify_by_ear,
        previous_level: None,
        true_peak_dbtp: Some(predicted_true_peak_dbtp(
            m.true_peak_dbtp,
            ref_level,
            plan.preset_level,
        )),
        persist_mismatch: Some(false),
        trade: None,
        base_boost: Some(crate::headroom_trade::BaseBoostSummary::from_plan(
            true,
            plan,
            crate::headroom_trade::TradeAmpMove {
                group_id,
                node_id,
                parameter_id,
                previous_value: job.knobs[0].current,
                value: Some(fader_value),
            },
        )),
        reamp_warning: None,
    })
}

#[cfg(test)]
mod scene_bearing_for_boost_gate_tests {
    use super::*;

    // The bug this gate exists to close: a field-8 read truncated before the (alphabetically
    // last) `scenes` key leaves the doc with NO `scenes` field at all — indistinguishable from
    // a genuinely scene-less preset by a bare doc check — but the read's own `has_fs_scenes`
    // flag is fail-safe `true` for exactly this "unknown" case, so the gate must still treat
    // it as scene-bearing.
    #[test]
    fn a_scenes_key_missing_from_a_truncated_read_still_gates_as_scene_bearing() {
        let truncated_doc = serde_json::json!({ "audioGraph": {} });
        assert!(scene_bearing_for_boost_gate(true, Some(&truncated_doc)));
    }

    // A COMPLETE read whose `scenes` array is genuinely empty is the one case that must NOT
    // gate — this is what lets a truly scene-less preset take the applied boost at all.
    #[test]
    fn a_confirmed_empty_scenes_array_is_not_scene_bearing() {
        let doc = serde_json::json!({ "scenes": [] });
        assert!(!scene_bearing_for_boost_gate(false, Some(&doc)));
    }

    // Defense in depth: even if a caller's `has_fs_scenes` were wrong (or absent, `None`
    // `saved`-less callers aside), a doc that actually carries `scenes[]` rows still gates.
    #[test]
    fn a_populated_scenes_array_gates_even_when_the_flag_says_otherwise() {
        let doc = serde_json::json!({ "scenes": [{ "sceneName": "Clean" }] });
        assert!(scene_bearing_for_boost_gate(false, Some(&doc)));
    }

    // No saved doc and no flag (the default `BoostContext`, every non-base caller) must not
    // gate — those callers never reach the boost-plan branch anyway (`base_amp` is always
    // `None` there), but the signal itself should still answer honestly.
    #[test]
    fn no_saved_doc_and_no_flag_is_not_scene_bearing() {
        assert!(!scene_bearing_for_boost_gate(false, None));
    }
}
