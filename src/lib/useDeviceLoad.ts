// src/lib/useDeviceLoad.ts — shared read-only load state machine for the
// device-backed views (Presets, Songs, Copy). Each used to declare an identical
// `LoadPhase` union + the same `loading → ready | error` refresh dance; that
// pattern lives here once.

import { useCallback, useEffect, useRef, useState } from "react";

import { errMsg } from "./format";

export type LoadPhase =
  { kind: "loading" } | { kind: "error"; message: string } | { kind: "ready" };

/**
 * Owns the load `phase`. `runLoad(body)` keeps a prior "ready" view during a
 * refresh (no flash back to the skeleton), runs `body` (which does the actual
 * fetching and sets the view's OWN data state), then lands the phase on `ready`
 * (success) or `error` (throw). Returns `setPhase` too for the rare callers that
 * drive the phase directly (e.g. a write that fails).
 *
 * Only the NEWEST load commits. Loads can overlap (the `connected` flag flipping
 * twice during a multi-second live read), and the last to SETTLE is not always the
 * last STARTED, so each run gets a generation and `body` receives `isCurrent()`:
 * it must check it after every await and before committing data. A superseded
 * run's result and its phase change are both dropped, so pre-reconnect data can't
 * overwrite fresh data and a late rejection can't flip a newer success to error.
 * Unmount retires the current run too.
 */
export function useDeviceLoad() {
  const [phase, setPhase] = useState<LoadPhase>({ kind: "loading" });
  const genRef = useRef(0);

  useEffect(
    () => () => {
      genRef.current += 1;
    },
    [],
  );

  const runLoad = useCallback(
    async (body: (isCurrent: () => boolean) => Promise<void>) => {
      genRef.current += 1;
      const gen = genRef.current;
      const isCurrent = () => gen === genRef.current;
      setPhase((p) => (p.kind === "ready" ? p : { kind: "loading" }));
      try {
        await body(isCurrent);
        if (isCurrent()) setPhase({ kind: "ready" });
      } catch (e) {
        if (isCurrent()) setPhase({ kind: "error", message: errMsg(e) });
      }
    },
    [],
  );

  return { phase, setPhase, runLoad };
}
