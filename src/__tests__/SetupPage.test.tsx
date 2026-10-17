// src/__tests__/SetupPage.test.tsx — Level's Set up page (look B): every sound shows
// Target, Block and Knob pre-set to what the run will use; "Target for all" says "By
// name", a target or "Mixed"; one instrument per preset (saved for the preset), with
// "Instrument for all" only for 2+ presets and the "Record yours in Settings" link
// (which replaced the calibration banner and the instrument nudge); "Balance its two amps
// first" only where the backend can balance; a scene with no amp waits for a block;
// the backup tick is the only gate before "Start leveling".

import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderHook, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ensureLibraryScan, resetLibraryScan } from "../device/libraryScan";
import { blockStripName, type SetupOption } from "../views/level/leveling";
import { useSetupRows } from "../views/level/useSetupRows";
import type {
  BackupPresetRow,
  SceneDrive,
  SceneHandleCandidate,
} from "../lib/types";
import {
  makeBackupPresetRow,
  makeBackupRead,
  makeFootswitchOption,
  makeSceneOption,
  makeSetupOption,
  mockCommands,
} from "./commandMocks";
import {
  choose,
  renderSetupPage,
  selectTrigger,
  startLeveling,
} from "./setupPageTestUtils";

const TWIN = blockStripName("ACD_TwinReverb65NoFx");
const BOOST = blockStripName("ACD_Boost");

const twinAmp = {
  groupId: "G1",
  nodeId: "amp",
  fenderId: "ACD_TwinReverb65NoFx",
};
const boostHandle: SceneHandleCandidate = {
  groupId: "G1",
  nodeId: "boost",
  fenderId: "ACD_Boost",
  parameterId: "gain",
  class: "level_db",
  range: [0, 12],
  current: 5,
  scope: "isolated",
  headroom: "full",
};

const drive = (sceneSlot: number, over: Partial<SceneDrive>): SceneDrive => ({
  sceneSlot,
  amps: [],
  routing: "single",
  skip: null,
  ...over,
});

/** Seed the backup scan with presets at list indices 0.. (device slot = index + 1). */
async function seedScan(presets: Partial<BackupPresetRow>[]) {
  mockCommands({
    read_library_via_backup: () =>
      makeBackupRead({
        total_rows: presets.length,
        presets: presets.map((p, i) =>
          makeBackupPresetRow({ slot: i + 1, name: `P${String(i)}`, ...p }),
        ),
      }),
  });
  await ensureLibraryScan();
}

const main = (slot: number, presetName: string): SetupOption =>
  makeSetupOption({ key: `p${String(slot)}`, slot, presetName });
const scene = (slot: number, i: number, presetName: string, name: string) =>
  makeSceneOption(i, {
    key: `s${String(slot)}:${String(i)}`,
    slot,
    presetName,
    sceneName: name,
  });

beforeEach(() => {
  resetLibraryScan();
  vi.mocked(invoke).mockReset();
  mockCommands();
});

