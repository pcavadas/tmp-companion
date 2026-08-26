// src/views/level/blockLevelGroups.ts — the pure candidate-grouping/derivation logic
// behind `BlockLevelPick`'s two-dropdown leveling-handle picker (D2/Part C). Split out
// of `BlockLevelPick.tsx` so the component stays JSX-only: everything here operates on
// plain data (no React, no DOM) and is unit-testable without rendering anything.
//
//   • `groupByBlock` — the flat candidate list (`list_level_blocks` /
//     `list_scene_level_handles`'s `allCandidates` / a footswitch's own `level_params`)
//     grouped BY BLOCK (`groupId:nodeId`), level-class-first rank both across blocks and
//     within one. One stable pass — `Map` preserves first-insertion key order.
//   • `bestEnabled` — a block's best-ranked ENABLED candidate (or `undefined` when every
//     candidate in the block is disabled). Shared by the auto-pick (selecting a
//     different block) and by `recommendedBlock` below — ONE "best control" rule, not
//     two that can drift apart.
//   • `recommendedBlock` — the single overall-best candidate: the FIRST block (in rank
//     order) that has an enabled candidate, not simply `blocks[0][0]`. A top block whose
//     every candidate is disabled (Scene's `shared_with_base`/`unknown` scope) must not
//     blank the "Recommended" tag for the whole list — it falls through to the next
//     block that actually has something pickable.
//   • `resolveHandle` — the DANGER-rule guard (`Pick`/`BlockPick` trap): resolves a
//     stored handle against the (resolved) candidate grouping into `blockStale` (the
//     stored BLOCK isn't offered at all) / `paramStale` (the block is present, the
//     stored PARAM isn't). Never falls back to the pseudo-option or `blocks[0]`.

import type { ParamClass } from "../../lib/types";

export type BlockLevelCandidate = {
  groupId: string;
  nodeId: string;
  fenderId: string;
  parameterId: string;
  /** The classifier's verdict — undefined when the source carries none (Base's
   *  `list_level_blocks`, which is already gated to level-safe params but doesn't
   *  annotate the class on the wire; see `session::LevelBlock`). An undefined-class
   *  candidate sorts after every classified one and never shows a tone-risk note. */
  paramClass?: ParamClass;
  /** `true` ⇒ this control can only make the sound QUIETER (already at/near the top
   *  of its range). Scene rows only. */
  lowersOnly?: boolean;
} & (
  | { disabled?: false; disabledTitle?: undefined }
  /** Scene rows only: this control's overlay scope — `shared_with_base`/`unknown`
   *  disable the row (the backend refuses that write). A disabled row always
   *  carries its reason — the producer (`sceneDisabledTitle`) never emits one
   *  without the other. */
  | { disabled: true; disabledTitle: string }
);

export interface BlockLevelHandle {
  groupId: string;
  nodeId: string;
  parameterId: string;
}

/** `groupId:nodeId` — the block-identity key threaded through grouping, the stale
 *  guard, and the block dropdown's `selected`/`Recommended` checks. */
export function blockKeyOf(c: { groupId: string; nodeId: string }): string {
  return `${c.groupId}:${c.nodeId}`;
}

/** Level-class-first rank, shared by the "Recommended" pick and the group order —
 *  mirrors `leveling.ts`'s `CLASS_RANK` (kept local: that table is keyed on the WIRE
 *  `ParamClass`, this one also has to rank the classless Base candidates). */
function rank(c: BlockLevelCandidate): number {
  if (c.paramClass === "level_linear" || c.paramClass === "level_db") return 0;
  if (c.paramClass === "wet_mix") return 1;
  return 2;
}

/** Narrows to the `disabled: true` arm — `Array.prototype.every` on the plain
 *  `c.disabled === true` predicate returns a `boolean`, not a narrowed element type,
 *  so reading `.disabledTitle` off a block's first candidate needs a `find` with this
 *  type-predicate instead (a block's disabled reason IS defined whenever it's
 *  disabled — the producer never emits one without the other). */
export function isDisabledCandidate(
  c: BlockLevelCandidate,
): c is BlockLevelCandidate & { disabled: true; disabledTitle: string } {
  return c.disabled === true;
}

