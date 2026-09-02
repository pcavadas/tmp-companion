//! Benefit-aware headroom trade + the clamp-error taxonomy the leveling lanes report.
//!
//! THE TRADE. When a sound cannot reach its loudness target at the top of its own handle,
//! the run can buy it headroom by raising the preset's base `presetLevel` — a pure linear
//! post-chain amplitude control (`captured_LUFS = 20·log10(presetLevel) + C`, HW-verified
//! fw 1.8.45 to ±0.002 dB THROUGH an active post-amp compressor) — and paying for it by
//! lowering the BASE amp's `outputLevel` so the base sound itself stays exactly where the
//! user asked for it.
//!
//! WHO ACTUALLY BENEFITS (the whole reason this module is not a one-liner). A base
//! `presetLevel` rise scales EVERY sound of the preset; the compensating base-fader drop
//! scales only the sounds that RENDER THROUGH the base fader value — see
//! [`benefits_from_base_raise`]'s doc for the overlay table that decides which sounds those
//! are. A footswitch sound inherits the benefit of the SCENE CONTEXT it is measured in; the
//! BASE sound is held at target by construction, so it never benefits and never needs to — the
//! trade exists precisely to leave it where it was.
//!
//! Hence [`plan_headroom_trade`] triggers ONLY when a *benefiting* sound clamps, and reports
//! every other clamp honestly ([`ClampKind::SceneCeiling`]) rather than churning the base
//! pair for no gain.
//!
//! WHAT THIS MODULE DELIBERATELY DOES NOT DO: compute the compensating fader value. Amp
//! `outputLevel` response through a real chain is NOT algebraically predictable (HW,
//! fw 1.8.45: a 0.5 fader through a soft-knee compressor measured −4.98 dB where the naive
//! taper says −6.02), so the base hold must be SOLVED with the existing bounded secant. The
//! naive `20·log10` arithmetic here is used for ONE thing only — a conservative PRE-FILTER
//! that keeps the planner from asking for a raise no fader could plausibly absorb. When the
//! real solve then pins at [`BASE_FADER_FLOOR`] anyway, that is reported at RUN time as
//! [`ClampKind::TradeFloor`], not predicted here.
//!
//! Everything in this module is pure. The device-facing half lives in `leveller.rs`.

/// The lowest base amp `outputLevel` the trade may solve down to. `outputLevel = 0` is DEEP
/// DIGITAL SILENCE on the real TMP (`danger.md`) and `loudest_loudness` errors on a silent
/// capture — a finite LUFS is not recoverable from it — so the floor sits just above, which
/// still leaves ~40 dB of trade room.
pub const BASE_FADER_FLOOR: f32 = 0.01;

/// The amplitude ceiling both linear controls clamp to — `presetLevel` and the base amp's
/// `outputLevel` share one `[0, 1]` range on this device.
const PRESET_LEVEL_MAX: f32 = crate::leveller::LEVEL_MAX;

/// The QUIETEST value in `currents` that is still ABOVE `floor` — the lane that binds a joint
/// scale-DOWN, since every lane moves by ONE factor and the nearest to the floor runs out
/// first. `None` = no audible lane at all (every value is at or below the floor).
///
/// A lane at or below `floor` is AUTHOR-PARKED and deliberately excluded: including it would
/// forbid every scale-down (`joint_k_floor`) and would report zero fader room for a preset the
/// trade can perfectly well pay from its audible lane. Shared by that solver bound and by the
/// planner's own `base_fader` fold, which must agree about which lane binds or the plan asks
/// for a raise the solve then refuses — folding the MAX instead of the min would have promised
/// a 0.8/0.02 pair ~38 dB of room when the 0.02 lane actually runs out after ~6.
pub(crate) fn min_audible_above(currents: impl Iterator<Item = f32>, floor: f32) -> Option<f32> {
    currents.filter(|c| *c > floor).reduce(f32::min)
}

/// Acceptance band for "this sound is at its target" — the scene lane's
/// [`crate::leveller::KNOB_TOL_LU`]. A deficit inside the band is not a clamp and must never
/// trigger a trade: the planner and the runner have to agree about what "done" means, or a
/// row reported done by one is churned by the other.
pub(crate) const TRADE_CLAMP_EPS_LU: f64 = crate::leveller::KNOB_TOL_LU;

/// WHICH sound a trade row / clamp error describes. Carried by identity, never by position —
/// a batch FILTERS failed rows out of its result vector, so a positional zip mislabels every
/// row after the first failure (the same lesson `validate_log`'s header records).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SoundId {
    /// The preset's base sound (all block-acting footswitches off).
    Base,
    /// One scene, by its 0-based `scenes[]` wire index.
    Scene {
        #[serde(rename = "sceneSlot")]
        scene_slot: u32,
    },
}

/// Why a sound could not be put on its target. Serializable and DISTINCT per cause: the UI
/// (and the e2e oracle) must be able to tell "the chain simply cannot get there" apart from
/// "we refused to gut the effect", "the trade ran out of fader", and "we backed the whole
/// thing out".
///
/// Additive on the wire: this rides ALONGSIDE the existing `clamp_reason` free-text field,
/// which stays pinned to its documented strings ("no signal on USB 1/2", the no-authority
/// wording). Nothing downstream keys on `clamp_reason` losing a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClampKind {
    /// Ordinary headroom clamp: the sound's handle is at the bound that blocks the needed
    /// direction and the target is still out of reach — after the trade, if one ran.
    SceneCeiling,
    /// The solve pinned at the wet/mix PRESERVATION FLOOR (`WET_FLOOR_FRACTION` × the
    /// authored value). The target is below what the handle may write without gutting the
    /// effect the player wrote, so the floor wins and the row clamps.
    WetFloor,
    /// The headroom trade could not hold the BASE sound at its target: the compensating base
    /// `outputLevel` solve pinned at [`BASE_FADER_FLOOR`]. Distinct from `SceneCeiling`
    /// because the failing control is the base fader, not the row's own handle.
    TradeFloor,
    /// The trade's base `presetLevel` + `outputLevel` pair was raised, but a DEPENDENT write
    /// (the sound the trade was bought for) did not land — so the run backed the pair out and
    /// persisted NOTHING. Reported so the row is never silently "done at the old value" while
    /// the preset carries half a trade.
    PartialTrade,
    /// The pre-existing off-branch / off-USB case: a big `outputLevel` move that did not move
    /// the USB 1/2 capture at all. Kept in the taxonomy so `clamp_kind` is TOTAL — every
    /// clamped row can name its cause — rather than leaving one cause readable only from the
    /// free-text `clamp_reason`.
    NoAuthority,
}

