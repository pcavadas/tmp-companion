// Contract-layer tests: assert a representative sample of typed wrappers call
// `invoke` with the EXACT command name + top-level arg KEYS the backend expects.
//
// The wire contract is sacred — these tests guard against an accidental rename or
// a camelCase→snake_case slip in the arg keys (Tauri auto-converts camelCase, so
// the keys MUST stay camelCase at the JS boundary).

import { describe, it, expect, vi, beforeEach } from "vitest";

// Mock the Tauri core invoke so no real IPC happens. The mock fn is typed with
// the same `(cmd, args?)` shape as Tauri's `invoke`, so spreading into it and
// destructuring `mock.calls[0]` below stay type-safe.
const invokeMock = vi.fn<
  (cmd: string, args?: Record<string, unknown>) => Promise<unknown>
>(() => Promise.resolve(undefined));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: [string, Record<string, unknown>?]) => invokeMock(...args),
  Channel: class MockChannel {
    onmessage: ((message: unknown) => void) | null = null;
  },
}));

import {
  appInfo,
  setAutoInstallUpdates,
  connectDevice,
  listPresets,
  levelPreset,
  restorePresetLevel,
  redistributeHeadroom,
  restoreRedistribution,
  levelScenesApplyBatched,
  cancelSceneLeveling,
  cancelPresetLeveling,
  levelFootswitchesApply,
  cancelFootswitchLeveling,
  calibrateProfile,
  readSetlists,
  listSetlistSongs,
  removeSong,
  addSetlist,
  renameSetlist,
  removeSetlist,
  removeSetlistSong,
  moveSetlistSong,
  doctorCheck,
  cancelDoctorCheck,
  doctorApply,
  doctorSave,
  doctorDiscard,
  cmd,
} from "../lib/invoke";
import type {
  SceneLevelProgressItem,
  FootswitchLevelProgressItem,
} from "../lib/invoke";
import type { LevelJob } from "../lib/types";

beforeEach(() => {
  invokeMock.mockClear();
});

/** Assert the most recent invoke call used `name` and exactly the given arg keys
 * (and that any provided values matched). */
function expectCall(name: string, args?: Record<string, unknown>) {
  expect(invokeMock).toHaveBeenCalledTimes(1);
  const [calledName, calledArgs] = invokeMock.mock.calls[0];
  expect(calledName).toBe(name);
  if (args === undefined) {
    expect(calledArgs).toBeUndefined();
  } else {
    expect(calledArgs).toEqual(args);
    // Guard the exact key SET (no extra / missing keys). Narrow `calledArgs` to a
    // real object so `Object.keys` is safe (the `toEqual` above already proved it
    // matches `args`; this throws loudly if the contract somehow regressed).
    if (calledArgs === undefined) {
      throw new Error(`expected invoke args for "${name}" but got none`);
    }
    expect(Object.keys(calledArgs).sort()).toEqual(Object.keys(args).sort());
  }
}

describe("no-arg commands", () => {
  it("connect_device — no args", async () => {
    await connectDevice();
    expectCall("connect_device");
  });

  it("connect_device — resolves with the monitor startup snapshot", async () => {
    const snapshot = { firmware: "1.7.75", graph: null };
    invokeMock.mockResolvedValueOnce(snapshot);
    await expect(connectDevice()).resolves.toBe(snapshot);
  });

  it("list_presets — no args", async () => {
    await listPresets();
    expectCall("list_presets");
  });

  it("app_info — no args", async () => {
    await appInfo();
    expectCall("app_info");
  });
});