describe("the page", () => {
  it("titles the run by its sound count and shows where the flow is", () => {
    renderSetupPage([main(0, "Plexi"), scene(0, 0, "Plexi", "Lead")]);
    expect(
      screen.getByRole("heading", { name: "Level 2 sounds" }),
    ).toBeInTheDocument();
    const steps = screen.getByRole("list", { name: "Steps" });
    expect(within(steps).getByText("Set up")).toHaveAttribute(
      "aria-current",
      "step",
    );
  });

  it("says one sound in the singular", () => {
    renderSetupPage([main(0, "Plexi")]);
    expect(
      screen.getByRole("heading", { name: "Level 1 sound" }),
    ).toBeInTheDocument();
  });

  it("gates Start on the backup tick alone, with no saving line", async () => {
    const { onStart, onCancel } = renderSetupPage([main(0, "Plexi")]);
    const start = screen.getByRole("button", { name: "Start leveling" });
    expect(start).toBeDisabled();
    expect(screen.queryByText(/save/i)).toBeNull();
    const user = userEvent.setup();
    await startLeveling(user);
    expect(onStart).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});

describe("Block and Knob show what the run will use", () => {
  it('a main sound levels the preset: "Preset" / "Level"', () => {
    renderSetupPage([main(0, "Plexi")]);
    expect(selectTrigger("block:p0")).toHaveAccessibleName(
      "Block for Plexi, Main sound: Preset",
    );
    expect(selectTrigger("knob:p0")).toHaveAccessibleName(
      "Knob for Plexi, Main sound: Level",
    );
  });

  it('a scene names the amp the run drives, and "Both amps" for two lanes', async () => {
    await seedScan([
      {
        scene_drives: [
          drive(0, { amps: [twinAmp] }),
          drive(1, {
            amps: [twinAmp, { ...twinAmp, nodeId: "amp2" }],
            routing: "merged",
          }),
        ],
      },
    ]);
    renderSetupPage([
      scene(0, 0, "Split", "Verse"),
      scene(0, 1, "Split", "Chorus"),
    ]);
    expect(selectTrigger("block:s0:0")).toHaveAccessibleName(
      `Block for Split, Verse: ${TWIN}`,
    );
    expect(selectTrigger("knob:s0:0")).toHaveAccessibleName(
      "Knob for Split, Verse: Output level",
    );
    expect(selectTrigger("block:s0:1")).toHaveAccessibleName(
      "Block for Split, Chorus: Both amps",
    );
  });

  it('a footswitch shows its own block and knob, and "footswitch 3" by its name', () => {
    renderSetupPage([makeFootswitchOption(2, { presetName: "Rig" })]);
    expect(screen.getByText("footswitch 3")).toBeInTheDocument();
    expect(selectTrigger("knob:f0:sw2")).toHaveAccessibleName(
      "Knob for Rig, Switch 3: Level",
    );
  });
});

describe("Target goes by name", () => {
  it("gives a sound named like a target that target, others the first target", () => {
    renderSetupPage([main(0, "Plexi"), scene(0, 0, "Plexi", "lead 2")], {
      targetNames: ["Rhythm", "Crunch", "Lead"],
    });
    expect(selectTrigger("target:p0")).toHaveAccessibleName(
      "Target for Plexi, Main sound: Rhythm",
    );
    expect(selectTrigger("target:s0:0")).toHaveAccessibleName(
      "Target for Plexi, lead 2: Lead",
    );
    expect(selectTrigger("target-all")).toHaveAccessibleName(
      "Target for all sounds: By name",
    );
  });

  it('"Target for all" sets every sound, shows "Mixed" when rows differ, and "By name" resets', async () => {
    const opts = [main(0, "Plexi"), scene(0, 0, "Plexi", "Lead")];
    const { onStart } = renderSetupPage(opts);
    const user = userEvent.setup();

    await choose(user, "target-all", "t:Lead");
    expect(selectTrigger("target:p0")).toHaveAccessibleName(
      "Target for Plexi, Main sound: Lead",
    );
    expect(selectTrigger("target-all")).toHaveAccessibleName(
      "Target for all sounds: Lead",
    );

    await choose(user, "target:p0", "Rhythm");
    expect(selectTrigger("target-all")).toHaveAccessibleName(
      "Target for all sounds: Mixed",
    );

    await choose(user, "target-all", "by-name");
    expect(selectTrigger("target:p0")).toHaveAccessibleName(
      "Target for Plexi, Main sound: Rhythm",
    );
    expect(selectTrigger("target:s0:0")).toHaveAccessibleName(
      "Target for Plexi, Lead: Lead",
    );

    await startLeveling(user);
    expect(onStart.mock.calls[0][0].map((c) => c.targetName)).toEqual([
      "Rhythm",
      "Lead",
    ]);
  });

  it("a picked target that is no longer configured goes back to the one by name", async () => {
    const opts = [main(0, "Plexi"), scene(0, 0, "Plexi", "Lead")];
    const { onStart, rerender } = renderSetupPage(opts, {
      targetNames: ["Rhythm", "Crunch", "Lead"],
    });
    const user = userEvent.setup();
    await choose(user, "target:p0", "Crunch");
    // Crunch is deleted in Settings while the page stays open.
    rerender({ targetNames: ["Rhythm", "Lead"] });
    expect(selectTrigger("target:p0")).toHaveAccessibleName(
      "Target for Plexi, Main sound: Rhythm",
    );
    expect(selectTrigger("target-all")).toHaveAccessibleName(
      "Target for all sounds: By name",
    );
    await startLeveling(user);
    expect(onStart.mock.calls[0][0].map((c) => c.targetName)).toEqual([
      "Rhythm",
      "Lead",
    ]);
  });
});

// Settings can delete every target. With none, there is no loudness to level a sound to,
// so Start waits for one and points at Settings; the run never gets an empty target.
describe("with no target configured", () => {
  it('keeps Start disabled and shows "Add a target in Settings"', async () => {
    const onAddTarget = vi.fn();
    const { onStart } = renderSetupPage(
      [main(0, "Plexi"), scene(0, 0, "Plexi", "Lead")],
      { targetNames: [], onAddTarget },
    );
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("checkbox", { name: "I've backed up with Pro Control" }),
    );
    const start = screen.getByRole("button", { name: "Start leveling" });
    expect(start).toBeDisabled();
    await user.click(start);
    expect(onStart).not.toHaveBeenCalled();
    await user.click(
      screen.getByRole("button", { name: "Add a target in Settings" }),
    );
    expect(onAddTarget).toHaveBeenCalledTimes(1);
  });

  it("says nothing is missing while the store is still loading", async () => {
    renderSetupPage([main(0, "Plexi")], {
      targetNames: [],
      storeLoading: true,
      onAddTarget: vi.fn(),
    });
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("checkbox", { name: "I've backed up with Pro Control" }),
    );
    expect(
      screen.getByRole("button", { name: "Start leveling" }),
    ).toBeDisabled();
    expect(screen.queryByText("Add a target in Settings")).toBeNull();
  });

  it("never builds the run's choices with an empty or unknown target", () => {
    const options = [main(0, "Plexi"), scene(0, 0, "Plexi", "Lead")];
    const { result, rerender } = renderHook(
      (targetNames: string[]) =>
        useSetupRows({
          options,
          instrumentOptions: [{ value: "none", label: "None" }],
          targetNames,
          savedInstruments: {},
          defaultInst: "none",
        }),
      { initialProps: [] as string[] },
    );
    expect(result.current.noTargets).toBe(true);
    expect(result.current.buildChoices().choices).toEqual([]);
    rerender(["Rhythm", "Lead"]);
    expect(result.current.noTargets).toBe(false);
    const names = result.current
      .buildChoices()
      .choices.map((c) => c.targetName);
    expect(names).toEqual(["Rhythm", "Lead"]);
  });

  it("starts once a target arrives, with that target", async () => {
    const { onStart, rerender } = renderSetupPage([main(0, "Plexi")], {
      targetNames: [],
      onAddTarget: vi.fn(),
    });
    rerender({ targetNames: ["Rhythm"], onAddTarget: vi.fn() });
    expect(screen.queryByText("Add a target in Settings")).toBeNull();
    await startLeveling(userEvent.setup());
    expect(onStart.mock.calls[0][0].map((c) => c.targetName)).toEqual([
      "Rhythm",
    ]);
  });
});