impl ClampKind {
    /// The ONE mapping from a solve's clamp FLAGS onto this taxonomy. Pure, so every lane's
    /// answer is unit-testable without a device — and kept HERE, next to the values and
    /// [`Self::message`].
    ///
    /// ORDER IS LOAD-BEARING. `clamp_reason` is the pre-existing routing/no-authority signal
    /// and its presence outranks everything else — a row whose knob never reached USB 1/2 has
    /// not "run out of wet floor", it was never heard at all. The wet floor comes next: it is
    /// a REFUSAL to write lower, so the bound that stopped the solve is the floor, not the
    /// chain's ceiling. Anything else clamped is the ordinary headroom case.
    ///
    /// [`Self::TradeFloor`] and [`Self::PartialTrade`] are never produced here — they describe
    /// the base pair's own fate, which no per-row solve can observe; the trade executor stamps
    /// those (`leveller::trade_hold_failure_kind`).
    pub fn from_flags(clamped: bool, wet_floor: bool, clamp_reason: Option<&str>) -> Option<Self> {
        if !clamped {
            return None;
        }
        Some(if clamp_reason.is_some() {
            Self::NoAuthority
        } else if wet_floor {
            Self::WetFloor
        } else {
            Self::SceneCeiling
        })
    }

    /// One user-facing sentence per cause. Kept here (not at the call sites) so the Level
    /// summary, the footswitch lane and the scene lane can never word the same cause two
    /// ways.
    pub fn message(self) -> &'static str {
        match self {
            Self::SceneCeiling => {
                "this sound can’t reach the target because its level control is already maxed out"
            }
            Self::WetFloor => {
                "this sound can’t reach the target without turning the effect’s mix down too \
                 far to still work"
            }
            Self::TradeFloor => {
                "giving this sound headroom used up the base amp’s spare room, so the base \
                 sound slipped off target"
            }
            Self::PartialTrade => {
                "a related write failed, so the headroom trade was undone and nothing was saved"
            }
            Self::NoAuthority => {
                "this level control has no effect on the sound coming out of USB 1/2"
            }
        }
    }
}

/// One sound as the trade planner sees it: what it can reach today, what it was asked for,
/// and whether a base `presetLevel` rise would actually help it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TradeSound {
    pub id: SoundId,
    /// Maximum reachable loudness AT THE CURRENT base `presetLevel` — the prepass ceiling
    /// (measured as-is, extrapolated to the handle's top bound). LUFS.
    pub ceiling_lufs: f64,
    /// The target this row asks for — the per-row override when the user set one, else the
    /// global. LUFS.
    pub target_lufs: f64,
    /// Does a `+Δ dB` base `presetLevel` rise raise THIS sound's ceiling by `+Δ dB`? True iff
    /// the sound's `outputLevel` is pinned independently of base — see the module header.
    /// Derived from the overlay dependency structure by [`benefits_from_base_raise`], never
    /// guessed.
    pub benefits: bool,
}

impl TradeSound {
    /// How far short of its target this sound falls today (LU). Negative = it has room to
    /// spare.
    pub fn deficit_lu(&self) -> f64 {
        self.target_lufs - self.ceiling_lufs
    }
}

/// Why the planner asked for LESS raise than the worst benefiting deficit wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TradeCap {
    /// `presetLevel` would have had to exceed 1.0.
    PresetLevelMax,
    /// The base amp fader would have had to go below [`BASE_FADER_FLOOR`] to hold base.
    BaseFaderFloor,
}

/// The plan: how much to raise base `presetLevel`, and why it was trimmed if it was.
///
/// DELIBERATELY NOT A FORECAST — every real verdict comes from the device solve that follows
/// (`run_scene_jobs`) or, when the trade itself fails, from `BatchTrade::stamp_failure`. A
/// second answer that can disagree with the measured one is worse than no answer.
#[derive(Debug, Clone, PartialEq)]
pub struct TradePlan {
    /// dB to ADD to base `presetLevel`. Exactly the missing dB of the worst benefiting clamp,
    /// trimmed by the caps. `0.0` = no trade: either nothing clamps, or nothing that clamps
    /// would benefit. The caller must not touch the base pair at `0.0`.
    pub raise_db: f64,
    /// Set when a cap trimmed `raise_db` below what the worst benefiting deficit wanted.
    pub capped: Option<TradeCap>,
}

impl TradePlan {
    /// Does this plan ask for any device work on the base pair?
    pub fn is_trade(&self) -> bool {
        self.raise_db > 0.0
    }
}

