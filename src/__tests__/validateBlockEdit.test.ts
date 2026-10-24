// Tests for the Copy tab's firmware-faithful block-cap pre-flight (up-front UX
// only — the Rust `copy_apply` guard is the real enforcement). Sync, no fake
// timers; ids are the real members from `block-classification.json`.

import { describe, expect, it } from "vitest";
import type { EditGraph, PresetEdit } from "../views/copy/copyModel";
import type { EditBlock } from "../views/copy/copyModel";
import {
  baseCounts,
  checkEdit,
  checkOp,
  classify,
} from "../views/copy/validateBlockEdit";
import { CAB_CATEGORY } from "../models/blockArtCatalog/cabGuitar";

// Real ids used across the cases below.
const CAB_1 = "ACD_AC30BrilliantCabIR"; // cabinet only
const CAB_2 = "ACD_AC30NormalCabIR"; // cabinet only
const CAB_3 = "ACD_Ampeg66B15CabIR"; // cabinet only
const CONV_STANDALONE = "ACD_TMSpring63Conv"; // conv only, no cab
const CONV_STANDALONE_2 = "ACD_TMHallOfDoomConv"; // conv only, no cab
const REVERB_COMBO = "ACD_DeluxeReverb68CustomCabIRConvRvb"; // conv AND cabinet
const REVERB_COMBO_DRY = "ACD_DeluxeReverb68CustomNoFxCabIR"; // cabinet only (its dry sibling)
const GLOOPER = "ACD_Glooper";
const FX_STEREO = "ACD_FxLoop3_4";
const FX_MONO_A = "ACD_FxLoop3";
const FX_MONO_B = "ACD_FxLoop4";
// A plain non-capped pedal, for the CPU-only case.
const HEAVY_PEDAL = "ACD_RackDelayStereo"; // 26.4% each, no conv/cabinet/glooper

let uidSeq = 0;
function block(model: string, opts: Partial<EditBlock> = {}): EditBlock {
  uidSeq += 1;
  return {
    uid: `u${String(uidSeq)}`,
    group: opts.group ?? "G1",
    nodeId: opts.nodeId ?? `n${String(uidSeq)}`,
    model,
    change: null,
    bypassed: false,
    secondCabSlot: opts.secondCabSlot,
  };
}

/** One flat series stage holding `models`, one block per array entry unless a
 *  richer block is passed in `blocks` directly. */
function graph(blocks: EditBlock[]): EditGraph {
  return {
    inputType: null,
    outputType: null,
    inputs: null,
    outputs: null,
    lanes: null,
    stages: [{ kind: "series", blocks }],
  };
}

function edit(blocks: EditBlock[]): PresetEdit {
  return { graph: graph(blocks), origByNodeId: new Map() };
}

describe("classify", () => {
  it("is exact-string membership — a conv reverb combo classifies as BOTH conv and cabinet", () => {
    expect(classify(REVERB_COMBO)).toEqual({
      convLimit: true,
      cabinet: true,
      glooper: false,
    });
  });

  it("does NOT fold a wet combo onto its dry NoFx sibling — the suffix is the signal", () => {
    expect(classify(REVERB_COMBO_DRY)).toEqual({
      convLimit: false,
      cabinet: true,
      glooper: false,
    });
  });

  it("any id outside the three sets classifies as all-false (e.g. a bypass-only FX loop)", () => {
    expect(classify("ACD_FxLoop1")).toEqual({
      convLimit: false,
      cabinet: false,
      glooper: false,
    });
  });
});

describe("checkOp — cabinet cap (max 2, dual-cab = 2 slots)", () => {
  it("2 cabinets present, inserting a 3rd errs", () => {
    const counts = baseCounts(graph([block(CAB_1), block(CAB_2)]));
    expect(checkOp(counts, CAB_3, "before")).toBe(
      "ComboHalfStackCabinetsLimit",
    );
  });

  it("a lone dual-cab node (counts as 2) is fine on its own — no room for a 2nd cab though", () => {
    const counts = baseCounts(graph([block(CAB_1, { secondCabSlot: true })]));
    expect(counts.cabinet).toBe(2);
    // the dual-cab node already occupies both slots, so a 2nd cabinet errs
    expect(checkOp(counts, CAB_2, "before")).toBe(
      "ComboHalfStackCabinetsLimit",
    );
  });

  it("a dual-cab node plus one more single cabinet errs (3rd slot)", () => {
    const counts = baseCounts(
      graph([block(CAB_1, { secondCabSlot: true }), block(CAB_2)]),
    );
    expect(checkOp(counts, CAB_3, "before")).toBe(
      "ComboHalfStackCabinetsLimit",
    );
  });
});

