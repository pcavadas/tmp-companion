//! The footswitch (engaged-state) lane: the classified solve target ([`FsParamTarget`]), the
//! ceiling prepass, the param-space secant solve ([`measure_footswitch`]), and the
//! single-session write + save ([`write_footswitch_values`]). The batch that assembles them
//! is `commands::level_footswitch::run_footswitch_batch`.

use super::*;

// ───────────────────────── Footswitch (engaged-state) leveling ─────────────────────────
//
// Levels a footswitch's engaged state by solving the switch-ON value (`valueA`) of a
// `param` function on that switch. The leveler creates/edits ONLY a parameter-change
// assignment for the user's chosen block+param — it does NOT touch on/off. Measurement
// sweeps the chosen param (via `changeParameter`) in the preset's natural state and finds
// the value that hits target; when the footswitch is later engaged, that param jumps to the
// solved `valueA`. The param is user-chosen (any continuous `[0,1]` control), so this is a
// generic param-space secant (not the amplitude one-shot) with an honest clamp — a param on
// a bypassed/inert block measures as no-authority and clamps.

/// One footswitch-leveling outcome (mirrors the `LevelResult` subset the UI shows).
#[derive(Debug, Clone, Serialize)]
pub struct FootswitchLevelResult {
    pub switch: u32,
    /// Engaged loudness at the low reference seed (context).
    pub measured_lufs: f64,
    /// Solved switch-ON value written as the `param` function's `valueA`.
    pub final_value: f32,
    pub target_lufs: f64,
    /// Achieved engaged loudness at `final_value`.
    pub predicted_lufs: f64,
    /// The knob RAN OUT: the solved value sits at a `[0,1]` bound and target lies beyond it,
    /// so this sound cannot reach target at all. Distinct from `unconverged` below — the UI
    /// must not tell a user to retry something physically unreachable.
    pub clamped: bool,
    /// Off target but the knob still had room: the bounded secant spent its
    /// `MEASURE_CORRECT_MAX` captures mid-range (a compressed / noisy response), so the best
    /// point found is reported and a RE-RUN can improve it.
    pub unconverged: bool,
    pub clamp_reason: Option<String>,
    /// The clamp's pinned bound IS the wet/mix floor ([`WET_FLOOR_FRACTION`] × the anchor):
    /// target lies below what the effect can give without being gutted, so the write stops
    /// at the floor and the row wants a "verify by ear" advisory — the UI owns that prose.
    /// Deliberately NOT a `clamp_reason` string: that field's contract is "the leveled
    /// signal isn't reaching USB 1/2" (`.claude/rules/leveling-dsp.md`), and the UI maps ANY
    /// non-null reason to the `offbranch` outcome — a wet-floored row would be labelled a
    /// routing failure. Rides only with `clamped: true`.
    pub wet_floor: bool,
    /// The clamp's CAUSE from the shared taxonomy ([`crate::headroom_trade::ClampKind`]) —
    /// `None` when the row is not clamped. ADDITIVE alongside `clamp_reason`/`wet_floor`,
    /// which keep their documented contracts verbatim: this field exists so a consumer can
    /// TELL THE CAUSES APART without pattern-matching free text, which is exactly what the
    /// `clamp_reason`-means-off-branch rule forbids. Derived in one place
    /// ([`crate::headroom_trade::ClampKind::from_flags`]) so the footswitch, scene and
    /// handle lanes can never disagree.
    pub clamp_kind: Option<crate::headroom_trade::ClampKind>,
    pub saved: bool,
    pub verify_lufs: Option<f64>,
    pub iterations: u32,
    pub dynamic_spread_lu: Option<f64>,
    /// `"baked"` (value written straight onto the block) or `"assigned"` (param-change
    /// footswitch function written) — which simplification the leveler chose.
    pub method: String,
    /// Post-save param-level verify (see `verify_fs_persisted_writes`): `Some(true)` = the
    /// saved preset does NOT hold the value this result reports (do not trust the number);
    /// `Some(false)` = re-read and confirmed; `None` = not checked (no save, nothing written,
    /// the re-read failed, or a path without the verify). It reads the STORED row
    /// (`presetDataRequest`), which on fw 1.8.58 holds a save the moment its handler returns.
    pub persist_mismatch: Option<bool>,
    /// The run-end re-amp warning, as on [`LevelResult::reamp_warning`]. Omitted when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reamp_warning: Option<String>,
    /// Re-amp may still be on, as on [`LevelResult::reamp_still_on`].
    pub reamp_still_on: bool,
}

impl FootswitchLevelResult {
    /// Record the run-end re-amp verdict on this row: the typed flag and its text.
    pub(crate) fn stamp_reamp(&mut self, out: ReampOffOutcome) {
        self.reamp_still_on = out.still_on();
        self.reamp_warning = out.warning();
    }
}

/// How to write the leveling `param` function — resolved by the caller, which only ever EDITS
/// the switch's existing matching function at its own index (adding one is refused — see
/// `resolve_footswitch_job`), preserving its display fields.
#[derive(Debug, Clone)]
pub struct FootswitchWriteSpec {
    pub function_index: u32,
    pub color_a: u32,
    pub color_b: u32,
    pub custom_label: String,
    pub link_group: u32,
    pub is_active: bool,
    pub switch_type: u32,
}

/// How the footswitch batch (`run_footswitch_batch`) persists the solved value.
#[derive(Debug, Clone)]
pub enum FsWrite {
    /// Write a `param`-change footswitch function (`valueA`=solved, `valueB`=`value_b`).
    Assign {
        value_b: f32,
        spec: FootswitchWriteSpec,
    },
    /// Bake the solved value straight onto the block (`change_parameter`), and clear a
    /// now-redundant `param` function at `clear_stale` so the bake is the single source.
    Bake {
        clear_stale: Option<u32>,
        /// Scenes whose overlay restates the base value of the leveled param — the solved
        /// value is also written into each (scene recall + bare write; the overlay exists,
        /// so no Scene Edit enable), else the full-param overlay masks the bake whenever
        /// a scene is active. See `footswitch::FsLevelPlan::Bake::mirror_scenes`.
        mirror_scenes: Vec<u32>,
    },
}

/// The fraction of a WET/MIX parameter's AUTHORED value the footswitch solve may never go
/// below. A wet/mix control is not a volume control: driving it toward 0 to hit a loudness
/// target does not make the effect quieter, it makes the effect DISAPPEAR — the player loses
/// the sound they wrote. So the solve is floored at a quarter of the authored mix and the row
/// is reported clamped, which is an honest "this can't reach target without gutting the
/// effect" rather than a silent tone change.
pub(crate) const WET_FLOOR_FRACTION: f32 = 0.25;

/// The CLASSIFIED footswitch-solve target: which block parameter is being swept, what
/// [`crate::param_class`] says it is, and the authored (pre-solve) value the wet floor is
/// measured against. Three things ride on the classification, all decided before any device
/// work:
///
/// * [`crate::param_class::ParamClass::Other`] ⇒ `solve_footswitch` REFUSES — sweeping a
///   non-level control changes the effect, not the volume.
/// * `range` is the param's own range everywhere the FS solve reasons about knob bounds
///   (seeds, secant clamp, bracket extremes, the pinned-at-a-bound verdict). Params are not
///   all `[0,1]`: `ACD_Boost.gain` is raw dB (HW), with its own `ParamInfo.range`.
/// * [`crate::param_class::ParamClass::WetMix`] ⇒ the solved value is floored at
///   [`WET_FLOOR_FRACTION`] × `authored`.
///
/// Shared by every USER-CHOSEN-PARAM lane, not just footswitches:
/// the preset block-knob lane (`commands/level_preset.rs`) and the scene HANDLE lane
/// ([`SceneJob::handle`], `commands/level_scenes.rs`) build one too, so the refusal wording,
/// the bounds and the wet floor are identical wherever a user names a control.
///
/// The amp `outputLevel` JOINT-K path is untouched by any of this — it is the closed-form
/// amplitude solve (`solve_joint_k_at`), not a search, and keeps `scene_jobs::knob_bounds`
/// and `LEVEL_MIN`/`LEVEL_MAX`.
#[derive(Debug, Clone)]
pub struct FsParamTarget {
    /// The block's FenderId — the classifier's override key, and named in the refusal.
    pub block: String,
    pub param: String,
    pub info: crate::param_class::ParamInfo,
    /// The param's authored value before this run — the block's base value / the assign's
    /// `valueB`. Anchors the wet floor; ignored for every other class. `solve_footswitch`
    /// raises it to an existing assign's stored `valueA` itself ([`Self::anchored`]).
    pub authored: f32,
}

/// The knob value [`FsParamTarget::to_coord`]/[`FsParamTarget::coord_to_value`] floor a
/// `LevelLinear` param's log-knob coordinate at, so `coord = 20*log10(v)` stays finite at
/// `v = 0` and the inverse map never produces a real, paid capture at a knob value that's
/// audibly indistinguishable from the quiet extreme. The block-knob coordinate maps
/// (`knob_to_coord`/`coord_to_knob`, `level_preset_block`'s `to_c`/`from_c`) keep their own
/// `f32` floor.
pub(super) const KNOB_LOG_FLOOR: f64 = 1e-3;

impl FsParamTarget {
    /// Classify `param` on the block `fender_id`, anchoring the wet floor at `authored`.
    pub fn new(fender_id: &str, param: &str, authored: f32) -> Self {
        Self {
            block: fender_id.to_string(),
            param: param.to_string(),
            info: crate::param_class::classify(fender_id, param),
            authored,
        }
    }

    /// This target with the wet-floor anchor raised to `max(authored, engaged)` when
    /// `engaged` (the switch's currently-configured engaged value — an existing assign's
    /// stored `valueA`) is known and finite. The solve targets the ENGAGED state, but
    /// `authored` starts from the BASE graph value — for an existing param assign that's the
    /// switch-OFF `valueB`, not what the player actually dialed in while engaged. A
    /// hand-authored assign engaging mix 0.9 over a near-dry base 0.05 would otherwise floor
    /// at 0.0125 and could be gutted to near-silence (a chorus mix → 0) — what the floor
    /// exists to prevent. Base-anchoring stays as the FLOOR of the max: it's what
    /// keeps a re-run from ratcheting (max(base, previous solve), base intact) stable across
    /// runs. Applied by `solve_footswitch` itself from the `current_value` it already
    /// receives, so no call site can forget it.
    pub(super) fn anchored(&self, engaged: Option<f32>) -> Self {
        let mut t = self.clone();
        if let Some(v) = engaged {
            if v.is_finite() && v > t.authored {
                t.authored = v;
            }
        }
        t
    }