/// The linear ratio a `db` shift multiplies an amplitude by.
fn db_ratio(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// The base `presetLevel` a `raise_db` trade lands on — exact (module header). Clamped to
/// `PRESET_LEVEL_MAX` as a belt — the planner's own cap already keeps it in range.
pub fn raised_preset_level(preset_level: f32, raise_db: f64) -> f32 {
    let raised = preset_level as f64 * db_ratio(raise_db);
    (raised as f32).clamp(0.0, PRESET_LEVEL_MAX)
}

/// dB of headroom left ABOVE `current`, up to `ceiling` (0 at or above it) — asked of
/// `presetLevel` and of the base fader alike, both against [`PRESET_LEVEL_MAX`].
fn room_up_db(current: f32, ceiling: f32) -> f64 {
    if current <= 0.0 {
        return 0.0;
    }
    (20.0 * (ceiling as f64 / current as f64).log10()).max(0.0)
}

/// dB of attenuation the QUIETEST audible base amp's fader ([`min_audible_above`]) could absorb
/// before [`BASE_FADER_FLOOR`]. PRE-FILTER ONLY — it bounds the ASK, never predicts the ANSWER.
fn base_fader_room_db(base_fader: f32) -> f64 {
    if base_fader <= BASE_FADER_FLOOR {
        return 0.0;
    }
    (20.0 * (base_fader as f64 / BASE_FADER_FLOOR as f64).log10()).max(0.0)
}

/// Does raising base `presetLevel` and re-leveling this scene's OWN row actually put the row
/// somewhere the compensating base-fader drop can't take back? Answered off `SceneOverlay`'s
/// "scene-writable" shape, not off ceiling-inheritance physics: `Full` already carries its own
/// pinned `outputLevel`, so the raise lands there whole. `Absent` has no overlay YET, but
/// `set_knobs`' Scene Edit enable MATERIALIZES one the moment this scene's own row is solved
/// (PHASE 3) — after that write the scene is exactly as independent of base as a `Full` scene
/// always was, so it benefits too (the earlier "Absent inherits base, net-zero" answer was
/// only true of an UNWRITTEN scene, and every benefiting scene in this batch gets written).
/// `BypassOnly` is REFUSED outright by `set_knobs` for a scene-scoped write (its Scene Edit
/// flag is off, knobs share base) — a raise buys it nothing, since no per-scene write can ever
/// land. `Unknown` (a truncated field-8 read) stays the conservative NO: a wrong YES churns the
/// base pair for a sound that may gain nothing and leaves every other sound quieter.
pub fn benefits_from_base_raise(overlay: &crate::probe_api::scene_jobs::SceneOverlay<'_>) -> bool {
    matches!(
        overlay,
        crate::probe_api::scene_jobs::SceneOverlay::Full(_)
            | crate::probe_api::scene_jobs::SceneOverlay::Absent
    )
}

/// Does a base `presetLevel` rise raise THIS SCENE'S ALREADY-MEASURED prepass reading by
/// exactly `raise_db`, so the write phase can reuse it instead of re-measuring? Narrower than
/// [`benefits_from_base_raise`] on purpose: that predicate asks "will this scene end up
/// independent of base once PHASE 3 writes it", which is true for `Full` AND `Absent` (the
/// enable materializes the overlay). This one asks "is the reading ALREADY taken — before that
/// write — the right one to shift", which is true ONLY for `Full`: its prepass capture already
/// rendered through the scene's OWN pinned `outputLevel`, untouched by the base-fader drop, so
/// `+raise_db` is exact. An `Absent` scene's prepass rendered through BASE's fader (no overlay
/// existed yet) — raise UP and base-fader drop DOWN net to ~zero at that moment — so shifting
/// it by `+raise_db` would predict a ceiling the scene never had; the reading is dropped
/// instead ([`crate::leveller::retarget_prepass_after_trade`]) and the scene's own PHASE-3
/// solve re-measures fresh, AFTER its own overlay exists.
pub fn retains_prepass_after_raise(
    overlay: &crate::probe_api::scene_jobs::SceneOverlay<'_>,
) -> bool {
    matches!(overlay, crate::probe_api::scene_jobs::SceneOverlay::Full(_))
}

/// The worst *benefiting* clamp's missing dB — the TRIGGER for a base-pair trade. A
/// non-benefiting clamp cannot be helped by the rise and must never drive one.
fn benefiting_deficit_db(sounds: &[TradeSound]) -> f64 {
    sounds
        .iter()
        .filter(|s| s.benefits && s.deficit_lu() > TRADE_CLAMP_EPS_LU)
        .map(TradeSound::deficit_lu)
        .fold(0.0f64, f64::max)
}

/// The fader SEED for the closed-loop solve that follows — never the answer (module header).
/// `None` at `df_db == 0`: seeding the fader to the value it already holds is a spurious write.
fn seed_fader_target(base_fader: f32, df_db: f64) -> Option<f32> {
    if df_db == 0.0 {
        return None;
    }
    let seeded = base_fader as f64 * db_ratio(df_db);
    Some((seeded as f32).clamp(BASE_FADER_FLOOR, PRESET_LEVEL_MAX))
}

/// The joint plan: how much to move `presetLevel` and the base fader, and why. See
/// [`plan_level_pair`]'s doc for the physics and the split policy.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelPairPlan {
    /// dB to ADD to base `presetLevel`. Never negative — this planner never lowers
    /// `presetLevel` below its authored value (module header: the existing trade only ever
    /// raises it, and BOOST inherits that convention).
    pub dp_db: f64,
    /// dB to ADD to the base fader's `outputLevel` (SIGNED — negative is the ordinary trade's
    /// compensating drop, positive is a boost's raise). `0.0` iff [`Self::fader_target`] is
    /// `None`. Read only by U7, which gates `Δp + Δf == G`: [`Self::fader_target`] is a
    /// CLAMPED seed, so it cannot stand in for this without hiding a violated identity.
    pub df_db: f64,
    /// The base `presetLevel` this plan lands on — exact (module header), via
    /// [`raised_preset_level`].
    pub preset_level: f32,
    /// The base fader's SEED value for the closed-loop solve that must follow — never a
    /// prediction of the solved answer (module header). `None` means "do not touch the
    /// fader": either nothing needs to move, or `presetLevel` alone already covers it.
    pub fader_target: Option<f32>,
    /// BOOST: `presetLevel` pinned at its ceiling and the fader raised past its
    /// absorb-a-raise role to close the rest. `false` covers the other three outcomes —
    /// see [`plan_level_pair`].
    pub boost: bool,
    /// Which bound trimmed the ask below what it wanted, if one did. Always `PresetLevelMax`
    /// on a BOOST (that outcome exists precisely because `presetLevel` pinned); on a trade,
    /// set when the worse of the two asks (benefiting deficit vs. the base's own `G`)
    /// exceeded what the bounds could supply.
    pub capped: Option<TradeCap>,
}

