// A combo amp (built-in speaker) must render as ONE combo tile on the signal-chain
// strip, NOT a synthesized head-over-cab half-stack. A combo node carries a
// `cab_sim_id` (its modeled speaker) exactly like a half-stack head with a baked
// cab, so `cab_sim_id` presence alone can't tell them apart — the form does. Both
// strip call sites (the Level hero `ActiveSignalChainView` and the Copy `CopyPath`)
// must thread the combo discriminator. We mock the art layer and capture which
// component renders (BlockArt = single tile, HalfStackArt = head-over-cab) — the
// caption text is identical for both, so it can't distinguish them.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render } from "@testing-library/react";

// SignalChainView imports BlockArt + HalfStackArt from this exact path; the mock
// replaces both for every strip caller (ActiveSignalChainView AND CopyPath).
const blockArtProps: Record<string, unknown>[] = [];
const halfStackProps: Record<string, unknown>[] = [];
vi.mock("../ui/BlockArt", () => ({
  BlockArt: (props: Record<string, unknown>) => {
    blockArtProps.push(props);
    return null;
  },
  HalfStackArt: (props: Record<string, unknown>) => {
    halfStackProps.push(props);
    return null;
  },
}));

// Imported AFTER the mock so the strips pick up the captured art components.
import { ThemeProvider } from "../theme/ThemeProvider";
import { ActiveSignalChainView } from "../views/ActiveSignalChainView";
import { CopyPath } from "../views/copy/CopyPath";
import type { ActiveGraph, GraphNode } from "../lib/types";
import type { EditGraph } from "../views/copy/copyModel";

const ampNode = (model: string, cabSimId: string): GraphNode => ({
  group_id: "G1",
  node_id: model,
  model,
  bypassed: false,
  cab_sim_id: cabSimId,
});

const heroGraph = (n: GraphNode): ActiveGraph => ({
  name: "Test",
  slot: 1,
  template: "gtrSeries",
  split_mix: null,
  nodes: [],
  stages: [{ kind: "series", blocks: [n] }],
});

const copyGraph = (model: string, cabSimId: string): EditGraph => ({
  inputType: null,
  outputType: null,
  inputs: null,
  outputs: null,
  lanes: null,
  stages: [
    {
      kind: "series",
      blocks: [
        { uid: "u1", group: "G1", nodeId: "n1", model, change: null, cabSimId },
      ],
    },
  ],
});

describe("signal-chain strip — combo amps render as a single combo tile", () => {
  beforeEach(() => {
    blockArtProps.length = 0;
    halfStackProps.length = 0;
  });

  it("hero strip: a COMBO amp with a cabsimid → BlockArt combo tile, NOT a half-stack", () => {
    render(
      <ThemeProvider>
        <ActiveSignalChainView
          graph={heroGraph(
            ampNode("ACD_TwinReverb65BlondeNoFx", "G1265Creamback"),
          )}
        />
      </ThemeProvider>,
    );
    expect(halfStackProps).toHaveLength(0); // no synthesized head-over-cab
    expect(blockArtProps.some((p) => p.icon === "combo")).toBe(true);
  });

  it("hero strip: a true half-stack (head + baked cab) STILL stacks (no regression)", () => {
    render(
      <ThemeProvider>
        <ActiveSignalChainView
          graph={heroGraph(
            ampNode("ACD_HiwattDR103CanModCabIR", "Mar1960aV30Alt"),
          )}
        />
      </ThemeProvider>,
    );
    expect(halfStackProps.length).toBeGreaterThan(0); // head-over-cab preserved
  });

  it("Copy strip: a COMBO amp with a cabsimid → BlockArt combo tile, NOT a half-stack", () => {
    render(
      <ThemeProvider>
        <CopyPath
          graph={copyGraph("ACD_TwinReverb65BlondeNoFx", "G1265Creamback")}
        />
      </ThemeProvider>,
    );
    expect(halfStackProps).toHaveLength(0);
    expect(blockArtProps.some((p) => p.icon === "combo")).toBe(true);
  });
});
