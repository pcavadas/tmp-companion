// Locks the "edited" semantics: any REPLACE is an edit — even a same-model pick, which
// re-stamps the model. So a same-model replace must show the ⟳ badge AND mark the preset
// edited AND emit a device op (the Guitar Boost bug: a ⟳ badge with no EDITED flag after
// replacing SAPPHR with the same SAPPHR). A Model op carries only the FenderId — not the
// reference block's parameter values.

import { describe, it, expect } from "vitest";

import type { ActiveGraph, GraphNode } from "../lib/types";
import {
  activeFromEditGraph,
  applyEditOp,
  blockArrays,
  diffToOps,
  editGraphFromActive,
  initEdit,
  isEdited,
  removeEditBlock,
} from "../views/copy/copyModel";

const node = (id: string, model: string, group = "G1"): GraphNode => ({
  group_id: group,
  node_id: id,
  model,
  bypassed: false,
});

function graph(): ActiveGraph {
  const nodes = [
    node("n1", "ACD_DynaComp"),
    node("n2", "ACD_TwinReverb65NoFx"),
  ];
  return {
    name: "Test",
    slot: null,
    template: "gtrSeries",
    split_mix: null,
    nodes,
    stages: [{ kind: "series", blocks: nodes }],
  };
}

// A multi-group series: a pedal in G1 followed by the amp as the FIRST (only) block of
// G2 — one visual series stage spanning two device groups (the gtrSeries shape that
// triggered the insert-before bug: the amp has no in-GROUP predecessor).
function multiGroupGraph(): ActiveGraph {
  const nodes = [
    node("n1", "ACD_DynaComp", "G1"),
    node("n2", "ACD_TwinReverb65NoFx", "G2"),
  ];
  return {
    name: "MG",
    slot: null,
    template: "gtrSeries",
    split_mix: null,
    nodes,
    stages: [{ kind: "series", blocks: nodes }],
  };
}

function stageBlocks(edit: ReturnType<typeof initEdit>[number]) {
  const stage = edit.graph.stages[0];
  if (stage.kind !== "series") throw new Error("expected a series stage");
  return stage.blocks;
}
const firstBlock = (edit: ReturnType<typeof initEdit>[number]) =>
  stageBlocks(edit)[0];

// A fresh edit on a 2-block series preset + the uid of its first block.
function setup() {
  const edit = initEdit([1], () => graph())[1];
  return { edit, uid: firstBlock(edit).uid };
}

describe("activeFromEditGraph — round-trips an edited graph for the optimistic cache patch", () => {
  it("preserves block group/model/order and re-stamps inserted blocks with a node id", () => {
    // Apply replace + insert-after + remove, then round-trip through the cache shape
    // (EditGraph → ActiveGraph → EditGraph) the optimistic patch uses.
    let edit = initEdit([1], () => graph())[1];
    const blocks0 = blockArrays(edit.graph).flat();
    const [uid1, uid2] = [blocks0[0].uid, blocks0[1].uid];
    edit = applyEditOp(edit, uid1, "replace", "ACD_Klon");
    edit = applyEditOp(edit, uid1, "after", "ACD_TapeEcho");
    edit = removeEditBlock(edit, uid2);

    const back = editGraphFromActive(activeFromEditGraph(edit.graph));

    const seq = (g: typeof edit.graph) =>
      blockArrays(g)
        .flat()
        .map((b) => `${b.group}:${b.model}`);
    // Same blocks, same signal order (n1→Klon, +TapeEcho after; n2 removed).
    expect(seq(back)).toEqual(["G1:ACD_Klon", "G1:ACD_TapeEcho"]);
    expect(seq(back)).toEqual(seq(edit.graph));
    // Every block carries a node id (the inserted block got a synthetic one) so a
    // SUBSEQUENT edit round treats it as an original — re-editable / removable.
    expect(
      blockArrays(back)
        .flat()
        .every((b) => b.nodeId != null),
    ).toBe(true);
  });
});

