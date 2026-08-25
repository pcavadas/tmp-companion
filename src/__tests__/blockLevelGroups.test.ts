// src/__tests__/blockLevelGroups.test.ts — direct unit coverage for the pure
// grouping/derivation logic behind `BlockLevelPick`'s two-dropdown picker (D2/Part
// C), split out of the component into `../views/level/blockLevelGroups.ts` so this
// algorithmic behavior — rank ordering, the DANGER-rule stale resolution, and the
// "Recommended"/auto-pick candidate — is testable without rendering anything.
// `src/__tests__/BlockLevelPick.test.tsx` keeps the DOM-level DANGER-guard
// rendering + click-wiring coverage; this file is the fast, render-free layer
// underneath it.

import { describe, it, expect } from "vitest";

import {
  groupByBlock,
  bestEnabled,
  recommendedBlock,
  resolveHandle,
  blockKeyOf,
  type BlockLevelCandidate,
} from "../views/level/blockLevelGroups";

const twinOutputLevel: BlockLevelCandidate = {
  groupId: "G1",
  nodeId: "amp",
  fenderId: "ACD_TwinReverb65NoFx",
  parameterId: "outputLevel",
  paramClass: "level_linear",
};
const boostToneOther: BlockLevelCandidate = {
  groupId: "G1",
  nodeId: "boost1",
  fenderId: "ACD_Boost",
  parameterId: "toneKnob",
  // No class → ranks LAST within its block.
};
const boostGain: BlockLevelCandidate = {
  groupId: "G1",
  nodeId: "boost1",
  fenderId: "ACD_Boost",
  parameterId: "gain",
  paramClass: "level_db",
};
const boostMix: BlockLevelCandidate = {
  groupId: "G1",
  nodeId: "boost1",
  fenderId: "ACD_Boost",
  parameterId: "mix",
  paramClass: "wet_mix",
};
const disabledBoostGain: BlockLevelCandidate = {
  ...boostGain,
  disabled: true,
  disabledTitle: "shared with the base preset",
};
const disabledBoostTone: BlockLevelCandidate = {
  ...boostToneOther,
  disabled: true,
  disabledTitle: "shared with the base preset",
};

describe("groupByBlock", () => {
  it("groups candidates by groupId:nodeId, preserving first-insertion block order for a rank TIE", () => {
    // Boost's gain (level_db) and Twin's outputLevel (level_linear) tie at rank 0 —
    // the block order then falls back to which block's key was inserted FIRST
    // (`boostToneOther`, the array's first element, establishes "G1:boost1" before
    // `twinOutputLevel` establishes "G1:amp"), not array order of the winning
    // candidate itself.
    const blocks = groupByBlock([boostToneOther, twinOutputLevel, boostGain]);
    expect(blocks.map((g) => blockKeyOf(g[0]))).toEqual([
      "G1:boost1",
      "G1:amp",
    ]);
  });

  it("sorts candidates WITHIN a block level-class first, then wet_mix, then unclassified — array order doesn't matter", () => {
    const blocks = groupByBlock([boostToneOther, boostMix, boostGain]);
    const boost = blocks.find((g) => blockKeyOf(g[0]) === "G1:boost1");
    expect(boost?.map((c) => c.parameterId)).toEqual([
      "gain",
      "mix",
      "toneKnob",
    ]);
  });

  it("sorts BLOCKS by their own best-ranked candidate, not list order", () => {
    // Boost's best candidate is level_db (rank 0); it's listed OTHER-first in the
    // flat array but must still sort ahead of a block with no level-class candidate
    // at all.
    const wetOnlyBlock: BlockLevelCandidate = {
      groupId: "G1",
      nodeId: "delay1",
      fenderId: "ACD_Delay",
      parameterId: "mix",
      paramClass: "wet_mix",
    };
    const blocks = groupByBlock([wetOnlyBlock, boostToneOther, boostGain]);
    expect(blocks.map((g) => blockKeyOf(g[0]))).toEqual([
      "G1:boost1",
      "G1:delay1",
    ]);
  });
});

