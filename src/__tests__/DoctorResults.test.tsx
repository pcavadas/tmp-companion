// src/__tests__/DoctorResults.test.tsx — the Doctor RESULTS page in its dense
// row-per-sound form: summary copy + counts, worst-first ordering, the "Needs a
// look" filter, per-row expansion (explainer + prescription), the healthy-collapse
// reveal, the shared-block caption at row level, the synthetic Level-jumps row, and
// the prescription lifecycle (apply → ack-gated save → saved; discard). The device
// commands (apply/save/discard) are mocked.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { ThemeProvider } from "../theme/ThemeProvider";

vi.mock("../lib/invoke", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/invoke")>();
  return {
    ...actual,
    doctorApply: vi.fn(),
    doctorSave: vi.fn(),
    doctorDiscard: vi.fn(),
  };
});

// Imported AFTER the mock so the view + prescription cards pick up the mocks.
import { doctorApply, doctorSave, doctorDiscard } from "../lib/invoke";
import { DoctorResults } from "../views/doctor/DoctorResults";
import type {
  DoctorApplyResult,
  DoctorCheckResult,
  DoctorDiag,
  DoctorOp,
  DoctorSoundResult,
  FootswitchInfo,
} from "../lib/types";

const NAMES = new Map<number, string>([
  [0, "Studio Clean"],
  [1, "Muddy Rhythm"],
  [2, "Broken Lead"],
]);

// One all-clear preset, one high-severity preset (a banded muddy diag with a
// oneclick + advisory rx, a time-domain washed-out diag, and a scene-consistency
// jump of +6 dB), and one preset whose single sound errored.
function fixture(): DoctorCheckResult {
  return {
    presets: [
      {
        listIndex: 0,
        sounds: [
          {
            key: "p0",
            listIndex: 0,
            scene: null,
            footswitch: null,
            label: "Clean Base",
            tag: null,
            diags: [],
            integratedLufs: -20,
            tailRatioDb: 0,
            balanceDb: [],
            bandLabels: [
              "Lows",
              "Low-mids",
              "Mids",
              "High-mids",
              "Highs",
              "Air",
            ],
            error: null,
          },
        ],
        sceneConsistency: null,
      },
      {
        listIndex: 1,
        sounds: [
          {
            key: "p1s0",
            listIndex: 1,
            scene: null,
            footswitch: null,
            label: "Rhythm Crunch",
            tag: "BASE",
            diags: [
              {
                key: "muddy",
                label: "Muddy",
                sev: "high",
                severity: 4.2,
                bands: [1],
                detail: "+4.2 dB around 250 Hz",
                fromLevel: "rehearsal",
                explain:
                  "Low-mids are piling up and swallowing your note definition.",
                rx: [
                  {
                    kind: "oneclick",
                    title: "Add a low cut at 90 Hz",
                    detail: "Trims the boom without thinning the body.",
                    cpuNote: "+0.4% CPU",
                    ops: [
                      {
                        kind: "param",
                        groupId: "g",
                        nodeId: "n",
                        param: "hpf",
                        value: 90,
                      },
                    ],
                  },
                  {
                    kind: "advisory",
                    title: "Nudge Bass down a notch",
                    detail: "Roll the amp's Bass back a hair.",
                    cpuNote: "",
                    ops: [],
                  },
                ],
              },
            ],
            integratedLufs: -18,
            tailRatioDb: 0,
            balanceDb: [-6, 4, -2, -8, -12, -18],
            bandLabels: [
              "Lows",
              "Low-mids",
              "Mids",
              "High-mids",
              "Highs",
              "Air",
            ],
            error: null,
          },
          {
            key: "p1s1",
            listIndex: 1,
            scene: 0,
            footswitch: null,
            label: "Lead Solo",
            tag: "FS1",
            diags: [
              {
                key: "washed",
                label: "Washed out",
                sev: "med",
                severity: 6,
                bands: [],
                detail: "tail 6 dB over dry",
                fromLevel: "quiet",
                explain: "The reverb tail is burying the dry signal.",
                rx: [
                  {
                    kind: "oneclick",
                    title: "Bring the reverb mix down to 25%",
                    detail: "Keeps the space, restores the attack.",
                    cpuNote: "0% CPU",
                    ops: [
                      {
                        kind: "param",
                        groupId: "g",
                        nodeId: "verb",
                        param: "mix",
                        value: 25,
                      },
                    ],
                  },
                ],
              },
            ],
            integratedLufs: -19,
            tailRatioDb: 6,
            balanceDb: [-4, -2, 0, -3, -5, -7],
            bandLabels: [
              "Lows",
              "Low-mids",
              "Mids",
              "High-mids",
              "Highs",
              "Air",
            ],
            error: null,
          },
        ],
        sceneConsistency: {
          rows: [
            { name: "Rhythm", tag: "BASE", deltaDb: 0, isRef: true },
            { name: "Crunch", tag: "FS1", deltaDb: 6, isRef: false },
          ],
          worstName: "Crunch",
          worstDeltaDb: 6,
          rx: [
            {
              kind: "advisory",
              title: "Crunch is much louder than the base sound",
              detail:
                "Crunch jumps +6.0 dB when you switch to it — level it from the Level tab.",
              cpuNote: "",
              ops: [],
            },
          ],
        },
      },
      {
        listIndex: 2,
        sounds: [
          {
            key: "p2",
            listIndex: 2,
            scene: null,
            footswitch: null,
            label: "Broken Base",
            tag: null,
            diags: [],
            integratedLufs: 0,
            tailRatioDb: 0,
            balanceDb: [],
            bandLabels: [
              "Lows",
              "Low-mids",
              "Mids",
              "High-mids",
              "Highs",
              "Air",
            ],
            error: "The capture came back silent — check the cable.",
          },
        ],
        sceneConsistency: null,
      },
    ],
    stopped: false,
  };
}