describe("one instrument per preset", () => {
  it('shows "Instrument for all" only with two or more presets', () => {
    const one = renderSetupPage([main(0, "Plexi")]);
    expect(document.querySelector('[data-select="inst-all"]')).toBeNull();
    one.view.unmount();
    renderSetupPage([main(0, "Plexi"), main(1, "Tweed")]);
    expect(selectTrigger("inst-all")).toBeInTheDocument();
  });

  it("starts from the instrument saved for each preset, and saves every choice", async () => {
    const onInstrumentChange = vi.fn();
    const { onStart } = renderSetupPage([main(0, "Plexi"), main(1, "Tweed")], {
      saved: { 0: "tele" },
      defaultInst: "strat",
      onInstrumentChange,
    });
    expect(selectTrigger("inst:0")).toHaveAccessibleName(
      "Instrument for Plexi: Tele",
    );
    expect(selectTrigger("inst:1")).toHaveAccessibleName(
      "Instrument for Tweed: Strat",
    );
    expect(selectTrigger("inst-all")).toHaveAccessibleName(
      "Instrument for all presets: Mixed",
    );
    const user = userEvent.setup();

    await choose(user, "inst:1", "tele");
    expect(onInstrumentChange).toHaveBeenLastCalledWith(1, "tele");
    expect(selectTrigger("inst-all")).toHaveAccessibleName(
      "Instrument for all presets: Tele",
    );

    await choose(user, "inst-all", "none");
    expect(onInstrumentChange).toHaveBeenCalledWith(0, "none");
    expect(onInstrumentChange).toHaveBeenCalledWith(1, "none");

    await startLeveling(user);
    expect(onStart.mock.calls[0][0].map((c) => c.instId)).toEqual([
      "none",
      "none",
    ]);
  });

  // Coming back to the Level tab mounts Set up before the app's store has loaded: the
  // instruments it offers, the ones saved per preset and the default all arrive later.
  it("follows the store when it loads after the page opened", async () => {
    const { onStart, rerender } = renderSetupPage(
      [main(0, "Plexi"), main(1, "Tweed")],
      {
        instrumentOptions: [{ value: "none", label: "None" }],
        saved: {},
        defaultInst: "none",
        storeLoading: true,
      },
    );
    expect(selectTrigger("inst:0")).toHaveAccessibleName(
      "Instrument for Plexi: None",
    );
    rerender({ saved: { 0: "tele" }, defaultInst: "strat" });
    expect(selectTrigger("inst:0")).toHaveAccessibleName(
      "Instrument for Plexi: Tele",
    );
    expect(selectTrigger("inst:1")).toHaveAccessibleName(
      "Instrument for Tweed: Strat",
    );
    await startLeveling(userEvent.setup());
    expect(onStart.mock.calls[0][0].map((c) => c.instId)).toEqual([
      "tele",
      "strat",
    ]);
  });

  it("keeps a pick over the saved one, and drops one the dropdown no longer offers", async () => {
    const { onStart, rerender } = renderSetupPage([main(0, "Plexi")], {
      saved: { 0: "tele" },
    });
    const user = userEvent.setup();
    await choose(user, "inst:0", "strat");
    // A refresh of the store keeps the pick.
    rerender({ saved: { 0: "tele" } });
    expect(selectTrigger("inst:0")).toHaveAccessibleName(
      "Instrument for Plexi: Strat",
    );
    // The picked instrument is deleted in Settings: back to the saved one.
    rerender({
      instrumentOptions: [
        { value: "none", label: "None" },
        { value: "tele", label: "Tele" },
      ],
      saved: { 0: "tele" },
    });
    expect(selectTrigger("inst:0")).toHaveAccessibleName(
      "Instrument for Plexi: Tele",
    );
    await startLeveling(user);
    expect(onStart.mock.calls[0][0].map((c) => c.instId)).toEqual(["tele"]);
  });

  it("every sound of a preset uses the preset's instrument", async () => {
    const { onStart } = renderSetupPage(
      [main(0, "Plexi"), scene(0, 0, "Plexi", "Lead")],
      { defaultInst: "strat" },
    );
    const user = userEvent.setup();
    await choose(user, "inst:0", "tele");
    await startLeveling(user);
    expect(onStart.mock.calls[0][0].map((c) => c.instId)).toEqual([
      "tele",
      "tele",
    ]);
  });
});

