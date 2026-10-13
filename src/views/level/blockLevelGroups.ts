// src/views/level/blockLevelGroups.ts — the pure candidate-grouping/derivation logic
// behind `FlatLevelPick`'s leveling-handle picker and `SetupPage`'s auto-pick. Kept out
// of the components so they stay JSX-only: everything here operates on plain data (no
// React, no DOM) and is unit-testable without rendering anything.
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

/** `groupId:nodeId` — the block-identity key threaded through grouping and the
 *  picker's `selected`/`Recommended` checks. */
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

/** A block's first (best-ranked) candidate — defensive because this tsconfig lacks
 *  `noUncheckedIndexedAccess`, even though `groupByBlock` never emits an empty group.
 *  Shared by every caller that would otherwise re-spell the same `length > 0 ? …`
 *  guard (`SetupPage`'s auto-pick). */
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
