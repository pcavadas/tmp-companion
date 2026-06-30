// src/__tests__/CopyView.test.tsx — the Copy feature: the save-diff model (the
// load-bearing logic — it maps the staged edits onto real device ops) + a render smoke.

import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";

import { ThemeProvider } from "../theme/ThemeProvider";
import { CopyView } from "../views/copy";
import {
  applyEditOp,
  cpuOfGraph,
  diffToOps,
  editGraphFromActive,
  initEdit,
  isEdited,
  originBlocks,
  removeEditBlock,
  type EditBlock,
  type PresetEdit,
} from "../views/copy/copyModel";
import { invoke } from "@tauri-apps/api/core";
import type { ActiveGraph, GraphNode } from "../lib/types";

const node = (group: string, id: string, model: string): GraphNode => ({
  group_id: group,
  node_id: id,
  model,
  bypassed: false,
});

// A simple 3-block series preset in one group.
function seriesGraph(): ActiveGraph {
  const nodes = [
    node("G1", "n1", "ACD_Comp"),
    node("G1", "n2", "ACD_TwinReverb"),
    node("G1", "n3", "ACD_SmallHall"),
  ];
  return {
    name: "Ref",
    slot: 0,
    template: "gtrSeries",
    split_mix: null,
    nodes,
    stages: [{ kind: "series", blocks: nodes }],
  };
}

// The blocks of the graph's first (series) stage.
function firstSeries(e: PresetEdit): EditBlock[] {
  const st = e.graph.stages[0];
  if (st.kind !== "series") throw new Error("expected a series stage");
  return st.blocks;
}

describe("copyModel — graph derivation", () => {
  it("derives a series preset as one series stage carrying real device addressing", () => {
    const g = editGraphFromActive(seriesGraph());
    expect(g.stages).toHaveLength(1);
    const st = g.stages[0];
    expect(st.kind).toBe("series");
    if (st.kind !== "series") throw new Error("expected series");
    expect(st.blocks.map((b) => b.model)).toEqual([
      "ACD_Comp",
      "ACD_TwinReverb",
      "ACD_SmallHall",
    ]);
    expect(st.blocks[0].group).toBe("G1");
    expect(st.blocks[0].nodeId).toBe("n1");
    expect(st.blocks[0].change).toBeNull();
  });

  it("preserves a true inline series → split → series topology", () => {
    const pre = [node("G1", "g", "ACD_Gate")];
    const a = [node("G2", "a1", "ACD_AC30"), node("G2", "a2", "ACD_Cab412")];
    const b = [node("G3", "b1", "ACD_Plexi"), node("G3", "b2", "ACD_Cab412")];
    const post = [node("G1", "h", "ACD_Hall")];
    const graph: ActiveGraph = {
      name: "Worship Swell",
      slot: 45,
      template: "gtrParallel1",
      split_mix: null,
      nodes: [...pre, ...a, ...b, ...post],
      stages: [
        { kind: "series", blocks: pre },
        { kind: "split", a, b },
        { kind: "series", blocks: post },
      ],
    };
    const g = editGraphFromActive(graph);
    expect(g.stages.map((s) => s.kind)).toEqual(["series", "split", "series"]);
    const split = g.stages[1];
    if (split.kind !== "split") throw new Error("expected split");
    expect(split.a.map((x) => x.model)).toEqual(["ACD_AC30", "ACD_Cab412"]);
    expect(split.b[0].group).toBe("G3");
  });
});