/// THE JOINT PLANNER, pure. `Δp + Δf = G = base_target_lufs − base_asis_lufs` — one measurement
/// fixes the total; the SPLIT RULE puts as much of `G` on `presetLevel` as bounds allow, so the
/// inexact fader (module header) moves least. Four outcomes:
///
/// * no move — base on target, nothing benefiting clamped.
/// * trade — `presetLevel` rises by `max(benefiting deficit, G)`; `G == 0` is the U1 shape.
/// * boost ([`LevelPairPlan::boost`]) — `presetLevel` pins, the fader closes the rest UPWARD.
/// * no move, infeasible — both at their limits; the caller reports today's honest clamp.
pub fn plan_level_pair(
    sounds: &[TradeSound],
    base_asis_lufs: f64,
    base_target_lufs: f64,
    preset_level: f32,
    base_fader: f32,
) -> LevelPairPlan {
    let g = base_target_lufs - base_asis_lufs;
    // BOOST needs the fader's UPWARD room: a base short even at `presetLevel`'s own ceiling
    // has nowhere left to buy headroom but the fader, and the ordinary trade's DOWNWARD room
    // (`base_fader_room_db`) cannot supply a raise.
    let p_up = room_up_db(preset_level, PRESET_LEVEL_MAX);
    let f_up = room_up_db(base_fader, PRESET_LEVEL_MAX);
    let f_dn = -base_fader_room_db(base_fader);

    // Feasible Δp window: Δf = G − Δp must land in [F_dn, F_up], and Δp itself must land in
    // [0, P_up] (this planner never lowers presetLevel below its authored value).
    let dp_lo = (g - f_up).max(0.0);
    let dp_hi = (g - f_dn).min(p_up);

    let no_move = || LevelPairPlan {
        dp_db: 0.0,
        df_db: 0.0,
        preset_level,
        fader_target: None,
        boost: false,
        capped: None,
    };

    // ORDER IS LOAD-BEARING (same discipline as `ClampKind::from_flags`). Infeasibility must
    // be checked BEFORE boost: a base short enough to trigger boost's own condition can ALSO
    // fail feasibility (both controls already maxed and still short of G), and boost-first
    // would report a fader raise the bounds just proved cannot happen.
    if dp_lo > dp_hi {
        return no_move();
    }

    if g > p_up + TRADE_CLAMP_EPS_LU {
        // BOOST. Feasibility (just checked) guarantees dp_hi == p_up here: F_dn <= 0 always
        // (base_fader_room_db never returns a negative room), so G - F_dn >= G > P_up, hence
        // dp_hi = min(P_up, G - F_dn) = P_up. Written as `p_up.min(dp_hi)` anyway so the
        // "presetLevel to its ceiling" intent reads directly off the arithmetic.
        let dp = p_up.min(dp_hi);
        let df = g - dp;
        return LevelPairPlan {
            dp_db: dp,
            df_db: df,
            preset_level: raised_preset_level(preset_level, dp),
            fader_target: seed_fader_target(base_fader, df),
            boost: true,
            // presetLevel is pinned at its own ceiling BY DEFINITION of this branch.
            capped: Some(TradeCap::PresetLevelMax),
        };
    }

    let d_ben = benefiting_deficit_db(sounds);

    // Both halves of this test matter: `plan_headroom_trade`'s wrapper always calls with
    // `base_asis_lufs == base_target_lufs`, so G is EXACTLY zero on every legacy call —
    // gating on G alone would silence every legacy trade the moment a benefiting sound
    // clamped, which the U1 equivalence gate forbids.
    if g.abs() <= TRADE_CLAMP_EPS_LU && d_ben <= TRADE_CLAMP_EPS_LU {
        return no_move();
    }

    // TRADE. The ask is the LARGER of what a benefiting sound wants and what the base's own G
    // demands — G must never be shorted here, or the fader (the UNPREDICTABLE control) absorbs
    // it whenever `d_ben` falls short, exactly the excursion the split rule avoids.
    let ask = d_ben.max(g);
    let dp = ask.clamp(dp_lo, dp_hi);
    let df = g - dp;
    let capped = if ask > dp_hi {
        // Same tie-break as the legacy formula: the SMALLER room is what actually bound the
        // raise. At G == 0 this is bit-for-bit the old `pl_room <= fader_room` comparison.
        Some(if p_up <= g - f_dn {
            TradeCap::PresetLevelMax
        } else {
            TradeCap::BaseFaderFloor
        })
    } else {
        None
    };

    LevelPairPlan {
        dp_db: dp,
        df_db: df,
        preset_level: raised_preset_level(preset_level, dp),
        fader_target: seed_fader_target(base_fader, df),
        boost: false,
        capped,
    }
}

/// [`plan_level_pair`] at `G == 0`, which can only answer no-move or trade: this lane's base is
/// on target by construction, buying headroom for OTHER sounds while base holds. `base_fader` is
/// the QUIETEST audible BASE amp's `outputLevel` ([`min_audible_above`]); U1 gates the equivalence.
pub fn plan_headroom_trade(sounds: &[TradeSound], preset_level: f32, base_fader: f32) -> TradePlan {
    let plan = plan_level_pair(sounds, 0.0, 0.0, preset_level, base_fader);
    TradePlan {
        raise_db: plan.dp_db,
        capped: plan.capped,
    }
}

/// RE-PLAN after a first attempt's base hold pinned at [`BASE_FADER_FLOOR`], at the raise the
/// hardware turned out to afford. `None` = not worth another attempt.
///
/// WHY THIS EXISTS AND WHY IT IS BOUNDED TO ONE USE. D4 says the trade repeats "until no room
/// remains", and [`plan_headroom_trade`]'s fader pre-filter is only a naive `20·log10` bound —
/// the real fader response is not algebraically predictable (module header), so the FIRST
/// measured floor pin is the only honest evidence of where the room actually ended. The
/// affordable raise is the first ask MINUS the base sound's measured overshoot (presetLevel is
/// exact — module header), so this is a MEASURED retry, not a second guess. It is used
/// exactly ONCE: a second squash would again be unpredictable, and every attempt costs a full
/// engage-per-capture hold solve.
///
/// The arithmetic AND the worth-retrying test live here rather than at the command that calls
/// it, so this module owns both of the run's plan derivations and they can never disagree
/// about what "there is still room" means: the retry threshold is the same
/// [`TRADE_CLAMP_EPS_LU`] acceptance band the planner refuses to trade inside of.
///
/// `capped` is always [`TradeCap::BaseFaderFloor`] — by construction this plan exists only
/// because the fader floor trimmed the ask.
pub fn replan_after_floor_pin(raise_db: f64, base_overshoot_lu: f64) -> Option<TradePlan> {
    let affordable = raise_db - base_overshoot_lu;
    (affordable > TRADE_CLAMP_EPS_LU).then_some(TradePlan {
        raise_db: affordable,
        capped: Some(TradeCap::BaseFaderFloor),
    })
}

/// ONE base amp the trade's hold moved (or WOULD move) — the per-lane half of
/// [`TradeSummary`].
/// SNAKE_CASE on the wire, like every other LEVELLER-tier payload (`LevelResult`,
/// `FootswitchLevelResult`, `BatchedSceneOutcome`): the repo's casing splits by LAYER, not by
/// module — command/wizard argument types are camelCase, leveling RESULTS are snake_case (see
/// `src/lib/types.ts`'s contract note). [`SoundId`]'s own `kind`/`sceneSlot` keys are a
/// deliberate exception and stay as they are: they are a tagged IDENTITY, pinned by test.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TradeAmpMove {
    pub group_id: String,
    pub node_id: String,
    pub parameter_id: String,
    /// The `outputLevel` the preset carried BEFORE the trade — the Restore anchor.
    pub previous_value: f32,
    /// The SOLVED value the hold landed on. `None` on a TRADE advisory: the fader response is
    /// not algebraically predictable, so a run that did not actually solve it must not invent
    /// one. Exception: a BOOST advisory (`BaseBoostSummary.applied == false`) populates this
    /// with `LevelPairPlan::fader_target` — that field is documented as a SEED, not a solved
    /// prediction, but the plan's own disclosure wording names it explicitly ("...raised the
    /// amp's output from 0.28 to 0.51..." / "would raise..." for the unapplied case), so the
    /// UI needs a number here even before any closed-loop solve has run.
    pub value: Option<f32>,
}

