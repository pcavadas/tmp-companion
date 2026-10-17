// src/__tests__/setupControls.test.ts — Set up's Block and Knob dropdowns, render-free
// (`setupControls.ts`). Ported from FlatLevelPick.test.tsx (the one combined picker this
// pair replaced) and extended to the look B defaults.
//
// The DANGER rule this guards (danger.md's Pick trap): a dropdown NEVER shows a value
// other than the one the run will use. So the default shows what it drives (Block
// "Preset" / Knob "Level" for a main sound; the amp a scene drives, "Both amps" where two
// lanes are driven together), a scene the run can't drive by default shows an EMPTY Block,
// and a stored knob the read list no longer offers shows its OWN name — never the
// first option — with `stale` set so the row can say so. "Not read yet" is not "gone":
// an unread list never flags a stored knob stale.

import { describe, expect, it } from "vitest";

import {
  DEFAULT_CONTROL,
  controlModel,
  knobNote,
  sceneDefault,
  targetByName,
} from "../views/level/setupControls";
import type { BlockLevelCandidate } from "../views/level/blockLevelGroups";
import type { CandidateFetch } from "../views/level/setupCandidates";
import { blockStripName } from "../views/level/leveling";
import type { SceneDrive } from "../lib/types";

const twinOutput: BlockLevelCandidate = {
  groupId: "G1",
  nodeId: "amp",
  fenderId: "ACD_TwinReverb65NoFx",
  parameterId: "outputLevel",
  paramClass: "level_linear",
};
const twinMaster: BlockLevelCandidate = {
  ...twinOutput,
  parameterId: "master",
  paramClass: "wet_mix",
};
const twinGain: BlockLevelCandidate = {
  ...twinOutput,
  parameterId: "gain",
  paramClass: "level_db",
  lowersOnly: true,
};
const boostGain: BlockLevelCandidate = {
  groupId: "G1",
  nodeId: "boost1",
  fenderId: "ACD_Boost",
  parameterId: "gain",
  paramClass: "level_db",
};
const boostTone: BlockLevelCandidate = {
  ...boostGain,
  parameterId: "toneKnob",
  paramClass: undefined,
};
const sharedBoost: BlockLevelCandidate = {
  ...boostGain,
  disabled: true,
  disabledTitle: "Shared with the main sound",
};

const read = (...list: BlockLevelCandidate[]): CandidateFetch => ({
  status: "resolved",
  list,
});
const TWIN = blockStripName("ACD_TwinReverb65NoFx");
const BOOST = blockStripName("ACD_Boost");

const drive = (over: Partial<SceneDrive>): SceneDrive => ({
  sceneSlot: 0,
  amps: [],
  routing: "single",
  skip: null,
  ...over,
});
const twinAmp = {
  groupId: "G1",
  nodeId: "amp",
  fenderId: "ACD_TwinReverb65NoFx",
};

describe("main sound: the preset's own level is the default", () => {
  it('shows Block "Preset" and Knob "Level", then the preset\'s blocks', () => {
    const m = controlModel(read(twinOutput, boostGain), null, {
      kind: "preset",
    });
    expect(m.block.value).toBe(DEFAULT_CONTROL);
    expect(m.block.options.map((o) => o.label)).toEqual([
      "Preset",
      TWIN,
      BOOST,
    ]);
    expect(m.knob.value).toBe(DEFAULT_CONTROL);
    expect(m.knob.options).toEqual([
      { value: DEFAULT_CONTROL, label: "Level", note: "Volume only, best" },
    ]);
    expect(m.needsBlock).toBe(false);
    expect(m.stale).toBe(false);
  });

  it("picking a block lands on its best knob; picking Preset goes back to the default", () => {
    const m = controlModel(read(boostTone, boostGain), null, {
      kind: "preset",
    });
    expect(m.pickBlock("G1:boost1")).toEqual({
      groupId: "G1",
      nodeId: "boost1",
      parameterId: "gain",
    });
    expect(m.pickBlock(DEFAULT_CONTROL)).toBeNull();
  });

  it("a chosen knob shows its block and its own knob list", () => {
    const m = controlModel(
      read(twinOutput, twinMaster, twinGain),
      { groupId: "G1", nodeId: "amp", parameterId: "master" },
      { kind: "preset" },
    );
    expect(m.block.value).toBe("G1:amp");
    expect(m.knob.value).toBe("master");
    expect(m.knob.options.map((o) => [o.label, o.note])).toEqual([
      ["Output level", "Volume only, best"],
      ["Gain", "Can only turn down"],
      ["Master", "Also changes the tone"],
    ]);
    expect(m.pickKnob("gain")).toEqual({
      groupId: "G1",
      nodeId: "amp",
      parameterId: "gain",
    });
  });
});

