// src/views/level/levelRunStore.ts — the leveling wizard's MODULE-SCOPED store and run
// engine.
//
// A leveling run outlives the Level tab: App renders only the active tab, so a tab switch
// unmounts `LevelView` while the run keeps driving the device. The wizard state (stage,
// chosen rows, run progress, live meter) and the run's own bookkeeping (running, stop
// requested, the keys it leveled) therefore live HERE, not in component state — the
// `libraryScan` / `useLiveDevice` pattern. `useLevelingFlow` binds to this store with
// `useSyncExternalStore`, so a remounted Level tab shows the live run with a working Stop,
// and a second run cannot start while one is in flight.
//
// The engine drives the run by composing shipped commands per chosen scene:
//   • BASE scene (or a scene-less "Whole preset") → level_preset (preset `presetLevel`,
//     preset-to-preset loudness).
//   • FS scenes → list_level_blocks (amp candidates, once per preset, cached) then
//     level_scenes_apply_batched with ADJACENT same-preset scenes sharing an instrument
//     batched into ONE call regardless of per-scene target (each job carries its own
//     targetLufs, like the footswitch lane) (amp `outputLevel`, scene-to-scene; per-scene
//     progress rides the channel) — per-scene calls re-loaded the preset each time,
//     flashing the unit back to base twice per scene.
//   • Block-acting footswitches → level_footswitches_apply, batched the same way.
// Each step is isolated: a per-item failure becomes "skipped", never aborting the run, and
// the whole loop sits in a try/finally so the run ALWAYS ends in a terminal state with
// `running` released, whatever throws. Every backend command ends its own run with re-amp
// OFF on a fresh connection (danger.md), so nothing here depends on the view being mounted.

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
  resolvedTargetLufs,
  sortRunItems,
  type RunItem,
  type SetupOption,
} from "./leveling";
import type { Stage } from "../overlays";
import type {
  AmpCandidate,
  LevelBlock,
  Profile,
  Verification,
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
// has no `verify_by_ear`, so its by-ear cause is `wet_floor` or none.
interface LevelOutcomeFields {
  clamped: boolean;
  clamp_reason: string | null;
  verify_lufs: number | null;
  predicted_lufs: number;
  verify_by_ear?: boolean;
  /** Footswitch rows only — `LevelResult` (preset/scene) has no such field, so it stays
   *  optional and those lanes simply never report an unconverged row. */
  unconverged?: boolean;
  /** Footswitch rows only: the clamp's pinned bound is the wet/mix floor, not headroom —
   *  see `FootswitchLevelResult.wet_floor`. */
  wet_floor?: boolean;
  /** Preset/scene rows only (footswitch rows always measure their result). */
  verification?: Verification;
}

// A `clamp_reason` is set ONLY when the leveled signal isn't effectively reaching the USB 1/2
// capture — a silent capture for preset/footswitch jobs (output not routed to USB), or the
// scene path's no-authority case (a big amp-outputLevel change doesn't move the capture:
// off-branch) → its own `offbranch` outcome ("not on USB 1/2"), which a re-level can't fix.
// A plain headroom/authority clamp (the knob has real effect but can't reach target) has
// `clamped` set with NO reason → "clamped at X". `unconverged` is last of the miss states
// (the backend's own precedence): it is only ever set when `clamped` is not, and it means
// the knob had room left, so a re-run helps. A row whose check capture FAILED is reported
// the same way: its number is only a prediction, and a re-run confirms it.
const unverifiedOf = (r: LevelOutcomeFields): boolean =>
  r.verification === "verify_failed";
const outcomeOf = (r: LevelOutcomeFields): RunItem["outcome"] =>
  r.clamp_reason != null
    ? "offbranch"
    : r.clamped
      ? "clamped"
      : r.unconverged || unverifiedOf(r)
        ? "unconverged"
        : "done";
const valueOf = (r: LevelOutcomeFields): number =>
  r.verify_lufs ?? r.predicted_lufs;
// Resolve to a SINGLE by-ear cause. `wet_floor` only ever rides on a footswitch row, which
// never sets `verify_by_ear` (scene-only), so the two can't collide in practice. There is no
// dynamics-spread cause (#241): a wide-spread capture is not a measurement problem.
const byEarCause = (r: LevelOutcomeFields): RunItem["verifyByEar"] =>
  r.wet_floor ? "wet_floor" : r.verify_by_ear ? "rebalance" : undefined;

/** Copy a batched command's run-end re-amp warning (stamped on every row it returned)
 *  onto every run item of the batch, so the Summary can show it. */
const stampReampWarning = (
  group: RunItem[],
  rows: readonly { reamp_warning?: string | null }[],
): void => {
  const warning = rows.find((r) => r.reamp_warning)?.reamp_warning ?? null;
  if (!warning) return;
  for (const g of group) g.reampWarning = warning;
};

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

/** The whole wizard as the Level view renders it. */
export interface LevelFlowState {
  /** `"closed"` ⇒ the wizard is not shown. */
  stage: Stage;
  /** The Set-up rows. */
  chosen: SetupOption[];
  /** Set up was reopened from the Summary for a re-run (hides the backup checkbox). */
  isRelevel: boolean;
  run: RunState;
  /** Advisory live measured loudness for the active run row's "measuring…" readout. Kept
   *  apart from `run` so a run publish never clobbers an event update. The backend only
   *  emits during a capture; reset on each item resolve and at run start. */
  liveLufs: number | null;
  /** Rolling per-hop momentary levels (dB) for the live VU bars — last 24 hops. */
  liveTrace: number[];
}

const INITIAL: LevelFlowState = {
  stage: "closed",
  chosen: [],
  isRelevel: false,
  run: EMPTY_RUN,
  liveLufs: null,
  liveTrace: [],
};

let state: LevelFlowState = INITIAL;
const subs = new Set<() => void>();

function set(next: Partial<LevelFlowState>): void {
  state = { ...state, ...next }; // new ref so useSyncExternalStore re-renders
  for (const cb of subs) cb();
}

export function subscribeLevelFlow(cb: () => void): () => void {
  subs.add(cb);
  return () => {
    subs.delete(cb);
  };
}

export function getLevelFlow(): LevelFlowState {
  return state;
}

// ── run bookkeeping (module-scoped for the same reason as the state) ────────────────
let running = false;
// True from the moment `running` drops until the run's final publish — the end-of-run
// list refresh sits in between, and a detach during it must not let the Summary land.
let finalizing = false;
// A device detach during a run or its finalization: the run's rows are keyed by list
// index, which a reconnect can re-map, so the flow resets once the run has wound down.
let resetOnDetach = false;
let cancelRequested = false;
// Bumped by `resetLevelFlow` so a run still awaiting a command from before the reset can
// never publish into the fresh state.
let generation = 0;
// BUG-4: once a run has leveled at least one sound, closing the wizard (Done OR Cancel)
// must DESELECT exactly the keys it leveled — and ACCUMULATE them across re-level rounds
// (each re-level run adds its keys here), so a Re-level→Cancel can't leave the whole
// selection ticked and silently re-run everything next time. A Cancel BEFORE any run
// leaves these empty, preserving the selection.
let didRun = false;
let ranKeys = new Set<string>();
// Opt-in: equalize a path-MERGE scene's two parallel-amp lanes before joint-k. SetupPage
// owns the visible pill and pushes its value here (incl. on mount, so a fresh Set-up resets
// it). Read at run time.
let rebalance = false;
// The mounted view's `refresh`, so a run that ends while the Level tab is remounted
// refreshes the list the user is looking at, not the one that started the run.
let boundRefresh: (() => Promise<void>) | null = null;

/** True while a run is in flight. */
export function isLevelRunning(): boolean {
  return running;
}

/** Open Set up with `options` (`isRelevel`: reopened from the Summary). */
export function openSetup(options: SetupOption[], isRelevel: boolean): void {
  if (running) return;
  set({ chosen: options, isRelevel, stage: "setup" });
}

/** Close the wizard. Returns the keys a run leveled since it opened (BUG-4: the caller
 *  deselects exactly these), or an empty list when nothing ran. Refused mid-run. */
export function closeFlow(): string[] {
  if (running) return [];
  const keys = didRun ? [...ranKeys] : [];
  didRun = false;
  ranKeys = new Set();
  set({ stage: "closed" });
  return keys;
}

/** Run → Summary (auto on a natural finish, or via Continue after a manual stop). */
export function showSummary(): void {
  set({ stage: "summary" });
}

export function setRebalance(on: boolean): void {
  rebalance = on;
}

export function bindRefresh(refresh: (() => Promise<void>) | null): void {
  boundRefresh = refresh;
}

/** Stop the run in place; levels already written stay saved. The in-flight command
 *  finishes, the loop breaks, and the final publish shows "Leveling stopped". */
export function requestStop(): void {
  if (!running) return;
  cancelRequested = true;
  // Cancel all three lanes — the in-flight item is a base preset, a scene, OR a
  // footswitch, and cancelling the idle lanes is a harmless no-op. The backend bails at
  // its seams (skipping apply+save); the ~6 s capture still finishes, so "Stopping…"
  // reflects the current item winding down.
  void cancelPresetLeveling();
  void cancelSceneLeveling();
  void cancelFootswitchLeveling();
  set({ run: { ...state.run, stopping: true } });
}

/** Back to a closed wizard with no run, keeping the view's bindings. */
function clearFlow(): void {
  generation++;
  running = false;
  finalizing = false;
  resetOnDetach = false;
  cancelRequested = false;
  didRun = false;
  ranKeys = new Set();
  state = INITIAL;
  for (const cb of subs) cb();
}

/** The device went away. Every row and Summary action is keyed by list index, and the
 *  next connection can map a different preset there (a Pro Control reorder while
 *  unplugged), so a Re-level from the old Summary could write the wrong preset. Close
 *  the flow now when idle; a run in flight (or finishing) resets once it has ended. */
export function onDeviceDetached(): void {
  if (running || finalizing) {
    resetOnDetach = true;
    return;
  }
  clearFlow();
}

/** Test isolation only: back to a closed wizard with no run. */
export function resetLevelFlow(): void {
  clearFlow();
  rebalance = false;
  boundRefresh = null;
}

/** What the engine needs from the view, captured when the run starts. */
export interface RunEnv {
  profileById: (id: string) => Profile | null;
  targetLufsByName: (name: string | null) => number;
  /** Per-preset amp `outputLevel` candidates from the startup backup, keyed by 0-based
   *  list index — so a scene run never needs a live discovery round-trip. */
  ampCandidates: Map<number, AmpCandidate[]>;
  /** Per-preset block roster (fender_ids), keyed by 0-based list index. */
  blocksByIndex: Map<number, string[]>;
  /** Reloads the preset list after the run (non-throwing). */
  refresh: () => Promise<void>;
}

/** Drive a run sequentially: one chosen row at a time, mark active → result. A call while
 *  a run is in flight is ignored. */
export async function runLeveling(
  items: RunItem[],
  env: RunEnv,
): Promise<void> {
  if (running) return;
  running = true;
  cancelRequested = false;
  didRun = true;
  const gen = generation;
  const { profileById, targetLufsByName, ampCandidates, blocksByIndex } = env;

  // The live meter follows THIS run only: subscribed here, dropped in the finally.
  const unlistenLufs = onLevelingLufs((e) => {
    if (gen !== generation) return;
    set({
      liveLufs: e.lufs,
      liveTrace: [...state.liveTrace, e.momentary].slice(-24),
    });
  });

  // Presets in first-appearance order, each preset's rows in `runRank` dependency
  // order (base first) — `sortRunItems` enforces it whatever order `items` arrived in.
  const work = sortRunItems(items).map((it) => ({ ...it }));
  const total = work.length;
  let lastPublishedIndex = 0;
  let runError: string | null = null;
  try {
    // Read the cancel flag through a getter so it isn't narrowed to its
    // last-assigned literal across awaits — the stop handler flips it meanwhile.
    const isCancelled = () => cancelRequested;
    const candCache = new Map<number, Candidate[]>();

    // `work` is mutated in place between publishes; pass a fresh ARRAY each time
    // (new ref so React renders) but skip the per-item spread — the bodies only read
    // items during render, never hold them across renders.
    // `lastPublishedIndex` lets a tail-only progress item (no row of its own — see
    // `batchResolve` below) re-publish the caption without inventing a step index.
    const publish = (
      currentIndex: number,
      done: boolean,
      stopped: boolean,
      tailMessage: string | null = null,
    ) => {
      lastPublishedIndex = currentIndex;
      // Once cancel is requested, every publish carries `stopping` until the final
      // done publish clears it (done ⇒ either "complete" or "stopped"). Every call
      // site but the tail-caption one below omits `tailMessage`, so the default
      // `null` clears it on the run's completion AND on every row activation —
      // exactly the two points issue 6b's contract requires.
      if (gen !== generation) return;
      set({
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

    set({ stage: "run", liveLufs: null, liveTrace: [] });
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
      ranKeys.add(item.key);
      if (gen === generation) set({ liveLufs: null, liveTrace: [] });
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
          entry.item.unverified = unverifiedOf(result);
          entry.item.value = valueOf(result);
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

    for (let i = 0; i < total;) {
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
            it.unverified = unverifiedOf(res);
            it.value = valueOf(res);
            it.ceilingLufs = res.constant_c;
            it.truePeakDbtp = res.true_peak_dbtp;
            it.verifyByEar = causeOf(res);
            it.baseBoost = res.base_boost;
            it.reampWarning = res.reamp_warning ?? null;
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
          const rows = await levelFootswitchesApply(
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
          // The rows streamed before the run-end re-amp OFF; its verdict rides the
          // returned rows only.
          stampReampWarning(group, rows);
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
        // skipping its scenes). That read can fail (the unit detached and the
        // scan was reset); like any other per-item failure it skips this
        // group with its reason and the run carries on.
        const fromBackup = ampCandidates.get(it.slot);
        if (fromBackup) {
          cands = fromBackup;
        } else {
          try {
            cands = ampBlocks(await listLevelBlocks(it.slot));
          } catch (error) {
            const reason = errMsg(error);
            group.forEach((g, k) => {
              g.skipReason = reason;
              g.outcome = "skipped";
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
      // Promise value — read only for the run-end re-amp warning. `LevelResult` DOES now carry `scene_slot`
      // (identity, not position — the batch filters failed scenes out of the array it
      // returns), so a reconcile-by-scene off the return value is available if a
      // dropped-stream-item hardening is ever wanted; it simply isn't needed while the
      // Channel is the live progress path and this call is awaited to completion.
      // Consequence, unchanged: the offline e2e HTTP bridge no-ops the Channel, so scene
      // rows there resolve to "skipped" (their physics is gated at the command level
      // instead — see level-defaults.spec.ts).
      let failureReason: string | null = null;
      try {
        const rows = await levelScenesApplyBatched(
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
        stampReampWarning(group, rows);
      } catch (error) {
        failureReason = errMsg(error);
      }
      sweepUnresolved(byScene, failureReason);
      i = end;
    }
  } catch (error) {
    // Anything the per-item handlers didn't catch: report it on every row still
    // unresolved, so the run ends in a terminal state the user can read.
    runError = errMsg(error);
  } finally {
    // A Stop pressed during the LAST item never trips the top-of-loop check (that item
    // was already in flight), so snapshot the cancel flag the instant the loop exits —
    // BEFORE refresh — to report it as "stopped". Snapshotting pre-refresh also avoids a
    // false positive: a Stop pressed during refresh() (the run already finished) is
    // ignored, so a fully-completed run still auto-advances instead of mislabeling.
    const stopped = cancelRequested;
    if (runError != null) {
      for (const w of work) {
        if (w.status !== "result") {
          w.status = "result";
          w.outcome = "skipped";
          w.skipReason = runError;
          w.activeMessage = null;
        }
      }
    }
    void unlistenLufs.then((u) => {
      u();
    });
    if (gen === generation) {
      running = false;
      finalizing = true;
      cancelRequested = false;
      set({ liveLufs: null, liveTrace: [] });
      await (boundRefresh ?? env.refresh)(); // non-throwing: runLoad catches into the error phase
      if (gen === generation && resetOnDetach) {
        clearFlow();
      } else if (gen === generation) {
        finalizing = false;
        set({
          run: {
            items: [...work],
            currentIndex: total,
            total,
            done: true,
            stopped,
            stopping: false,
            tailMessage: null,
          },
        });
      }
    }
  }
}
