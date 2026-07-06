// src/views/level/useLevelingFlow.ts — the unified leveling WIZARD orchestrator.
//
// Owns the wizard's stage machine (setup → run → summary, all in one persistent
// frame) and DRIVES the run by composing shipped commands per chosen scene:
//   • BASE scene (or a scene-less "Whole preset") → level_preset (preset `presetLevel`,
//     preset-to-preset loudness).
//   • FS scene → list_level_blocks (amp candidates, once per preset, cached) then
//     level_scenes_apply_batched for that one scene (amp `outputLevel`, scene-to-scene).
// The SCENES are chosen in the list (the scene tree); setup only configures them. The
// backup acknowledgment is an inline checkbox in the Set-up footer (no separate step)
// that gates the commit. Leveling always WRITES (save:true). Each step is isolated: a
// per-item failure becomes "skipped", never aborting the run.

import { useCallback, useEffect, useRef, useState } from "react";

import {
  levelPreset,
  cancelPresetLeveling,
  listLevelBlocks,
  levelScenesApplyBatched,
  cancelSceneLeveling,
  levelFootswitchesApply,
  cancelFootswitchLeveling,
} from "../../lib/invoke";
import { onLevelingLufs } from "../../lib/liveEvents";
import { MODELS } from "../../models/catalog";
import { resolveDeviceId } from "../../models/blockArt";
import {
  buildLevelJob,
  chosenFrom,
  DYNAMIC_SPREAD_LU,
  optionToRunItem,
  runItemToOption,
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
}

// A `clamp_reason` is set ONLY when the leveled signal isn't effectively reaching the USB 1/2
// capture — a silent capture for preset/footswitch jobs (output not routed to USB), or the
// scene path's no-authority case (a big amp-outputLevel change doesn't move the capture:
// off-branch) → its own `offbranch` outcome ("not on USB 1/2"), which a re-level can't fix.
// A plain headroom/authority clamp (the knob has real effect but can't reach target) has
// `clamped` set with NO reason → "clamped at X".
const outcomeOf = (r: LevelOutcomeFields): RunItem["outcome"] =>
  r.clamp_reason != null ? "offbranch" : r.clamped ? "clamped" : "done";
const valueOf = (r: LevelOutcomeFields): number =>
  r.verify_lufs ?? r.predicted_lufs;
// Resolve to a SINGLE by-ear cause. If a row is both dynamic AND rebalance-uncertain (rare —
// rebalance is opt-in), "dynamic" wins INTENTIONALLY: it's the primary, more common ear-check
// signal. Each row resolves to one cause; the summary footnote counts whichever each resolved to.
const byEarCause = (r: LevelOutcomeFields): RunItem["verifyByEar"] =>
  (r.dynamic_spread_lu ?? 0) >= DYNAMIC_SPREAD_LU
    ? "dynamic"
    : r.verify_by_ear
      ? "rebalance"
      : undefined;

/** The run's live state, published by the run loop and read by RunBody/SummaryBody. */
export interface RunState {
  items: RunItem[];
  currentIndex: number;
  total: number;
  done: boolean;
  stopped: boolean;
  /** A stop was requested and the in-flight item is still winding down. Drives the
   *  "Stopping…" feedback so the user isn't left staring at a "leveling…" spinner. */
  stopping: boolean;
}

