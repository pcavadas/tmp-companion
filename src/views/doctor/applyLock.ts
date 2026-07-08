// src/views/doctor/applyLock.ts — the "one applied-but-unsaved prescription at
// a time" lock.
//
// The DEVICE has a single edit buffer, so EVERY PrescriptionCard on the results
// page — even ones in different presets — targets the same live, unsaved state:
// a second card's apply/discard would clobber the first card's edit while it
// still shows "Applied · not saved" with stale A/B clips. This lock lets a card
// disable its Apply button while any other card holds an unsaved edit. Provided
// ONCE by DoctorResults (the page); a dedicated module (not DoctorResults) so
// PrescriptionCard can import it without a cycle.

import { createContext, useContext } from "react";

/** The card holding an applied-but-unsaved edit + the preset it lives in (so
 *  the owner can discard the device's edit buffer on leave). */
export interface ActiveApplyCard {
  id: string;
  listIndex: number;
}

export interface ApplyLock {
  /** The card currently holding an applied-but-unsaved edit, or null. */
  activeCard: ActiveApplyCard | null;
  acquire: (id: string, listIndex: number) => void;
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
