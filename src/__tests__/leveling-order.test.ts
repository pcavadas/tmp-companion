// Regression test for the leveling run-order invariant: within each preset, the Base
// option (which levels `presetLevel`, a global multiplier over that preset's scenes) MUST
// come before its FS scenes — leveling Base after a scene pass would shift every already-
// leveled scene off-target. `chosenFrom` is the source of that order; this pins it.

import { describe, expect, it } from "vitest";

import {
  baseKey,
  ceilingOf,
  chosenFrom,
  defaultParamIndex,
  footswitchName,
  fswKey,
  sceneKeyOf,
  targetFromCandidate,
} from "../views/level/leveling";
import type { RunItem } from "../views/level/leveling";
import type { PresetRow } from "../views/PresetList";
import type {
  FootswitchInfo,
  LevelParamCandidate,
  SceneInfo,
} from "../lib/types";
import { shortFallback } from "../models/blockArt";

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
        class: "level_linear",
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

  it("emits footswitches AFTER scenes, defaulting to its tone-safe candidate (D2)", () => {
    // Alpha (slot 0): 2 scenes + 1 footswitch (switch index 4 → tag FS5). Order must be
    // Base → scenes → footswitch. Every row levels now (D2 — the backend removed the
    // verify-only footswitch mode entirely): the default target is the switch's
    // (only, here) candidate, measured against the preset's BASE sound (D3's
    // `sceneContext: null`) until the Set up step's picker resolves a `suggested` scene.
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
        sceneContext: null,
      },
    });
  });

  it("footswitchName falls back to the toggled block's DEVICE name when the label is blank", () => {
    // A footswitch the player never named → the name the UNIT prints under that switch,
    // not a blank row. `ACD_TubeScreamer` reads "Greenbox 8" on the hardware; the old
    // fallback said "Tube Screamer", which is the pedal Fender EMULATES — a name the
    // device never shows anywhere (BUG→GATE 2026-08-20, see the strip-name gate below).
    const blank: FootswitchInfo = { ...fsw(4, ""), label: "" };
    blank.level_params = [
      { ...blank.level_params[0], fender_id: "ACD_TubeScreamer" },
    ];
    expect(footswitchName(blank)).toBe("Greenbox 8");
    // A named footswitch keeps the player's own label.
    expect(footswitchName(fsw(4, "Solo"))).toBe("Solo");
  });

  // BUG→GATE (2026-08-20 HW report, preset 30 "Plumes+BD2+OCD"): all four of that
  // preset's switches carry an empty `customLabel`, so every row fell back to the
  // de-camel-cased FenderId — "Blues Driver", "Obsessive Drive", "Rat" — while the unit's
  // scribble strips read "Sapphire OD", "CSD", "Rodent". The player could not match a row
  // to the switch under their foot. These are the reported blocks with their reported
  // strip names; if a future catalog regen drops `strip_name` (the `name8` half of the
  // display-name table the catalog already reads for `pro_control_name`), this fails.
  it.each([
    ["ACD_BluesDriver", "Sapphire OD"],
    ["ACD_ObsessiveDrive", "CSD"],
    ["ACD_Rat", "Rodent"],
    ["ACD_Plumes", "Pinions"],
  ])(
    "names an unlabeled switch on %s as the unit does: %s",
    (fid, expected) => {
      const f: FootswitchInfo = { ...fsw(4, ""), label: "" };
      f.level_params = [{ ...f.level_params[0], fender_id: fid }];
      expect(footswitchName(f)).toBe(expected);
    },
  );

  // The name is not "any candidate" — it follows the same tone-safe RANKED pick Set up
  // recommends (item 1: `defaultParamIndex` off the wire `class`), never the first
  // entry in array order. Load-bearing: this string is written on-device as the
  // switch's `customLabel`, so naming the wrong block is a wire write, not cosmetic.
  it("footswitchName names the rank-0 (level) block, not an earlier rank-1 (wet_mix) one", () => {
    const f: FootswitchInfo = {
      switch: 2,
      label: "",
      link_group: null,
      functions: [],
      level_params: [
        {
          group_id: "G1",
          node_id: "chorus",
          fender_id: "ACD_Chorus",
          parameter_id: "mix",
          current: 0.8,
          class: "wet_mix",
        },
        {
          group_id: "G1",
          node_id: "boost",
          fender_id: "ACD_Boost",
          parameter_id: "gain",
          current: 2.5,
          class: "level_db",
        },
      ],
    };
    expect(footswitchName(f)).toBe(shortFallback("ACD_Boost"));
  });

  // No classifiable level param at all (blank label too): name the switch after the
  // block its own function toggles/adjusts — never fall back to a `level_params[0]`
  // that doesn't exist.
  it("footswitchName falls back to the switch's function block when it has no level params", () => {
    const f: FootswitchInfo = {
      switch: 3,
      label: "",
      link_group: null,
      functions: [
        {
          func: "on-off",
          group_id: "G1",
          node_id: "tuner",
          fender_id: "ACD_Tuner",
          parameter_id: null,
          value_a: null,
          value_b: null,
          is_active: false,
        },
      ],
      level_params: [],
    };
    expect(footswitchName(f)).toBe(shortFallback("ACD_Tuner"));
  });

  // A footswitch acting on a block with three levelable params. Alphabetical order is
  // [gain, level, tone] — so the OLD `[0]` default landed on `gain`, the bug the picker
  // fixes. `gain`/`tone` stand in for a lower-ranked `wet_mix` candidate here (the wire
  // never carries "other" — see leveling.ts's `defaultParamIndex`); the tone-safe
  // default is `level` (`level_linear`, rank 0).
  function fswMulti(sw: number, label: string): FootswitchInfo {
    const at = (
      parameter_id: string,
      current: number,
      cls: LevelParamCandidate["class"],
    ) => ({
      group_id: "G1",
      node_id: `N${String(sw)}`,
      fender_id: "ACD_BluesDriver",
      parameter_id,
      current,
      class: cls,
    });
    return {
      switch: sw,
      label,
      link_group: null,
      functions: [],
      level_params: [
        at("gain", 0.4, "wet_mix"),
        at("level", 0.6, "level_linear"),
        at("tone", 0.5, "wet_mix"),
      ],
    };
  }

  it("defaultParamIndex prefers a rank-0 (level) param over an alphabetically-earlier rank-1 (wet_mix) one", () => {
    const f = fswMulti(0, "Drive");
    expect(defaultParamIndex(f.level_params)).toBe(1); // level, not gain[0]
    expect(f.level_params[defaultParamIndex(f.level_params)].parameter_id).toBe(
      "level",
    );
  });

  // The only remaining -1 case: an EMPTY candidate list. A non-empty list always has a
  // valid default now — every candidate the backend offers already carries a real
  // (never "other") class (item 1: `class` ships on the wire, gated by
  // `footswitch::level_candidates_for_node`), so there is no "unclassifiable-only" list
  // left to construct.
  it("defaultParamIndex returns -1 for an empty candidate list", () => {
    expect(defaultParamIndex([])).toBe(-1);
  });

  // A Lightspeed compressor's own param is literally named "loudness" — without it in
  // LOUDNESS_PARAMS, defaultParamIndex fell back to the alphabetically-first candidate
  // (drive, a tone knob) and the run row was named after it instead of "Loudness".
  it("defaultParamIndex prefers loudness over an alphabetically-earlier drive param", () => {
    const at = (
      parameter_id: string,
      current: number,
      cls: LevelParamCandidate["class"],
    ) => ({
      group_id: "G1",
      node_id: "N0",
      fender_id: "ACD_Lightspeed",
      parameter_id,
      current,
      class: cls,
    });
    const params = [
      at("drive", 0.6, "wet_mix"),
      at("loudness", 0.5, "level_linear"),
    ];
    expect(defaultParamIndex(params)).toBe(1);
    expect(params[defaultParamIndex(params)].parameter_id).toBe("loudness");
  });

  it("chosenFrom defaults a footswitch to its tone-safe candidate + a base scene context", () => {
    const fswInfo = new Map<number, FootswitchInfo[]>([
      [0, [fswMulti(4, "Solo")]],
    ]);
    const out = chosenFrom(new Set([fswKey(0, 0)]), rows, sceneInfo, fswInfo);
    expect(out).toHaveLength(1);
    // Every row levels now (D2 — the backend removed the verify-only footswitch mode
    // entirely) — the default target is the tone-safe candidate (LEVEL, not
    // alphabetical-first GAIN), measured against the preset's BASE sound (D3's
    // `sceneContext: null`) until the Set up step's picker resolves a `suggested` scene.
    expect(out[0].footswitch).toEqual({
      switchIndex: 4,
      levGroupId: "G1",
      levNodeId: "N4",
      levParameterId: "level",
      sceneContext: null,
    });
    // The full candidate list survives for the Set up picker to offer.
    expect(out[0].levelParams?.map((c) => c.parameter_id)).toEqual([
      "gain",
      "level",
      "tone",
    ]);
  });

  it("targetFromCandidate builds coords + scene context from any chosen candidate (user override to gain)", () => {
    const gain = fswMulti(4, "Solo").level_params[0];
    expect(targetFromCandidate(4, 2, gain)).toEqual({
      switchIndex: 4,
      levGroupId: "G1",
      levNodeId: "N4",
      levParameterId: "gain",
      sceneContext: 2,
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
    expect(out[0]).toMatchObject({ isBase: true, sceneName: "Base Preset" });
    expect(out[1]).toMatchObject({ tag: "FS1", sceneName: "Drive" });
  });
});

