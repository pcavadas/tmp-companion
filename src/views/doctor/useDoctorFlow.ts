// src/views/doctor/useDoctorFlow.ts — the Doctor RUN orchestrator.
//
// Owns the run's live state (one streamed progress row per sound, keyed by the
// selection key) and the per-sound-deterministic DoctorCheckResult that rides
// the command's return value. DoctorView owns the stage machine; this hook only
// drives the single `doctor_check` call and exposes `buildItems` (turns the
// list's chosen SetupOptions + the setup step's per-row instrument into the
// wire DoctorInputArgs). Mirrors useLevelingFlow's structure, minus the
// per-item command composition — Doctor is ONE backend command for the whole run.

import { useCallback, useEffect, useMemo, useReducer, useRef } from "react";

import { doctorCheck, cancelDoctorCheck } from "../../lib/invoke";
import { errMsg } from "../../lib/format";
import { NO_INSTRUMENT, type SetupOption } from "../level/leveling";
import type { RailStep } from "../overlays/wizardContext";
import type {
  DoctorInputArg,
  DoctorCheckResult,
  Store,
  ActiveGraph,
  FootswitchInfo,
  NodeOverride,
} from "../../lib/types";

/** The Doctor wizard's 3-step rail, shared by the full-page Set up (current 0)
 *  and the modal Check run (current 1). Results is the last node (current 2). */
export const DOCTOR_STEPS: readonly RailStep[] = [
  { key: "setup", label: "Set up" },
  { key: "check", label: "Check" },
  { key: "results", label: "Results" },
];

/** Per-sound run status. `queued` is the local pre-stream state; the backend
 *  streams `active` → `done` (or `error`) per key. */
export type DoctorRunStatus = "queued" | "active" | "done" | "error";

/** The whole run's status — ONE value, so a failure can never read as a Stop:
 *  `stopping` = Stop requested, the in-flight sound still winding down;
 *  `done` = finished on its own; `stopped` = ended by the user's Stop;
 *  `failed` = the `doctor_check` command itself rejected. */
export type DoctorFlowStatus =
  "idle" | "running" | "stopping" | "done" | "stopped" | "failed";

export interface DoctorRunState {
  status: DoctorFlowStatus;
  /** The exact args sent to `doctor_check` (drives the run rows). */
  items: DoctorInputArg[];
  /** key → live status. */
  statusByKey: Record<string, DoctorRunStatus>;
  /** Set when the run resolved (its results) — null otherwise. */
  result: DoctorCheckResult | null;
  /** The rejection text when `status === "failed"`. */
  error: string | null;
  /** Bumped on every start — a retry remounts the run page's live timers. */
  attempt: number;
}

const EMPTY_RUN: DoctorRunState = {
  attempt: 0,
  status: "idle",
  items: [],
  statusByKey: {},
  result: null,
  error: null,
};

type RunAction =
  | { type: "start"; items: DoctorInputArg[] }
  | { type: "progress"; key: string; status: DoctorRunStatus }
  | { type: "stop" }
  | { type: "resolved"; result: DoctorCheckResult }
  | { type: "failed"; error: string }
  | { type: "reset" };

export function runReducer(
  state: DoctorRunState,
  action: RunAction,
): DoctorRunState {
  switch (action.type) {
    case "start": {
      const statusByKey: Record<string, DoctorRunStatus> = {};
      action.items.forEach((it) => (statusByKey[it.key] = "queued"));
      return {
        ...EMPTY_RUN,
        attempt: state.attempt + 1,
        status: "running",
        items: action.items,
        statusByKey,
      };
    }
    case "progress":
      return {
        ...state,
        statusByKey: { ...state.statusByKey, [action.key]: action.status },
      };
    case "stop":
      return state.status === "running"
        ? { ...state, status: "stopping" }
        : state;
    case "resolved":
      return {
        ...state,
        status: action.result.stopped ? "stopped" : "done",
        result: action.result,
      };
    case "failed":
      return { ...state, status: "failed", error: action.error };
    case "reset":
      return { ...EMPTY_RUN, attempt: state.attempt };
  }
}

/** The run has ended — on its own, by Stop, or by failure. */
export function runEnded(status: DoctorFlowStatus): boolean {
  return status === "done" || status === "stopped" || status === "failed";
}

/** Sounds that reached a terminal status (done/error) — the progress index. A
 *  run that resolved counts every sound (nothing is still in flight). */
export function countFinished(
  status: DoctorFlowStatus,
  items: DoctorInputArg[],
  statusByKey: Record<string, DoctorRunStatus>,
): number {
  if (status === "done" || status === "stopped") return items.length;
  return items.reduce((n, it) => {
    const s = statusByKey[it.key];
    return s === "done" || s === "error" ? n + 1 : n;
  }, 0);
}

