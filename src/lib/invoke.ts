// TMP Companion — typed Tauri command wrappers.
//
// Typed wrappers, one per backend command; arg keys are camelCase, Tauri converts
// to snake_case.
//
// CASING (load-bearing): top-level arg keys are camelCase — Tauri 2 auto-converts
// them to the Rust snake_case params. Keys nested inside a JSON payload object
// (job/op/spec/recipe/filter/edit/store) are snake_case and modelled by the types
// in ./types. Do NOT "fix" either casing.

import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  AppInfo,
  PresetEntry,
  LevelJob,
  LevelResult,
  FootswitchLevelResult,
  LevelBlock,
  Profile,
  CalibrateResult,
  Target,
  Store,
  PlaybackLevel,
  TopologyInfo,
  ActiveGraph,
  ConnectResult,
  SongRecord,
  SetlistRecord,
  SongSaveOutcome,
  PresetScenes,
  BackupReadResult,
  CopyJob,
  CopyApplyItem,
  DoctorInputArg,
  DoctorProgressItem,
  DoctorCheckResult,
  DoctorApplyJob,
  DoctorApplyResult,
} from "./types";

/**
 * True when running inside Tauri's WKWebView (the global injected by the
 * runtime). Screens may use this to render a "not in app" notice during a plain
 * `vite` browser session; the wrappers themselves always call `invoke`
 * (`invoke` rejects gracefully off-host).
 */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

// ─── Connection + app ─────────────────────────────────────────────────────────

/** App identity — name + the running build's version (Tauri config). */
export const appInfo = (): Promise<AppInfo> => invoke("app_info");

/** Seize the TMP + combined handshake (firmware + discovery in one
 * burst, mirroring Pro Control). Returns both firmware version and the active
 * preset's signal graph, so the preset list and signal chain can render
 * simultaneously with no second handshake. */
export const connectDevice = (): Promise<ConnectResult> =>
  invoke("connect_device");

/** Device "My Presets"; each `{ slot, name }` (slot = 0-based list index). */
export const listPresets = (): Promise<PresetEntry[]> => invoke("list_presets");

// ─── Loudness leveling ──────────────────────────────────────────────────────

/** Enumerate a preset's level-type block controls (load → discovery). */
export const listLevelBlocks = (slot: number): Promise<LevelBlock[]> =>
  invoke("list_level_blocks", { slot });

/** One-shot open-loop on `presetLevel` (or closed-loop on a block knob
 * when all three `block_*` coords are set). DEVICE WRITE when `job.save` is true. */
export const levelPreset = (job: LevelJob): Promise<LevelResult> =>
  invoke("level_preset", { job });

/** Cooperatively cancel an in-flight {@link levelPreset} run — sets a backend flag the
 *  leveller checks at its seams, so the current item bails before its apply+save. Like the
 *  scene/footswitch cancels it's a named export (deliberately not in `cmd`). */
export const cancelPresetLeveling = (): Promise<void> =>
  invoke("cancel_preset_leveling");

/** Restore a preset's `presetLevel` to its pre-leveling value (Summary "Restore
 * original"). DEVICE WRITE (set + save). `presetLevel` only — scene/footswitch
 * `outputLevel` writes are not reverted. `expectedName` is the display name the
 * run recorded for the slot — the backend re-reads the preset list and refuses
 * the write if the slot no longer holds that preset (slot-drift guard). */
export const restorePresetLevel = (
  slot: number,
  level: number,
  expectedName: string,
): Promise<void> =>
  invoke("restore_preset_level", { slot, level, expectedName });

/** A candidate leveling knob for `levelScenesApply` — pass EVERY amp-level candidate;
 * the backend picks per scene the one whose block is ON in that scene (scenes can
 * swap which amp is live, and a bypassed amp's knob measures flat). */
export interface LevelBlockCandidate {
  groupId: string;
  nodeId: string;
  parameterId: string;
  value: number;
}

export interface SceneLevelProgressItem {
  sceneSlot: number;
  status: "active" | "done" | "error" | "cancelled";
  result: LevelResult | null;
  message: string | null;
}

/** Batched APPLY path. One backend command levels all selected scenes
 * and streams row progress over a Tauri channel. Each `job` carries its OWN
 * per-scene target (camelCase nested keys, like `levelFootswitchesApply`), so a
 * mixed-target preset still levels in ONE batch. */
