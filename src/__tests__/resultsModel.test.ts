// src/__tests__/resultsModel.test.ts — what Doctor's Results page says, pure: the flat
// worst-first list and its groups, the titles (a stopped or partly failed check never
// claims "nothing to change in these sounds" — #190), every caption chosen by typed
// kind, the "Scenes" and "Footswitches" rows' words, and the "To sound like …" step (the
// EQ the chain already has, a new one, or — with no CPU room — one already there).

import { describe, it, expect } from "vitest";

import {
  buildResults,
  errorLine,
  footswitchLines,
  heardAt,
  matchAdvice,
  moreGoodLabel,
  ownNodeIdsFor,
  sceneLoudness,
  soundLabel,
  soundNameOf,
  touchesSharedBlock,
  type ResultsContext,
  type SoundName,
} from "../views/doctor/resultsModel";
import type {
  DoctorCheckResult,
  DoctorDiag,
  DoctorPresetResult,
  DoctorSoundResult,
  FootswitchInfo,
  GraphNode,
  SilenceHint,
} from "../lib/types";

const LABELS = ["Lows", "Low mids", "Mids", "High mids", "Highs", "Air"];

function sound(
  key: string,
  listIndex: number,
  over: Partial<DoctorSoundResult> = {},
): DoctorSoundResult {
  return {
    key,
    listIndex,
    scene: null,
    footswitch: null,
    label: key,
    tag: null,
    diags: [],
    status: "measured",
    integratedLufs: -20,
    tailRatioDb: -40,
    balanceDb: [0, 0, 0, 0, 0, 0],
    bandLabels: LABELS,
    cutThrough: null,
    error: null,
    errorKind: null,
    skippedBandCount: 0,
    ...over,
  };
}

function diag(
  key: string,
  sev: "high" | "med",
  z: number,
  over: Partial<DoctorDiag> = {},
): DoctorDiag {
  return {
    key,
    label: key[0].toUpperCase() + key.slice(1),
    sev,
    severity: z * 0.4,
    z,
    bands: [1],
    direction: "strong",
    detail: "",
    explain: "",
    rx: [],
    fromLevel: "quiet",
    ...over,
  };
}

function preset(
  listIndex: number,
  sounds: DoctorSoundResult[],
  over: Partial<DoctorPresetResult> = {},
): DoctorPresetResult {
  return {
    listIndex,
    sounds,
    sceneConsistency: null,
    levelingDamage: [],
    ...over,
  };
}

function ctx(
  names: Record<string, SoundName> = {},
  hints: [number, SilenceHint][] = [],
): ResultsContext {
  return {
    soundNames: new Map(Object.entries(names)),
    presetNames: new Map([
      [0, "Clean DI"],
      [1, "Tweed Warm"],
      [2, "Plexi Crunch"],
      [5, "Stadium Lead"],
    ]),
    silenceHintByIndex: new Map(hints),
  };
}

const result = (
  presets: DoctorPresetResult[],
  stopped = false,
): DoctorCheckResult => ({ presets, stopped });

