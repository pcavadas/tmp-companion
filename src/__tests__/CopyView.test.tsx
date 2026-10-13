// src/__tests__/CopyView.test.tsx — the Copy feature: the save-diff model (the
// load-bearing logic — it maps the staged edits onto real device ops), a render smoke,
// and the view's live-device regressions: BUG-7 (the ON-UNIT chip follows the live
// preset) and BUG-2 (a save to the active preset optimistically repaints the hero).

import { describe, it, expect, beforeEach, vi } from "vitest";
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { CopyView } from "../views/copy";
import {
  activeFromEditGraph,
  applyEditOp,
  cpuOfGraph,
  diffToOps,
  editGraphFromActive,
  initEdit,
  isEdited,
  originBlocks,
  removeEditBlock,
  retargetHistory,
  type EditBlock,
  type EditMap,
  type PresetEdit,
} from "../views/copy/copyModel";
import { invoke } from "@tauri-apps/api/core";
import type { ActiveGraph, CopyApplyItem, GraphNode } from "../lib/types";
import { ensureLibraryScan, resetLibraryScan } from "../device/libraryScan";
import { resetLiveDevice, useLiveDevice } from "../device/useLiveDevice";
import { backupRow, seriesGraph as fixtureGraph } from "./copyFixtures";
import {
  forceTauri,
  makeActiveGraph,
  makeBackupRead,
  mockCommands,
  mockLiveEvents,
} from "./commandMocks";
import { renderWithTheme } from "./testKit";

const listeners = mockLiveEvents();

/** Nearest ancestor matching `sel` — avoids the lint-forbidden non-null `!`. */
function ancestor(el: HTMLElement, sel: string): HTMLElement {
  const found = el.closest(sel);
  if (!found) throw new Error(`no ${sel} ancestor`);
  return found as HTMLElement;
}