describe("the DANGER-rule guard for a stored knob", () => {
  const gone = { groupId: "G9", nodeId: "fuzz7", parameterId: "level" };

  it("a knob the read list no longer has shows its own name and is flagged", () => {
    const m = controlModel(read(twinOutput), gone, { kind: "preset" });
    expect(m.stale).toBe(true);
    // The value is not an option, so the dropdown shows the stored name — never the
    // first option ("Preset" / "Level").
    expect(m.block.value).toBe("G9:fuzz7");
    expect(m.block.options.some((o) => o.value === "G9:fuzz7")).toBe(false);
    expect(m.block.staleLabel).toBe("fuzz7");
    expect(m.knob.value).toBe("level");
    expect(m.knob.staleLabel).toBe("Level");
  });

  it("an unread list never flags a stored knob (not read yet is not gone)", () => {
    for (const status of ["unfetched", "loading", "error"] as const) {
      const m = controlModel({ status }, gone, { kind: "preset" });
      expect(m.stale).toBe(false);
    }
  });

  it("while the list is read, the default still shows and the open list says so", () => {
    const m = controlModel({ status: "loading" }, null, { kind: "preset" });
    expect(m.block.value).toBe(DEFAULT_CONTROL);
    expect(m.block.loading).toBe(true);
    const failed = controlModel({ status: "error" }, null, { kind: "preset" });
    expect(failed.block.value).toBe(DEFAULT_CONTROL);
    expect(failed.block.listNote).toBe("Couldn't read this preset's knobs.");
  });
});

describe("knob notes", () => {
  it("names the block's best knob, tone-changing and turn-down-only knobs", () => {
    expect(knobNote(twinOutput, twinOutput)).toBe("Volume only, best");
    expect(knobNote(twinMaster, twinMaster)).toBe(
      "Best here, also changes the tone",
    );
    expect(knobNote(twinMaster, twinOutput)).toBe("Also changes the tone");
    expect(knobNote(twinGain, twinOutput)).toBe("Can only turn down");
    // Even the block's best knob says so: a sound that must get louder can't with it.
    expect(knobNote(twinGain, twinGain)).toBe("Can only turn down");
    expect(knobNote(sharedBoost, twinOutput)).toBe(
      "Shared with the main sound",
    );
  });

  it("a disabled knob is offered but not choosable", () => {
    const m = controlModel(
      read(twinOutput, sharedBoost, boostTone),
      { groupId: "G1", nodeId: "boost1", parameterId: "toneKnob" },
      { kind: "preset" },
    );
    const shared = m.knob.options.find((o) => o.value === "gain");
    expect(shared).toMatchObject({
      disabled: true,
      note: "Shared with the main sound",
    });
    expect(m.pickKnob("gain")).toBeUndefined();
  });

  it("a block whose every knob is disabled is offered disabled and can't be picked (was recommendedBlock's BUG FIX)", () => {
    const m = controlModel(read(sharedBoost, twinOutput), null, {
      kind: "preset",
    });
    const boost = m.block.options.find((o) => o.value === "G1:boost1");
    expect(boost).toMatchObject({
      disabled: true,
      note: "Shared with the main sound",
    });
    expect(m.pickBlock("G1:boost1")).toBeUndefined();
  });
});