    /// The classifier's override key for `node_id`: the base graph's FenderId, falling back
    /// to the node id when the graph carries none. One resolution rule for every constructor
    /// below — `ACD_Boost.gain` (raw dB) and `ACD_TMRumbleV3.level` (barred) are keyed on it,
    /// so a lane that resolved it differently would classify the same control two ways.
    pub(super) fn fender_id_of(preset: &serde_json::Value, node_id: &str) -> String {
        crate::audiograph::roster_entry(preset, node_id)
            .map(|(_, _, fid)| fid)
            .unwrap_or_else(|| node_id.to_string())
    }

    /// Resolve straight off the SAVED (`presetDataRequest`) preset: the node's FenderId
    /// (falling back to the node id when the graph carries none) and its authored value for
    /// `param`. The one constructor every production call site uses — each already holds the
    /// preset that the batch's single saved-preset read produced.
    pub fn from_preset(preset: &serde_json::Value, node_id: &str, param: &str) -> Self {
        let authored =
            crate::audiograph::node_param_f64(preset, node_id, param).unwrap_or(0.0) as f32;
        Self::new(&Self::fender_id_of(preset, node_id), param, authored)
    }

    /// [`Self::from_preset`] with an explicit wet-floor anchor and the CLASS GATE folded in:
    /// resolve the FenderId off `preset`, classify, and return the shared refusal as an `Err`
    /// instead of an admissible target. The one entry point for a lane that takes a
    /// USER-NAMED control — the preset block-knob lane and the scene HANDLE lane both hold
    /// their own `authored` value (the picker's displayed value / the scene's own overlaid
    /// one, never base's), and both must refuse BEFORE any device work, so the
    /// resolve→classify→refuse sequence lives here once rather than at each call site.
    pub fn classified(
        preset: &serde_json::Value,
        node_id: &str,
        param: &str,
        authored: f32,
    ) -> Result<Self, LevelError> {
        let target = Self::new(&Self::fender_id_of(preset, node_id), param, authored);
        match target.refuse_if_not_a_level_control() {
            Some(refusal) => Err(LevelError::refused(SkipKind::UnsupportedKnob, refusal)),
            None => Ok(target),
        }
    }

    /// The refusal EVERY lane shares when the classifier doesn't recognise this target as a
    /// level or wet/mix control — `None` when it is admissible. One wording so the
    /// footswitch solve, the preset block-knob lane and the scene handle lane can't drift
    /// (and so a user who hits it in two places reads the same sentence). Always checked
    /// BEFORE any device work: sweeping a non-level control changes the sound the player
    /// wrote, not its loudness.
    pub fn refuse_if_not_a_level_control(&self) -> Option<String> {
        (self.info.class == crate::param_class::ParamClass::Other).then(|| {
            format!(
                "{} on {} is not a level control — leveling it would change the effect, not \
                 the volume",
                self.param, self.block
            )
        })
    }

    /// Does loudness rise with this param across its whole range? True for the level
    /// classes, whose law is monotone; a wet/mix blend can peak mid-range. A reading at any
    /// value of a monotone param is then a floor under its ceiling.
    pub fn is_monotone_level(&self) -> bool {
        matches!(
            self.info.class,
            crate::param_class::ParamClass::LevelLinear | crate::param_class::ParamClass::LevelDb
        )
    }

    /// The param's usable `(lo, hi)`. For a wet/mix param the LOW bound IS the wet floor
    /// (`max(range lo, `[`WET_FLOOR_FRACTION`]` × authored)`), so the whole solve — seeds,
    /// bracket extremes, secant clamp, the pinned-at-a-bound verdict — can never even PROBE
    /// a value that would gut the effect, and every reported loudness is a real reading of a
    /// writable value.
    pub fn bounds(&self) -> (f32, f32) {
        let (lo, hi) = self.info.range;
        (self.wet_floor().map_or(lo, |f| lo.max(f)), hi)
    }

    /// `frac` of the way across the param's range — how the two secant seeds and the
    /// bracket extremes are placed. `frac` 0.25/0.75 on a `[0,1]` param reproduces the
    /// validated 0.25/0.75 seeds exactly.
    pub(super) fn at_fraction(&self, frac: f32) -> f32 {
        let (lo, hi) = self.bounds();
        lo + frac * (hi - lo)
    }

    /// Seed-2 prediction from the linear law `L = coord + C` (`coord = Self::to_coord(v)`)
    /// fixed by seed 1's own `(v_a, l_a_lufs)` — exact in solve-coord space for
    /// `LevelLinear` (log-knob coord) and `LevelDb` (already ~1:1 dB→LU, identity coord);
    /// `None` for `WetMix`/`Other`, which have no known closed-form law and always take
    /// the fixed-fraction fallback. Returns the predicted raw VALUE (already mapped back
    /// via [`Self::coord_to_value`]), unclamped and un-gated for plausibility — see
    /// [`Self::seed2_plausible`] for whether it's worth spending a real capture on.
    pub(super) fn law_predicted(&self, v_a: f32, l_a_lufs: f64, target_lufs: f64) -> Option<f64> {
        matches!(
            self.info.class,
            crate::param_class::ParamClass::LevelLinear | crate::param_class::ParamClass::LevelDb
        )
        .then(|| self.coord_to_value(self.to_coord(f64::from(v_a)) + (target_lufs - l_a_lufs)))
    }

    /// Is a [`Self::law_predicted`] candidate `p` worth spending a real capture on, or
    /// should the solver fall back to the fixed complement fraction? Two independent
    /// gates: `p` must land in the central 5%–95% of the param's own range (a prediction
    /// at the very edge usually means a wrong/nonlinear law, not a genuine target), AND
    /// the expected LUFS separation `target_lufs − l_a_lufs` must clear
    /// [`FS_MIN_SEED_GAP_LU`] (see its doc for why gating in loudness space rather than
    /// knob-value space is both the correct check and still safe against a false
    /// no-authority verdict). No separate finiteness check on `p`: a non-finite prediction
    /// (NaN or ±inf from a degenerate law) fails the frac-range comparison outright —
    /// `Range::contains` is `false` for NaN against either bound — so the frac gate alone
    /// already rejects it.
    pub(super) fn seed2_plausible(&self, p: f64, l_a_lufs: f64, target_lufs: f64) -> bool {
        let (bound_lo, bound_hi) = self.bounds();
        let frac = (p - f64::from(bound_lo)) / f64::from(bound_hi - bound_lo);
        (0.05..=0.95).contains(&frac) && (target_lufs - l_a_lufs).abs() >= FS_MIN_SEED_GAP_LU
    }

    /// Solve-COORD-space value for `v`. [`LevelLinear`] interpolates in
    /// LOG-KNOB space (`coord = 20*log10(v)`, floored at `v = `[`KNOB_LOG_FLOOR`]` so the 0
    /// bound stays finite) where the documented law `L = 20*log10(v) + C` is a straight
    /// line, making the secant EXACT where a knob-space secant creeps ~1 LU/capture (HW).
    /// `LevelDb` is already ~1:1 dB→LU (raw space IS log space) and `WetMix` has no known
    /// law — both keep the identity map. (`Other` never reaches the solve.)
    ///
    /// [`LevelLinear`]: crate::param_class::ParamClass::LevelLinear
    ///
    /// Two other sites in this file build a coordinate map with the SAME log-vs-identity
    /// shape but a DIFFERENT discriminator, deliberately not merged with this one: the
    /// free functions `knob_to_coord`/`coord_to_knob` (paired with `knob_search_space`)
    /// pick log-space by BOUNDS SHAPE (`lo >= 0.0 && hi <= 1.0`) because their callers have
    /// no `ParamClass` to consult; `level_preset_block`'s local `to_c`/`from_c` closures do
    /// the same bounds-shape inference inline. Here the discriminator is the param's own
    /// classification — semantically correct where it's available — so a `LevelDb` param
    /// with a `[0, 1]`-shaped range (identity map) and a `LevelLinear` param with a wider
    /// range (log map) both solve correctly, which bounds-shape inference alone cannot
    /// distinguish.
    pub(super) fn to_coord(&self, v: f64) -> f64 {
        match self.info.class {
            crate::param_class::ParamClass::LevelLinear => 20.0 * v.max(KNOB_LOG_FLOOR).log10(),
            _ => v,
        }
    }

    /// Inverse of [`Self::to_coord`] — caller clamps to [`Self::bounds`]. Floors its
    /// `LevelLinear` output at [`KNOB_LOG_FLOOR`] (not just `to_coord`'s input): without
    /// this floor, a correction-loop coordinate below `to_coord`'s own `-60` floor
    /// (`20*log10(KNOB_LOG_FLOOR)`) would still invert to a REAL, DISTINCT-per-coordinate
    /// value below `KNOB_LOG_FLOOR` (e.g. coord `-70` → `3.16e-4`, coord `-80` → `1e-4`) —
    /// paid captures at knob values that are audibly indistinguishable from the quiet
    /// extreme, silently degenerating the secant pair instead of collapsing cleanly onto
    /// the floor `to_coord` already treats as the bottom. With the floor here, round-trip
    /// `to_coord(coord_to_value(u)) == u` holds exactly for every `u >= -60`. Never emits
    /// exactly `0.0` for `LevelLinear`; the exact-0 bound stays reachable only through
    /// [`fs_bracket_expansion`]'s v-space extreme (see its doc — one-line pointer back
    /// here for the full explanation).
    pub(super) fn coord_to_value(&self, coord: f64) -> f64 {
        match self.info.class {
            crate::param_class::ParamClass::LevelLinear => {
                10f64.powf(coord / 20.0).max(KNOB_LOG_FLOOR)
            }
            _ => coord,
        }
    }

    /// The lowest value a wet/mix solve may write, or `None` for every other class. An
    /// authored value of `0.0` (the effect is already fully dry) floors at `0.0`, i.e. no
    /// constraint — the floor is RELATIVE to what the player wrote, never an absolute 0.25.
    /// Enforced structurally: [`Self::bounds`] folds it into the low bound.
    pub(super) fn wet_floor(&self) -> Option<f32> {
        (self.info.class == crate::param_class::ParamClass::WetMix)
            .then_some(self.authored * WET_FLOOR_FRACTION)
    }

    /// Did this (clamped) solve pin at the wet floor specifically — i.e. the low bound was
    /// RAISED by the floor and `v` sits on it? Distinguishes the "verify by ear" advisory
    /// from an ordinary range-edge clamp (e.g. a dB ceiling), which needs none.
    pub(super) fn pinned_at_wet_floor(&self, v: f32) -> bool {
        self.wet_floor()
            .is_some_and(|f| f > self.info.range.0 && v <= f + 1e-6)
    }
}

/// Adopt `(v, l)` as the new best-so-far when it beats `*best_lufs`'s distance to
/// `target_lufs` — shared by `measure_footswitch`'s bracket-expansion probe and its secant
/// loop, both of which do exactly this after every extra capture.
pub(super) fn improve_best(
    target_lufs: f64,
    v: f32,
    l: &lufs::Loudness,
    best_v: &mut f32,
    best_lufs: &mut f64,
    best_spread: &mut f64,
) {
    if (l.integrated_lufs - target_lufs).abs() < (*best_lufs - target_lufs).abs() {
        *best_v = v;
        *best_lufs = l.integrated_lufs;
        *best_spread = l.spread_lu();
    }
}

