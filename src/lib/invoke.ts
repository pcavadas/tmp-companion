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
  SetlistJobEntry,
  SetlistResult,
  LevelBlock,
  Profile,
  Target,
  Store,
  PlaybackLevel,
  TopologyInfo,
  SampleInfo,
  LibraryRecord,
  ReconcileReport,
  FilterArgs,
  OpSpec,
  DryRunEntry,
  BulkApplyResult,
  RevertEntry,
  RenameSpecArg,
  RenameApplyRow,
  RecipeArg,
  BlockTemplate,
  SpectrumResult,
  EqMatchResult,
  SicRank,
  Finding,
  MigrationRow,
  MigrationPlan,
  MigrationApplyRow,
  ActiveGraph,
  ConnectResult,
  SongRecord,
  SetlistRecord,
  SongSaveOutcome,
  SceneListRow,
  PresetScenes,
  BackupReadResult,
  SavedBlock,
  UserIr,
  BulkReplaceItem,
  CopyJob,
  CopyApplyItem,
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

/** App identity `{ name, version }`. */
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

/** Common-target leveling: every preset to `min(C) − headroom`.
 * DEVICE WRITE when `save` is true. */
export const levelSetlist = (
  entries: SetlistJobEntry[],
  save: boolean,
): Promise<SetlistResult> => invoke("level_setlist", { entries, save });

/** MEASURE path: re-amp + loadScene per scene → per-scene gain offsets.
 * Drives the device. */
export const levelScenes = (
  slot: number,
  sceneCount: number,
  topologyId: string | null,
  headroomLu: number,
): Promise<number[]> =>
  invoke("level_scenes", { slot, sceneCount, topologyId, headroomLu });

/** A candidate leveling knob for `levelScenesApply` — pass EVERY amp-level candidate;
 * the backend picks per scene the one whose block is ON in that scene (scenes can
 * swap which amp is live, and a bypassed amp's knob measures flat). */
export interface LevelBlockCandidate {
  groupId: string;
  nodeId: string;
  parameterId: string;
  value: number;
}

/** APPLY per-scene leveling: drive the scene's ACTIVE amp's level knob
 * closed-loop to `targetLufs` for each selected scene with per-block Scene Edit
 * enabled (so the level lands on that scene's overlay). `sceneSlots` are the WIRE
 * slots: 0-based scenes[] indices; BASE_SCENE_SLOT (8) = the base/preset value.
 * DEVICE WRITE when `save` (opt-in, behind the read-only HW policy). Returns one
 * LevelResult per scene. */
export const levelScenesApply = (args: {
  slot: number;
  sceneSlots: number[];
  candidates: LevelBlockCandidate[];
  targetLufs: number;
  save: boolean;
  topologyId: string | null;
  calibrationLufs: number | null;
}): Promise<LevelResult[]> => invoke("level_scenes_apply", args);

export interface SceneLevelProgressItem {
  sceneSlot: number;
  status: "active" | "done" | "error" | "cancelled";
  result: LevelResult | null;
  message: string | null;
}

/** Batched APPLY path. One backend command levels all selected scenes
 * and streams row progress over a Tauri channel. */