export const levelScenesApplyBatched = (
  args: {
    slot: number;
    jobs: { sceneSlot: number; targetLufs: number }[];
    candidates: LevelBlockCandidate[];
    save: boolean;
    /** Opt-in: equalize a path-MERGE scene's two lanes before joint-k (no effect on
     * series / single-amp / split-output scenes). */
    rebalance: boolean;
    topologyId: string | null;
    calibrationLufs: number | null;
    profileId: string | null;
  },
  onResult: (item: SceneLevelProgressItem) => void,
): Promise<LevelResult[]> => {
  const channel = new Channel<SceneLevelProgressItem>();
  channel.onmessage = onResult;
  return invoke("level_scenes_apply_batched", { ...args, onResult: channel });
};

/** Cooperatively stop an in-flight batched scene-leveling run. */
export const cancelSceneLeveling = (): Promise<void> =>
  invoke("cancel_scene_leveling");

/** One streamed footswitch-leveling row (`lib::FootswitchLevelProgressItem`). */
export interface FootswitchLevelProgressItem {
  switch: number;
  status: "active" | "done" | "error" | "cancelled";
  result: FootswitchLevelResult | null;
  message: string | null;
}

/** Level one or more block-acting footswitches' engaged states for preset `slot`,
 * streaming a progress row per switch. Each `job` picks a switch + the block param to
 * solve. Mirrors `levelScenesApplyBatched`. */
export const levelFootswitchesApply = (
  args: {
    slot: number;
    jobs: {
      switch: number;
      levGroupId: string;
      levNodeId: string;
      levParameterId: string;
      targetLufs: number;
    }[];
    save: boolean;
    topologyId: string | null;
    calibrationLufs: number | null;
    profileId: string | null;
  },
  onResult: (item: FootswitchLevelProgressItem) => void,
): Promise<FootswitchLevelResult[]> => {
  const channel = new Channel<FootswitchLevelProgressItem>();
  channel.onmessage = onResult;
  return invoke("level_footswitches_apply", { ...args, onResult: channel });
};

/** Cooperatively stop an in-flight footswitch-leveling run. */
export const cancelFootswitchLeveling = (): Promise<void> =>
  invoke("cancel_footswitch_leveling");

// ─── Doctor (tone diagnosis) ─────────────────────────────────────────────────

/** The Doctor RUN: capture + measure every selected sound (one backend command,
 * ~9 s each, read-only on the unit), streaming a progress row per sound; the
 * cohort-relative diagnoses + per-preset scene consistency ride the return.
 * `restoreListIndex` is the pre-run active preset (0-based), reloaded when the
 * run ends so the player's slot survives the check; null → the backend reloads
 * the last-scanned slot (either way the reference-level edit buffer is cleared). */
export const doctorCheck = (
  items: DoctorInputArg[],
  restoreListIndex: number | null,
  onResult: (item: DoctorProgressItem) => void,
): Promise<DoctorCheckResult> => {
  const channel = new Channel<DoctorProgressItem>();
  channel.onmessage = onResult;
  return invoke("doctor_check", { items, restoreListIndex, onResult: channel });
};

/** Cooperatively stop an in-flight Doctor check — already-measured sounds keep
 * their results. */
export const cancelDoctorCheck = (): Promise<void> =>
  invoke("cancel_doctor_check");

/** Apply one prescription's ops LIVE (no save): captures the stored state
 * (before clip), applies the edits to the device's edit buffer, captures again
 * without reloading (after clip). Revert with `doctorDiscard`; persist with
 * `doctorSave`. On a failed structural edit the preset is auto-restored. */
export const doctorApply = (job: DoctorApplyJob): Promise<DoctorApplyResult> =>
  invoke("doctor_apply", { job });

/** Persist an applied prescription (`save_current_preset`, identity-guarded).
 * Only offered behind the backup acknowledgment. */
export const doctorSave = (
  listIndex: number,
  expectName: string,
): Promise<void> => invoke("doctor_save", { listIndex, expectName });

/** Discard an applied-but-unsaved prescription by reloading the stored preset
 * (the device's edit buffer is dropped on load — the established revert). */
export const doctorDiscard = (listIndex: number): Promise<void> =>
  invoke("doctor_discard", { listIndex });