describe('"Record yours in Settings"', () => {
  it("shows beside the for-all choice while a preset uses None or an unrecorded instrument", async () => {
    const onCalibrate = vi.fn();
    renderSetupPage([main(0, "Plexi"), main(1, "Tweed")], {
      saved: { 0: "strat", 1: "none" },
      recorded: new Set(["strat"]),
      onCalibrate,
    });
    const link = screen.getByRole("button", {
      name: "Record yours in Settings",
    });
    const user = userEvent.setup();
    await user.click(link);
    expect(onCalibrate).toHaveBeenCalledTimes(1);
    // Once every preset uses a recorded instrument, it goes.
    await choose(user, "inst-all", "strat");
    expect(
      screen.queryByRole("button", { name: "Record yours in Settings" }),
    ).toBeNull();
  });

  it("with one preset, sits after that preset's instrument", () => {
    renderSetupPage([main(0, "Plexi")], { onCalibrate: vi.fn() });
    const header = document.querySelector('[data-setup-preset="0"]');
    if (!(header instanceof HTMLElement)) throw new Error("no header row");
    expect(
      within(header).getByRole("button", { name: "Record yours in Settings" }),
    ).toBeInTheDocument();
  });

  it("never shows the old banner or nudge", () => {
    renderSetupPage([main(0, "Plexi")], { onCalibrate: vi.fn() });
    expect(screen.queryByText(/Level with your own guitar/)).toBeNull();
    expect(screen.queryByText(/Calibrate/)).toBeNull();
  });
});