describe("applyEditOp / isEdited — any replace is an edit", () => {
  it("replacing a block with its OWN model still marks edited + a replaced badge", () => {
    const { edit, uid } = setup();
    const after = applyEditOp(edit, uid, "replace", "ACD_DynaComp");

    expect(isEdited(after)).toBe(true);
    expect(firstBlock(after).change).toBe("replaced");
    expect(diffToOps(after)).toHaveLength(1);
  });

  it("replacing with a DIFFERENT model marks edited + a replaced badge", () => {
    const { edit, uid } = setup();
    const after = applyEditOp(edit, uid, "replace", "ACD_Klon");

    expect(isEdited(after)).toBe(true);
    expect(firstBlock(after).change).toBe("replaced");
  });

  it("inserting a block marks edited + an added badge, even if it duplicates a model", () => {
    const { edit, uid } = setup();
    const after = applyEditOp(edit, uid, "after", "ACD_DynaComp");

    expect(isEdited(after)).toBe(true);
  });
});

// The device's insertNode field-2 inserts the new block BEFORE the referenced node
// (HW-verified fw 1.8.45), and field-2 omitted appends. So diffToOps anchors each insert
// on its in-array SUCCESSOR (beforeFenderId), in the successor's group, or null to append.
describe("diffToOps — insert anchoring (before-the-successor)", () => {
  it("insert BEFORE the amp anchors on the amp itself, in the amp's group", () => {
    // Amp is the only block of group G2; inserting before it must anchor on the amp
    // (beforeFenderId), landing the new block ahead of it inside G2.
    const edit = initEdit([1], () => multiGroupGraph())[1];
    const ampUid = stageBlocks(edit)[1].uid;
    const after = applyEditOp(edit, ampUid, "before", "ACD_Klon");

    const ins = diffToOps(after).find((o) => o.kind === "insert");
    if (ins?.kind !== "insert") throw new Error("expected an insert op");
    expect(ins.group).toBe("G2"); // the amp's group — the new block joins it ahead of the amp
    expect(ins.beforeFenderId).toBe("ACD_TwinReverb65NoFx"); // before the amp
  });

  it("insert AFTER a block anchors on that block's SUCCESSOR", () => {
    // After DynaComp = before its successor (the amp).
    const { edit, uid } = setup();
    const after = applyEditOp(edit, uid, "after", "ACD_Klon");

    const ins = diffToOps(after).find((o) => o.kind === "insert");
    if (ins?.kind !== "insert") throw new Error("expected an insert op");
    expect(ins.group).toBe("G1");
    expect(ins.beforeFenderId).toBe("ACD_TwinReverb65NoFx"); // successor → lands after DynaComp
  });

  it("insert BEFORE the FIRST block anchors on that first block (a head insert)", () => {
    const { edit, uid } = setup();
    const after = applyEditOp(edit, uid, "before", "ACD_Klon");

    const ins = diffToOps(after).find((o) => o.kind === "insert");
    if (ins?.kind !== "insert") throw new Error("expected an insert op");
    expect(ins.beforeFenderId).toBe("ACD_DynaComp"); // before the first block → at the head
  });

  it("insert AFTER the LAST block appends (beforeFenderId null)", () => {
    const edit = initEdit([1], () => graph())[1];
    const lastUid = stageBlocks(edit)[1].uid;
    const after = applyEditOp(edit, lastUid, "after", "ACD_Klon");

    const ins = diffToOps(after).find((o) => o.kind === "insert");
    if (ins?.kind !== "insert") throw new Error("expected an insert op");
    expect(ins.group).toBe("G1");
    expect(ins.beforeFenderId == null).toBe(true); // no successor → append at group end
  });

  // A visual series spanning two groups: amp in G1 then a pedal in G4. The in-array
  // successor of an "after the amp" insert is the G4 pedal — a DIFFERENT group — so the
  // op must NOT anchor on it (the device would reject a G4 id in a G1 insert); it appends
  // at the end of G1 instead. HW-verified: cross-group anchoring dropped the insert.
  function crossGroupSeries(): ActiveGraph {
    const nodes = [
      node("amp", "ACD_DeluxeReverb65BlondeVibratoNoFxCabIR", "G1"),
      node("pog", "ACD_POG", "G4"),
    ];
    return {
      name: "XG",
      slot: null,
      template: "gtrSeries",
      split_mix: null,
      nodes,
      stages: [{ kind: "series", blocks: nodes }],
    };
  }

  it("insert AFTER the last block of its group APPENDS, never anchors on a later group", () => {
    const edit = initEdit([1], () => crossGroupSeries())[1];
    const ampUid = stageBlocks(edit)[0].uid; // amp in G1 (G4 pedal follows in the array)
    const after = applyEditOp(edit, ampUid, "after", "ACD_BluesDriver");

    const ins = diffToOps(after).find((o) => o.kind === "insert");
    if (ins?.kind !== "insert") throw new Error("expected an insert op");
    expect(ins.group).toBe("G1");
    expect(ins.beforeFenderId == null).toBe(true); // append to G1, NOT before the G4 pedal
  });

  it("insert BEFORE a later-group block anchors on it, in that block's group", () => {
    const edit = initEdit([1], () => crossGroupSeries())[1];
    const pogUid = stageBlocks(edit)[1].uid; // POG in G4
    const after = applyEditOp(edit, pogUid, "before", "ACD_BluesDriver");

    const ins = diffToOps(after).find((o) => o.kind === "insert");
    if (ins?.kind !== "insert") throw new Error("expected an insert op");
    expect(ins.group).toBe("G4"); // joins the pedal's group
    expect(ins.beforeFenderId).toBe("ACD_POG"); // before the G4 pedal
  });
});

