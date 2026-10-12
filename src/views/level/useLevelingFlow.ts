// src/views/level/useLevelingFlow.ts — the unified leveling WIZARD orchestrator.
//
// Owns the wizard's stage machine (setup → run → summary, all in one persistent
// frame) and DRIVES the run by composing shipped commands per chosen scene:
//   • BASE scene (or a scene-less "Whole preset") → level_preset (preset `presetLevel`,
//     preset-to-preset loudness).
//   • FS scenes → list_level_blocks (amp candidates, once per preset, cached) then
//     level_scenes_apply_batched with ADJACENT same-preset scenes sharing an instrument
//     batched into ONE call regardless of per-scene target (each job carries its own
//     targetLufs, like the footswitch lane) (amp `outputLevel`, scene-to-scene; per-scene
//     progress rides the channel) — per-scene calls re-loaded the preset each time,
//     flashing the unit back to base twice per scene.
// The SCENES are chosen in the list (the scene tree); setup only configures them. The
// backup acknowledgment is an inline checkbox in the Set-up footer (no separate step)
// that gates the commit. Leveling always WRITES (save:true). Each step is isolated: a
// per-item failure becomes "skipped", never aborting the run.
//
// No revert of any kind lives in this flow (design 1a, user directive): the backup
// acknowledgment is the one revert path (restore from a Pro Control backup), so this
// hook carries no gain-budget redistribution, reachable-common-target, or per-row
// "Restore original" state — a clamped row is reported and left for the user to
// re-level at a lower target, nothing more.
//
// The wizard state AND the run engine live in a MODULE-SCOPED store (the `libraryScan` /
// `useLiveDevice` pattern), not in component state: a tab switch unmounts LevelView, and a
// run held in per-mount state kept writing to the device while a remounted tab showed a
// closed wizard with no Stop (#283). The run is app-global — at most ONE is ever in flight —
// and a remounted Level tab binds back to it through `useSyncExternalStore`, Stop included.
// The run's re-amp OFF is the backend's own per-command `reamp_off_guaranteed`, so it runs
// whatever the user does with tabs.

import { useCallback, useEffect, useSyncExternalStore } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";

import {
  levelPreset,
  cancelPresetLeveling,
  listLevelBlocks,
  levelScenesApplyBatched,
  cancelSceneLeveling,
  levelFootswitchesApply,
  cancelFootswitchLeveling,
  toFootswitchJobWire,
} from "../../lib/invoke";
import { onLevelingLufs } from "../../lib/liveEvents";
import { errMsg } from "../../lib/format";
import { MODELS } from "../../models/catalog";
import { resolveDeviceId } from "../../models/blockArt";
import {
  buildLevelJob,
  chosenFrom,
  DYNAMIC_SPREAD_LU,
  optionToRunItem,
  resolvedTargetLufs,
  runItemToOption,
  runRank,
  type RunItem,
  type SetupOption,
  type SetupChoice,
} from "./leveling";
import type { Stage } from "../overlays";
import type { PresetRow } from "../PresetList";
import type {
  SceneInfo,
  FootswitchInfo,
  AmpCandidate,
  Store,
  Profile,
  LevelBlock,
  SilenceHint,
} from "../../lib/types";

// AMP model ids (the catalog's amp categories) — the amp's outputLevel knob is the
// one with real loudness authority. A non-amp level control saturates to ~1.7 LU;
// preamp/master/volume params alter the preset's sound, so they're excluded.
const AMP_CATS = new Set([
  "Combo Amps",
  "Amp Heads",
  "Half Stacks",
  "Bass Amps",
]);
const AMP_BIDS = new Set(
  MODELS.filter((m) => AMP_CATS.has(m.cat))
    .map((m) => m.bid)
    .filter((bid): bid is string => bid != null),
);

// Envelope-follower effects (`effect_type: "Envelope Filter"` — the guide's own
// discriminator, so manual CryBaby wahs, which are expression-driven, stay out).
// Their response tracks the stimulus's attack/decay envelope, so the synthetic clip
// can park them in a different regime than real playing — any preset carrying one
// gets the "envelope" verify-by-ear cause regardless of how clean the numbers look.
// Pedal fender_ids on the wire are exact (no CabIR-class suffixes) → plain Set.has.
const ENVELOPE_BIDS = new Set(
  MODELS.filter((m) => m.et === "Envelope Filter")
    .map((m) => m.bid)
    .filter((bid): bid is string => bid != null),
);

interface Candidate {
  groupId: string;
  nodeId: string;
  parameterId: string;
  value: number;
}

// Live fallback only: filter a live block read down to amp `outputLevel` candidates
// (the common path uses the startup-backup candidates instead). A discovered amp
// block may carry merged cab/IR/convolution suffixes the catalog's bare amp bids
// lack (e.g. "ACD_HiwattDR103CanModCabIR") — `baseDeviceId` strips them via the
// canonical suffix set so amp+cab(+reverb) combo blocks still match their amp bid.
function ampBlocks(blocks: LevelBlock[]): Candidate[] {
  return blocks
    .filter(
      (b) =>
        AMP_BIDS.has(resolveDeviceId(b.model_id, (id) => AMP_BIDS.has(id))) &&
        b.parameter_id === "outputLevel",
    )
    .map((b) => ({
      groupId: b.group_id,
      nodeId: b.node_id,
      parameterId: b.parameter_id,
      value: b.value,
    }));
}

