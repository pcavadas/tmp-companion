// src/__tests__/SetupPage.fsSceneContext.test.tsx — BUG→GATE #286: a footswitch row's
// measurement scene came from a lazy read fired only when its scene picker was opened, so
// the same selection measured the switch in base or in its enabling scene depending on a
// click. The suggested scene now comes from the startup backup scan for every footswitch
// row up front, and Start waits for it. Look B: the "Level it in scene" dropdown shows it;
// an unread one stays empty, the row says so, and the footer's link points at it.
//
// (#284, the re-run that reset every sound to the first target and instrument, lived here
// too while the Summary reopened Set up; Try again no longer does, so its gate moved to
// `useLevelingFlow-try-again.test.ts`.)

import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ensureLibraryScan, resetLibraryScan } from "../device/libraryScan";
import type { SetupOption } from "../views/level/leveling";
import type { FsSceneContext } from "../lib/types";
import {
  fired,
  makeBackupPresetRow,
  makeBackupRead,
  mockCommands,
  rejectWith,
} from "./commandMocks";
import {
  choose,
  renderSetupPage,
  selectTrigger,
  startLeveling,
} from "./setupPageTestUtils";

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

const renderSetup = (options: SetupOption[]) =>
  renderSetupPage(options, {
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

describe("a footswitch row's scene context comes from data (#286)", () => {
  it("shows and submits the suggested scene without the dropdown ever being opened", async () => {
    await seedScan([{ switch: 2, enablingScenes: [1], suggested: 1 }]);
    vi.mocked(invoke).mockClear();
    const onStart = renderSetup([fsOption()]);
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
    const onStart = renderSetup([fsOption()]);
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
    const onStart = renderSetup([fsOption()]);
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
    const onStart = renderSetup([fsOption()]);
    const user = userEvent.setup();
    await choose(user, "scene:f0:sw2", "base");
    await startLeveling(user);
    expect(onStart.mock.calls[0][0][0].option.footswitch).toMatchObject({
      sceneContext: null,
    });
  });
});