describe("buildResults — order and groups", () => {
  it("sorts findings by the tone they show, then slot, then order inside the preset", () => {
    const m = buildResults(
      result([
        preset(5, [sound("p5", 5, { diags: [diag("muddy", "high", 10)] })]),
        preset(1, [
          sound("p1", 1, { diags: [diag("dark", "med", 8)] }),
          sound("s1:0", 1, { scene: 0, diags: [diag("harsh", "high", 9)] }),
        ]),
        preset(0, [sound("p0", 0)]),
      ]),
      ctx(),
    );
    expect(m.findings.map((r) => r.id)).toEqual(["1|s1:0", "5|p5", "1|p1"]);
    expect(m.clean.map((r) => r.id)).toEqual(["0|p0"]);
    expect(m.firstOpen).toBe("1|s1:0");
    expect(m.title).toBe("3 sounds need a look");
    expect(m.subtitle).toBe(
      "Worst first. Make the change on your Tone Master Pro, then check again.",
    );
    expect(m.foldClean).toBe(true);
    expect(m.allClear).toBe(false);
  });

  it("never sorts a borderline row above a row that shows real danger", () => {
    // Slot 0 only has a borderline high finding (caution); slot 5 a confident one.
    const m = buildResults(
      result([
        preset(0, [sound("p0", 0, { diags: [diag("muddy", "high", 1.2)] })]),
        preset(5, [sound("p5", 5, { diags: [diag("boomy", "high", 9)] })]),
      ]),
      ctx(),
    );
    expect(m.findings.map((r) => [r.id, r.mark])).toEqual([
      ["5|p5", "danger"],
      ["0|p0", "caution"],
    ]);
    expect(m.findings[1].caption).toBe("Muddy (borderline)");
  });

  it("lists a sound's findings in the order the row's tone follows", () => {
    const m = buildResults(
      result([
        preset(1, [
          sound("p1", 1, {
            diags: [diag("dark", "med", 30), diag("muddy", "high", 10)],
          }),
        ]),
      ]),
      ctx(),
    );
    expect(m.findings[0].caption).toBe("Muddy, Dark");
    expect(m.title).toBe("1 sound needs a look");
  });

  it("sorts preset-level rows in with the sounds, by their own tone", () => {
    const m = buildResults(
      result([
        preset(2, [sound("p2", 2, { diags: [diag("fizzy", "med", 9)] })], {
          sceneConsistency: {
            rows: [],
            worstName: "x",
            worstDeltaDb: 6.2,
            rx: [],
          },
          levelingDamage: [
            {
              switch: 8,
              label: "VERB KILL",
              nodeId: "n",
              fenderId: "f",
              parameterId: "mix",
              kind: "deletedEffect",
              detail: "",
            },
          ],
        }),
      ]),
      ctx(),
    );
    expect(m.findings.map((r) => [r.soundName, r.mark, r.caption])).toEqual([
      ["Scenes", "danger", "Volume jumps between scenes"],
      ["p2", "caution", "Fizzy"],
      ["Footswitches", "caution", "1 footswitch worth checking"],
    ]);
  });

  it("puts sounds that couldn't be checked, then sounds never reached, after the findings", () => {
    const m = buildResults(
      result(
        [
          preset(1, [
            sound("p1", 1, { diags: [diag("muddy", "high", 10)] }),
            sound("s1:0", 1, { status: "unrun", integratedLufs: null }),
            sound("s1:1", 1, {
              status: "error",
              error: "boom",
              errorKind: "other",
              integratedLufs: null,
            }),
          ]),
        ],
        true,
      ),
      ctx(),
    );
    expect(m.failed.map((r) => r.caption)).toEqual([
      "Couldn't check this sound. Check again.",
    ]);
    expect(m.unrun.map((r) => [r.caption, r.mark])).toEqual([
      ["Not checked", "waiting"],
    ]);
    expect(m.failed[0].expandable).toBe(false);
    expect(m.unrun[0].expandable).toBe(false);
  });
});

describe("buildResults — titles (#190)", () => {
  it("all clear: every sound checked, nothing to change", () => {
    const three = buildResults(
      result([preset(0, [sound("a", 0), sound("b", 0), sound("c", 0)])]),
      ctx(),
    );
    expect(three.title).toBe("Nothing to change in these 3 sounds");
    expect(three.subtitle).toBeNull();
    expect(three.allClear).toBe(true);
    expect(three.foldClean).toBe(false);
    const one = buildResults(result([preset(0, [sound("a", 0)])]), ctx());
    expect(one.title).toBe("Nothing to change in this sound");
  });

  it("a stopped check with nothing found says it stopped, and never claims all clear", () => {
    const m = buildResults(
      result(
        [
          preset(0, [
            sound("a", 0),
            sound("b", 0),
            sound("c", 0, { status: "unrun", integratedLufs: null }),
          ]),
        ],
        true,
      ),
      ctx(),
    );
    expect(m.title).toBe("Stopped. Nothing to change in the 2 sounds checked.");
    expect(m.title).not.toMatch(/in (these|this sound)/);
    expect(m.allClear).toBe(false);
    expect(m.subtitle).toBeNull();
  });

  it("a stop before the first sound says so", () => {
    const m = buildResults(
      result(
        [preset(0, [sound("a", 0, { status: "unrun", integratedLufs: null })])],
        true,
      ),
      ctx(),
    );
    expect(m.title).toBe("Stopped before any sound was checked");
    expect(m.allClear).toBe(false);
  });

  it("a finished check with a sound that couldn't be checked never claims all clear", () => {
    const m = buildResults(
      result([
        preset(0, [
          sound("a", 0),
          sound("b", 0, {
            status: "error",
            error: "x",
            errorKind: "other",
            integratedLufs: null,
          }),
        ]),
      ]),
      ctx(),
    );
    expect(m.title).toBe("Nothing to change in the 1 sound checked");
    expect(m.allClear).toBe(false);
    const none = buildResults(
      result([
        preset(0, [
          sound("a", 0, {
            status: "error",
            error: "x",
            errorKind: "other",
            integratedLufs: null,
          }),
        ]),
      ]),
      ctx(),
    );
    expect(none.title).toBe("Couldn't check this sound");
  });
});