// ─── Instrument profiles + persisted store (Settings window) ──────────────────

/** Settings — profiles + per-slot assignment + named loudness targets. */
export const getStore = (): Promise<Store> => invoke("get_store");

/** Settings — persist the profile list. */
export const saveProfiles = (profiles: Profile[]): Promise<void> =>
  invoke("save_profiles", { profiles });

/** Settings — persist the editable LUFS targets. */
export const saveTargets = (targets: Target[]): Promise<void> =>
  invoke("save_targets", { targets });

/** Settings — persist the playback level leveling compensates for. */
export const setPlaybackLevel = (level: PlaybackLevel): Promise<void> =>
  invoke("set_playback_level", { level });

/** Settings — persist the auto-install-updates preference. */
export const setAutoInstallUpdates = (on: boolean): Promise<void> =>
  invoke("set_auto_install_updates", { on });

/** Settings — Tier-2 calibration: capture dry instrument for `secs` (clamped
 * 2..30), store K-weighted LUFS, return it + clip/stimulus-ceiling caveats.
 * DEVICE WRITE (persists calibration). Use a countdown, NOT window.confirm. */
export const calibrateProfile = (
  profileId: string,
  secs: number,
): Promise<CalibrateResult> => invoke("calibrate_profile", { profileId, secs });

/** Settings — shipped pickup-topology catalog. */
export const listPickupTopologies = (): Promise<TopologyInfo[]> =>
  invoke("list_pickup_topologies");

// ─── LevelView — active preset + songs + slot ops ──────────────────────────────

/** LevelView — the active (currently-loaded) preset's signal graph (live read). */
export const readActivePreset = (): Promise<ActiveGraph> =>
  invoke("read_active_preset");

/** LevelView — the monitor's CURRENT cached graph (the snapshot kept current by
 * every field-3 push). A cheap, no-device-I/O read used to seed the hero when a
 * fresh mount has no graph (e.g. a graphless connect + an idle device). Returns
 * null when the cache itself has no graph. */
export const currentGraph = (): Promise<ActiveGraph | null> =>
  invoke("current_graph");

/** One streamed row of the Level dialog's batch scene scan. `result` is null
 * when the slot read went unanswered (the row renders scanned-with-no-scenes). */
export interface SceneScanItem {
  list_index: number;
  result: PresetScenes | null;
}

/** Presets two-phase load — read the WHOLE user library (every preset + its scenes)
 * from one device backup (~22 s). DEVICE READ, non-destructive, persists nothing
 * (archive in RAM, temp DB dropped). Emits `tmp://backup-progress` as it streams
 * (subscribe via `onBackupProgress` to drive the scan strip). */
export const readLibraryViaBackup = (): Promise<BackupReadResult> =>
  invoke("read_library_via_backup");

/** Copy feature — apply each target preset's staged structural ops (replace / insert /
 * remove) LIVE on a held session, saving a preset only when all its ops confirmed.
 * DEVICE WRITE — only after the backup acknowledgment. Streams a {@link CopyApplyItem}
 * per preset as it goes; {@link cancelCopyApply} stops before the next preset. */
export const copyApply = (
  jobs: CopyJob[],
  save: boolean,
  onResult: (item: CopyApplyItem) => void,
): Promise<CopyApplyItem[]> => {
  const channel = new Channel<CopyApplyItem>();
  channel.onmessage = onResult;
  return invoke("copy_apply", { jobs, save, onResult: channel });
};

/** LevelView — songs as read live from the device (read-only overview). */
export const listSongs = (): Promise<SongRecord[]> => invoke("list_songs");

// ─── Songs & setlists — device-backed CRUD (Songs view) ───────────────────────
// The device is the single source of truth. Every WRITE follows a read-back-after-
// write contract: it returns the fresh authoritative list, because the device's
// positional slots shift on every add/remove (the UI must never predict them).
// `listSongs` is the song read. HW WRITES — gated by the read-only HW
// policy (lifted per-session by explicit authorization + a device backup).

/** Songs page — read every setlist's name live from the device. */
export const readSetlists = (): Promise<SetlistRecord[]> =>
  invoke("read_setlists");

/** Songs page — read a setlist's songs (ordered GLOBAL song slots, dense). The
 * 1-based index of a slot in THIS list is the position the membership ops address. */
