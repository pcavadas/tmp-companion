// Contract-layer tests: assert every typed wrapper calls `invoke` with the EXACT
// command name + top-level arg KEYS the backend expects, and (last test) that every
// command in the registry (`lib/commands.ts`) is reached by a wrapper exercised here.
//
// The wire contract is sacred — these tests guard against an accidental rename or
// a camelCase→snake_case slip in the arg keys (Tauri auto-converts camelCase, so
// the keys MUST stay camelCase at the JS boundary).

import { describe, it, expect, vi, beforeEach } from "vitest";
import { error as pluginError } from "@tauri-apps/plugin-log";

// Mock the Tauri core invoke so no real IPC happens. The mock fn is typed with
// the same `(cmd, args?)` shape as Tauri's `invoke`, so spreading into it and
// destructuring `mock.calls[0]` below stay type-safe.
const invokeMock = vi.fn<
  (cmd: string, args?: Record<string, unknown>) => Promise<unknown>
>(() => Promise.resolve(undefined));
// Every command name any test below invoked — never cleared, so the file's LAST test
// can check the wrappers exercised here cover the whole registry.
const invokedNames = new Set<string>();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: [string, Record<string, unknown>?]) => {
    invokedNames.add(args[0]);
    return invokeMock(...args);
  },
  Channel: class MockChannel {
    onmessage: ((message: unknown) => void) | null = null;
  },
}));

// log.ts imports @tauri-apps/plugin-log directly; stub it so the invoke-failure
// logging path (below) is observable without a real WKWebView plugin bridge.
vi.mock("@tauri-apps/plugin-log", () => ({
  error: vi.fn(() => Promise.resolve()),
  warn: vi.fn(() => Promise.resolve()),
}));

import {
  appInfo,
  setAutoInstallUpdates,
  listLevelBlocks,
  getStore,
  saveProfiles,
  saveTargets,
  setPlaybackChannels,
  setPlaybackLevel,
  listPickupTopologies,
  readActivePreset,
  currentGraph,
  readLibraryViaBackup,
  copyApply,
  listSongs,
  createSongFull,
  updateSongFull,
  addSetlistSongs,
  connectDevice,
  listPresets,
  CommandError,
  levelPreset,
  listSceneLevelHandles,
  listFootswitchSceneContexts,
  levelScenesApplyBatched,
  cancelSceneLeveling,
  cancelPresetLeveling,
  levelFootswitchesApply,
  cancelFootswitchLeveling,
  calibrateProfile,
  saveSupportBundle,
  buildSupportBundle,
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
} from "../lib/invoke";
import { DEFAULT_HANDLERS } from "./commandMocks";
import type {
  CopyApplyItem,
  FootswitchLevelProgressItem,
  LevelJob,
  SceneLevelProgressItem,
} from "../lib/types";

const pluginErrorMock = vi.mocked(pluginError);

