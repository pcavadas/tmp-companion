//! The parallel-amp rebalance flow: solo and lane-mute-floor measurement and the
//! equal-solo rebalance runner ([`level_scenes_rebalance`]).

use super::*;

/// Fresh-connect, set a SET of knobs (before engage), engage re-amp once, measure the
/// processed pair on the full capture — the multi-knob `measure_knob_at` used by the
/// rebalance flow to read one lane SOLO (the other muted) and the balanced combination.
/// `intended_preset_level` is re-asserted AFTER `set_knobs`, whose scene recall would
/// otherwise revert an unsaved level (see [`LevelOptions::intended_preset_level`]).
pub(super) fn measure_knobs_at(
    stimulus: &[f32],
    targets: &[(&LevelKnob, f32)],
    saved: Option<&serde_json::Value>,
    intended_preset_level: Option<f32>,
) -> Result<lufs::Loudness, LevelError> {
    let mut s = Session::connect_lean()?;
    set_knobs(&mut s, targets, saved)?;
    if let Some(pl) = intended_preset_level {
        set_knob(&mut s, &LevelKnob::PresetLevel, pl, None)?;
    }
    settle_or_cancel(SETTLE_AFTER_SET_MS)?;
    engage_measure_disengage(&mut s, stimulus)
}

/// Measure the both-lanes-muted FLOOR. `outputLevel`=0 is often DEEP silence — the ideal
/// mute — which `processed_loudness` reports as "no signal captured"; treat that as a sentinel
/// deep floor (`MUTE_FLOOR_SILENT_LUFS`), the BEST case (no bleed), rather than failing.
/// Other capture errors still propagate.
pub(super) fn measure_mute_floor(
    stimulus: &[f32],
    a: &LevelKnob,
    b: &LevelKnob,
    saved: Option<&serde_json::Value>,
    intended_preset_level: Option<f32>,
) -> Result<f64, LevelError> {
    match measure_knobs_at(
        stimulus,
        &[(a, 0.0), (b, 0.0)],
        saved,
        intended_preset_level,
    ) {
        Ok(l) => Ok(l.integrated_lufs),
        Err(e) if e.is_no_signal() => Ok(MUTE_FLOOR_SILENT_LUFS),
        Err(e) => Err(e),
    }
}

/// READ-ONLY mute-isolation diagnostic for `probe --mute-floor` (rebalance validation).
/// For a 2-amp scene, measures the combined output, the both-lanes-muted FLOOR
/// (`outputLevel`=0 on both), and each lane SOLO (other muted), and reports the
/// solo-above-floor margins. A small margin means `outputLevel`=0 isn't deep silence — the
/// muted lane bleeds into the solo, so the equal-solo rebalance balance is only approximate
/// (the combined joint-k still hits the overall target). NO SAVE (each measure reloads).
pub fn mute_floor_report(
    slot: ListIndex,
    a: &LevelKnob,
    cur_a: f32,
    b: &LevelKnob,
    cur_b: f32,
    stimulus: &[f32],
) -> Result<String, LevelError> {
    // Before the load: the field-8 read this diagnostic's scene writes need (see
    // `saved_for_scene_knobs`) — `read_saved_preset` sleeps after itself, so the load
    // session that follows still gets a spaced boundary.
    let saved = saved_for_scene_knobs(slot, [a, b]);
    let saved = saved.as_ref();
    load_preset_isolated(slot, CancelMode::Cleanup)?;
    let combined = measure_knobs_at(stimulus, &[(a, cur_a), (b, cur_b)], saved, None)?;
    let floor_lufs = measure_mute_floor(stimulus, a, b, saved, None)?;
    let a_solo = measure_knobs_at(stimulus, &[(a, cur_a), (b, 0.0)], saved, None)?;
    let b_solo = measure_knobs_at(stimulus, &[(a, 0.0), (b, cur_b)], saved, None)?;
    reamp_off_fresh("mute_floor_report");

    let silent = floor_lufs <= MUTE_FLOOR_SILENT_LUFS + 1e-6;
    let margin_a = a_solo.integrated_lufs - floor_lufs;
    let margin_b = b_solo.integrated_lufs - floor_lufs;
    let min_margin = margin_a.min(margin_b);
    let flag = !silent && min_margin < REBALANCE_BLEED_MARGIN_DB;
    let floor_disp = if silent {
        "silent (no signal — ideal mute)".to_string()
    } else {
        format!("{floor_lufs:.2} LUFS")
    };
    Ok(format!(
        "mute-floor (list index {slot})\n\
         combined (both at current): {:.2} LUFS\n\
         both muted (outputLevel=0):  {floor_disp}  ← floor\n\
         lane A solo: {:.2} LUFS\n\
         lane B solo: {:.2} LUFS\n\
         {}verify_by_ear = {flag}\n",
        combined.integrated_lufs,
        a_solo.integrated_lufs,
        b_solo.integrated_lufs,
        if silent {
            "floor is digital silence → no lane bleed → ".to_string()
        } else {
            format!(
                "min solo-above-floor margin {min_margin:.1} dB {} threshold {REBALANCE_BLEED_MARGIN_DB:.0} dB → ",
                if flag { "<" } else { ">=" }
            )
        },
    ))
}