describe("checkOp — convolution-reverb cap (max 1)", () => {
  it("1 standalone conv present, inserting a baked reverb combo errs — insert BEFORE", () => {
    const counts = baseCounts(graph([block(CONV_STANDALONE)]));
    expect(checkOp(counts, REVERB_COMBO, "before")).toBe(
      "ConvolutionReverbLimit",
    );
  });

  it("1 standalone conv present, inserting a baked reverb combo errs — insert AFTER", () => {
    const counts = baseCounts(graph([block(CONV_STANDALONE)]));
    expect(checkOp(counts, REVERB_COMBO, "after")).toBe(
      "ConvolutionReverbLimit",
    );
  });

  it("conv -> conv replace is fine (net count unchanged)", () => {
    const anchor = block(CONV_STANDALONE);
    const counts = baseCounts(graph([anchor]));
    const reason = checkOp(counts, CONV_STANDALONE_2, "replace", {
      anchor: { model: anchor.model },
    });
    expect(reason).toBeNull();
  });

  it("replacing a baked reverb combo with its dry NoFx variant frees the conv slot, so a 2nd conv now fits", () => {
    const comboAnchor = block(REVERB_COMBO);
    const counts = baseCounts(graph([comboAnchor]));
    // Replacing the combo with its dry sibling should itself be allowed...
    const replaceReason = checkOp(counts, REVERB_COMBO_DRY, "replace", {
      anchor: { model: comboAnchor.model },
    });
    expect(replaceReason).toBeNull();
    // ...and after that replace, the resulting graph has room for a standalone conv.
    const afterReplace = graph([block(REVERB_COMBO_DRY)]);
    const countsAfter = baseCounts(afterReplace);
    expect(checkOp(countsAfter, CONV_STANDALONE, "before")).toBeNull();
  });
});

describe("checkOp — Glooper cap (max 2)", () => {
  it("2 Gloopers present is fine on its own", () => {
    const counts = baseCounts(graph([block(GLOOPER), block(GLOOPER)]));
    expect(counts.glooper).toBe(2);
  });

  it("2 Gloopers present, inserting a 3rd errs", () => {
    const counts = baseCounts(graph([block(GLOOPER), block(GLOOPER)]));
    expect(checkOp(counts, GLOOPER, "before")).toBe("GlooperEffectsLimit");
  });
});

describe("checkOp — FXLoopCoexistence (bidirectional)", () => {
  it("stereo FxLoop3_4 present, inserting mono FxLoop3 errs", () => {
    const counts = baseCounts(graph([block(FX_STEREO)]));
    expect(checkOp(counts, FX_MONO_A, "before")).toBe("FXLoopCoexistence");
  });

  it("reverse: mono FxLoop3 present, inserting stereo FxLoop3_4 errs", () => {
    const counts = baseCounts(graph([block(FX_MONO_A)]));
    expect(checkOp(counts, FX_STEREO, "before")).toBe("FXLoopCoexistence");
  });

  it("mono FxLoop3 + FxLoop4 together is fine (no stereo coexistence rule between two monos)", () => {
    const counts = baseCounts(graph([block(FX_MONO_A)]));
    expect(checkOp(counts, FX_MONO_B, "before")).toBeNull();
    expect(checkEdit(edit([block(FX_MONO_A), block(FX_MONO_B)]))).toBeNull();
  });

  it("no FX loop may appear twice (fw 1.8.58)", () => {
    for (const id of [
      "ACD_FxLoop1",
      "ACD_FxLoop2",
      FX_MONO_A,
      FX_MONO_B,
      FX_STEREO,
    ]) {
      const counts = baseCounts(graph([block(id)]));
      expect(checkOp(counts, id, "before")).toBe("FXLoopCoexistence");
      expect(
        checkOp(counts, id, "replace", { anchor: { model: id } }),
      ).toBeNull();
      expect(checkEdit(edit([block(id), block(id)]))).toBe("FXLoopCoexistence");
    }
  });

  it("replacing the mono anchor with stereo is fine when it's the only mono present", () => {
    const anchor = block(FX_MONO_A);
    const counts = baseCounts(graph([anchor]));
    expect(
      checkOp(counts, FX_STEREO, "replace", { anchor: { model: FX_MONO_A } }),
    ).toBeNull();
  });

  it("replacing one mono anchor with stereo still errs while the OTHER mono remains", () => {
    const anchorA = block(FX_MONO_A);
    const counts = baseCounts(graph([anchorA, block(FX_MONO_B)]));
    expect(
      checkOp(counts, FX_STEREO, "replace", { anchor: { model: FX_MONO_A } }),
    ).toBe("FXLoopCoexistence");
  });
});