describe("captions and names", () => {
  it("words a failed sound by its typed kind, refining a silent capture by the silence hint", () => {
    expect(errorLine("no_signal", "amp_zero")).toBe(
      "The amp's output is at zero, so there was nothing to hear.",
    );
    expect(errorLine("no_signal", "exp_mute")).toBe(
      "An expression pedal mutes this sound. Move it, then check again.",
    );
    expect(errorLine("no_signal", undefined)).toBe(
      "Nothing came out of USB 1/2, so there was nothing to hear.",
    );
    // A hint never refines any other kind — the kind decides, never the text.
    expect(errorLine("no_stimulus", "amp_zero")).toBe(
      "Couldn't check this sound. Check again.",
    );
    expect(errorLine(null, undefined)).toBe(
      "Couldn't check this sound. Check again.",
    );
  });

  it("reads the hint for the failed sound's own preset", () => {
    const m = buildResults(
      result([
        preset(2, [
          sound("p2", 2, {
            status: "error",
            // The backend's own words never reach the screen, whatever they say.
            error: "no signal captured",
            errorKind: "no_signal",
            integratedLufs: null,
          }),
        ]),
      ]),
      ctx({}, [[2, "amp_zero"]]),
    );
    expect(m.failed[0].caption).toBe(
      "The amp's output is at zero, so there was nothing to hear.",
    );
  });

  it("names sounds from the list's own words", () => {
    const names: Record<string, SoundName> = {
      p1: { preset: "Tweed Warm", name: "Main sound", footswitch: null },
      f1: { preset: "Tweed Warm", name: "Boost", footswitch: 3 },
    };
    expect(soundLabel(names.p1)).toBe("Main sound");
    expect(soundLabel(names.f1)).toBe("Boost, footswitch 3");
    const m = buildResults(
      result([
        preset(1, [
          sound("p1", 1, {
            label: "Tweed Warm · Base",
            diags: [diag("muddy", "high", 9)],
          }),
          sound("f1", 1, { footswitch: 2, diags: [diag("dark", "med", 9)] }),
        ]),
      ]),
      ctx(names),
    );
    expect(m.findings.map((r) => [r.presetName, r.soundName])).toEqual([
      ["Tweed Warm", "Main sound"],
      ["Tweed Warm", "Boost, footswitch 3"],
    ]);
  });

  it("takes a chosen sound's name verbatim from the list (`chosenFrom`)", () => {
    expect(
      soundNameOf({
        key: "p0",
        slot: 0,
        presetName: "Friedman HBE",
        isBase: true,
        sceneSlot: null,
        sceneName: "Main sound",
        tag: "BASE",
        hasScenes: true,
      }),
    ).toEqual({ preset: "Friedman HBE", name: "Main sound", footswitch: null });
  });

  it("says when a finding is heard, and folds clean sounds into one count", () => {
    expect(heardAt("quiet")).toBe("Heard at every volume");
    expect(heardAt("rehearsal")).toBe("Heard at rehearsal volume and up");
    expect(heardAt("stage")).toBe("Heard only at stage volume");
    expect(moreGoodLabel(4)).toBe("4 more sound good");
    expect(moreGoodLabel(1)).toBe("1 more sounds good");
  });
});

