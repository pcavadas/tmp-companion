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

import { useCallback, useEffect, useRef, useState } from "react";

import {
  levelPreset,
  cancelPresetLeveling,
  listLevelBlocks,
  levelScenesApplyBatched,
  cancelSceneLeveling,
  levelFootswitchesApply,
  cancelFootswitchLeveling,
  redistributeHeadroom,
  restoreRedistribution,
  type PreviousKnob,
} from "../../lib/invoke";
import { onLevelingLufs } from "../../lib/liveEvents";
import { MODELS } from "../../models/catalog";
import { resolveDeviceId } from "../../models/blockArt";
import {
  BASE_SCENE_SLOT,
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

/** A run row that levels via amp `outputLevel` in scene mode — not Base (`presetLevel`), not
 *  a block-acting footswitch. The run loop batches these; redistribution compensates them. */
const isSceneItem = (it: RunItem): boolean =>
  !it.isBase && it.footswitch == null && it.sceneSlot != null;

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
  // Rolling per-hop momentary levels (dB) for the decorative live VU bars — last 24 hops.
  const [liveTrace, setLiveTrace] = useState<number[]>([]);

  // Subscribe once for the hook's lifetime (inert off-Tauri). The backend streams
  // `tmp://leveling-lufs` only while a leveling capture runs, so no extra gating is needed.
  useEffect(() => {
    const unlisten = onLevelingLufs((e) => {
      setLiveLufs(e.lufs);
      setLiveTrace((prev) => [...prev, e.momentary].slice(-24));
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
        ranKeysRef.current.add(item.key);
        setLiveLufs(null);
        setLiveTrace([]);
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
        (key: K, status: string, result: LevelOutcomeFields | null) => {
          const entry = entries.get(key);
          if (!entry) return;
          if (status === "active") {
            entry.item.status = "active";
            publish(entry.idx, false, false);
          } else if (status === "done" && result) {
            entry.item.outcome = outcomeOf(result);
            entry.item.value = valueOf(result);
            entry.item.spreadLu = result.dynamic_spread_lu;
            entry.item.verifyByEar = causeOf(result);
            finishItem(entry.item, entry.idx);
          } else if (status === "error") {
            entry.item.outcome = "skipped";
            finishItem(entry.item, entry.idx);
          }
        };
      const sweepUnresolved = <K>(entries: Map<K, BatchEntry>) => {
        if (isCancelled()) return;
        for (const entry of entries.values()) {
          if (entry.item.status !== "result") {
            entry.item.outcome = "skipped";
            finishItem(entry.item, entry.idx);
          }
        }
      };

      for (let i = 0; i < total;) {
        if (isCancelled()) break;
        const it = work[i];
        const profile = profileById(it.instId);
        const targetLufs = targetLufsByName(it.targetName);
        const causeOf = causeFor(it.slot);

        if (it.isBase || (it.footswitch == null && it.sceneSlot == null)) {
          it.status = "active";
          publish(i, false, false);
          try {
            if (it.isBase) {
              const res = await levelPreset(
                buildLevelJob(it.slot, targetLufs, profile, true),
              );
              it.outcome = outcomeOf(res);
              it.value = valueOf(res);
              it.spreadLu = res.dynamic_spread_lu;
              it.previousLevel = res.previous_level;
              it.truePeakDbtp = res.true_peak_dbtp;
              it.verifyByEar = causeOf(res);
            } else {
              // A scene item with no wire slot — nothing to level.
              it.outcome = "skipped";
            }
          } catch {
            // One sound's failure shouldn't abort the run — flag it skipped.
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
          try {
            await levelFootswitchesApply(
              {
                slot: it.slot,
                jobs: group.flatMap((g) =>
                  g.footswitch != null
                    ? [
                        {
                          switch: g.footswitch.switchIndex,
                          levGroupId: g.footswitch.levGroupId,
                          levNodeId: g.footswitch.levNodeId,
                          levParameterId: g.footswitch.levParameterId,
                          targetLufs: targetLufsByName(g.targetName),
                        },
                      ]
                    : [],
                ),
                save: true,
                topologyId: profile?.topology_id ?? null,
                calibrationLufs: profile?.calibration_lufs ?? null,
                profileId: profile?.id ?? null,
              },
              (item) => {
                resolveFs(item.switch, item.status, item.result);
              },
            );
          } catch {
            /* whole-group failure — the sweep flags unresolved rows skipped */
          }
          sweepUnresolved(bySwitch);
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
          // skipping its scenes).
          cands =
            ampCandidates.get(it.slot) ??
            ampBlocks(await listLevelBlocks(it.slot));
          candCache.set(it.slot, cands);
        }
        if (cands.length === 0) {
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
        // ponytail: per-scene outcomes arrive via the Channel (`onResult`), NOT the returned
        // Promise value (deliberately discarded — the returned LevelResult[] carries no scene_slot,
        // so it can't be reconciled by scene without a backend contract change). Consequence: the
        // offline e2e HTTP bridge no-ops the Channel, so scene rows there resolve to "skipped"
        // (their physics is gated at the command level instead — see level-defaults.spec.ts). If a
        // dropped-stream-item-online hardening is ever needed, add scene_slot to the return + reconcile here.
        try {
          await levelScenesApplyBatched(
            {
              slot: it.slot,
              jobs: group.map((g) => ({
                sceneSlot: g.sceneSlot ?? 0,
                targetLufs: targetLufsByName(g.targetName),
              })),
              candidates: cands,
              save: true,
              rebalance: rebalanceRef.current,
              topologyId: profile?.topology_id ?? null,
              calibrationLufs: profile?.calibration_lufs ?? null,
              profileId: profile?.id ?? null,
            },
            (item) => {
              resolveScene(item.sceneSlot, item.status, item.result);
            },
          );
        } catch {
          /* whole-group failure — the sweep flags unresolved rows skipped */
        }
        sweepUnresolved(byScene);
        i = end;
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
      const items = choices.map((c) => ({
        ...optionToRunItem(c.option, c.instId, c.targetName),
        silenceHint: silenceHintByIndex.get(c.option.slot),
      }));
      void runLeveling(items);
    },
    [runLeveling, silenceHintByIndex],
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

  // ── Gain-budget redistribution (loud-preset clamp class, single-amp v1) ──────────────
  // Per-preset recorded pre-redistribution values (the one-click Restore anchor), keyed by
  // 0-based slot.
  const [redistUndo, setRedistUndo] = useState<
    Map<number, { presetLevel: number; knobs: PreviousKnob[]; name: string }>
  >(new Map());

  // Which presets in a finished run can be redistributed: a SINGLE-amp preset whose Base was
  // leveled and did NOT clamp (presetLevel < 1.0 ⇒ headroom) with ≥1 headroom-clamped scene.
  // Multi-amp presets are excluded in v1 (compensating one amp would drift the others).
  const redistributablePresets = useCallback(
    (items: RunItem[]): number[] =>
      [...new Set(items.map((it) => it.slot))].filter((slot) => {
        const group = items.filter((it) => it.slot === slot);
        const base = group.find((it) => it.isBase);
        const clamps = group.filter(
          (it) => isSceneItem(it) && it.outcome === "clamped",
        );
        const nodes = new Set(
          (ampCandidates.get(slot) ?? [])
            .filter((a) => a.parameterId === "outputLevel")
            .map((a) => a.nodeId),
        );
        return (
          base?.outcome === "done" && clamps.length > 0 && nodes.size === 1
        );
      }),
    [ampCandidates],
  );

  // What a redistribution would rewrite (for the Summary's opt-in enumeration), or null when
  // it doesn't apply. `scenes` counts the FS scenes compensated across the affected presets;
  // the base amp + presetLevel of each are always rewritten too.
  const redistributePlan = useCallback(
    (items: RunItem[]): { presets: number; scenes: number } | null => {
      const slots = redistributablePresets(items);
      if (slots.length === 0) return null;
      const scenes = items.filter(
        (it) => slots.includes(it.slot) && isSceneItem(it),
      ).length;
      return { presets: slots.length, scenes };
    },
    [redistributablePresets],
  );

  // Summary "Give clamped scenes headroom" → for each redistributable preset, raise
  // presetLevel and re-level the base amp + every scene back to target (the backend streams
  // per-sound progress; the run stage shows it). Records the pre-values for Restore.
  const redistribute = useCallback(
    async (items: RunItem[]) => {
      const slots = redistributablePresets(items);
      if (slots.length === 0) return;
      const work = items.map((it) => ({ ...it }));
      const total = work.length;
      setStage("run");
      const publish = (idx: number, done: boolean) => {
        setRun({
          items: [...work],
          currentIndex: idx,
          total,
          done,
          stopped: false,
          stopping: false,
        });
      };
      publish(0, false);
      const newUndo: [
        number,
        { presetLevel: number; knobs: PreviousKnob[]; name: string },
      ][] = [];
      for (const slot of slots) {
        const group = work.filter((it) => it.slot === slot);
        const base = group.find((it) => it.isBase);
        if (!base) continue;
        const scenes = group.filter(isSceneItem);
        const profile = profileById(base.instId);
        const bySound = new Map<number, RunItem>([[BASE_SCENE_SLOT, base]]);
        for (const s of scenes)
          if (s.sceneSlot != null) bySound.set(s.sceneSlot, s);
        const jobs = [...bySound].map(([sceneSlot, it]) => ({
          sceneSlot,
          targetLufs: targetLufsByName(it.targetName),
        }));
        const worst = Math.max(
          ...scenes
            .filter((s) => s.outcome === "clamped")
            .map(
              (s) => targetLufsByName(s.targetName) - (s.value ?? -Infinity),
            ),
        );
        try {
          const res = await redistributeHeadroom(
            {
              slot,
              jobs,
              candidates: ampCandidates.get(slot) ?? [],
              worstClampedDeficitDb: worst,
              topologyId: profile?.topology_id ?? null,
              calibrationLufs: profile?.calibration_lufs ?? null,
              profileId: profile?.id ?? null,
            },
            (item) => {
              const target = bySound.get(item.sceneSlot);
              if (!target) return;
              if (item.status === "active") {
                target.status = "active";
              } else if (item.status === "done" && item.result) {
                target.outcome = outcomeOf(item.result);
                target.value = valueOf(item.result);
                target.status = "result";
              } else if (item.status === "error") {
                target.outcome = "skipped";
                target.status = "result";
              }
              publish(work.indexOf(target), false);
            },
          );
          newUndo.push([
            slot,
            {
              presetLevel: res.previousPresetLevel,
              knobs: res.previousKnobs,
              name: base.presetName,
            },
          ]);
        } catch {
          // Redistribution aborted (a compensating write didn't land): nothing persisted for
          // this preset — leave its rows as the run left them.
        }
      }
      setRedistUndo((m) => new Map([...m, ...newUndo]));
      publish(total, true);
      await refresh();
      setStage("summary");
    },
    [
      redistributablePresets,
      profileById,
      targetLufsByName,
      ampCandidates,
      refresh,
    ],
  );

  // Undo every redistribution this Summary applied (writes the recorded pre-values back).
  const undoRedistribute = useCallback(async () => {
    const entries = [...redistUndo];
    for (const [slot, rec] of entries) {
      try {
        await restoreRedistribution(slot, rec.presetLevel, rec.knobs, rec.name);
      } catch {
        /* a drifted slot fails the name guard — leave it, the user is told */
      }
    }
    setRedistUndo(new Map());
    await refresh();
  }, [redistUndo, refresh]);

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
    liveTrace,
    openFlow,
    onCancel,
    onSetupStart,
    onRunCancel,
    onRunComplete,
    onRelevel,
    onAccept,
    setRebalance,
    // Gain-budget redistribution (loud-preset clamp class, single-amp v1).
    redistribution: {
      plan: redistributePlan,
      run: redistribute,
      undoCount: redistUndo.size,
      undo: undoRedistribute,
    },
  };
}
