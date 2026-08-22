// src/views/level/useSceneHandles.ts — the Set-up step's per-preset scene-handle
// candidate cache. INSTANT-FIRST: the fetchFn consults the startup backup scan's
// `sceneHandlesByIndex` (derived offline from the same presetJson `list_scene_level_handles`
// reads live) and falls back to that live command ONLY when the backup scan has NO ENTRY
// for the slot at all — never merely because the row resolved to `[]`, which is the
// correct, expected answer for a scene-less preset (and for a preset the scan hasn't
// reached yet inside its OWN loop — see the map-presence discriminator below). The
// fallback lives INSIDE this hook's fetchFn — see `useLazySlotCache`'s no-self-heal doc:
// an error/empty result is cached for the rest of the mount, so a two-step "try backup,
// then separately try device" would need its own retry plumbing.
//
// `candidatesFor` returns an EXPLICIT fetch-state discriminant rather than
// `SceneHandleCandidate[] | "loading" | "error" | undefined` — the old shape let a
// bare `undefined` ("not fetched yet") and an empty/mismatched list ("fetched,
// this handle is gone") collapse into the same falsy check, so a carried-forward
// VALID handle rendered "(removed)" until the fetch resolved (BUG→GATE). Naming
// "unfetched" as its own state is what SceneLevelPick derives BOTH `stale` and
// `triggerLabel` from now — see that component.

import { listSceneLevelHandles } from "../../lib/invoke";
import type { SceneHandleRow, SceneHandleCandidate } from "../../lib/types";
import { getLibraryScan } from "./libraryScan";
import { useLazySlotCache } from "./useLazySlotCache";

export type HandleFetchState =
  | { status: "unfetched" }
  | { status: "loading" }
  | { status: "error" }
  | {
      status: "resolved";
      /** The safe-preselect list: level-safe candidates only, never `"other"`. */
      candidates: SceneHandleCandidate[];
      /** EVERY numeric control of every block in this scene, class-annotated and
       *  level-class first — the combined block+param picker's source (a superset of
       *  `candidates`). */
      allCandidates: SceneHandleCandidate[];
    };

export interface UseSceneHandlesResult {
  /** Fire the lazy fetch for `slot`'s scene-handle rows. Idempotent — safe to call
   *  on every menu open; only the first call per slot per mount actually resolves.
   *  When the backup scan has NO entry for `slot`, this DOES reach the device
   *  (`list_scene_level_handles`) — callers doing an eager, provably-device-free warm
   *  must gate on `hasBackupData` first (see `SetupBody`'s Set-up-render warm effect). */
  prefetch: (slot: number) => void;
  /** This preset+scene's candidate fetch state, right now. */
  candidatesFor: (slot: number, sceneSlot: number) => HandleFetchState;
  /** True iff the backup scan's `sceneHandlesByIndex` has an entry (possibly `[]`) for
   *  `slot` — i.e. `prefetch(slot)` is PROVEN not to reach the device. */
  hasBackupData: (slot: number) => boolean;
}

/** The one fetchFn `useLazySlotCache` calls per slot: the backup scan's row set for
 *  `slot` when the scan has REACHED that slot (map key present — mirrors the precedent
 *  `useLevelingFlow`'s amp-candidate lookup already sets: `.get(slot) ?? fallback()`),
 *  `list_scene_level_handles` otherwise. An empty array IS a present key — a scene-less
 *  preset legitimately has zero rows, and firing the device command for every such
 *  preset's eager prefetch would burn a live read for no gain (the device would answer
 *  `[]` too). */
async function fetchSceneHandles(slot: number): Promise<SceneHandleRow[]> {
  const backup = getLibraryScan().sceneHandlesByIndex.get(slot);
  if (backup) return backup;
  return listSceneLevelHandles(slot);
}

export function useSceneHandles(): UseSceneHandlesResult {
  const { prefetch, listFor } = useLazySlotCache(fetchSceneHandles);

  const candidatesFor = (slot: number, sceneSlot: number): HandleFetchState => {
    const st = listFor(slot);
    if (st.status !== "resolved") return st;
    const row: SceneHandleRow | undefined = st.list.find(
      (r) => r.sceneSlot === sceneSlot,
    );
    return {
      status: "resolved",
      candidates: row?.candidates ?? [],
      allCandidates: row?.allCandidates ?? [],
    };
  };

  const hasBackupData = (slot: number): boolean =>
    getLibraryScan().sceneHandlesByIndex.has(slot);

  return { prefetch, candidatesFor, hasBackupData };
}
