// src/views/level/useLevelBlocks.ts — the Set-up step's per-preset BASE handle
// candidate cache. INSTANT-FIRST: the fetchFn consults the startup backup scan's
// `baseHandlesByIndex` (derived offline from the same presetJson the backup already
// decoded — no device I/O, resolves synchronously inside the Promise) and falls back to
// `list_level_blocks` (a real device read: load + discovery) ONLY when the backup scan has
// NO ENTRY for the slot at all — never merely because the row resolved to `[]`, which is
// the correct, expected answer for a genuinely blockless/unparseable preset (mirrors
// `useSceneHandles`'s own discriminator: MAP KEY PRESENCE, not list emptiness). The
// fallback lives INSIDE this hook's fetchFn, not as a second `useLazySlotCache` pass — see
// that hook's "no self-heal" doc: an error/empty result is cached for the rest of the
// mount, so a two-step "try backup, then separately try device" would need its OWN retry
// plumbing to avoid getting stuck on a transient backup-scan-not-ready gap.

import { listLevelBlocks } from "../../lib/invoke";
import type { ParamClass } from "../../lib/types";
import { getLibraryScan } from "./libraryScan";
import { useLazySlotCache } from "./useLazySlotCache";

/** One Base-row leveling candidate, normalized from EITHER source
 *  (`baseHandlesByIndex`'s `SceneHandleCandidate` or `list_level_blocks`'s `LevelBlock`) into
 *  one shape `SetupBody` consumes without caring which lane answered — no `live` flag: a
 *  candidate's `value` is display-only now (the current reading shown in the picker),
 *  never forwarded as `LevelJob.block_value` regardless of source (see `BaseHandlePick`'s
 *  doc in `leveling.ts`), so there is nothing left for the two sources to diverge on. */
export interface BaseCandidate {
  group_id: string;
  node_id: string;
  model_id: string;
  parameter_id: string;
  /** The control's current reading — display-only (`BlockLevelPick` renders it). */
  value: number;
  /** Present only for a backup-derived candidate (`list_level_blocks`'s `LevelBlock` wire
   *  shape carries no class annotation) — `BlockLevelPick`'s rank() already sorts a
   *  `undefined` class after every classified one, so the device-fallback arm's
   *  "classless" candidates fall in exactly where they always have; nothing here
   *  re-classifies them frontend-side. */
  paramClass?: ParamClass;
  headroom?: "full" | "lowers_only";
}

export type BaseBlockFetchState =
  | { status: "unfetched" }
  | { status: "loading" }
  | { status: "error" }
  | { status: "resolved"; blocks: BaseCandidate[] };

export interface UseLevelBlocksResult {
  /** Fire the lazy fetch for `slot`'s level-type blocks. Idempotent — safe to call on
   *  every menu open; only the first call per slot per mount actually resolves. When the
   *  backup scan has NO entry for `slot`, this DOES reach the device (`list_level_blocks`)
   *  — callers doing an eager, provably-device-free warm must gate on `hasBackupData`
   *  first (see `SetupBody`'s Set-up-render warm effect). */
  prefetch: (slot: number) => void;
  /** This preset's block-candidate fetch state, right now. */
  blocksFor: (slot: number) => BaseBlockFetchState;
  /** True iff the backup scan's `baseHandlesByIndex` has an entry (possibly `[]`) for
   *  `slot` — i.e. `prefetch(slot)` is PROVEN not to reach the device. */
  hasBackupData: (slot: number) => boolean;
}

/** The one fetchFn `useLazySlotCache` calls per slot: backup row first (resolves with no
 *  `await` reaching a device command) when the scan has REACHED that slot (map key
 *  present), `list_level_blocks` otherwise. An empty array IS a present key — a
 *  genuinely blockless/unparseable preset legitimately has zero candidates, and firing
 *  the device command for every such preset's warm would burn a live read for no gain
 *  (the device would answer `[]` too). */
async function fetchBaseCandidates(slot: number): Promise<BaseCandidate[]> {
  const backup = getLibraryScan().baseHandlesByIndex.get(slot);
  if (backup) {
    return backup.map((c) => ({
      group_id: c.groupId,
      node_id: c.nodeId,
      model_id: c.fenderId,
      parameter_id: c.parameterId,
      value: c.current,
      paramClass: c.class,
      headroom: c.headroom,
    }));
  }
  const blocks = await listLevelBlocks(slot);
  return blocks.map((b) => ({
    group_id: b.group_id,
    node_id: b.node_id,
    model_id: b.model_id,
    parameter_id: b.parameter_id,
    value: b.value,
  }));
}

export function useLevelBlocks(): UseLevelBlocksResult {
  const { prefetch, listFor } = useLazySlotCache(fetchBaseCandidates);

  const blocksFor = (slot: number): BaseBlockFetchState => {
    const st = listFor(slot);
    return st.status === "resolved"
      ? { status: "resolved", blocks: st.list }
      : st;
  };

  const hasBackupData = (slot: number): boolean =>
    getLibraryScan().baseHandlesByIndex.has(slot);

  return { prefetch, blocksFor, hasBackupData };
}