function renderResults(
  result: DoctorCheckResult = fixture(),
  onCheckMore: () => void = () => undefined,
) {
  return render(
    <ThemeProvider>
      <DoctorResults
        result={result}
        presetNames={NAMES}
        footswitchInfo={new Map()}
        onCheckMore={onCheckMore}
      />
    </ThemeProvider>,
  );
}

function resetMocks() {
  vi.mocked(doctorApply).mockReset();
  vi.mocked(doctorSave).mockReset();
  vi.mocked(doctorDiscard).mockReset();
  vi.mocked(doctorApply).mockResolvedValue({
    beforeClip: "data:audio/wav;base64,AAAA",
    afterClip: "data:audio/wav;base64,BBBB",
  });
  vi.mocked(doctorSave).mockResolvedValue(undefined);
  vi.mocked(doctorDiscard).mockResolvedValue(undefined);
}

describe("DoctorResults — summary + cards", () => {
  beforeEach(resetMocks);

  it("summarizes worst-first with the right counts", () => {
    renderResults();
    expect(screen.getByText("1 of 3 presets need a look")).toBeInTheDocument();
    // n-of-total flagged, singular "needs attention" clause.
    expect(
      screen.getByText(/2 of 4 sounds flagged · 1 needs attention/),
    ).toBeInTheDocument();
  });

  it("shows an all-clear summary + no filter when nothing is flagged", () => {
    const clean: DoctorCheckResult = {
      presets: [
        {
          listIndex: 0,
          sounds: [
            {
              key: "p0",
              listIndex: 0,
              scene: null,
              footswitch: null,
              label: "Clean Base",
              tag: null,
              diags: [],
              integratedLufs: -20,
              tailRatioDb: 0,
              balanceDb: [],
              bandLabels: [
                "Lows",
                "Low-mids",
                "Mids",
                "High-mids",
                "Highs",
                "Air",
              ],
              error: null,
            },
          ],
          sceneConsistency: null,
        },
      ],
      stopped: false,
    };
    renderResults(clean);
    expect(screen.getByText("All 1 sound sounds good")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Nothing to fix — Doctor didn't find any tone problems.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("radiogroup", { name: "Filter results" }),
    ).not.toBeInTheDocument();
    // The happy path shows the clean card itself — no "Show all" strip hiding it.
    expect(screen.getByText("Clean Base")).toBeInTheDocument();
    expect(screen.queryByText("Show all")).not.toBeInTheDocument();
  });

  it("shows per-card status badges (count + all-clear)", async () => {
    const user = userEvent.setup();
    renderResults();
    // The high preset: 2 diagnoses + 1 scene finding = 3 things.
    expect(screen.getByText("3 to look at")).toBeInTheDocument();
    // Default filter hides the fully-clean preset; the errored preset stays and
    // reads "All clear" (no diagnoses). Reveal the clean one via "Everything".
    expect(screen.getAllByText("All clear")).toHaveLength(1);
    await user.click(screen.getByRole("radio", { name: "Everything" }));
    expect(screen.getAllByText("All clear")).toHaveLength(2);
  });

  it("orders the cards worst-first, ties broken by slot", async () => {
    const user = userEvent.setup();
    const { container } = renderResults();
    await user.click(screen.getByRole("radio", { name: "Everything" }));
    const text = container.textContent;
    const muddy = text.indexOf("Muddy Rhythm");
    const clean = text.indexOf("Studio Clean");
    const broken = text.indexOf("Broken Lead");
    expect(muddy).toBeGreaterThanOrEqual(0);
    expect(muddy).toBeLessThan(clean);
    expect(clean).toBeLessThan(broken);
  });

  it("renders an errored sound as a message, not a diagnosis", () => {
    renderResults();
    expect(
      screen.getByText("The capture came back silent — check the cable."),
    ).toBeInTheDocument();
  });

  it("expands a problem row to show its explanation, detail, and fix; collapses on a second click", async () => {
    const user = userEvent.setup();
    renderResults();
    const explain =
      "Low-mids are piling up and swallowing your note definition.";
    expect(screen.queryByText(explain)).not.toBeInTheDocument();

    await user.click(screen.getByText("Rhythm Crunch"));
    expect(screen.getByText(explain)).toBeInTheDocument();
    expect(screen.getByText("+4.2 dB around 250 Hz")).toBeInTheDocument();
    // The finding carries a level indicator for the quietest level it fires at
    // (fromLevel "rehearsal" → the aria/title says "at rehearsal volume and up").
    expect(
      screen.getAllByRole("img", { name: /at rehearsal volume and up/i })
        .length,
    ).toBeGreaterThan(0);
    expect(screen.getByText("Add a low cut at 90 Hz")).toBeInTheDocument();

    await user.click(screen.getByText("Rhythm Crunch"));
    expect(screen.queryByText(explain)).not.toBeInTheDocument();
  });

  it("shows a fires-at-every-volume finding as an at-any-volume indicator (previously invisible)", () => {
    renderResults();
    // "Washed out" fires at fromLevel "quiet" (every volume). The old text pill
    // rendered NOTHING for that case; the indicator now shows it as an all-lit,
    // accessibly-labelled state on the collapsed triage row.
    expect(
      screen.getAllByRole("img", { name: /washed out at any volume/i }).length,
    ).toBeGreaterThan(0);
  });

  it("shows the full BandMeter only for a banded diagnosis, no toggle", async () => {
    const user = userEvent.setup();
    renderResults();
    // Muddy (bands: [1]) draws the labelled BandMeter; Washed out (no bands) doesn't.
    await user.click(screen.getByText("Rhythm Crunch"));
    await user.click(screen.getByText("Lead Solo"));
    expect(screen.getAllByText("Lows")).toHaveLength(1);
    expect(screen.queryByText("Show the frequencies")).not.toBeInTheDocument();
  });

  it("does not expand a clear or errored row", async () => {
    const user = userEvent.setup();
    renderResults();
    await user.click(screen.getByRole("radio", { name: "Everything" }));
    // Clear row: clicking reveals nothing (no prescription surfaces).
    await user.click(screen.getByText("Clean Base"));
    // Errored row: clicking keeps the message, opens nothing.
    await user.click(screen.getByText("Broken Base"));
    expect(
      screen.queryByRole("button", { name: /apply to the unit/i }),
    ).not.toBeInTheDocument();
  });

  it("gives an advisory prescription no Apply button", async () => {
    const user = userEvent.setup();
    renderResults();
    await user.click(screen.getByText("Rhythm Crunch"));
    // Only the oneclick rx is applicable; the advisory is static.
    expect(
      screen.getAllByRole("button", { name: /apply to the unit/i }),
    ).toHaveLength(1);
    expect(screen.getByText("Nudge Bass down a notch")).toBeInTheDocument();
  });
});

