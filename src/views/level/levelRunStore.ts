// src/views/level/levelRunStore.ts — the leveling flow's MODULE-SCOPED store.
//
// A leveling run outlives the Level tab: App renders only the active tab, so a tab switch
// unmounts `LevelView` while the run keeps driving the device. The flow state (stage,
// chosen rows, run progress, the Summary's rows) and the run's own bookkeeping (running,
// stop requested, the keys it leveled) therefore live HERE, not in component state — the
// `libraryScan` / `useLiveDevice` pattern. `useLevelingFlow` binds to this store with
// `useSyncExternalStore`, so a remounted Level tab shows the live run with a working Stop,
// and a second run cannot start while one is in flight.
//
// One flow can hold several runs: the first (from Set up) and any number of Summary
// "Try again" runs (`retryLeveling`), each on the first run's own settings (`RunEnv`) and
// with no Set up or backup tick in between. `run` is the run on screen (a Try again shows
// only the sounds it tries again); `results` is every sound any run of the flow ran, in
// run order, with its latest result — what the Summary lists. A run's rows merge back
// into `results` by key when it ends.
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
import { logError } from "../../lib/log";
import {
  sortRunItems,
  type RunItem,
  type SetupOption,
  type TargetLufsOf,
} from "./leveling";
import {
  applyPatches,
  runEngine,
  type EngineStep,
  type RunVerdict,
} from "./runEngine";
import { IDLE_METER, pushLiveMeter, setLiveMeter } from "./liveMeterStore";
import type { Stage } from "../overlays/wizardContext";
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
   *  already resolved, so no row's own status can show it. While set, `RunPage`'s now
   *  line says the preset is finishing (the backend's own word is not shown). Cleared on
   *  the run's completion and whenever a NEW row goes active, so it can't outlive the
   *  window it describes. */
  tailMessage: string | null;
}

const EMPTY_RUN: RunState = {
  items: [],
  currentIndex: 0,
  total: 0,
  phase: "idle",
  tailMessage: null,
};

/** A footswitch's scene picked on the Summary for its Try again (0-based `scenes[]`
 *  slot, or `null` = the main sound), keyed by the row's key. */
export type SummaryScenes = Partial<Record<string, number | null>>;

/** The whole flow as the Level view renders it. */
export interface LevelFlowState {
  /** `"closed"` ⇒ the flow is not shown. */
  stage: Stage;
  /** The Set-up rows. */
  chosen: SetupOption[];
  /** The run on screen (the latest one). */
  run: RunState;
  /** Every sound a run of this flow ran, in run order, with its latest result — the
   *  Summary's rows. Updated when a run ends. */
  results: RunItem[];
  /** Footswitch scenes changed on the Summary, waiting for Try again. */
  summaryScenes: SummaryScenes;
  /** Re-amp may still be on, so the unit's guitar input may still be muted: the Summary's
   *  restart Notice. The flow's latest reading, not any row's — a run overwrites it only
   *  when one of its commands read re-amp back (`RunVerdict.reampStillOn`), so a clean
   *  Try again clears it and a run that never reached the unit leaves it as it was. */
  reampStillOn: boolean;
}

const NO_SCENES: SummaryScenes = {};

const INITIAL: LevelFlowState = {
  stage: "closed",
  chosen: [],
  run: EMPTY_RUN,
  results: [],
  summaryScenes: NO_SCENES,
  reampStillOn: false,
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
// BUG-4: once a run has leveled at least one sound, closing the flow (Done) must DESELECT
// exactly the keys it leveled — and ACCUMULATE them across Try again rounds (each run adds
// its keys here), so the next Level can't silently re-run everything. A Cancel BEFORE any
// run leaves these empty, preserving the selection.
let didRun = false;
let ranKeys = new Set<string>();
// The first run's settings (instrument per sound via its rows, targets, candidates, the
// two-amp choice): every Try again of the flow runs on exactly these.
let flowEnv: RunEnv | null = null;
// The mounted view's `refresh`, so a run that ends while the Level tab is remounted
// refreshes the list the user is looking at, not the one that started the run.
let boundRefresh: (() => Promise<void>) | null = null;

/** True while a run is in flight. */
export function isLevelRunning(): boolean {
  return running;
}

/** Open Set up with `options`. */
export function openSetup(options: SetupOption[]): void {
  if (running) return;
  set({ chosen: options, stage: "setup" });
}

/** Close the flow. Returns the keys a run leveled since it opened (BUG-4: the caller
 *  deselects exactly these), or an empty list when nothing ran. Refused mid-run. */
export function closeFlow(): string[] {
  if (running) return [];
  const keys = didRun ? [...ranKeys] : [];
  didRun = false;
  ranKeys = new Set();
  flowEnv = null;
  set({
    stage: "closed",
    results: [],
    summaryScenes: NO_SCENES,
    reampStillOn: false,
  });
  return keys;
}

/** Run → Summary after a natural finish (a Stop lands on the Summary on its own). */
export function showSummary(): void {
  set({ stage: "summary" });
}

/** A footswitch's scene picked on its Summary row (`undefined` puts back the one used). */
export function setSummaryScene(
  key: string,
  scene: number | null | undefined,
): void {
  const next: SummaryScenes = Object.fromEntries(
    Object.entries(state.summaryScenes).filter(([k]) => k !== key),
  );
  if (scene !== undefined) next[key] = scene;
  set({ summaryScenes: next });
}

export function bindRefresh(refresh: (() => Promise<void>) | null): void {
  boundRefresh = refresh;
}

/** Stop the run in place; levels already written stay saved. The in-flight command
 *  finishes, the loop breaks, and the final publish lands on the Summary. */
export function requestStop(): void {
  if (!running) return;
  cancelRequested = true;
  // Cancel all three lanes — the in-flight item is a base preset, a scene, OR a
  // footswitch, and cancelling the idle lanes is a harmless no-op. The backend bails at
  // its seams (skipping apply+save); the in-flight capture still winds down, so the run
  // page says it is stopping after the current sound.
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
  flowEnv = null;
  state = INITIAL;
  setLiveMeter(IDLE_METER);
  for (const cb of subs) cb();
}

/** The device went away. Every row and Summary action is keyed by list index, and the
 *  next connection can map a different preset there (a Pro Control reorder while
 *  unplugged), so a Try again from the old Summary could write the wrong preset. Close
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
  targetLufsByName: TargetLufsOf;
  /** Per-preset amp `outputLevel` candidates from the startup backup, keyed by 0-based
   *  list index — so a scene run never needs a live discovery round-trip. */
  ampCandidates: Map<number, AmpCandidate[]>;
  /** Per-preset block roster (fender_ids), keyed by 0-based list index. */
  blocksByIndex: Map<number, string[]>;
  /** The presets (0-based list indices) whose path-MERGE scenes get their two
   *  parallel-amp lanes equalized first — Set up's "Balance its two amps first", per
   *  preset. */
  rebalance: ReadonlySet<number>;
  /** Reloads the preset list after the run (non-throwing). */
  refresh: () => Promise<void>;
}