/// Fraction of the param's whole coordinate span an EXTRAPOLATING secant step may always
/// take — the trust region's floor, so a small seed pair can still walk the range.
pub(super) const FS_TRUST_SPAN_FRACTION: f64 = 0.25;

/// Trust region for an extrapolating (non-straddling) footswitch secant step: `raw` is
/// clamped to `p1 ± max(2·|p1 − p0|, FS_TRUST_SPAN_FRACTION·span)` in solve-coord space.
/// A pair whose ΔLUFS is mostly capture noise has an arbitrary slope; without this its next
/// probe is clamped only by the param's bounds (the scene lane caps its own at
/// `BATCH_TRUST_DB`).
pub(super) fn fs_trust_clamp(raw: f64, p0: f64, p1: f64, span: f64) -> f64 {
    let reach = (2.0 * (p1 - p0).abs()).max(FS_TRUST_SPAN_FRACTION * span);
    raw.clamp(p1 - reach, p1 + reach)
}

/// The extreme knob value worth ONE extra probe before giving up on a FLAT `(v_a, l_a)`/
/// `(v_b, l_b)` secant seed pair — `None` when the pair already has slope (the plain
/// secant can extrapolate from it as-is, bracketed or not — unchanged from before this
/// fix) or already includes the relevant extreme (nothing left to try). The pair may
/// arrive in EITHER order (seed 1 need not be the smaller value, and a law-predicted
/// seed 2 can land on either side of it) — every check below is order-agnostic: the
/// flatness test is a symmetric `abs()` difference, `(lo_l, hi_l)` re-sorts via
/// `min`/`max` rather than trusting the argument order, and both bound checks require
/// BOTH points to clear the same side, so swapping which argument is `_a` vs `_b`
/// changes nothing about the result. A knob whose useful range is a small slice of
/// `[0, 1]` (e.g. a compressor already saturated by 0.75) can seed a pair that reads flat
/// even though a reachable, non-flat point exists further out — the fix for THAT specific
/// pathology is one more sample at 1.0 (target needs MORE loudness than either seed) or
/// 0.0 (target needs LESS), so the existing plain secant gets a genuine slope instead of
/// an honest-but-avoidable "no authority" clamp. Gated on flatness specifically (not
/// merely "unbracketed") so an ordinary out-of-bracket-but-sloped pair — which the plain
/// secant already extrapolates from correctly — doesn't pay for an extra real device
/// capture it doesn't need.
///
/// `(lo, hi)` are the PARAM's own bounds (`FsParamTarget::bounds`), not a hard-coded
/// `[0, 1]` — the extremes worth probing are the ends of the range the param actually has.
///
/// Deliberately stays in REAL v-space rather than the solve's coord space: it is
/// choosing between the actual bounds, and `FsParamTarget::coord_to_value` never emits
/// exactly `0.0` for `LevelLinear` (see its doc for why), so the exact-0 bound is
/// reachable ONLY through this v-space extreme.
pub(super) fn fs_bracket_expansion(
    v_a: f32,
    l_a: f64,
    v_b: f32,
    l_b: f64,
    target: f64,
    (lo, hi): (f32, f32),
) -> Option<f32> {
    if (l_b - l_a).abs() >= KNOB_TOL_LU {
        return None;
    }
    let (lo_l, hi_l) = (l_a.min(l_b), l_a.max(l_b));
    if target > hi_l {
        (v_a < hi && v_b < hi).then_some(hi)
    } else if target < lo_l {
        (v_a > lo && v_b > lo).then_some(lo)
    } else {
        None
    }
}

/// True if `ftsw[switch][index]` is a `param` function targeting `param` — the post-write
/// read-back confirmation (the schema has no dedicated `…Changed` echo).
pub(super) fn param_fn_present(
    ftsw: &serde_json::Value,
    switch: u32,
    index: u32,
    param: &str,
) -> bool {
    ftsw.as_array()
        .and_then(|a| a.get(switch as usize))
        .and_then(|s| s.as_array())
        .and_then(|fns| fns.get(index as usize))
        .map(|f| {
            f.get("func").and_then(|v| v.as_str()) == Some("param")
                && f.get("parameterId").and_then(|v| v.as_str()) == Some(param)
        })
        .unwrap_or(false)
}

/// One fresh-connection engaged-state measurement point for a footswitch job: recall BASE,
/// set the swept param there, force the engaged bypass list, engage re-amp once, measure —
/// the `arm_measurement` order. The recall is mandatory: a preset loads into its saved
/// `lastLoadedScene` (HW), so without it the sweep measured (and the solved value described)
/// whatever scene the preset happened to save, not the footswitch's base sound. Because the
/// recall re-asserts that scene's bypass state, the FULL isolation list is re-sent here on
/// EVERY capture — never once per batch. The forced state lives only on this throwaway
/// connection's working-copy edits; the batch write session's reload discards ALL accumulated
/// sweep pollution at once.
///
/// `scene` is the SCENE CONTEXT the switch's sound is measured in: `None` = base.
/// `Some(i)` routes through [`measure_fs_state`]'s
/// HW-validated `loadScene → write → engage` composition instead, because a scene-context
/// measurement must write on the LIVE RENDERED LAYER: the persistent, overlay-aware `set_knobs`
/// path `arm_measurement` uses would REFUSE outright on a `BypassOnly` scene, and this is a
/// throwaway measurement, not a write the preset should keep.
///
/// `intended_preset_level` is the run's OWN `presetLevel`, asserted on every capture (see
/// [`arm_measurement`] step 1b and [`measure_fs_state`]): without it the capture can render
/// at a stale base-snapshot level, because this path recalls a scene (base included) and
/// then writes a BLOCK param — it never writes `presetLevel` the way the base lane
/// incidentally does. `None` = assert nothing.
pub(crate) fn measure_fs_at(
    scene: Option<u32>,
    lev: (&str, &str, &str),
    engaged_bypass: &[(String, String, bool)],
    stimulus: &[f32],
    v: f32,
    intended_preset_level: Option<f32>,
    window: CaptureWindow,
) -> Result<lufs::Loudness, LevelError> {
    match scene {
        None => processed_loudness(capture_fs_at_in(
            lev,
            engaged_bypass,
            stimulus,
            v,
            intended_preset_level,
            window,
        )),
        Some(_) => measure_fs_state(
            scene,
            engaged_bypass,
            &[(lev.0.to_string(), lev.1.to_string(), lev.2.to_string(), v)],
            stimulus,
            intended_preset_level,
            window,
        ),
    }
}

/// [`measure_fs_at`] stopping at the raw capture — the PCM-keeping twin, same
/// connect/arm/engage sequence, for `measure_sound_asis_strict`'s validation dump.
pub(crate) fn capture_fs_at(
    lev: (&str, &str, &str),
    engaged_bypass: &[(String, String, bool)],
    stimulus: &[f32],
    v: f32,
    intended_preset_level: Option<f32>,
) -> Result<audio::Capture, LevelError> {
    capture_fs_at_in(
        lev,
        engaged_bypass,
        stimulus,
        v,
        intended_preset_level,
        CaptureWindow::Full,
    )
}

fn capture_fs_at_in(
    lev: (&str, &str, &str),
    engaged_bypass: &[(String, String, bool)],
    stimulus: &[f32],
    v: f32,
    intended_preset_level: Option<f32>,
    window: CaptureWindow,
) -> Result<audio::Capture, LevelError> {
    let mut s = Session::connect_lean()?;
    let knob = LevelKnob::Block {
        group_id: lev.0.to_string(),
        node_id: lev.1.to_string(),
        parameter_id: lev.2.to_string(),
        scene_slot: None,
    };
    arm_measurement(
        &mut s,
        &knob,
        v,
        engaged_bypass,
        None,
        intended_preset_level,
    )?;
    engage_capture_disengage_in(&mut s, stimulus, window)
}

/// ONE fresh-connection, WRITE-NOTHING-PERMANENT capture of ONE footswitch STATE — the
/// measurement shape the reordered run's FS prepass is built on
/// ([`measure_fs_ceiling`]). Same `arm_measurement` order and for the same reasons:
/// scene recall FIRST (a preset loads into its saved `lastLoadedScene`, HW — without the
/// recall the reading would describe whatever scene the preset happens to save), then the
/// state's param writes, then the isolation bypasses LAST (the recall re-asserts that
/// scene's own bypass state, so the full list is re-sent on EVERY capture), then ONE engage.
/// `params` is `(group, node, param, value)` — the switch's `param` functions at this
/// state's `valueA`/`valueB`, plus (for a ceiling read) the leveling handle at its top
/// bound. Every write lands on the throwaway connection's working copy; the command's
/// reload discards them.
///
/// `scene` is the SCENE CONTEXT the sound is measured in: `None` = base
/// (`BASE_SCENE_SLOT`), `Some(i)` = that 0-based `scenes[]` wire index. The bare
/// `change_parameter` writes below act on the LIVE RENDERED LAYER under a scene recall (a
/// scene-context write to a param in a Full overlay lands in that overlay and is heard at
/// once — static RE, fw 1.8.58), which is what makes a per-scene FS ceiling readable at
/// all. These are measurement-only writes on a throwaway connection;
/// PERSISTENT scene-scoped writes still go through the overlay-aware `set_knobs`.
///
/// `intended_preset_level` is the run's OWN `presetLevel`, written straight after the recall
/// (a base recall while base is current re-applies the base snapshot's level, see
/// `recall_reassert_save`) and before the param writes. `None` = assert nothing, i.e. the
/// capture renders at whatever level the recall leaves. No settle is added: `setPresetLevel` is
/// one more command in the run of writes the trailing `SETTLE_AFTER_SET_MS` already covers,
/// so no idle gap grows.
pub(super) fn measure_fs_state(
    scene: Option<u32>,
    bypass: &[(String, String, bool)],
    params: &[(String, String, String, f32)],
    stimulus: &[f32],
    intended_preset_level: Option<f32>,
    window: CaptureWindow,
) -> Result<lufs::Loudness, LevelError> {
    let mut s = Session::connect_lean()?;
    match scene {
        None => recall_base(&mut s)?,
        Some(sc) => {
            s.load_scene(sc)?;
            crate::device_gate::settle(Duration::from_millis(SETTLE_AFTER_SCENE_RECALL_MS));
        }
    }
    if let Some(pl) = intended_preset_level {
        set_knob(&mut s, &LevelKnob::PresetLevel, pl, None)?;
    }
    for (g, n, p, v) in params {
        s.change_parameter(g, n, p, *v)?;
    }
    for (g, n, byp) in bypass {
        s.change_parameter_bool(g, n, "bypass", *byp)?;
    }
    settle_or_cancel(pre_engage_settle_ms())?;
    engage_measure_disengage_in(&mut s, stimulus, window)
}

