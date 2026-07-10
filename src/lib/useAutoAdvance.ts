// src/lib/useAutoAdvance.ts — the run wizards' "auto-advance to the summary".
//
// Both the Doctor and Leveling run bodies jump to their summary step a short beat
// after the run finishes ON ITS OWN. A manual stop suppresses it (the user gets a
// Continue button instead). `onAdvance` is read through a ref so a fresh callback
// identity from the parent doesn't reset the in-flight timer; the timer is cleared
// on unmount or when done/stopped change.

import { useEffect, useRef } from "react";

/** @param done      run has finished (naturally or via stop)
 *  @param stopped   the finish was a manual stop ⇒ do NOT auto-advance
 *  @param onAdvance advance to the summary step
 *  @param delayMs   beat before advancing (default 650) */
export function useAutoAdvance(
  done: boolean,
  stopped: boolean,
  onAdvance: () => void,
  delayMs = 650,
): void {
  const onAdvanceRef = useRef(onAdvance);
  useEffect(() => {
    onAdvanceRef.current = onAdvance;
  });
  useEffect(() => {
    if (done && !stopped) {
      const id = window.setTimeout(() => {
        onAdvanceRef.current();
      }, delayMs);
      return () => {
        window.clearTimeout(id);
      };
    }
  }, [done, stopped, delayMs]);
}
