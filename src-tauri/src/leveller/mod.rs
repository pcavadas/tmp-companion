//! Preset leveling — derived from the live de-risk against real hardware.
//!
//! Findings that shape this (all confirmed on-device):
//!   1. The re-amp USB-Out tap reflects `presetLevel`, but ONLY the value that
//!      was set BEFORE re-amp engaged — re-amp latches preset state at engage.
//!   2. Re-amp engages reliably only ONCE per connection (fresh connect →
//!      single toggle). Repeated toggling within a session is unreliable.
//!   3. `set_preset_level` is IGNORED when it immediately follows `load_preset`
//!      in the SAME connection — the load's own level-apply overrides our set.
//!      A no-load `set_preset_level` (on the already-current preset) sticks.
//!      → load the preset in its own connection, then measure/set on FRESH
//!      connections. The device keeps the loaded preset "current" across USB
//!      reconnects, so the no-load set targets the right preset.
//!   4. `presetLevel` is a LINEAR amplitude control:
//!      `captured_LUFS = 20·log10(presetLevel) + C`,
//!      where `C` folds the preset's inherent processed loudness + stimulus
//!      level + the fixed re-amp tap gain. Verified to ~0.2 LU across 0.1–0.9.
//!
//! So leveling is one-shot/open-loop: measure once at a reference level, solve
//! for `C`, compute the exact `presetLevel` that hits the target, set it, save.
//!
//! Layout: one file per lane, behind this module's flat API (every lane's items are
//! re-exported here, so callers keep addressing them as `leveller::…`):
//!   - `load` — the latch-rule isolated load and the stored-preset restores.
//!   - `measure` — the floor guard, `C` measurement and the shared capture seams.
//!   - `doctor_capture` — the Doctor's stimulus window, onset split and captures.
//!   - `reamp_off` — the guaranteed re-amp OFF and the run-end guard.
//!   - `secant` — the one secant step and its per-unit flatness rules.
//!   - `knob` — the scene-aware level-control writes.
//!   - `base` — the `presetLevel` lane and the setlist runner.
//!   - `footswitch` — the footswitch (engaged-state) lane.
//!   - `scene` — scene jobs, the one-shot joint-k and handle solves, the batch runner.
//!   - `trade` — the headroom trade and the clamped-headroom redistribution.
//!   - `persist` — the post-save persist verify.
//!   - `rebalance` — the parallel-amp rebalance flow.
//!   - `block` — the closed-loop block-control seam.
//!
//! This file holds what every lane shares: the settle and capture constants, the
//! tolerance bands, [`Verification`], [`LevelResult`], [`LevelOptions`] and [`LevelKnob`].
//! The retired live-batched scene harness lives with its only caller,
//! `probe_api::scene_bench`.

use crate::slot::ListIndex;
use std::time::Duration;

use serde::Serialize;

use crate::audio;
use crate::error::{AppError, LevelError, Refusal, SkipKind};
use crate::lufs;
use crate::scene_jobs::{overlay_param, SceneParamRead};
use crate::session::Session;
use crate::{
    device_gate::settle_abortable, device_gate::settle_or_cancel, scene_jobs::read_saved_preset,
    scene_jobs::read_saved_preset_complete, scene_jobs::scene_write_verdict_for_param,
    scene_jobs::SceneWriteVerdict,
};

mod base;
mod block;
mod doctor_capture;
mod footswitch;
mod knob;
mod load;
mod measure;
mod persist;
mod reamp_off;
mod rebalance;
#[cfg(test)]
mod reordered_run_tests;
mod scene;
mod secant;
#[cfg(test)]
mod tests;
mod trade;

pub use base::*;
pub use block::*;
pub use doctor_capture::*;
pub use footswitch::*;
pub(crate) use knob::*;
pub(crate) use load::*;
pub use measure::*;
pub(crate) use persist::*;
pub(crate) use reamp_off::*;
pub use rebalance::*;
pub use scene::*;
pub(crate) use secant::*;
pub use trade::*;