export const listSetlistSongs = (setlistSlot: number): Promise<number[]> =>
  invoke("list_setlist_songs", { setlistSlot });

/** Delete a song by slot → fresh song list. DEVICE WRITE (removeSong). `expectName`
 * guards against a stale-mapping wrong-slot delete. DESTRUCTIVE. */
export const removeSong = (
  slot: number,
  expectName: string,
): Promise<SongRecord[]> => invoke("remove_song", { slot, expectName });

/** Create a setlist → fresh setlist list. DEVICE WRITE (addSetlist). */
export const addSetlist = (name: string): Promise<SetlistRecord[]> =>
  invoke("add_setlist", { name });

/** Rename a setlist by slot → fresh setlist list. DEVICE WRITE (renameSetlist). */
export const renameSetlist = (
  slot: number,
  name: string,
): Promise<SetlistRecord[]> => invoke("rename_setlist", { slot, name });

/** Delete a setlist by slot → fresh setlist list. DEVICE WRITE (removeSetlist);
 * the songs themselves are kept. `expectName` guards the slot. DESTRUCTIVE. */
export const removeSetlist = (
  slot: number,
  expectName: string,
): Promise<SetlistRecord[]> => invoke("remove_setlist", { slot, expectName });

/** Remove a song from a setlist by its 1-based POSITION within the setlist (NOT the
 * global song slot) → fresh ordered member slots. DEVICE WRITE (removeSetlistSong). */
export const removeSetlistSong = (
  setlistSlot: number,
  setlistSongSlot: number,
): Promise<number[]> =>
  invoke("remove_setlist_song", { setlistSlot, setlistSongSlot });

/** Reorder a song within a setlist by 1-based POSITIONS (array-splice semantics) →
 * fresh ordered member slots. DEVICE WRITE (moveSetlistSong). */
export const moveSetlistSong = (
  setlistSlot: number,
  oldPos: number,
  newPos: number,
): Promise<number[]> =>
  invoke("move_setlist_song", { setlistSlot, oldPos, newPos });

/** Batched song create: name + optional notes / BPM / setlist membership as ONE
 * device transaction (one seize bookend, one final read — vs one of each PER
 * FIELD on the granular commands). DEVICE WRITE. */
export const createSongFull = (
  name: string,
  notes: string | null,
  bpm: number | null,
  addToSetlist: number | null,
): Promise<SongSaveOutcome> =>
  invoke("create_song_full", { name, notes, bpm, addToSetlist });

/** Batched song update: only the CHANGED fields (null = unchanged) as ONE device
 * transaction. Callers skip the call entirely when nothing changed. DEVICE WRITE. */
export const updateSongFull = (
  slot: number,
  name: string | null,
  notes: string | null,
  bpm: number | null,
): Promise<SongSaveOutcome> =>
  invoke("update_song_full", { slot, name, notes, bpm });

/** Add several songs (GLOBAL song slots) to a setlist under ONE bookend with ONE
 * final membership read (vs one of each per song) → fresh ordered member slots.
 * DEVICE WRITE (addSetlistSong ×N). */
export const addSetlistSongs = (
  setlistSlot: number,
  songSlots: number[],
): Promise<number[]> => invoke("add_setlist_songs", { setlistSlot, songSlots });

/**
 * Namespaced handle for screens that prefer `cmd.importLibrary(...)` over named
 * imports. Every wrapper above is also exported individually.
 */
export const cmd = {
  // Connection + app
  appInfo,
  connectDevice,
  listPresets,
  // Leveling
  listLevelBlocks,
  levelPreset,
  restorePresetLevel,
  // Profiles + store
  getStore,
  saveProfiles,
  saveTargets,
  setPlaybackLevel,
  setAutoInstallUpdates,
  calibrateProfile,
  listPickupTopologies,
  // LevelView — active preset + songs
  readActivePreset,
  currentGraph,
  listSongs,
  // Songs & setlists — device-backed CRUD
  readSetlists,
  listSetlistSongs,
  removeSong,
  addSetlist,
  renameSetlist,
  removeSetlist,
  removeSetlistSong,
  moveSetlistSong,
  createSongFull,
  updateSongFull,
  addSetlistSongs,
  // Copy blocks between presets
  copyApply,
  // Doctor (tone diagnosis)
  doctorCheck,
  doctorApply,
  doctorSave,
  doctorDiscard,
} as const;