/** The run's rows merged back into the flow's: a row the flow already holds is replaced
 *  in place (its run-order position kept), a new one is appended. */
function mergeResults(
  results: readonly RunItem[],
  ran: readonly RunItem[],
): RunItem[] {
  const byKey = new Map(ran.map((r) => [r.key, r]));
  const merged = results.map((r) => byKey.get(r.key) ?? r);
  const known = new Set(results.map((r) => r.key));
  return [...merged, ...ran.filter((r) => !known.has(r.key))];
}

/** One line in the app's log for each sound a run could not level and for a run that may
 *  have left re-amp on, in the backend's own words: the screen says what happened in plain
 *  words and never shows them, so this is where a bug report finds them. */
function logRunProblems(items: readonly RunItem[]): void {
  for (const it of items) {
    if (it.outcome === "skipped" && it.skipKind !== "cancelled")
      logError(
        `Leveling skipped ${it.presetName} · ${it.sceneName} (${it.skipKind ?? "other"}): ${it.skipReason ?? "no reason given"}`,
      );
  }
  const stuck = items.find((it) => it.reampStillOn === true);
  if (stuck)
    logError(
      `Leveling ended with re-amp possibly still on (${stuck.presetName}): ${stuck.reampWarning ?? "no warning text"}`,
    );
}

/** Start the flow's first run (Set up's Start): publish its rows, let `runEngine` level
 *  them, then refresh the list and publish the terminal phase. Its `env` is kept for the
 *  flow's Try again runs. A call while a run is in flight is ignored. */
export async function runLeveling(
  items: RunItem[],
  env: RunEnv,
): Promise<void> {
  if (running) return;
  flowEnv = env;
  set({ results: [], summaryScenes: NO_SCENES, reampStillOn: false });
  await drive(items, env, []);
}

/** The Summary's "Try again": run the flow's rows `keys` again straight away, on the first
 *  run's settings (no Set up, no backup tick — it was given for this flow), each footswitch
 *  in the scene picked for it on the Summary, then come back to the same Summary with
 *  their rows updated. Ignored mid-run or with no earlier run. */
export async function retryLeveling(keys: readonly string[]): Promise<void> {
  if (running || !flowEnv) return;
  const want = new Set(keys);
  const scenes = state.summaryScenes;
  const items: RunItem[] = state.results
    .filter((r) => want.has(r.key))
    .map((r) => {
      const scene = scenes[r.key];
      return {
        key: r.key,
        slot: r.slot,
        presetName: r.presetName,
        isBase: r.isBase,
        sceneSlot: r.sceneSlot,
        sceneName: r.sceneName,
        footswitch:
          r.footswitch != null && scene !== undefined
            ? { ...r.footswitch, sceneContext: scene }
            : (r.footswitch ?? null),
        instId: r.instId,
        targetName: r.targetName,
        status: "queued",
        handle: r.handle,
        baseHandle: r.baseHandle,
        silenceHint: r.silenceHint,
      };
    });
  if (items.length === 0) return;
  set({ summaryScenes: NO_SCENES });
  await drive(
    items,
    flowEnv,
    state.results.filter((r) => r.isBase),
  );
}

/** Drive one run of the flow. A Stop lands on the Summary as soon as the run has wound
 *  down; a natural finish leaves the run page to show its last row for a beat first. */
async function drive(
  items: RunItem[],
  env: RunEnv,
  earlierBaseRows: readonly RunItem[],
): Promise<void> {
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
    if (live()) pushLiveMeter(e.lufs);
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

  // No reading until the engine returns one (it never rejects; a reset mid-run drops it).
  let verdict: RunVerdict = { reampStillOn: null };
  try {
    verdict = await runEngine(
      work,
      {
        ...env,
        earlierBaseRows,
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
      logRunProblems(state.run.items);
      await (boundRefresh ?? env.refresh)(); // non-throwing: runLoad catches into the error phase
      if (live() && resetOnDetach) {
        clearFlow();
      } else if (live()) {
        finalizing = false;
        set({
          ...(stopped ? { stage: "summary" as const } : {}),
          results: mergeResults(state.results, state.run.items),
          reampStillOn: verdict.reampStillOn ?? state.reampStillOn,
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