/// The measurement inputs for ONE footswitch sound's PREPASS CEILING read. Built by the
/// caller that owns the preset document, so this module never re-derives switch state.
pub struct FsCeilingProbe<'a> {
    /// THE SCENE CONTEXT this sound is measured in: a 0-based `scenes[]` wire slot, or
    /// `None` = the preset's BASE sound. The capture recalls it before engaging, so the ceiling
    /// describes the switch AS IT SOUNDS THERE — the same context its solve measures in, or the
    /// two describe different sounds.
    pub scene: Option<u32>,
    /// The switch's engaged/disengaged isolation + `param`-function state, straight from
    /// [`crate::footswitch::switch_states`] — the same derivation the solve uses, so a
    /// ceiling can never describe a different sound than the row it belongs to.
    pub states: &'a crate::footswitch::SwitchStates,
    /// The row's leveling handle — `(group, node)` plus its classified target. The ceiling is
    /// read with this param PINNED AT ITS TOP BOUND.
    pub handle: (String, String, FsParamTarget),
}

impl FsCeilingProbe<'_> {
    /// The `(group, node, param, value)` writes the ceiling capture sends: the switch's own
    /// `param` functions at their ENGAGED (`valueA`) values, with the LEVELING HANDLE pinned
    /// at the top of its own (classified, wet-floor-aware) range appended LAST so it wins
    /// over any function addressing the same control — the ceiling is the handle at its top,
    /// by definition. Pure, so the composition is unit-testable without a device.
    pub fn ceiling_params(&self) -> Vec<(String, String, String, f32)> {
        let (group, node, target) = &self.handle;
        let (_, hi) = target.bounds();
        let mut params: Vec<(String, String, String, f32)> = self
            .states
            .params
            .iter()
            .map(|(g, n, p, a, _b)| (g.clone(), n.clone(), p.clone(), *a))
            .collect();
        params.retain(|(_, n, p, _)| !(n == node && p == &target.param));
        params.push((group.clone(), node.clone(), target.param.clone(), hi));
        params
    }
}

/// THE FS HALF OF THE REORDERED RUN'S PREPASS: read one footswitch sound's CEILING with ONE
/// engage, by measuring its engaged state with the leveling handle pinned at the top of its
/// own (classified, wet-floor-aware) range.
///
/// A MEASUREMENT, NEVER AN EXTRAPOLATION. An arbitrary block param has no algebraically
/// predictable response (`headroom_trade` module header), so the only honest way to know what
/// a footswitch sound can reach is to put its handle at the top and listen. One engage yields
/// the ceiling — never the solve, which still budgets its own captures after the trade.
///
/// TRUE-PEAK CAVEAT: a ceiling read at handle-max can CLIP on a hot chain, and the
/// true-peak caveat machinery is base-only. The returned loudness is the reading as taken;
/// the caller decides how much to trust a hot one.
///
/// `intended_preset_level` is the run's OWN `presetLevel` (see [`measure_fs_state`]). A
/// ceiling read at a stale level is the whole reading off by the difference (10.2 dB on HW —
/// enough for `fs_target_beyond_ceiling` to clamp sounds that are in reach). `None` = assert
/// nothing.
pub fn measure_fs_ceiling(
    probe: &FsCeilingProbe<'_>,
    stimulus: &[f32],
    intended_preset_level: Option<f32>,
) -> Result<lufs::Loudness, LevelError> {
    let params = probe.ceiling_params();
    require_live(
        || {
            measure_fs_state(
                probe.scene,
                &probe.states.engaged_bypass,
                &params,
                stimulus,
                intended_preset_level,
                CaptureWindow::Full,
            )
        },
        stimulus,
    )
}

/// How far a footswitch row's target must sit ABOVE its measured ceiling before the prepass
/// declares it unreachable and skips the solve.
///
/// Deliberately much wider than the lane's `ACCEPT_TOL_LU` acceptance band. The ceiling is read
/// with the handle PINNED AT ITS TOP, which is the hottest this sound ever gets — on a hot
/// chain that capture can CLIP, and the true-peak caveat machinery is base-only, so the
/// reading carries more uncertainty than an ordinary solve point. Skipping a row that could
/// actually have reached target is a silent product bug; paying a full secant budget to
/// rediscover a clamp is only slow. So the margin is sized to make FALSE CLAMPS implausible
/// and lets marginal rows fall through to the solve, which reports honestly either way.
pub(crate) const FS_CEILING_SKIP_MARGIN_LU: f64 = 1.5;

/// Is this footswitch row's target out of reach even with its handle at the top?
/// `ceiling` is [`measure_fs_ceiling`]'s reading. Pure — the decision is unit-testable.
pub(crate) fn fs_target_beyond_ceiling(ceiling: f64, target: f64) -> bool {
    ceiling.is_finite() && target - ceiling > FS_CEILING_SKIP_MARGIN_LU
}

/// The prepass verdict for ONE footswitch row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FsCeiling {
    /// Measured loudness with the handle at the top of its range (LUFS).
    pub ceiling_lufs: f64,
    /// Dynamics spread (LU) of that capture.
    pub spread_lu: f64,
    /// The target is beyond the ceiling by more than [`FS_CEILING_SKIP_MARGIN_LU`] — the row
    /// cannot reach it and the solve is skipped in favour of an honest clamp.
    pub unreachable: bool,
}

/// A CLAMPED footswitch result built from the prepass alone — no solve ran. Used when the
/// ceiling read already proves the target is out of reach: the row reports the loudest it can
/// actually be, at the handle value that produced it, with the shared
/// [`crate::headroom_trade::ClampKind::SceneCeiling`] cause. Writing nothing is correct — the
/// authored value is left exactly as the player wrote it.
pub fn fs_result_from_ceiling(
    switch: u32,
    target_lufs: f64,
    handle: &FsParamTarget,
    ceiling: &FsCeiling,
    method: &str,
) -> FootswitchLevelResult {
    let (_, hi) = handle.bounds();
    FootswitchLevelResult {
        switch,
        measured_lufs: ceiling.ceiling_lufs,
        final_value: hi,
        target_lufs,
        predicted_lufs: ceiling.ceiling_lufs,
        clamped: true,
        unconverged: false,
        clamp_kind: Some(crate::headroom_trade::ClampKind::SceneCeiling),
        clamp_reason: None,
        wet_floor: false,
        saved: false,
        verify_lufs: None,
        // The ONE prepass capture this verdict rests on.
        iterations: 1,
        dynamic_spread_lu: Some(ceiling.spread_lu),
        method: method.to_string(),
        persist_mismatch: None,
        reamp_warning: None,
        reamp_still_on: false,
    }
}

/// Verdict on a solved footswitch value: `(clamped, unconverged)` — the footswitch mirror of
/// the scene path's "report from the FINAL point" rule (`jointk_one_scene`), which likewise
/// drops the open-loop's initial want. Three outcomes, not one flag: at target → neither;
/// knob pinned at the bound that blocks the needed direction → CLAMPED (unreachable, a
/// re-run cannot help); off target with knob room left → UNCONVERGED (the secant ran out of
/// captures on a compressed/noisy response; the best point is written and a re-run can
/// improve it). The third state, no-authority (`clamp_reason: Some(..)` from the seed's
/// routing probe), is decided before this and rides `clamped` as it always did.
///
/// `(lo, hi)` are the PARAM's own bounds (`FsParamTarget::bounds`) — "pinned at a bound"
/// means pinned at the end of THAT param's range, which is `[0, 1]` for most controls but
/// `[0, 10]` for `ACD_Boost.gain`. `LEVEL_MIN`/`LEVEL_MAX` stay reserved for the preset/scene
/// lanes' amplitude knobs.
pub(super) fn classify_fs_outcome(
    best_v: f32,
    best_lufs: f64,
    target_lufs: f64,
    (lo, hi): (f32, f32),
) -> (bool, bool) {
    if (best_lufs - target_lufs).abs() <= solve_accept_tol() {
        return (false, false);
    }
    // Unreachable only when the knob is pinned at the bound that blocks the direction target
    // needs: maxed and target still LOUDER, or zeroed and target still QUIETER. A bound hit
    // whose miss points back INTO the knob's range means the search stopped early, not that
    // the sound can't get there.
    let pinned_loud = best_v >= hi - 1e-3 && target_lufs > best_lufs;
    let pinned_quiet = best_v <= lo + 1e-3 && target_lufs < best_lufs;
    let clamped = pinned_loud || pinned_quiet;
    (clamped, !clamped)
}

/// Is the switch's engaged loudness already at target (within `ACCEPT_TOL_LU`, not clamped)? The
/// footswitch counterpart of `scene_at_target`, on this lane's tighter band, so a re-run
/// leaves an in-tolerance switch untouched instead of re-randomizing it. `clamped` is always
/// `false` here (the probe measures a real value at `cur`), but the param matches
/// `scene_at_target` for parity and testability.
///
/// KEEPS the `!clamped` requirement where the BASE lane's `already_on_target` drops it — do
/// not "restore parity" by copying that band here. That lane can drop the flag only because
/// its skip is provably disjoint from a boost (see its doc); this one has no pair planner
/// behind it, so dropping the flag would silently suppress an honest FS clamp.
pub(super) fn switch_at_target(measured: f64, target: f64, clamped: bool) -> bool {
    !clamped && (measured - target).abs() <= solve_accept_tol()
}

/// Measurement/solve phase of ONE footswitch job — no write, no save, no reload.
/// CALLER CONTRACT: the preset is already current (load it once per batch), and the
/// caller discards the sweep pollution afterwards (the batch write session's reload,
/// or a plain reload on the dry/no-signal paths). Returns the un-persisted result
/// (`saved:false`, `verify_lufs:None`); `final_value` is the solved value.
///
/// `current_value` = the switch's currently-configured engaged value (a live-read prior
/// `valueA` on the Assign re-run path). When `Some`, the leveler probes it FIRST: if the
/// engaged loudness there is already at target it returns `final_value == current_value`
/// verbatim so the caller writes nothing — the re-run idempotency skip (the same INTENT as the
/// base `level_unchanged` + `already_on_target` and scene `scene_at_target` skips; the TESTS
/// differ per lane — see `switch_at_target`). A Bake plan passes the block's own
/// stored param value here too (baking writes straight to the block, so that value IS the
/// engaged value) — `None` remains for fresh assigns and probe seams, which have no prior
/// value to anchor on.
///
/// `param` is the CLASSIFIED target ([`FsParamTarget`]) — it supplies the solve's bounds,
/// the not-a-level-control refusal and the wet floor. Build it with
/// [`FsParamTarget::from_preset`] off the batch's single saved-preset read.
#[allow(clippy::too_many_arguments)]
///
/// `intended_preset_level` is the run's OWN `presetLevel`, re-asserted on EVERY capture of
/// the solve for the same reason the isolation list is re-sent on every capture: each one
/// recalls a scene, and a base recall can re-apply a stale snapshot level. Solving against a
/// stale level solves the wrong sound. `None` = assert nothing.
pub fn measure_footswitch(
    switch: u32,
    scene: Option<u32>,
    lev: (&str, &str, &str),
    engaged_bypass: &[(String, String, bool)],
    stimulus: &[f32],
    target_lufs: f64,
    method: &str,
    current_value: Option<f32>,
    param: &FsParamTarget,
    intended_preset_level: Option<f32>,
    probe: Option<SeedPoint>,
) -> Result<FootswitchLevelResult, LevelError> {
    solve_footswitch_seeded(
        switch,
        engaged_bypass,
        stimulus,
        target_lufs,
        method,
        current_value,
        probe,
        param,
        || reamp_off_fresh("measure_footswitch"),
        // EVERY capture of this row recalls `scene` before engaging — the isolation list and
        // the intended `presetLevel` are both re-sent per capture for the same reason (a
        // recall re-asserts its own bypass state AND can re-apply a stale level). Far
        // steps take a short window ([`CoarseFs`]), picking up from the prepass probe's miss.
        {
            let mut coarse = CoarseFs::new(target_lufs, probe.map(|p| p.lufs));
            move |bypass, v| {
                coarse.measure(|w| {
                    measure_fs_at(scene, lev, bypass, stimulus, v, intended_preset_level, w)
                })
            }
        },
    )
}