describe("DoctorResults — filter", () => {
  beforeEach(resetMocks);

  it("hides fully-clean presets by default and flips via the 'Show all' strip", async () => {
    const user = userEvent.setup();
    renderResults();
    // Default "Needs a look": the clean preset is hidden.
    expect(screen.queryByText("Studio Clean")).not.toBeInTheDocument();
    const strip = screen.getByText("1 preset sounds good");
    expect(strip).toBeInTheDocument();
    await user.click(strip);
    expect(screen.getByText("Studio Clean")).toBeInTheDocument();
  });
});

describe("DoctorResults — healthy collapse", () => {
  beforeEach(resetMocks);

  // A flagged preset that also has a clear sound → the collapse line appears.
  function mixedFixture(): DoctorCheckResult {
    const muddy: DoctorDiag = {
      key: "muddy",
      label: "Muddy",
      sev: "high",
      severity: 4,
      bands: [1],
      detail: "+4 dB around 250 Hz",
      fromLevel: "quiet",
      explain: "Low-mids are piling up.",
      rx: [
        {
          kind: "advisory",
          title: "Nudge Bass",
          detail: "Roll it back.",
          cpuNote: "",
          ops: [],
        },
      ],
    };
    return {
      presets: [
        {
          listIndex: 1,
          sounds: [
            {
              key: "p1s0",
              listIndex: 1,
              scene: null,
              footswitch: null,
              label: "Rhythm Crunch",
              tag: "BASE",
              diags: [muddy],
              integratedLufs: -18,
              tailRatioDb: 0,
              balanceDb: [-6, 4, -2, -8, -12, -18],
              bandLabels: [
                "Lows",
                "Low-mids",
                "Mids",
                "High-mids",
                "Highs",
                "Air",
              ],
              error: null,
            },
            {
              key: "p1s1",
              listIndex: 1,
              scene: 0,
              footswitch: null,
              label: "Clean Lead",
              tag: "FS1",
              diags: [],
              integratedLufs: -19,
              tailRatioDb: 0,
              balanceDb: [-4, -2, 0, -3, -5, -7],
              bandLabels: [
                "Lows",
                "Low-mids",
                "Mids",
                "High-mids",
                "Highs",
                "Air",
              ],
              error: null,
            },
          ],
          sceneConsistency: null,
        },
      ],
      stopped: false,
    };
  }

  it("collapses clear rows and reveals them on click", async () => {
    const user = userEvent.setup();
    renderResults(mixedFixture());
    expect(screen.queryByText("Clean Lead")).not.toBeInTheDocument();
    await user.click(screen.getByText("1 sound checks out"));
    expect(screen.getByText("Clean Lead")).toBeInTheDocument();
  });
});