const node = (group: string, id: string, model: string): GraphNode => ({
  group_id: group,
  node_id: id,
  model,
  bypassed: false,
  params: {},
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
        key: expect.any(String) as string,
        before: { kind: "node", nodeId: "n2" }, // before the successor → lands after Comp
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
        key: expect.any(String) as string,
        before: { kind: "node", nodeId: "n2" }, // before the tapped block
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
        key: expect.any(String) as string,
        before: { kind: "node", nodeId: "d" }, // before Hall's successor → after Hall, within G1
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

describe("copyModel — retargetHistory (#292)", () => {
  const g0 = seriesGraph();
  const g1 = { ...seriesGraph(), slot: 1 };
  const graphs = new Map<number, ActiveGraph>([
    [0, g0],
    [1, g1],
  ]);
  const graphFor = (s: number): ActiveGraph | null => graphs.get(s) ?? null;
  const replaceTwin = (e: PresetEdit): PresetEdit =>
    applyEditOp(e, firstSeries(e)[1].uid, "replace", "ACD_DeluxeReverb65");

  // Two edit steps: step 1 edits slot 0, step 2 edits slot 1.
  function twoSlotHistory() {
    const h = retargetHistory(null, [0, 1], graphFor);
    const s0 = h.stack[0];
    const e0 = s0[0];
    const e1 = s0[1];
    if (!e0 || !e1) throw new Error("missing slot");
    const s1: EditMap = { ...s0, 0: replaceTwin(e0) };
    const s2: EditMap = { ...s1, 1: replaceTwin(e1) };
    return { ...h, stack: [s0, s1, s2], idx: 2 };
  }

  it("keeps every staged edit and the pointer when the selection is unchanged", () => {
    const h = twoSlotHistory();
    const r = retargetHistory(h, [0, 1], graphFor);
    expect(r.stack).toHaveLength(3);
    expect(r.idx).toBe(2);
    expect(r.stack[2][0]).toBe(h.stack[2][0]);
    expect(r.stack[2][1]).toBe(h.stack[2][1]);
  });

  it("drops a deselected slot and collapses the steps that only touched it", () => {
    const h = twoSlotHistory();
    const r = retargetHistory(h, [0], graphFor);
    expect(r.stack).toHaveLength(2); // the slot-1 step became a no-op
    expect(r.idx).toBe(1);
    expect(r.stack[1][1]).toBeUndefined();
    expect(r.stack[1][0]).toBe(h.stack[2][0]);
  });

  it("re-initialises a reset slot, or one whose library graph changed", () => {
    const h = twoSlotHistory();
    const reset = retargetHistory(h, [0, 1], graphFor, new Set([1]));
    const fresh1 = reset.stack[reset.idx][1];
    expect(fresh1 && isEdited(fresh1)).toBe(false);
    const kept0 = reset.stack[reset.idx][0];
    expect(kept0 && isEdited(kept0)).toBe(true);

    const rescanned = new Map(graphs).set(0, { ...g0 });
    const r = retargetHistory(h, [0, 1], (s) => rescanned.get(s) ?? null);
    const fresh0 = r.stack[r.idx][0];
    expect(fresh0 && isEdited(fresh0)).toBe(false);
  });
});

describe("copyModel — bypass state (#293)", () => {
  it("carries bypass through the edit graph; replaced and inserted blocks are active", () => {
    const g = seriesGraph();
    const nodes = g.nodes.map((n) => ({ ...n, bypassed: true }));
    const bypassed: ActiveGraph = {
      ...g,
      nodes,
      stages: [{ kind: "series", blocks: nodes }],
    };
    let e = initEdit([0], () => bypassed)[0];
    expect(firstSeries(e).map((b) => b.bypassed)).toEqual([true, true, true]);
    e = applyEditOp(e, firstSeries(e)[1].uid, "replace", "ACD_Klon");
    e = applyEditOp(e, firstSeries(e)[0].uid, "after", "ACD_Klon");
    expect(
      activeFromEditGraph(e.graph).nodes.map((n) => [n.model, n.bypassed]),
    ).toEqual([
      ["ACD_Comp", true],
      ["ACD_Klon", false],
      ["ACD_Klon", false],
      ["ACD_SmallHall", true],
    ]);
  });
});

describe("CopyView — render", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    mockCommands();
  });

  it("shows the disconnected empty state when no unit", () => {
    renderWithTheme(<CopyView connected={false} />);
    expect(
      screen.getByText("Copy lives on the Tone Master Pro"),
    ).toBeInTheDocument();
  });

  it("shows Step 1 when connected", async () => {
    mockCommands({
      list_presets: () => [{ slot: 0, name: "Stadium Lead" }],
      read_library_via_backup: () => makeBackupRead({ scene_mode: "off" }),
    });
    renderWithTheme(<CopyView connected={true} />);
    await waitFor(() => {
      expect(
        screen.getByText("Copy blocks between presets"),
      ).toBeInTheDocument();
    });
  });
});