/// How far (LU) a footswitch reading may miss its target before the NEXT capture of that row
/// takes the short window: far steps only aim the secant, so their exact level never decides
/// the outcome (`TMP_FS_COARSE_FAR_LU`, a bench knob).
fn coarse_far_lu() -> f64 {
    std::env::var("TMP_FS_COARSE_FAR_LU")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0)
}

/// Coarse-to-fine capture windows for ONE footswitch row's captures: a capture whose previous
/// reading missed the target by more than [`coarse_far_lu`] (or that has no previous reading)
/// takes the [`CaptureWindow::Short`] window, every other capture the full one. A short
/// reading that lands inside the acceptance band (within [`coarse_far_lu`] for the row's first
/// reading, which also decides the at-target skip), or that hears no signal, is re-measured
/// full at the same value before anyone sees it, so the solve never accepts, skips or
/// verdicts on a short reading. It saves about 30% of the footswitch step (offline replay of
/// a 155-capture HW bench), every accepted reading full. Off with `TMP_FS_COARSE=0` (bench
/// A/B).
pub(crate) struct CoarseFs {
    target: f64,
    last_lufs: Option<f64>,
    on: bool,
}

impl CoarseFs {
    /// `last_lufs` = the row's previous reading, if one was already taken (the prepass probe).
    pub(crate) fn new(target: f64, last_lufs: Option<f64>) -> Self {
        CoarseFs {
            target,
            last_lufs,
            on: super::env_ms("TMP_FS_COARSE", 1) != 0,
        }
    }

    /// The window the next capture takes.
    fn window(&self) -> CaptureWindow {
        let far = self
            .last_lufs
            .is_none_or(|l| (l - self.target).abs() > coarse_far_lu());
        if self.on && far {
            CaptureWindow::Short
        } else {
            CaptureWindow::Full
        }
    }

    /// Take the next capture of this row through `at`, in the window [`Self::window`] picks.
    pub(crate) fn measure(
        &mut self,
        mut at: impl FnMut(CaptureWindow) -> Result<lufs::Loudness, LevelError>,
    ) -> Result<lufs::Loudness, LevelError> {
        let window = self.window();
        if window == CaptureWindow::Full {
            return self.record(at(CaptureWindow::Full));
        }
        // A row's FIRST reading also decides the at-target skip, so a short one near the band
        // is re-measured too: its window bias (up to ~0.5 LU) must not turn an in-band sound
        // into a fresh solve and a rewrite on a re-run. Later short readings only need the band.
        let confirm_within = if self.last_lufs.is_none() {
            coarse_far_lu()
        } else {
            solve_accept_tol()
        };
        match at(CaptureWindow::Short) {
            Ok(l) if (l.integrated_lufs - self.target).abs() > confirm_within => self.record(Ok(l)),
            Ok(_) => self.record(at(CaptureWindow::Full)),
            // Silence in a short window is not evidence: a wet-only or slow-building sound can
            // stay below the gate for 3 s. Only the full window may report it.
            Err(e) if e.is_no_signal() => self.record(at(CaptureWindow::Full)),
            Err(e) => Err(e),
        }
    }

    fn record(
        &mut self,
        l: Result<lufs::Loudness, LevelError>,
    ) -> Result<lufs::Loudness, LevelError> {
        let l = l?;
        self.last_lufs = Some(l.integrated_lufs);
        Ok(l)
    }
}

/// [`solve_footswitch_seeded`] with no prepaid probe — the solve suites' seam.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(super) fn solve_footswitch_with(
    switch: u32,
    engaged_bypass: &[(String, String, bool)],
    stimulus: &[f32],
    target_lufs: f64,
    method: &str,
    current_value: Option<f32>,
    param: &FsParamTarget,
    reamp_off: impl FnMut(),
    measure: impl FnMut(&[(String, String, bool)], f32) -> Result<lufs::Loudness, LevelError>,
) -> Result<FootswitchLevelResult, LevelError> {
    solve_footswitch_seeded(
        switch,
        engaged_bypass,
        stimulus,
        target_lufs,
        method,
        current_value,
        None,
        param,
        reamp_off,
        measure,
    )
}

/// The device-independent solve loop behind [`measure_footswitch`]: seed → bracket → bounded
/// secant, with `measure(isolation, v)` doing the actual capture. The isolation list is an
/// ARGUMENT of every call, not state captured once: each capture recalls a scene (base
/// included) and a recall RE-ASSERTS that scene's own bypass state, so a once-only forced
/// write is silently reverted for captures 2..N and every later point measures a
/// NON-isolated sound (working-copy edits survive the reconnects, but the recall undoes
/// them). Injectable — `reamp_off` too (see
/// [`solve_param_secant`]) — so the loop + its outcome verdict are unit tested without a
/// device.
///
/// `probe` is the idempotency probe's reading when the caller already took it (the batch
/// prepass's capture at `current_value`, see `solve_param_secant`'s `known_seed`), so the
/// solve does not buy that capture a second time.
#[allow(clippy::too_many_arguments)]
pub(super) fn solve_footswitch_seeded(
    switch: u32,
    engaged_bypass: &[(String, String, bool)],
    stimulus: &[f32],
    target_lufs: f64,
    method: &str,
    current_value: Option<f32>,
    probe: Option<SeedPoint>,
    param: &FsParamTarget,
    reamp_off: impl FnMut(),
    mut measure: impl FnMut(&[(String, String, bool)], f32) -> Result<lufs::Loudness, LevelError>,
) -> Result<FootswitchLevelResult, LevelError> {
    let solved = solve_param_secant(
        stimulus,
        target_lufs,
        current_value,
        probe,
        param,
        FS_CORRECT_MAX,
        reamp_off,
        |v| measure(engaged_bypass, v),
    )?;
    Ok(solved.into_fs_result(switch, target_lufs, method))
}

/// [`solve_footswitch_with`] with no disengage hook — the solve suites that don't count OFFs.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(super) fn solve_footswitch(
    switch: u32,
    engaged_bypass: &[(String, String, bool)],
    stimulus: &[f32],
    target_lufs: f64,
    method: &str,
    current_value: Option<f32>,
    param: &FsParamTarget,
    measure: impl FnMut(&[(String, String, bool)], f32) -> Result<lufs::Loudness, LevelError>,
) -> Result<FootswitchLevelResult, LevelError> {
    solve_footswitch_with(
        switch,
        engaged_bypass,
        stimulus,
        target_lufs,
        method,
        current_value,
        param,
        || {},
        measure,
    )
}

/// What the param-space secant ACTUALLY found — the solve's own vocabulary, with no wire
/// shape attached. Each caller maps this into its own shape: [`solve_footswitch_seeded`] via
/// [`ParamSolve::into_fs_result`], the scene HANDLE lane into a [`SceneSolve`].
pub(super) struct ParamSolve {
    /// The value to write — the best point the search reached, always inside the param's
    /// [`FsParamTarget::bounds`] (the wet floor included), so it is always writable. ONE
    /// exception: the idempotency skip's `final_value == current_value` verbatim can sit
    /// BELOW the wet floor (an existing assign whose stored `valueA` predates the floor,
    /// or predates a base value change) — safe only because that equality is the
    /// caller's no-op signal, so this field is never clamped there; clamping would break
    /// the no-op detect and make an unchanged switch look like a solved write.
    pub(super) final_value: f32,
    /// Loudness at the first REFERENCE point: the low seed's capture, the idempotency
    /// probe's, or the routing-clamp sentinel. Context for the report, never the achieved
    /// value.
    pub(super) measured_lufs: f64,
    /// Achieved loudness AT `final_value` — a real capture, not a model prediction.
    pub(super) predicted_lufs: f64,
    /// The knob RAN OUT (pinned at the bound blocking the needed direction) or has no
    /// authority over loudness at all. A re-run cannot help.
    pub(super) clamped: bool,
    /// Off target with knob room left — the bounded secant spent its budget on a
    /// compressed/noisy response. A re-run CAN improve it.
    pub(super) unconverged: bool,
    /// `Some` ONLY for the first seed's routing probe ("no signal on USB 1/2"); every later
    /// silence is the knob's own quiet extreme, which is data.
    pub(super) clamp_reason: Option<String>,
    /// The clamp pinned at the WET FLOOR specifically — the row wants a "verify by ear"
    /// advisory. Rides only with `clamped`.
    pub(super) wet_floor: bool,
    /// Dynamics spread (LU) of the capture behind `predicted_lufs`; `None` when no real
    /// capture backs it (the routing clamp).
    pub(super) spread_lu: Option<f64>,
    /// Real device captures this solve paid for.
    pub(super) iterations: u32,
}

impl ParamSolve {
    /// Map onto the FOOTSWITCH wire shape. `switch` and `method` are report-only tags the
    /// solve has no opinion about, and the persistence fields (`saved`, `verify_lufs`,
    /// `persist_mismatch`) belong to the write path that runs after this.
    pub(super) fn into_fs_result(
        self,
        switch: u32,
        target_lufs: f64,
        method: &str,
    ) -> FootswitchLevelResult {
        FootswitchLevelResult {
            switch,
            measured_lufs: self.measured_lufs,
            final_value: self.final_value,
            target_lufs,
            predicted_lufs: self.predicted_lufs,
            clamped: self.clamped,
            unconverged: self.unconverged,
            clamp_kind: crate::headroom_trade::ClampKind::from_flags(
                self.clamped,
                self.wet_floor,
                self.clamp_reason.as_deref(),
            ),
            clamp_reason: self.clamp_reason,
            wet_floor: self.wet_floor,
            saved: false,
            verify_lufs: None,
            iterations: self.iterations,
            dynamic_spread_lu: self.spread_lu,
            method: method.into(),
            persist_mismatch: None,
            reamp_warning: None,
            reamp_still_on: false,
        }
    }
}

