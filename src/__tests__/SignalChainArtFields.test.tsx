// Strip↔Catalog parity: the signal-chain strip must feed BlockArt the SAME
// art-derived prop set the Catalog does. The bug was that the strip dropped the
// pedal-specific fields (footswitch/accent/lab), so a Boss BD-2 rendered with the
// default round footswitch instead of its plate. We mock BlockArt and capture the
// props each tile passes — covering the whole path (adapter → StripBlock →
// BlockTile → BlockArt) without asserting brittle SVG geometry.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render } from "@testing-library/react";

import { ThemeProvider } from "../theme/ThemeProvider";
import type { ActiveGraph, GraphNode, Stage } from "../lib/types";

// Capture every BlockArt render's props. SignalChainView imports BlockArt from
// this exact path, so the mock replaces the one the strip renders.
const blockArtProps: Record<string, unknown>[] = [];
vi.mock("../ui/BlockArt", () => ({
  BlockArt: (props: Record<string, unknown>) => {
    blockArtProps.push(props);
    return null;
  },
  HalfStackArt: () => null,
}));

// Imported AFTER the mock so the strip picks up the captured BlockArt.
import { ActiveSignalChainView } from "../views/ActiveSignalChainView";
import { blockArtTile, resolveBlockArt } from "../models/blockArt";

function node(model: string): GraphNode {
  return { group_id: "G1", node_id: "n0", model, bypassed: false };
}

function oneBlockGraph(model: string): ActiveGraph {
  const stage: Stage = { kind: "series", blocks: [node(model)] };
  return {
    name: "test",
    slot: 0,
    template: "gtrSeries",
    split_mix: null,
    nodes: [],
    stages: [stage],
  };
}

function renderStrip(model: string) {
  render(
    <ThemeProvider>
      <ActiveSignalChainView graph={oneBlockGraph(model)} />
    </ThemeProvider>,
  );
}

describe("SignalChainView — strip feeds BlockArt the full art prop set", () => {
  beforeEach(() => {
    blockArtProps.length = 0;
  });

  it("threads footswitch into the Boss BD-2 tile (plate, not the round default)", () => {
    renderStrip("ACD_BluesDriver");
    expect(blockArtProps.some((p) => p.footswitch === "plate")).toBe(true);
  });

  it("threads the accent chassis into a cream-chassis Fender reverb tile", () => {
    renderStrip("ACD_TMSmallHall");
    expect(
      blockArtProps.some(
        (p) => typeof p.accentColor === "string" && p.accentColor.length > 0,
      ),
    ).toBe(true);
  });
});

describe("blockArtTile — the shared adapter both strips consume", () => {
  it("carries footswitch/accent/lab from the resolved art", () => {
    const bd2 = blockArtTile("ACD_BluesDriver");
    expect(bd2.footswitch).toBe("plate"); // Boss → plate
    expect(bd2.lab).toBe(resolveBlockArt("ACD_BluesDriver")?.short);

    const reverb = blockArtTile("ACD_TMSmallHall");
    expect(reverb.accent).toBeTruthy(); // cream-chassis Fender reverb
  });

  it("falls back to an uppercased name for an uncatalogued model, no art fields", () => {
    const t = blockArtTile("ACD_SomeWeirdPedal");
    expect(t.name).toBe("SOME WEIRD PEDAL");
    expect(t.footswitch).toBeUndefined();
    expect(t.lab).toBeUndefined();
  });
});
