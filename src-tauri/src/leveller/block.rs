//! Block-control leveling ([`level_preset_block`]): the closed-loop single-knob seam the
//! probe and legacy callers use.

use super::*;

/// Level `slot` to `target_lufs` by driving `knob` in a closed loop within
/// `[lo, hi]`. Loads the preset once (own connection), seeds two measurements,
/// then secant-iterates (each a fresh re-amp capture) until within `KNOB_TOL_LU`
/// or `KNOB_MAX_ITERS`. Self-contained: opens its own connections, so the caller
/// must NOT hold a device seize. `clamped` = the target needed a knob value
/// outside `[lo, hi]` (unreachable). Optionally verifies and saves.
#[allow(clippy::too_many_arguments)]
pub fn level_preset_block(
    slot: ListIndex,
    stimulus: &[f32],
    knob: &LevelKnob,
    lo: f32,
    hi: f32,
    target_lufs: f64,
    opts: LevelOptions,
    mut cancelled: impl FnMut() -> bool,
) -> Result<LevelResult, LevelError> {
    // Pre-measure cancel: no device touch yet → return without the restore wrapper.
    if cancelled() {
        return Err(LevelError::Cancelled);
    }
    let result = (|| {
        if hi <= lo {
            return Err(LevelError::Device(format!(
                "invalid knob bounds [{lo}, {hi}]"
            )));
        }
        // ONE field-8 read up front for a SCENE knob — every measure and the final apply
        // write through `set_knobs`, whose Scene Edit decision needs it (see
        // `saved_for_scene_knobs`). Read before the load session, never per capture.
        let overlays = saved_for_scene_knobs(slot, [knob]);
        let overlays = overlays.as_ref();
        // Load the preset in its own connection (the set-after-load-in-same-conn
        // override applies to any setter, so isolate the load).
        load_preset_isolated(slot, CancelMode::Cleanup)?;

        // Search in a coordinate where the knob is ~linear in LUFS so the secant
        // converges in 1–2 steps. Amplitude knobs (range within [0,1]) are linear in
        // dB-of-knob (`20·log10(x)` — the de-risk's proven `presetLevel`/`outputLevel`
        // model); dB-unit knobs (e.g. an IR `outputlevel`) are already ~linear, so
        // search them in raw units. Same log-vs-identity shape as `knob_search_space` and
        // `FsParamTarget::to_coord`/`coord_to_value`, but this seam infers log-space from
        // the BOUNDS SHAPE (no `ParamClass` available here) — see `FsParamTarget::to_coord`'s
        // doc for why the three maps stay separate rather than merged.
        let log_space = lo >= 0.0 && hi <= 1.0 + 1e-6;
        let eps = 1e-3f32;
        let to_c = |x: f32| {
            if log_space {
                20.0 * x.max(eps).log10()
            } else {
                x
            }
        };
        let from_c = |c: f32| {
            if log_space {
                10f32.powf(c / 20.0).clamp(lo, hi)
            } else {
                c.clamp(lo, hi)
            }
        };
        let c_lo = to_c(if log_space { lo.max(eps) } else { lo });
        let c_hi = to_c(hi);
        let cspan = c_hi - c_lo;

        // Seed two points inside the range (avoid the extremes), in c-space.
        let mut ca = c_lo + 0.4 * cspan;
        let mut cb = c_lo + 0.75 * cspan;
        if cancelled() {
            return Err(LevelError::Cancelled);
        }
        // Floor-guarded: the first capture characterizes the preset (spread is
        // gain-invariant), so a floor read here poisons the whole secant loop.
        // Mid-loop captures (`yb`, `ynext`) stay UNGUARDED on purpose: a preset that
        // legitimately measures near the trip gate would pay the retry on EVERY
        // iteration, and a persistent mid-loop floor lands on the secant's
        // flat-response / no-authority backstops instead of a wrong write.
        let first = require_live(
            // Standalone block-knob seam: no run-owned `presetLevel` to assert.
            || measure_knob_at(stimulus, knob, from_c(ca), &[], overlays, None),
            stimulus,
        )?;
        let dynamic_spread_lu = first.spread_lu();
        let mut ya = first.integrated_lufs;
        if cancelled() {
            return Err(LevelError::Cancelled);
        }
        let mut yb =
            measure_knob_at(stimulus, knob, from_c(cb), &[], overlays, None)?.integrated_lufs;
        let mut iterations = 2u32;

        // Track the best (closest-to-target) measured point as the result.
        let mut best = if (ya - target_lufs).abs() <= (yb - target_lufs).abs() {
            (ca, ya)
        } else {
            (cb, yb)
        };

        while iterations < KNOB_MAX_ITERS && (best.1 - target_lufs).abs() > KNOB_TOL_LU {
            let Some(raw_next) = secant_step((ca, ya), (cb, yb), target_lufs, Flatness::Knob, None)
            else {
                break; // flat response — knob can't move loudness here
            };
            let cnext = raw_next.clamp(c_lo, c_hi);
            // If the secant keeps pinning us to the same bound, we've converged to
            // the reachable extreme — stop.
            if (cnext - cb).abs() < 1e-4 {
                break;
            }
            if cancelled() {
                return Err(LevelError::Cancelled);
            }
            let ynext = measure_knob_at(stimulus, knob, from_c(cnext), &[], overlays, None)?
                .integrated_lufs;
            iterations += 1;
            if (ynext - target_lufs).abs() < (best.1 - target_lufs).abs() {
                best = (cnext, ynext);
            }
            (ca, ya, cb, yb) = (cb, yb, cnext, ynext);
        }

        let (final_level, measured_at_final) = (from_c(best.0), best.1);
        // "Clamped" = target not achieved within tolerance — the knob couldn't reach
        // it, whether because it hit a bound or its response went flat (e.g. a
        // normalvolume whose channel can't go quiet enough). This is the
        // user-meaningful "target unreachable with this knob" signal.
        let clamped = (measured_at_final - target_lufs).abs() > KNOB_TOL_LU;

        // Apply the solved value + optional verify/save. The preset is still current
        // from the initial load, so no reload — the same Conn-3 seam the one-shot path
        // uses, just with a block knob.
        if cancelled() {
            return Err(LevelError::Cancelled);
        }
        let (saved, verify_lufs) = apply_levels(
            slot,
            stimulus,
            &[(knob, final_level)],
            opts,
            false,
            overlays,
            &[],
        )?;

        Ok(LevelResult {
            slot,
            // A block-knob row is not a scene row on the wire — the shipped scene lane is
            // the BATCHED runner (`outcome_to_level_result`), which stamps the real slot.
            scene_slot: None,
            ref_level: final_level, // for a block knob, "ref" carries the solved value
            measured_lufs: measured_at_final,
            constant_c: f64::NAN, // no single-constant model for an arbitrary knob
            final_level,
            target_lufs,
            predicted_lufs: measured_at_final,
            clamped,
            saved,
            verify_lufs,
            iterations,
            dynamic_spread_lu: Some(dynamic_spread_lu),
            clamp_kind: crate::headroom_trade::ClampKind::from_flags(clamped, false, None),
            ..Default::default()
        })
    })();
    restore_after_unsaved_error(slot, opts.save, result)
}