const EMPTY_RUN: RunState = {
  items: [],
  currentIndex: 0,
  total: 0,
  done: false,
  stopped: false,
  stopping: false,
};

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
  targetLufsByName,
  deselectKeys,
  refresh,
}: UseLevelingFlowDeps) {
  // Wizard stage + the per-stage inputs. The frame stays mounted across stages; only
  // the body swaps. `stage === "closed"` ⇒ the wizard is unmounted.
  const [stage, setStage] = useState<Stage>("closed");
  const [chosen, setChosen] = useState<SetupOption[]>([]);
  const [flowPresetCount, setFlowPresetCount] = useState(0);
  const [isRelevel, setIsRelevel] = useState(false);
  const [run, setRun] = useState<RunState>(EMPTY_RUN);
  // Advisory live measured loudness for the active run row's "measuring…" readout. Held
  // OUTSIDE `RunState` on purpose: the run loop's `publish()` rebuilds RunState on every
  // transition and would clobber these async event updates. The backend only emits during a
  // capture, so this stays null outside a run; reset to null on each item resolve + run end.
  const [liveLufs, setLiveLufs] = useState<number | null>(null);

  // Subscribe once for the hook's lifetime (inert off-Tauri). The backend streams
  // `tmp://leveling-lufs` only while a leveling capture runs, so no extra gating is needed.
  useEffect(() => {
    const unlisten = onLevelingLufs((e) => {
      setLiveLufs(e.lufs);
    });
    return () => {
      void unlisten.then((u) => {
        u();
      });
    };
  }, []);

  const runningRef = useRef(false);
  const cancelRef = useRef(false);
  // BUG-4: once a run has leveled at least one sound, closing the wizard (Done OR Cancel)
  // must DESELECT exactly the keys it leveled — and ACCUMULATE them across re-level rounds
  // (each re-level run adds its keys here), so a Re-level→Cancel can't leave the whole
  // selection ticked and silently re-run everything next time. A Cancel BEFORE any run
  // leaves these empty, preserving the selection.
  const didRunRef = useRef(false);
  const ranKeysRef = useRef<Set<string>>(new Set());
  // Opt-in: equalize a path-MERGE scene's two parallel-amp lanes before joint-k. A no-op
  // on series / single-amp / split-output scenes (the backend only rebalances scenes it
  // classifies as mergeable). Held in a REF so toggling it never re-renders the flow;
  // SetupBody owns the visible pill and pushes its value here via `setRebalance` — incl.
  // on mount, so a remount (re-level / Back→Continue / new flow) resets the ref to the
  // freshly-defaulted (off) pill rather than leaking a stale ON. Read at run time.
  const rebalanceRef = useRef(false);

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
      setChosen(options);
      setIsRelevel(false);
      setFlowPresetCount(new Set(options.map((o) => o.slot)).size);
      setStage("setup");
    },
    [rows, sceneInfo, footswitchInfo],
  );

  // Close the wizard, deselecting exactly what a run leveled (BUG-4). Reset the run
  // bookkeeping so the next flow starts clean. A close with no run preserves selection.
  const closeFlow = useCallback(() => {
    setStage("closed");
    if (didRunRef.current) deselectKeys([...ranKeysRef.current]);
    didRunRef.current = false;
    ranKeysRef.current = new Set();
  }, [deselectKeys]);

  const onCancel = closeFlow;

  // Drive the run sequentially: one chosen scene at a time, mark active → result.
  const runLeveling = useCallback(
    async (items: RunItem[]) => {
      if (runningRef.current) return;
      runningRef.current = true;
      cancelRef.current = false;
      didRunRef.current = true;
      // Read the cancel flag through a getter so it isn't narrowed to its
      // last-assigned literal across awaits — the cancel handler flips it meanwhile.
      const isCancelled = () => cancelRef.current;
      const candCache = new Map<number, Candidate[]>();
      const work = items.map((it) => ({ ...it }));
      // Base-first within each preset: a preset's Base levels `presetLevel` — a global
      // multiplier over its scenes — so it MUST run before its FS scenes, else the base
      // write shifts every already-leveled scene off-target. `chosenFrom` already emits
      // this order; this stable sort (0 for differing slots ⇒ input order preserved)
      // guarantees it regardless of how `items` was assembled.
      const baseRank = (it: RunItem) => (it.isBase ? 0 : 1);
      work.sort((a, b) => (a.slot === b.slot ? baseRank(a) - baseRank(b) : 0));
      const total = work.length;

      // `work` is mutated in place between publishes; pass a fresh ARRAY each time
      // (new ref so React renders) but skip the per-item spread — the bodies only read
      // items during render, never hold them across renders.
      const publish = (
        currentIndex: number,
        done: boolean,
        stopped: boolean,
      ) => {
        // Once cancel is requested, every publish carries `stopping` until the final
        // done publish clears it (done ⇒ either "complete" or "stopped").
        setRun({
          items: [...work],
          currentIndex,
          total,
          done,
          stopped,
          stopping: isCancelled() && !done,
        });
      };

      setStage("run");
      publish(0, false, false);

      for (let i = 0; i < total; i++) {
        if (isCancelled()) break;
        const it = work[i];
        it.status = "active";
        publish(i, false, false);
        const profile = profileById(it.instId);
        const targetLufs = targetLufsByName(it.targetName);
        // Envelope-follower presets get the "envelope" cause over any result-derived
        // one: the effect tracks the stimulus envelope, so the measurement itself is
        // suspect no matter how clean the numbers look.
        const envelope = (blocksByIndex.get(it.slot) ?? []).some((id) =>
          ENVELOPE_BIDS.has(id),
        );
        const causeOf = (r: LevelOutcomeFields): RunItem["verifyByEar"] =>
          envelope ? "envelope" : byEarCause(r);
        try {
          if (it.isBase) {
            const res = await levelPreset(
              buildLevelJob(it.slot, targetLufs, profile, true),
            );
            it.outcome = outcomeOf(res);
            it.value = valueOf(res);
            it.spreadLu = res.dynamic_spread_lu;
            it.verifyByEar = causeOf(res);
          } else if (it.footswitch != null) {
            // A block-acting FOOTSWITCH — level its engaged state so stomping it lands
            // on target. One job per call (one footswitch at a time, like a scene); the
            // backend classifies bake vs assign and decides whether to overwrite the
            // block or write a footswitch param-change. `method` is intentionally not
            // surfaced — the user only ever sees "leveled".
            const fsw = it.footswitch;
            const results = await levelFootswitchesApply(
              {
                slot: it.slot,
                jobs: [
                  {
                    switch: fsw.switchIndex,
                    levGroupId: fsw.levGroupId,
                    levNodeId: fsw.levNodeId,
                    levParameterId: fsw.levParameterId,
                    targetLufs,
                  },
                ],
                save: true,
                topologyId: profile?.topology_id ?? null,
                calibrationLufs: profile?.calibration_lufs ?? null,
              },
              () => {
                /* single-job call — the returned result is enough */
              },
            );
            if (results.length === 0) {
              it.outcome = "skipped";
            } else {
              const r = results[0];
              it.outcome = outcomeOf(r);
              it.value = valueOf(r);
              it.spreadLu = r.dynamic_spread_lu;
              it.verifyByEar = causeOf(r);
            }
          } else {
            let cands = candCache.get(it.slot);
            if (!cands) {
              // Amp candidates come from the startup backup (no live discovery
              // round-trip). Fall back to a live block read only if the backup
              // missed this preset (rare — keeps a stray preset from silently
              // skipping its scenes).
              cands =
                ampCandidates.get(it.slot) ??
                ampBlocks(await listLevelBlocks(it.slot));
              candCache.set(it.slot, cands);
            }
            if (cands.length === 0) {
              it.outcome = "skipped";
            } else {
              const results = await levelScenesApplyBatched(
                {
                  slot: it.slot,
                  sceneSlots: it.sceneSlot == null ? [] : [it.sceneSlot],
                  candidates: cands,
                  targetLufs,
                  save: true,
                  rebalance: rebalanceRef.current,
                  topologyId: profile?.topology_id ?? null,
                  calibrationLufs: profile?.calibration_lufs ?? null,
                },
                () => {
                  /* no per-scene progress callback */
                },
              );
              if (results.length === 0) {
                it.outcome = "skipped";
              } else {
                const r = results[0];
                it.outcome = outcomeOf(r);
                it.value = valueOf(r);
                it.spreadLu = r.dynamic_spread_lu;
                it.verifyByEar = causeOf(r);
              }
            }
          }
        } catch {
          // One scene's failure shouldn't abort the run — flag it skipped.
          it.outcome = "skipped";
        }
        it.status = "result";
        // This sound was actually leveled — remember its key so closing the wizard
        // deselects it (BUG-4). A stopped run never reaches the un-run items, so their
        // keys stay selected for a follow-up run.
        ranKeysRef.current.add(it.key);
        // Drop the live readout the instant the row resolves; the result row's value is the
        // confirm. The next active item re-populates it on its first capture event.
        setLiveLufs(null);
        publish(i + 1, false, false);
      }

      // A Stop pressed during the LAST item never trips the top-of-loop check (that item
      // was already in flight), so snapshot the cancel flag the instant the loop exits —
      // BEFORE refresh — to report it as "stopped". Snapshotting pre-refresh also avoids a
      // false positive: a Stop pressed during refresh() (the run already finished) is
      // ignored, so a fully-completed run still auto-advances instead of mislabeling.
      const stopped = isCancelled();
      runningRef.current = false;
      try {
        await refresh();
      } catch {
        /* re-read is best-effort */
      }
      publish(total, true, stopped);
    },
    [profileById, targetLufsByName, refresh, ampCandidates, blocksByIndex],
  );

  // Set-up "Level N sounds" (the COMMIT) → build run items and start. The backup
  // acknowledgment gated this button inline, so this goes straight to the run.
  const onSetupStart = useCallback(
    (choices: SetupChoice[]) => {
      if (choices.length === 0) return;
      const items = choices.map((c) =>
        optionToRunItem(c.option, c.instId, c.targetName),
      );
      void runLeveling(items);
    },
    [runLeveling],
  );

  // Run "Stop" → halt the run in place; levels already written stay saved. Do NOT close
  // the wizard — closing it hid that the in-flight item was still finishing on the device
  // ("I don't know if it stopped"). Instead flip to "Stopping…" immediately; the in-flight
  // command finishes, the loop breaks on `cancelRef`, and `publish(total,true,stopped)`
  // shows "Leveling stopped" + Continue.
  const onRunCancel = useCallback(() => {
    cancelRef.current = true;
    // ponytail: cancel all three lanes — the in-flight item is a base preset, a scene, OR
    // a footswitch, and cancelling the idle lanes is a harmless no-op. The backend bails at
    // its seams (skipping apply+save); the ~6 s capture still finishes, so "Stopping…"
    // reflects the current item winding down.
    void cancelPresetLeveling();
    void cancelSceneLeveling();
    void cancelFootswitchLeveling();
    setRun((r) => ({ ...r, stopping: true }));
  }, []);

  // Run → Summary (auto on a natural finish, or via Continue after a manual stop).
  const onRunComplete = useCallback(() => {
    setStage("summary");
  }, []);

  // Summary "Re-level clamped…" → re-enter at Set up with just the clamped subset
  // (re-level mode: the Set-up body hides the backup acknowledgment — already given).
  const onRelevel = useCallback((clamped: RunItem[]) => {
    const options: SetupOption[] = clamped.map(runItemToOption);
    setChosen(options);
    setIsRelevel(true);
    setFlowPresetCount(new Set(clamped.map((it) => it.slot)).size);
    setStage("setup");
  }, []);

  // Summary "Accept" / "Done" → close, deselecting just the leveled sounds.
  const onAccept = closeFlow;

  // Toggle the opt-in rebalance (read at run time). Stored in a ref so toggling it
  // doesn't re-render the flow; the SetupBody owns its own checkbox state.
  const setRebalance = useCallback((on: boolean) => {
    rebalanceRef.current = on;
  }, []);

  return {
    stage,
    chosen,
    flowPresetCount,
    isRelevel,
    run,
    liveLufs,
    openFlow,
    onCancel,
    onSetupStart,
    onRunCancel,
    onRunComplete,
    onRelevel,
    onAccept,
    setRebalance,
  };
}
