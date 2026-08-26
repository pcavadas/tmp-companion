// src/views/level/useFootswitchSceneContexts.ts — the Set-up step's per-preset
// footswitch scene-context cache (D3). `list_footswitch_scene_contexts` is a real
// device read (one field-8 fetch — plus a multi-second whole-library backup re-read
// when a large preset's `ftsw` tail is cut, see `lib/invoke`), so it fires LAZILY —
// on first open of a
// footswitch row's scene-context picker, once per PRESET. A thin wrapper over
// `useLazySlotCache` — see that hook for the shared fetch/cache mechanics and its
// no-self-heal-within-a-mount contract.

import { listFootswitchSceneContexts } from "../../lib/invoke";
import type { FsSceneContext } from "../../lib/types";
import { useLazySlotCache } from "./useLazySlotCache";

export type FsContextFetchState =
  | { status: "unfetched" }
  | { status: "loading" }
  | { status: "error" }
  | { status: "resolved"; row: FsSceneContext | null };

export interface UseFootswitchSceneContextsResult {
  /** Fire the lazy fetch for `slot`'s scene-context rows. Idempotent — safe to call on
   *  every menu open; only the first call per slot per mount actually reads. */
  prefetch: (slot: number) => void;
  /** This preset+switch's scene-context fetch state, right now. */
  contextFor: (slot: number, switchIndex: number) => FsContextFetchState;
}

export function useFootswitchSceneContexts(): UseFootswitchSceneContextsResult {
  const { prefetch, listFor } = useLazySlotCache(listFootswitchSceneContexts);

  const contextFor = (
    slot: number,
    switchIndex: number,
  ): FsContextFetchState => {
    const st = listFor(slot);
    if (st.status !== "resolved") return st;
    return {
      status: "resolved",
      row: st.list.find((r) => r.switch === switchIndex) ?? null,
    };
  };

  return { prefetch, contextFor };
}