export const levelScenesApplyBatched = (
  args: {
    slot: number;
    sceneSlots: number[];
    candidates: LevelBlockCandidate[];
    targetLufs: number;
    save: boolean;
    /** Opt-in: equalize a path-MERGE scene's two lanes before joint-k (no effect on
     * series / single-amp / split-output scenes). */
    rebalance: boolean;
    topologyId: string | null;
    calibrationLufs: number | null;
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

/** Settings — Tier-2 calibration: capture dry instrument for `secs` (clamped
 * 2..30), store K-weighted LUFS, return it. DEVICE WRITE (persists calibration).
 * Use a countdown, NOT window.confirm. */
export const calibrateProfile = (
  profileId: string,
  secs: number,
): Promise<number> => invoke("calibrate_profile", { profileId, secs });

/** Settings — shipped pickup-topology catalog. */
export const listPickupTopologies = (): Promise<TopologyInfo[]> =>
  invoke("list_pickup_topologies");

/** Settings — bundled stimulus WAVs. */
export const listSamples = (): Promise<SampleInfo[]> => invoke("list_samples");

// ─── Library ───────────────────────────────────────────────────────────────

/** Ingest an OFFLINE `.preset` folder + reconcile against the device list. */
export const importLibrary = (folder: string): Promise<ReconcileReport> =>
  invoke("import_library", { folder });

/** All ingested records (`decoded_json` omitted on the wire). Errors if
 * no import yet. */
export const libraryRecords = (): Promise<LibraryRecord[]> =>
  invoke("library_records");

/** List indices of matching writable (matched) records; unmatched /
 * ambiguous dropped. `filter` keys are snake_case (FilterArgs). */
export const libraryFilter = (filter: FilterArgs): Promise<number[]> =>
  invoke("library_filter", { filter });

// ─── Bulk run engine ──────────────────────────────────────────────────────────

/** Preview a bulk op over a selection; writes nothing. */
export const bulkDryRun = (
  selection: number[],
  op: OpSpec,
): Promise<DryRunEntry[]> => invoke("bulk_dry_run", { selection, op });

/** Apply a bulk op over a selection. DEVICE WRITE / persists changes;
 * `backup` snapshots originals. */
export const bulkApply = (
  selection: number[],
  op: OpSpec,
  backup: boolean,
): Promise<BulkApplyResult> => invoke("bulk_apply", { selection, op, backup });

/** Restore from a run's snapshots. DEVICE WRITE / restores changes. */
export const bulkRevert = (runId: string): Promise<RevertEntry[]> =>
  invoke("bulk_revert", { runId });

/** Backup snapshot file paths, newest first (audit trail). */
export const listSnapshots = (): Promise<string[]> => invoke("list_snapshots");

// ─── Rename + variants ────────────────────────────────────────────────────────

/** LIVE per-preset rename+save. DEVICE WRITE. Skips no-op / invalid rows. */
export const bulkRename = (
  selection: number[],
  spec: RenameSpecArg,
): Promise<RenameApplyRow[]> => invoke("bulk_rename", { selection, spec });

/** LIVE append-import (device files at next empty slot). DEVICE WRITE.
 * Returns a human status string. */
export const createVariant = (
  sourceListIndex: number,
  recipe: RecipeArg,
): Promise<string> => invoke("create_variant", { sourceListIndex, recipe });

// ─── Block templates ──────────────────────────────────────────────────────────

/** Saved block templates (app config dir). */
export const listBlockTemplates = (): Promise<BlockTemplate[]> =>
  invoke("list_block_templates");

/** Capture first block of `model` from source; replaces same-named.
 * Returns the new list. */
export const saveBlockTemplate = (
  sourceListIndex: number,
  model: string,
  name: string,
): Promise<BlockTemplate[]> =>
  invoke("save_block_template", { sourceListIndex, model, name });

// ─── Spectral analysis + audition ─────────────────────────────────────────────

/** Per-band energies + tonal flags. Drives device (re-amp capture). */
export const spectrumScan = (
  slot: number,
  topologyId: string | null,
): Promise<SpectrumResult> => invoke("spectrum_scan", { slot, topologyId });

/** Source vs reference spectra + match deltas. Drives device. */
export const eqMatch = (
  sourceSlot: number,
  referenceSlot: number,
  topologyId: string | null,
): Promise<EqMatchResult> =>
  invoke("eq_match", { sourceSlot, referenceSlot, topologyId });

/** Rank candidates by spectral distance to target. Drives device. */
export const rankCandidates = (
  targetSlot: number,
  candidateSlots: number[],
  topologyId: string | null,
): Promise<SicRank[]> =>
  invoke("rank_candidates", { targetSlot, candidateSlots, topologyId });

/** Render a preset clip; returns a (cached) data/clip URL string. Drives device. */
export const auditionRender = (
  slot: number,
  topologyId: string | null,
): Promise<string> => invoke("audition_render", { slot, topologyId });

// ─── Migration + loudness audit ───────────────────────────────────────────────

/** Presets affected by a firmware catalog change. */
export const migrationScan = (
  targetCatalog: string[],
): Promise<MigrationRow[]> => invoke("migration_scan", { targetCatalog });

/** Classified diff + planned swaps. `renameMap` keys = old model id. */
export const migrationPlan = (
  targetCatalog: string[],
  renameMap: Record<string, string>,
): Promise<MigrationPlan> =>
  invoke("migration_plan", { targetCatalog, renameMap });

/** Apply swaps + OFFLINE re-import. DEVICE WRITE when
 * `dryRun=false`. */
export const migrationApply = (
  targetCatalog: string[],
  renameMap: Record<string, string>,
  dryRun: boolean,
): Promise<MigrationApplyRow[]> =>
  invoke("migration_apply", { targetCatalog, renameMap, dryRun });

/** Flag loudness outliers (drives device). */
export const auditLoudness = (
  slots: number[],
  topologyId: string | null,
  outlierLu: number,
): Promise<Finding[]> =>
  invoke("audit_loudness", { slots, topologyId, outlierLu });

// ─── Songs — preset assignment ────────────────────────────────────────────────

/** Assign a preset to a song row. DEVICE WRITE (assignSongPreset).
 * `userListIndex` is 0-based (session adds +1). */
export const songAssign = (
  songSlot: number,
  songPresetSlot: number,
  userListIndex: number,
  footswitchLabel: string,
  footswitchColor: number,
  presetSceneSlot: number,
): Promise<void> =>
  invoke("song_assign", {
    songSlot,
    songPresetSlot,
    userListIndex,
    footswitchLabel,
    footswitchColor,
    presetSceneSlot,
  });

/** Clear a song row. DEVICE WRITE (clearSongPreset). */
export const songClear = (
  songSlot: number,
  songPresetSlot: number,
): Promise<void> => invoke("song_clear", { songSlot, songPresetSlot });

/** Move a song row. DEVICE WRITE (moveSongPreset). Rust params are `old`/`new`. */
export const songMove = (
  songSlot: number,
  old: number,
  newPos: number,
): Promise<void> => invoke("song_move", { songSlot, old, new: newPos });

/** Swap two song rows. DEVICE WRITE (swapSongPreset). */
export const songSwap = (
  songSlot: number,
  a: number,
  b: number,
): Promise<void> => invoke("song_swap", { songSlot, a, b });

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

/** Scenes — fetch the ACTIVE preset's scene list on demand (`sceneListRequest`).
 * The live-sync monitor already emits `tmp://scene-list` UNSOLICITED on every preset
 * load (the canonical path); this is a manual / first-paint top-up for a mid-preset
 * connect. `fs` is best-effort (null for now — em-dash). DEVICE READ. */
export const requestSceneList = (): Promise<SceneListRow[]> =>
  invoke("request_scene_list");

/** Live-sync — STOP: drop the monitor's seize and re-establish the persistent UI
 * session (so `listPresets` / slot ops work as before). Resolves with the reclaimed
 * session's firmware version (like `connectDevice`), or null. Idempotent. */
export const stopLiveSync = (): Promise<string | null> =>
  invoke("stop_live_sync");

/** Scenes — pure-lazy metadata read for one preset by 0-based My-Presets index.
 * Non-destructive field-8 slot read; returns scene names and real FS tags. */
export const readPresetScenes = (listIndex: number): Promise<PresetScenes> =>
  invoke("read_preset_scenes", { listIndex });

/** One streamed row of the Level dialog's batch scene scan. `result` is null
 * when the slot read went unanswered (the row renders scanned-with-no-scenes). */
export interface SceneScanItem {
  list_index: number;
  result: PresetScenes | null;
}

/** Scenes — batch scan for the Level dialog: one dedicated session reads every
 * selected preset's scenes back-to-back (~0.5 s each, zero LoadPreset) and
 * streams each result as it lands. Resolves when the sweep ends. */
export const scanPresetScenes = (
  listIndices: number[],
  onResult: (item: SceneScanItem) => void,
): Promise<void> => {
  const channel = new Channel<SceneScanItem>();
  channel.onmessage = onResult;
  return invoke("scan_preset_scenes", { listIndices, onResult: channel });
};

/** Cooperatively stop an in-flight `scanPresetScenes` sweep (skip / dialog close). */
export const cancelSceneScan = (): Promise<void> => invoke("cancel_scene_scan");

/** Presets two-phase load — read the WHOLE user library (every preset + its scenes)
 * from one device backup (~22 s). DEVICE READ, non-destructive, persists nothing
 * (archive in RAM, temp DB dropped). Emits `tmp://backup-progress` as it streams
 * (subscribe via `onBackupProgress` to drive the scan strip). */
export const readLibraryViaBackup = (): Promise<BackupReadResult> =>
  invoke("read_library_via_backup");

/** The user's saved blocks (incl. saved dual-cabs), read live in
 * one handshake burst (`RequestAllBlockPresets`). DEVICE READ, instant, no backup. */
export const listSavedBlocks = (): Promise<SavedBlock[]> =>
  invoke("list_saved_blocks");

/** The user's impulse responses, read live (`UserIRListRequest`).
 * DEVICE READ, instant. Empty when the device has no user IRs loaded. */
export const listUserIrs = (): Promise<UserIr[]> => invoke("list_user_irs");

/** Apply one per-node edit across the selected presets, live, via
 * the device's own structural edit (`ReplaceNode` for a model, `ReplaceNodeWithBlock`
 * for a saved block / dual cab by index, `ReplaceNode`→`ACD_UserIRTMS` + a string
 * `ChangeParameter` for a user IR, `RemoveNode` to delete the block). DEVICE WRITE —
 * only after the backup acknowledgment. Streams a `BulkReplaceItem` per preset as it
 * goes; `cancelBulkReplace` stops the sweep before the next preset. */
export const bulkReplaceLive = (
  args: {
    selection: number[];
    fromId: string;
    repl:
      | { kind: "model"; fenderId: string }
      | { kind: "ir"; fenderId: string; file: string }
      | { kind: "saved"; fenderId: string; index: number }
      | { kind: "remove" };
    save: boolean;
  },
  onResult: (item: BulkReplaceItem) => void,
): Promise<BulkReplaceItem[]> => {
  const channel = new Channel<BulkReplaceItem>();
  channel.onmessage = onResult;
  return invoke("bulk_replace_live", { ...args, onResult: channel });
};

/** Stop an in-flight {@link bulkReplaceLive} sweep after the current preset (the
 * wizard's Stop). Presets already saved stay changed; the rest are left untouched. */
export const cancelBulkReplace = (): Promise<void> =>
  invoke("cancel_bulk_replace");

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

/** Stop an in-flight {@link copyApply} run after the current preset. Presets already
 * saved stay changed; the rest are left untouched. */
export const cancelCopyApply = (): Promise<void> => invoke("cancel_copy_apply");

/** LevelView — songs as read live from the device (read-only overview). */
export const listSongs = (): Promise<SongRecord[]> => invoke("list_songs");

/** LevelView — load a preset onto the amp by 0-based list index. DEVICE WRITE
 * (loadPreset; session adds +1 for the device slot). */
export const loadPresetOnAmp = (listIndex: number): Promise<void> =>
  invoke("load_preset_on_amp", { listIndex });

/** LevelView — delete (clear) a preset by 0-based list index. DEVICE WRITE
 * (clearUserPreset). `expectName` guards against a stale-mapping wrong-slot wipe. */
export const deletePreset = (
  listIndex: number,
  expectName: string,
): Promise<void> => invoke("delete_preset", { listIndex, expectName });

/** LevelView — move a preset between 0-based list indices. DEVICE WRITE
 * (moveUserPreset). */
export const movePreset = (from: number, to: number): Promise<void> =>
  invoke("move_preset", { from, to });

/** LevelView — rename + save a preset by 0-based list index. DEVICE WRITE
 * (renameCurrentPreset + saveCurrentPreset). */
export const renameSavePreset = (
  listIndex: number,
  name: string,
): Promise<void> => invoke("rename_save_preset", { listIndex, name });

/** Scenes — recall a scene on the amp (`loadScene`). `sceneSlot` is the 0-based
 * scenes[] index within the active preset (row index maps 1:1); BASE_SCENE_SLOT (8)
 * recalls the base scene. Pass `listIndex` (0-based) to load that preset first when
 * the scene belongs to a non-active preset; pass `null` to recall on the active
 * preset. DEVICE WRITE (switches the live tone). */
export const loadSceneOnAmp = (
  listIndex: number | null,
  sceneSlot: number,
): Promise<void> => invoke("load_scene_on_amp", { listIndex, sceneSlot });

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

/** Create a song → fresh song list. DEVICE WRITE (addSong). */
export const addSong = (name: string): Promise<SongRecord[]> =>
  invoke("add_song", { name });

/** Rename a song by slot → fresh song list. DEVICE WRITE (renameSong). */
export const renameSong = (slot: number, name: string): Promise<SongRecord[]> =>
  invoke("rename_song", { slot, name });

/** Delete a song by slot → fresh song list. DEVICE WRITE (removeSong). `expectName`
 * guards against a stale-mapping wrong-slot delete. DESTRUCTIVE. */
export const removeSong = (
  slot: number,
  expectName: string,
): Promise<SongRecord[]> => invoke("remove_song", { slot, expectName });

/** Set a song's notes by slot → fresh song list. DEVICE WRITE (songNotes). */
export const setSongNotes = (
  slot: number,
  notes: string,
): Promise<SongRecord[]> => invoke("set_song_notes", { slot, notes });

/** Set a song's BPM by slot → fresh song list. DEVICE WRITE (tapTempoBpm on the
 * activated song; the backend hides the footswitch+activate+retry flow). */
export const setSongBpm = (slot: number, bpm: number): Promise<SongRecord[]> =>
  invoke("set_song_bpm", { slot, bpm });

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

/** Add a song (by GLOBAL song slot) to a setlist → fresh ordered member slots.
 * DEVICE WRITE (addSetlistSong). */
export const addSetlistSong = (
  setlistSlot: number,
  songSlot: number,
): Promise<number[]> => invoke("add_setlist_song", { setlistSlot, songSlot });

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
  levelSetlist,
  levelScenes,
  levelScenesApply,
  // Profiles + store
  getStore,
  saveProfiles,
  saveTargets,
  setPlaybackLevel,
  calibrateProfile,
  listPickupTopologies,
  listSamples,
  // Library
  importLibrary,
  libraryRecords,
  libraryFilter,
  // Bulk run
  bulkDryRun,
  bulkApply,
  bulkRevert,
  listSnapshots,
  // Rename + variants
  bulkRename,
  createVariant,
  // Block templates + globals
  listBlockTemplates,
  saveBlockTemplate,
  // Spectral + audition
  spectrumScan,
  eqMatch,
  rankCandidates,
  auditionRender,
  // Migration + loudness
  migrationScan,
  migrationPlan,
  migrationApply,
  auditLoudness,
  // Songs — preset assignment
  songAssign,
  songClear,
  songMove,
  songSwap,
  // LevelView — active preset + songs + slot ops
  readActivePreset,
  currentGraph,
  requestSceneList,
  readPresetScenes,
  scanPresetScenes,
  cancelSceneScan,
  stopLiveSync,
  listSongs,
  loadPresetOnAmp,
  deletePreset,
  movePreset,
  renameSavePreset,
  loadSceneOnAmp,
  // Songs & setlists — device-backed CRUD
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
  createSongFull,
  updateSongFull,
  addSetlistSongs,
  // Live block replace/remove
  listSavedBlocks,
  listUserIrs,
  bulkReplaceLive,
  cancelBulkReplace,
  // Copy blocks between presets
  copyApply,
  cancelCopyApply,
} as const;
