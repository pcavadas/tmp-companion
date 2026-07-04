// src/views/doctor/applyLock.ts — a per-preset "one applied-but-unsaved
// prescription at a time" lock.
//
// Every PrescriptionCard in a preset targets the SAME device edit buffer (same
// listIndex), so a second card's apply/discard would clobber the first card's
// live, unsaved edit while it still shows "Applied · not saved" with stale A/B
// clips. This lock lets a card disable its Apply button while a sibling holds an
// unsaved edit. Provided per preset by PresetResultCard; a dedicated module (not
// PresetResultCard) so PrescriptionCard can import it without a cycle.

import { createContext, useContext } from "react";

export interface ApplyLock {
  /** The card id currently holding an applied-but-unsaved edit, or null. */
  activeCard: string | null;
  acquire: (id: string) => void;
  /** Release the lock IFF `id` currently holds it (a stale release is a no-op). */
  release: (id: string) => void;
}

const NOOP: ApplyLock = {
  activeCard: null,
  acquire: () => {
    /* no provider: unguarded */
  },
  release: () => {
    /* no provider: unguarded */
  },
};

/** Default is a no-op lock, so a PrescriptionCard rendered outside a provider
 *  (e.g. in isolation in a test) still works — it just isn't sibling-guarded. */
export const ApplyLockContext = createContext<ApplyLock>(NOOP);

export const useApplyLock = (): ApplyLock => useContext(ApplyLockContext);