// The result fields the per-item mappers read — the STRUCTURAL subset shared by
// `LevelResult` (preset/scene) and `FootswitchLevelResult` (footswitch). The latter
// has no `verify_by_ear`, so its by-ear cause is dynamic-only.
interface LevelOutcomeFields {
  clamped: boolean;
  clamp_reason: string | null;
  verify_lufs: number | null;
  predicted_lufs: number;
  dynamic_spread_lu: number | null;
  verify_by_ear?: boolean;
  /** Footswitch rows only — `LevelResult` (preset/scene) has no such field, so it stays
   *  optional and those lanes simply never report an unconverged row. */
  unconverged?: boolean;
  /** Footswitch rows only: the clamp's pinned bound is the wet/mix floor, not headroom —
   *  see `FootswitchLevelResult.wet_floor`. */
  wet_floor?: boolean;
}

// A `clamp_reason` is set ONLY when the leveled signal isn't effectively reaching the USB 1/2
// capture — a silent capture for preset/footswitch jobs (output not routed to USB), or the
// scene path's no-authority case (a big amp-outputLevel change doesn't move the capture:
// off-branch) → its own `offbranch` outcome ("not on USB 1/2"), which a re-level can't fix.
// A plain headroom/authority clamp (the knob has real effect but can't reach target) has
// `clamped` set with NO reason → "clamped at X". `unconverged` is last of the miss states
// (the backend's own precedence): it is only ever set when `clamped` is not, and it means
// the knob had room left, so a re-run helps.
const outcomeOf = (r: LevelOutcomeFields): RunItem["outcome"] =>
  r.clamp_reason != null
    ? "offbranch"
    : r.clamped
      ? "clamped"
      : r.unconverged
        ? "unconverged"
        : "done";
const valueOf = (r: LevelOutcomeFields): number =>
  r.verify_lufs ?? r.predicted_lufs;
// Resolve to a SINGLE by-ear cause. If a row is both dynamic AND rebalance-uncertain (rare —
// rebalance is opt-in), "dynamic" wins INTENTIONALLY: it's the primary, more common ear-check
// signal. `wet_floor` ranks between them: it's definitionally a CLAMP cause (why the row
// stopped where it did), not a measurement-quality flag like `dynamic`, but it only ever
// rides on a footswitch row, which never sets `verify_by_ear` (scene-only) — so the two
// can't actually collide in practice. Each row resolves to one cause; the summary footnote
// counts whichever each resolved to.
const byEarCause = (r: LevelOutcomeFields): RunItem["verifyByEar"] =>
  (r.dynamic_spread_lu ?? 0) >= DYNAMIC_SPREAD_LU
    ? "dynamic"
    : r.wet_floor
      ? "wet_floor"
      : r.verify_by_ear
        ? "rebalance"
        : undefined;

/** A run row that levels via amp `outputLevel` in scene mode — not Base (`presetLevel`), not
 *  a block-acting footswitch. The run loop batches these. */
const isSceneItem = (it: RunItem): boolean =>
  !it.isBase && it.footswitch == null && it.sceneSlot != null;

/** The run's live state, published by the run loop and read by RunPage/SummaryPage. */
export interface RunState {
  items: RunItem[];
  currentIndex: number;
  total: number;
  done: boolean;
  stopped: boolean;
  /** A stop was requested and the in-flight item is still winding down. Drives the
   *  "Stopping…" feedback so the user isn't left staring at a "leveling…" spinner. */
  stopping: boolean;
  /** A batch-wide caption ("Saving…" / "Verifying…") from a `SceneLevelProgressItem.tail`
   *  (issue 6b) — the deferred-save/persist-verify phase AFTER every scene in the group
   *  already resolved, so no row's own status can show it. `RunPage`'s header renders
   *  this in preference while set. Cleared on the run's completion and whenever a NEW
   *  row goes active (both already `publish` every time), so it can't outlive the
   *  window it describes. */
  tailMessage: string | null;
}

const EMPTY_RUN: RunState = {
  items: [],
  currentIndex: 0,
  total: 0,
  done: false,
  stopped: false,
  stopping: false,
  tailMessage: null,
};

/** The whole wizard's state — what LevelView renders. Module-scoped (see the file header). */
interface LevelFlowState {
  /** `"closed"` ⇒ the wizard is unmounted. The frame stays mounted across the other
   *  stages; only the body swaps. */
  stage: Stage;
  chosen: SetupOption[];
  isRelevel: boolean;
  run: RunState;
  /** Advisory live measured loudness for the active run row's "measuring…" readout. Held
   *  OUTSIDE `RunState` on purpose: the run loop's `publish()` rebuilds RunState on every
   *  transition and would clobber these async event updates. The backend only emits during
   *  a capture, so this stays null outside a run; reset to null on each item resolve. */
  liveLufs: number | null;
  /** Rolling per-hop momentary levels (dB) for the decorative live VU bars — last 24 hops. */
  liveTrace: number[];
}

