// src/views/level/levelRunStore.ts — the leveling wizard's MODULE-SCOPED store.
//
// A leveling run outlives the Level tab: App renders only the active tab, so a tab switch
// unmounts `LevelView` while the run keeps driving the device. The wizard state (stage,
// chosen rows, run progress) and the run's own bookkeeping (running, stop
// requested, the keys it leveled) therefore live HERE, not in component state — the
// `libraryScan` / `useLiveDevice` pattern. `useLevelingFlow` binds to this store with
// `useSyncExternalStore`, so a remounted Level tab shows the live run with a working Stop,
// and a second run cannot start while one is in flight.
//
// The engine itself (`runEngine`) is plain async code: this module feeds it the device
// commands and applies the immutable patches it emits, so a published `RunState.items`
// object is never mutated. The live meter streams into its own store (`liveMeterStore`),
// so a meter tick never re-renders the views bound here. The run sits in a try/finally,
// so it ALWAYS ends in a terminal phase with `running` released, whatever throws. Every
// backend command ends its own run with re-amp OFF on a fresh connection (danger.md), so
// nothing here depends on the view being mounted.

import {
  levelPreset,
  cancelPresetLeveling,
  listLevelBlocks,
  levelScenesApplyBatched,
  cancelSceneLeveling,
  levelFootswitchesApply,
  cancelFootswitchLeveling,
} from "../../lib/invoke";
import { onLive } from "../../lib/liveEvents";
import { sortRunItems, type RunItem, type SetupOption } from "./leveling";
import { applyPatches, runEngine, type EngineStep } from "./runEngine";
import { IDLE_METER, pushLiveMeter, setLiveMeter } from "./liveMeterStore";
import type { Stage } from "../overlays";
import type { AmpCandidate, Profile } from "../../lib/types";

/** Where a run is: none yet, in flight, winding down after a Stop, or finished on its own
 *  (`done`) or by a Stop (`stopped`). */
export type RunPhase = "idle" | "running" | "stopping" | "done" | "stopped";

/** A finished run, either way. */
export const isRunOver = (phase: RunPhase): boolean =>
  phase === "done" || phase === "stopped";

/** The run's live state, published by the run loop and read by RunPage/SummaryPage. */
export interface RunState {
  items: RunItem[];
  currentIndex: number;
  total: number;
  phase: RunPhase;
  /** A batch-wide caption ("Saving…" / "Verifying…") from a `SceneLevelProgressItem.tail`
   *  (issue 6b) — the deferred-save/persist-verify phase AFTER every scene in the group
   *  already resolved, so no row's own status can show it. `RunPage`'s header renders
   *  this in preference while set. Cleared on the run's completion and whenever a NEW
   *  row goes active, so it can't outlive the window it describes. */
  tailMessage: string | null;
}

const EMPTY_RUN: RunState = {
  items: [],
  currentIndex: 0,
  total: 0,
  phase: "idle",
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
}

const INITIAL: LevelFlowState = {
  stage: "closed",
  chosen: [],
  isRelevel: false,
  run: EMPTY_RUN,
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
  set({ run: { ...state.run, phase: "stopping" } });
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
  setLiveMeter(IDLE_METER);
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
  /** Opt-in: equalize a path-MERGE scene's two parallel-amp lanes (Set up's toggle). */
  rebalance: boolean;
  /** Reloads the preset list after the run (non-throwing). */
  refresh: () => Promise<void>;
}

/** Drive a run: publish its rows, let `runEngine` level them, then refresh the list and
 *  publish the terminal phase. A call while a run is in flight is ignored. */
export async function runLeveling(
  items: RunItem[],
  env: RunEnv,
): Promise<void> {
  if (running) return;
  running = true;
  cancelRequested = false;
  didRun = true;
  const gen = generation;
  const live = () => gen === generation;
  // Read the cancel flag through a getter so it isn't narrowed to its last-assigned
  // literal across awaits — the Stop handler flips it meanwhile.
  const isCancelled = () => cancelRequested;

  // The live meter follows THIS run only: subscribed here, dropped in the finally.
  const unlistenLufs = onLive("levelingLufs", (e) => {
    if (live()) pushLiveMeter(e.lufs, e.momentary);
  });

  // Presets in first-appearance order, each preset's rows in `runRank` dependency
  // order (base first) — `sortRunItems` enforces it whatever order `items` arrived in.
  const work = sortRunItems(items);
  const total = work.length;
  setLiveMeter(IDLE_METER);
  set({
    stage: "run",
    run: {
      items: work,
      currentIndex: 0,
      total,
      phase: "running",
      tailMessage: null,
    },
  });

  const apply = (step: EngineStep) => {
    if (!live()) return;
    if (step.resolvedKey != null) {
      ranKeys.add(step.resolvedKey);
      setLiveMeter(IDLE_METER);
    }
    set({
      run: {
        items: applyPatches(state.run.items, step.patches),
        currentIndex: step.currentIndex,
        total,
        phase: isCancelled() ? "stopping" : "running",
        tailMessage: step.tailMessage,
      },
    });
  };

  try {
    await runEngine(
      work,
      {
        ...env,
        levelPreset,
        listLevelBlocks,
        levelScenesApplyBatched,
        levelFootswitchesApply,
        isCancelled,
      },
      apply,
    );
  } finally {
    // A Stop pressed during the LAST item never trips the engine's between-batch check
    // (that item was already in flight), so snapshot the cancel flag the instant it
    // returns — BEFORE refresh — to report it as "stopped". Snapshotting pre-refresh also
    // avoids a false positive: a Stop pressed during refresh() (the run already finished)
    // is ignored, so a fully-completed run still auto-advances instead of mislabeling.
    const stopped = isCancelled();
    void unlistenLufs.then((u) => {
      u();
    });
    if (live()) {
      running = false;
      finalizing = true;
      cancelRequested = false;
      setLiveMeter(IDLE_METER);
      await (boundRefresh ?? env.refresh)(); // non-throwing: runLoad catches into the error phase
      if (live() && resetOnDetach) {
        clearFlow();
      } else if (live()) {
        finalizing = false;
        set({
          run: {
            ...state.run,
            currentIndex: total,
            phase: stopped ? "stopped" : "done",
            tailMessage: null,
          },
        });
      }
    }
  }
}
