// src/__tests__/SummaryPage.test.tsx — the leveling Summary page (look B): its title and
// sections, one plain sentence per sound with no loudness numbers, at most one badge, one
// Notice, "Try again (N)" before "Done", and the footswitch scene choice. What each row
// says is pinned in `summaryModel.test.ts`; this suite pins what reaches the screen and
// what the buttons do. It replaces the Summary* suites of the old wizard (unconverged vs
// clamped, true peak, wet floor, base boost, re-amp warning), ported to look B.

import { beforeEach, describe, expect, it } from "vitest";
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { ensureLibraryScan, resetLibraryScan } from "../device/libraryScan";
import {
  makeBackupPresetRow,
  makeBackupRead,
  mockCommands,
} from "./commandMocks";
import { base, fsRow, renderSummary, sceneRow } from "./summaryTestUtils";

beforeEach(() => {
  resetLibraryScan();
  mockCommands();
});

describe("Summary page", () => {
  it("all leveled: the title, the saved line, one Leveled group, only Done", async () => {
    const { onDone } = renderSummary([base({}), sceneRow(0, {}), fsRow({})]);
    expect(
      screen.getByRole("heading", { name: "All 3 sounds leveled." }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Everything is saved on your Tone Master Pro."),
    ).toBeInTheDocument();
    expect(screen.getByText("Summary")).toHaveAttribute("aria-current", "step");
    const leveled = screen.getByRole("region", { name: "Leveled" });
    expect(within(leveled).getAllByText("On target")).toHaveLength(3);
    // A lone section shows no label: the title says it.
    expect(screen.queryByRole("heading", { name: "Leveled" })).toBeNull();
    expect(
      within(leveled).getByText("Boost, footswitch 1"),
    ).toBeInTheDocument();
    // No loudness numbers on the Summary.
    expect(screen.queryByText(/[−-]\d+\.\d/)).toBeNull();
    expect(screen.queryByRole("button", { name: /Try again/ })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(onDone).toHaveBeenCalledOnce();
  });

  // The two miss states differ in the next action (was SummaryUnconverged.test.tsx): an
  // off-target sound still had knob room, so Try again covers it; a sound already at full
  // volume says what to change and gets no button (running it again can't help).
  it("off target is tried again; full volume says what to change, with no button", async () => {
    const { onTryAgain } = renderSummary([
      fsRow({ outcome: "unconverged" }),
      base({ outcome: "clamped", value: -24, clampKind: "scene_ceiling" }),
    ]);
    expect(
      screen.getByRole("heading", {
        name: "Nothing was leveled. 2 sounds need you.",
      }),
    ).toBeInTheDocument();
    const need = screen.getByRole("region", { name: "Need you" });
    expect(
      within(need).getByText("Didn't reach its target this time."),
    ).toBeInTheDocument();
    expect(
      within(need).getByText(
        "Already at full volume, so it can't reach Lead. Choose a quieter target for it.",
      ),
    ).toBeInTheDocument();
    // No per-row buttons of the old wizard.
    for (const old of [
      /Run again/,
      /Level lower/,
      /Retry/,
      /Re-level clamped/,
      /Re-run off target/,
      /Accept/,
    ])
      expect(screen.queryByRole("button", { name: old })).toBeNull();
    await userEvent.click(
      screen.getByRole("button", { name: "Try again (1)" }),
    );
    expect(onTryAgain).toHaveBeenCalledWith(["f3:0"]);
  });

  it("a stopped run: the sounds it didn't reach have their own section, and Try again covers them", () => {
    renderSummary(
      [
        base({}),
        sceneRow(0, { outcome: "skipped", skipKind: "cancelled" }),
        sceneRow(1, { outcome: undefined, status: "queued" }),
      ],
      { stopped: true },
    );
    expect(
      screen.getByRole("heading", {
        name: "Stopped. 1 sound leveled, 2 not reached.",
      }),
    ).toBeInTheDocument();
    // Two sections (a stopped run): each under its label.
    expect(
      screen.getByRole("heading", { name: "Not reached" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Leveled" }),
    ).toBeInTheDocument();
    const notReached = screen.getByRole("region", { name: "Not reached" });
    expect(
      within(notReached).getAllByText("Not reached", { selector: "span" }),
    ).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Try again (2)" })).toBeEnabled();
  });

  // The save after a run is checked: a sound whose save didn't hold needs you.
  it("a save that didn't hold needs you, and is tried again", () => {
    renderSummary([base({ persistMismatch: true })]);
    expect(
      screen.getByText("Leveled, but the save didn't hold. Try again."),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Try again (1)" })).toBeEnabled();
  });

  // Was SummaryTruePeak.test.tsx / SummaryWetFloor.test.tsx.
  it("loud peaks and check by ear: one badge per row and its sentence", () => {
    renderSummary([
      base({ truePeakDbtp: -0.4 }),
      fsRow({ verifyByEar: "wet_floor" }),
    ]);
    expect(screen.getByText("Loud peaks")).toBeInTheDocument();
    expect(
      screen.getByText(
        "It peaks close to the maximum. If your interface or speaker distorts on it, level it to a quieter target.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("Check by ear")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Its effect is at the lowest mix that keeps it in the sound. Listen to check it.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText("may clip")).toBeNull();
    expect(screen.queryByText("by ear")).toBeNull();
  });

  // Was SummaryBaseBoost.test.tsx: words only, no knob values.
  it("a main sound whose preset level ran out says the amp's output went up, without numbers", () => {
    const { view } = renderSummary([
      base({
        baseBoost: {
          applied: true,
          preset_level: 1,
          base_amps: [
            {
              group_id: "G1",
              node_id: "amp1",
              parameter_id: "outputLevel",
              previous_value: 0.28,
              value: 0.51,
            },
          ],
        },
      }),
    ]);
    expect(
      screen.getByText(
        "Turned the preset up as far as it goes, then raised the amp's output to reach Lead.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText(/0\.28|0\.51/)).toBeNull();
    expect(
      view.container.querySelector('[data-base-boost="applied"]'),
    ).not.toBeNull();
  });

  // Was SummaryReampWarning.test.tsx: keyed on the typed flag, the flow's latest reading.
  it("re-amp may still be on: one Notice that says to restart the unit", () => {
    renderSummary(
      [
        base({ reampStillOn: true }),
        base({ key: "p4", slot: 4, presetName: "Clean", reampStillOn: true }),
      ],
      { reampStillOn: true },
    );
    expect(
      screen.getAllByText(
        "Your guitar input may still be muted. Restart your Tone Master Pro.",
      ),
    ).toHaveLength(1);
    expect(screen.queryByText("Re-amp may still be on")).toBeNull();
  });

  it("a unit set to throw away scene changes: one row says how to fix it, with no Try again", () => {
    renderSummary([
      base({}),
      sceneRow(0, { outcome: "skipped", skipKind: "scene_change_discards" }),
      sceneRow(1, { outcome: "skipped", skipKind: "scene_change_discards" }),
    ]);
    expect(
      screen.getByRole("heading", { name: "1 sound leveled. 2 need you." }),
    ).toBeInTheDocument();
    const need = screen.getByRole("region", { name: "Need you" });
    expect(
      within(need).getAllByText(
        "Your Tone Master Pro is set to throw away scene changes, so leveling can't save them. On your Tone Master Pro, set Scene Change Behavior to Maintain changes. Then unplug it, plug it back in, and level it again.",
      ),
    ).toHaveLength(2);
    expect(screen.queryByText(/Couldn't level this sound/)).toBeNull();
    expect(screen.queryByRole("button", { name: /Try again/ })).toBeNull();
  });

  it("the footswitch scene choice adds the sound to Try again", async () => {
    mockCommands({
      read_library_via_backup: () =>
        makeBackupRead({
          total_rows: 1,
          presets: [
            makeBackupPresetRow({
              slot: 4, // list index 3
              name: "Guitar",
              scene_count: 3,
              scenes: [
                { name: "Clean", fs: 1 },
                { name: "Crunch", fs: 2 },
                { name: "Lead", fs: 3 },
              ],
              fs_scene_contexts: [
                { switch: 2, enablingScenes: [2], suggested: 2 },
              ],
            }),
          ],
        }),
    });
    await ensureLibraryScan();
    const row = fsRow({
      outcome: "skipped",
      skipKind: "block_off",
      footswitch: {
        switchIndex: 2,
        levGroupId: "G1",
        levNodeId: "boost",
        levParameterId: "level",
        sceneContext: 1,
      },
    });
    const { onSceneChange, view } = renderSummary([row]);
    expect(
      screen.getByText(
        "Footswitch 3 doesn't turn its effect on in Crunch, so there was nothing to level. Choose another scene for it.",
      ),
    ).toBeInTheDocument();
    // Not tried again until its scene changes.
    expect(screen.queryByRole("button", { name: /Try again/ })).toBeNull();
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", {
        name: "Scene to level footswitch 3 of Guitar in: Crunch",
      }),
    );
    await user.click(screen.getByRole("option", { name: "Lead" }));
    expect(onSceneChange).toHaveBeenLastCalledWith("f3:0", 2);

    // The store holds the pick; with it, Try again counts the sound.
    view.unmount();
    const again = renderSummary([row], { sceneOverrides: { "f3:0": 2 } });
    expect(
      screen.getByRole("button", {
        name: "Scene to level footswitch 3 of Guitar in: Lead",
      }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Try again (1)" })).toBeEnabled();
    // Picking the scene it was leveled in again takes it back out.
    await user.click(
      screen.getByRole("button", {
        name: "Scene to level footswitch 3 of Guitar in: Lead",
      }),
    );
    await user.click(screen.getByRole("option", { name: "Crunch" }));
    expect(again.onSceneChange).toHaveBeenLastCalledWith("f3:0", undefined);
  });
});
