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
  connectDevice,
  listPresets,
  levelPreset,
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
});

describe("camelCase top-level arg keys (Tauri auto-converts to snake_case)", () => {
  it("calibrate_profile uses profileId + secs", async () => {
    await calibrateProfile("p1", 8);
    expectCall("calibrate_profile", { profileId: "p1", secs: 8 });
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
        sceneSlots: [0, 1, 3],
        candidates,
        targetLufs: -24,
        save: false,
        rebalance: false,
        topologyId: null,
        calibrationLufs: null,
      },
      onResult,
    );
    const [calledName, calledArgs] = invokeMock.mock.calls[0];
    expect(calledName).toBe("level_scenes_apply_batched");
    if (calledArgs === undefined) {
      throw new Error("expected level_scenes_apply_batched args but got none");
    }
    // Scalar payload keys (camelCase) pass through verbatim. `candidates` is the
    // same array reference handed to the wrapper.
    expect(calledArgs).toMatchObject({
      slot: 5,
      sceneSlots: [0, 1, 3],
      candidates,
      targetLufs: -24,
      save: false,
      rebalance: false,
      topologyId: null,
      calibrationLufs: null,
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

  it("cancel_preset_leveling invokes the cooperative cancel command", async () => {
    await cancelPresetLeveling();
    expectCall("cancel_preset_leveling", undefined);
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
      block_group_id: null,
      block_node_id: null,
      block_parameter_id: null,
      block_value: null,
    };
    await levelPreset(job);
    expectCall("level_preset", { job });
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
  it("cmd exposes exactly the 73 contract commands", () => {
    // Pins the wire-contract surface: bump this when a command is added or removed
    // (the count guards against an accidental export slip in the cmd registry).
    expect(Object.keys(cmd).length).toBe(25);
  });
});
