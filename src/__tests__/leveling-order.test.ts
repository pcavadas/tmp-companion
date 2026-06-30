// Regression test for the leveling run-order invariant: within each preset, the Base
// option (which levels `presetLevel`, a global multiplier over that preset's scenes) MUST
// come before its FS scenes — leveling Base after a scene pass would shift every already-
// leveled scene off-target. `chosenFrom` is the source of that order; this pins it.

import { describe, expect, it } from "vitest";

import {
  baseKey,
  chosenFrom,
  defaultParamIndex,
  footswitchName,
  fswKey,
  sceneKeyOf,
  targetFromCandidate,
} from "../views/level/leveling";
import type { PresetRow } from "../views/PresetList";
import type { FootswitchInfo, SceneInfo } from "../lib/types";

const rows: PresetRow[] = [
  { slot: 0, name: "Alpha", empty: false },
  { slot: 1, name: "Bravo", empty: false },
];
const sceneInfo = new Map<number, SceneInfo[]>([
  [
    0,
    [
      { name: "Verse", fs: 1 },
      { name: "Chorus", fs: 2 },
    ],
  ],
  [1, []], // Bravo is scene-less
]);
const noFsw = new Map<number, FootswitchInfo[]>();

function fsw(sw: number, label: string): FootswitchInfo {
  return {
    switch: sw,
    label,
    link_group: null,
    functions: [],
    level_params: [
      {
        group_id: "G1",
        node_id: `N${String(sw)}`,
        fender_id: "ACD_BluesDriver",
        parameter_id: "gain",
        current: 0.5,
      },
    ],
  };
}