export interface UseDoctorFlowDeps {
  store: Store | null;
  /** Per-preset signal graph from the startup backup, keyed by 0-based list
   *  index — the source of each sound's `nodes` (chain passed verbatim so
   *  prescriptions target real blocks with no extra device read). */
  graphByIndex: Map<number, ActiveGraph>;
  /** Per-preset block-acting footswitches from the SAME startup backup, keyed
   *  by 0-based list index — the source of each sound's `footswitches` (drives
   *  the backend's OFFLINE force-bypass isolation derivation, no device read). */
  footswitchesByIndex: Map<number, FootswitchInfo[]>;
  /** Per-preset scene overlays (indexed by scene wire index) from the SAME startup
   *  backup, keyed by 0-based list index — a scene sound's diagnosed graph. */
  sceneOverridesByIndex: Map<number, NodeOverride[][]>;
}

/** One scene's overrides out of a preset's per-scene list — empty for a base or
 *  footswitch sound, and for a scene the scan has no overlay for. */
function sceneOverridesFor(
  perScene: NodeOverride[][] | undefined,
  sceneSlot: number | null,
): NodeOverride[] {
  if (perScene == null || sceneSlot == null || sceneSlot >= perScene.length) {
    return [];
  }
  return perScene[sceneSlot];
}

export function useDoctorFlow({
  store,
  graphByIndex,
  footswitchesByIndex,
  sceneOverridesByIndex,
}: UseDoctorFlowDeps) {
  const [run, dispatch] = useReducer(runReducer, EMPTY_RUN);
  const runningRef = useRef(false);

  // Turn each chosen list row into a wire DoctorInputArg. The scene wire index is
  // taken verbatim from `SetupOption.sceneSlot` — the SAME 0-based `scenes[]`
  // index leveling's `chosenFrom` assigns (base/whole-preset/footswitch → null,
  // an FS scene → its row index). Instrument → topology + calibration off the
  // chosen profile ("none" → null/null); nodes off the backup graph.
  const buildItems = useCallback(
    (
      chosen: SetupOption[],
      instByKey: Record<string, string>,
    ): DoctorInputArg[] =>
      chosen.map((o) => {
        const instId = instByKey[o.key] ?? NO_INSTRUMENT;
        const profile =
          instId === NO_INSTRUMENT
            ? null
            : (store?.profiles.find((p) => p.id === instId) ?? null);
        const label = o.isBase
          ? o.hasScenes
            ? `${o.presetName} · Base`
            : o.presetName
          : `${o.presetName} · ${o.sceneName}`;
        return {
          key: o.key,
          listIndex: o.slot,
          scene: o.sceneSlot,
          footswitch: o.footswitch?.switchIndex ?? null,
          label,
          tag: o.tag,
          topologyId: profile?.topology_id ?? null,
          calibrationLufs: profile?.calibration_lufs ?? null,
          // The chosen profile's id (null when "none") — the backend picks up its
          // Tier-2 DI capture as the verbatim stimulus + CAPTURE diagnosis space.
          profileId: profile?.id ?? null,
          nodes: graphByIndex.get(o.slot)?.nodes ?? [],
          footswitches: footswitchesByIndex.get(o.slot) ?? [],
          sceneOverrides: sceneOverridesFor(
            sceneOverridesByIndex.get(o.slot),
            o.sceneSlot,
          ),
        };
      }),
    [store, graphByIndex, footswitchesByIndex, sceneOverridesByIndex],
  );

  // Unmounting mid-run (a tab switch) would orphan the backend check — fire the
  // cooperative cancel so it winds down (the in-flight ~9 s capture still
  // finishes, then the run resolves with nobody listening, which is fine).
  useEffect(
    () => () => {
      if (runningRef.current) {
        void cancelDoctorCheck().catch(() => undefined);
      }
    },
    [],
  );

  // Fire the ONE backend command for the whole run. Progress rows update per key
  // as the stream lands; the per-sound target-deviation diagnoses ride the resolved
  // value (each verdict depends only on that sound, never on which others ran).
  // `restoreListIndex` = the pre-run active preset, reloaded when the run ends.
  const startRun = useCallback(
    (items: DoctorInputArg[], restoreListIndex: number | null) => {
      if (runningRef.current) return;
      runningRef.current = true;
      dispatch({ type: "start", items });

      void doctorCheck(items, restoreListIndex, (p) => {
        dispatch({ type: "progress", key: p.key, status: p.status });
      })
        .then((result) => {
          dispatch({ type: "resolved", result });
        })
        .catch((e: unknown) => {
          // The whole command rejected (an IPC/backend failure) — its own
          // status, never a Stop: the run page says it failed and offers a retry.
          dispatch({ type: "failed", error: errMsg(e) });
        })
        .finally(() => {
          runningRef.current = false;
        });
    },
    [],
  );

  // Stop the in-flight check — already-checked sounds keep their results. The
  // backend resolves `doctor_check` with `stopped: true`, so the run lands
  // `stopped` through the same path. `stopping` flips immediately so the UI can
  // acknowledge the request before the in-flight ~9s capture winds down.
  const stopRun = useCallback(() => {
    dispatch({ type: "stop" });
    void cancelDoctorCheck().catch(() => undefined);
  }, []);

  const reset = useCallback(() => {
    dispatch({ type: "reset" });
  }, []);

  return useMemo(
    () => ({ run, buildItems, startRun, stopRun, reset }),
    [run, buildItems, startRun, stopRun, reset],
  );
}