const INITIAL_FLOW: LevelFlowState = {
  stage: "closed",
  chosen: [],
  isRelevel: false,
  run: EMPTY_RUN,
  liveLufs: null,
  liveTrace: [],
};

// ── MODULE-SCOPED store ──────────────────────────────────────────────────────
let flowState: LevelFlowState = INITIAL_FLOW;
const flowSubs = new Set<() => void>();

function patchFlow(patch: Partial<LevelFlowState>): void {
  flowState = { ...flowState, ...patch }; // new ref so useSyncExternalStore re-renders
  for (const cb of flowSubs) cb();
}
function subscribeFlow(cb: () => void): () => void {
  flowSubs.add(cb);
  return () => {
    flowSubs.delete(cb);
  };
}
const getFlow = (): LevelFlowState => flowState;

// Run-engine bookkeeping — module-scoped for the same reason: a remounted tab must reach
// the in-flight run's own cancel flag, and a second run must see the first one's `running`.
/** A run is in flight (from Start until its final `done` publish). */
let running = false;
/** Stop was requested for the in-flight run. */
let cancelRequested = false;
/** Bumped by `resetLevelRun` so a run orphaned by a test reset can never publish again. */
let generation = 0;
// BUG-4: once a run has leveled at least one sound, closing the wizard (Done OR Cancel)
// must DESELECT exactly the keys it leveled — and ACCUMULATE them across re-level rounds
// (each re-level run adds its keys here), so a Re-level→Cancel can't leave the whole
// selection ticked and silently re-run everything next time. A Cancel BEFORE any run
// leaves these empty, preserving the selection.
let didRun = false;
let ranKeys = new Set<string>();
// Opt-in: equalize a path-MERGE scene's two parallel-amp lanes before joint-k. A no-op
// on series / single-amp / split-output scenes (the backend only rebalances scenes it
// classifies as mergeable). SetupPage owns the visible pill and pushes its value here via
// `setRebalance` — incl. on mount, so a remount (re-level / Back→Continue / new flow)
// resets it to the freshly-defaulted (off) pill rather than leaking a stale ON. Read at
// run time.
let rebalance = false;
/** The MOUNTED Level view's list refresh, run once a run ends. Null while the tab is
 *  unmounted — its own mount then reloads the list anyway. */
let boundRefresh: (() => Promise<void>) | null = null;
let lufsBridge: Promise<UnlistenFn> | null = null;

// Subscribe to `tmp://leveling-lufs` ONCE for the app's lifetime (inert off-Tauri). Events
// outside a run (another feature's capture) are ignored, so a stale reading can never
// seed the next run's meter.
function startLufsBridgeOnce(): void {
  if (lufsBridge) return;
  lufsBridge = onLevelingLufs((e) => {
    if (!running) return;
    patchFlow({
      liveLufs: e.lufs,
      liveTrace: [...flowState.liveTrace, e.momentary].slice(-24),
    });
  });
}

/** Is a leveling run in flight right now? */
export function isLevelRunInFlight(): boolean {
  return running;
}

/** FULL reset — for test isolation only (call in `beforeEach`, like `resetLibraryScan`).
 *  Orphans any in-flight run: its `generation` no longer matches, so it never publishes
 *  or clears `running` again. */
export function resetLevelRun(): void {
  generation += 1;
  running = false;
  cancelRequested = false;
  didRun = false;
  ranKeys = new Set();
  rebalance = false;
  boundRefresh = null;
  if (lufsBridge) {
    void lufsBridge.then((u) => {
      u();
    });
    lufsBridge = null;
  }
  flowState = INITIAL_FLOW;
  for (const cb of flowSubs) cb();
}

/** What the run engine reads from the Level view's data at Start. */
interface RunContext {
  profileById: (id: string) => Profile | null;
  targetLufsByName: (name: string | null) => number;
  ampCandidates: Map<number, AmpCandidate[]>;
  blocksByIndex: Map<number, string[]>;
}

