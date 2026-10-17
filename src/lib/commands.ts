// TMP Companion — the typed Tauri command registry.
//
// ONE entry per backend command the frontend calls: `args` is the exact top-level
// invoke payload (camelCase keys — Tauri converts them to the Rust snake_case params;
// nested payload structs keep their own serde casing, modelled in ./types) or
// `undefined` for a no-arg command, and `ret` is what the command resolves with.
//
// Both sides of the IPC contract are checked against it: `invoke.ts`'s wrappers all go
// through `invokeCommand<K>` (a wrong arg key or return type is a `tsc` error there),
// and the test mocks (`src/__tests__/commandMocks.ts`, the global `setup.ts` defaults)
// type every handler's return as `Commands[K]["ret"]`, so a mock can no longer drift
// from the shape the wrapper promises. The registry still mirrors the Rust handlers by
// hand — a Rust-side change needs its entry here updated.

import type { Channel } from "@tauri-apps/api/core";

import type {
  ActiveGraph,
  AppInfo,
  BackupReadResult,
  BaseAnchor,
  BatchLevelRun,
  CalibrateResult,
  ConnectResult,
  CopyApplyItem,
  CopyJob,
  DoctorCheckResult,
  DoctorInputArg,
  DoctorProgressItem,
  FootswitchLevelJobWire,
  FootswitchLevelProgressItem,
  FootswitchLevelResult,
  FsSceneContext,
  LevelBlock,
  LevelBlockCandidate,
  LevelJob,
  LevelResult,
  PlaybackChannels,
  PlaybackLevel,
  PresetEntry,
  Profile,
  SceneHandleRow,
  SceneLevelJobWire,
  SceneLevelProgressItem,
  SetlistRecord,
  SongRecord,
  SongSaveOutcome,
  Store,
  SupportBundleResult,
  Target,
  TopologyInfo,
} from "./types";

/** A Rust `()` return. It arrives as `null`, which no caller reads; it is typed
 * `undefined` so the wrapper can stay a plain `Promise<void>`. */
type Unit = undefined;

/** The shared args of the two streamed leveling APPLY commands. */
interface LevelApplyCommon {
  slot: number;
  save: boolean;
  topologyId: string | null;
  calibrationLufs: number | null;
  profileId: string | null;
}

/** `level_scenes_apply_batched`'s args, minus the progress channel. */
export type SceneLevelApplyArgs = LevelApplyCommon & {
  jobs: SceneLevelJobWire[];
  candidates: LevelBlockCandidate[];
  /** Opt-in: equalize a path-MERGE scene's two lanes before joint-k (no effect on
   * series / single-amp / split-output scenes). */
  rebalance: boolean;
  /** Keeps the wizard's force-appended BASE job alive through the batch's prepass +
   *  headroom-trade phases (see `BaseAnchor`). Omitted/undefined = no base job in the
   *  batch; base still levels via the separate `levelPreset` lane. */
  baseAnchor?: BaseAnchor | null;
};

/** `level_footswitches_apply`'s args, minus the progress channel. */
export type FootswitchLevelApplyArgs = LevelApplyCommon & {
  jobs: FootswitchLevelJobWire[];
};

/** The guard every slot-keyed song/setlist write carries: the name at the slot and the
 * whole list the UI saw (names can repeat, so only the whole list confirms a slot). */
interface SlotGuard {
  expectName: string;
  expectList: string[];
}

/** The support-bundle inputs, all nullable. */
interface SupportBundleArgs {
  firmware: string | null;
  presetJson: string | null;
  presetName: string | null;
}

/** Every backend command the frontend calls → its top-level args and its return. */
export interface Commands {
  // Connection + app
  app_info: { args: undefined; ret: AppInfo };
  connect_device: { args: undefined; ret: ConnectResult };
  list_presets: { args: undefined; ret: PresetEntry[] };

  // Loudness leveling
  list_level_blocks: { args: { slot: number }; ret: LevelBlock[] };
  level_preset: { args: { job: LevelJob }; ret: LevelResult };
  cancel_preset_leveling: { args: undefined; ret: Unit };
  level_scenes_apply_batched: {
    args: SceneLevelApplyArgs & { onResult: Channel<SceneLevelProgressItem> };
    ret: BatchLevelRun<LevelResult>;
  };
  cancel_scene_leveling: { args: undefined; ret: Unit };
  list_scene_level_handles: { args: { slot: number }; ret: SceneHandleRow[] };
  list_footswitch_scene_contexts: {
    args: { slot: number };
    ret: FsSceneContext[];
  };
  level_footswitches_apply: {
    args: FootswitchLevelApplyArgs & {
      onResult: Channel<FootswitchLevelProgressItem>;
    };
    ret: BatchLevelRun<FootswitchLevelResult>;
  };
  cancel_footswitch_leveling: { args: undefined; ret: Unit };