// Post-load DSP settle before a capture. Was a conservative 1200; HW-bisected to 400
// on fw 1.8.45 (dry slot 11 + wet delay slot 5): measured C, presetLevel, and verify
// error are byte-identical to 1200, and the verify captures confirm writes also land
// at 400 (`notes/perf.md`). TMP_SETTLE_AFTER_LOAD_MS is the diagnostic env override
// for future bisects.
const SETTLE_AFTER_LOAD_MS: u64 = 400;
pub(crate) fn settle_after_load_ms() -> u64 {
    std::env::var("TMP_SETTLE_AFTER_LOAD_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(SETTLE_AFTER_LOAD_MS)
}
// Settle after a reload when the next step is a PURE WRITE (no verify capture).
// DELIBERATELY kept above SETTLE_AFTER_LOAD_MS: on this branch a settle-caused
// dropped write is maximally silent (no verify capture follows; the save persists
// the old value while the result reports success), so the 200 ms it could save
// isn't worth the risk class — see the scene-write-cliff history.
const SETTLE_BEFORE_WRITE_MS: u64 = 600;
pub(crate) const SETTLE_AFTER_SET_MS: u64 = 300;
pub(crate) const SETTLE_AFTER_REAMP_MS: u64 = 500;
const CAPTURE_TAIL_MS: u64 = 800;

// The device silently DROPS a scene-context write when the session sat IDLE too long
// immediately before it. On fw 1.8.45 this read as a PER-COMMAND idle-gap cliff at
// ~400–450 ms rather than a "time since loadScene" window. The HW run (Hiwatt slot 31,
// fw 1.8.45): the enable-dropped branch's back-to-back 150+300 ms sleeps formed ONE
// 450 ms idle gap and 6/6 bare scene writes vanished (no presetError, nothing in the
// working copy, nothing persisted) even though only ~450 ms had passed since
// `loadScene` — while the bisect's load_scene→300→edit→400→write (max single gap
// 400 ms) lands and …→450→write drops. Those are fw 1.8.45 numbers. On fw 1.8.58 the
// client lapses 750 ms after the last OUT report, read windows included (740 ms kept,
// 760 ms dropped, device-confirmed). The companion measures gaps from the command and
// excludes its read window of up to 300 ms, so 300 + 450 = 750 ms (dropped) and
// 300 + 400 = 700 ms (lands): the total-window reading is the consistent one (INFERRED).
// So: keep EVERY pre-write idle gap ≤~300 ms, and never sleep a settle for a
// command that wasn't sent (the conditional in `set_knobs`).
const SETTLE_AFTER_SCENE_EDIT_MS: u64 = 300;
// Gap between the `loadScene` recall and the next command (`SetNodeSceneEdit`, or the
// value write itself when the enable is dropped). 150 (not the general 300
// `SETTLE_AFTER_SET_MS`) keeps the gap far under the ~400–450 ms idle cliff above.
// HW-bisected lower edge (`probe --bisect-scene`, fw 1.8.45): scene_settle 150, 100,
// and even 50 all land ON the scene overlay (never leak to base) and persist.
pub(crate) const SETTLE_AFTER_SCENE_RECALL_MS: u64 = 150;
pub(crate) const RATE: u32 = 48_000;
const LEVEL_MIN: f32 = 0.0;
/// THE amplitude ceiling every `presetLevel` / amp-`outputLevel` lane clamps to.
pub(crate) const LEVEL_MAX: f32 = 1.0;

/// How a written level's reported loudness is KNOWN — the one vocabulary the base and
/// scene lanes report in, so a prediction can never pass for a measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    /// No post-write capture was planned (verify off, an idempotency skip, a clamp that
    /// wrote nothing): the reported loudness is the model's prediction.
    #[default]
    Predicted,
    /// A capture of the written state backs the reported loudness.
    Verified,
    /// A post-write capture was planned and failed on every try (the retry included): the
    /// reported loudness is only a prediction, and the row must not read as on target.
    VerifyFailed,
}

