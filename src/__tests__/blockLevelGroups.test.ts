// src/__tests__/blockLevelGroups.test.ts — direct unit coverage for the pure
// grouping/derivation logic in `../views/level/blockLevelGroups.ts` behind Set up's Block
// and Knob dropdowns, so this algorithmic behavior — rank ordering and the best-knob
// pick — is testable without rendering anything. `setupControls.test.ts` covers the
// dropdowns built on it (including a block whose every knob is disabled).

import { describe, it, expect } from "vitest";

import {
  groupByBlock,
  bestEnabled,
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