/** A block's first (best-ranked) candidate — defensive because this tsconfig lacks
 *  `noUncheckedIndexedAccess`, even though `groupByBlock` never emits an empty group.
 *  Shared by every caller that would otherwise re-spell the same `length > 0 ? …`
 *  guard (the auto-pick and the block-row render, both in `BlockLevelPick.tsx`). */
export function groupHead(
  group: BlockLevelCandidate[],
): BlockLevelCandidate | undefined {
  return group.length > 0 ? group[0] : undefined;
}

/** Blocks (`groupId:nodeId`) ordered by their best-ranked candidate; candidates within
 *  a block ordered by rank. */
export function groupByBlock(
  list: BlockLevelCandidate[],
): BlockLevelCandidate[][] {
  const byBlock = new Map<string, BlockLevelCandidate[]>();
  list.forEach((c) => {
    const key = blockKeyOf(c);
    const g = byBlock.get(key);
    if (g) g.push(c);
    else byBlock.set(key, [c]);
  });
  return [...byBlock.values()]
    .map((g) => [...g].sort((a, b) => rank(a) - rank(b)))
    .sort((a, b) => {
      const ra = a.length > 0 ? rank(a[0]) : 2;
      const rb = b.length > 0 ? rank(b[0]) : 2;
      return ra - rb;
    });
}

/** A block's best-ranked ENABLED candidate — `undefined` when every candidate in the
 *  block is disabled. THE one "best control" rule: both the auto-pick (selecting a
 *  different block lands on its best enabled candidate) and `recommendedBlock` below
 *  read this same function, so the "Recommended" tag and the auto-pick can never
 *  disagree about which candidate is best. */
export function bestEnabled(
  group: BlockLevelCandidate[],
): BlockLevelCandidate | undefined {
  return group.find((c) => c.disabled !== true);
}

/** The single overall-best candidate across every block: the FIRST block (in rank
 *  order) that has an enabled candidate — not simply `blocks[0][0]`. A top block whose
 *  every candidate is disabled must not blank the "Recommended" tag for the whole
 *  list; it falls through to the next block that actually has something pickable.
 *  `null` only when every block is fully disabled (or there are no blocks at all). */
export function recommendedBlock(
  blocks: BlockLevelCandidate[][],
): BlockLevelCandidate | null {
  for (const group of blocks) {
    const best = bestEnabled(group);
    if (best) return best;
  }
  return null;
}

export interface ResolvedHandle {
  /** The stored handle's block-key (`null` for the pseudo pick / a not-yet-seeded
   *  footswitch row). */
  handleBlockKey: string | null;
  /** The block group holding the stored handle, or `undefined` when that block isn't
   *  offered by the (resolved) candidate list at all. */
  blockGroup: BlockLevelCandidate[] | undefined;
  /** The stored handle's own candidate within `blockGroup`, or `undefined` when the
   *  block is present but the stored param isn't among its candidates any more. */
  matched: BlockLevelCandidate | undefined;
  /** DANGER-rule guard, block arm: the stored handle's BLOCK isn't in the (resolved)
   *  candidate list at all — never silently fall back to the pseudo-option or
   *  `blocks[0]`. Gated on `resolved` so a not-yet-fetched list doesn't flag a
   *  perfectly valid carried-forward handle as stale. */
  blockStale: boolean;
  /** DANGER-rule guard, param arm: the block IS present but the stored param isn't
   *  among its candidates any more. */
  paramStale: boolean;
}

/** Resolves a stored `handle` against the block grouping — the DANGER-rule guard
 *  (`Pick`/`BlockPick` trap) that a stale stored value must render verbatim + a
 *  warning, never silently fall back. */
export function resolveHandle(
  blocks: BlockLevelCandidate[][],
  handle: BlockLevelHandle | null,
  resolved: boolean,
): ResolvedHandle {
  const handleBlockKey = handle ? blockKeyOf(handle) : null;
  const blockGroup = handleBlockKey
    ? blocks.find((g) => {
        const first = groupHead(g);
        return first != null && blockKeyOf(first) === handleBlockKey;
      })
    : undefined;
  const matched = blockGroup?.find(
    (c) => c.parameterId === handle?.parameterId,
  );
  const blockStale = handle != null && resolved && blockGroup == null;
  const paramStale = blockGroup != null && matched == null;
  return { handleBlockKey, blockGroup, matched, blockStale, paramStale };
}