describe("ceilingOf", () => {
  function baseRunItem(over: Partial<RunItem>): RunItem {
    return {
      key: "p0",
      slot: 0,
      presetName: "Test",
      isBase: true,
      sceneSlot: null,
      sceneName: "Base Preset",
      tag: null,
      instId: "",
      targetName: "Rhythm",
      status: "result",
      ...over,
    };
  }

  // Preset/scene clamps are top-rail only (LEVEL_MIN=0, ideal=10^x>0 is always
  // reachable from below) — a clamped row's measured value genuinely IS its ceiling.
  it("a clamped Base row's value is its ceiling", () => {
    const it_ = baseRunItem({ outcome: "clamped", value: -18.5 });
    expect(ceilingOf(it_)).toBe(-18.5);
  });

  // measure_footswitch's clamp is direction-agnostic (a switch can clamp because
  // it's too LOUD, not just too quiet) — treating its value as a ceiling would feed
  // a FLOOR into min(ceiling) and drag the whole library's common target down. A
  // clamped footswitch row falls back to ceilingLufs instead (undefined here → null).
  it("a clamped FOOTSWITCH row does NOT use value as its ceiling", () => {
    const it_ = baseRunItem({
      isBase: false,
      footswitch: {
        switchIndex: 0,
        levGroupId: "G1",
        levNodeId: "N0",
        levParameterId: "loudness",
        sceneContext: null,
      },
      outcome: "clamped",
      value: -6.0, // e.g. clamped because it's too LOUD
      ceilingLufs: -14,
    });
    expect(ceilingOf(it_)).toBe(-14);
  });

  // A done row (not clamped) always reads ceilingLufs, footswitch or not.
  it("a done row reads ceilingLufs regardless of footswitch", () => {
    const it_ = baseRunItem({ outcome: "done", value: -22, ceilingLufs: -14 });
    expect(ceilingOf(it_)).toBe(-14);
  });
});