/// One already-captured solve point: the knob `value`, its integrated loudness and the
/// dynamics spread of that capture — all the secant reads off a seed.
#[derive(Debug, Clone, Copy)]
pub struct SeedPoint {
    pub value: f32,
    pub lufs: f64,
    pub spread: f64,
}

/// [`solve_footswitch_seeded`]'s loop with the CAPTURE BUDGET as an argument — the seam the scene
/// handle lane reuses. Every correction iterate is a fresh-connect re-amp capture (~10 s),
/// and the two lanes' cost profiles differ: a footswitch batch pays it per switch
/// ([`FS_CORRECT_MAX`]), a scene batch per scene and up to 9 of them
/// ([`SCENE_HANDLE_CORRECT_MAX`]). Nothing else about the solve differs, so the budget is
/// the ONLY thing threaded — inheriting the footswitch cap would inflate per-scene cost.
///
/// `measure(v)` captures at knob value `v`; whatever isolation that needs (a footswitch's
/// engaged-bypass list, a scene recall) is the CALLER's closure to carry — the solve itself
/// has no footswitch concepts left in it.
///
/// `reamp_off()` is the caller's mid-solve disengage (production: [`reamp_off_fresh`]), sent
/// when the solve leaves early or after a failed capture, because the measurement's own
/// last disengage can be dropped. The run-end OFF stays the caller's [`ReampRunGuard`], so
/// the solve itself never opens a connection.
///
/// `known_seed` is a point the CALLER already captured ([`SeedPoint`]), taken in exactly
/// the measurement context `measure_at(value)` would produce (same knob value, stimulus,
/// `presetLevel`, scene and isolation) on its own fresh engage. When `Some` it is seed 1 —
/// no capture is bought for it, exactly like a successful idempotency probe below. When
/// `current_value` is `Some`, a seed AT that value is the probe's own reading, taken by the
/// caller (the footswitch batch's prepass): the probe uses it instead of capturing, and any
/// other seed is ignored (the probe then owns seed 1). It does NOT
/// anchor the wet floor (that stays `current_value`'s job) and it is never the routing probe:
/// the caller's capture already proved signal. The scene HANDLE lane passes its as-is
/// reading here when `scene::asis_seed_context_matches` proves the contexts identical;
/// `None` everywhere else keeps the fixed-seed path.
#[allow(clippy::too_many_arguments)]
pub(super) fn solve_param_secant(
    stimulus: &[f32],
    target_lufs: f64,
    current_value: Option<f32>,
    known_seed: Option<SeedPoint>,
    param: &FsParamTarget,
    correct_max: u32,
    mut reamp_off: impl FnMut(),
    mut measure_at: impl FnMut(f32) -> Result<lufs::Loudness, LevelError>,
) -> Result<ParamSolve, LevelError> {
    // ENTRY GUARD, before any device work: a param the classifier doesn't recognise as a
    // level or wet/mix control is not a volume control. Sweeping it would change the sound
    // the player wrote, not its loudness — refuse instead of "levelling" it. Surfaces as a
    // clean per-switch `status: "error"` item (the batched command's `Err` arm).
    if let Some(refusal) = param.refuse_if_not_a_level_control() {
        return Err(LevelError::refused(SkipKind::UnsupportedKnob, refusal));
    }
    // The wet floor anchors on the ENGAGED value when the switch already has one (see
    // `anchored`'s doc); applied here, not at call sites, so it cannot be forgotten.
    let param = &param.anchored(current_value);
    let (bound_lo, bound_hi) = param.bounds();

    // Idempotency probe: if the switch's currently-configured engaged value already hits
    // target, leave it untouched (a re-run must not re-solve + re-randomize an in-tolerance
    // switch). Reuses `measure_at` (so a success arms the isolation-once optimization for the
    // seeds); a NO_SIGNAL / floor / transient error falls through to the seed pass, which
    // owns the routing-clamp verdict. `current_value` is None for fresh assigns / probe seams.
    // A successful-but-off-target probe is a PAID capture: it is kept as `probe_seed` and
    // reused as the solve's seed 1 below instead of being discarded and re-bought at a fresh
    // `at_fraction(0.25)`.
    let mut probe_seed: Option<SeedPoint> = None;
    if let Some(cur) = current_value {
        // The caller's reading at `cur` stands in for the capture (see `known_seed`'s doc).
        // Its capture already disengaged and the caller ran the OFF ladder after it, so a
        // prepaid skip sends no OFF of its own.
        let prepaid = known_seed.filter(|k| k.value == cur);
        let reading = match prepaid {
            Some(k) => Ok(k),
            None => require_live(|| measure_at(cur), stimulus).map(|l| SeedPoint {
                value: cur,
                lufs: l.integrated_lufs,
                spread: l.spread_lu(),
            }),
        };
        match reading {
            Ok(l) if switch_at_target(l.lufs, target_lufs, false) => {
                if prepaid.is_none() {
                    reamp_off();
                }
                // Skip signal: `final_value` == the caller's current value verbatim, so the
                // caller detects the no-op by `final_value == current` and writes nothing
                // (the footswitch mirror of the scene lane's off-wire `writes: 0`). See
                // `ParamSolve::final_value`'s doc: this is the one path that can leave the
                // returned value below the wet floor, safely, because it is never written.
                return Ok(ParamSolve {
                    final_value: cur,
                    measured_lufs: l.lufs,
                    predicted_lufs: l.lufs,
                    clamped: false,
                    unconverged: false,
                    clamp_reason: None,
                    wet_floor: false,
                    spread_lu: Some(l.spread),
                    iterations: 1,
                });
            }
            // In-tolerance-but-not-a-skip carries into the seed pass as seed 1; a probe
            // error must disengage re-amp first (the seed pass re-engages on a fresh
            // connection) and falls through to the fixed-seed path below.
            Ok(l) => probe_seed = Some(l),
            Err(_) => reamp_off(),
        }
    }

    // Seed 1: reuse a successful idempotency probe when there is one (no new capture — it
    // already proved signal, so the routing-clamp verdict below belongs to the fixed-seed
    // path alone); else the caller's own context-matched capture (`known_seed`, never paid by
    // this solve); else the fixed `at_fraction(0.25)` position exactly as before.
    let known_seed = known_seed.filter(|_| current_value.is_none());
    let seed1_paid_here = known_seed.is_none();
    let (v_a, l_a_lufs, l_a_spread) = if let Some(p) = probe_seed.or(known_seed) {
        (p.value, p.lufs, p.spread)
    } else {
        let v = param.at_fraction(0.25);
        // This seed doubles as the routing probe: a genuinely silent capture (device output
        // not on USB 1/2) makes `processed_loudness` error "no signal captured" — convert
        // THAT one to the honest "not on USB 1/2" clamp (mirrors the scene mute-floor idiom
        // below). Signal-present but flat/short-of-target is a headroom/authority clamp with
        // NO reason, not a routing error. ONLY this seed can mean broken routing (silent from
        // the very first bought capture); every LATER silence — including a silent PROBE seed
        // taking this seed's place above — is the knob's own quiet extreme (a pedal
        // `level`/`volume` at 0 IS deep digital silence on the real unit — DATA the secant
        // needs, not a fatal abort). See `FS_SILENT_GEOMETRY_LUFS`'s doc: seed 2, the
        // bracket-expansion probe, and the correction loop below each convert a later
        // silence into a pseudo point instead of propagating the error (HW: a Plumes `level`
        // knob whose seeds read the compressed plateau extrapolates a negative knob value,
        // clamped to 0.0, and 0.0 is silent).
        // Floor-guarded (the flat-but-finite silent-inject case); the NO_SIGNAL arm below
        // stays separate — genuine silence is the routing clamp, not a floor read.
        let l = match require_live(|| measure_at(v), stimulus) {
            Ok(l) => l,
            Err(e) if e.is_no_signal() => {
                reamp_off();
                return Ok(ParamSolve {
                    final_value: v,
                    measured_lufs: MUTE_FLOOR_SILENT_LUFS,
                    predicted_lufs: MUTE_FLOOR_SILENT_LUFS,
                    clamped: true,
                    unconverged: false,
                    clamp_reason: Some(NO_SIGNAL_REASON.into()),
                    wet_floor: false,
                    spread_lu: None,
                    iterations: 1,
                });
            }
            Err(e) => return Err(e),
        };
        (v, l.integrated_lufs, l.spread_lu())
    };

    // Seed 2: probe → LAW → fixed-fraction ladder. `law_predicted` returns the linear-law
    // candidate (or `None` for a `WetMix`/no-known-law param, which always takes the fixed
    // fallback); `seed2_plausible` is the acceptance gate — see both methods' docs for the
    // law itself, the accuracy band it's validated against (HW), and why a rejected/wrong
    // prediction degrades to the fixed complement fraction (0.75 if seed 1 sits in the lower
    // half of the range, else 0.25).
    let fixed_fallback = || {
        if v_a <= param.at_fraction(0.5) {
            param.at_fraction(0.75)
        } else {
            param.at_fraction(0.25)
        }
    };
    let v_b = param
        .law_predicted(v_a, l_a_lufs, target_lufs)
        .filter(|&p| param.seed2_plausible(p, l_a_lufs, target_lufs))
        .map(|p| p as f32)
        .unwrap_or_else(fixed_fallback);
    // Seed 2 can land silent (a knob whose useful range is a narrow slice of its own
    // range) — past seed 1, that's data, never a routing error (see seed 1's doc above).
    // A clipped seed 2 is the mirror case, the knob's loud end (see `fs_clipped_geometry`).
    // `seed2_synthetic` feeds the initial best-seed pick below so a synthesized point can
    // never win it (mirrors `improve_best`'s exclusion of synthetic points).
    let (l_b_lufs, l_b_spread, seed2_synthetic) = match require_live(|| measure_at(v_b), stimulus) {
        Ok(l) => (l.integrated_lufs, l.spread_lu(), false),
        Err(e) if e.is_no_signal() => (fs_silent_geometry(l_a_lufs), 0.0, true),
        Err(LevelError::Clipped) => (fs_clipped_geometry(l_a_lufs, target_lufs), 0.0, true),
        Err(e) => return Err(e),
    };
    // The quietest REAL capture seen so far — the anchor `fs_silent_geometry` floors every
    // silent pseudo point under.
    let (mut min_real, mut max_real) = if seed2_synthetic {
        (l_a_lufs, l_a_lufs)
    } else {
        (l_a_lufs.min(l_b_lufs), l_a_lufs.max(l_b_lufs))
    };
    // The probe capture (when present) counts as capture 1, so after seed 2 this is 2 real
    // device round-trips on BOTH seeded paths — probe-as-seed-1+seed2, or fixed-seed1+
    // seed2. It is NOT the count on the probe-ERROR path (probe fails, falls through to a
    // fresh fixed seed 1, then seed 2 — 3 round-trips: the failed probe, seed 1, seed 2);
    // that extra failed round-trip is deliberately left uncounted here. A caller-supplied
    // `known_seed` was NOT paid by this solve, so only seed 2 counts on that path.
    let mut iterations = if seed1_paid_here { 2u32 } else { 1u32 };
    let err = |l: f64| (l - target_lufs).abs();
    let tol = solve_accept_tol();
    let (mut best_v, mut best_lufs, mut best_spread) =
        if seed2_synthetic || err(l_a_lufs) <= err(l_b_lufs) {
            (v_a, l_a_lufs, l_a_spread)
        } else {
            (v_b, l_b_lufs, l_b_spread)
        };
    // A seed pair that cannot move loudness is a physical dead end, not an exhausted
    // search — it must never advertise a re-run. Set ONLY where no-authority is proven
    // (the seed/expansion pair below); a mid-loop stall keeps `unconverged`.
    let mut flat_response = false;
    if err(best_lufs) > tol {
        // The correction loop below interpolates in solve-COORD space
        // (`param.to_coord`), not raw parameter value — see `to_coord`'s doc for why
        // that makes the secant exact for a `LevelLinear`/`LevelDb` param.
        let mut p0 = (param.to_coord(v_a as f64), l_a_lufs);
        let mut p1 = (param.to_coord(v_b as f64), l_b_lufs);
        // Bracket before falling to the correction loop — see `fs_bracket_expansion`'s doc.
        if let Some(v_extreme) = fs_bracket_expansion(
            v_a,
            l_a_lufs,
            v_b,
            l_b_lufs,
            target_lufs,
            (bound_lo, bound_hi),
        ) {
            // Silence at the probe is data (see `FS_SILENT_GEOMETRY_LUFS`); any other
            // error just forfeits the extra probe (the plain secant still runs).
            let extreme_point = match require_live(|| measure_at(v_extreme), stimulus) {
                Ok(l_extreme) => {
                    iterations += 1;
                    min_real = min_real.min(l_extreme.integrated_lufs);
                    max_real = max_real.max(l_extreme.integrated_lufs);
                    improve_best(
                        target_lufs,
                        v_extreme,
                        &l_extreme,
                        &mut best_v,
                        &mut best_lufs,
                        &mut best_spread,
                    );
                    Some((param.to_coord(v_extreme as f64), l_extreme.integrated_lufs))
                }
                Err(e) if e.is_no_signal() => {
                    iterations += 1;
                    Some((
                        param.to_coord(v_extreme as f64),
                        fs_silent_geometry(min_real),
                    ))
                }
                Err(_) => None,
            };
            if let Some(extreme_point) = extreme_point {
                if err(p0.1) <= err(p1.1) {
                    p1 = extreme_point;
                } else {
                    p0 = extreme_point;
                }
            }
        }
        // Bracket-aware secant, gated on the pair having authority (still flat → the
        // knob truly can't move loudness here — an honest, reason-less clamp). Each
        // iterate interpolates the SAME `secant_step` (`Flatness::SolveCoord`); only the
        // endpoint-update rule differs by pair shape:
        //   * pair STRADDLES the target → keep the bracket (replace the same-side
        //     endpoint) with Illinois damping — a repeat on one side halves the retained
        //     endpoint's offset so a stale far end can't pin the interpolation. This is
        //     what converges on a steep-taper cliff (HW: a UniVibe `volume` measures
        //     −45.6 LUFS at 0.25 but −16.9 at 0.61, a >60 LU/knob-unit cliff; the plain
        //     sliding secant zig-zags across it and stops 3.1 LU hot).
        //   * pair does NOT straddle → the plain slide (drop the older point); the first
        //     crossing iterate forms a straddling consecutive pair, which flips the loop
        //     into bracket mode by itself.
        if err(best_lufs) > tol {
            if (p1.1 - p0.1).abs() < KNOB_TOL_LU {
                // The seed pair itself is flat (and the expansion probe didn't land
                // it): the knob demonstrably can't move loudness, the correction loop
                // has nothing to interpolate on, and best_v sits mid-range. This is
                // the ONE place no-authority is proven — the noise-robust KNOB_TOL_LU
                // band over the widest pair the solve ever holds.
                flat_response = true;
                // A caller-supplied `known_seed` is the knob's UNCHANGED as-is value, so
                // letting it win a flat seed pair would turn a no-authority run into a
                // silent no-op write ("a run always WRITES"). Without a known seed this
                // landing is the solve's first OWN capture (the fixed seed 1); with one, it
                // is the first own capture too — seed 2 — unless the expansion probe
                // already took the lead.
                if !seed1_paid_here && !seed2_synthetic && best_v == v_a {
                    (best_v, best_lufs, best_spread) = (v_b, l_b_lufs, l_b_spread);
                }
            } else {
                // −1 / +1: which side of the target the previous bracket-mode iterate
                // landed on (0 until the pair straddles).
                let mut last_side = 0i8;
                let coord_span =
                    (param.to_coord(bound_hi as f64) - param.to_coord(bound_lo as f64)).abs();
                for _ in 0..correct_max {
                    let straddles = (p0.1 - target_lufs) * (p1.1 - target_lufs) < 0.0;
                    let Some(raw) = secant_step(p0, p1, target_lufs, Flatness::SolveCoord, None)
                    else {
                        // NOT a no-authority verdict: this loop is only entered after
                        // the seeds proved ≥ KNOB_TOL_LU of authority, so a mid-loop
                        // flat/degenerate pair (e.g. both endpoints clamped onto the
                        // same bound on a non-monotone response) means the SEARCH is
                        // stuck, not that the knob is dead — keep the honest
                        // `unconverged` from classify_fs_outcome below.
                        break;
                    };
                    // A straddling pair interpolates inside its own bracket; an
                    // extrapolating one gets a trust region, so two close points whose
                    // ΔLUFS is mostly noise can't fling the next probe across the range.
                    let raw = if straddles {
                        raw
                    } else {
                        fs_trust_clamp(raw, p0.0, p1.0, coord_span)
                    };
                    let v2 = param
                        .coord_to_value(raw)
                        .clamp(bound_lo as f64, bound_hi as f64)
                        as f32;
                    // Silence is data (see `FS_SILENT_GEOMETRY_LUFS`); `l2_real` gates the
                    // at-target break so only a REAL capture may declare victory.
                    let (l2_lufs, l2_real) = match require_live(|| measure_at(v2), stimulus) {
                        Ok(l2) => {
                            iterations += 1;
                            min_real = min_real.min(l2.integrated_lufs);
                            max_real = max_real.max(l2.integrated_lufs);
                            improve_best(
                                target_lufs,
                                v2,
                                &l2,
                                &mut best_v,
                                &mut best_lufs,
                                &mut best_spread,
                            );
                            (l2.integrated_lufs, true)
                        }
                        Err(e) if e.is_no_signal() => {
                            iterations += 1;
                            (fs_silent_geometry(min_real), false)
                        }
                        Err(LevelError::Clipped) => {
                            iterations += 1;
                            (fs_clipped_geometry(max_real, target_lufs), false)
                        }
                        Err(e) => return Err(e),
                    };
                    if l2_real && err(l2_lufs) <= tol {
                        break;
                    }
                    let p2 = (param.to_coord(v2 as f64), l2_lufs);
                    if straddles {
                        if (p2.1 - target_lufs) * (p0.1 - target_lufs) > 0.0 {
                            if last_side == -1 {
                                p1.1 = target_lufs + (p1.1 - target_lufs) / 2.0;
                            }
                            p0 = p2;
                            last_side = -1;
                        } else {
                            if last_side == 1 {
                                p0.1 = target_lufs + (p0.1 - target_lufs) / 2.0;
                            }
                            p1 = p2;
                            last_side = 1;
                        }
                    } else {
                        p0 = p1;
                        p1 = p2;
                        last_side = 0;
                    }
                }
            }
        }
    }
    // Signal is present past the seed probe, so a miss is a headroom or convergence limit,
    // never a routing error → `clamp_reason` stays None (that reason belongs to the seed's
    // routing probe alone). A FLAT response is the exception: the knob has no authority
    // over loudness, so a re-run repeats the same miss — report it as the reason-less
    // clamp (the scene lane's precedent), never as `unconverged`.
    let (clamped, unconverged) = if flat_response {
        (true, false)
    } else {
        classify_fs_outcome(best_v, best_lufs, target_lufs, (bound_lo, bound_hi))
    };
    Ok(ParamSolve {
        // The wet floor needs no epilogue: `bounds()` folds it into `bound_lo`, so the
        // secant never probed below it, `best_v` is always writable, and `predicted_lufs`
        // is a real reading OF the written value. A wet solve pinned at the floor arrives
        // here as an ordinary at-a-bound clamp; only the advisory flag below names it.
        final_value: best_v,
        measured_lufs: l_a_lufs,
        predicted_lufs: best_lufs,
        clamped,
        unconverged,
        clamp_reason: None,
        wet_floor: clamped && param.pinned_at_wet_floor(best_v),
        spread_lu: Some(best_spread),
        iterations,
    })
}