/// Balanced per-lane levels for EQUAL solo loudness given each lane's solo ceiling `C`
/// (its captured loudness at level 1.0): the quieter-ceiling lane is pinned at 1.0 and
/// the louder lane is attenuated to match, so both stay ≤ 1.0. The absolute level is then
/// set by the joint-k pass over the combined capture, so the choice of equal point
/// (= the quieter ceiling) is just the max-headroom anchor. Pure → unit-testable.
pub(super) fn balanced_solo_levels(c_a: f64, c_b: f64) -> (f32, f32) {
    let equal_point = c_a.min(c_b);
    let la = 10f64.powf((equal_point - c_a) / 20.0).clamp(0.0, 1.0) as f32;
    let lb = 10f64.powf((equal_point - c_b) / 20.0).clamp(0.0, 1.0) as f32;
    (la, lb)
}

/// OPT-IN rebalance leveling (only on a path MERGE, never on separate outputs).
/// For each `rebalanceable` scene (≥2 lane amps that re-merge), it first equalizes the
/// two lanes' SOLO loudness (mute one, measure the other — 2 isolated captures), then
/// joint-ks the balanced pair to the target (1 combined measure + apply). Non-rebalanceable
/// scenes (series / single / split-output) fall through to the plain joint-k (`jointk_one_scene`).
/// Same signature + outcome shape as `level_scenes_oneshot`, so the command path swaps in.
///
/// HW-validated (2026-07-20, preset "Bass Dual Amps", 2-amp merge): `probe --rebalance-scenes`
/// Base @ −26 achieved −25.79 (+0.21, level 0.91); @ −23 honest clamp at −25.16/level 1.0.
/// `probe --mute-floor` confirmed the muting assumption: both lanes muted = digital silence
/// (ideal mute, zero bleed → trustworthy equal-solo balance; lane solos −31.3/−24.6), and
/// `probe --mute-floor` correctly refuses a single-amp split-output preset ("needs a 2-amp
/// merged parallel scene") — the leveller itself falls back to `jointk_one_scene` there.
/// Restores via preset reload on no-save; ends with a guaranteed re-amp OFF.
#[allow(clippy::too_many_arguments)]
pub fn level_scenes_rebalance(
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
    // Same rule and same level as `level_scenes_oneshot`: every per-scene capture recalls its
    // scene, which reverts an unsaved raise — so the held or saved level is re-asserted per
    // capture, matching the prepass (see `scene_capture_level`).
    let intended_preset_level = scene_capture_level(hold, saved);
    let result = run_scene_jobs(
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
        |job| {
            // Non-mergeable scenes: plain joint-k (nothing to rebalance), self-correcting.
            if !job.rebalanceable || job.knobs.len() < 2 {
                jointk_one_scene(
                    slot,
                    job,
                    stimulus,
                    job.target_lufs,
                    save,
                    true,
                    saved,
                    LEVEL_MIN,
                    intended_preset_level,
                )
            } else {
                // Rebalanceable: 2-lane equalize → joint-k. (Only the first two knobs are the
                // rebalance pair; the classifier never produces >2 for a single split.)
                rebalance_one_scene(
                    slot,
                    job,
                    stimulus,
                    job.target_lufs,
                    save,
                    true,
                    saved,
                    intended_preset_level,
                )
            }
        },
    );
    restore_after_unsaved_error(slot, save, result)
}

