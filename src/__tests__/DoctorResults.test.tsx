// src/__tests__/DoctorResults.test.tsx — Doctor's Results page (look B): one flat list
// worst first, the worst sound open on arrival with its findings (name, when it's heard,
// explanation, band meter, "What to change" in the backend's order), rows with no
// loudness, band picture or venue icons, the folded clean row, the "Scenes" and
// "Footswitches" rows, errors worded by typed kind, the reference and "To sound like …",
// the wide window's side panel, and the footer. Doctor never writes: no command is sent
// from this page. The stopped-check titles are the #190 gate.

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { DoctorResults } from "../views/doctor/DoctorResults";
import {
  DOCTOR_STEPS,
  DOCTOR_STEPS_SKIPPED,
} from "../views/doctor/useDoctorFlow";
import type { ResultsContext, SoundName } from "../views/doctor/resultsModel";
import type {
  DoctorCheckResult,
  DoctorDiag,
  DoctorInputArg,
  DoctorPresetResult,
  DoctorSoundResult,
  FootswitchInfo,
  GraphNode,
  SilenceHint,
} from "../lib/types";
import { renderWithTheme } from "./testKit";

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
    integratedLufs: -18,
    tailRatioDb: -40,
    balanceDb: [-6, 4, -2, -8, -12, -18],
    bandLabels: LABELS,
    cutThrough: { contrastDb: 2, factoryPercentile: 62.4, advisory: false },
    error: null,
    errorKind: null,
    skippedBandCount: 0,
    ...over,
  };
}

const MUDDY: DoctorDiag = {
  key: "muddy",
  label: "Muddy",
  sev: "high",
  severity: 4.2,
  z: 10.5,
  bands: [1],
  direction: "strong",
  detail: "+4.2 dB around 250 Hz",
  fromLevel: "quiet",
  explain: "The low mids build up and stack with the bass player.",
  rx: [
    {
      kind: "advisory",
      title: "Turn the amp's Bass down 1 or 2.",
      detail: "",
      cpuNote: "",
      ops: [],
    },
    {
      kind: "chain",
      title: "Or add a 10-band EQ after the cab and cut the low mids.",
      detail: "EQ-10 250 Hz −6 dB",
      cpuNote: "+3.1% CPU",
      ops: [],
    },
  ],
};

const DARK: DoctorDiag = {
  key: "dark",
  label: "Dark",
  sev: "med",
  severity: 1,
  z: 20,
  bands: [3, 4, 5],
  direction: "weak",
  detail: "tilted 5.0 dB/octave darker",
  fromLevel: "rehearsal",
  explain: "The whole tone leans dark, so it sounds dull and boxed in.",
  rx: [
    {
      kind: "advisory",
      title: "Raise the amp's Treble or Presence.",
      detail: "",
      cpuNote: "",
      ops: [],
    },
  ],
};

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

const NAMES: Record<string, SoundName> = {
  p0: { preset: "Clean DI", name: "Main sound", footswitch: null },
  p1: { preset: "Tweed Warm", name: "Main sound", footswitch: null },
  "s1:0": { preset: "Tweed Warm", name: "Lead", footswitch: null },
  "s1:1": { preset: "Tweed Warm", name: "Lead", footswitch: null },
  p2: { preset: "Broken Lead", name: "Main sound", footswitch: null },
};

function ctx(hints: [number, SilenceHint][] = []): ResultsContext {
  return {
    soundNames: new Map(Object.entries(NAMES)),
    presetNames: new Map([
      [0, "Clean DI"],
      [1, "Tweed Warm"],
      [2, "Broken Lead"],
    ]),
    silenceHintByIndex: new Map(hints),
  };
}

// One clean preset, one with a confident Muddy + medium Dark on its main sound and a
// medium Dark on its Lead scene, and one whose sound couldn't be checked.
function fixture(stopped = false): DoctorCheckResult {
  return {
    presets: [
      preset(0, [sound("p0", 0)]),
      preset(1, [
        sound("p1", 1, { diags: [DARK, MUDDY] }),
        sound("s1:0", 1, { scene: 0, diags: [{ ...DARK, z: 9 }] }),
      ]),
      preset(2, [
        sound("p2", 2, {
          status: "error",
          error: "The capture came back silent — check the cable.",
          errorKind: "other",
          integratedLufs: null,
          cutThrough: null,
        }),
      ]),
    ],
    stopped,
  };
}