describe("copyModel — edits → device ops", () => {
  const graph = seriesGraph();
  const baseEdit = () => initEdit([0], () => graph)[0];

  it("a fresh working copy has no ops + is not edited", () => {
    const e = baseEdit();
    expect(isEdited(e)).toBe(false);
    expect(diffToOps(e)).toHaveLength(0);
  });

  it("Replace → one replace op keyed by the original node id + a 'replaced' badge", () => {
    let e = baseEdit();
    const uid = firstSeries(e)[1].uid; // ACD_TwinReverb
    e = applyEditOp(e, uid, "replace", "ACD_DeluxeReverb65");
    expect(isEdited(e)).toBe(true);
    expect(diffToOps(e)).toEqual([
      {
        kind: "replace",
        group: "G1",
        nodeId: "n2",
        repl: { kind: "model", fenderId: "ACD_DeluxeReverb65" },
      },
    ]);
    // The replaced tile is badged "replaced" (⟲), not "added".
    expect(firstSeries(e)[1].change).toBe("replaced");
  });

  it("Insert after → one insert op anchored to the tapped block, badged 'added'", () => {
    let e = baseEdit();
    const uid = firstSeries(e)[0].uid; // ACD_Comp
    e = applyEditOp(e, uid, "after", "ACD_Klon");
    expect(firstSeries(e).map((b) => b.model)).toEqual([
      "ACD_Comp",
      "ACD_Klon",
      "ACD_TwinReverb",
      "ACD_SmallHall",
    ]);
    expect(firstSeries(e)[1].change).toBe("added");
    expect(diffToOps(e)).toEqual([
      {
        kind: "insert",
        group: "G1",
        beforeFenderId: "ACD_TwinReverb", // before the successor → lands after Comp
        repl: { kind: "model", fenderId: "ACD_Klon" },
      },
    ]);
  });

  it("Insert before → anchored on the tapped block (lands before it)", () => {
    let e = baseEdit();
    const uid = firstSeries(e)[1].uid; // ACD_TwinReverb
    e = applyEditOp(e, uid, "before", "ACD_Klon");
    expect(diffToOps(e)).toEqual([
      {
        kind: "insert",
        group: "G1",
        beforeFenderId: "ACD_TwinReverb", // before the tapped block
        repl: { kind: "model", fenderId: "ACD_Klon" },
      },
    ]);
  });

  it("Remove → one remove op keyed by node id", () => {
    let e = baseEdit();
    const uid = firstSeries(e)[2].uid; // ACD_SmallHall
    e = removeEditBlock(e, uid);
    expect(diffToOps(e)).toEqual([
      { kind: "remove", group: "G1", nodeId: "n3" },
    ]);
  });

  it("ops order is removes → replaces → inserts", () => {
    let e = baseEdit();
    e = removeEditBlock(e, firstSeries(e)[2].uid); // remove SmallHall
    e = applyEditOp(e, firstSeries(e)[1].uid, "replace", "ACD_DeluxeReverb65"); // replace TwinReverb
    e = applyEditOp(e, firstSeries(e)[0].uid, "after", "ACD_Klon"); // insert after Comp
    const kinds = diffToOps(e).map((o) => o.kind);
    expect(kinds).toEqual(["remove", "replace", "insert"]);
  });

  it("edits inside a split sub-lane address the right group/node", () => {
    const a = [node("G2", "a1", "ACD_AC30")];
    const b = [node("G3", "b1", "ACD_Plexi")];
    const splitGraph: ActiveGraph = {
      name: "Split",
      slot: 1,
      template: "gtrParallel1",
      split_mix: null,
      nodes: [...a, ...b],
      stages: [{ kind: "split", a, b }],
    };
    let e = initEdit([1], () => splitGraph)[1];
    const split = e.graph.stages[0];
    if (split.kind !== "split") throw new Error("expected split");
    const uid = split.a[0].uid; // ACD_AC30 in lane A
    e = applyEditOp(e, uid, "replace", "ACD_Bassman");
    expect(diffToOps(e)).toEqual([
      {
        kind: "replace",
        group: "G2",
        nodeId: "a1",
        repl: { kind: "model", fenderId: "ACD_Bassman" },
      },
    ]);
  });

  it("an insert in a post-split series stage anchors within its group (signal order)", () => {
    // series(G1) → split(G2 ∥ G3) → series(G1). The post-split stage reuses G1, so a
    // correct insert anchor depends on blockArrays yielding blocks in true signal order
    // (pre → split → post) — not on the split's G2/G3 blocks in between.
    const pre = [node("G1", "g", "ACD_Gate")];
    const a = [node("G2", "a1", "ACD_AC30")];
    const b = [node("G3", "b1", "ACD_Plexi")];
    const post = [node("G1", "h", "ACD_Hall"), node("G1", "d", "ACD_Delay")];
    const graph2: ActiveGraph = {
      name: "SeriesSplitSeries",
      slot: 2,
      template: "gtrParallel1",
      split_mix: null,
      nodes: [...pre, ...a, ...b, ...post],
      stages: [
        { kind: "series", blocks: pre },
        { kind: "split", a, b },
        { kind: "series", blocks: post },
      ],
    };
    let e = initEdit([2], () => graph2)[2];
    const postStage = e.graph.stages[2];
    if (postStage.kind !== "series") throw new Error("expected series");
    const hallUid = postStage.blocks[0].uid; // ACD_Hall (G1)
    e = applyEditOp(e, hallUid, "after", "ACD_Klon");
    expect(diffToOps(e)).toEqual([
      {
        kind: "insert",
        group: "G1",
        beforeFenderId: "ACD_Delay", // before Hall's successor → after Hall, within G1
        repl: { kind: "model", fenderId: "ACD_Klon" },
      },
    ]);
  });

  it("cpuOfGraph returns a finite total + originBlocks de-dupes by model", () => {
    const e = baseEdit();
    expect(Number.isFinite(cpuOfGraph(e.graph))).toBe(true);
    const dupGraph: ActiveGraph = {
      ...graph,
      nodes: [node("G1", "x", "ACD_Comp"), node("G1", "y", "ACD_Comp")],
    };
    expect(originBlocks(dupGraph).map((o) => o.model)).toEqual(["ACD_Comp"]);
  });
});

describe("CopyView — render", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("shows the disconnected empty state when no unit", () => {
    render(
      <ThemeProvider>
        <CopyView connected={false} />
      </ThemeProvider>,
    );
    expect(
      screen.getByText("Copy lives on the Tone Master Pro"),
    ).toBeInTheDocument();
  });

  it("shows Step 1 when connected", async () => {
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "list_presets")
        return Promise.resolve([{ slot: 0, name: "Stadium Lead" }]);
      if (cmd === "get_store")
        return Promise.resolve({
          profiles: [],
          profile_by_slot: {},
          targets: [],
          playback_level: "stage",
        });
      if (cmd === "read_library_via_backup")
        return Promise.resolve({
          members: [],
          db_bytes: 0,
          total_rows: 0,
          scene_mode: "off",
          presets: [],
          song_presets: [],
          songs: [],
          setlists: [],
          setlist_songs: [],
        });
      return Promise.resolve(null);
    });
    render(
      <ThemeProvider>
        <CopyView connected={true} />
      </ThemeProvider>,
    );
    await waitFor(() => {
      expect(
        screen.getByText("Copy blocks between presets"),
      ).toBeInTheDocument();
    });
  });
});