/// THE TRADE, ON THE WIRE. `danger.md`: a save cannot be undone from the app — so a run that
/// moved the preset's whole gain structure has to SAY so, with enough detail for the UI to
/// disclose it and offer a restore.
///
/// ONE shape serves both cases, discriminated by [`Self::applied`]:
/// * `true` — the base pair was raised and held, and (on a `save` run) persisted.
/// * `false` — ADVISORY. A no-save run plans the trade but does not execute it (executing it
///   would be wiped by the run's own restore, taking the benefiting rows' re-targeted
///   readings with it), so this reports what WOULD be traded and every clamped benefiting row
///   keeps its honest clamp.
///
/// Snake_case on the wire — see [`TradeAmpMove`] for the layer rule.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TradeSummary {
    pub applied: bool,
    /// dB added to the base `presetLevel`.
    pub raise_db: f64,
    pub previous_preset_level: f32,
    /// The raised `presetLevel` — exact either way (module header), so an advisory can state
    /// it without measuring.
    pub preset_level: f32,
    pub base_amps: Vec<TradeAmpMove>,
    /// Why the raise was trimmed below what the worst benefiting clamp wanted, if it was.
    pub cap: Option<TradeCap>,
    /// The SOUNDS the raise was bought for, by identity. Not a bare slot list: the taxonomy
    /// already distinguishes a scene from a footswitch-in-a-scene from base, and flattening
    /// that to `u32` would make the FS lane's rows indistinguishable from their context scene's
    /// the moment it reports a trade of its own.
    pub benefiting: Vec<SoundId>,
}

/// The base row's own BOOST on the wire — the mirror image of [`TradeSummary`]'s downward trade,
/// sharing its `applied`/advisory discriminator, disclosure rationale and snake_case.
///
/// A BOOST advisory populates `base_amps[0].value` with the planner's SEED rather than leaving
/// it `None` — the one exception, argued at [`TradeAmpMove::value`].
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct BaseBoostSummary {
    pub applied: bool,
    /// The raised `presetLevel` — exact either way (module header), so an advisory can state
    /// it without measuring.
    pub preset_level: f32,
    /// The base amp candidate the boost moves. Exactly one element: v1 boost refuses when more
    /// than one amp candidate is eligible (see `commands::level_preset`'s single-job guard).
    pub base_amps: Vec<TradeAmpMove>,
}