describe("scene: the amp the run drives is the default", () => {
  it('one amp: Block names it, Knob "Output level"; its other knobs sit under it', () => {
    const def = sceneDefault(drive({ amps: [twinAmp] }));
    expect(def).toEqual({
      kind: "amp",
      label: TWIN,
      ampKey: "G1:amp",
      ampKeys: ["G1:amp"],
    });
    const m = controlModel(read(twinOutput, twinMaster, boostGain), null, def);
    // The amp shows once — as the default, not again as a block of its own.
    expect(m.block.options.map((o) => o.label)).toEqual([TWIN, BOOST]);
    expect(m.knob.options.map((o) => o.label)).toEqual([
      "Output level",
      "Master",
    ]);
    expect(m.pickKnob("master")).toEqual({
      groupId: "G1",
      nodeId: "amp",
      parameterId: "master",
    });
    expect(m.pickKnob(DEFAULT_CONTROL)).toBeNull();
    // The amp's own Master, once chosen, still reads under the amp's Block option.
    const chosen = controlModel(
      read(twinOutput, twinMaster),
      { groupId: "G1", nodeId: "amp", parameterId: "master" },
      def,
    );
    expect(chosen.block.value).toBe(DEFAULT_CONTROL);
    expect(chosen.knob.value).toBe("master");
  });

  it('an amp already at the top of its output says "Can only turn down"', () => {
    const topped = { ...twinOutput, lowersOnly: true };
    const one = controlModel(
      read(topped),
      null,
      sceneDefault(drive({ amps: [twinAmp] })),
    );
    expect(one.knob.options[0]).toMatchObject({
      label: "Output level",
      note: "Can only turn down",
    });
    const both = controlModel(
      read(topped, { ...twinOutput, nodeId: "amp2" }),
      null,
      sceneDefault(drive({ amps: [twinAmp, { ...twinAmp, nodeId: "amp2" }] })),
    );
    expect(both.knob.options[0].note).toBe("Can only turn down");
  });

  it('two amps driven together read "Both amps", three "All 3 amps"', () => {
    const second = { ...twinAmp, nodeId: "amp2" };
    expect(sceneDefault(drive({ amps: [twinAmp, second] }))).toEqual({
      kind: "amp",
      label: "Both amps",
      ampKey: null,
      ampKeys: ["G1:amp", "G1:amp2"],
    });
    expect(
      sceneDefault(
        drive({ amps: [twinAmp, second, { ...twinAmp, nodeId: "amp3" }] }),
      ),
    ).toMatchObject({ label: "All 3 amps" });
  });

  it("no drive read for the scene keeps the amp default under a plain name", () => {
    expect(sceneDefault(undefined)).toEqual({
      kind: "amp",
      label: "Amp",
      ampKey: null,
      ampKeys: [],
    });
  });

  it("a scene with no amp shows an empty Block and waits for a choice", () => {
    const def = sceneDefault(drive({ skip: "no_active_amp" }));
    expect(def).toEqual({ kind: "none", reason: "no_active_amp" });
    const m = controlModel(read(boostGain), null, def);
    expect(m.block.value).toBe("");
    expect(m.block.options.map((o) => o.label)).toEqual([BOOST]);
    expect(m.knob.value).toBe("");
    expect(m.knob.disabled).toBe(true);
    expect(m.needsBlock).toBe(true);
    expect(m.nothingToChoose).toBe(false);
    // There is no default to go back to.
    expect(m.pickBlock(DEFAULT_CONTROL)).toBeUndefined();
    expect(m.pickBlock("G1:boost1")).toMatchObject({ parameterId: "gain" });
  });

  it("a scene with no amp and nothing usable at all: nothing to choose, so nothing to wait for", () => {
    const m = controlModel(
      read(sharedBoost),
      null,
      sceneDefault(drive({ skip: "level_manually" })),
    );
    expect(m.needsBlock).toBe(true);
    expect(m.nothingToChoose).toBe(true);
    expect(m.block.disabled).toBe(true);
  });
});

describe("footswitch: no default, always a real knob", () => {
  it("shows the switch's own block and knob", () => {
    const m = controlModel(
      read(boostGain),
      { groupId: "G1", nodeId: "boost1", parameterId: "gain" },
      { kind: "none" },
    );
    expect(m.block.value).toBe("G1:boost1");
    expect(m.knob.value).toBe("gain");
    expect(m.needsBlock).toBe(false);
  });
});

describe("targetByName", () => {
  const names = ["Rhythm", "Crunch", "Lead", "Lead Boost"];

  it("gives a sound the target its name starts with, ignoring case", () => {
    expect(targetByName("Lead 2", names)).toBe("Lead");
    expect(targetByName("crunchy", names)).toBe("Crunch");
  });

  it("prefers the longest matching name", () => {
    expect(targetByName("Lead Boost 2", names)).toBe("Lead Boost");
  });

  it("falls back to the FIRST configured target, never a fixed name", () => {
    expect(targetByName("Main sound", names)).toBe("Rhythm");
    expect(targetByName("Main sound", ["Quiet", "Loud"])).toBe("Quiet");
    expect(targetByName("Main sound", [])).toBe("");
  });
});