describe('"Balance its two amps first"', () => {
  it("shows only for a preset the backend can balance, and goes to the run per preset", async () => {
    await seedScan([{ rebalanceable: true }, { rebalanceable: false }]);
    const { onStart } = renderSetupPage([
      scene(0, 0, "Split", "Verse"),
      scene(1, 0, "Single", "Verse"),
    ]);
    const boxes = screen.getAllByRole("checkbox", {
      name: "Balance its two amps first",
    });
    expect(boxes).toHaveLength(1);
    expect(boxes[0]).not.toBeChecked();
    const user = userEvent.setup();
    await user.click(boxes[0]);
    await startLeveling(user);
    expect(onStart.mock.calls[0][1]).toEqual([0]);
  });

  it("stays hidden when the preset has no scene in the run", async () => {
    await seedScan([{ rebalanceable: true }]);
    renderSetupPage([main(0, "Split")]);
    expect(
      screen.queryByRole("checkbox", { name: "Balance its two amps first" }),
    ).toBeNull();
  });

  it("left unticked, balances nothing", async () => {
    await seedScan([{ rebalanceable: true }]);
    const { onStart } = renderSetupPage([scene(0, 0, "Split", "Verse")]);
    await startLeveling(userEvent.setup());
    expect(onStart.mock.calls[0][1]).toEqual([]);
  });
});

describe("a scene with no amp waits for a block", () => {
  async function seedNoAmp() {
    await seedScan([
      {
        scenes: [{ name: "Lead", fs: null }],
        scene_count: 1,
        scene_drives: [drive(0, { skip: "no_active_amp" })],
        scene_handles: [
          {
            sceneSlot: 0,
            candidates: [boostHandle],
            allCandidates: [boostHandle],
          },
        ],
      },
    ]);
  }

  it("shows an empty Block, says why, and points Start at it", async () => {
    await seedNoAmp();
    const { onStart } = renderSetupPage([scene(0, 0, "Clean", "Lead")]);
    const block = selectTrigger("block:s0:0");
    expect(block).toHaveAccessibleName("Block for Clean, Lead: not chosen");
    expect(block).toHaveAttribute("data-attention");
    expect(selectTrigger("knob:s0:0")).toBeDisabled();
    expect(
      screen.getByText(
        "No amp found for this scene. Choose a block and knob for it.",
      ),
    ).toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(
      screen.getByRole("checkbox", { name: "I've backed up with Pro Control" }),
    );
    expect(
      screen.getByRole("button", { name: "Start leveling" }),
    ).toBeDisabled();
    // The link opens the Block list; choosing a block lands on its best knob.
    await user.click(
      screen.getByRole("button", { name: "Choose a block for Lead" }),
    );
    expect(
      screen.getByRole("listbox", { name: "Block for Clean, Lead" }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: BOOST }));
    expect(selectTrigger("knob:s0:0")).toHaveAccessibleName(
      "Knob for Clean, Lead: Gain",
    );
    await user.click(screen.getByRole("button", { name: "Start leveling" }));
    expect(onStart.mock.calls[0][0][0].option.sceneHandle).toEqual({
      groupId: "G1",
      nodeId: "boost",
      parameterId: "gain",
    });
  });
});

