// src/views/level/useLazySlotCache.ts — the shared lazy per-slot fetch cache behind
// `useSceneHandles`, `useLevelBlocks` and `useFootswitchSceneContexts`. All three fetch a
// PER-PRESET list off a real device read that must fire lazily (on first open of the
// relevant row's picker, once per preset — the Set-up step otherwise does no device
// reads) and cache it for the rest of the mount. This hook carries that one cache shape;
// each caller stays a thin wrapper supplying its own `invoke` call and deriving its own
// (often secondary-keyed, e.g. by scene slot or switch index) result shape from the
// resolved list.
//
// NO SELF-HEAL: an "error" result for a slot is cached for the rest of THIS mount
// (`fetchedSlotsRef` marks the slot fetched even on failure) — re-opening the wizard (a
// fresh SetupBody mount) is the only retry path. Documented here because it's easy to
// assume a picker retry re-fetches.

import { useRef, useState } from "react";

export type SlotFetchState<T> =
  | { status: "unfetched" }
  | { status: "loading" }
  | { status: "error" }
  | { status: "resolved"; list: T[] };

export interface UseLazySlotCacheResult<T> {
  /** Fire the lazy fetch for `slot`. Idempotent — safe to call on every menu open;
   *  only the first call per slot per mount actually reads. */
  prefetch: (slot: number) => void;
  /** This slot's fetch state, right now. */
  listFor: (slot: number) => SlotFetchState<T>;
}

/** `fetchFn` should be a STABLE reference (a module-level `lib/invoke` wrapper, as every
 *  current caller passes) — it is called fresh on every `prefetch`, never memoized. */
export function useLazySlotCache<T>(
  fetchFn: (slot: number) => Promise<T[]>,
): UseLazySlotCacheResult<T> {
  const [bySlot, setBySlot] = useState<
    Partial<Record<number, T[] | "loading" | "error">>
  >({});
  const fetchedSlotsRef = useRef(new Set<number>());

  const prefetch = (slot: number) => {
    if (fetchedSlotsRef.current.has(slot)) return;
    fetchedSlotsRef.current.add(slot);
    setBySlot((p) => ({ ...p, [slot]: "loading" }));
    fetchFn(slot)
      .then((list) => {
        setBySlot((p) => ({ ...p, [slot]: list }));
      })
      .catch(() => {
        setBySlot((p) => ({ ...p, [slot]: "error" }));
      });
  };

  const listFor = (slot: number): SlotFetchState<T> => {
    const v = bySlot[slot];
    if (v === undefined) return { status: "unfetched" };
    if (v === "loading") return { status: "loading" };
    if (v === "error") return { status: "error" };
    return { status: "resolved", list: v };
  };

  return { prefetch, listFor };
}
