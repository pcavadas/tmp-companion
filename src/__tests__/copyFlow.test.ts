// src/__tests__/copyFlow.test.ts — the Copy flow reducer's look B state (the chosen source
// block and the open block menu) and the words the save gate picks from state.

import { describe, expect, it } from "vitest";

import {
  copyReducer,
  INITIAL_COPY_STATE,
  type CopyState,
} from "../views/copy/copyFlow";
import { retargetHistory } from "../views/copy/copyModel";
import { saveReason, unchangedLine, gapLabel } from "../views/copy/copyText";
import type { ActiveGraph } from "../lib/types";
import { seriesGraph } from "./copyFixtures";

const graphs = new Map<number, ActiveGraph>([
  [1, seriesGraph("Clean Verse", 1)],
]);
const graphFor = (s: number) => graphs.get(s) ?? null;

function editing(): CopyState {
  const s = copyReducer(
    { ...INITIAL_COPY_STATE, fromSlot: 0, toSet: new Set([1]) },
    { type: "enterEdit", hist: retargetHistory(null, [1], graphFor) },
  );
  if (s.phase.kind !== "edit") throw new Error("edit expected");
  return s;
}

function uidAt(s: CopyState, i: number): string {
  const e = s.phase.draft.hist?.stack[s.phase.draft.hist.idx][1];
  const st = e?.graph.stages[0];
  if (st?.kind !== "series") throw new Error("series expected");
  return st.blocks[i].uid;
}

describe("copyReducer — the chosen block and the block menu", () => {
  it("enters Place blocks with nothing chosen and no menu open", () => {
    const s = editing();
    expect(s.phase.kind === "edit" && s.phase.pick).toBeNull();
    expect(s.phase.kind === "edit" && s.phase.open).toBeNull();
  });

  it("chooses a source block, and clears it on a second click", () => {
    let s = copyReducer(editing(), {
      type: "pickBlock",
      uid: "cb9",
      model: "ACD_DynaComp",
    });
    expect(s.phase.kind === "edit" && s.phase.pick).toEqual({
      uid: "cb9",
      model: "ACD_DynaComp",
    });
    s = copyReducer(s, {
      type: "pickBlock",
      uid: "cb9",
      model: "ACD_DynaComp",
    });
    expect(s.phase.kind === "edit" && s.phase.pick).toBeNull();
  });

  it("keeps the chosen block after placing it, and closes the menu", () => {
    let s = editing();
    const uid = uidAt(s, 1);
    s = copyReducer(s, { type: "pickBlock", uid: "x", model: "ACD_DynaComp" });
    s = copyReducer(s, { type: "tapBlock", slot: 1, uid });
    expect(s.phase.kind === "edit" && s.phase.open).toEqual({ slot: 1, uid });
    s = copyReducer(s, {
      type: "apply",
      slot: 1,
      uid,
      mode: "replace",
      model: "ACD_DynaComp",
    });
    expect(s.phase.kind === "edit" && s.phase.pick?.model).toBe("ACD_DynaComp");
    expect(s.phase.kind === "edit" && s.phase.open).toBeNull();
  });

  it("opens one menu at a time; the open block again closes it", () => {
    let s = editing();
    const a = uidAt(s, 0);
    const b = uidAt(s, 2);
    s = copyReducer(s, { type: "tapBlock", slot: 1, uid: a });
    s = copyReducer(s, { type: "tapBlock", slot: 1, uid: b });
    expect(s.phase.kind === "edit" && s.phase.open).toEqual({
      slot: 1,
      uid: b,
    });
    s = copyReducer(s, { type: "tapBlock", slot: 1, uid: b });
    expect(s.phase.kind === "edit" && s.phase.open).toBeNull();
    s = copyReducer(s, { type: "tapBlock", slot: 1, uid: a });
    s = copyReducer(s, { type: "closeMenu" });
    expect(s.phase.kind === "edit" && s.phase.open).toBeNull();
  });

  it("drops the choice on Back, so a new visit starts with nothing chosen", () => {
    let s = copyReducer(editing(), {
      type: "pickBlock",
      uid: "x",
      model: "ACD_DynaComp",
    });
    s = copyReducer(s, { type: "back" });
    s = copyReducer(s, {
      type: "enterEdit",
      hist: retargetHistory(s.phase.draft.hist, [1], graphFor),
    });
    expect(s.phase.kind === "edit" && s.phase.pick).toBeNull();
  });
});

describe("copyText — the save gate's words", () => {
  it("names one blocked preset, counts several, or asks for a change", () => {
    expect(
      saveReason([{ name: "Tweed", reason: "GlooperEffectsLimit" }], 1),
    ).toBe("Tweed: Only two Glooper effects fit in a preset.");
    expect(
      saveReason(
        [
          { name: "A", reason: "ProcessorUtilization" },
          { name: "B", reason: "FXLoopCoexistence" },
        ],
        2,
      ),
    ).toBe("2 presets can't be saved yet");
    expect(saveReason([], 0)).toBe("Add a block to a preset first");
    expect(saveReason([], 1)).toBeNull();
  });

  it("names the unchanged presets once", () => {
    expect(unchangedLine([])).toBeNull();
    expect(unchangedLine(["Tweed Warm"])).toBe(
      "Tweed Warm is unchanged, so it isn't saved.",
    );
  });

  it("says where a gap places the block", () => {
    expect(gapLabel("Verse", { before: "Greenbox 8" })).toBe(
      "Add here in Verse, before Greenbox 8",
    );
    expect(gapLabel("Verse", { after: "Small Hall" })).toBe(
      "Add here in Verse, after Small Hall",
    );
    expect(gapLabel("Verse", "end")).toBe("Add here in Verse, at the end");
  });
});
