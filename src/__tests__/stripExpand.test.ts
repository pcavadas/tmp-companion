// Unit tests for the dual-cab display-layer split (src/views/stripExpand.ts) — the
// helper that makes the strip mirror Pro Control's two-parallel-cab drawing of a
// single dual-cab CabSim node, shared by the Level hero and the Copy strip.

import { describe, it, expect, vi } from "vitest";

import { expandDualCab } from "../views/stripExpand";
import type { StripBlock, StripStage } from "../views/SignalChainView";

const tile = (name: string, extra: Partial<StripBlock> = {}): StripBlock => ({
  name,
  ...extra,
});

// A dual-cab CabSim tile (already named from cab1 by toStripBlock/mkTile).
const dualCab = (extra: Partial<StripBlock> = {}): StripBlock =>
  tile("M4 V30", {
    model: "ACD_CabSimTMS",
    cabSimId: "Mar1960aV30Alt",
    cabSimId2: "Mar1960aV30Alt",
    cabSim2Enabled: true,
    ...extra,
  });

describe("expandDualCab", () => {
  it("splits a mid-stage dual cab into series → split(two cabs) → series", () => {
    const stages: StripStage[] = [
      { kind: "series", blocks: [tile("DR103"), dualCab(), tile("DMM")] },
    ];
    const [before, split, after] = expandDualCab(stages);
    expect(expandDualCab(stages).map((s) => s.kind)).toEqual([
      "series",
      "split",
      "series",
    ]);
    if (before.kind === "series")
      expect(before.blocks.map((b) => b.name)).toEqual(["DR103"]);
    if (after.kind === "series")
      expect(after.blocks.map((b) => b.name)).toEqual(["DMM"]);
    // Both parallel branches are the British cab, cab2 resolved by id.
    if (split.kind === "split") {
      expect(split.a).toHaveLength(1);
      expect(split.b).toHaveLength(1);
      expect(split.a[0].name).toBe("M4 V30");
      expect(split.b[0].name).toBe("M4 V30");
    }
  });

  it("omits the empty leading segment when the dual cab is first", () => {
    const out = expandDualCab([
      { kind: "series", blocks: [dualCab(), tile("DMM")] },
    ]);
    expect(out.map((s) => s.kind)).toEqual(["split", "series"]);
  });

  it("omits the empty trailing segment when the dual cab is last", () => {
    const out = expandDualCab([
      { kind: "series", blocks: [tile("DR103"), dualCab()] },
    ]);
    expect(out.map((s) => s.kind)).toEqual(["series", "split"]);
  });

  it("leaves a single-cab CabSim as one tile (no split)", () => {
    const single = tile("M4 V30", {
      cabSimId: "Mar1960aV30Alt",
      cabSim2Enabled: false,
    });
    const out = expandDualCab([{ kind: "series", blocks: [single] }]);
    expect(out).toHaveLength(1);
    expect(out[0].kind).toBe("series");
  });

  it("leaves an already-split stage untouched", () => {
    const stages: StripStage[] = [
      { kind: "split", a: [tile("A")], b: [tile("B")] },
    ];
    expect(expandDualCab(stages)).toEqual(stages);
  });

  it("both decomposed cab tiles share the source tile's click handler (one CabSim node)", () => {
    const onClick = vi.fn();
    const split = expandDualCab([
      { kind: "series", blocks: [dualCab({ onClick })] },
    ])[0];
    // Tapping either parallel cab tile routes to the SAME handler → the single
    // CabSim node stays the edit target (no synthetic per-cab nodes).
    if (split.kind === "split") {
      expect(split.a[0].onClick).toBe(onClick);
      expect(split.b[0].onClick).toBe(onClick);
    }
  });
});