describe("chosenFrom run-order", () => {
  it("emits each preset's Base before its FS scenes", () => {
    // Select Alpha's two scenes FIRST in the set, then its Base — order must NOT follow
    // insertion: chosenFrom always lists Base ahead of the preset's scenes.
    const sel = new Set([sceneKeyOf(0, 1), sceneKeyOf(0, 0), baseKey(0)]);
    const out = chosenFrom(sel, rows, sceneInfo, noFsw);
    expect(out.map((o) => o.key)).toEqual([
      baseKey(0),
      sceneKeyOf(0, 0),
      sceneKeyOf(0, 1),
    ]);
    expect(out[0].isBase).toBe(true);
  });

  it("keeps each preset's Base ahead of its own scenes across presets", () => {
    const sel = new Set([
      baseKey(1), // Bravo (scene-less)
      sceneKeyOf(0, 0), // Alpha scene
      baseKey(0), // Alpha base
    ]);
    const out = chosenFrom(sel, rows, sceneInfo, noFsw);
    // Per preset (sorted by slot): Alpha base → Alpha scene, then Bravo base.
    expect(out.map((o) => o.key)).toEqual([
      baseKey(0),
      sceneKeyOf(0, 0),
      baseKey(1),
    ]);
    // Within every preset, the Base index precedes that preset's scene indices.
    for (const r of rows) {
      const baseIdx = out.findIndex((o) => o.slot === r.slot && o.isBase);
      const sceneIdxs = out
        .map((o, i) => ({ o, i }))
        .filter(({ o }) => o.slot === r.slot && !o.isBase)
        .map(({ i }) => i);
      for (const si of sceneIdxs) expect(baseIdx).toBeLessThan(si);
    }
  });

  it("renders a scene-less preset's Base as the whole preset", () => {
    const out = chosenFrom(new Set([baseKey(1)]), rows, sceneInfo, noFsw);
    expect(out).toHaveLength(1);
    expect(out[0]).toMatchObject({
      isBase: true,
      hasScenes: false,
      sceneName: "Whole preset",
      tag: null,
    });
  });

  it("emits footswitches AFTER scenes, carrying their leveling target", () => {
    // Alpha (slot 0): 2 scenes + 1 footswitch (switch index 4 → tag FS5). Order must be
    // Base → scenes → footswitch, and the footswitch row carries its solve coords.
    const fswInfo = new Map<number, FootswitchInfo[]>([[0, [fsw(4, "Solo")]]]);
    const sel = new Set([
      fswKey(0, 0),
      sceneKeyOf(0, 0),
      sceneKeyOf(0, 1),
      baseKey(0),
    ]);
    const out = chosenFrom(sel, rows, sceneInfo, fswInfo);
    expect(out.map((o) => o.key)).toEqual([
      baseKey(0),
      sceneKeyOf(0, 0),
      sceneKeyOf(0, 1),
      fswKey(0, 0),
    ]);
    expect(out[3]).toMatchObject({
      isBase: false,
      sceneName: "Solo",
      tag: "FS5", // switch index 4 → human FS number 5
      footswitch: {
        switchIndex: 4,
        levGroupId: "G1",
        levNodeId: "N4",
        levParameterId: "gain",
      },
    });
  });

  it("footswitchName falls back to the toggled block's name when the label is blank", () => {
    // A footswitch the player never named → the block's friendly name, not a blank row.
    const blank: FootswitchInfo = { ...fsw(4, ""), label: "" };
    blank.level_params = [
      { ...blank.level_params[0], fender_id: "ACD_TubeScreamer" },
    ];
    expect(footswitchName(blank)).toBe("Tube Screamer");
    // A named footswitch keeps the player's own label.
    expect(footswitchName(fsw(4, "Solo"))).toBe("Solo");
  });

  // A footswitch acting on a block with three levelable params. Alphabetical order is
  // [gain, level, tone] — so the OLD `[0]` default landed on `gain` (a TONE knob), the
  // bug the picker fixes. The tone-safe default is `level` (loudness only).
  function fswMulti(sw: number, label: string): FootswitchInfo {
    const at = (parameter_id: string, current: number) => ({
      group_id: "G1",
      node_id: `N${String(sw)}`,
      fender_id: "ACD_BluesDriver",
      parameter_id,
      current,
    });
    return {
      switch: sw,
      label,
      link_group: null,
      functions: [],
      level_params: [at("gain", 0.4), at("level", 0.6), at("tone", 0.5)],
    };
  }

  it("defaultParamIndex prefers a loudness param over an alphabetically-earlier tone param", () => {
    const f = fswMulti(0, "Drive");
    expect(defaultParamIndex(f.level_params)).toBe(1); // level, not gain[0]
    expect(f.level_params[defaultParamIndex(f.level_params)].parameter_id).toBe(
      "level",
    );
  });

  it("defaultParamIndex falls back to the first candidate when none is loudness-only", () => {
    const toneOnly = fswMulti(0, "Drive").level_params.filter(
      (c) => c.parameter_id !== "level",
    );
    expect(defaultParamIndex(toneOnly)).toBe(0); // [gain, tone] → first
  });

  it("chosenFrom defaults a footswitch to its loudness param and carries the full candidate list", () => {
    const fswInfo = new Map<number, FootswitchInfo[]>([
      [0, [fswMulti(4, "Solo")]],
    ]);
    const out = chosenFrom(new Set([fswKey(0, 0)]), rows, sceneInfo, fswInfo);
    expect(out).toHaveLength(1);
    // Default leveling target is the LOUDNESS param (level), NOT alphabetical-first (gain).
    expect(out[0].footswitch?.levParameterId).toBe("level");
    // The full candidate list survives for the Set up picker to offer.
    expect(out[0].levelParams?.map((c) => c.parameter_id)).toEqual([
      "gain",
      "level",
      "tone",
    ]);
  });

  it("targetFromCandidate builds coords from any chosen candidate (user override to gain)", () => {
    const gain = fswMulti(4, "Solo").level_params[0];
    expect(targetFromCandidate(4, gain)).toEqual({
      switchIndex: 4,
      levGroupId: "G1",
      levNodeId: "N4",
      levParameterId: "gain",
    });
  });

  it("scene-less preset with only a footswitch keeps Base as the whole preset", () => {
    // Bravo (slot 1) has no scenes but one footswitch: Base is still "Whole preset"
    // (footswitches read like scenes but the Base is the cross-preset essential).
    const fswInfo = new Map<number, FootswitchInfo[]>([[1, [fsw(0, "Drive")]]]);
    const out = chosenFrom(
      new Set([baseKey(1), fswKey(1, 0)]),
      rows,
      sceneInfo,
      fswInfo,
    );
    expect(out.map((o) => o.key)).toEqual([baseKey(1), fswKey(1, 0)]);
    expect(out[0]).toMatchObject({ isBase: true, sceneName: "Base" });
    expect(out[1]).toMatchObject({ tag: "FS1", sceneName: "Drive" });
  });
});