// The user's seed correctness scenarios: the op-list must replay to the intended path
// even when an insert anchors near a block that the same edit deletes.
describe("diffToOps — bracket-and-delete + net-no-op", () => {
  function oneBlock(model: string): ActiveGraph {
    const b = node("nB", model);
    return {
      name: "Seed",
      slot: null,
      template: "gtrSeries",
      split_mix: null,
      nodes: [b],
      stages: [{ kind: "series", blocks: [b] }],
    };
  }

  it("INV-A: insert A before B + insert C after B + remove B → exactly [A, C]", () => {
    let edit = initEdit([1], () => oneBlock("ACD_TwinReverb65NoFx"))[1];
    const bUid = firstBlock(edit).uid; // captured before any op — always B
    edit = applyEditOp(edit, bUid, "before", "ACD_Klon"); // A before B
    edit = applyEditOp(edit, bUid, "after", "ACD_TapeEcho"); // C after B
    edit = removeEditBlock(edit, bUid); // delete B

    // The staged signal path is exactly [A, C].
    expect(
      blockArrays(edit.graph)
        .flat()
        .map((b) => b.model),
    ).toEqual(["ACD_Klon", "ACD_TapeEcho"]);

    // The device op-list replays to [A, C]: remove B, then inserts RIGHT-TO-LEFT (C
    // appended first, then A anchored before C) — so no insert ever anchors on the
    // already-removed B, and B is never left in place.
    const ops = diffToOps(edit);
    expect(ops.flatMap((o) => (o.kind === "remove" ? [o.nodeId] : []))).toEqual(
      ["nB"],
    );
    const insModels = ops.flatMap((o) =>
      o.kind === "insert" ? [o.repl.fenderId] : [],
    );
    expect(insModels).toEqual(["ACD_TapeEcho", "ACD_Klon"]); // C then A
    const aIns = ops.find(
      (o) => o.kind === "insert" && o.repl.fenderId === "ACD_Klon",
    );
    if (aIns?.kind !== "insert") throw new Error("expected A's insert op");
    expect(aIns.beforeFenderId).toBe("ACD_TapeEcho"); // before C, NOT the deleted B
  });

  it("insert then remove the same inserted block is a net no-op → no device ops", () => {
    let edit = initEdit([1], () => oneBlock("ACD_TwinReverb65NoFx"))[1];
    const bUid = firstBlock(edit).uid;
    edit = applyEditOp(edit, bUid, "after", "ACD_TapeEcho"); // insert C
    const cUid = stageBlocks(edit).find((b) => b.uid !== bUid)?.uid;
    if (cUid == null) throw new Error("expected the inserted block");
    edit = removeEditBlock(edit, cUid); // remove it again

    expect(diffToOps(edit)).toHaveLength(0); // nothing to write → Save is a no-op
    expect(isEdited(edit)).toBe(false);
  });
});
