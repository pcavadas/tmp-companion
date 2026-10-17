// src/__tests__/CopyGaps.test.tsx — Copy's "+" gaps (look B), the one approved
// change near the signal-path code:
//   - SignalChainView draws `graph.gap` in place of every wire between two tiles of a row
//     or series stage, plus one at each end, and draws exactly the old wires without it;
//   - CopyPath names each gap by where it places the block, puts gaps only on a target
//     strip with a chosen block, and keeps a dual cab drawn as its two cab tiles while
//     placing: the upper tile's gaps place the block before or after the whole cab,
//     and the lower tile draws none.

import { describe, expect, it } from "vitest";
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import {
  SignalChainView,
  type StripBlock,
  type StripGraph,
} from "../views/SignalChainView";
import { CopyPath, type CopyGaps } from "../views/copy/CopyPath";
import { editGraphFromActive, type EditGraph } from "../views/copy/copyModel";
import type { ActiveGraph, GraphNode } from "../lib/types";
import { renderWithTheme } from "./testKit";

const tile = (name: string): StripBlock => ({ name, uid: name });

/** Every gap the strip asked for, in DOM order, as "after|before". */
function renderGaps(graph: Omit<StripGraph, "gap">): string[] {
  const g: StripGraph = {
    ...graph,
    gap: {
      render: ({ beforeUid, afterUid }) => (
        <i data-at={`${afterUid ?? ""}|${beforeUid ?? ""}`} />
      ),
    },
  };
  const { container } = renderWithTheme(<SignalChainView graph={g} />);
  return Array.from(container.querySelectorAll("[data-at]")).map(
    (el) => el.getAttribute("data-at") ?? "",
  );
}

describe("SignalChainView — the gap lines", () => {
  it("puts a gap at each end and between every two tiles of a series stage", () => {
    expect(
      renderGaps({
        stages: [{ kind: "series", blocks: [tile("A"), tile("B"), tile("C")] }],
      }),
    ).toEqual(["|A", "A|B", "B|C", "C|"]);
  });

  it("does the same in each lane of a split, and of inputs, outputs and lanes", () => {
    expect(
      renderGaps({
        stages: [
          { kind: "series", blocks: [tile("G")] },
          { kind: "split", a: [tile("A1"), tile("A2")], b: [tile("B1")] },
        ],
      }),
    ).toEqual(["|G", "G|", "|A1", "A1|A2", "A2|", "|B1", "B1|"]);
    expect(
      renderGaps({
        inputs: {
          a: { type: "guitar", blocks: [tile("I")] },
          b: { type: "mic", blocks: [] },
        },
        outputs: {
          a: { type: "out1", blocks: [tile("O")] },
          b: { type: "out2", blocks: [] },
        },
        stages: [],
      }),
    ).toEqual(["|I", "I|", "|O", "O|"]);
    expect(
      renderGaps({
        lanes: [
          { input: "guitar", output: "out1", blocks: [tile("L1"), tile("L2")] },
        ],
        stages: [],
      }),
    ).toEqual(["|L1", "L1|L2", "L2|"]);
  });

  it("gives an empty lane no gap (it has no block to anchor to)", () => {
    expect(
      renderGaps({ stages: [{ kind: "split", a: [], b: [tile("B")] }] }),
    ).toEqual(["|B", "B|"]);
  });

  it("without a gap, draws the same strip as a graph that never had one", () => {
    const graph: StripGraph = {
      stages: [
        { kind: "series", blocks: [tile("A"), tile("B")] },
        { kind: "split", a: [tile("C")], b: [tile("D")] },
      ],
    };
    const plain = renderWithTheme(<SignalChainView graph={graph} />);
    const html = plain.container.innerHTML;
    plain.unmount();
    const unset = renderWithTheme(
      <SignalChainView graph={{ ...graph, gap: undefined }} />,
    );
    expect(unset.container.innerHTML).toBe(html);
  });
});

const node = (
  id: string,
  model: string,
  extra: Partial<GraphNode> = {},
): GraphNode => ({
  group_id: "G1",
  node_id: id,
  model,
  bypassed: false,
  params: {},
  ...extra,
});

function series(nodes: GraphNode[]): EditGraph {
  const g: ActiveGraph = {
    name: "T",
    slot: 1,
    template: "gtrSeries",
    split_mix: null,
    nodes,
    stages: [{ kind: "series", blocks: nodes }],
  };
  return editGraphFromActive(g);
}

const SIMPLE = series([
  node("n1", "ACD_DynaComp"),
  node("n2", "ACD_TwinReverb65NoFx"),
  node("n3", "ACD_TMSmallHall"),
]);

function gapsFor(placed: [string, string][], disabled = false): CopyGaps {
  return {
    presetName: "Clean Verse",
    disabled,
    onPlace: (uid, mode) => {
      placed.push([uid, mode]);
    },
  };
}

const gapButtons = (): HTMLElement[] =>
  Array.from(document.querySelectorAll<HTMLElement>("[data-gap]"));

