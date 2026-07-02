// Tests for the Copy tab's firmware-faithful block-cap pre-flight (up-front UX
// only — the Rust `copy_apply` guard is the real enforcement). Sync, no fake
// timers; ids are the real fw 1.8.45 members from `block-classification.json`.

import { describe, expect, it } from "vitest";
import type { EditGraph, PresetEdit } from "../views/copy/copyModel";
import type { EditBlock } from "../views/copy/copyModel";
import {
  baseCounts,
  checkEdit,
  checkOp,
  classify,
} from "../views/copy/validateBlockEdit";

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
    cabSim2Enabled: opts.cabSim2Enabled,
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
    const counts = baseCounts(graph([block(CAB_1, { cabSim2Enabled: true })]));
    expect(counts.cabinet).toBe(2);
    // the dual-cab node already occupies both slots, so a 2nd cabinet errs
    expect(checkOp(counts, CAB_2, "before")).toBe(
      "ComboHalfStackCabinetsLimit",
    );
  });

  it("a dual-cab node plus one more single cabinet errs (3rd slot)", () => {
    const counts = baseCounts(
      graph([block(CAB_1, { cabSim2Enabled: true }), block(CAB_2)]),
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
    const counts = baseCounts(graph([block(FX_MONO_A), block(FX_MONO_B)]));
    expect(checkOp(counts, FX_MONO_B, "before")).toBeNull();
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