// Drive the run sequentially: one chosen scene at a time, mark active → result. A no-op
// while another run is in flight. Every exit — a normal finish, a Stop, or an unexpected
// throw (#285) — ends in the terminal `done` publish and releases `running`.
async function runLeveling(items: RunItem[], ctx: RunContext): Promise<void> {
  if (running) return;
  running = true;
  cancelRequested = false;
  didRun = true;
  const gen = generation;
  const live = () => gen === generation;
  const { profileById, targetLufsByName, ampCandidates, blocksByIndex } = ctx;
  // Read the cancel flag through a getter so it isn't narrowed to its
  // last-assigned literal across awaits — the cancel handler flips it meanwhile.
  const isCancelled = () => cancelRequested;
  const candCache = new Map<number, Candidate[]>();
  const work = items.map((it) => ({ ...it }));
  // Dependency order documented on `runRank`. Stable sort (0 for differing slots ⇒
  // input order preserved) guarantees it regardless of how `items` was assembled.
  work.sort((a, b) => (a.slot === b.slot ? runRank(a) - runRank(b) : 0));
  const total = work.length;

  // `work` is mutated in place between publishes; pass a fresh ARRAY each time
  // (new ref so React renders) but skip the per-item spread — the bodies only read
  // items during render, never hold them across renders.
  // `lastPublishedIndex` lets a tail-only progress item (no row of its own — see
  // `batchResolve` below) re-publish the caption without inventing a step index.
  let lastPublishedIndex = 0;
  const publish = (
    currentIndex: number,
    done: boolean,
    stopped: boolean,
    tailMessage: string | null = null,
  ) => {
    if (!live()) return;
    lastPublishedIndex = currentIndex;
    // Once cancel is requested, every publish carries `stopping` until the final
    // done publish clears it (done ⇒ either "complete" or "stopped"). Every call
    // site but the tail-caption one below omits `tailMessage`, so the default
    // `null` clears it on the run's completion AND on every row activation —
    // exactly the two points issue 6b's contract requires.
    patchFlow({
      run: {
        items: [...work],
        currentIndex,
        total,
        done,
        stopped,
        stopping: isCancelled() && !done,
        tailMessage,
      },
    });
  };

  if (live()) patchFlow({ stage: "run" });
  publish(0, false, false);

  // INVARIANT (BUG 2): at most ONE row is ever "active". The reported bug — several
  // rows all rendering "leveling · <the same LUFS>" at once — traced to nothing ever
  // clearing a row OUT of "active" when the channel (or the optimistic
  // `markGroupActive` pre-flip) named a NEW one active; `RunPage` renders the one
  // shared `liveLufs` on every row whose `status === "active"`, so a stale active row
  // kept showing the new row's live number too. Every site that flips a row active
  // must go through this so a future call site can't reintroduce the bug — it demotes
  // any OTHER currently-active row back to "queued" (never touching an already-
  // resolved "result" row) before promoting the target.
  const setSoleActive = (target: RunItem) => {
    for (const w of work) {
      if (w !== target && w.status === "active") w.status = "queued";
    }
    target.status = "active";
  };

  // Envelope-follower presets get the "envelope" cause over any result-derived
  // one: the effect tracks the stimulus envelope, so the measurement itself is
  // suspect no matter how clean the numbers look.
  const causeFor = (slot: number) => {
    const envelope = (blocksByIndex.get(slot) ?? []).some((id) =>
      ENVELOPE_BIDS.has(id),
    );
    return (r: LevelOutcomeFields): RunItem["verifyByEar"] =>
      envelope ? "envelope" : byEarCause(r);
  };
  // Shared per-item resolve tail: mark result, remember the key so closing the
  // wizard deselects it (BUG-4; a stopped run never reaches un-run items, so their
  // keys stay selected), and drop the live readout — the result row is the confirm.
  const finishItem = (item: RunItem, globalIdx: number) => {
    item.status = "result";
    if (!live()) return;
    ranKeys.add(item.key);
    patchFlow({ liveLufs: null, liveTrace: [] });
    publish(globalIdx + 1, false, false);
  };

  // ONE per-row contract for a batched backend call, shared by the scene and
  // footswitch groups (only the map key differs): resolve a channel progress
  // item to its RunItem (active → spinner; done → outcome via the result
  // mappers; error → skipped; anything else — e.g. the backend's cancelled
  // sentinel — ignored), and after the call sweep rows the channel never
  // resolved — a stopped run leaves them queued (still selected for a
  // follow-up run); otherwise they're skipped (whole-call failure).
  interface BatchEntry {
    item: RunItem;
    idx: number;
  }
  const batchResolve =
    <K>(
      entries: Map<K, BatchEntry>,
      causeOf: (r: LevelOutcomeFields) => RunItem["verifyByEar"],
    ) =>
    (
      key: K,
      status: string,
      result: LevelOutcomeFields | null,
      message?: string | null,
      tail?: string | null,
    ) => {
      // Issue 6b: a `tail` caption ("Saving…" / "Verifying…") rides on a batch-wide
      // progress item that carries NO VALID row key — handle it BEFORE the entry
      // lookup below, independent of whatever key/status/result also arrived on the
      // same item, or it would be silently swallowed by the "unknown key" guard
      // exactly like the rows that guard already drops. Re-publish at the CURRENT
      // step (never invents progress) so the header subtitle picks it up.
      if (tail != null) {
        publish(lastPublishedIndex, false, false, tail);
      }
      const entry = entries.get(key);
      if (!entry) return;
      if (status === "active") {
        setSoleActive(entry.item);
        // The row's caption: the ceiling prepass's "measuring" (rendered as the verb
        // before the live number, since a capture IS streaming), or a note shown
        // verbatim when nothing is. See RunPage's rowStatus. Cleared once the row
        // resolves — or when a cancelled sweep reverts it — so a later re-run's default
        // "connecting…" isn't shadowed by a stale message.
        entry.item.activeMessage = message ?? null;
        publish(entry.idx, false, false);
      } else if (status === "done" && result) {
        entry.item.activeMessage = null;
        entry.item.outcome = outcomeOf(result);
        entry.item.value = valueOf(result);
        entry.item.spreadLu = result.dynamic_spread_lu;
        entry.item.verifyByEar = causeOf(result);
        finishItem(entry.item, entry.idx);
      } else if (status === "error") {
        entry.item.activeMessage = null;
        entry.item.outcome = "skipped";
        entry.item.skipReason = message ?? null;
        finishItem(entry.item, entry.idx);
      }
    };
  // A batched call (scene / footswitch) does ~8 s of un-streamed setup — the field-8
  // read, the preset load and their HID gaps — before the backend's first `active`
  // item arrives. Until then no row is active, so the run table reads "queued" for
  // every row and the header meter stays hidden: a dead-looking wizard. Flip the
  // group's first row active up front (the same thing the Base lane does before its
  // own await); the channel's own `active` items then take over per row.
  // `idx` is the group's own start index (the caller's loop counter), NOT re-derived
  // from the map: a Map keyed on scene slot returns its values in first-KEY order, so a
  // derived index could disagree with the one the channel publishes moments later and
  // make the step counter jump backward.
  const markGroupActive = <K>(entries: Map<K, BatchEntry>, idx: number) => {
    const rows = [...entries.values()];
    if (rows.length === 0) return;
    setSoleActive(rows[0].item);
    publish(idx, false, false);
  };
  const sweepUnresolved = <K>(
    entries: Map<K, BatchEntry>,
    failureReason: string | null,
  ) => {
    if (isCancelled()) {
      // A cancelled batch keeps un-run rows queued (still selected for a
      // follow-up run), but the optimistic `markGroupActive` row never got a
      // backend result — revert it or it spins "stopping…" forever on the
      // finished run (the final done publish picks this mutation up).
      // Drop its caption with it: a cancelled row never reaches `finishItem`,
      // which is the only other place that clears one, and every prepass row
      // now carries "measuring" — so leaving it would shadow the next run's
      // "connecting…" on each row the stopped run had already reached.
      for (const entry of entries.values()) {
        if (entry.item.status === "active") {
          entry.item.status = "queued";
          entry.item.activeMessage = null;
        }
      }
      return;
    }
    for (const entry of entries.values()) {
      if (entry.item.status !== "result") {
        entry.item.skipReason = failureReason;
        entry.item.outcome = "skipped";
        finishItem(entry.item, entry.idx);
      }
    }
  };

  // A row's effective target: the reachable-common-target override when set, else its
  // named target. Used at every dispatch site so the fallback re-levels everything to
  // one target while a normal run keeps each row's named pick. Shared with the run
  // table's Target cell so the two can't drift.
  const targetOf = (x: RunItem) => resolvedTargetLufs(x, targetLufsByName);

  // #285: the loop is wrapped so NOTHING it throws can strand the wizard mid-run
  // ("Saving the last one…" / a permanent "Stopping…") or leave `running` held.
  let i = 0;
  let threw = false;
  try {
    for (; i < total;) {
      if (isCancelled()) break;
      const it = work[i];
      // The common-target fallback marks signal-less rows `skipRelevel` — leave their
      // existing outcome as-is (no wasted re-capture) but keep them visible/counted.
      if (it.skipRelevel) {
        it.status = "result";
        publish(i + 1, false, false);
        i += 1;
        continue;
      }
      const profile = profileById(it.instId);
      const targetLufs = targetOf(it);
      const causeOf = causeFor(it.slot);

      if (it.isBase || (it.footswitch == null && it.sceneSlot == null)) {
        setSoleActive(it);
        publish(i, false, false);
        try {
          if (it.isBase) {
            const res = await levelPreset(
              buildLevelJob(it.slot, targetLufs, profile, true, it.baseHandle),
            );
            it.outcome = outcomeOf(res);
            it.value = valueOf(res);
            it.ceilingLufs = res.constant_c;
            it.spreadLu = res.dynamic_spread_lu;
            it.truePeakDbtp = res.true_peak_dbtp;
            it.verifyByEar = causeOf(res);
            it.baseBoost = res.base_boost;
          } else {
            // A scene item with no wire slot — nothing to level.
            it.outcome = "skipped";
          }
        } catch (error) {
          // One sound's failure shouldn't abort the run — retain its reason.
          it.skipReason = errMsg(error);
          it.outcome = "skipped";
        }
        finishItem(it, i);
        i += 1;
        continue;
      }

      if (it.footswitch != null) {
        // ── Block-acting FOOTSWITCHES — level each engaged state so stomping it
        // lands on target. ADJACENT rows of the same preset sharing an instrument
        // go in ONE call (per-job targets ride the jobs array): the backend
        // measures every switch, then writes them all on ONE session and saves the
        // preset ONCE — per-switch calls re-loaded + saved per switch. Bake vs
        // assign stays internal; the user only ever sees "leveled".
        let end = i + 1;
        while (
          end < total &&
          !work[end].skipRelevel &&
          work[end].footswitch != null &&
          work[end].slot === it.slot &&
          work[end].instId === it.instId
        ) {
          end += 1;
        }
        const group = work.slice(i, end);
        const bySwitch = new Map(
          group.flatMap((g, k) =>
            g.footswitch != null
              ? [[g.footswitch.switchIndex, { item: g, idx: i + k }] as const]
              : [],
          ),
        );
        const resolveFs = batchResolve(bySwitch, causeOf);
        markGroupActive(bySwitch, i);
        let failureReason: string | null = null;
        try {
          await levelFootswitchesApply(
            {
              slot: it.slot,
              jobs: group.flatMap((g) => {
                if (g.footswitch == null) return [];
                // The backend never creates a footswitch function any more (assign
                // only edits an EXISTING `param` fn or refuses), so there is nothing
                // for a row's display name to travel over the wire for.
                return [toFootswitchJobWire(g.footswitch, targetOf(g))];
              }),
              save: true,
              topologyId: profile?.topology_id ?? null,
              calibrationLufs: profile?.calibration_lufs ?? null,
              profileId: profile?.id ?? null,
            },
            (item) => {
              resolveFs(item.switch, item.status, item.result, item.message);
            },
          );
        } catch (error) {
          failureReason = errMsg(error);
        }
        sweepUnresolved(bySwitch, failureReason);
        i = end;
        continue;
      }

      // ── Scene rows: ADJACENT rows of the same preset sharing an instrument level
      // in ONE backend call (one prepass + one runner) REGARDLESS of per-scene target
      // — each job carries its own targetLufs (like the footswitch lane), so a
      // mixed-target preset no longer splits into several prepasses/saves. Each call
      // re-loads the preset, so the old per-scene dispatch flashed the unit back to the
      // preset's base between every scene — twice per scene of user-visible churn.
      // Per-scene progress rides the command's channel.
      let end = i + 1;
      while (
        end < total &&
        !work[end].skipRelevel &&
        isSceneItem(work[end]) &&
        work[end].slot === it.slot &&
        work[end].instId === it.instId
      ) {
        end += 1;
      }
      const group = work.slice(i, end);
      let cands = candCache.get(it.slot);
      if (!cands) {
        // Amp candidates come from the startup backup (no live discovery
        // round-trip). Fall back to a live block read only if the backup
        // missed this preset (rare — keeps a stray preset from silently
        // skipping its scenes). A failed read (e.g. a detach/reconnect emptied the
        // scan before Start) skips just this group, with the read's own reason.
        const fromBackup = ampCandidates.get(it.slot);
        if (fromBackup) {
          cands = fromBackup;
        } else {
          try {
            cands = ampBlocks(await listLevelBlocks(it.slot));
          } catch (error) {
            const reason = errMsg(error);
            group.forEach((g, k) => {
              g.outcome = "skipped";
              g.skipReason = reason;
              finishItem(g, i + k);
            });
            i = end;
            continue;
          }
        }
        candCache.set(it.slot, cands);
      }
      // A row that names its own control (`handle`) needs no amp candidate — mirrors
      // the backend's own mixed-batch guard (`level_scenes_apply_batched`'s doc). Only
      // bail LOCALLY (no device call at all) when NOBODY in the group has a handle;
      // a mixed group still dispatches, and the backend reports the amp-needing rows
      // skipped individually via the progress channel.
      const groupHasHandle = group.some((g) => g.handle != null);
      if (cands.length === 0 && !groupHasHandle) {
        group.forEach((g, k) => {
          g.outcome = "skipped";
          finishItem(g, i + k);
        });
        i = end;
        continue;
      }
      const byScene = new Map(
        group.map((g, k) => [g.sceneSlot, { item: g, idx: i + k }]),
      );
      const resolveScene = batchResolve(byScene, causeOf);
      markGroupActive(byScene, i);
      // Keep this preset's already-force-appended BASE job alive through the batch's
      // prepass + headroom-trade phases (issue 1 — the trade is otherwise unreachable
      // from the wizard, which levels Base via the separate `levelPreset` lane). The
      // base row for this slot ran FIRST (the run's base-first sort) and already holds
      // its result by the time this group dispatches, so its TARGET is available here —
      // look it up in `work`, not `group` (the base row is never part of a scene group).
      // Omit the key entirely (not `null`) when the preset has no base row selected:
      // conditional spread drops it from the serialized args, matching "no trade to
      // plan" rather than sending an anchor nothing asked for. The trade itself is
      // still a backend-internal behavior (see `notes/leveling.md`); the wizard no
      // longer surfaces which rows benefited from it.
      const baseItem = work.find((w) => w.slot === it.slot && w.isBase);
      const baseAnchor = baseItem
        ? { targetLufs: targetOf(baseItem) }
        : undefined;
      // ponytail: per-scene outcomes arrive via the Channel (`onResult`), NOT the returned
      // Promise value (deliberately discarded). `LevelResult` DOES now carry `scene_slot`
      // (identity, not position — the batch filters failed scenes out of the array it
      // returns), so a reconcile-by-scene off the return value is available if a
      // dropped-stream-item hardening is ever wanted; it simply isn't needed while the
      // Channel is the live progress path and this call is awaited to completion.
      // Consequence, unchanged: the offline e2e HTTP bridge no-ops the Channel, so scene
      // rows there resolve to "skipped" (their physics is gated at the command level
      // instead — see level-defaults.spec.ts).
      let failureReason: string | null = null;
      try {
        await levelScenesApplyBatched(
          {
            slot: it.slot,
            // Unlike the footswitch job's `lev*` fields (plain `String`s that reject an
            // explicit `null`), `handle` is `Option`/serde-defaulted on the Rust side —
            // an `undefined` value here is simply OMITTED by JSON serialization (never
            // sent as `null`).
            jobs: group.map((g) => ({
              sceneSlot: g.sceneSlot ?? 0,
              targetLufs: targetOf(g),
              handle: g.handle,
            })),
            candidates: cands,
            save: true,
            rebalance,
            topologyId: profile?.topology_id ?? null,
            calibrationLufs: profile?.calibration_lufs ?? null,
            profileId: profile?.id ?? null,
            ...(baseAnchor ? { baseAnchor } : {}),
          },
          (item) => {
            resolveScene(
              item.sceneSlot,
              item.status,
              item.result,
              item.message,
              item.tail,
            );
          },
        );
      } catch (error) {
        failureReason = errMsg(error);
      }
      sweepUnresolved(byScene, failureReason);
      i = end;
    }
  } catch (error) {
    // An unexpected throw mid-run (#285): pin its reason on the row it hit — the active
    // row, else the row the loop was on — and end the run as STOPPED, so the header shows
    // "Leveling stopped" + Continue and every row it never reached stays queued (and
    // still selected for a follow-up run).
    threw = true;
    const reason = errMsg(error);
    const hit =
      work.find((w) => w.status === "active") ??
      (i < total && work[i].status !== "result" ? work[i] : undefined);
    if (hit) {
      hit.activeMessage = null;
      hit.outcome = "skipped";
      hit.skipReason = reason;
      hit.status = "result";
      if (live()) ranKeys.add(hit.key);
    }
  } finally {
    // A Stop pressed during the LAST item never trips the top-of-loop check (that item
    // was already in flight), so snapshot the cancel flag the instant the loop exits —
    // BEFORE refresh — to report it as "stopped". Snapshotting pre-refresh also avoids a
    // false positive: a Stop pressed during refresh() (the run already finished) is
    // ignored, so a fully-completed run still auto-advances instead of mislabeling.
    const stopped = threw || isCancelled();
    // A row a stopped/aborted run never resolved goes back to queued, never left spinning.
    for (const w of work) {
      if (w.status === "active") {
        w.status = "queued";
        w.activeMessage = null;
      }
    }
    try {
      // The MOUNTED view's reload (non-throwing: runLoad catches into the error phase).
      if (live()) await boundRefresh?.();
    } catch {
      /* the list reload is advisory; the run's own terminal state below is not */
    }
    if (live()) {
      running = false;
      patchFlow({ liveLufs: null, liveTrace: [] });
      publish(total, true, stopped);
    }
  }
}