describe("CopyPath — the + gaps", () => {
  it("draws no gap without a chosen block", () => {
    renderWithTheme(<CopyPath graph={SIMPLE} onTap={() => undefined} />);
    expect(gapButtons()).toHaveLength(0);
  });

  it("names every gap by where it places the block, and places it there", async () => {
    const placed: [string, string][] = [];
    const user = userEvent.setup();
    renderWithTheme(
      <CopyPath
        graph={SIMPLE}
        onTap={() => undefined}
        gaps={gapsFor(placed)}
      />,
    );
    expect(gapButtons().map((b) => b.getAttribute("aria-label"))).toEqual([
      "Add here in Clean Verse, before Dynamic Comp",
      "Add here in Clean Verse, before '65 Twin",
      "Add here in Clean Verse, before Small Hall",
      "Add here in Clean Verse, at the end",
    ]);
    const [uDyna, uTwin, uHall] = (() => {
      const st = SIMPLE.stages[0];
      if (st.kind !== "series") throw new Error("series expected");
      return st.blocks.map((b) => b.uid);
    })();
    for (const b of gapButtons()) await user.click(b);
    expect(placed).toEqual([
      [uDyna, "before"],
      [uTwin, "before"],
      [uHall, "before"],
      [uHall, "after"],
    ]);
  });

  it("says 'after' for a row's last gap that is not the end of the chain", () => {
    const g: ActiveGraph = {
      name: "P",
      slot: 1,
      template: "gtrParallel1",
      split_mix: null,
      nodes: [],
      stages: [
        { kind: "series", blocks: [node("g", "ACD_DynaComp")] },
        {
          kind: "split",
          a: [node("a", "ACD_TwinReverb65NoFx")],
          b: [node("b", "ACD_TMSmallHall")],
        },
      ],
    };
    renderWithTheme(
      <CopyPath graph={editGraphFromActive(g)} gaps={gapsFor([])} />,
    );
    const labels = gapButtons().map((b) => b.getAttribute("aria-label"));
    expect(labels).toContain("Add here in Clean Verse, after Dynamic Comp");
    expect(labels).toContain("Add here in Clean Verse, after '65 Twin");
    expect(labels.some((l) => l?.endsWith("at the end"))).toBe(false);
  });

  it("greys every gap when the chosen block can't fit", () => {
    renderWithTheme(<CopyPath graph={SIMPLE} gaps={gapsFor([], true)} />);
    expect(gapButtons()).toHaveLength(4);
    for (const b of gapButtons()) expect(b).toBeDisabled();
  });

  describe("a dual cab is one block, drawn as its two cab tiles", () => {
    const cabNode = (id: string) =>
      node(id, "ACD_CabSimTMS", {
        cab_sim_id: "Cab412BritGB",
        cab_sim_id2: "Cab112DlxC12K",
        cab_sim2_enabled: true,
      });
    const uidsOf = (g: EditGraph): string[] => {
      const st = g.stages[0];
      if (st.kind !== "series") throw new Error("series expected");
      return st.blocks.map((b) => b.uid);
    };
    const cabTiles = () =>
      document.querySelectorAll('[data-block-tile="ACD_CabSimTMS"]');
    const TRAILING = series([node("n1", "ACD_DynaComp"), cabNode("n2")]);

    it("draws as two parallel cab tiles while no block is chosen", () => {
      renderWithTheme(<CopyPath graph={TRAILING} onTap={() => undefined} />);
      expect(cabTiles()).toHaveLength(2);
    });

    it("keeps both cab tiles with gaps, a + only around the upper one", async () => {
      const placed: [string, string][] = [];
      const user = userEvent.setup();
      renderWithTheme(
        <CopyPath
          graph={TRAILING}
          onTap={() => undefined}
          gaps={gapsFor(placed)}
        />,
      );
      expect(cabTiles()).toHaveLength(2);
      const labels = gapButtons().map((b) => b.getAttribute("aria-label"));
      expect(labels).toHaveLength(4);
      expect(labels[0]).toBe("Add here in Clean Verse, before Dynamic Comp");
      expect(labels[1]).toBe("Add here in Clean Verse, after Dynamic Comp");
      expect(labels[2]).toMatch(/^Add here in Clean Verse, before /);
      expect(labels[3]).toBe("Add here in Clean Verse, at the end");
      for (const b of gapButtons()) await user.click(b);
      const [uDyna, uCab] = uidsOf(TRAILING);
      expect(placed).toEqual([
        [uDyna, "before"],
        [uDyna, "after"],
        [uCab, "before"],
        [uCab, "after"],
      ]);
    });

    it("places a block before a leading cab and between it and the next block", async () => {
      const placed: [string, string][] = [];
      const user = userEvent.setup();
      const LEADING = series([cabNode("n1"), node("n2", "ACD_TMSmallHall")]);
      renderWithTheme(
        <CopyPath
          graph={LEADING}
          onTap={() => undefined}
          gaps={gapsFor(placed)}
        />,
      );
      expect(cabTiles()).toHaveLength(2);
      const labels = gapButtons().map((b) => b.getAttribute("aria-label"));
      expect(labels).toHaveLength(4);
      expect(labels[0]).toMatch(/^Add here in Clean Verse, before /);
      expect(labels[1]).toMatch(/^Add here in Clean Verse, after /);
      expect(labels[2]).toBe("Add here in Clean Verse, before Small Hall");
      expect(labels[3]).toBe("Add here in Clean Verse, at the end");
      for (const b of gapButtons()) await user.click(b);
      const [uCab, uHall] = uidsOf(LEADING);
      expect(placed).toEqual([
        [uCab, "before"],
        [uCab, "after"],
        [uHall, "before"],
        [uHall, "after"],
      ]);
    });
  });

  it("makes the source strip's tiles choosable, the chosen one pressed", async () => {
    const tapped: string[] = [];
    const user = userEvent.setup();
    const st = SIMPLE.stages[0];
    if (st.kind !== "series") throw new Error("series expected");
    renderWithTheme(
      <div data-testid="strip">
        <CopyPath
          graph={SIMPLE}
          onTap={(uid) => tapped.push(uid)}
          selectedUid={st.blocks[1].uid}
        />
      </div>,
    );
    const tiles = within(screen.getByTestId("strip")).getAllByRole("button");
    expect(tiles.map((b) => b.getAttribute("aria-pressed"))).toEqual([
      "false",
      "true",
      "false",
    ]);
    await user.click(tiles[2]);
    expect(tapped).toEqual([st.blocks[2].uid]);
  });
});