describe("DoctorResults — Level jumps (scene consistency) row", () => {
  beforeEach(resetMocks);

  it("renders as a synthetic row and expands to the advisory scene fix", async () => {
    const user = userEvent.setup();
    renderResults();
    expect(screen.getByText("Level jumps")).toBeInTheDocument();
    expect(screen.getByText("Crunch +6.0 dB vs base")).toBeInTheDocument();
    // Collapsed: no Apply button (the scene fix is advised, not applied).
    expect(
      screen.queryByText(
        "Run scene leveling from the Level tab to apply this one.",
      ),
    ).not.toBeInTheDocument();

    await user.click(screen.getByText("Level jumps"));
    expect(
      screen.getByText(
        "Run scene leveling from the Level tab to apply this one.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /apply to the unit/i }),
    ).not.toBeInTheDocument();
  });
});

describe("DoctorResults — prescription lifecycle", () => {
  beforeEach(resetMocks);

  it("applies, auditions, then ack-gates the save through to saved", async () => {
    const user = userEvent.setup();
    renderResults();

    await user.click(screen.getByText("Rhythm Crunch"));
    await user.click(
      screen.getByRole("button", { name: /apply to the unit/i }),
    );

    expect(await screen.findByText("Listen & compare")).toBeInTheDocument();
    expect(
      screen.getByText("Applied to the unit · not saved"),
    ).toBeInTheDocument();
    expect(doctorApply).toHaveBeenCalledWith(
      expect.objectContaining({ listIndex: 1, name: "Muddy Rhythm" }),
    );

    const save = screen.getByRole("button", { name: /save to preset/i });
    expect(save).toBeDisabled();
    expect(doctorSave).not.toHaveBeenCalled();

    await user.click(screen.getByText("I've backed up with Pro Control"));
    expect(save).toBeEnabled();
    await user.click(save);

    expect(await screen.findByText("Saved to the preset.")).toBeInTheDocument();
    expect(doctorSave).toHaveBeenCalledWith(1, "Muddy Rhythm");
  });

  it("discards an applied prescription back to draft", async () => {
    const user = userEvent.setup();
    renderResults();

    await user.click(screen.getByText("Rhythm Crunch"));
    await user.click(
      screen.getByRole("button", { name: /apply to the unit/i }),
    );
    const discard = await screen.findByRole("button", { name: /discard/i });
    await user.click(discard);

    expect(
      await screen.findByRole("button", { name: /apply to the unit/i }),
    ).toBeInTheDocument();
    expect(doctorDiscard).toHaveBeenCalledWith(1);
    expect(doctorSave).not.toHaveBeenCalled();
  });

  it("locks sibling Apply buttons from the moment an apply is IN FLIGHT", async () => {
    let resolveApply: (r: DoctorApplyResult) => void = () => undefined;
    vi.mocked(doctorApply).mockImplementation(
      () =>
        new Promise<DoctorApplyResult>((res) => {
          resolveApply = res;
        }),
    );
    const user = userEvent.setup();
    renderResults();

    // Expand both problem rows → two Apply buttons.
    await user.click(screen.getByText("Rhythm Crunch"));
    await user.click(screen.getByText("Lead Solo"));
    const applies = screen.getAllByRole("button", {
      name: /apply to the unit/i,
    });
    expect(applies).toHaveLength(2);
    await user.click(applies[0]);

    const sibling = screen.getByRole("button", { name: /apply to the unit/i });
    expect(sibling).toBeDisabled();
    expect(
      screen.getByText("Save or discard the applied fix first."),
    ).toBeInTheDocument();

    resolveApply({
      beforeClip: "data:audio/wav;base64,AAAA",
      afterClip: "data:audio/wav;base64,BBBB",
    });
    expect(await screen.findByText("Listen & compare")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /apply to the unit/i }),
    ).toBeDisabled();
  });

  it("discards the applied-but-unsaved edit on 'Check other sounds' (once)", async () => {
    const onCheckMore = vi.fn();
    const user = userEvent.setup();
    const { unmount } = renderResults(fixture(), onCheckMore);

    await user.click(screen.getByText("Rhythm Crunch"));
    await user.click(
      screen.getByRole("button", { name: /apply to the unit/i }),
    );
    await screen.findByText("Listen & compare");

    await user.click(
      screen.getByRole("button", { name: /check other sounds/i }),
    );
    expect(doctorDiscard).toHaveBeenCalledWith(1);
    expect(onCheckMore).toHaveBeenCalled();

    unmount();
    expect(doctorDiscard).toHaveBeenCalledTimes(1);
  });

  it("discards the applied-but-unsaved edit on unmount", async () => {
    const user = userEvent.setup();
    const { unmount } = renderResults();

    await user.click(screen.getByText("Rhythm Crunch"));
    await user.click(
      screen.getByRole("button", { name: /apply to the unit/i }),
    );
    await screen.findByText("Listen & compare");
    expect(doctorDiscard).not.toHaveBeenCalled();

    unmount();
    expect(doctorDiscard).toHaveBeenCalledWith(1);
    expect(doctorDiscard).toHaveBeenCalledTimes(1);
  });

  it("does not discard on leave when nothing is applied (or after a save)", async () => {
    const user = userEvent.setup();
    const { unmount } = renderResults();

    await user.click(screen.getByText("Rhythm Crunch"));
    await user.click(
      screen.getByRole("button", { name: /apply to the unit/i }),
    );
    await screen.findByText("Listen & compare");
    await user.click(screen.getByText("I've backed up with Pro Control"));
    await user.click(screen.getByRole("button", { name: /save to preset/i }));
    await screen.findByText("Saved to the preset.");

    unmount();
    expect(doctorDiscard).not.toHaveBeenCalled();
  });
});