// Run "Stop" → halt the run in place; levels already written stay saved. Do NOT close
// the wizard — closing it hid that the in-flight item was still finishing on the device
// ("I don't know if it stopped"). Instead flip to "Stopping…" immediately; the in-flight
// command finishes, the loop breaks on `cancelRequested`, and `publish(total,true,stopped)`
// shows "Leveling stopped" + Continue. Module-level, so a REMOUNTED Level tab's Stop
// reaches the same run (#283).
function stopRun(): void {
  if (!running) return;
  cancelRequested = true;
  // ponytail: cancel all three lanes — the in-flight item is a base preset, a scene, OR
  // a footswitch, and cancelling the idle lanes is a harmless no-op. The backend bails at
  // its seams (skipping apply+save); the ~6 s capture still finishes, so "Stopping…"
  // reflects the current item winding down.
  void cancelPresetLeveling();
  void cancelSceneLeveling();
  void cancelFootswitchLeveling();
  patchFlow({ run: { ...flowState.run, stopping: true } });
}

// Run → Summary (auto on a natural finish, or via Continue after a manual stop).
function completeRun(): void {
  if (running) return;
  patchFlow({ stage: "summary" });
}

// Summary "Re-level clamped…" / "Re-run off target…" → re-enter at Set up with just
// that subset (re-level mode: the Set-up body hides the backup acknowledgment —
// already given). Each row keeps the instrument + target it was leveled with (#284).
function relevel(subset: RunItem[]): void {
  if (running) return;
  patchFlow({
    chosen: subset.map(runItemToOption),
    isRelevel: true,
    stage: "setup",
  });
}