/// The rebalance flow for ONE mergeable scene: equalize the two lanes' solo loudness, then
/// joint-k the balanced pair to target. Returns a [`SceneSolve`] like `jointk_one_scene`,
/// plus a `verify_by_ear` flag when the lane-mute floor is too shallow to trust the balance.
#[allow(clippy::too_many_arguments)]
pub(super) fn rebalance_one_scene(
    slot: ListIndex,
    job: &SceneJob,
    stimulus: &[f32],
    target_lufs: f64,
    defer: bool,
    verify: bool,
    saved: Option<&serde_json::Value>,
    // The run's own `presetLevel`, re-asserted on EVERY capture below (solos, floor, combined,
    // first apply, correction) — see [`LevelOptions::intended_preset_level`]. Each recalls the
    // scene, which reverts an unsaved level; asserting it on only some captures skews the secant.
    intended_preset_level: Option<f32>,
) -> Result<SceneSolve, LevelError> {
    // 0. The scene AS-IS, exactly as the joint-k and handle lanes open (the prepass reading
    // when the batch paid it, else one isolated capture): a SILENT scene — e.g. both lane amps
    // saved at `outputLevel` 0, deep digital silence on the unit — is the shared "no signal on
    // USB 1/2" routing clamp with every knob left at its authored value, never a failed row.
    // The solos below cannot tell: they clamp each lane's level up to 1e-3 before measuring.
    if let Err(silent) = scene_prologue_or_silent(job, stimulus, intended_preset_level, || {
        job.knobs.iter().map(|kt| kt.current).collect()
    })? {
        return Ok(silent);
    }
    let a = &job.knobs[0];
    let b = &job.knobs[1];
    let cur_a = a.current.clamp(1e-3, 1.0);
    let cur_b = b.current.clamp(1e-3, 1.0);

    // 1+2. Each lane SOLO (the other muted to 0) at its current level → solo ceiling C.
    // Solo captures feed the per-lane model constants (c_a/c_b) with no verify
    // backstop downstream — floor-guarded like the combined measurement below.
    let la_solo = require_live(
        || {
            measure_knobs_at(
                stimulus,
                &[(&a.knob, cur_a), (&b.knob, 0.0)],
                saved,
                intended_preset_level,
            )
        },
        stimulus,
    )?;
    let lb_solo = require_live(
        || {
            measure_knobs_at(
                stimulus,
                &[(&a.knob, 0.0), (&b.knob, cur_b)],
                saved,
                intended_preset_level,
            )
        },
        stimulus,
    )?;
    let c_a = la_solo.integrated_lufs - 20.0 * (cur_a as f64).log10();
    let c_b = lb_solo.integrated_lufs - 20.0 * (cur_b as f64).log10();

    // 2b. Mute FLOOR: BOTH lanes at 0. `outputLevel`=0 may floor near ~−40 dB (not −∞), so a
    // "solo" carries the muted lane's bleed when the lanes sit within ~`REBALANCE_BLEED_MARGIN_DB`.
    // If so, the equal-solo balance is only approximate (the combined joint-k still hits the
    // overall target) → flag the scene "verify by ear". One extra capture; rebalance is opt-in.
    // A SILENT floor (deep mute) is the best case → huge margin → no flag.
    let floor_lufs = measure_mute_floor(stimulus, &a.knob, &b.knob, saved, intended_preset_level)?;
    let min_solo = la_solo.integrated_lufs.min(lb_solo.integrated_lufs);
    let verify_by_ear = (min_solo - floor_lufs) < REBALANCE_BLEED_MARGIN_DB;

    // 3. Balanced levels for equal solo loudness.
    let (la_bal, lb_bal) = balanced_solo_levels(c_a, c_b);

    // 4. Measure the COMBINED output at the balanced levels (correlation-real sum).
    // Floor-guarded: both lanes live at balanced levels must produce a lively capture
    // (the DELIBERATE floor measurement above is measure_mute_floor — never guarded).
    let combined = require_live(
        || {
            measure_knobs_at(
                stimulus,
                &[(&a.knob, la_bal), (&b.knob, lb_bal)],
                saved,
                intended_preset_level,
            )
        },
        stimulus,
    )?;
    let spread = combined.spread_lu();

    // 5. Joint-k the balanced pair to target (scale both by one k from the combined point).
    let balanced_knobs = vec![
        KnobTarget {
            knob: a.knob.clone(),
            lo: a.lo,
            hi: a.hi,
            current: la_bal,
        },
        KnobTarget {
            knob: b.knob.clone(),
            lo: b.lo,
            hi: b.hi,
            current: lb_bal,
        },
    ];
    let JointK {
        levels,
        clamped: _, // the FINAL point decides clamped (see below), not the open-loop want
        achieved,
        k_eff,
    } = solve_joint_k_at(
        &balanced_knobs,
        target_lufs,
        combined.integrated_lufs,
        LEVEL_MIN,
    )?;

    // 6. Apply the final balanced+scaled levels (reload discards the temporary mutes), then the
    // bounded secant correction — the balanced pair feeds the same downstream chain (e.g. a
    // post-merge compressor), so the open-loop solve undershoots there exactly like joint-k.
    let opts = LevelOptions {
        verify,
        defer,
        intended_preset_level,
        ..Default::default()
    };
    let knob_refs = [&a.knob, &b.knob];
    let base = [la_bal, lb_bal];
    let expected_db = 20.0 * k_eff.max(1e-9).log10();
    let (v0, retry_writes) = apply_first_verified(
        slot,
        stimulus,
        &knob_refs,
        &levels,
        opts,
        expected_db,
        combined.integrated_lufs,
        saved,
        &job.force_bypass,
    )?;
    let (best_lufs, best_levels, clamp_reason, corr_writes) = match v0 {
        Some(v0) if verify => {
            let c = correct_iter(
                slot,
                stimulus,
                &knob_refs,
                &base,
                levels,
                combined.integrated_lufs,
                v0,
                target_lufs,
                defer,
                saved,
                LEVEL_MIN,
                intended_preset_level,
                &job.force_bypass,
            )?;
            (c.lufs, c.levels, c.clamp_reason, c.writes)
        }
        _ => (v0.unwrap_or(achieved), levels, None, 0),
    };
    let verification = Verification::of(verify, v0);
    // Clamped from the FINAL point, not the open-loop want (see `jointk_one_scene`): a
    // verify+correct that landed within tolerance means the lane reached target — "done".
    let clamped = clamp_reason.is_some() || (best_lufs - target_lufs).abs() > ACCEPT_TOL_LU;
    let (pinned, clamp_kind) = joint_clamp_report(
        &best_levels,
        &base,
        LEVEL_MIN,
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
        writes: 1 + retry_writes + corr_writes,
        clamp_reason,
        verify_by_ear,
        verification,
    })
}