interface RenderOpts {
  hints?: [number, SilenceHint][];
  inputs?: DoctorInputArg[];
  steps?: readonly string[];
}

function renderResults(result: DoctorCheckResult, opts: RenderOpts = {}) {
  const onCheckMore = vi.fn();
  const onCheckAgain = vi.fn();
  const view = renderWithTheme(
    <DoctorResults
      result={result}
      ctx={ctx(opts.hints)}
      inputByKey={new Map((opts.inputs ?? []).map((i) => [i.key, i]))}
      steps={opts.steps ?? DOCTOR_STEPS}
      onCheckMore={onCheckMore}
      onCheckAgain={onCheckAgain}
    />,
  );
  return { onCheckMore, onCheckAgain, view };
}

const rowEl = (id: string) => {
  const el = document.querySelector<HTMLElement>(`[data-result-row="${id}"]`);
  if (!el) throw new Error(`no row ${id}`);
  return el;
};
const row = (id: string) => within(rowEl(id));
const rowButton = (id: string) => row(id).getByRole("button");
const detail = (id: string) =>
  document.querySelector<HTMLElement>(`[data-result-detail="${id}"]`);

beforeEach(() => {
  vi.mocked(invoke).mockClear();
});

describe("DoctorResults — the list", () => {
  it("titles the page by the rows that need a look, worst first", () => {
    renderResults(fixture());
    expect(
      screen.getByRole("heading", { name: "2 sounds need a look" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Worst first. Make the change on your Tone Master Pro, then check again.",
      ),
    ).toBeInTheDocument();
    const ids = [...document.querySelectorAll("[data-result-row]")].map((e) =>
      e.getAttribute("data-result-row"),
    );
    // The danger row, then the caution row, then the one that couldn't be checked,
    // then the folded clean row (not a result row itself).
    expect(ids).toEqual(["1|p1", "1|s1:0", "2|p2"]);
    const steps = screen.getByRole("list", { name: "Steps" });
    expect(within(steps).getByText("Results")).toHaveAttribute(
      "aria-current",
      "step",
    );
  });

  it("names each row by its preset and sound, and lists its findings in order", () => {
    renderResults(fixture());
    expect(row("1|p1").getByText("Tweed Warm")).toBeInTheDocument();
    expect(row("1|p1").getByText("Main sound")).toBeInTheDocument();
    expect(row("1|p1").getByText("Muddy, Dark")).toBeInTheDocument();
    expect(row("1|s1:0").getByText("Lead")).toBeInTheDocument();
    expect(row("1|s1:0").getByText("Dark")).toBeInTheDocument();
  });

  // The scoping half of doctor.online.spec.ts's Edge EQ-ring run, which no device
  // behaviour can change: a ring diagnosed on every sound of one preset shows on each of
  // that preset's rows, and never on another preset's row, clean or not.
  it("keeps a ringing finding on its own preset's rows", async () => {
    const ring: DoctorDiag = {
      key: "resonant",
      label: "Ringing",
      sev: "med",
      severity: 3,
      z: 8,
      bands: [3],
      direction: "strong",
      detail: "2.6 kHz, 17.0 dB above its neighbours",
      fromLevel: "quiet",
      explain: "A narrow peak rings over every note.",
      rx: [],
    };
    const result = fixture();
    const flagged = result.presets[1];
    // Every sound of the middle preset rings, as Edge's base-graph EQ ring does.
    flagged.sounds = flagged.sounds.map((s) => ({
      ...s,
      diags: [...s.diags, ring],
    }));
    renderResults(result);
    const rows = [
      ...document.querySelectorAll<HTMLElement>('[data-result-row^="1|"]'),
    ];
    expect(rows).toHaveLength(flagged.sounds.length);
    for (const el of rows) {
      expect(within(el).getByText(/Ringing/)).toBeInTheDocument();
    }
    await userEvent
      .setup()
      .click(screen.getByRole("button", { name: "1 more sounds good" }));
    for (const id of ["0|p0", "2|p2"]) {
      expect(rowEl(id)).not.toHaveTextContent("Ringing");
    }
  });

  it("puts no loudness, band picture or venue icons on a row", () => {
    renderResults(fixture());
    expect(document.body.textContent).not.toMatch(/LUFS/);
    // Only the open sound (the worst) carries band meters: one per banded finding.
    expect(screen.getAllByRole("img", { name: /^Tone by band/ })).toHaveLength(
      2,
    );
    expect(row("1|s1:0").queryByRole("img")).toBeNull();
    expect(row("1|s1:0").queryByText("Quiet")).toBeNull();
  });

  it("opens the worst sound on arrival with its findings, worst first", () => {
    renderResults(fixture());
    expect(rowButton("1|p1")).toHaveAttribute("aria-expanded", "true");
    expect(rowButton("1|s1:0")).toHaveAttribute("aria-expanded", "false");
    const open = within(detail("1|p1") ?? document.body);
    const headings = open.getAllByRole("heading").map((h) => h.textContent);
    expect(headings).toEqual(["Muddy", "Dark"]);
    expect(open.getByText("Heard at every volume")).toBeInTheDocument();
    expect(
      open.getByText("Heard at rehearsal volume and up"),
    ).toBeInTheDocument();
    expect(
      open.getByText("The low mids build up and stack with the bass player."),
    ).toBeInTheDocument();
    expect(
      open.getByRole("img", { name: "Tone by band: Low mids too strong" }),
    ).toBeInTheDocument();
    expect(
      open.getByRole("img", {
        name: "Tone by band: High mids, Highs and Air too weak",
      }),
    ).toBeInTheDocument();
  });

  it("lists the fixes as steps in the backend's order, and nothing technical", () => {
    renderResults(fixture());
    const open = within(detail("1|p1") ?? document.body);
    const muddy = open.getAllByRole("list")[0];
    expect(
      within(muddy)
        .getAllByRole("listitem")
        .map((li) => li.textContent),
    ).toEqual([
      "Turn the amp's Bass down 1 or 2.",
      "Or add a 10-band EQ after the cab and cut the low mids.",
    ]);
    expect(open.getAllByText("What to change")).toHaveLength(2);
    // The technical detail, the CPU note: the log's, not the screen's.
    expect(screen.queryByText("EQ-10 250 Hz −6 dB")).toBeNull();
    expect(screen.queryByText("+3.1% CPU")).toBeNull();
    expect(screen.queryByText("+4.2 dB around 250 Hz")).toBeNull();
  });

  it("opens and closes rows on click; several may be open", async () => {
    renderResults(fixture());
    const user = userEvent.setup();
    await user.click(rowButton("1|s1:0"));
    expect(rowButton("1|s1:0")).toHaveAttribute("aria-expanded", "true");
    expect(detail("1|p1")).not.toBeNull();
    expect(detail("1|s1:0")).not.toBeNull();
    await user.click(rowButton("1|p1"));
    expect(detail("1|p1")).toBeNull();
  });

  it("words a sound that couldn't be checked by its kind, never the backend's text", () => {
    renderResults(fixture());
    expect(
      row("2|p2").getByText("Couldn't check this sound. Check again."),
    ).toBeInTheDocument();
    expect(screen.queryByText(/came back silent/)).toBeNull();
    // Nothing to open.
    expect(row("2|p2").queryByRole("button")).toBeNull();
  });

  it("refines a silent capture by the preset's silence hint", () => {
    const r = fixture();
    r.presets[2].sounds[0] = {
      ...r.presets[2].sounds[0],
      error: "no signal captured",
      errorKind: "no_signal",
    };
    renderResults(r, { hints: [[2, "exp_mute"]] });
    expect(
      row("2|p2").getByText(
        "An expression pedal mutes this sound. Move it, then check again.",
      ),
    ).toBeInTheDocument();
  });

  it("folds the clean sounds into one row and opens it on click", async () => {
    renderResults(fixture());
    const user = userEvent.setup();
    const fold = screen.getByRole("button", { name: "1 more sounds good" });
    expect(fold).toHaveAttribute("aria-expanded", "false");
    expect(document.querySelector('[data-result-row="0|p0"]')).toBeNull();
    await user.click(fold);
    expect(row("0|p0").getByText("Sounds good")).toBeInTheDocument();
  });

  it("marks a borderline finding as such, and never sorts it above a real danger row", () => {
    const borderline: DoctorDiag = { ...MUDDY, z: 1.2 };
    renderResults({
      presets: [
        preset(0, [sound("p0", 0, { diags: [borderline] })]),
        preset(1, [sound("p1", 1, { diags: [MUDDY] })]),
      ],
      stopped: false,
    });
    const ids = [...document.querySelectorAll("[data-result-row]")].map((e) =>
      e.getAttribute("data-result-row"),
    );
    expect(ids).toEqual(["1|p1", "0|p0"]);
    expect(row("0|p0").getByText("Muddy (borderline)")).toBeInTheDocument();
  });

  it("says when some bands were too quiet to check", () => {
    const r = fixture();
    r.presets[1].sounds[0] = { ...r.presets[1].sounds[0], skippedBandCount: 2 };
    renderResults(r);
    expect(
      within(detail("1|p1") ?? document.body).getByText(
        "Some bands were too quiet to check.",
      ),
    ).toBeInTheDocument();
  });
});

describe("DoctorResults — all clear and stopped checks (#190)", () => {
  it("all clear: says there is nothing to change, offers only Check other sounds", async () => {
    const { onCheckMore } = renderResults(
      {
        presets: [preset(0, [sound("p0", 0)]), preset(1, [sound("p1", 1)])],
        stopped: false,
      },
      { steps: DOCTOR_STEPS_SKIPPED },
    );
    expect(
      screen.getByRole("heading", {
        name: "Nothing to change in these 2 sounds",
      }),
    ).toBeInTheDocument();
    expect(screen.queryByText(/^Worst first/)).toBeNull();
    expect(row("0|p0").getByText("Sounds good")).toBeInTheDocument();
    expect(row("1|p1").getByText("Sounds good")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Check again" })).toBeNull();
    await userEvent
      .setup()
      .click(screen.getByRole("button", { name: "Check other sounds" }));
    expect(onCheckMore).toHaveBeenCalledOnce();
    expect(screen.getByRole("list", { name: "Steps" })).not.toHaveTextContent(
      "Instrument",
    );
  });

  it("a stopped check never claims all clear: it says it stopped, and lists the sounds it never reached", () => {
    renderResults({
      presets: [
        preset(0, [
          sound("p0", 0),
          sound("s0:0", 0, { scene: 0, status: "unrun", integratedLufs: null }),
        ]),
      ],
      stopped: true,
    });
    expect(
      screen.getByRole("heading", {
        name: "Stopped. Nothing to change in the 1 sound checked.",
      }),
    ).toBeInTheDocument();
    // The all-clear title is never claimed while a sound went unchecked.
    expect(
      screen.queryByRole("heading", {
        name: /^Nothing to change in th(ese|is)/,
      }),
    ).toBeNull();
    expect(row("0|s0:0").getByText("Not checked")).toBeInTheDocument();
    expect(document.body.textContent).not.toMatch(/(^|[^\d.])-?0\.0\s*LUFS/);
    expect(
      screen.getByRole("button", { name: "Check again" }),
    ).toBeInTheDocument();
  });

  it("a stopped check with findings lists the unreached sounds too", () => {
    renderResults({
      presets: [
        preset(1, [
          sound("p1", 1, { diags: [MUDDY] }),
          sound("s1:0", 1, { scene: 0, status: "unrun", integratedLufs: null }),
        ]),
      ],
      stopped: true,
    });
    expect(
      screen.getByRole("heading", { name: "1 sound needs a look" }),
    ).toBeInTheDocument();
    expect(row("1|s1:0").getByText("Not checked")).toBeInTheDocument();
  });

  it("Check again re-runs; Check other sounds goes back to the list", async () => {
    const { onCheckAgain, onCheckMore } = renderResults(fixture());
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Check again" }));
    expect(onCheckAgain).toHaveBeenCalledOnce();
    await user.click(
      screen.getByRole("button", { name: "Check other sounds" }),
    );
    expect(onCheckMore).toHaveBeenCalledOnce();
  });
});

describe("DoctorResults — preset-level rows", () => {
  const advice = (title: string) => ({
    kind: "advisory" as const,
    title,
    detail: "",
    cpuNote: "",
    ops: [],
  });
  // A preset whose `scene` sound jumps 6 dB over its main sound, with the steps the
  // backend writes for that jump.
  const jump = (
    scene: number,
    key: string,
    titles: string[],
  ): DoctorCheckResult => ({
    presets: [
      preset(
        1,
        [
          sound("p1", 1, { label: "Tweed Warm · Base" }),
          sound(key, 1, { scene, label: "Tweed Warm · Lead" }),
        ],
        {
          sceneConsistency: {
            rows: [
              {
                name: "Tweed Warm · Base",
                tag: "BASE",
                deltaDb: 0,
                isRef: true,
              },
              {
                name: "Tweed Warm · Lead",
                tag: "FS1",
                deltaDb: 6,
                isRef: false,
              },
            ],
            worstName: "Tweed Warm · Lead",
            worstDeltaDb: 6,
            rx: titles.map(advice),
          },
        },
      ),
    ],
    stopped: false,
  });

  it("shows a volume jump between scenes as its own row, opened to the steps", () => {
    renderResults(
      jump(1, "s1:1", [
        "Turn down the amp's Level in Lead until the jump is 3 dB or less.",
        "Or, to stand out in Lead, raise the amp's Middle instead of its volume.",
      ]),
    );
    expect(row("1|consistency").getByText("Scenes")).toBeInTheDocument();
    expect(
      row("1|consistency").getByText("Volume jumps between scenes"),
    ).toBeInTheDocument();
    // It is the only finding, so it is open on arrival.
    const open = within(detail("1|consistency") ?? document.body);
    expect(
      open.getByText(
        "Your Lead scene is 6.0 dB louder than the main sound. That's a big jump when you switch.",
      ),
    ).toBeInTheDocument();
    expect(open.getByText("Main sound")).toBeInTheDocument();
    expect(open.getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "Turn down the amp's Level in Lead until the jump is 3 dB or less.",
      "Or, to stand out in Lead, raise the amp's Middle instead of its volume.",
      "Level this preset's scenes on the Level tab.",
    ]);
    expect(open.queryByText(/Tweed Warm/)).toBeNull();
  });

  it("calls a louder first scene a reading that isn't certain, with no leveling step", () => {
    renderResults(
      jump(0, "s1:0", [
        "Couldn't confirm the reading for Lead. Check it by ear before you change it.",
      ]),
    );
    const open = within(detail("1|consistency") ?? document.body);
    expect(
      open.getByText(
        "Your Lead scene read 6.0 dB louder than the main sound, but that reading isn't certain.",
      ),
    ).toBeInTheDocument();
    expect(open.queryByText(/That's a big jump/)).toBeNull();
    expect(open.getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "Couldn't confirm the reading for Lead. Check it by ear before you change it.",
    ]);
  });

  it("shows footswitch assignments worth checking, read from the preset alone", () => {
    renderResults({
      presets: [
        preset(0, [sound("p0", 0)], {
          levelingDamage: [
            {
              switch: 8,
              label: "VERB KILL",
              nodeId: "n1",
              fenderId: "ACD_TMSpring63",
              parameterId: "mix",
              kind: "deletedEffect",
              detail: "mix 0.3 → 0.0",
            },
            {
              switch: 9,
              label: "WAH SWEEP",
              nodeId: "n2",
              fenderId: "ACD_CryBabyQ535",
              parameterId: "position",
              kind: "sweptOther",
              detail: "position 0.2 → 0.9",
            },
          ],
        }),
      ],
      stopped: false,
    });
    expect(
      row("0|leveling-damage").getByText("2 footswitches worth checking"),
    ).toBeInTheDocument();
    const open = within(detail("0|leveling-damage") ?? document.body);
    expect(
      open.getByText(
        "These footswitches change more than the volume. Check them if you didn't set them up that way.",
      ),
    ).toBeInTheDocument();
    expect(open.getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "VERB KILL: Turns the effect almost silent when you press it.",
      "WAH SWEEP: Changes the tone, not only the volume, when you press it.",
    ]);
    expect(screen.queryByText("mix 0.3 → 0.0")).toBeNull();
  });
});

describe("DoctorResults — shared-block line", () => {
  const AMP: GraphNode = {
    group_id: "G1",
    node_id: "amp1",
    model: "ACD_TweedDeluxe",
    bypassed: false,
    params: {},
  };
  const onOff = (nodeId: string, fenderId: string) => ({
    func: "on-off" as const,
    group_id: "G1",
    node_id: nodeId,
    fender_id: fenderId,
    parameter_id: null,
    value_a: null,
    value_b: null,
    is_active: false,
  });
  // A switch with nothing to level sits first in the preset's full roster, so the
  // footswitch sound's own switch (2) is not the first entry: a lookup by the sound's
  // position in the levelable list would find the Phaser's block instead of the Boost's.
  const PHASER: FootswitchInfo = {
    switch: 0,
    label: "Phaser",
    link_group: null,
    functions: [onOff("PHS1", "ACD_Phaser")],
    level_params: [],
  };
  const FS: FootswitchInfo = {
    switch: 2,
    label: "Boost",
    link_group: null,
    functions: [onOff("drive1", "ACD_Boost")],
    level_params: [],
  };
  const fixOn = (d: DoctorDiag, nodeId: string, param: string): DoctorDiag => ({
    ...d,
    rx: [
      {
        kind: "oneclick",
        title: d.rx[0].title,
        detail: "",
        cpuNote: "",
        ops: [{ kind: "param", groupId: "G1", nodeId, param, value: 3 }],
      },
    ],
  });
  const input = (key: string, footswitch: number | null): DoctorInputArg => ({
    key,
    listIndex: 1,
    scene: null,
    footswitch,
    label: key,
    soundName: footswitch == null ? "Main sound" : "footswitch 3",
    tag: footswitch == null ? "BASE" : "FS3",
    topologyId: null,
    calibrationLufs: null,
    profileId: null,
    nodes: [AMP],
    footswitches: [PHASER, FS],
    sceneOverrides: [],
  });
  const fsSound = (diags: DoctorDiag[]) =>
    sound("f1:0", 1, { footswitch: 2, diags });
  const shared =
    "This block is shared, so the change affects every sound in this preset.";

  it("says so once when a footswitch sound's fixes edit a block every sound uses", () => {
    renderResults(
      {
        presets: [
          preset(1, [
            fsSound([
              fixOn(MUDDY, "amp1", "bass"),
              fixOn(DARK, "amp1", "treble"),
            ]),
          ]),
        ],
        stopped: false,
      },
      { inputs: [input("f1:0", 2)] },
    );
    const open = within(detail("1|f1:0") ?? document.body);
    expect(open.getAllByRole("heading")).toHaveLength(2);
    expect(screen.getAllByText(shared)).toHaveLength(1);
  });

  it("stays quiet when the fix edits the switch's own block, found by its switch number", () => {
    renderResults(
      {
        presets: [preset(1, [fsSound([fixOn(MUDDY, "drive1", "bass")])])],
        stopped: false,
      },
      { inputs: [input("f1:0", 2)] },
    );
    expect(detail("1|f1:0")).not.toBeNull();
    expect(screen.queryByText(shared)).toBeNull();
  });

  it("never shows the line on a main sound, whatever block its fix edits", () => {
    renderResults(
      {
        presets: [
          preset(1, [
            sound("p1", 1, { diags: [fixOn(MUDDY, "amp1", "bass")] }),
          ]),
        ],
        stopped: false,
      },
      { inputs: [input("p1", null)] },
    );
    expect(detail("1|p1")).not.toBeNull();
    expect(screen.queryByText(shared)).toBeNull();
  });
});

describe("DoctorResults — reference and cut-through", () => {
  it("reads cut-through against the factory presets, and hides it with no comparison", async () => {
    const r: DoctorCheckResult = {
      presets: [
        preset(0, [
          sound("p0", 0, {
            cutThrough: {
              contrastDb: -4,
              factoryPercentile: 12,
              advisory: true,
            },
          }),
        ]),
        preset(1, [
          sound("p1", 1, {
            cutThrough: {
              contrastDb: 2,
              factoryPercentile: null,
              advisory: false,
            },
          }),
        ]),
      ],
      stopped: false,
    };
    renderResults(r);
    const user = userEvent.setup();
    await user.click(rowButton("0|p0"));
    const open = within(detail("0|p0") ?? document.body);
    expect(open.getByText("Cutting through the mix")).toBeInTheDocument();
    expect(
      open.getByText("More presence than 12% of the factory presets."),
    ).toBeInTheDocument();
    expect(
      open.getByText(
        "Less presence than most factory presets, so it gets lost in a busy mix.",
      ),
    ).toBeInTheDocument();
    await user.click(rowButton("1|p1"));
    expect(
      within(detail("1|p1") ?? document.body).queryByText(
        "Cutting through the mix",
      ),
    ).toBeNull();
  });

  it("uses a sound as the reference and tells another how to sound like it", async () => {
    renderResults(
      {
        presets: [
          preset(0, [sound("p0", 0, { balanceDb: [3, 0, 0, 0, 0, 0] })]),
          preset(1, [
            sound("p1", 1, { diags: [MUDDY], balanceDb: [0, 0, 0, 0, 0, 0] }),
          ]),
        ],
        stopped: false,
      },
      {
        inputs: [
          {
            key: "p1",
            listIndex: 1,
            scene: null,
            footswitch: null,
            label: "p1",
            soundName: "Main sound",
            tag: null,
            topologyId: null,
            calibrationLufs: null,
            profileId: null,
            nodes: [
              {
                group_id: "G1",
                node_id: "amp1",
                model: "ACD_TweedDeluxe",
                bypassed: false,
                params: {},
              },
            ],
            footswitches: [],
            sceneOverrides: [],
          },
        ],
      },
    );
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "1 more sounds good" }),
    );
    await user.click(rowButton("0|p0"));
    await user.click(
      within(detail("0|p0") ?? document.body).getByRole("button", {
        name: "Use as reference",
      }),
    );
    const ref = within(detail("0|p0") ?? document.body);
    expect(ref.getByText("Reference")).toBeInTheDocument();
    expect(ref.getByRole("button", { name: "Clear" })).toBeInTheDocument();
    const other = within(detail("1|p1") ?? document.body);
    expect(
      other.getByText("To sound like Clean DI, Main sound"),
    ).toBeInTheDocument();
    expect(
      other.getByText("Add a 10-band EQ after the cab and set 62 Hz +3 dB."),
    ).toBeInTheDocument();
    await user.click(ref.getByRole("button", { name: "Clear" }));
    expect(screen.queryByText("To sound like Clean DI, Main sound")).toBeNull();
  });
});