// Toggle the opt-in rebalance (read at run time). Module state, so toggling it never
// re-renders the flow; SetupPage owns its own checkbox state.
function setRebalance(on: boolean): void {
  rebalance = on;
}

export interface UseLevelingFlowDeps {
  rows: PresetRow[];
  store: Store | null;
  /** Per-preset scenes from the backup read, keyed by 0-based list index. */
  sceneInfo: Map<number, SceneInfo[]>;
  /** Per-preset levelable footswitches from the SAME backup read, keyed by 0-based
   *  list index — resolves footswitch rows + their leveling coords with no extra read. */
  footswitchInfo: Map<number, FootswitchInfo[]>;
  /** Per-preset amp `outputLevel` candidates from the SAME backup read, keyed by
   *  0-based list index — so a scene run never needs a live discovery round-trip. */
  ampCandidates: Map<number, AmpCandidate[]>;
  /** Per-preset block roster (fender_ids) from the SAME backup read, keyed by
   *  0-based list index — drives the envelope-follower verify-by-ear cause. */
  blocksByIndex: Map<number, string[]>;
  /** Per-preset silence hint from the SAME backup read, keyed by 0-based list index —
   *  refines the offbranch row status (stamped on each RunItem at build). */
  silenceHintByIndex: Map<number, SilenceHint>;
  targetLufsByName: (name: string | null) => number;
  /** Drop just the given selection keys (BUG-4: prune the keys a run actually leveled,
   *  accumulating across re-level rounds, so un-run sounds stay selected). */
  deselectKeys: (keys: string[]) => void;
  refresh: () => Promise<void>;
}

