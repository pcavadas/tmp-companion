// src/views/doctor/useDoctorFlow.ts — the Doctor RUN orchestrator.
//
// Owns the run's live state (one streamed progress row per sound, keyed by the
// selection key) and the per-sound-deterministic DoctorCheckResult that rides
// the command's return value. DoctorView owns the stage machine; this hook only
// drives the single `doctor_check` call and exposes `buildItems` (turns the
// list's chosen SetupOptions + the setup step's per-row instrument into the
// wire DoctorInputArgs). Mirrors useLevelingFlow's structure, minus the
// per-item command composition — Doctor is ONE backend command for the whole run.

import { useCallback, useEffect, useRef, useState } from "react";

import { doctorCheck, cancelDoctorCheck } from "../../lib/invoke";
import type { SetupOption } from "../level/leveling";
import type { RailStep } from "../overlays/wizardContext";
import type {
  DoctorInputArg,
  DoctorCheckResult,
  Store,
  ActiveGraph,
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

export interface DoctorRunState {
  /** The exact args sent to `doctor_check` (drives the run rows). */
  items: DoctorInputArg[];
  /** key → live status. */
  statusByKey: Record<string, DoctorRunStatus>;
  /** Sounds that reached a terminal status (done/error) — the progress index. */
  currentIndex: number;
  total: number;
  done: boolean;
  stopped: boolean;
}

const EMPTY_RUN: DoctorRunState = {
  items: [],
  statusByKey: {},
  currentIndex: 0,
  total: 0,
  done: false,
  stopped: false,
};

export interface UseDoctorFlowDeps {
  store: Store | null;
  /** Per-preset signal graph from the startup backup, keyed by 0-based list
   *  index — the source of each sound's `nodes` (chain passed verbatim so
   *  prescriptions target real blocks with no extra device read). */
  graphByIndex: Map<number, ActiveGraph>;
}

function countTerminal(
  items: DoctorInputArg[],
  statusByKey: Record<string, DoctorRunStatus>,
): number {
  return items.reduce((n, it) => {
    const s = statusByKey[it.key];
    return s === "done" || s === "error" ? n + 1 : n;
  }, 0);
}

export function useDoctorFlow({ store, graphByIndex }: UseDoctorFlowDeps) {
  const [run, setRun] = useState<DoctorRunState>(EMPTY_RUN);
  const [result, setResult] = useState<DoctorCheckResult | null>(null);
  // Set only when the whole `doctor_check` command REJECTS (an IPC/backend
  // failure) — distinct from a user Stop, which resolves normally with results.
  // Lets the results stage show a failure notice instead of a blank page.
  const [error, setError] = useState<string | null>(null);
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
        const instId = instByKey[o.key] ?? "none";
        const profile =
          instId === "none"
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
        };
      }),
    [store, graphByIndex],
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
      setResult(null);
      setError(null);
      const statusByKey: Record<string, DoctorRunStatus> = {};
      items.forEach((it) => (statusByKey[it.key] = "queued"));
      setRun({
        items,
        statusByKey,
        currentIndex: 0,
        total: items.length,
        done: false,
        stopped: false,
      });

      void doctorCheck(items, restoreListIndex, (p) => {
        setRun((prev) => {
          const nextStatus = { ...prev.statusByKey, [p.key]: p.status };
          return {
            ...prev,
            statusByKey: nextStatus,
            currentIndex: countTerminal(prev.items, nextStatus),
          };
        });
      })
        .then((res) => {
          setResult(res);
          setRun((prev) => ({
            ...prev,
            done: true,
            stopped: res.stopped,
            currentIndex: prev.total,
          }));
        })
        .catch((e: unknown) => {
          // A whole-run failure ends the run (the rows keep their last streamed
          // status). Record the error so the results stage surfaces a failure
          // notice rather than looking like an intentional Stop with no results.
          setError(e instanceof Error ? e.message : String(e));
          setRun((prev) => ({ ...prev, done: true, stopped: true }));
        })
        .finally(() => {
          runningRef.current = false;
        });
    },
    [],
  );

  // Stop the in-flight check — already-checked sounds keep their results. The
  // backend resolves `doctor_check` with `stopped: true`, so the run lands
  // done+stopped through the same path.
  const stopRun = useCallback(() => {
    void cancelDoctorCheck();
  }, []);

  const reset = useCallback(() => {
    setRun(EMPTY_RUN);
    setResult(null);
    setError(null);
  }, []);

  return { run, result, error, buildItems, startRun, stopRun, reset };
}
