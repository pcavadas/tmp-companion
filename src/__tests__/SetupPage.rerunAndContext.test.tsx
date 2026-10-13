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
// backup scan for every footswitch row up front, and Start waits for it.

import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { SetupPage, type SetupChoice } from "../views/level/SetupPage";
import { WithCard } from "./pickCardTestUtils";
import { ensureLibraryScan, resetLibraryScan } from "../device/libraryScan";
import {
  optionToRunItem,
  runItemToOption,
  type SetupOption,
} from "../views/level/leveling";
import type { FsSceneContext } from "../lib/types";
import type { PickOption } from "../views/overlays/Pick";
import {
  fired,
  makeBackupPresetRow,
  makeBackupRead,
  makeSetupOption,
  mockCommands,
  rejectWith,
} from "./commandMocks";
import { renderWithTheme } from "./testKit";

const instrumentOptions: PickOption[] = [
  { id: "none", label: "None" },
  { id: "strat", label: "Strat" },
  { id: "tele", label: "Tele" },
];
const targetOptions: PickOption[] = [
  { id: "Rhythm", label: "Rhythm −27" },
  { id: "Lead", label: "Lead −23" },
];

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
    fsSceneNames: ["Clean", "Crunch", "Lead"],
  };
}

function renderSetup(options: SetupOption[], isRelevel: boolean) {
  const onStart = vi.fn<(c: SetupChoice[]) => void>();
  renderWithTheme(
    <WithCard>
      <SetupPage
        options={options}
        isRelevel={isRelevel}
        instrumentOptions={instrumentOptions}
        targetOptions={targetOptions}
        defaultInst="strat"
        defaultTarget="Rhythm"
        onCancel={vi.fn()}
        onStart={onStart}
      />
    </WithCard>,
  );
  return onStart;
}

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

  it("seeds the rows from them, so Start submits the original settings", async () => {
    const rerun = runItemToOption(
      optionToRunItem(baseOption(), "tele", "Lead"),
    );
    const onStart = renderSetup([rerun], true);
    // The bulk pickers show the shared values too, not the first profile and target.
    expect(screen.getAllByText("Tele").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Lead −23").length).toBeGreaterThan(0);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /start.*1 sound/i }));
    expect(onStart.mock.calls[0][0][0]).toMatchObject({
      instId: "tele",
      targetName: "Lead",
    });
  });

  it("falls back to the defaults for an id the pickers no longer offer", async () => {
    const rerun = runItemToOption(
      optionToRunItem(baseOption(), "deleted-profile", "Removed target"),
    );
    const onStart = renderSetup([rerun], true);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /start.*1 sound/i }));
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
    await user.click(screen.getByRole("button", { name: /start.*1 sound/i }));
    expect(onStart.mock.calls[0][0][0].option.footswitch).toMatchObject({
      sceneContext: 0,
    });
  });
});

describe("a footswitch row's scene context comes from data (#286)", () => {
  it("submits the suggested scene without the picker ever being opened", async () => {
    await seedScan([{ switch: 2, enablingScenes: [1], suggested: 1 }]);
    vi.mocked(invoke).mockClear();
    const onStart = renderSetup([fsOption()], false);
    const user = userEvent.setup();
    await user.click(screen.getByText(/I.ve backed up with Pro Control/i));
    const start = await screen.findByRole("button", {
      name: /start.*1 sound/i,
    });
    await waitFor(() => {
      expect(start).toBeEnabled();
    });
    await user.click(start);
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
    await user.click(screen.getByText(/I.ve backed up with Pro Control/i));
    const pending = screen.getByRole("button", {
      name: /reading footswitches/i,
    });
    expect(pending).toBeDisabled();
    expect(fired("list_footswitch_scene_contexts")).toBe(true);

    answer([{ switch: 2, enablingScenes: [2], suggested: 2 }]);
    const start = await screen.findByRole("button", {
      name: /start.*1 sound/i,
    });
    await user.click(start);
    expect(onStart.mock.calls[0][0][0].option.footswitch).toMatchObject({
      sceneContext: 2,
    });
  });

  it("holds Start on a failed read until a scene is chosen", async () => {
    mockCommands({
      list_footswitch_scene_contexts: () =>
        rejectWith(new Error("Device not connected")),
    });
    const withPicker: SetupOption = {
      ...fsOption(),
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
    };
    const onStart = renderSetup([withPicker], false);
    const user = userEvent.setup();
    await user.click(screen.getByText(/I.ve backed up with Pro Control/i));
    const blocked = await screen.findByRole("button", {
      name: /choose a footswitch scene/i,
    });
    expect(blocked).toBeDisabled();
    expect(screen.getByText(/Couldn.t read FS3.s scenes/)).toBeInTheDocument();

    // An explicit pick (even Base) is the user's answer, so Start opens.
    const picker = document.querySelector<HTMLElement>('[data-pick="fsctx:2"]');
    if (!picker) throw new Error("scene picker not rendered");
    await user.click(picker);
    const crunch = document.querySelector<HTMLElement>(
      '[data-pick-option="fsctx:2:1"]',
    );
    if (!crunch) throw new Error("scene option not rendered");
    await user.click(crunch);
    await user.click(screen.getByRole("button", { name: /start.*1 sound/i }));
    expect(onStart.mock.calls[0][0][0].option.footswitch).toMatchObject({
      sceneContext: 1,
    });
  });
});