/** Bind a Level view to the app-global wizard + run store. */
export function useLevelingFlow({
  rows,
  store,
  sceneInfo,
  footswitchInfo,
  ampCandidates,
  blocksByIndex,
  silenceHintByIndex,
  targetLufsByName,
  deselectKeys,
  refresh,
}: UseLevelingFlowDeps) {
  const state = useSyncExternalStore(subscribeFlow, getFlow);

  useEffect(() => {
    startLufsBridgeOnce();
  }, []);

  // The mounted view's list reload — what a run calls when it ends. Cleared on unmount
  // (a remount reloads the list itself), never left pointing at a dead view.
  useEffect(() => {
    boundRefresh = refresh;
    return () => {
      if (boundRefresh === refresh) boundRefresh = null;
    };
  }, [refresh]);

  const profileById = useCallback(
    (id: string): Profile | null =>
      store?.profiles.find((p) => p.id === id) ?? null,
    [store],
  );

  // "Level N presets…" — resolve the list's selected scene keys into the setup rows and
  // open the wizard at the Set-up step. The backup acknowledgment is an inline gate in
  // the Set-up footer (it no longer has its own step). Refused while a run is in flight.
  const openFlow = useCallback(
    (sel: Set<string>) => {
      if (running) return;
      const options = chosenFrom(sel, rows, sceneInfo, footswitchInfo);
      if (options.length === 0) return;
      patchFlow({ chosen: options, isRelevel: false, stage: "setup" });
    },
    [rows, sceneInfo, footswitchInfo],
  );

  // Close the wizard, deselecting exactly what a run leveled (BUG-4). Reset the run
  // bookkeeping so the next flow starts clean. A close with no run preserves selection.
  // Never while a run is in flight — the run page has no close; Stop is the way out.
  const closeFlow = useCallback(() => {
    if (running) return;
    patchFlow({ stage: "closed" });
    if (didRun) deselectKeys([...ranKeys]);
    didRun = false;
    ranKeys = new Set();
  }, [deselectKeys]);

  // Set-up "Level N sounds" (the COMMIT) → build run items and start. The backup
  // acknowledgment gated this button inline, so this goes straight to the run. A no-op
  // while another run is in flight (`runLeveling`'s own guard).
  const onSetupStart = useCallback(
    (choices: SetupChoice[]) => {
      if (choices.length === 0) return;
      const items = choices.map((c) => ({
        ...optionToRunItem(c.option, c.instId, c.targetName),
        silenceHint: silenceHintByIndex.get(c.option.slot),
      }));
      void runLeveling(items, {
        profileById,
        targetLufsByName,
        ampCandidates,
        blocksByIndex,
      });
    },
    [
      silenceHintByIndex,
      profileById,
      targetLufsByName,
      ampCandidates,
      blocksByIndex,
    ],
  );

  return {
    stage: state.stage,
    chosen: state.chosen,
    isRelevel: state.isRelevel,
    run: state.run,
    liveLufs: state.liveLufs,
    liveTrace: state.liveTrace,
    openFlow,
    onCancel: closeFlow,
    onSetupStart,
    onRunCancel: stopRun,
    onRunComplete: completeRun,
    onRelevel: relevel,
    // Summary "Accept" / "Done" → close, deselecting just the leveled sounds.
    onAccept: closeFlow,
    setRebalance,
  };
}
