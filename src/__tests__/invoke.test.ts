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
  connectDevice,
  listPresets,
  levelPreset,
  levelSetlist,
  levelScenes,
  levelScenesApply,
  levelScenesApplyBatched,
  cancelSceneLeveling,
  cancelPresetLeveling,
  levelFootswitchesApply,
  cancelFootswitchLeveling,
  calibrateProfile,
  importLibrary,
  libraryFilter,
  bulkDryRun,
  bulkApply,
  bulkRevert,
  createVariant,
  saveBlockTemplate,
  spectrumScan,
  eqMatch,
  rankCandidates,
  auditionRender,
  migrationScan,
  migrationPlan,
  migrationApply,
  auditLoudness,
  songAssign,
  songClear,
  songMove,
  songSwap,
  loadSceneOnAmp,
  requestSceneList,
  readPresetScenes,
  stopLiveSync,
  readSetlists,
  listSetlistSongs,
  addSong,
  renameSong,
  removeSong,
  setSongNotes,
  setSongBpm,
  addSetlist,
  renameSetlist,
  removeSetlist,
  addSetlistSong,
  removeSetlistSong,
  moveSetlistSong,
  cmd,
} from "../lib/invoke";
import type {
  SceneLevelProgressItem,
  FootswitchLevelProgressItem,
} from "../lib/invoke";
import type { LevelJob, OpSpec, RecipeArg } from "../lib/types";

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
  it("app_info — no args", async () => {
    await appInfo();
    expectCall("app_info");
  });

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

  it("eq_match uses sourceSlot/referenceSlot/topologyId", async () => {
    await eqMatch(1, 2, null);
    expectCall("eq_match", {
      sourceSlot: 1,
      referenceSlot: 2,
      topologyId: null,
    });
  });

  it("rank_candidates uses targetSlot/candidateSlots/topologyId", async () => {
    await rankCandidates(0, [1, 2], null);
    expectCall("rank_candidates", {
      targetSlot: 0,
      candidateSlots: [1, 2],
      topologyId: null,
    });
  });

  it("spectrum_scan uses slot/topologyId", async () => {
    await spectrumScan(3, null);
    expectCall("spectrum_scan", { slot: 3, topologyId: null });
  });

  it("audition_render uses slot/topologyId", async () => {
    await auditionRender(3, "humbucker");
    expectCall("audition_render", { slot: 3, topologyId: "humbucker" });
  });

  it("audit_loudness uses slots/topologyId/outlierLu", async () => {
    await auditLoudness([0, 1], null, 2.0);
    expectCall("audit_loudness", {
      slots: [0, 1],
      topologyId: null,
      outlierLu: 2.0,
    });
  });

  it("level_scenes uses sceneCount/topologyId/headroomLu", async () => {
    await levelScenes(5, 3, null, 1.5);
    expectCall("level_scenes", {
      slot: 5,
      sceneCount: 3,
      topologyId: null,
      headroomLu: 1.5,
    });
  });

  it("level_scenes_apply passes the per-scene leveling job (camelCase keys, amp candidates)", async () => {
    const candidates = [
      { groupId: "G1", nodeId: "ampA", parameterId: "outputLevel", value: 0.5 },
      { groupId: "G1", nodeId: "ampB", parameterId: "outputLevel", value: 0.7 },
    ];
    await levelScenesApply({
      slot: 5,
      sceneSlots: [0, 1, 3],
      candidates,
      targetLufs: -24,
      save: false,
      topologyId: null,
      calibrationLufs: null,
    });
    expectCall("level_scenes_apply", {
      slot: 5,
      sceneSlots: [0, 1, 3],
      candidates,
      targetLufs: -24,
      save: false,
      topologyId: null,
      calibrationLufs: null,
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

  it("import_library uses folder", async () => {
    await importLibrary("/presets");
    expectCall("import_library", { folder: "/presets" });
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

  it("level_setlist uses entries + save", async () => {
    const entries = [{ slot: 1, topology_id: null, calibration_lufs: null }];
    await levelSetlist(entries, true);
    expectCall("level_setlist", { entries, save: true });
  });

  it("library_filter wraps args under { filter }", async () => {
    const filter = { amp: "Twin", level_lt: 0.5 };
    await libraryFilter(filter);
    expectCall("library_filter", { filter });
  });
});

describe("bulk run engine (OpSpec)", () => {
  const op: OpSpec = {
    type: "ParamEdit",
    model: "ACD_amp",
    param: "gain",
    mode: { mode: "set", value: 0.5 },
    min: 0,
    max: 1,
  };

  it("bulk_dry_run uses selection + op", async () => {
    await bulkDryRun([0, 1], op);
    expectCall("bulk_dry_run", { selection: [0, 1], op });
  });

  it("bulk_apply uses selection + op + backup", async () => {
    await bulkApply([0], op, true);
    expectCall("bulk_apply", { selection: [0], op, backup: true });
  });

  it("bulk_revert uses runId (camelCase)", async () => {
    await bulkRevert("run-123");
    expectCall("bulk_revert", { runId: "run-123" });
  });
});

describe("rename + variants — sourceListIndex/recipe/spec", () => {
  it("create_variant uses sourceListIndex/recipe", async () => {
    const recipe: RecipeArg = { name_suffix: " alt", edits: [] };
    await createVariant(3, recipe);
    expectCall("create_variant", { sourceListIndex: 3, recipe });
  });
});

describe("block templates", () => {
  it("save_block_template uses sourceListIndex/model/name", async () => {
    await saveBlockTemplate(2, "ACD_reverb", "My Verb");
    expectCall("save_block_template", {
      sourceListIndex: 2,
      model: "ACD_reverb",
      name: "My Verb",
    });
  });
});

describe("migration", () => {
  it("migration_scan uses targetCatalog", async () => {
    await migrationScan(["ACD_a", "ACD_b"]);
    expectCall("migration_scan", { targetCatalog: ["ACD_a", "ACD_b"] });
  });

  it("migration_plan uses targetCatalog/renameMap", async () => {
    await migrationPlan(["ACD_a"], { old: "new" });
    expectCall("migration_plan", {
      targetCatalog: ["ACD_a"],
      renameMap: { old: "new" },
    });
  });

  it("migration_apply uses targetCatalog/renameMap/dryRun", async () => {
    await migrationApply(["ACD_a"], { old: "new" }, false);
    expectCall("migration_apply", {
      targetCatalog: ["ACD_a"],
      renameMap: { old: "new" },
      dryRun: false,
    });
  });
});

describe("songs & setlists — device CRUD", () => {
  it("song_assign sends all six camelCase keys", async () => {
    await songAssign(1, 0, 4, "FS1", 2, 1);
    expectCall("song_assign", {
      songSlot: 1,
      songPresetSlot: 0,
      userListIndex: 4,
      footswitchLabel: "FS1",
      footswitchColor: 2,
      presetSceneSlot: 1,
    });
  });

  it("song_clear uses songSlot/songPresetSlot", async () => {
    await songClear(1, 0);
    expectCall("song_clear", { songSlot: 1, songPresetSlot: 0 });
  });

  it("song_move maps newPos → the reserved Rust `new` key", async () => {
    await songMove(1, 0, 2);
    expectCall("song_move", { songSlot: 1, old: 0, new: 2 });
  });

  it("song_swap uses songSlot/a/b", async () => {
    await songSwap(1, 0, 2);
    expectCall("song_swap", { songSlot: 1, a: 0, b: 2 });
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
  it("add_song uses name", async () => {
    await addSong("Toto");
    expectCall("add_song", { name: "Toto" });
  });
  it("rename_song uses slot/name", async () => {
    await renameSong(2, "Toto Toto");
    expectCall("rename_song", { slot: 2, name: "Toto Toto" });
  });
  it("remove_song uses slot/expectName (guarded delete)", async () => {
    await removeSong(2, "Toto");
    expectCall("remove_song", { slot: 2, expectName: "Toto" });
  });
  it("set_song_notes uses slot/notes", async () => {
    await setSongNotes(2, "titi tutu");
    expectCall("set_song_notes", { slot: 2, notes: "titi tutu" });
  });
  it("set_song_bpm uses slot/bpm", async () => {
    await setSongBpm(2, 102);
    expectCall("set_song_bpm", { slot: 2, bpm: 102 });
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
  it("add_setlist_song uses setlistSlot/songSlot (GLOBAL song slot)", async () => {
    await addSetlistSong(4, 23);
    expectCall("add_setlist_song", { setlistSlot: 4, songSlot: 23 });
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

describe("scenes — nested under presets", () => {
  it("load_scene_on_amp uses listIndex/sceneSlot (load-first when listIndex set)", async () => {
    await loadSceneOnAmp(7, 2);
    expectCall("load_scene_on_amp", { listIndex: 7, sceneSlot: 2 });
  });

  it("load_scene_on_amp passes null listIndex to recall on the active preset", async () => {
    await loadSceneOnAmp(null, 3);
    expectCall("load_scene_on_amp", { listIndex: null, sceneSlot: 3 });
  });

  it("request_scene_list — no args (manual scene-list top-up; push is canonical)", async () => {
    await requestSceneList();
    expectCall("request_scene_list");
  });

  it("read_preset_scenes uses listIndex", async () => {
    await readPresetScenes(7);
    expectCall("read_preset_scenes", { listIndex: 7 });
  });

  it("stop_live_sync — no args, resolves with the reclaimed firmware version", async () => {
    invokeMock.mockResolvedValueOnce("1.7.75");
    await expect(stopLiveSync()).resolves.toBe("1.7.75");
    expectCall("stop_live_sync");
  });
});

describe("cmd namespace mirrors the named exports", () => {
  it("cmd.appInfo invokes app_info", async () => {
    await cmd.appInfo();
    expectCall("app_info");
  });

  it("cmd exposes exactly the 73 contract commands", () => {
    // Pins the wire-contract surface: bump this when a command is added or removed
    // (the count guards against an accidental export slip in the cmd registry).
    expect(Object.keys(cmd).length).toBe(73);
  });
});