describe("camelCase top-level arg keys (Tauri auto-converts to snake_case)", () => {
  it("calibrate_profile uses profileId + secs", async () => {
    await calibrateProfile("p1", 8);
    expectCall("calibrate_profile", { profileId: "p1", secs: 8 });
  });

  it("set_auto_install_updates uses on", async () => {
    await setAutoInstallUpdates(true);
    expectCall("set_auto_install_updates", { on: true });
  });

  it("level_scenes_apply_batched passes scene jobs with a progress channel", async () => {
    const candidates = [
      { groupId: "G1", nodeId: "ampA", parameterId: "outputLevel", value: 0.5 },
    ];
    const onResult = vi.fn<(item: SceneLevelProgressItem) => void>(() => {
      /* no-op */
    });
    await levelScenesApplyBatched(
      {
        slot: 5,
        jobs: [
          { sceneSlot: 0, targetLufs: -24 },
          { sceneSlot: 1, targetLufs: -24 },
          { sceneSlot: 3, targetLufs: -22 },
        ],
        candidates,
        save: false,
        rebalance: false,
        topologyId: null,
        calibrationLufs: null,
        profileId: null,
      },
      onResult,
    );
    const [calledName, calledArgs] = invokeMock.mock.calls[0];
    expect(calledName).toBe("level_scenes_apply_batched");
    if (calledArgs === undefined) {
      throw new Error("expected level_scenes_apply_batched args but got none");
    }
    // Scalar payload keys (camelCase) pass through verbatim. `candidates` is the
    // same array reference handed to the wrapper; each job carries its OWN
    // per-scene target (camelCase nested keys, like levelFootswitchesApply).
    expect(calledArgs).toMatchObject({
      slot: 5,
      jobs: [
        { sceneSlot: 0, targetLufs: -24 },
        { sceneSlot: 1, targetLufs: -24 },
        { sceneSlot: 3, targetLufs: -22 },
      ],
      candidates,
      save: false,
      rebalance: false,
      topologyId: null,
      calibrationLufs: null,
      profileId: null,
    });
    // The progress channel is wrapped under `onResult`; its `onmessage` must be
    // the callback the wrapper was given. Narrow the channel before reading it.
    const channel = calledArgs.onResult;
    if (!(channel instanceof Object) || !("onmessage" in channel)) {
      throw new Error("expected an onResult channel with an onmessage handler");
    }
    expect((channel as { onmessage: unknown }).onmessage).toBe(onResult);
  });

  it("cancel_scene_leveling invokes the cooperative cancel command", async () => {
    await cancelSceneLeveling();
    expectCall("cancel_scene_leveling", undefined);
  });

  it("redistribute_headroom passes jobs + deficit + a progress channel", async () => {
    const onResult = vi.fn(() => {
      /* no-op */
    });
    await redistributeHeadroom(
      {
        slot: 7,
        jobs: [
          { sceneSlot: 8, targetLufs: -24 },
          { sceneSlot: 0, targetLufs: -24 },
        ],
        candidates: [
          {
            groupId: "G1",
            nodeId: "amp",
            parameterId: "outputLevel",
            value: 1,
          },
        ],
        worstClampedDeficitDb: 3,
        topologyId: null,
        calibrationLufs: null,
        profileId: null,
      },
      onResult,
    );
    const [name, args] = invokeMock.mock.calls[0];
    expect(name).toBe("redistribute_headroom");
    expect(args).toMatchObject({ slot: 7, worstClampedDeficitDb: 3 });
  });

  it("restore_redistribution writes the recorded values back", async () => {
    await restoreRedistribution(
      7,
      0.32,
      [{ groupId: "G1", nodeId: "amp", sceneSlot: null, value: 0.5 }],
      "My Preset",
    );
    expectCall("restore_redistribution", {
      slot: 7,
      presetLevel: 0.32,
      knobs: [{ groupId: "G1", nodeId: "amp", sceneSlot: null, value: 0.5 }],
      expectedName: "My Preset",
    });
  });

  it("cancel_preset_leveling invokes the cooperative cancel command", async () => {
    await cancelPresetLeveling();
    expectCall("cancel_preset_leveling", undefined);
  });

  it("doctor_check passes the sound list + restore slot with a progress channel", async () => {
    const items = [
      {
        key: "p3",
        listIndex: 3,
        scene: null,
        footswitch: null,
        label: "Synth",
        tag: null,
        topologyId: "guitar-humbucker",
        calibrationLufs: null,
        profileId: null,
        nodes: [],
        footswitches: [],
      },
    ];
    const onResult = vi.fn(() => {
      /* no-op */
    });
    await doctorCheck(items, 5, onResult);
    const [calledName, calledArgs] = invokeMock.mock.calls[0];
    expect(calledName).toBe("doctor_check");
    if (calledArgs === undefined) {
      throw new Error("expected doctor_check args but got none");
    }
    expect(calledArgs).toMatchObject({ items, restoreListIndex: 5 });
    const channel = calledArgs.onResult;
    if (!(channel instanceof Object) || !("onmessage" in channel)) {
      throw new Error("expected an onResult channel with an onmessage handler");
    }
    expect((channel as { onmessage: unknown }).onmessage).toBe(onResult);
  });

  it("cancel_doctor_check invokes the cooperative cancel command", async () => {
    await cancelDoctorCheck();
    expectCall("cancel_doctor_check", undefined);
  });

  it("doctor_apply wraps the job; save + discard are identity-addressed", async () => {
    const ops = [
      {
        kind: "param" as const,
        groupId: "G1",
        nodeId: "ACD_CabSimTMS",
        param: "lpf",
        value: 8000,
      },
    ];
    const job = {
      listIndex: 3,
      name: "Synth",
      ops,
      topologyId: null,
      calibrationLufs: null,
      profileId: null,
      scene: null,
      footswitch: null,
      nodes: [],
      footswitches: [],
    };
    await doctorApply(job);
    expectCall("doctor_apply", { job });
    invokeMock.mockClear();
    await doctorSave(3, "Synth", ops);
    expectCall("doctor_save", { listIndex: 3, expectName: "Synth", ops });
    invokeMock.mockClear();
    await doctorDiscard(3);
    expectCall("doctor_discard", { listIndex: 3 });
  });

  it("level_footswitches_apply passes footswitch jobs with a progress channel", async () => {
    const jobs = [
      {
        switch: 6,
        levGroupId: "G1",
        levNodeId: "ACD_BluesDriver",
        levParameterId: "gain",
        targetLufs: -23,
      },
    ];
    const onResult = vi.fn<(item: FootswitchLevelProgressItem) => void>(() => {
      /* no-op */
    });
    await levelFootswitchesApply(
      {
        slot: 23,
        jobs,
        save: false,
        topologyId: null,
        calibrationLufs: null,
        profileId: null,
      },
      onResult,
    );
    const [calledName, calledArgs] = invokeMock.mock.calls[0];
    expect(calledName).toBe("level_footswitches_apply");
    if (calledArgs === undefined) {
      throw new Error("expected level_footswitches_apply args but got none");
    }
    expect(calledArgs).toMatchObject({
      slot: 23,
      jobs,
      save: false,
      topologyId: null,
      calibrationLufs: null,
      profileId: null,
    });
    const channel = calledArgs.onResult;
    if (!(channel instanceof Object) || !("onmessage" in channel)) {
      throw new Error("expected an onResult channel with an onmessage handler");
    }
    expect((channel as { onmessage: unknown }).onmessage).toBe(onResult);
  });

  it("cancel_footswitch_leveling invokes the cooperative cancel command", async () => {
    await cancelFootswitchLeveling();
    expectCall("cancel_footswitch_leveling", undefined);
  });
});

