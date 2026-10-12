// src/views/level/useFootswitchSceneContexts.ts — the Set-up step's per-preset
// footswitch scene-context cache (D3). INSTANT-FIRST, like `useSceneHandles`: the fetchFn
// answers from the startup backup scan's `fsSceneContextsByIndex` and falls back to
// `list_footswitch_scene_contexts` (a real device read — one field-8 fetch, plus a
// multi-second whole-library backup re-read when a large preset's `ftsw` tail is cut, see
// `lib/invoke`) only when the scan has NO ENTRY for the slot.
//
// The suggested scene is the footswitch row's DEFAULT measurement context, so the Set-up
// step resolves it for every footswitch row as soon as it opens (`SetupPage`'s warm
// effect) and holds Start until it lands. The default therefore comes from the preset's
// data, never from whether the user happened to open the row's picker. A thin wrapper over
// `useLazySlotCache` — see that hook for the shared fetch/cache mechanics and its
// no-self-heal-within-a-mount contract.

import { listFootswitchSceneContexts } from "../../lib/invoke";
import type { FsSceneContext } from "../../lib/types";
import { getLibraryScan } from "./libraryScan";
import { useLazySlotCache } from "./useLazySlotCache";

export type FsContextFetchState =
  | { status: "unfetched" }
  | { status: "loading" }
  | { status: "error" }
  | { status: "resolved"; row: FsSceneContext | null };

export interface UseFootswitchSceneContextsResult {
  /** Fire the fetch for `slot`'s scene-context rows. Idempotent — only the first call
   *  per slot per mount actually resolves. Reaches the device only when the backup scan
   *  has no entry for `slot`. */
  prefetch: (slot: number) => void;
  /** This preset+switch's scene-context fetch state, right now. */
  contextFor: (slot: number, switchIndex: number) => FsContextFetchState;
}

/** The backup scan's rows for `slot` when the scan reached it (map key present — an empty
 *  array is a real answer), else the live device read. */
async function fetchFsContexts(slot: number): Promise<FsSceneContext[]> {
  const backup = getLibraryScan().fsSceneContextsByIndex.get(slot);
  if (backup) return backup;
  return listFootswitchSceneContexts(slot);
}

export function useFootswitchSceneContexts(): UseFootswitchSceneContextsResult {
  const { prefetch, listFor } = useLazySlotCache(fetchFsContexts);

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