describe("the Scenes and Footswitches rows", () => {
  const p = preset(2, [
    sound("p2", 2, { label: "Plexi Crunch · Base" }),
    sound("s2:0", 2, { scene: 0, label: "Plexi Crunch · Intro" }),
    sound("s2:1", 2, { scene: 1, label: "Plexi Crunch · Chorus" }),
    sound("f2:0", 2, { footswitch: 4, label: "Plexi Crunch · Boost" }),
  ]);
  const names: Record<string, SoundName> = {
    p2: { preset: "Plexi Crunch", name: "Main sound", footswitch: null },
    "s2:0": { preset: "Plexi Crunch", name: "Intro", footswitch: null },
    "s2:1": { preset: "Plexi Crunch", name: "Chorus", footswitch: null },
    "f2:0": { preset: "Plexi Crunch", name: "Boost", footswitch: 5 },
  };
  const step = (title: string) => ({
    kind: "advisory" as const,
    title,
    detail: "",
    cpuNote: "",
    ops: [],
  });

  it("says how far the loudest scene sits from the main sound, naming each bar", () => {
    const out = sceneLoudness(ctx(names), p, {
      rows: [
        { name: "Plexi Crunch · Base", tag: "BASE", deltaDb: 0, isRef: true },
        {
          name: "Plexi Crunch · Chorus",
          tag: "FS2",
          deltaDb: 6.2,
          isRef: false,
        },
      ],
      worstName: "Plexi Crunch · Chorus",
      worstDeltaDb: 6.2,
      rx: [
        step(
          "Turn down the amp's Level in Chorus until the jump is 3 dB or less.",
        ),
      ],
    });
    expect(out.sentence).toBe(
      "Your Chorus scene is 6.2 dB louder than the main sound. That's a big jump when you switch.",
    );
    expect(out.bars.map((b) => b.label)).toEqual(["Main sound", "Chorus"]);
    expect(out.steps).toEqual([
      "Turn down the amp's Level in Chorus until the jump is 3 dB or less.",
      "Level this preset's scenes on the Level tab.",
    ]);
  });

  it("calls a louder first scene a reading that isn't certain, and adds no leveling step", () => {
    const sc = {
      rows: [
        { name: "Plexi Crunch · Base", tag: "BASE", deltaDb: 0, isRef: true },
        {
          name: "Plexi Crunch · Intro",
          tag: "FS1",
          deltaDb: 6.2,
          isRef: false,
        },
      ],
      worstName: "Plexi Crunch · Intro",
      worstDeltaDb: 6.2,
      rx: [
        step(
          "Couldn't confirm the reading for Intro. Check it by ear before you change it.",
        ),
      ],
    };
    const out = sceneLoudness(ctx(names), p, sc);
    expect(out.sentence).toBe(
      "Your Intro scene read 6.2 dB louder than the main sound, but that reading isn't certain.",
    );
    expect(out.steps).toEqual([
      "Couldn't confirm the reading for Intro. Check it by ear before you change it.",
    ]);
    // The dot and the bars keep the tones of the jump they show.
    expect(out.bars.map((b) => b.label)).toEqual(["Main sound", "Intro"]);
    const row = buildResults(
      result([preset(2, p.sounds, { sceneConsistency: sc })]),
      ctx(names),
    ).findings.find((r) => r.kind === "scenes");
    expect(row?.tone).toBe("danger");
    // A quieter first scene gets the backend's plain advice, so it reads as usual.
    const quieter = sceneLoudness(ctx(names), p, {
      ...sc,
      worstDeltaDb: -6.2,
      rx: [
        step(
          "If you didn't set it up that way, turn up the amp's Level in Intro.",
        ),
      ],
    });
    expect(quieter.sentence).toBe(
      "Your Intro scene is 6.2 dB quieter than the main sound. That's a big jump when you switch.",
    );
    expect(quieter.steps).toContain(
      "Level this preset's scenes on the Level tab.",
    );
  });

  it("says quieter for a negative jump, and names a footswitch by its number", () => {
    const out = sceneLoudness(ctx(names), p, {
      rows: [],
      worstName: "Plexi Crunch · Boost",
      worstDeltaDb: -4.5,
      rx: [],
    });
    expect(out.sentence).toBe(
      "Footswitch 5 is 4.5 dB quieter than the main sound. That's a big jump when you switch.",
    );
  });

  it("gives one line per footswitch, saying what its assignments do", () => {
    const hint = (
      sw: number,
      label: string,
      kind: "deletedEffect" | "sweptOther",
    ) => ({
      switch: sw,
      label,
      nodeId: "n",
      fenderId: "f",
      parameterId: "p",
      kind,
      detail: "",
    });
    expect(
      footswitchLines([
        hint(9, "WAH SWEEP", "sweptOther"),
        hint(8, "VERB KILL", "deletedEffect"),
        hint(8, "VERB KILL", "deletedEffect"),
        hint(2, "", "sweptOther"),
      ]),
    ).toEqual([
      {
        name: "Footswitch 3",
        does: ["Changes the tone, not only the volume, when you press it."],
      },
      {
        name: "VERB KILL",
        does: ["Turns the effect almost silent when you press it."],
      },
      {
        name: "WAH SWEEP",
        does: ["Changes the tone, not only the volume, when you press it."],
      },
    ]);
  });
});

