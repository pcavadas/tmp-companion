// src/views/level/useLevelingFlow.ts — the unified leveling WIZARD's binding for the
// Level view.
//
// The wizard's stage machine (setup → run → summary, all in one persistent frame) and the
// run (driven by the pure `runEngine`) live in the module-scoped `levelRunStore`, so a run
// survives a tab switch: this hook subscribes to that store and adapts the view's data
// (selection, store, backup maps) into the engine's inputs. The SCENES are chosen in the list (the scene tree); setup
// only configures them. The backup acknowledgment is an inline checkbox in the Set-up
// footer (no separate step) that gates the commit. Leveling always WRITES (save:true).
//
// No revert of any kind lives in this flow (design 1a, user directive): the backup
// acknowledgment is the one revert path (restore from a Pro Control backup), so this
// hook carries no gain-budget redistribution, reachable-common-target, or per-row
// "Restore original" state — a clamped row is reported and left for the user to
// re-level at a lower target, nothing more.

import { useCallback, useEffect, useMemo, useSyncExternalStore } from "react";

import {
  bindRefresh,
  closeFlow,
  getLevelFlow,
  openSetup,
  requestStop,
  runLeveling,
  showSummary,
  subscribeLevelFlow,
} from "./levelRunStore";
import {
  chosenFrom,
  optionToRunItem,
  runItemToOption,
  type RunItem,
  type SetupChoice,
  type PresetRow,
} from "./leveling";
import type {
  SceneInfo,
  FootswitchInfo,
  AmpCandidate,
  Store,
  Profile,
  SilenceHint,
} from "../../lib/types";

export type { RunPhase, RunState } from "./levelRunStore";

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

  // "Level N presets…" — resolve the list's selected scene keys into the setup rows and
  // open the wizard at the Set-up step. The backup acknowledgment is an inline gate in
  // the Set-up footer (it no longer has its own step).
  const openFlow = useCallback(
    (sel: Set<string>) => {
      const options = chosenFrom(sel, rows, sceneInfo, footswitchInfo);
      if (options.length === 0) return;
      openSetup(options, false);
    },
    [rows, sceneInfo, footswitchInfo],
  );

  // Close the wizard, deselecting exactly what a run leveled (BUG-4). A close with no run
  // preserves the selection.
  const onClose = useCallback(() => {
    const keys = closeFlow();
    if (keys.length > 0) deselectKeys(keys);
  }, [deselectKeys]);

  // Set-up "Level N sounds" (the COMMIT) → build run items and start. The backup
  // acknowledgment gated this button inline, so this goes straight to the run. Set up
  // hands over its opt-in rebalance toggle with the choices.
  const onSetupStart = useCallback(
    (choices: SetupChoice[], rebalance = false) => {
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
        rebalance,
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

  // Summary "Re-level clamped…" / "Re-run off target…" → re-enter at Set up with just
  // that subset (re-level mode: the Set-up body hides the backup acknowledgment —
  // already given). Each row carries the instrument and target it was leveled with.
  const onRelevel = useCallback((subset: RunItem[]) => {
    openSetup(subset.map(runItemToOption), true);
  }, []);

  // One object per store publish (or binding change), so a consumer's memo/callback that
  // depends on it — LevelView's `handleLevel` — is not rebuilt on every render.
  return useMemo(
    () => ({
      stage: flow.stage,
      chosen: flow.chosen,
      isRelevel: flow.isRelevel,
      run: flow.run,
      openFlow,
      onCancel: onClose,
      onSetupStart,
      // Run "Stop" → halt the run in place; levels already written stay saved. Do NOT
      // close the wizard — closing it hid that the in-flight item was still finishing on
      // the device ("I don't know if it stopped"). The store flips to "Stopping…" at once
      // and shows "Leveling stopped" + Continue when the in-flight command finishes.
      onRunCancel: requestStop,
      // Run → Summary (auto on a natural finish, or via Continue after a manual stop).
      onRunComplete: showSummary,
      onRelevel,
      // Summary "Accept" / "Done" → close, deselecting just the leveled sounds.
      onAccept: onClose,
    }),
    [flow, openFlow, onClose, onSetupStart, onRelevel],
  );
}
