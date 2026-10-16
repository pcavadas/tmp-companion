// src/__tests__/SetupPage.rerunAndContext.test.tsx — BUG→GATE for two Set-up defaults.
//
// #284: "Run again" / "Re-run off target…" reopened Set up with every row reset to the
// FIRST target and FIRST instrument, so one Start re-leveled a Lead sound at Rhythm. A
// re-run row now carries the instrument and target it was leveled with, and Set up seeds
// from them (falling back to the defaults only for an id the pickers no longer offer).
//
// #286: a footswitch row's measurement scene came from a lazy read fired only when its
// scene picker was opened, so the same selection measured the switch in base or in its
// enabling scene depending on a click. The suggested scene now comes from the startup
// backup scan for every footswitch row up front, and Start waits for it. Look B: the
// "Level it in scene" dropdown shows it; an unread one stays empty, the row says so, and
// the footer's link points at it (Q3).

import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ensureLibraryScan, resetLibraryScan } from "../device/libraryScan";
import {
  optionToRunItem,
  runItemToOption,
  type SetupOption,
} from "../views/level/leveling";
import type { FsSceneContext } from "../lib/types";
import {
  fired,
  makeBackupPresetRow,
  makeBackupRead,
  makeSetupOption,
  mockCommands,
  rejectWith,
} from "./commandMocks";
import {
  choose,
  renderSetupPage,
  selectTrigger,
  startLeveling,
} from "./setupPageTestUtils";

const baseOption = (): SetupOption => makeSetupOption({ presetName: "Plexi" });

function fsOption(): SetupOption {
  return {
    key: "f0:0",
    slot: 0,
    presetName: "Plexi",
    isBase: false,
    sceneSlot: null,
    sceneName: "Boost",
    tag: "FS3",
    hasScenes: true,
    footswitch: {
      switchIndex: 2,
      levGroupId: "G1",
      levNodeId: "ACD_Boost",
      levParameterId: "gain",
      sceneContext: null,
    },
    levelParams: [
      {
        group_id: "G1",
        node_id: "ACD_Boost",
        fender_id: "ACD_Boost",
        parameter_id: "gain",
        current: 0.5,
        class: "level_linear",
      },
    ],
    fsSceneNames: ["Clean", "Crunch", "Lead"],
  };
}

const renderSetup = (options: SetupOption[], isRelevel: boolean) =>
  renderSetupPage(options, {
    isRelevel,
    defaultInst: "strat",
    targetNames: ["Rhythm", "Lead"],
  }).onStart;

/** Seed the startup backup scan with slot 0 carrying `contexts`. */
async function seedScan(contexts: FsSceneContext[]) {
  mockCommands({
    read_library_via_backup: () =>
      makeBackupRead({
        total_rows: 1,
        presets: [
          makeBackupPresetRow({
            slot: 1,
            name: "Plexi",
            scene_count: 3,
            fs_scene_contexts: contexts,
          }),
        ],
      }),
  });
  await ensureLibraryScan();
}

beforeEach(() => {
  resetLibraryScan();
  vi.mocked(invoke).mockReset();
  mockCommands();
});

describe("a re-run keeps each sound's instrument and target (#284)", () => {
  it("carries instId and targetName through runItemToOption", () => {
    const item = optionToRunItem(baseOption(), "tele", "Lead");
    expect(runItemToOption(item)).toMatchObject({
      instId: "tele",
      targetName: "Lead",
    });
  });

  it("seeds the dropdowns from them, so Start submits the original settings", async () => {
    const rerun = runItemToOption(
      optionToRunItem(baseOption(), "tele", "Lead"),
    );
    const onStart = renderSetup([rerun], true);
    // The preset's instrument and the row's target show them, not the defaults.
    expect(selectTrigger("inst:0")).toHaveAccessibleName(
      "Instrument for Plexi: Tele",
    );
    expect(selectTrigger("target:p0")).toHaveAccessibleName(
      "Target for Plexi, Main sound: Lead",
    );
    const user = userEvent.setup();
    // A re-level asks for no backup tick (already given).
    await user.click(screen.getByRole("button", { name: "Start leveling" }));
    expect(onStart.mock.calls[0][0][0]).toMatchObject({
      instId: "tele",
      targetName: "Lead",
    });
  });

  it("falls back to the defaults for an id the dropdowns no longer offer", async () => {
    const rerun = runItemToOption(
      optionToRunItem(baseOption(), "deleted-profile", "Removed target"),
    );
    const onStart = renderSetup([rerun], true);
    // Shown is what is used: the default instrument, the by-name (here the first) target.
    expect(selectTrigger("inst:0")).toHaveAccessibleName(
      "Instrument for Plexi: Strat",
    );
    expect(selectTrigger("target:p0")).toHaveAccessibleName(
      "Target for Plexi, Main sound: Rhythm",
    );
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Start leveling" }));
    expect(onStart.mock.calls[0][0][0]).toMatchObject({
      instId: "strat",
      targetName: "Rhythm",
    });
  });

  it("keeps the scene a re-run footswitch was measured in", async () => {
    await seedScan([{ switch: 2, enablingScenes: [2], suggested: 2 }]);
    const fs = fsOption();
    const measuredIn: SetupOption = {
      ...fs,
      footswitch: fs.footswitch ? { ...fs.footswitch, sceneContext: 0 } : null,
    };
    const rerun = runItemToOption(optionToRunItem(measuredIn, "tele", "Lead"));
    const onStart = renderSetup([rerun], true);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Start leveling" }));
    expect(onStart.mock.calls[0][0][0].option.footswitch).toMatchObject({
      sceneContext: 0,
    });
  });
});