describe("rows in run order", () => {
  it("puts a footswitch leveled in a scene after the scenes, one leveled in the main sound before", async () => {
    await seedScan([
      {
        scenes: [
          { name: "Clean", fs: null },
          { name: "Crunch", fs: null },
        ],
        scene_count: 2,
        fs_scene_contexts: [
          { switch: 2, enablingScenes: [1], suggested: 1 },
          { switch: 4, enablingScenes: [], suggested: null },
        ],
      },
    ]);
    const names = ["Clean", "Crunch"];
    // `chosenFrom`'s order: the main sound, then every footswitch, then the scenes.
    renderSetupPage([
      main(0, "Rig"),
      makeFootswitchOption(2, { presetName: "Rig", fsSceneNames: names }),
      makeFootswitchOption(4, { presetName: "Rig", fsSceneNames: names }),
      scene(0, 0, "Rig", "Clean"),
    ]);
    const order = () =>
      [...document.querySelectorAll("[data-setup-row]")].map((el) =>
        el.getAttribute("data-setup-row"),
      );
    await waitFor(() => {
      expect(order()).toEqual(["p0", "f0:sw4", "s0:0", "f0:sw2"]);
    });
  });
});

describe("the scene column", () => {
  it('appears only when a chosen footswitch of a preset with scenes needs "Level it in scene"', () => {
    const one = renderSetupPage([main(0, "Plexi")]);
    expect(screen.queryByText("Level it in scene")).toBeNull();
    one.view.unmount();
    renderSetupPage([
      makeFootswitchOption(2, { presetName: "Rig", fsSceneNames: ["Clean"] }),
    ]);
    expect(screen.getByText("Level it in scene")).toBeInTheDocument();
  });

  it("says when the chosen scene doesn't turn the footswitch's effect on", async () => {
    await seedScan([
      {
        scenes: [
          { name: "Clean", fs: null },
          { name: "Crunch", fs: null },
        ],
        scene_count: 2,
        fs_scene_contexts: [{ switch: 2, enablingScenes: [0], suggested: 0 }],
      },
    ]);
    renderSetupPage([
      makeFootswitchOption(2, {
        presetName: "Rig",
        fsSceneNames: ["Clean", "Crunch"],
      }),
    ]);
    const user = userEvent.setup();
    await choose(user, "scene:f0:sw2", "1");
    expect(
      screen.getByText(
        "Footswitch 3 doesn't turn its effect on in Crunch. Choose another scene for it.",
      ),
    ).toBeInTheDocument();
  });
});

describe("a stored knob the preset no longer has", () => {
  it("shows its own name and says so, without blocking Start", async () => {
    await seedScan([{ base_handles: [] }]);
    const { onStart } = renderSetupPage([
      makeSetupOption({
        presetName: "Plexi",
        baseHandle: { groupId: "G9", nodeId: "fuzz7", parameterId: "level" },
      }),
    ]);
    expect(
      await screen.findByText(
        "This knob isn't in the preset any more. Choose another.",
      ),
    ).toBeInTheDocument();
    expect(selectTrigger("knob:p0")).toHaveAccessibleName(
      "Knob for Plexi, Main sound: Level",
    );
    expect(selectTrigger("block:p0")).toHaveAccessibleName(
      "Block for Plexi, Main sound: fuzz7",
    );
    await startLeveling(userEvent.setup());
    expect(onStart).toHaveBeenCalledTimes(1);
  });
});