beforeEach(() => {
  invokeMock.mockClear();
  pluginErrorMock.mockClear();
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

  it("save_support_bundle uses firmware/presetJson/presetName", async () => {
    await saveSupportBundle("1.8.45", '{"graph":1}', "My Preset");
    expectCall("save_support_bundle", {
      firmware: "1.8.45",
      presetJson: '{"graph":1}',
      presetName: "My Preset",
    });
  });

  it("build_support_bundle uses firmware/presetJson/presetName", async () => {
    await buildSupportBundle("1.8.45", '{"graph":1}', "My Preset");
    expectCall("build_support_bundle", {
      firmware: "1.8.45",
      presetJson: '{"graph":1}',
      presetName: "My Preset",
    });
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

  it("list_scene_level_handles uses slot", async () => {
    await listSceneLevelHandles(9);
    expectCall("list_scene_level_handles", { slot: 9 });
  });

  it("list_footswitch_scene_contexts uses slot (D3)", async () => {
    await listFootswitchSceneContexts(9);
    expectCall("list_footswitch_scene_contexts", { slot: 9 });
  });

  it("level_scenes_apply_batched jobs carry an optional handle (D2)", async () => {
    const onResult = vi.fn<(item: SceneLevelProgressItem) => void>(() => {
      /* no-op */
    });
    await levelScenesApplyBatched(
      {
        slot: 5,
        jobs: [
          { sceneSlot: 0, targetLufs: -24 },
          {
            sceneSlot: 1,
            targetLufs: -24,
            handle: { groupId: "G1", nodeId: "ped", parameterId: "volume" },
          },
        ],
        candidates: [],
        save: false,
        rebalance: false,
        topologyId: null,
        calibrationLufs: null,
        profileId: null,
      },
      onResult,
    );
    const [, calledArgs] = invokeMock.mock.calls[0];
    expect(calledArgs).toMatchObject({
      jobs: [
        { sceneSlot: 0, targetLufs: -24 },
        {
          sceneSlot: 1,
          targetLufs: -24,
          handle: { groupId: "G1", nodeId: "ped", parameterId: "volume" },
        },
      ],
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
        sceneOverrides: [],
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
      sceneOverrides: [],
    };
    await doctorApply(job);
    expectCall("doctor_apply", { job });
    invokeMock.mockClear();
    await doctorSave(3, "Synth", 2, ops);
    expectCall("doctor_save", {
      listIndex: 3,
      expectName: "Synth",
      scene: 2,
      ops,
    });
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
  const SEEN = [{ name: "Tete" }, { name: "Tutu" }];
  const SEEN_NAMES = ["Tete", "Tutu"];
  it("remove_song uses slot/expectName/expectList (guarded delete)", async () => {
    await removeSong(2, "Toto", [{ name: "Tata" }, { name: "Toto" }]);
    expectCall("remove_song", {
      slot: 2,
      expectName: "Toto",
      expectList: ["Tata", "Toto"],
    });
  });
  it("add_setlist uses name", async () => {
    await addSetlist("Tutu");
    expectCall("add_setlist", { name: "Tutu" });
  });
  it("rename_setlist uses slot/expectName/expectList/name (guarded rename)", async () => {
    await renameSetlist(4, "Tutu", SEEN, "Tutu Tutu");
    expectCall("rename_setlist", {
      slot: 4,
      expectName: "Tutu",
      expectList: SEEN_NAMES,
      name: "Tutu Tutu",
    });
  });
  it("remove_setlist uses slot/expectName/expectList (guarded delete)", async () => {
    await removeSetlist(4, "Tutu", SEEN);
    expectCall("remove_setlist", {
      slot: 4,
      expectName: "Tutu",
      expectList: SEEN_NAMES,
    });
  });
  it("remove_setlist_song uses setlistSlot/setlistSongSlot (1-based POSITION), guarded", async () => {
    await removeSetlistSong(4, "Tutu", SEEN, 2, 9);
    expectCall("remove_setlist_song", {
      setlistSlot: 4,
      expectName: "Tutu",
      expectList: SEEN_NAMES,
      setlistSongSlot: 2,
      expectSongSlot: 9,
    });
  });
  it("move_setlist_song uses setlistSlot/oldPos/newPos (1-based POSITIONS), guarded", async () => {
    await moveSetlistSong(4, "Tutu", SEEN, 3, 1, 9);
    expectCall("move_setlist_song", {
      setlistSlot: 4,
      expectName: "Tutu",
      expectList: SEEN_NAMES,
      oldPos: 3,
      newPos: 1,
      expectSongSlot: 9,
    });
  });
});

describe("the remaining wrappers", () => {
  it("get_store / list_pickup_topologies / list_songs — no args", async () => {
    await getStore();
    expectCall("get_store");
    invokeMock.mockClear();
    await listPickupTopologies();
    expectCall("list_pickup_topologies");
    invokeMock.mockClear();
    await listSongs();
    expectCall("list_songs");
  });

  it("read_active_preset / current_graph / read_library_via_backup — no args", async () => {
    await readActivePreset();
    expectCall("read_active_preset");
    invokeMock.mockClear();
    await currentGraph();
    expectCall("current_graph");
    invokeMock.mockClear();
    await readLibraryViaBackup();
    expectCall("read_library_via_backup");
  });

  it("list_level_blocks uses slot", async () => {
    await listLevelBlocks(7);
    expectCall("list_level_blocks", { slot: 7 });
  });

  it("save_profiles / save_targets / set_playback_level / set_playback_channels wrap their payload", async () => {
    const profiles = [
      { id: "p1", name: "Strat", topology_id: "sc", calibration_lufs: null },
    ];
    await saveProfiles(profiles);
    expectCall("save_profiles", { profiles });
    invokeMock.mockClear();
    const targets = [{ name: "Rhythm", lufs: -26 }];
    await saveTargets(targets);
    expectCall("save_targets", { targets });
    invokeMock.mockClear();
    await setPlaybackLevel("quiet");
    expectCall("set_playback_level", { level: "quiet" });
    invokeMock.mockClear();
    await setPlaybackChannels("mono_sum");
    expectCall("set_playback_channels", { channels: "mono_sum" });
  });

  it("copy_apply passes jobs + save with a progress channel", async () => {
    const jobs = [
      {
        listIndex: 2,
        name: "Lead",
        ops: [{ kind: "remove" as const, group: "G1", nodeId: "n1" }],
      },
    ];
    const onResult = vi.fn<(item: CopyApplyItem) => void>(() => {
      /* no-op */
    });
    await copyApply(jobs, true, onResult);
    const [calledName, calledArgs] = invokeMock.mock.calls[0];
    expect(calledName).toBe("copy_apply");
    if (calledArgs === undefined) {
      throw new Error("expected copy_apply args but got none");
    }
    expect(Object.keys(calledArgs).sort()).toEqual([
      "jobs",
      "onResult",
      "save",
    ]);
    expect(calledArgs).toMatchObject({ jobs, save: true });
    const channel = calledArgs.onResult;
    if (!(channel instanceof Object) || !("onmessage" in channel)) {
      throw new Error("expected an onResult channel with an onmessage handler");
    }
    expect((channel as { onmessage: unknown }).onmessage).toBe(onResult);
  });

  const SETLISTS = [{ slot: 1, name: "Gig" }];
  const SONGS = [
    { slot: 3, name: "Intro", notes: "", bpm: 120 },
    { slot: 5, name: "Outro", notes: "", bpm: 90 },
  ];

  it("create_song_full sends the setlist guard only with a setlist", async () => {
    await createSongFull("New", "n", 100, SETLISTS[0], SETLISTS);
    expectCall("create_song_full", {
      name: "New",
      notes: "n",
      bpm: 100,
      addToSetlist: 1,
      expectSetlistName: "Gig",
      expectSetlistList: ["Gig"],
    });
    invokeMock.mockClear();
    await createSongFull("Solo", null, null, null, SETLISTS);
    expectCall("create_song_full", {
      name: "Solo",
      notes: null,
      bpm: null,
      addToSetlist: null,
      expectSetlistName: null,
      expectSetlistList: null,
    });
  });

  it("update_song_full uses slot/expectName/expectList + the changed fields", async () => {
    await updateSongFull(3, "Intro", SONGS, "Intro 2", null, 128);
    expectCall("update_song_full", {
      slot: 3,
      expectName: "Intro",
      expectList: ["Intro", "Outro"],
      name: "Intro 2",
      notes: null,
      bpm: 128,
    });
  });

  it("add_setlist_songs guards the setlist and every song by name", async () => {
    await addSetlistSongs(SETLISTS[0], SETLISTS, SONGS, SONGS);
    expectCall("add_setlist_songs", {
      setlistSlot: 1,
      expectName: "Gig",
      expectList: ["Gig"],
      songSlots: [3, 5],
      expectSongNames: ["Intro", "Outro"],
      expectSongList: ["Intro", "Outro"],
    });
  });
});

describe("invoke rejections are normalized to CommandError", () => {
  it("wraps a bare-string backend rejection in an Error carrying its text + command", async () => {
    // Most commands return Result<_, String>, so the real IPC rejects with a
    // STRING — the shape that made `e instanceof Error ? e.message : "generic"`
    // drop the backend's reason.
    invokeMock.mockRejectedValueOnce(
      "guarded insert refused: over the CPU budget",
    );
    const err: unknown = await listPresets().then(
      () => null,
      (e: unknown) => e,
    );
    expect(err).toBeInstanceOf(CommandError);
    expect(err).toBeInstanceOf(Error);
    expect((err as CommandError).message).toBe(
      "guarded insert refused: over the CPU budget",
    );
    expect((err as CommandError).cmd).toBe("list_presets");
    // A bare string carries no kind of its own.
    expect((err as CommandError).kind).toBe("other");
  });

  it("keeps a typed { kind, message } rejection's kind and message (#269)", async () => {
    // `connect_device` returns the typed `error::AppError`, serialized as
    // `{ kind, message }` — the frontend classifies by `kind`, not by the text.
    invokeMock.mockRejectedValueOnce({
      kind: "busy",
      message: "IOHIDDeviceOpen failed: 0xe00002c5 — close Fender Pro Control",
    });
    const err: unknown = await connectDevice().then(
      () => null,
      (e: unknown) => e,
    );
    expect(err).toBeInstanceOf(CommandError);
    expect((err as CommandError).kind).toBe("busy");
    expect((err as CommandError).message).toBe(
      "IOHIDDeviceOpen failed: 0xe00002c5 — close Fender Pro Control",
    );
    expect((err as CommandError).cmd).toBe("connect_device");
  });

  it("reads an unknown kind as other rather than trusting it", async () => {
    invokeMock.mockRejectedValueOnce({ kind: "exploded", message: "boom" });
    const err: unknown = await connectDevice().then(
      () => null,
      (e: unknown) => e,
    );
    expect((err as CommandError).kind).toBe("other");
    expect((err as CommandError).message).toBe("boom");
  });
});

describe("invoke-failure logging", () => {
  it("logs a rejected invoke via the plugin log and still rethrows", async () => {
    const win = window as unknown as Record<string, unknown>;
    win.__TAURI_INTERNALS__ = {};
    invokeMock.mockRejectedValueOnce(new Error("boom"));
    try {
      await expect(listPresets()).rejects.toThrow("boom");
      expect(pluginErrorMock).toHaveBeenCalledWith(
        expect.stringContaining("invoke(list_presets) failed"),
      );
    } finally {
      delete win.__TAURI_INTERNALS__;
    }
  });

  it.each([
    [
      "not_connected",
      "no TMP found (VID 0x1ED8 / PID 0x44) — is it plugged in?",
    ],
    ["not_ready", "IOHIDDeviceSetReport failed: 0xe00002d6"],
  ])(
    "stays quiet on the designed connect-poll rejection (%s)",
    async (kind, message) => {
      // The 3 s auto-connect retry rejects while nothing is plugged in (or the
      // unit is still booting) — logging it would churn the size-capped log file
      // forever. Keyed on the typed kind, not the text.
      const win = window as unknown as Record<string, unknown>;
      win.__TAURI_INTERNALS__ = {};
      invokeMock.mockRejectedValueOnce({ kind, message });
      try {
        await expect(connectDevice()).rejects.toThrow(message);
        expect(pluginErrorMock).not.toHaveBeenCalled();
      } finally {
        delete win.__TAURI_INTERNALS__;
      }
    },
  );

  it("logs a connect failure whose text looks like the gate but whose kind is not", async () => {
    // The old regexes would have swallowed this; only the kind decides now.
    const win = window as unknown as Record<string, unknown>;
    win.__TAURI_INTERNALS__ = {};
    invokeMock.mockRejectedValueOnce(new Error("no TMP found"));
    try {
      await expect(connectDevice()).rejects.toThrow("no TMP found");
      expect(pluginErrorMock).toHaveBeenCalledWith(
        expect.stringContaining("invoke(connect_device) failed"),
      );
    } finally {
      delete win.__TAURI_INTERNALS__;
    }
  });

  it("still logs an actionable connect failure (close Pro Control)", async () => {
    const win = window as unknown as Record<string, unknown>;
    win.__TAURI_INTERNALS__ = {};
    invokeMock.mockRejectedValueOnce({
      kind: "busy",
      message: "IOHIDDeviceOpen failed: close Fender Pro Control",
    });
    try {
      await expect(connectDevice()).rejects.toThrow("IOHIDDeviceOpen");
      expect(pluginErrorMock).toHaveBeenCalledWith(
        expect.stringContaining("invoke(connect_device) failed"),
      );
    } finally {
      delete win.__TAURI_INTERNALS__;
    }
  });
});

// Runs LAST (Vitest runs a file's tests in order): the registry is `Commands`, and
// `DEFAULT_HANDLERS` is typed total over it, so its keys are every command. A
// registry entry no wrapper above reaches — or a wrapper calling a name outside the
// registry — fails here.
describe("registry coverage", () => {
  it("the wrappers exercised above reach exactly the registry's commands", () => {
    expect([...invokedNames].sort()).toEqual(
      Object.keys(DEFAULT_HANDLERS).sort(),
    );
  });
});