describe("checkEdit — the save-gate", () => {
  it("CPU over budget on the final graph errs", () => {
    // 3 x 26.4% = 79.2% > 76.5% budget, none of these ids trip any other cap.
    const e = edit([
      block(HEAVY_PEDAL),
      block(HEAVY_PEDAL),
      block(HEAVY_PEDAL),
    ]);
    expect(checkEdit(e)).toBe("ProcessorUtilization");
  });

  it("a fully valid edit returns null (no false positive)", () => {
    const e = edit([
      block(CAB_1),
      block(CAB_2),
      block(CONV_STANDALONE),
      block(GLOOPER),
    ]);
    expect(checkEdit(e)).toBeNull();
  });

  it("an over-cap final graph (3 cabinets) errs via the save-gate too", () => {
    const e = edit([block(CAB_1), block(CAB_2), block(CAB_3)]);
    expect(checkEdit(e)).toBe("ComboHalfStackCabinetsLimit");
  });
});

describe("checkOp — CPU budget (per op, like the device)", () => {
  it("greys an insert that would push the total over budget", () => {
    const counts = baseCounts(graph([block(HEAVY_PEDAL), block(HEAVY_PEDAL)]));
    expect(checkOp(counts, HEAVY_PEDAL, "before")).toBe("ProcessorUtilization");
  });

  it("a replace frees the anchor's own cost", () => {
    const anchor = block(HEAVY_PEDAL);
    const counts = baseCounts(graph([block(HEAVY_PEDAL), anchor]));
    expect(
      checkOp(counts, HEAVY_PEDAL, "replace", {
        anchor: { model: anchor.model },
      }),
    ).toBeNull();
  });

  it("charges an uncosted id 20, as the device does", () => {
    // 52.8 + 20 = 72.8 fits; a second unknown reaches 92.8.
    const counts = baseCounts(graph([block(HEAVY_PEDAL), block(HEAVY_PEDAL)]));
    expect(checkOp(counts, "ACD_NotARealBlock", "before")).toBeNull();
    const withOne = baseCounts(
      graph([
        block(HEAVY_PEDAL),
        block(HEAVY_PEDAL),
        block("ACD_NotARealBlock"),
      ]),
    );
    expect(checkOp(withOne, "ACD_NotARealBlock", "before")).toBe(
      "ProcessorUtilization",
    );
  });
});

