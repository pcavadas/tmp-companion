// src/views/level/useLevelingFlow.ts — the leveling flow's binding for the Level view.
//
// The flow's stage machine (Set up → run → Summary, each a page in the Level tab) and the
// run (driven by the pure `runEngine`) live in the module-scoped `levelRunStore`, so a run
// survives a tab switch: this hook subscribes to that store and adapts the view's data
// (selection, store, backup maps) into the engine's inputs. The sounds are chosen in the
// list (the scene tree); Set up only configures them. The backup acknowledgment is an
// inline checkbox in the Set-up footer that gates the commit. Leveling always WRITES
// (save:true). The Summary's one "Try again" re-runs sounds straight away on the first
// run's settings (`retryLeveling`), with no second Set up and no second backup tick.
//
// No revert of any kind lives in this flow (design 1a, user directive): the backup
// acknowledgment is the one revert path (restore from a Pro Control backup), so this
// hook carries no gain-budget redistribution, reachable-common-target, or per-row
// "Restore original" state — a sound that can't reach its target is reported with what
// to change, nothing more.

import { useCallback, useEffect, useMemo, useSyncExternalStore } from "react";

import {
  bindRefresh,
  closeFlow,
  getLevelFlow,
  openSetup,
  requestStop,
  retryLeveling,
  runLeveling,
  setSummaryScene,
  showSummary,
  subscribeLevelFlow,
} from "./levelRunStore";
import {
  chosenFrom,
  optionToRunItem,
  type SetupChoice,
  type PresetRow,
  type TargetLufsOf,
} from "./leveling";
import type {
  SceneInfo,
  FootswitchInfo,
  AmpCandidate,
  Store,
  Profile,
  SilenceHint,
} from "../../lib/types";

export type { RunPhase, RunState, SummaryScenes } from "./levelRunStore";

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
  targetLufsByName: TargetLufsOf;
  /** Drop just the given selection keys (BUG-4: prune the keys a run actually leveled,
   *  accumulating across Try again rounds, so un-run sounds stay selected). */
  deselectKeys: (keys: string[]) => void;
  refresh: () => Promise<void>;
}

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
  const flow = useSyncExternalStore(subscribeLevelFlow, getLevelFlow);

  // A run that ends while this view is mounted refreshes THIS view's list.
  useEffect(() => {
    bindRefresh(refresh);
    return () => {
      bindRefresh(null);
    };
  }, [refresh]);

  const profileById = useCallback(
    (id: string): Profile | null =>
      store?.profiles.find((p) => p.id === id) ?? null,
    [store],
  );

  // "Level N sounds" — resolve the list's selected sound keys into the Set up rows and
  // open the flow at Set up. The backup acknowledgment is an inline gate in the Set-up
  // footer (it has no step of its own).
  const openFlow = useCallback(
    (sel: Set<string>) => {
      const options = chosenFrom(sel, rows, sceneInfo, footswitchInfo);
      if (options.length === 0) return;
      openSetup(options);
    },
    [rows, sceneInfo, footswitchInfo],
  );

  // Close the flow, deselecting exactly what a run leveled (BUG-4). A close with no run
  // preserves the selection.
  const onClose = useCallback(() => {
    const keys = closeFlow();
    if (keys.length > 0) deselectKeys(keys);
  }, [deselectKeys]);

  // Set up's "Start leveling" (the COMMIT) → build run items and start. The backup
  // acknowledgment gated this button inline, so this goes straight to the run. Set up
  // hands over the presets whose two amps are balanced first with the choices.
  const onSetupStart = useCallback(
    (choices: SetupChoice[], rebalance: readonly number[] = []) => {
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
        rebalance: new Set(rebalance),
        refresh,
      });
    },
    [
      silenceHintByIndex,
      profileById,
      targetLufsByName,
      ampCandidates,
      blocksByIndex,
      refresh,
    ],
  );

  // Summary "Try again (N)" → run those sounds again at once, on the first run's
  // settings, and come back to the same Summary.
  const onTryAgain = useCallback((keys: readonly string[]) => {
    void retryLeveling(keys);
  }, []);

  // One object per store publish (or binding change), so a consumer's memo/callback that
  // depends on it — LevelView's `handleLevel` — is not rebuilt on every render.
  return useMemo(
    () => ({
      stage: flow.stage,
      chosen: flow.chosen,
      run: flow.run,
      results: flow.results,
      summaryScenes: flow.summaryScenes,
      reampStillOn: flow.reampStillOn,
      openFlow,
      onCancel: onClose,
      onSetupStart,
      // The Stop dialog's "Stop" → halt the run in place; levels already written stay
      // saved. The flow stays on the run page, which says it is stopping after the sound
      // in progress, and lands on the Summary once that sound has wound down.
      onRunCancel: requestStop,
      // Run → Summary, a beat after a natural finish.
      onRunComplete: showSummary,
      onTryAgain,
      onSummaryScene: setSummaryScene,
      // Summary "Done" → close, deselecting just the leveled sounds.
      onDone: onClose,
    }),
    [flow, openFlow, onClose, onSetupStart, onTryAgain],
  );
}