describe("single-struct / nested-payload args (snake_case inside the payload)", () => {
  it("level_preset wraps the whole job under { job }", async () => {
    const job: LevelJob = {
      slot: 11,
      target_lufs: -30,
      save: false,
      topology_id: null,
      calibration_lufs: null,
      profile_id: null,
      block_group_id: null,
      block_node_id: null,
      block_parameter_id: null,
      block_value: null,
    };
    await levelPreset(job);
    expectCall("level_preset", { job });
  });
  it("restore_preset_level uses slot/level/expectedName (the Summary revert write)", async () => {
    await restorePresetLevel(3, 0.62, "Guitar");
    expectCall("restore_preset_level", {
      slot: 3,
      level: 0.62,
      expectedName: "Guitar",
    });
  });
});

describe("device-backed song/setlist CRUD (Songs page)", () => {
  it("read_setlists — no args", async () => {
    await readSetlists();
    expectCall("read_setlists");
  });
  it("list_setlist_songs uses setlistSlot", async () => {
    await listSetlistSongs(4);
    expectCall("list_setlist_songs", { setlistSlot: 4 });
  });
  it("remove_song uses slot/expectName (guarded delete)", async () => {
    await removeSong(2, "Toto");
    expectCall("remove_song", { slot: 2, expectName: "Toto" });
  });
  it("add_setlist uses name", async () => {
    await addSetlist("Tutu");
    expectCall("add_setlist", { name: "Tutu" });
  });
  it("rename_setlist uses slot/name", async () => {
    await renameSetlist(4, "Tutu Tutu");
    expectCall("rename_setlist", { slot: 4, name: "Tutu Tutu" });
  });
  it("remove_setlist uses slot/expectName (guarded delete)", async () => {
    await removeSetlist(4, "Tutu");
    expectCall("remove_setlist", { slot: 4, expectName: "Tutu" });
  });
  it("remove_setlist_song uses setlistSlot/setlistSongSlot (1-based POSITION)", async () => {
    await removeSetlistSong(4, 2);
    expectCall("remove_setlist_song", { setlistSlot: 4, setlistSongSlot: 2 });
  });
  it("move_setlist_song uses setlistSlot/oldPos/newPos (1-based POSITIONS)", async () => {
    await moveSetlistSong(4, 3, 1);
    expectCall("move_setlist_song", { setlistSlot: 4, oldPos: 3, newPos: 1 });
  });
});

describe("cmd namespace mirrors the named exports", () => {
  it("cmd exposes exactly the 34 contract commands", () => {
    // Pins the wire-contract surface: bump this when a command is added or removed
    // (the count guards against an accidental export slip in the cmd registry).
    expect(Object.keys(cmd).length).toBe(34);
  });
});