describe("DoctorResults — nothing writes", () => {
  it("sends no command from any click on the page", async () => {
    renderResults(fixture());
    const user = userEvent.setup();
    await user.click(rowButton("1|s1:0"));
    await user.click(
      screen.getByRole("button", { name: "1 more sounds good" }),
    );
    await user.click(rowButton("0|p0"));
    await user.click(
      within(detail("0|p0") ?? document.body).getByRole("button", {
        name: "Use as reference",
      }),
    );
    expect(vi.mocked(invoke)).not.toHaveBeenCalled();
  });
});

describe("DoctorResults — wide window", () => {
  const original = window.innerWidth;
  afterEach(() => {
    act(() => {
      Object.defineProperty(window, "innerWidth", {
        configurable: true,
        value: original,
      });
      window.dispatchEvent(new Event("resize"));
    });
  });

  it("shows the opened sound beside the list, and a click switches it", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1440,
    });
    renderResults(fixture());
    // The worst sound is shown beside the list, once — not under its row.
    expect(rowButton("1|p1")).toHaveAttribute("aria-current", "true");
    expect(
      document.querySelectorAll('[data-result-detail="1|p1"]'),
    ).toHaveLength(1);
    expect(
      screen.getByRole("heading", { name: "Tweed Warm Main sound" }),
    ).toBeInTheDocument();
    await userEvent.setup().click(rowButton("1|s1:0"));
    expect(rowButton("1|s1:0")).toHaveAttribute("aria-current", "true");
    expect(rowButton("1|p1")).not.toHaveAttribute("aria-current");
    expect(detail("1|p1")).toBeNull();
    expect(detail("1|s1:0")).not.toBeNull();
  });
});