describe("DoctorResults — shared-block caption", () => {
  beforeEach(resetMocks);

  const CAPTION =
    "This block is shared — the change affects all sounds of this preset.";

  function diag(op: DoctorOp, key: string): DoctorDiag {
    return {
      key,
      label: key === "muddy" ? "Muddy" : "Harsh",
      sev: "high",
      severity: 4,
      bands: [1],
      detail: "+4 dB around 250 Hz",
      fromLevel: "quiet",
      explain: "Low-mids are piling up.",
      rx: [
        {
          kind: "oneclick",
          title: "Add a low cut",
          detail: "Trims the boom.",
          cpuNote: "+0.4% CPU",
          ops: [op],
        },
      ],
    };
  }

  // A single-preset result whose one sound carries the given diagnoses — the caller
  // supplies whether the sound is a footswitch (key `f0:0`, FootswitchInfo index 0).
  function fsFixture(
    diags: DoctorDiag[],
    footswitch: number | null,
  ): DoctorCheckResult {
    const sound: DoctorSoundResult = {
      key: footswitch == null ? "p0" : "f0:0",
      listIndex: 0,
      scene: null,
      footswitch,
      label: "Overdrive",
      tag: footswitch == null ? null : "FS4",
      diags,
      integratedLufs: -18,
      tailRatioDb: 0,
      balanceDb: [-6, 4, -2, -8, -12, -18],
      bandLabels: ["Lows", "Low-mids", "Mids", "High-mids", "Highs", "Air"],
      error: null,
    };
    return {
      presets: [{ listIndex: 0, sounds: [sound], sceneConsistency: null }],
      stopped: false,
    };
  }

  // FS4 (switch index 3) toggles one block, node "DRV1".
  const fsInfo = new Map<number, FootswitchInfo[]>([
    [
      0,
      [
        {
          switch: 3,
          label: "Drive",
          link_group: null,
          functions: [
            {
              func: "on-off",
              group_id: "g",
              node_id: "DRV1",
              fender_id: "ACD_Overdrive",
              parameter_id: null,
              value_a: null,
              value_b: null,
            },
          ],
          level_params: [],
        },
      ],
    ],
  ]);

  const paramOp = (nodeId: string): DoctorOp => ({
    kind: "param",
    groupId: "g",
    nodeId,
    param: "hpf",
    value: 90,
  });

  function renderShared(result: DoctorCheckResult) {
    return render(
      <ThemeProvider>
        <DoctorResults
          result={result}
          presetNames={new Map([[0, "Overdrive Rhythm"]])}
          footswitchInfo={fsInfo}
          onCheckMore={() => undefined}
        />
      </ThemeProvider>,
    );
  }

  it("captions an FS fix that edits a block outside the switch's own set — once per row", async () => {
    const user = userEvent.setup();
    // Two diags both editing the shared CAB1 → the caption still shows exactly once.
    renderShared(
      fsFixture(
        [diag(paramOp("CAB1"), "muddy"), diag(paramOp("CAB1"), "harsh")],
        3,
      ),
    );
    await user.click(screen.getByText("Overdrive"));
    expect(screen.getAllByText(CAPTION)).toHaveLength(1);
  });

  it("omits the caption when the fix edits the switch's own block", async () => {
    const user = userEvent.setup();
    renderShared(fsFixture([diag(paramOp("DRV1"), "muddy")], 3));
    await user.click(screen.getByText("Overdrive"));
    expect(screen.queryByText(CAPTION)).not.toBeInTheDocument();
  });

  it("never captions a Base sound (footswitch == null)", async () => {
    const user = userEvent.setup();
    renderShared(fsFixture([diag(paramOp("CAB1"), "muddy")], null));
    await user.click(screen.getByText("Overdrive"));
    expect(screen.queryByText(CAPTION)).not.toBeInTheDocument();
  });
});