// The live bridges subscribe only in-app; each case below starts with clean module stores.
describe("CopyView — live device", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    resetLibraryScan();
    resetLiveDevice();
    localStorage.clear();
    return forceTauri();
  });

  describe("regression: BUG-7 the ON UNIT chip follows the live preset", () => {
    it("moves the chip from the connect-time slot to the live one on a tmp://live-preset push", async () => {
      mockCommands({
        list_presets: () => [
          { slot: 0, name: "Alpha" },
          { slot: 1, name: "Beta" },
        ],
      });
      renderWithTheme(
        <CopyView
          connected={true}
          initialGraph={makeActiveGraph({
            name: "On Unit",
            slot: 1,
            template: "gtrSeries",
          })}
        />,
      );
      // Exactly one ON UNIT chip; it starts in Beta's row (the connect-time graph slot).
      // Anchor on the unique chip, not the name (names can repeat across Copy's columns).
      const chip0 = await screen.findByText("ON UNIT");
      expect(
        within(ancestor(chip0, "button")).getByText("Beta"),
      ).toBeInTheDocument();
      expect(within(ancestor(chip0, "button")).queryByText("Alpha")).toBeNull();

      // A hardware preset change to slot 0 arrives — the chip follows to Alpha.
      await waitFor(() => {
        expect(listeners.has("tmp://live-preset")).toBe(true);
      });
      act(() => {
        listeners.get("tmp://live-preset")?.({
          payload: {
            listIndex: 0,
            name: "Alpha",
            isDirty: false,
            isFavorite: false,
          },
        });
      });
      await waitFor(() => {
        const chip = screen.getByText("ON UNIT"); // still exactly one
        expect(
          within(ancestor(chip, "button")).queryByText("Alpha"),
        ).toBeInTheDocument();
      });
      expect(
        within(ancestor(screen.getByText("ON UNIT"), "button")).queryByText(
          "Beta",
        ),
      ).toBeNull();
    });
  });

  // A live structural edit pushes NO device field-3, so the hero (driven by useLiveDevice's
  // graph) would stay stale until the monitor reconnects. CopyView must patch the live graph
  // optimistically when the edited target IS the active preset — no device re-read.
  describe("regression: BUG-2 saving to the active preset optimistically repaints the hero", () => {
    // The post-save read-back the device returns on the item's `graph` field: the EDITED
    // roster (Twin → DynaComp) under a distinct name so the hero assertion is unambiguous.
    // It must show the acked edit — CopyView adopts a read-back only when its blocks match
    // the edit (a pre-edit document is a stale held-session buffer and is ignored).
    const SAVED: ActiveGraph = (() => {
      const g = fixtureGraph("HERO-AFTER-SAVE", 1);
      const nodes = g.nodes.map((n) =>
        n.node_id === "n2" ? { ...n, model: "ACD_DynaComp" } : n,
      );
      return { ...g, nodes, stages: [{ kind: "series", blocks: nodes }] };
    })();

    function HeroProbe() {
      const live = useLiveDevice(true);
      return <div data-testid="hero-graph">{live.graph?.name ?? "none"}</div>;
    }

    it("repaints the hero with the saved graph when the edited target is the active preset", async () => {
      mockCommands({
        list_presets: () => [
          { slot: 0, name: "Stadium Lead" },
          { slot: 1, name: "Clean Verse" },
        ],
        read_library_via_backup: () =>
          makeBackupRead({
            total_rows: 2,
            scene_mode: "off",
            presets: [
              backupRow(0, "Stadium Lead"),
              backupRow(1, "Clean Verse"),
            ],
          }),
        copy_apply: ({ jobs, onResult }) => {
          const items: CopyApplyItem[] = jobs.map((j) => ({
            slot: j.listIndex,
            name: j.name,
            outcome: "updated",
            detail: "",
            graph: SAVED, // authoritative device read-back
          }));
          items.forEach((i) => {
            onResult.onmessage(i);
          });
          return items;
        },
      });
      const user = userEvent.setup();
      // active preset = slot 1 ("Clean Verse") via initialGraph; that's also the target we edit
      renderWithTheme(
        <>
          <CopyView
            connected={true}
            initialGraph={fixtureGraph("Clean Verse", 1)}
          />
          <HeroProbe />
        </>,
      );
      void ensureLibraryScan();

      // Hero starts WITHOUT the saved graph.
      expect(screen.getByTestId("hero-graph").textContent).not.toBe(
        "HERO-AFTER-SAVE",
      );

      // Step 1: reference = Stadium Lead (slot 0), target = Clean Verse (slot 1 = active).
      await screen.findByText("Copy blocks between presets", undefined, {
        timeout: 3000,
      });
      const stadium = await screen.findAllByText("Stadium Lead", undefined, {
        timeout: 3000,
      });
      await user.click(stadium[0]);
      const clean = await screen.findAllByText("Clean Verse", undefined, {
        timeout: 3000,
      });
      await user.click(clean[clean.length - 1]);
      await user.click(
        await screen.findByRole(
          "button",
          { name: /place the blocks/i },
          { timeout: 3000 },
        ),
      );

      // Step 2: replace the TwinReverb tile with DynaComp, back up, Save.
      const twin = await screen.findAllByText("65 TWN", undefined, {
        timeout: 3000,
      });
      await user.click(twin[twin.length - 1]);
      await user.click(
        await screen.findByText("DYNAMIC COMPRESSOR", undefined, {
          timeout: 3000,
        }),
      );
      await user.click(
        await screen.findByText(/backed up with pro control/i, undefined, {
          timeout: 3000,
        }),
      );
      await act(async () => {
        await user.click(
          await screen.findByRole(
            "button",
            { name: /save to the unit/i },
            { timeout: 3000 },
          ),
        );
      });
      await screen.findByText("Saved to the unit.", undefined, {
        timeout: 3000,
      });

      // The lock: the hero now shows the saved graph (optimistic, no device re-read).
      await waitFor(() => {
        expect(screen.getByTestId("hero-graph").textContent).toBe(
          "HERO-AFTER-SAVE",
        );
      });
    });
  });
});