  // Doctor
  doctor_check: {
    args: {
      items: DoctorInputArg[];
      restoreListIndex: number | null;
      onResult: Channel<DoctorProgressItem>;
    };
    ret: DoctorCheckResult;
  };
  cancel_doctor_check: { args: undefined; ret: Unit };

  // Instrument profiles + persisted store
  get_store: { args: undefined; ret: Store };
  save_profiles: { args: { profiles: Profile[] }; ret: Unit };
  save_targets: { args: { targets: Target[] }; ret: Unit };
  set_playback_level: { args: { level: PlaybackLevel }; ret: Unit };
  set_playback_channels: { args: { channels: PlaybackChannels }; ret: Unit };
  set_auto_install_updates: { args: { on: boolean }; ret: Unit };
  set_preset_instrument: {
    args: { slot: number; profileId: string };
    ret: Unit;
  };
  calibrate_profile: {
    args: { profileId: string; secs: number };
    ret: CalibrateResult;
  };
  list_pickup_topologies: { args: undefined; ret: TopologyInfo[] };
  save_support_bundle: { args: SupportBundleArgs; ret: SupportBundleResult };
  build_support_bundle: { args: SupportBundleArgs; ret: ArrayBuffer };

  // Active preset + library
  read_active_preset: { args: undefined; ret: ActiveGraph };
  current_graph: { args: undefined; ret: ActiveGraph | null };
  read_library_via_backup: { args: undefined; ret: BackupReadResult };
  copy_apply: {
    args: {
      jobs: CopyJob[];
      save: boolean;
      onResult: Channel<CopyApplyItem>;
    };
    ret: CopyApplyItem[];
  };

  // Songs & setlists
  list_songs: { args: undefined; ret: SongRecord[] };
  read_setlists: { args: undefined; ret: SetlistRecord[] };
  list_setlist_songs: { args: { setlistSlot: number }; ret: number[] };
  remove_song: { args: SlotGuard & { slot: number }; ret: SongRecord[] };
  add_setlist: { args: { name: string }; ret: SetlistRecord[] };
  rename_setlist: {
    args: SlotGuard & { slot: number; name: string };
    ret: SetlistRecord[];
  };
  remove_setlist: { args: SlotGuard & { slot: number }; ret: SetlistRecord[] };
  remove_setlist_song: {
    args: SlotGuard & {
      setlistSlot: number;
      setlistSongSlot: number;
      expectSongSlot: number;
    };
    ret: number[];
  };
  move_setlist_song: {
    args: SlotGuard & {
      setlistSlot: number;
      oldPos: number;
      newPos: number;
      expectSongSlot: number;
    };
    ret: number[];
  };
  create_song_full: {
    args: {
      name: string;
      notes: string | null;
      bpm: number | null;
      addToSetlist: number | null;
      expectSetlistName: string | null;
      expectSetlistList: string[] | null;
    };
    ret: SongSaveOutcome;
  };
  update_song_full: {
    args: SlotGuard & {
      slot: number;
      name: string | null;
      notes: string | null;
      bpm: number | null;
    };
    ret: SongSaveOutcome;
  };
  add_setlist_songs: {
    args: SlotGuard & {
      setlistSlot: number;
      songSlots: number[];
      expectSongNames: string[];
      expectSongList: string[];
    };
    ret: number[];
  };
}

/** A backend command name. */
export type CommandName = keyof Commands;
/** A command's top-level invoke args (`undefined` for a no-arg command), flattened to
 * an anonymous object type: an `interface` member of an entry has no index signature,
 * so it would not satisfy Tauri's `InvokeArgs` (`Record<string, unknown>`). */
export type CommandArgs<K extends CommandName> = Flat<Commands[K]["args"]>;
type Flat<T> = T extends undefined ? undefined : { [P in keyof T]: T[P] };
/** What a command resolves with. */
export type CommandRet<K extends CommandName> = Commands[K]["ret"];
/** The rest-parameter tuple a typed invoke takes for `K`: nothing for a no-arg command,
 * else exactly its args object. */
export type CommandArgList<K extends CommandName> = [CommandArgs<K>] extends [
  undefined,
]
  ? []
  : [CommandArgs<K>];