describe("DoctorResults — spiky (time-domain chain rx)", () => {
  beforeEach(resetMocks);

  // A time-domain diag (bands: []) whose rx pairs an advisory with a chain
  // insert — pins that a chain-kind rx renders (the strip preview + the
  // "Rebuilds the chain" badge) alongside an advisory, with no BandMeter.
  function spikyFixture(): DoctorCheckResult {
    return {
      presets: [
        {
          listIndex: 0,
          sounds: [
            {
              key: "p0",
              listIndex: 0,
              scene: null,
              footswitch: null,
              label: "Ambient Swell",
              tag: null,
              diags: [
                {
                  key: "spiky",
                  label: "Spiky",
                  sev: "med",
                  severity: 5,
                  bands: [],
                  detail: "swings 5.0 LU between peaks and average",
                  fromLevel: "quiet",
                  explain:
                    "The level jumps between loud peaks and a much quieter average — it pokes out of the mix one moment and disappears the next.",
                  rx: [
                    {
                      kind: "advisory",
                      title: "Tame the swings at the source",
                      detail:
                        "If the swings come from a volume swell, tremolo, or a delay building up, easing that effect's depth or level is the honest fix.",
                      cpuNote: "",
                      ops: [],
                    },
                    {
                      kind: "chain",
                      title: "Add a studio compressor after the cab",
                      detail:
                        "Evens out the level after the cab, transparently.",
                      cpuNote: "+1.0% CPU",
                      ops: [
                        {
                          kind: "insert_node",
                          groupId: "g",
                          beforeFenderId: null,
                          fenderId: "ACD_CompressorSimpleSoftKnee",
                          params: [],
                        },
                      ],
                      chain: {
                        template: "after · +COMP",
                        blocks: [
                          { model: "ACD_TweedDeluxe" },
                          {
                            model: "ACD_CompressorSimpleSoftKnee",
                            added: true,
                          },
                        ],
                      },
                    },
                  ],
                },
              ],
              integratedLufs: -20,
              tailRatioDb: 0,
              balanceDb: [0, 0, 0, 0, 0, 0],
              bandLabels: [
                "Lows",
                "Low-mids",
                "Mids",
                "High-mids",
                "Highs",
                "Air",
              ],
              error: null,
            },
          ],
          sceneConsistency: null,
        },
      ],
      stopped: false,
    };
  }

  it("renders the chain-preview rx + advisory with no BandMeter", async () => {
    const user = userEvent.setup();
    renderResults(spikyFixture());
    await user.click(screen.getByText("Ambient Swell"));
    expect(
      screen.getByText("Tame the swings at the source"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Add a studio compressor after the cab"),
    ).toBeInTheDocument();
    expect(screen.getByText("You turn the knob")).toBeInTheDocument();
    expect(screen.getByText("Rebuilds the chain")).toBeInTheDocument();
    expect(screen.getByTitle("added")).toBeInTheDocument();
    // Time-domain finding: no band chip renders.
    expect(screen.queryByText("Lows")).not.toBeInTheDocument();
    // Only the chain rx is applicable (the advisory has nothing to apply).
    expect(
      screen.getAllByRole("button", { name: /apply to the unit/i }),
    ).toHaveLength(1);
  });
});

describe("DoctorResults — severity (possible verdicts)", () => {
  beforeEach(resetMocks);

  // One sound with two equal-sev-tint diagnoses so severity is the ONLY
  // differentiator: a confidently-past-threshold finding (3.0) and a
  // near-threshold one (0.4, below POSSIBLE_MAX_SEVERITY).
  function severityFixture(): DoctorCheckResult {
    const mk = (key: string, label: string, severity: number): DoctorDiag => ({
      key,
      label,
      sev: "med",
      severity,
      bands: [],
      detail: `${label} detail`,
      fromLevel: "quiet",
      explain: `${label} explanation.`,
      rx: [
        {
          kind: "advisory",
          title: `Fix ${label}`,
          detail: "Advisory.",
          cpuNote: "",
          ops: [],
        },
      ],
    });
    return {
      presets: [
        {
          listIndex: 0,
          sounds: [
            {
              key: "p0",
              listIndex: 0,
              scene: null,
              footswitch: null,
              label: "Edge Rhythm",
              tag: null,
              // Deliberately possible-first in the source array — the UI must
              // REORDER it below the confident one.
              diags: [mk("fizzy", "Fizzy", 0.4), mk("harsh", "Harsh", 3.0)],
              integratedLufs: -18,
              tailRatioDb: 0,
              balanceDb: [0, 0, 0, 0, 0, 0],
              bandLabels: [
                "Lows",
                "Low-mids",
                "Mids",
                "High-mids",
                "Highs",
                "Air",
              ],
              error: null,
            },
          ],
          sceneConsistency: null,
        },
      ],
      stopped: false,
    };
  }

  it("mutes a low-severity finding as 'Possible …' and leaves a confident one plain", () => {
    renderResults(severityFixture());
    // Low severity (0.4 < POSSIBLE_MAX_SEVERITY) → the "possible" chip treatment.
    expect(screen.getByText("Possible Fizzy")).toBeInTheDocument();
    // High severity (3.0) → the plain label, no "Possible" prefix.
    expect(screen.getByText("Harsh")).toBeInTheDocument();
    expect(screen.queryByText("Possible Harsh")).not.toBeInTheDocument();
  });

  it("ranks the confident finding above the 'possible' one", () => {
    const { container } = renderResults(severityFixture());
    const text = container.textContent;
    // Confident (Harsh) renders before possible (Fizzy) despite the source order.
    expect(text.indexOf("Harsh")).toBeGreaterThanOrEqual(0);
    expect(text.indexOf("Harsh")).toBeLessThan(text.indexOf("Possible Fizzy"));
  });
});