impl Verification {
    /// The verdict for a lane that asked for a verify (`planned`) and got `read` back from
    /// `apply_levels` — whose `None` under a planned verify now means "the capture failed"
    /// (a Stop propagates as an error instead).
    pub(crate) fn of(planned: bool, read: Option<f64>) -> Self {
        match (planned, read) {
            (_, Some(_)) => Verification::Verified,
            (true, None) => Verification::VerifyFailed,
            (false, None) => Verification::Predicted,
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LevelResult {
    pub slot: ListIndex,
    /// WHICH sound this row describes, when the row is a SCENE row: the 0-based
    /// `scenes[]` wire index. `None` on every base / block / footswitch row.
    ///
    /// Load-bearing for identity, not decoration. `level_scenes_apply_batched` FILTERS
    /// failed scenes out of the vec it returns (`commands/level_scenes.rs`), so the
    /// result vec can be SHORTER than the request's `jobs` array — and a consumer that
    /// re-derives "which scene is row i?" by position mislabels every row after the
    /// first failure. The batched runner's outcome carries the slot
    /// ([`BatchedSceneOutcome::scene_slot`], whose own doc says the same thing about
    /// positional zips); this forwards it onto the wire so no consumer has to guess.
    /// Mirrored in `src/lib/types.ts`.
    pub scene_slot: Option<u32>,
    pub ref_level: f32,
    /// Captured integrated LUFS measured at `ref_level`.
    pub measured_lufs: f64,
    /// Solved constant `C` in `LUFS = 20·log10(level) + C` (= max reachable LUFS).
    pub constant_c: f64,
    /// presetLevel computed to hit the target (clamped 0..1).
    pub final_level: f32,
    pub target_lufs: f64,
    /// Predicted captured LUFS at `final_level` (== target unless clamped).
    pub predicted_lufs: f64,
    /// True if the target needed level outside [0,1] (unreachable — clamped).
    pub clamped: bool,
    /// Whether `final_level` was persisted to the preset (SaveCurrentPreset).
    pub saved: bool,
    /// Independent re-measure at `final_level` on a fresh capture (None if skipped).
    pub verify_lufs: Option<f64>,
    /// Whether the reported loudness was measured after the write, predicted, or a
    /// planned verify FAILED (see [`Verification`]). Mirrored in `src/lib/types.ts`.
    pub verification: Verification,
    /// Number of capture iterations the solve used (1 = one-shot presetLevel
    /// path; 2..=N for the closed-loop block-knob path).
    pub iterations: u32,
    /// Short-term-max − integrated of the measure capture (LU), gain-invariant.
    /// Large (≳6 LU) = a dynamic preset whose gated-integrated reading understates
    /// its peaks vs a compressed one — the UI flags it "verify by ear". `None`
    /// when the measuring path has no full-capture meter (live windows).
    pub dynamic_spread_lu: Option<f64>,
    /// When clamped for a SPECIFIC reason (currently "no authority" — the amp's
    /// `outputLevel` doesn't reach the USB 1/2 capture), this is set. Not surfaced in
    /// the UI (design 1a shows one generic clamp message regardless of cause); its
    /// presence is used only to classify the row as `offbranch`. `None` for the
    /// preset-level path / plain clamp.
    pub clamp_reason: Option<String>,
    /// The clamp's CAUSE from the shared taxonomy ([`crate::headroom_trade::ClampKind`]) —
    /// `None` when the row is not clamped. ADDITIVE alongside `clamp_reason`, whose contract
    /// ("the leveled signal isn't reaching USB 1/2", `.claude/rules/leveling-dsp.md`) is
    /// unchanged: this is the machine-readable cause, that one stays the sentinel the UI
    /// maps to `offbranch`. Mirrored in `src/lib/types.ts`.
    pub clamp_kind: Option<crate::headroom_trade::ClampKind>,
    /// Best-effort rebalance "verify by ear" flag (lane-mute bleed may have skewed the
    /// equal-solo balance). Distinct from `dynamic_spread_lu`; the UI ORs both.
    pub verify_by_ear: bool,
    /// The preset's saved `presetLevel` BEFORE this run wrote it — enables the
    /// re-run idempotency skip (see `level_unchanged`): a re-run that solves the
    /// SAME level as last time reloads the stored preset and returns without
    /// writing. Stamped by the `level_preset` command (from its base-isolation
    /// preset read); `None` when the read failed or the path doesn't write
    /// `presetLevel` (block-knob / scene paths). Not surfaced in the UI.
    pub previous_level: Option<f32>,
    /// PREDICTED true peak (dBTP) at `final_level`, extrapolated from the reference
    /// capture's measured true peak (see `predicted_true_peak_dbtp`) — an ESTIMATE,
    /// never a re-measurement. Only the one-shot `presetLevel` path (`level_preset`)
    /// sets this; `None` for scene/block/footswitch paths this cycle.
    pub true_peak_dbtp: Option<f64>,
    /// Post-save param-level verify, forwarded from the batched scene runner's
    /// [`BatchedSceneOutcome::persist_mismatch`]: `Some(true)` = the saved preset does
    /// NOT hold the value this result reports (do not trust the number); `Some(false)`
    /// = re-read and confirmed; `None` = not checked (no save, nothing written, the
    /// re-read failed, or a path without the verify).
    pub persist_mismatch: Option<bool>,
    /// THE HEADROOM TRADE this run made (or, on a preview, WOULD make) — see
    /// [`crate::headroom_trade::TradeSummary`] (disclosure rationale: its own doc). Stamped on
    /// EVERY row of a batch that traded, because the trade moved the whole preset's gain
    /// structure, not one row's. `None` on every untraded run and on every lane that has no
    /// trade (base, block, footswitch). Mirrored in `src/lib/types.ts`.
    pub trade: Option<crate::headroom_trade::TradeSummary>,
    /// The base-pair BOOST this run made, or — when its shape couldn't apply one — what it
    /// WOULD make. See [`crate::headroom_trade::BaseBoostSummary`]. `None` whenever the
    /// planner chose no boost, and on every lane that isn't the base row (block, footswitch,
    /// scene). Mirrored in `src/lib/types.ts`.
    pub base_boost: Option<crate::headroom_trade::BaseBoostSummary>,
    /// [`REAMP_STILL_ON_WARNING`] when the run-end [`reamp_off_guaranteed`] could not read
    /// re-amp back OFF; stamped by the command on every row it returns, because it is a fact
    /// about the RUN, not the row. Omitted from the wire when `None`. Mirrored in
    /// `src/lib/types.ts`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reamp_warning: Option<String>,
    /// Re-amp may still be on: the run-end OFF never read back OFF — the TYPED twin of
    /// `reamp_warning`, which the frontend reads instead of the text (the unit's guitar
    /// input stays muted until it is restarted). Stamped with it, on every row.
    pub reamp_still_on: bool,
}

impl LevelResult {
    /// Record the run-end re-amp verdict on this row: the typed flag and its text.
    pub(crate) fn stamp_reamp(&mut self, out: ReampOffOutcome) {
        self.reamp_still_on = out.still_on();
        self.reamp_warning = out.warning();
    }
}

#[derive(Clone, Copy)]
pub struct LevelOptions {
    /// Persist `final_level` to the preset after computing it.
    pub save: bool,
    /// Re-measure at `final_level` on a fresh capture to confirm the result.
    pub verify: bool,
    /// Reference level to measure at (the model is solved from this point).
    pub ref_level: f32,
    /// Leave the written values UNSAVED in the device working copy: no save AND no
    /// restore-reload. The scene runners' accumulate-then-single-save mode — unsaved
    /// scene-edit writes survive scene recalls and reconnects, and ONE final
    /// `saveCurrentPreset` persists every accumulated overlay (HW,
    /// `probe --defer-scenes`). Meaningless with `save: true`.
    pub defer: bool,
    /// Scene to recall right before a `save: true` save, so the save re-stamps the
    /// preset's ORIGINAL `lastLoadedScene` (0-based scene index, or
    /// `session::BASE_SCENE_SLOT`). The base-context measurement recalls base (issue:
    /// base must measure base, not the saved scene), which leaves base active at save
    /// time — without this the save silently rewrites `lastLoadedScene` to 8 (HW,
    /// Hiwatt slot 31: pre 3 → post 8), changing which scene the preset loads into on
    /// the pedalboard. `None` = save whatever context is active (old behavior).
    pub restore_scene: Option<u32>,
    /// The run's OWN `presetLevel` — its solved value, or the UNSAVED raise a headroom
    /// trade is holding — re-asserted after `set_knobs` and before the verify engage.
    ///
    /// `set_knobs` recalls the knob's scene, and a recall runs the device's own
    /// level-apply (`recall_reassert_save`'s doc carries the HW evidence), so without
    /// this the VERIFY capture renders at the level the device HAS SAVED even when the
    /// as-is capture was correctly asserted. That asymmetry is silent and it INVERTS the
    /// correction: the verify reads quiet, so the bounded secant walks the fader the wrong
    /// way (offline gate `a_batched_scene_run_persists_both_halves_of_a_landed_headroom_trade`:
    /// the compensating fader rose 0.69 → 0.726 instead of falling).
    ///
    /// Ignored when a `PresetLevel` target is in the same batch — there `set_knobs` is
    /// already writing the level under measurement and must win. `None` = assert nothing,
    /// which is every caller that has no run-owned value.
    pub intended_preset_level: Option<f32>,
}

impl Default for LevelOptions {
    fn default() -> Self {
        LevelOptions {
            save: false,
            verify: false,
            ref_level: 0.5,
            defer: false,
            restore_scene: None,
            intended_preset_level: None,
        }
    }
}

// ─── Closed-loop block-control leveling ──────────────────────────────────────

/// Which control to drive when leveling. `PresetLevel` is the validated one-shot
/// master path; `Block` drives a chosen block parameter via `ChangeParameter`
/// (e.g. an amp's `outputLevel`) and is solved with a closed loop because an
/// arbitrary level knob's response isn't guaranteed linear-in-dB.
#[derive(Debug, Clone)]
pub enum LevelKnob {
    PresetLevel,
    Block {
        group_id: String,
        node_id: String,
        parameter_id: String,
        /// When `Some(scene_slot)` (0-based `scenes[]` wire index), each connection
        /// loads that scene and enables per-block Scene Edit before driving the knob,
        /// so the write lands on the SCENE overlay (per-scene leveling). `None` =
        /// level the base value — `set_knob`/`set_knobs` still recall base
        /// explicitly (via `BASE_SCENE_SLOT`) before writing: a preset load
        /// activates its SAVED `lastLoadedScene`, not necessarily base (HW), so
        /// omitting the recall would silently write whatever scene the connection
        /// defaulted to.
        scene_slot: Option<u32>,
    },
}

impl LevelKnob {
    pub fn label(&self) -> String {
        match self {
            LevelKnob::PresetLevel => "presetLevel".to_string(),
            LevelKnob::Block {
                group_id,
                node_id,
                parameter_id,
                scene_slot,
            } => match scene_slot {
                Some(s) => format!("{group_id}/{node_id}/{parameter_id}@scene{s}"),
                None => format!("{group_id}/{node_id}/{parameter_id}"),
            },
        }
    }
}

/// Closed-loop convergence tolerance and iteration cap. Each iteration is one
/// fresh connection (re-amp engages once per connection), so the cap bounds the
/// device round-trips. NOT an acceptance band (that is [`ACCEPT_TOL_LU`]): what sits on this
/// constant is the noise-floor/flatness/no-authority class ("did this move change anything?"),
/// [`level_unchanged`]'s "is this the same level" range test, and the block-knob lane.
pub(crate) const KNOB_TOL_LU: f64 = 0.3;
/// THE acceptance band — "on target" means the same thing in every lane: the base lane's
/// idempotency skip ([`already_on_target`]) and verify correction, every scene at-target
/// check, and every footswitch at-target check. About 2σ of the ~0.12 LU run-to-run capture
/// noise (`notes/gotchas.md`): a perfectly converged point reads inside it ~96% of the time,
/// where the old footswitch band (0.1 LU, under σ) read it inside only ~60% and the loop kept
/// buying captures to chase noise.
pub(crate) const ACCEPT_TOL_LU: f64 = 0.25;
const KNOB_MAX_ITERS: u32 = 6;

/// Max secant CORRECTIONS after the first apply, shared by every re-amp-measured solve
/// (scene `outputLevel` AND footswitch `param` valueA). Each correction is a fresh-connect
/// re-amp capture (~10 s), so this is kept small — 2–3 slope-corrected steps converge any
/// chain with slope ≥ ~0.15 (below that it's the no-authority case), and a large cap would
/// re-inflate per-scene cost toward the legacy 80–93 s regime. (NOT `KNOB_MAX_ITERS`, which
/// counts 2 seed measurements in its budget.)
const MEASURE_CORRECT_MAX: u32 = 3;

/// Capture budget for the footswitch solve's bracket-aware secant (see
/// `solve_footswitch`). Larger than the scene path's `MEASURE_CORRECT_MAX`
/// because arbitrary block params (a `volume` audio taper, a `drive` knob) can
/// put the target on a steep cliff that needs several Illinois-damped bracket
/// iterates (HW, Hiwatt fs12: ~4 from a 21-LU-wide bracket) — converged solves
/// still exit on the first in-tolerance iterate, so the headroom costs well-behaved knobs
/// nothing. Kept at 16 when the band widened to [`ACCEPT_TOL_LU`] (0.1 → 0.25 LU): the
/// noise-chasing the old band caused is gone because a converged point now reads inside
/// the band (`fs_solve_noise_tests`), and the budget only runs out on genuinely slow
/// curves (the quiet-chain silence-sentinel fixture needs ~12 iterates).
const FS_CORRECT_MAX: u32 = 16;
/// Minimum acceptable `|target_lufs − seed1's measured LUFS|` the law-predicted second
/// seed's IMPLIED separation must clear before it's trusted over the fixed 0.75/0.25-
/// fraction fallback (see `FsParamTarget::seed2_plausible`). Gates in the SAME space the
/// downstream flatness proof (`KNOB_TOL_LU`) reads, not raw knob-value span: the
/// predecessor of this gate, `FS_MIN_SEED_SPAN_FRAC` (0.12 of the param's own v-space
/// range), wrongly REJECTED correct predictions at low knob values — at `v_a = 0.05` a
/// genuine 6 LU correction moves `v` by only `0.0498`, under a 12%-of-`[0,1]` span, so the
/// old gate silently fell back to the fixed seed exactly where the log-knob law matters
/// most (the quiet end of the range, where `v` compresses hardest per dB).
///
/// Why gating on the EXPECTED LU gap is still safe against a false no-authority verdict:
/// for an ACCEPTED prediction the coord-space law makes the pair's expected separation
/// exactly `target_lufs − l_a` (1:1 for `LevelDb`, exact for `LevelLinear`'s log-knob
/// coord), so an accepted pair that nonetheless MEASURES flat (`|l_b − l_a| < KNOB_TOL_LU`
/// despite ≥ 1 LU expected) is precisely `fs_bracket_expansion`'s entry condition
/// (`|l_b − l_a| < KNOB_TOL_LU`) — it fires and probes a bound extreme, so the pair either
/// widens with real slope or the knob is confirmed to have no authority even at its
/// extremes (the correct verdict either way). A FALSE no-authority read would require the
/// law to be off by `≥ 1.0 − KNOB_TOL_LU ≈ 0.7 LU` while the prediction still lands inside
/// the central 5–95% of the range — well outside the law's HW-validated accuracy band.
const FS_MIN_SEED_GAP_LU: f64 = 1.0;
/// An `outputLevel` change of at least this many dB that moves the captured loudness by
/// less than `KNOB_TOL_LU` means the amp has no authority over the USB 1/2 capture
/// (off-branch / off-USB output, or hard-limited downstream).
const NO_AUTHORITY_MIN_DB: f64 = 6.0;
/// Rebalance: if a solo lane is within this many dB of the both-muted floor, the muted
/// lane's bleed corrupts the equal-solo balance → flag the scene "verify by ear".
const REBALANCE_BLEED_MARGIN_DB: f64 = 28.0;
/// Sentinel loudness for a both-lanes-muted capture that reads as digital silence — the
/// IDEAL mute (no bleed). `processed_loudness` errors on silence; this stands in so the
/// solo-above-floor margin is huge (→ no verify-by-ear flag) instead of failing the scene.
const MUTE_FLOOR_SILENT_LUFS: f64 = -120.0;

/// THE routing clamp's `clamp_reason`: the sound's as-is (first) capture was SILENT, so its
/// output is not reaching USB 1/2. Every lane that converts a silent first capture into the
/// [`crate::headroom_trade::ClampKind::NoAuthority`] clamp — the base lane
/// (`level_preset_impl`), the footswitch lane's first seed (`solve_param_secant`) and the
/// scene lanes' as-is prologue ([`scene::no_signal_scene_solve`]) — reports exactly this
/// string, which `.claude/rules/leveling-dsp.md` pins as `clamp_reason`'s documented value.
const NO_SIGNAL_REASON: &str = "no signal on USB 1/2";

/// A silent capture PAST THE FIRST SEED is DATA (the knob's quiet extreme), not a routing
/// failure — `solve_footswitch` converts it into a pseudo point at this LUFS for the
/// second-seed / bracket-expansion-probe / correction-loop's internal GEOMETRY only (never
/// handed to `improve_best`, and the correction loop's at-target break is additionally
/// gated on the point being REAL — see both call sites — so this sentinel can never become
/// the reported "achieved" loudness). Deliberately NOT the true digital-silence floor
/// (`MUTE_FLOOR_SILENT_LUFS`, −120 — still used for the FIRST seed's routing-clamp report):
/// pinning the geometry to the literal floor forced the bounded Illinois-damped correction
/// loop (`FS_CORRECT_MAX` = 8 iterates) to spend its ENTIRE budget walking the bracket back
/// down from a 90+ LU gap, landing ONE iterate short of converging on the reported repro
/// (HW, fw 1.8.45, preset "TR+BD2+BMP": Plumes `level` knob, target −26 LUFS, mono-era
/// convention — see the PR2 metering re-baseline in `notes/leveling.md`). −50 sits 21 LU
/// below the quietest user-settable target (`TargetRow` TMIN = −29) — well clear of any real
/// target, yet close enough that the damping converges with iterates to spare (a sentinel
/// sensitivity sweep found −62..−70 and −98..−100 to be non-converging "dead zones" for this
/// fixture's geometry — avoid those bands if this value ever needs to move).
const FS_SILENT_GEOMETRY_LUFS: f64 = -50.0;

/// Fixed margin `fs_silent_geometry` keeps the silent-capture pseudo point BELOW the
/// quietest real capture of the solve.
const FS_SILENT_MARGIN_LU: f64 = 10.0;
