// src/__tests__/DoctorResults.test.tsx — the Doctor RESULTS page: summary line,
// worst-first ordering, per-card status badges, chip expand, the opt-in frequency
// toggle, and the prescription lifecycle (apply → ack-gated save → saved; discard).
// The device commands (apply/save/discard) are mocked.

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
                bands: [1],
                detail: "+4.2 dB around 250 Hz",
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
                bands: [],
                detail: "tail 6 dB over dry",
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
              kind: "oneclick",
              title: "Trim the Crunch scene by 6 dB",
              detail: "Levels the stomp between sounds.",
              cpuNote: "0% CPU",
              ops: [{ kind: "scene_trim", scene: 0, targetDeltaDb: -6 }],
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
            error: "The capture came back silent — check the cable.",
          },
        ],
        sceneConsistency: null,
      },
    ],
    stopped: false,
    cohort: "absolute",
  };
}

function renderResults(onCheckMore: () => void = () => undefined) {
  return render(
    <ThemeProvider>
      <DoctorResults
        result={fixture()}
        presetNames={NAMES}
        footswitchInfo={new Map()}
        onCheckMore={onCheckMore}
      />
    </ThemeProvider>,
  );
}

describe("DoctorResults — summary + cards", () => {
  beforeEach(() => {
    vi.mocked(doctorApply).mockReset();
    vi.mocked(doctorSave).mockReset();
    vi.mocked(doctorDiscard).mockReset();
    vi.mocked(doctorApply).mockResolvedValue({
      beforeClip: "data:audio/wav;base64,AAAA",
      afterClip: "data:audio/wav;base64,BBBB",
    });
    vi.mocked(doctorSave).mockResolvedValue(undefined);
    vi.mocked(doctorDiscard).mockResolvedValue(undefined);
  });

  it("summarizes worst-first with the right counts", () => {
    renderResults();
    expect(screen.getByText("1 of 3 presets need a look")).toBeInTheDocument();
    // Plural sounds flagged, singular need attention.
    expect(
      screen.getByText(/2 sounds flagged · 1 needs attention/),
    ).toBeInTheDocument();
  });

  it("shows per-card status badges (singular error/all-clear + plural)", () => {
    renderResults();
    // The high preset: 2 diagnoses + 1 scene finding = 3 things (plural).
    expect(screen.getByText("3 things to look at")).toBeInTheDocument();
    // The all-clear preset and the errored preset both read "All clear".
    expect(screen.getAllByText("All clear")).toHaveLength(2);
  });

  it("orders the cards worst-first, ties broken by slot", () => {
    const { container } = renderResults();
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

  it("expands a chip to show its explanation + detail", async () => {
    const user = userEvent.setup();
    renderResults();
    const explain =
      "Low-mids are piling up and swallowing your note definition.";
    expect(screen.queryByText(explain)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Muddy" }));
    expect(screen.getByText(explain)).toBeInTheDocument();
    expect(screen.getByText("+4.2 dB around 250 Hz")).toBeInTheDocument();
  });

  it("offers the frequency toggle only for banded diagnoses", async () => {
    const user = userEvent.setup();
    renderResults();
    // Open BOTH diagnoses; only the banded one (Muddy) offers "Show the frequencies".
    await user.click(screen.getByRole("button", { name: "Muddy" }));
    await user.click(screen.getByRole("button", { name: "Washed out" }));
    expect(screen.getAllByText("Show the frequencies")).toHaveLength(1);
  });

  it("gives an advisory prescription no Apply button", async () => {
    const user = userEvent.setup();
    renderResults();
    await user.click(screen.getByRole("button", { name: "Muddy" }));
    // Only the oneclick rx is applicable; the advisory is static.
    expect(
      screen.getAllByRole("button", { name: /apply to the unit/i }),
    ).toHaveLength(1);
    expect(screen.getByText("Nudge Bass down a notch")).toBeInTheDocument();
  });

  it("gives a scene-consistency prescription no Apply button", () => {
    renderResults();
    // The scene section is always visible; its fix is advised, not applied.
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
  beforeEach(() => {
    vi.mocked(doctorApply).mockReset();
    vi.mocked(doctorSave).mockReset();
    vi.mocked(doctorDiscard).mockReset();
    vi.mocked(doctorApply).mockResolvedValue({
      beforeClip: "data:audio/wav;base64,AAAA",
      afterClip: "data:audio/wav;base64,BBBB",
    });
    vi.mocked(doctorSave).mockResolvedValue(undefined);
    vi.mocked(doctorDiscard).mockResolvedValue(undefined);
  });

  it("applies, auditions, then ack-gates the save through to saved", async () => {
    const user = userEvent.setup();
    renderResults();

    await user.click(screen.getByRole("button", { name: "Muddy" }));
    await user.click(
      screen.getByRole("button", { name: /apply to the unit/i }),
    );

    // Applied → the A/B audition + the unsaved pill appear.
    expect(await screen.findByText("Listen & compare")).toBeInTheDocument();
    expect(
      screen.getByText("Applied to the unit · not saved"),
    ).toBeInTheDocument();
    expect(doctorApply).toHaveBeenCalledWith(
      expect.objectContaining({ listIndex: 1, name: "Muddy Rhythm" }),
    );

    // Save is gated on the backup acknowledgment.
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

    await user.click(screen.getByRole("button", { name: "Muddy" }));
    await user.click(
      screen.getByRole("button", { name: /apply to the unit/i }),
    );
    const discard = await screen.findByRole("button", { name: /discard/i });
    await user.click(discard);

    // Back to draft — the Apply button returns and nothing was saved.
    expect(
      await screen.findByRole("button", { name: /apply to the unit/i }),
    ).toBeInTheDocument();
    expect(doctorDiscard).toHaveBeenCalledWith(1);
    expect(doctorSave).not.toHaveBeenCalled();
  });

  it("locks sibling Apply buttons from the moment an apply is IN FLIGHT", async () => {
    // A hanging apply: the lock must be taken BEFORE the command resolves, so a
    // sibling can't fire into the same device edit buffer mid-flight.
    let resolveApply: (r: DoctorApplyResult) => void = () => undefined;
    vi.mocked(doctorApply).mockImplementation(
      () =>
        new Promise<DoctorApplyResult>((res) => {
          resolveApply = res;
        }),
    );
    const user = userEvent.setup();
    renderResults();

    await user.click(screen.getByRole("button", { name: "Muddy" }));
    await user.click(screen.getByRole("button", { name: "Washed out" }));
    const applies = screen.getAllByRole("button", {
      name: /apply to the unit/i,
    });
    expect(applies).toHaveLength(2);
    await user.click(applies[0]);

    // The busy card reads "Applying…", so the one remaining Apply button is the
    // sibling — disabled, with the lock explainer.
    const sibling = screen.getByRole("button", { name: /apply to the unit/i });
    expect(sibling).toBeDisabled();
    expect(
      screen.getByText("Save or discard the applied fix first."),
    ).toBeInTheDocument();

    // Still locked once the apply lands (applied-but-unsaved).
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
    const { unmount } = renderResults(onCheckMore);

    await user.click(screen.getByRole("button", { name: "Muddy" }));
    await user.click(
      screen.getByRole("button", { name: /apply to the unit/i }),
    );
    await screen.findByText("Listen & compare");

    await user.click(
      screen.getByRole("button", { name: /check other sounds/i }),
    );
    expect(doctorDiscard).toHaveBeenCalledWith(1);
    expect(onCheckMore).toHaveBeenCalled();

    // The reset path already discarded — the unmount cleanup must not re-fire.
    unmount();
    expect(doctorDiscard).toHaveBeenCalledTimes(1);
  });

  it("discards the applied-but-unsaved edit on unmount", async () => {
    const user = userEvent.setup();
    const { unmount } = renderResults();

    await user.click(screen.getByRole("button", { name: "Muddy" }));
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

    await user.click(screen.getByRole("button", { name: "Muddy" }));
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
  const CAPTION =
    "This block is shared — the change affects all sounds of this preset.";

  // A single-preset result whose one sound carries a `muddy` diag with one
  // prescription op — the caller supplies the op + whether the sound is a
  // footswitch (key `f0:0`, so its FootswitchInfo array index is 0).
  function fsFixture(
    op: DoctorOp,
    footswitch: number | null,
  ): DoctorCheckResult {
    const sound: DoctorSoundResult = {
      key: footswitch == null ? "p0" : "f0:0",
      listIndex: 0,
      scene: null,
      footswitch,
      label: "Overdrive",
      tag: footswitch == null ? null : "FS4",
      diags: [
        {
          key: "muddy",
          label: "Muddy",
          sev: "high",
          bands: [1],
          detail: "+4 dB around 250 Hz",
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
        },
      ],
      integratedLufs: -18,
      tailRatioDb: 0,
      balanceDb: [-6, 4, -2, -8, -12, -18],
      error: null,
    };
    return {
      presets: [{ listIndex: 0, sounds: [sound], sceneConsistency: null }],
      stopped: false,
      cohort: "absolute",
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

  it("captions an FS fix that edits a block outside the switch's own set", async () => {
    const user = userEvent.setup();
    renderShared(fsFixture(paramOp("CAB1"), 3));
    await user.click(screen.getByRole("button", { name: "Muddy" }));
    expect(screen.getByText(CAPTION)).toBeInTheDocument();
  });

  it("omits the caption when the fix edits the switch's own block", async () => {
    const user = userEvent.setup();
    renderShared(fsFixture(paramOp("DRV1"), 3));
    await user.click(screen.getByRole("button", { name: "Muddy" }));
    expect(screen.queryByText(CAPTION)).not.toBeInTheDocument();
  });

  it("never captions a Base sound (footswitch == null)", async () => {
    const user = userEvent.setup();
    renderShared(fsFixture(paramOp("CAB1"), null));
    await user.click(screen.getByRole("button", { name: "Muddy" }));
    expect(screen.queryByText(CAPTION)).not.toBeInTheDocument();
  });
});