/// One footswitch's solved write, pending the batch's single write+save session.
pub struct FsPendingWrite {
    pub switch: u32,
    /// The leveled `(group, node, param)`.
    pub lev: (String, String, String),
    pub write: FsWrite,
    /// The solved value (`valueA` for Assign; the baked block value for Bake).
    pub value: f32,
}

/// Write every pending footswitch value on ONE live-edit session and persist with ONE
/// `saveCurrentPreset` — the per-preset single save (not a reload + save per switch: N
/// base flashes + N saves of user-visible churn). Session shape
/// per the chunked-edit rules: establish a LIVE CONTROLLER (`begin_live_edit` warmup),
/// then load (discarding ALL the measurement pollution the batch's sweeps accumulated),
/// then keep the session live with heartbeat bursts right up to each chunked `ftsw`
/// edit (chunked edits are silently DROPPED if the session lapses — a passive sleep
/// lets it lapse; HW-confirmed). Each write keeps its confirm gate
/// (`setFootswitchAssignment` (54) echo / read-back, retry-once, never save on
/// `presetError`); ANY
/// unconfirmed write aborts BEFORE the save, so nothing half-applied persists.
///
/// A `FsWrite::Bake` persists the solved value with `changeParameter`, which lands in
/// whatever scene the connection currently holds — and the load below activates the preset's
/// SAVED `lastLoadedScene`, not base (HW). Hence the mandatory base recall: without it a
/// scened preset's bake lands in that scene's overlay while BASE — the state the leveler
/// measured, and the one the switch's off position renders — keeps its old value.
pub fn write_footswitch_values(
    slot: ListIndex,
    pending: &[FsPendingWrite],
    restore_scene: Option<u32>,
) -> Result<(), LevelError> {
    if pending.is_empty() {
        return Ok(());
    }
    // Guaranteed re-amp OFF first — the measurement's last disengage can be dropped.
    reamp_off_fresh("write_footswitch_values");
    let mut s = Session::connect()?;
    write_fs_values_on_session(&mut s, slot, pending, restore_scene)
}