describe("bestEnabled", () => {
  it("returns the best-ranked ENABLED candidate, skipping a disabled one ranked ahead of it", () => {
    const boost = groupByBlock([disabledBoostGain, boostToneOther]).find(
      (g) => blockKeyOf(g[0]) === "G1:boost1",
    );
    expect(boost).toBeDefined();
    expect(bestEnabled(boost ?? [])?.parameterId).toBe("toneKnob");
  });

  it("returns undefined when every candidate in the block is disabled", () => {
    const boost = groupByBlock([disabledBoostGain, disabledBoostTone]).find(
      (g) => blockKeyOf(g[0]) === "G1:boost1",
    );
    expect(boost).toBeDefined();
    expect(bestEnabled(boost ?? [])).toBeUndefined();
  });
});

describe("recommendedBlock", () => {
  it("is the first block's best candidate when that block has one enabled", () => {
    const blocks = groupByBlock([boostToneOther, twinOutputLevel]);
    // Twin's outputLevel (level_linear) outranks Boost's toneKnob (no class).
    expect(recommendedBlock(blocks)?.parameterId).toBe("outputLevel");
  });

  it("BUG FIX: a top block entirely disabled is skipped — the next block with an enabled candidate is recommended instead of nothing at all", () => {
    // Boost's gain is level_db (rank 0, ties Twin's outputLevel) and sorts FIRST,
    // but every Boost candidate is disabled.
    const blocks = groupByBlock([disabledBoostGain, twinOutputLevel]);
    expect(blockKeyOf(blocks[0][0])).toBe("G1:boost1"); // still sorts first
    expect(recommendedBlock(blocks)?.parameterId).toBe("outputLevel");
  });

  it("returns null when every block is fully disabled", () => {
    const blocks = groupByBlock([disabledBoostGain, disabledBoostTone]);
    expect(recommendedBlock(blocks)).toBeNull();
  });

  it("returns null for an empty candidate list", () => {
    expect(recommendedBlock(groupByBlock([]))).toBeNull();
  });
});

describe("resolveHandle — the DANGER-rule stale guard", () => {
  const blocks = groupByBlock([twinOutputLevel, boostGain, boostToneOther]);

  it("a null handle resolves with no staleness and no block/param match", () => {
    const r = resolveHandle(blocks, null, true);
    expect(r).toMatchObject({
      handleBlockKey: null,
      blockGroup: undefined,
      matched: undefined,
      blockStale: false,
      paramStale: false,
    });
  });

  it("not yet resolved: a carried-forward handle is never flagged stale, even if its block isn't in (the still-empty) `blocks`", () => {
    const r = resolveHandle(
      [],
      { groupId: "G1", nodeId: "amp", parameterId: "outputLevel" },
      false,
    );
    expect(r.blockStale).toBe(false);
    expect(r.paramStale).toBe(false);
  });

  it("resolved, block missing from the list entirely: blockStale true, no blockGroup", () => {
    const r = resolveHandle(
      blocks,
      { groupId: "G1", nodeId: "gone", parameterId: "outputLevel" },
      true,
    );
    expect(r.blockStale).toBe(true);
    expect(r.blockGroup).toBeUndefined();
    expect(r.paramStale).toBe(false);
  });

  it("resolved, block present but the stored param is gone: paramStale true, blockGroup found, no match", () => {
    const r = resolveHandle(
      blocks,
      { groupId: "G1", nodeId: "amp", parameterId: "goneParam" },
      true,
    );
    expect(r.blockStale).toBe(false);
    expect(r.blockGroup).toBeDefined();
    expect(r.matched).toBeUndefined();
    expect(r.paramStale).toBe(true);
  });

  it("resolved, block AND param both present: a clean match, no staleness", () => {
    const r = resolveHandle(
      blocks,
      { groupId: "G1", nodeId: "boost1", parameterId: "gain" },
      true,
    );
    expect(r.blockStale).toBe(false);
    expect(r.paramStale).toBe(false);
    expect(r.matched).toBe(boostGain);
    // Referential identity: `matched` is the SAME object `blockGroup` holds, not a
    // structurally-equal copy — callers (`BlockLevelPick`'s `controlRow`) rely on
    // `c === matched` rather than a field-by-field compare.
    expect(r.blockGroup?.includes(boostGain)).toBe(true);
  });
});
