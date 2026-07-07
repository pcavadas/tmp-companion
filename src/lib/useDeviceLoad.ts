// src/lib/useDeviceLoad.ts — shared read-only load state machine for the
// device-backed views (Presets, Songs). Both used to declare an identical
// `LoadPhase` union + the same `loading → ready | error` refresh dance + a
// mounted guard; that pattern lives here once.

import { useCallback, useEffect, useRef, useState } from "react";

import { errMsg } from "./format";

export type LoadPhase =
  { kind: "loading" } | { kind: "error"; message: string } | { kind: "ready" };

/**
 * Owns the load `phase` + a mounted guard so a fetch that resolves after unmount
 * can't `setState`. `runLoad(body)` keeps a prior "ready" view during a refresh
 * (no flash back to the skeleton), runs `body` (which does the actual fetching and
 * sets the view's OWN data state — guarding with the returned `mountedRef`), then
 * lands the phase on `ready` (success) or `error` (throw). Returns `setPhase` too
 * for the rare callers that drive the phase directly (e.g. a write that fails).
 */
export function useDeviceLoad() {
  const [phase, setPhase] = useState<LoadPhase>({ kind: "loading" });
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const runLoad = useCallback(async (body: () => Promise<void>) => {
    setPhase((p) => (p.kind === "ready" ? p : { kind: "loading" }));
    try {
      await body();
      if (mountedRef.current) setPhase({ kind: "ready" });
    } catch (e) {
      if (mountedRef.current) setPhase({ kind: "error", message: errMsg(e) });
    }
  }, []);

  return { phase, setPhase, mountedRef, runLoad };
}