// fw 1.8.58 client rules, the cabinet weights confirmed on the unit's own screen.
describe("fw 1.8.58 cabinet and convolution caps", () => {
  const AMP_COMBO = "ACD_TwinReverb65NoFxCabIR";
  const STANDALONE_CAB_MODEL = "ACD_Fen57Champ";
  const EXTERNAL_CAB = "ACD_ExternalCab";

  it("an amp with its second mic takes two cabinet slots", () => {
    const counts = baseCounts(
      graph([block(AMP_COMBO, { secondCabSlot: true })]),
    );
    expect(counts.cabinet).toBe(2);
    expect(checkOp(counts, "ACD_CabSimTMS", "after")).toBe(
      "ComboHalfStackCabinetsLimit",
    );
    const oneMic = baseCounts(graph([block(AMP_COMBO)]));
    expect(checkOp(oneMic, "ACD_CabSimTMS", "after")).toBeNull();
  });

  it("a stock Cabinet over a cab model the unit carries over takes two slots", () => {
    // HW, fw 1.8.58: a stock ACD_CabSimTMS replacing the cab model ACD_MarG12H30BB came
    // out with that cab on both sides and its second cab on.
    const counts = baseCounts(
      graph([
        block("ACD_MarG12H30BB", { nodeId: "ACD_MarG12H30BB" }),
        block(CAB_1),
      ]),
    );
    expect(counts.cabinet).toBe(2);
    expect(
      checkOp(counts, "ACD_CabSimTMS", "replace", {
        anchor: { model: "ACD_MarG12H30BB", nodeId: "ACD_MarG12H30BB" },
      }),
    ).toBe("ComboHalfStackCabinetsLimit");
    // A non-minted id gets the block's on-load default instead: one slot.
    expect(
      checkOp(counts, "ACD_CabSimTMS", "replace", {
        anchor: { model: "ACD_MarG12H30BB", nodeId: "cab1" },
      }),
    ).toBeNull();
  });

  it("External Cab neither counts nor is ever refused", () => {
    const counts = baseCounts(graph([block(EXTERNAL_CAB), block(AMP_COMBO)]));
    expect(counts.cabinet).toBe(1);
    expect(checkOp(counts, CAB_1, "after")).toBeNull();
    const full = baseCounts(graph([block(CAB_1), block(CAB_2), block(CAB_3)]));
    expect(checkOp(full, EXTERNAL_CAB, "after")).toBeNull();
    expect(checkOp(full, GLOOPER, "after")).toBe("ComboHalfStackCabinetsLimit");
  });

  it("replacing a dual-mic combo with another combo takes one slot, as the Rust guard counts", () => {
    // A combo with its second mic (2) + a cab (1) = 3 over the cap; the replacement is a
    // fresh default-param combo: 3 + 1 - 2 = 2, allowed.
    const anchor = { model: AMP_COMBO, dualCab: true };
    const counts = baseCounts(
      graph([block(AMP_COMBO, { secondCabSlot: true }), block(CAB_1)]),
    );
    expect(
      checkOp(counts, "ACD_DeluxeReverb65NoFxCabIR", "replace", { anchor }),
    ).toBeNull();
  });

  it("a dual flag on a non-member anchor frees no cabinet slot", () => {
    const anchor = { model: GLOOPER, dualCab: true };
    const counts = baseCounts(
      graph([block(CAB_1), block(CAB_2), block(GLOOPER)]),
    );
    expect(checkOp(counts, CAB_3, "replace", { anchor })).toBe(
      "ComboHalfStackCabinetsLimit",
    );
  });

  it("standalone cabinet models count toward the cap", () => {
    const counts = baseCounts(
      graph([block(STANDALONE_CAB_MODEL), block(CAB_1)]),
    );
    expect(checkOp(counts, CAB_2, "after")).toBe("ComboHalfStackCabinetsLimit");
  });

  it("the convolution cap counts presence, not members", () => {
    const two = baseCounts(
      graph([block(CONV_STANDALONE), block(CONV_STANDALONE_2)]),
    );
    expect(
      checkOp(two, CONV_STANDALONE, "replace", {
        anchor: { model: CONV_STANDALONE_2 },
      }),
    ).toBeNull();
    expect(checkOp(two, GLOOPER, "after")).toBeNull();
    const princeton = baseCounts(
      graph([block("ACD_PrincetonReverb68CabIRConvRvb")]),
    );
    expect(checkOp(princeton, CONV_STANDALONE, "after")).toBe(
      "ConvolutionReverbLimit",
    );
  });
});

describe("cabinet classification", () => {
  it("counts every catalogued cabinet, available or not", () => {
    const missing = CAB_CATEGORY.blocks
      .map(([id]) => id)
      .filter((id) => !classify(id).cabinet);
    expect(missing).toEqual([]);
  });
});