describe("a footswitch row's scene context comes from data (#286)", () => {
  it("shows and submits the suggested scene without the dropdown ever being opened", async () => {
    await seedScan([{ switch: 2, enablingScenes: [1], suggested: 1 }]);
    vi.mocked(invoke).mockClear();
    const onStart = renderSetup([fsOption()], false);
    await waitFor(() => {
      expect(selectTrigger("scene:f0:sw2")).toHaveAccessibleName(
        "Scene to level footswitch 3 of Plexi in: Crunch",
      );
    });
    const user = userEvent.setup();
    await startLeveling(user);
    expect(onStart.mock.calls[0][0][0].option.footswitch).toMatchObject({
      switchIndex: 2,
      sceneContext: 1,
    });
    // Answered off the backup scan: no device read.
    expect(fired("list_footswitch_scene_contexts")).toBe(false);
  });

  it("holds Start until a device-read context lands, then submits it", async () => {
    // No backup entry for the slot: the context comes from the device read.
    let answer: (rows: FsSceneContext[]) => void = () => undefined;
    mockCommands({
      list_footswitch_scene_contexts: () =>
        new Promise<FsSceneContext[]>((res) => {
          answer = res;
        }),
    });
    const onStart = renderSetup([fsOption()], false);
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("checkbox", { name: "I've backed up with Pro Control" }),
    );
    // Start keeps its words and waits, busy; the footer says what it waits for.
    const start = screen.getByRole("button", { name: "Start leveling" });
    expect(start).toBeDisabled();
    expect(start).toHaveAttribute("aria-busy", "true");
    expect(screen.getByText("Reading footswitch scenes")).toBeInTheDocument();
    expect(selectTrigger("scene:f0:sw2")).toHaveAccessibleName(
      "Scene to level footswitch 3 of Plexi in: reading",
    );
    expect(fired("list_footswitch_scene_contexts")).toBe(true);

    answer([{ switch: 2, enablingScenes: [2], suggested: 2 }]);
    await waitFor(() => {
      expect(start).toBeEnabled();
    });
    await user.click(start);
    expect(onStart.mock.calls[0][0][0].option.footswitch).toMatchObject({
      sceneContext: 2,
    });
  });

  it("holds Start on a failed read until a scene is chosen, and points at it", async () => {
    mockCommands({
      list_footswitch_scene_contexts: () =>
        rejectWith(new Error("Device not connected")),
    });
    const onStart = renderSetup([fsOption()], false);
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("checkbox", { name: "I've backed up with Pro Control" }),
    );
    const link = await screen.findByRole("button", {
      name: "Choose a scene for footswitch 3",
    });
    expect(
      screen.getByRole("button", { name: "Start leveling" }),
    ).toBeDisabled();
    expect(
      screen.getByText("Couldn't read which scene to level footswitch 3 in."),
    ).toBeInTheDocument();
    // The empty dropdown says nothing is chosen, in the attention look.
    const trigger = selectTrigger("scene:f0:sw2");
    expect(trigger).toHaveAccessibleName(
      "Scene to level footswitch 3 of Plexi in: not chosen",
    );
    expect(trigger).toHaveAttribute("data-attention");

    // The footer link opens that row's dropdown.
    await user.click(link);
    expect(
      screen.getByRole("listbox", {
        name: "Scene to level footswitch 3 of Plexi in",
      }),
    ).toBeInTheDocument();
    // An explicit pick (even the main sound) is the user's answer, so Start opens.
    await user.click(screen.getByRole("option", { name: "Crunch" }));
    expect(
      screen.queryByRole("button", { name: "Choose a scene for footswitch 3" }),
    ).toBeNull();
    await user.click(screen.getByRole("button", { name: "Start leveling" }));
    expect(onStart.mock.calls[0][0][0].option.footswitch).toMatchObject({
      sceneContext: 1,
    });
  });

  it("an explicit Main sound pick submits the base context", async () => {
    await seedScan([{ switch: 2, enablingScenes: [1], suggested: 1 }]);
    const onStart = renderSetup([fsOption()], false);
    const user = userEvent.setup();
    await choose(user, "scene:f0:sw2", "base");
    await startLeveling(user);
    expect(onStart.mock.calls[0][0][0].option.footswitch).toMatchObject({
      sceneContext: null,
    });
  });
});