impl BaseBoostSummary {
    /// Build from the plan + the base amp's before/after values — the ONE construction both
    /// the applied path (`leveller::apply_base_boost`) and the advisory path
    /// (`leveller::level_preset_impl`) use, so the two can never state `preset_level` two
    /// different ways.
    pub(crate) fn from_plan(applied: bool, plan: &LevelPairPlan, amp: TradeAmpMove) -> Self {
        BaseBoostSummary {
            applied,
            preset_level: plan.preset_level,
            base_amps: vec![amp],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(slot: u32, ceiling: f64, target: f64, benefits: bool) -> TradeSound {
        TradeSound {
            id: SoundId::Scene { scene_slot: slot },
            ceiling_lufs: ceiling,
            target_lufs: target,
            benefits,
        }
    }

    fn base(ceiling: f64, target: f64) -> TradeSound {
        TradeSound {
            id: SoundId::Base,
            ceiling_lufs: ceiling,
            target_lufs: target,
            // The base sound is HELD at target by the trade; it never benefits from it.
            benefits: false,
        }
    }

    // Nothing clamps ⇒ no trade at all. The base pair must not be touched on a run that had
    // no problem to solve — every write is a chance to drop one.
    #[test]
    fn a_run_with_no_clamp_plans_no_trade() {
        let plan = plan_headroom_trade(
            &[base(-14.0, -23.0), scene(0, -15.0, -23.0, true)],
            0.5,
            0.8,
        );
        assert_eq!(plan.raise_db, 0.0);
        assert!(!plan.is_trade());
        assert_eq!(plan.capped, None);
    }

    // ⟦R2⟧ THE CENTRAL RULE. A clamp on a sound the trade cannot help must be reported
    // honestly and must NOT move the base pair — the raise buys it nothing while making
    // every other sound's fader drop for free.
    #[test]
    fn a_clamp_that_would_not_benefit_never_triggers_the_trade() {
        // Scene 1 inherits base's outputLevel (net-zero) and is 4 LU short.
        let plan = plan_headroom_trade(
            &[base(-14.0, -23.0), scene(1, -19.0, -15.0, false)],
            0.5,
            0.8,
        );
        assert_eq!(plan.raise_db, 0.0, "no benefiting clamp ⇒ no trade");
        assert_eq!(
            plan.capped, None,
            "and no cap is blamed — nothing was asked for"
        );
    }

    // The raise is EXACTLY the missing dB (module header), and the benefiting sound then
    // reaches its target.
    #[test]
    fn a_benefiting_clamp_buys_exactly_its_missing_db() {
        let plan = plan_headroom_trade(
            &[base(-14.0, -23.0), scene(2, -19.0, -15.0, true)],
            0.5,
            0.8,
        );
        assert!((plan.raise_db - 4.0).abs() < 1e-9, "{:?}", plan.raise_db);
        assert_eq!(plan.capped, None);
    }

    // The WORST benefiting deficit sets the raise; a smaller one rides along.
    #[test]
    fn the_raise_covers_the_worst_benefiting_clamp() {
        let plan = plan_headroom_trade(
            &[
                scene(0, -19.0, -15.0, true),  // 4 LU short
                scene(1, -21.5, -15.0, true),  // 6.5 LU short — the binding one
                scene(2, -30.0, -15.0, false), // 15 LU short but cannot benefit
            ],
            0.4,
            0.9,
        );
        assert!(
            (plan.raise_db - 6.5).abs() < 1e-9,
            "the 6.5 LU benefiting deficit binds, NOT scene 2's 15 — a sound the raise \
             cannot help must never drag the whole preset's gain structure around: {:?}",
            plan.raise_db
        );
        assert_eq!(plan.capped, None);
    }

    // presetLevel cannot exceed 1.0, so a preset already near the top can only trade what is
    // left — and the shortfall is REPORTED, never silently swallowed.
    #[test]
    fn the_raise_is_capped_by_preset_levels_own_ceiling() {
        // pl 0.5 ⇒ 6.02 dB of room; the clamp wants 12.
        let plan = plan_headroom_trade(&[scene(0, -27.0, -15.0, true)], 0.5, 0.9);
        assert!((plan.raise_db - 6.0206).abs() < 1e-3, "{:?}", plan.raise_db);
        assert_eq!(plan.capped, Some(TradeCap::PresetLevelMax));
    }

    // The base fader has to absorb the whole raise to hold base on target. A fader already
    // near the floor bounds the ask — the PRE-FILTER, not a prediction of the solve.
    #[test]
    fn the_raise_is_capped_by_the_base_fader_floor() {
        // fader 0.02 ⇒ 6.02 dB above the 0.01 floor; pl 0.05 leaves 26 dB, so the fader binds.
        let plan = plan_headroom_trade(&[scene(0, -27.0, -15.0, true)], 0.05, 0.02);
        assert!((plan.raise_db - 6.0206).abs() < 1e-3, "{:?}", plan.raise_db);
        assert_eq!(plan.capped, Some(TradeCap::BaseFaderFloor));
    }

    // ⟦4c⟧ THE ONE BOUNDED RE-PLAN (see fn doc above for the arithmetic and why it lives here).
    #[test]
    fn a_replan_after_a_floor_pin_asks_for_the_measured_affordable_raise() {
        let plan = replan_after_floor_pin(6.0, 3.5).expect("3.5 LU of room is worth a retry");
        assert!((plan.raise_db - 2.5).abs() < 1e-9, "{:?}", plan.raise_db);
        assert_eq!(
            plan.capped,
            Some(TradeCap::BaseFaderFloor),
            "by construction this plan exists only because the fader floor trimmed the ask"
        );
        // Inside the acceptance band there is nothing left to buy — the planner and the retry
        // have to agree about "done", so the same band decides both.
        assert_eq!(
            replan_after_floor_pin(6.0, 6.0 - TRADE_CLAMP_EPS_LU),
            None,
            "a raise inside the acceptance band is not worth a second engage-per-capture hold"
        );
        assert_eq!(
            replan_after_floor_pin(2.0, 4.0),
            None,
            "and neither is a negative one"
        );
    }

    // ⟦4b⟧ THE SHARED FOLD (see min_audible_above's doc for why the quietest lane binds).
    #[test]
    fn the_audible_fold_takes_the_quietest_lane_and_ignores_the_parked_ones() {
        assert_eq!(
            min_audible_above([0.8, 0.02, 0.5].into_iter(), BASE_FADER_FLOOR),
            Some(0.02)
        );
        assert_eq!(
            min_audible_above([0.8, BASE_FADER_FLOOR, 0.0].into_iter(), BASE_FADER_FLOOR),
            Some(0.8),
            "a lane AT or below the floor is author-parked and does not bind"
        );
        assert_eq!(
            min_audible_above([0.0, BASE_FADER_FLOOR].into_iter(), BASE_FADER_FLOOR),
            None,
            "no audible lane at all ⇒ no answer, which the callers read as zero room"
        );
        assert_eq!(min_audible_above(std::iter::empty(), 0.0), None);
    }

    // A fader already AT the floor can absorb nothing: no trade, and the clamp is honest.
    #[test]
    fn a_fader_already_at_the_floor_trades_nothing() {
        let plan = plan_headroom_trade(&[scene(0, -27.0, -15.0, true)], 0.5, BASE_FADER_FLOOR);
        assert_eq!(plan.raise_db, 0.0);
        assert_eq!(plan.capped, Some(TradeCap::BaseFaderFloor));
    }

    // A deficit inside the acceptance band is NOT a clamp: the planner and the runner must
    // agree about "done", or a row one calls finished the other churns.
    #[test]
    fn a_deficit_inside_the_acceptance_band_is_not_a_clamp() {
        let inside = TRADE_CLAMP_EPS_LU - 0.01;
        let plan = plan_headroom_trade(&[scene(0, -15.0 - inside, -15.0, true)], 0.5, 0.9);
        assert_eq!(plan.raise_db, 0.0, "no trade for a row already in band");
    }

    // The raise is applied to presetLevel EXACTLY (module header), not through any taper model.
    #[test]
    fn the_raised_preset_level_is_the_exact_linear_solution() {
        assert!((raised_preset_level(0.5, 6.0206) - 1.0).abs() < 1e-4);
        assert!((raised_preset_level(0.25, 6.0206) - 0.5).abs() < 1e-4);
        assert!((raised_preset_level(0.5, 0.0) - 0.5).abs() < 1e-6);
        // Belt: never above the control's own ceiling.
        assert_eq!(raised_preset_level(0.9, 20.0), 1.0);
    }

    // The benefit answer comes from the OVERLAY STRUCTURE, and an unreadable one is answered
    // NO — the conservative side (a wrong YES churns the base pair and leaves every other
    // sound quieter for nothing). Full AND Absent both accept: `set_knobs`' Scene Edit enable
    // materializes an overlay for an Absent scene the moment PHASE 3 writes it, so by the time
    // the raise matters that scene is exactly as independent of base as a Full one.
    #[test]
    fn benefit_is_read_off_the_overlay_full_and_absent_accept_bypassonly_and_unknown_refuse() {
        use crate::probe_api::scene_jobs::SceneOverlay;
        let params = serde_json::Map::new();
        assert!(benefits_from_base_raise(&SceneOverlay::Full(&params)));
        assert!(
            benefits_from_base_raise(&SceneOverlay::Absent),
            "the enable materializes the overlay when PHASE 3 writes this scene's own row"
        );
        assert!(!benefits_from_base_raise(&SceneOverlay::BypassOnly(
            &params
        )));
        assert!(!benefits_from_base_raise(&SceneOverlay::Unknown));
    }

    // `retains_prepass_after_raise` is the NARROWER, Full-only predicate: an Absent scene
    // benefits from the eventual raise but its ALREADY-TAKEN prepass reading rendered through
    // base's (not yet raised) fader and must be dropped, not shifted.
    #[test]
    fn retains_prepass_after_raise_is_full_only() {
        use crate::probe_api::scene_jobs::SceneOverlay;
        let params = serde_json::Map::new();
        assert!(retains_prepass_after_raise(&SceneOverlay::Full(&params)));
        assert!(!retains_prepass_after_raise(&SceneOverlay::Absent));
        assert!(!retains_prepass_after_raise(&SceneOverlay::BypassOnly(
            &params
        )));
        assert!(!retains_prepass_after_raise(&SceneOverlay::Unknown));
    }

    // Every clamp cause has its own wording — the taxonomy exists so the UI can tell them
    // apart, which it cannot do if two causes read the same.
    #[test]
    fn every_clamp_kind_has_a_distinct_message() {
        let all = [
            ClampKind::SceneCeiling,
            ClampKind::WetFloor,
            ClampKind::TradeFloor,
            ClampKind::PartialTrade,
            ClampKind::NoAuthority,
        ];
        let mut seen: Vec<&str> = all.iter().map(|k| k.message()).collect();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(before, seen.len(), "two clamp kinds share a message");
        assert!(seen.iter().all(|m| !m.is_empty()));
    }

    // ⟦6⟧ THE TRADE ON THE WIRE. `danger.md`: a save cannot be undone from the app, so a
    // landed trade must be disclosable — and an ADVISORY must be tellable apart from it
    // without the consumer guessing (an advisory never solved a fader, so it says so).
    #[test]
    fn the_trade_summary_serializes_snake_case_and_distinguishes_advisory_from_applied() {
        let amp = TradeAmpMove {
            group_id: "G1".into(),
            node_id: "amp".into(),
            parameter_id: "outputLevel".into(),
            previous_value: 0.8,
            value: None,
        };
        let advisory = TradeSummary {
            applied: false,
            raise_db: 4.0,
            previous_preset_level: 0.5,
            preset_level: raised_preset_level(0.5, 4.0),
            base_amps: vec![amp],
            cap: Some(TradeCap::BaseFaderFloor),
            benefiting: vec![SoundId::Scene { scene_slot: 2 }],
        };
        let json = serde_json::to_value(&advisory).expect("serialize");
        assert_eq!(json["applied"], false);
        // LEVELLER-TIER = snake_case keys, like every other leveling result payload.
        assert_eq!(json["raise_db"], 4.0);
        assert_eq!(json["previous_preset_level"], 0.5);
        assert_eq!(json["cap"], "base_fader_floor");
        // The benefiting set keeps its IDENTITY tag.
        assert_eq!(
            json["benefiting"],
            serde_json::json!([{ "kind": "scene", "sceneSlot": 2 }])
        );
        let prev = json["base_amps"][0]["previous_value"]
            .as_f64()
            .expect("previous_value is a number");
        assert!((prev - 0.8).abs() < 1e-6, "{prev}");
        assert!(
            json["base_amps"][0]["value"].is_null(),
            "an advisory never solved the fader — it must not invent one"
        );
        assert_eq!(json["base_amps"][0]["group_id"], "G1");
    }

    // The wire shape the frontend and the e2e oracle read.
    #[test]
    fn the_taxonomy_serializes_to_stable_snake_case_tokens() {
        let json = |k: ClampKind| serde_json::to_string(&k).expect("serialize");
        assert_eq!(json(ClampKind::SceneCeiling), "\"scene_ceiling\"");
        assert_eq!(json(ClampKind::WetFloor), "\"wet_floor\"");
        assert_eq!(json(ClampKind::TradeFloor), "\"trade_floor\"");
        assert_eq!(json(ClampKind::PartialTrade), "\"partial_trade\"");
        assert_eq!(json(ClampKind::NoAuthority), "\"no_authority\"");
    }

    // Unit gates U1-U9 for `plan_level_pair`; each is named for the fact it pins.

    // U1. THE EQUIVALENCE GATE. `plan_headroom_trade`'s wrapper always calls
    // `plan_level_pair` with G == 0 exactly (base "already on target"), which must reproduce
    // `want.min(pl_room).min(fader_room).max(0.0)` bit-compatibly, with the SAME cap
    // tie-break. Swept over a p0 x f0 x D_ben grid, including the D_ben == 0 boundary.
    #[test]
    fn the_pair_planner_reproduces_plan_headroom_trade_when_base_is_already_on_target() {
        let p0_values = [0.05_f32, 0.27, 0.5, 0.8, 1.0];
        let f0_values = [0.02_f32, 0.28, 0.5, 0.9, 1.0];
        let d_ben_values = [0.0_f64, 2.0, 6.0, 12.0, 20.0];

        for &p0 in &p0_values {
            for &f0 in &f0_values {
                for &d_ben in &d_ben_values {
                    let sounds: Vec<TradeSound> = if d_ben > 0.0 {
                        vec![scene(0, -15.0 - d_ben, -15.0, true)]
                    } else {
                        vec![]
                    };

                    let legacy = plan_headroom_trade(&sounds, p0, f0);
                    let pair = plan_level_pair(&sounds, 0.0, 0.0, p0, f0);

                    let pl_room = room_up_db(p0, PRESET_LEVEL_MAX);
                    let fader_room = base_fader_room_db(f0);
                    let expected_raise = d_ben.min(pl_room).min(fader_room).max(0.0);

                    assert!(
                        (legacy.raise_db - expected_raise).abs() < 1e-6,
                        "legacy oracle drifted at p0={p0} f0={f0} d_ben={d_ben}: {:?}",
                        legacy
                    );
                    assert!(
                        (pair.dp_db - expected_raise).abs() < 1e-6,
                        "pair planner disagrees with the legacy oracle at p0={p0} f0={f0} \
                         d_ben={d_ben}: {:?}",
                        pair
                    );
                    assert_eq!(
                        legacy.raise_db, pair.dp_db,
                        "wrapper must forward the planner's own raise unchanged"
                    );
                    assert_eq!(legacy.capped, pair.capped, "and the same cap verdict");
                }
            }
        }
    }

    // U2. Plumes+BD2+OCD numbers (2026-08-31 investigation): presetLevel alone (P_up ~11.1
    // dB) cannot cover the ~16.4 LU deficit, so it pins at its ceiling and the base fader
    // absorbs the remaining ~5 dB, UPWARD.
    #[test]
    fn base_unreachable_at_preset_level_max_pins_the_level_and_raises_the_fader() {
        let plan = plan_level_pair(&[], -39.37, -23.0, 0.27, 0.28);
        assert!(plan.boost, "{:?}", plan);
        assert_eq!(plan.capped, Some(TradeCap::PresetLevelMax));
        assert!((plan.dp_db - 11.37).abs() < 1e-2, "{:?}", plan);
        assert!((plan.preset_level - 1.0).abs() < 1e-4, "{:?}", plan);
        let fader_target = plan.fader_target.expect("Boost always seeds a fader move");
        assert!((fader_target - 0.498).abs() < 2e-3, "{:?}", plan);
    }

    // U3. Friedman HBE numbers: the base's own deficit (~1.02 LU) fits comfortably inside
    // presetLevel's own room (~6.0 dB), so the fader must NEVER move (see `plan_level_pair`'s
    // TRADE comment on why the ask is `max(d_ben, G)`, not a smaller quantity that would
    // leave the fader to cover the remainder).
    #[test]
    fn a_base_preset_level_alone_can_reach_never_moves_the_fader() {
        let plan = plan_level_pair(&[], -24.02, -23.0, 0.5, 1.0);
        assert!(!plan.boost, "{:?}", plan);
        assert!((plan.dp_db - 1.02).abs() < 1e-6, "{:?}", plan);
        assert_eq!(
            plan.fader_target, None,
            "presetLevel alone reaches target -- the fader must not move"
        );
        assert!((plan.preset_level - 0.5623).abs() < 1e-3, "{:?}", plan);
    }

    // U4. Both controls already at their ceilings (`presetLevel == 1.0`, base fader == 1.0)
    // and the base is still short: neither control has room left in EITHER direction, so the
    // window is empty (`dp_lo > dp_hi`) -- no move, honest clamp upstream.
    #[test]
    fn a_base_short_with_both_controls_at_their_ceilings_is_infeasible() {
        let plan = plan_level_pair(&[], -30.0, -20.0, 1.0, 1.0);
        assert!(!plan.boost, "{:?}", plan);
        assert_eq!(plan.dp_db, 0.0);
        assert_eq!(plan.fader_target, None);
        assert_eq!(
            plan.preset_level, 1.0,
            "an infeasible plan never touches presetLevel"
        );
        assert_eq!(plan.capped, None);
    }

    // U5. Whatever the plan, a SEEDED fader value (`Some`) must land inside
    // `[BASE_FADER_FLOOR, PRESET_LEVEL_MAX]` -- it is, after all, a value the closed-loop
    // solve is about to WRITE to the device. Swept over a broad p0 x f0 x G grid.
    #[test]
    fn the_pair_plan_never_seeds_a_fader_below_the_base_fader_floor_or_above_level_max() {
        let p0_values = [0.05_f32, 0.27, 0.5, 0.9, 1.0];
        let f0_values = [0.02_f32, 0.28, 0.5, 0.9, 1.0];
        let g_values = [-30.0_f64, -10.0, -1.0, 0.0, 1.0, 5.0, 16.4, 30.0, 60.0];

        for &p0 in &p0_values {
            for &f0 in &f0_values {
                for &g in &g_values {
                    let plan = plan_level_pair(&[], 0.0, g, p0, f0);
                    if let Some(ft) = plan.fader_target {
                        assert!(
                            (BASE_FADER_FLOOR..=PRESET_LEVEL_MAX).contains(&ft),
                            "fader_target {ft} outside [{BASE_FADER_FLOOR}, \
                             {PRESET_LEVEL_MAX}] at p0={p0} f0={f0} g={g}: {:?}",
                            plan
                        );
                    }
                }
            }
        }
    }

    // U6. A base deficit inside the acceptance band, with NO benefiting sound clamped either,
    // plans no move at all -- the planner and the runner must agree about "done" (module
    // header on `TRADE_CLAMP_EPS_LU`). NB a benefiting clamp at the same |G| still trades
    // (that is legacy behaviour, proven by U1) -- this gate deliberately uses a non-benefiting
    // sound so both halves of the no-move gate are exercised.
    #[test]
    fn a_deficit_inside_the_acceptance_band_plans_no_move() {
        let inside = TRADE_CLAMP_EPS_LU - 0.01;
        let sounds = [scene(0, -30.0, -15.0, false)]; // clamped, but does not benefit
        let plan = plan_level_pair(&sounds, -23.0, -23.0 + inside, 0.5, 0.8);
        assert!(!plan.boost, "{:?}", plan);
        assert_eq!(plan.dp_db, 0.0);
        assert_eq!(plan.fader_target, None);
        assert_eq!(plan.preset_level, 0.5);
        assert_eq!(plan.capped, None);
    }

    // U7. THE ARITHMETIC CONTRACT the write phase depends on: a Full-overlay scene rides
    // `Δp` alone (module header, `benefits_from_base_raise`), while the base sound itself
    // rides `Δp + Δf` and that total must land it EXACTLY on its own target (the physics this
    // whole planner exists to satisfy) -- so a scene's shift can never exceed the base's own
    // total move.
    #[test]
    fn benefiting_ceilings_shift_by_delta_p_and_fader_riding_ceilings_by_the_total() {
        let asis = -39.37;
        let target = -23.0;
        let plan = plan_level_pair(&[], asis, target, 0.27, 0.28);

        let full_overlay_scene_shift = plan.dp_db;
        let base_and_fs_shift = plan.dp_db + plan.df_db;

        assert!(
            (base_and_fs_shift - (target - asis)).abs() < 1e-9,
            "the base's own move must land it exactly on target: {:?}",
            plan
        );
        assert!(
            full_overlay_scene_shift <= base_and_fs_shift + 1e-9,
            "a Full-overlay scene rides dp alone, which can never exceed the base's total \
             move: {:?}",
            plan
        );
    }

    // U8 -- SKIPPED BY DESIGN. The >=2-amp refusal (danger.md's OPEN distrust of parallel-amp
    // scene-0 leveling) is enforced at the Phase-2 call site that decides WHICH amp
    // candidates ever reach this planner -- `plan_level_pair` is pure arithmetic over whatever
    // `sounds`/levels it is handed and has no way to see "how many amps" produced them. See
    // leveller.rs's Base-arm derivation for that guard.

    // U9. `LevelPairPlan::preset_level` must be EXACTLY what `raised_preset_level` computes
    // from the SAME `dp_db` -- two call sites (the plan and the eventual write) must never be
    // able to disagree about what "the new presetLevel" is.
    #[test]
    fn raised_preset_level_and_the_plan_agree_on_the_exact_level() {
        let plan = plan_level_pair(&[], -39.37, -23.0, 0.27, 0.28);
        assert_eq!(plan.preset_level, raised_preset_level(0.27, plan.dp_db));
    }
}