describe("To sound like … (the EQ step)", () => {
  const AMP: GraphNode = {
    group_id: "G1",
    node_id: "amp1",
    model: "ACD_TweedDeluxe",
    bypassed: false,
    params: {},
  };
  const EQ: GraphNode = {
    group_id: "G1",
    node_id: "eq",
    model: "ACD_TenBandEQStereo",
    bypassed: false,
    params: {},
  };
  // Only Lows carries a delta (3 dB, ref − sound) → one move on the 62 Hz band.
  const reference = sound("ref", 0, { balanceDb: [3, 0, 0, 0, 0, 0] });
  const me = sound("me", 1, { balanceDb: [0, 0, 0, 0, 0, 0] });

  it("uses the 10-band EQ the chain already has", () => {
    expect(matchAdvice(me, reference, [AMP, EQ])?.step).toBe(
      "On your 10-band EQ, set 62 Hz +3 dB.",
    );
  });

  it("adds a 10-band EQ when there is CPU room for one", () => {
    expect(matchAdvice(me, reference, [AMP])?.step).toBe(
      "Add a 10-band EQ after the cab and set 62 Hz +3 dB.",
    );
    // A bypassed EQ doesn't count as one to use.
    expect(
      matchAdvice(me, reference, [AMP, { ...EQ, bypassed: true }])?.step,
    ).toBe("Add a 10-band EQ after the cab and set 62 Hz +3 dB.");
  });

  it("says when there is no CPU room for a new EQ", () => {
    const synth = (id: string): GraphNode => ({
      ...AMP,
      node_id: id,
      model: "ACD_GuitarSynth",
    });
    expect(
      matchAdvice(me, reference, [AMP, synth("s1"), synth("s2")])?.step,
    ).toBe(
      "There isn't enough CPU left for a 10-band EQ. To get closer, set 62 Hz +3 dB on an EQ you already have.",
    );
  });

  it("lists several moves, and says when an EQ can't close the gap", () => {
    const far = sound("far", 0, { balanceDb: [0, 9, 0, 0, -3, 0] });
    const out = matchAdvice(me, far, [AMP]);
    expect(out?.step).toMatch(
      /^Add a 10-band EQ after the cab and set .+ and .+\.$/,
    );
    expect(out?.residual).toBe(
      "EQ only gets part of the way. A different cab or amp gets closer.",
    );
  });

  it("has nothing to say for sounds that already match, or can't be compared", () => {
    expect(matchAdvice(me, sound("same", 0), [AMP])).toBeNull();
    const bassVi = sound("vi", 0, {
      bandLabels: ["Sub", ...LABELS],
      balanceDb: [0, 3, 0, 0, 0, 0, 0],
    });
    expect(matchAdvice(me, bassVi, [AMP])).toBeNull();
  });
});

describe("touchesSharedBlock", () => {
  const withFix = (nodeId: string) =>
    sound("f", 0, {
      footswitch: 2,
      diags: [
        diag("muddy", "high", 9, {
          rx: [
            {
              kind: "oneclick",
              title: "Turn it down.",
              detail: "",
              cpuNote: "",
              ops: [
                {
                  kind: "param",
                  groupId: "G1",
                  nodeId,
                  param: "bass",
                  value: 3,
                },
              ],
            },
          ],
        }),
      ],
    });

  it("is true only for a footswitch sound whose fix edits a block outside its own switch", () => {
    expect(touchesSharedBlock(withFix("amp"), ["drive"])).toBe(true);
    expect(touchesSharedBlock(withFix("drive"), ["drive"])).toBe(false);
    expect(touchesSharedBlock(withFix("amp"), undefined)).toBe(false);
  });
});

describe("ownNodeIdsFor", () => {
  const sw = (n: number, ...nodeIds: string[]): FootswitchInfo => ({
    switch: n,
    label: "",
    link_group: null,
    functions: nodeIds.map((id) => ({
      func: "on-off",
      group_id: "G1",
      node_id: id,
      fender_id: "ACD_Boost",
      parameter_id: null,
      value_a: null,
      value_b: null,
      is_active: false,
    })),
    level_params: [],
  });
  // The full roster: a switch with nothing to level comes first, so the sound's switch
  // number and its position in the levelable list point at different switches.
  const roster = [sw(0, "PHS1"), sw(2, "drive1", "delay1")];

  it("finds a footswitch sound's blocks by its switch number", () => {
    expect(ownNodeIdsFor(sound("f:0", 0, { footswitch: 2 }), roster)).toEqual([
      "drive1",
      "delay1",
    ]);
  });

  it("owns nothing for a main or scene sound, or a switch the roster lacks", () => {
    expect(ownNodeIdsFor(sound("p", 0), roster)).toBeUndefined();
    expect(
      ownNodeIdsFor(sound("s:0", 0, { scene: 0 }), roster),
    ).toBeUndefined();
    expect(
      ownNodeIdsFor(sound("f:1", 0, { footswitch: 5 }), roster),
    ).toBeUndefined();
    expect(
      ownNodeIdsFor(sound("f:0", 0, { footswitch: 2 }), undefined),
    ).toBeUndefined();
  });
});