/// [`write_footswitch_values`]' body on an ALREADY-OPEN session — split out so the write
/// order (load → warmup → BASE recall → confirm-gated writes → ONE save) is unit-tested
/// against `SimDevice` with no real HID open.
///
/// The recall is issued ONCE, before the loop, never per item: a recall can undo earlier
/// UNSAVED writes (every one under DISCARD CHANGES; an unsaved `presetLevel` on a base →
/// base recall), so a per-item recall would put the earlier bakes at risk. These are
/// base-context writes with no Scene Edit arming.
pub(super) fn write_fs_values_on_session(
    s: &mut Session,
    slot: ListIndex,
    pending: &[FsPendingWrite],
    restore_scene: Option<u32>,
) -> Result<(), LevelError> {
    s.begin_live_edit()?;
    s.load_preset(slot)?;
    let name = s.active_preset_name().unwrap_or_default();
    if !name.is_empty() && !s.await_active_preset(&name, 20) {
        return Err("after reload, active preset changed — aborting before write".into());
    }
    for _ in 0..8 {
        let _ = s.heartbeat();
        let _ = s.pump_silent(150);
    }
    // Base is a RECALL (wire slot 8), never an omission — see this function's doc.
    recall_base(s)?;
    for p in pending {
        let (group, node, param) = (&p.lev.0, &p.lev.1, &p.lev.2);
        match &p.write {
            FsWrite::Assign { value_b, spec } => {
                let json = serde_json::json!({
                    "func": "param", "groupId": group, "nodeId": node, "parameterId": param,
                    "valueA": p.value, "valueB": value_b, "valueType": 2,
                    "colorA": spec.color_a, "colorB": spec.color_b,
                    "customLabel": spec.custom_label, "switchType": spec.switch_type,
                    "isActive": spec.is_active, "linkGroup": spec.link_group
                })
                .to_string();
                // Confirm the set landed: the device ECHOES field 54 on success (checked first,
                // before the read-back clears the buffer); the working-copy read-back corroborates
                // (it can lag a heartbeat on a post-measurement flooded line). The first edit after
                // a fresh load can be silently dropped, so retry the whole set+confirm once.
                let mut confirmed = false;
                let mut last_seen = Vec::new();
                for _ in 0..2 {
                    s.set_footswitch_assignment(p.switch, spec.function_index, &json, false, None)?;
                    if s.saw_preset_error() {
                        return Err(
                            "device rejected the footswitch assignment (presetError) — not saved"
                                .into(),
                        );
                    }
                    last_seen = s.seen_preset_fields();
                    if last_seen.contains(&54) {
                        confirmed = true;
                        break;
                    }
                    for _ in 0..3 {
                        if s.live_ftsw().is_some_and(|f| {
                            param_fn_present(&f, p.switch, spec.function_index, param)
                        }) {
                            confirmed = true;
                            break;
                        }
                        let _ = s.heartbeat();
                        crate::device_gate::settle(Duration::from_millis(200));
                    }
                    if confirmed {
                        break;
                    }
                }
                if !confirmed {
                    return Err(LevelError::Device(format!(
                        "footswitch assignment not confirmed (no field-54 echo / read-back, \
                         retried; device replied with PresetMessage fields {last_seen:?}) — not saved"
                    )));
                }
            }
            FsWrite::Bake { clear_stale, .. } => {
                // Clear a now-redundant param fn FIRST (a chunked `ftsw` edit — done while the
                // session is freshest), confirming it's gone (else its valueA would override the
                // baked value when engaged). Then bake the value onto the block. Abort before
                // save if the clear can't be confirmed (nothing is persisted on the reload).
                if let Some(idx) = clear_stale {
                    s.clear_footswitch_assignment(p.switch, *idx)?;
                    if s.saw_preset_error() {
                        return Err(
                            "device rejected the footswitch clear (presetError) — not saved".into(),
                        );
                    }
                    let mut cleared = false;
                    for _ in 0..4 {
                        if s.live_ftsw().is_some_and(|f| {
                            crate::footswitch::existing_param_fn_index(&f, p.switch, node, param)
                                .is_none()
                        }) {
                            cleared = true;
                            break;
                        }
                        let _ = s.heartbeat();
                        crate::device_gate::settle(Duration::from_millis(200));
                    }
                    if !cleared {
                        return Err(
                            "redundant footswitch param fn not confirmed cleared — not saved"
                                .into(),
                        );
                    }
                }
                s.change_parameter(group, node, param, p.value)?;
            }
        }
        // Keep the live controller warm between chunked writes.
        let _ = s.heartbeat();
        let _ = s.pump_silent(150);
    }
    // Mirror each bake into the scenes whose overlay restated the base value (grouped by
    // scene: one recall, then that scene's writes back-to-back — the write must follow its
    // recall inside the ~400 ms idle-gap acceptance window; the overlay exists by
    // construction, so no Scene Edit enable). Unsaved writes survive the recalls (HW,
    // `probe --defer-scenes`); the single save below persists base + every overlay.
    let mut by_scene: std::collections::BTreeMap<u32, Vec<&FsPendingWrite>> =
        std::collections::BTreeMap::new();
    for p in pending {
        if let FsWrite::Bake { mirror_scenes, .. } = &p.write {
            for &scene in mirror_scenes {
                by_scene.entry(scene).or_default().push(p);
            }
        }
    }
    for (scene, writes) in &by_scene {
        s.load_scene(*scene)?;
        crate::device_gate::settle(Duration::from_millis(SETTLE_AFTER_SCENE_RECALL_MS));
        for p in writes {
            s.change_parameter(&p.lev.0, &p.lev.1, &p.lev.2, p.value)?;
        }
        let _ = s.heartbeat();
    }
    recall_original_scene(s, restore_scene)?;
    s.save_current_preset(slot)?;
    Ok(())
}

/// The pseudo-LUFS for a silent post-first-seed capture: `FS_SILENT_GEOMETRY_LUFS`, floored
/// `FS_SILENT_MARGIN_LU` below the quietest REAL capture the solve has seen — a fixed
/// sentinel sitting ABOVE a real point would invert the secant pair's slope and walk the
/// solve the wrong way (real chains do measure below −50: the Plumes fixture's own bottom
/// anchor is −66.9 LUFS, and a whole-curve-quiet chain puts EVERY capture under the fixed
/// value).
pub(super) fn fs_silent_geometry(min_real_lufs: f64) -> f64 {
    FS_SILENT_GEOMETRY_LUFS.min(min_real_lufs - FS_SILENT_MARGIN_LU)
}

/// How far above both the loudest REAL capture and the target `fs_clipped_geometry` puts a
/// clipped probe's pseudo point.
pub(super) const FS_CLIP_MARGIN_LU: f64 = 6.0;

/// The pseudo-LUFS for a CLIPPED probe past the first seed — the loud mirror of
/// [`fs_silent_geometry`]. A clipped capture's integrated reading is LOW (the hub refuses it
/// with [`LevelError::Clipped`]), but the knob value that produced it is plainly too loud, so
/// the geometry places it `FS_CLIP_MARGIN_LU` above both the loudest real capture and the
/// target: the secant then steps back down instead of the whole row aborting. Geometry only,
/// like the silent point — never handed to `improve_best` or reported.
pub(super) fn fs_clipped_geometry(max_real_lufs: f64, target_lufs: f64) -> f64 {
    max_real_lufs.max(target_lufs) + FS_CLIP_MARGIN_LU
}

#[cfg(test)]
mod fs_solve_noise_tests {
    use super::*;

    /// Deterministic N(0, sigma) noise (xorshift + Box–Muller).
    struct Noise(u64);
    impl Noise {
        fn uniform(&mut self) -> f64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        }
        fn gauss(&mut self, sigma: f64) -> f64 {
            let (u1, u2) = (self.uniform(), self.uniform());
            sigma * (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
        }
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

    /// (mean captures per solve, false-`unconverged`/`clamped` rate) for a pedal `level`
    /// knob whose true response is exactly linear in dB (`-14 + 20·log10 v`), read through
    /// σ = 0.12 LU capture noise, over targets the knob can reach comfortably.
    fn noisy_solve_stats(trials: u32) -> (f64, f64) {
        let param = FsParamTarget::new("ACD_SomeBlock", "level", 0.5);
        let mut noise = Noise(0x9E37_79B9_7F4A_7C15);
        let (mut captures, mut misses) = (0u32, 0u32);
        for i in 0..trials {
            let target = -34.0 + 14.0 * (i as f64 / trials as f64);
            let mut n = 0u32;
            let solved = solve_param_secant(
                &[],
                target,
                None,
                None,
                &param,
                FS_CORRECT_MAX,
                || {},
                |v| {
                    n += 1;
                    Ok(reading(
                        -14.0 + 20.0 * (v.max(1e-6) as f64).log10() + noise.gauss(0.12),
                    ))
                },
            )
            .expect("solve");
            captures += n;
            if solved.unconverged || solved.clamped {
                misses += 1;
            }
        }
        (
            captures as f64 / trials as f64,
            misses as f64 / trials as f64,
        )
    }

    // With σ = 0.12 LU capture noise a converged footswitch solve must exit early and
    // never end `unconverged` on noise alone (200 targets: the shared 0.25 LU band averages
    // 2.25 captures per solve, a 0.1 LU band 3.33).
    #[test]
    fn noisy_converged_solves_exit_early_and_rarely_read_unconverged() {
        let (mean_captures, miss_rate) = noisy_solve_stats(200);
        eprintln!("fs noise sim: mean captures {mean_captures:.2}, false-miss rate {miss_rate:.3}");
        assert!(mean_captures <= 2.6, "mean captures {mean_captures:.2}");
        assert!(miss_rate <= 0.02, "false-miss rate {miss_rate:.3}");
    }

    // The extrapolating secant's trust region: never further than twice the seed pair's
    // own span or a quarter of the whole range, whichever is larger.
    #[test]
    fn fs_trust_clamp_bounds_an_extrapolating_step() {
        // A noisy near-flat pair 1 coord apart predicting a 40-coord jump → 15 (¼ of 60).
        assert_eq!(fs_trust_clamp(-50.0, -9.0, -10.0, 60.0), -25.0);
        // A wide pair keeps 2× its own span.
        assert_eq!(fs_trust_clamp(-60.0, -10.0, -30.0, 60.0), -60.0);
        // A step inside the region is untouched.
        assert_eq!(fs_trust_clamp(-12.0, -9.0, -10.0, 60.0), -12.0);
    }
}
