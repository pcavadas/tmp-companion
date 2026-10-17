// TMP Companion — typed Tauri command wrappers.
//
// Typed wrappers, one per backend command; arg keys are camelCase, Tauri converts
// to snake_case. Every wrapper goes through `invokeCommand<K>`, whose args and return
// are checked against the ONE command registry (`./commands`), so a wrapper can't
// send a key or promise a return its registry entry doesn't declare.
//
// CASING (load-bearing): top-level arg keys are camelCase — Tauri 2 auto-converts
// them to the Rust snake_case params. Keys nested inside a JSON payload object
// (job/op/spec/recipe/filter/edit/store) are snake_case and modelled by the types
// in ./types. Do NOT "fix" either casing.

import { Channel, invoke as tauriInvoke } from "@tauri-apps/api/core";

import type {
  CommandArgList,
  CommandName,
  CommandRet,
  FootswitchLevelApplyArgs,
  SceneLevelApplyArgs,
} from "./commands";
import {
  errorKind,
  errorReampStillOn,
  isConnectGateKind,
} from "./connectError";
import { errMsg } from "./format";
import { logError } from "./log";
import type {
  AppInfo,
  BatchLevelRun,
  CommandErrorKind,
  PresetEntry,
  LevelJob,
  LevelResult,
  FootswitchLevelResult,
  FootswitchLevelProgressItem,
  LevelBlock,
  Profile,
  CalibrateResult,
  Target,
  Store,
  PlaybackChannels,
  PlaybackLevel,
  TopologyInfo,
  ActiveGraph,
  ConnectResult,
  SongRecord,
  SetlistRecord,
  SongSaveOutcome,
  SceneHandleRow,
  SceneLevelProgressItem,
  FsSceneContext,
  BackupReadResult,
  CopyJob,
  CopyApplyItem,
  DoctorInputArg,
  DoctorProgressItem,
  DoctorCheckResult,
  SupportBundleResult,
} from "./types";

// isTauri lives in ./log (the dependency-free end of the invoke→log edge — a
// definition here would close an invoke⇄log module cycle, the TDZ-crash class);
// re-exported so callers keep importing it from the wrappers module.
export { isTauri } from "./log";

/**
 * A rejected backend command. Most commands return `Result<_, String>`, so a
 * rejection reaches JS as a bare STRING — and `e instanceof Error ? e.message :
 * "generic text"` at a call site silently threw the backend's real reason away.
 * A command migrated to the typed error (`error::AppError`: `connect_device` and the
 * three leveling commands) rejects with `{ kind, message }` instead — the leveling
 * ones add `reampStillOn`. `invoke` normalizes BOTH shapes into
 * this one `Error` subclass at the seam, so a `catch` always gets an `Error` whose
 * `message` is the backend text and whose `kind` says what failed (`"other"` for a
 * bare string). Branch on `kind`, never on the message text.
 */
export class CommandError extends Error {
  /** The backend command that rejected (e.g. `copy_apply`). */
  readonly cmd: string;
  /** What failed, for a caller that presents kinds differently. A leveling command's
   *  kind is a `SkipKind` (why the run, or the sound, was not leveled). */
  readonly kind: CommandErrorKind;
  /** A failed leveling command whose run-end re-amp OFF never read back OFF: the unit's
   *  guitar input may still be muted. `false` for every other rejection. */
  readonly reampStillOn: boolean;

  constructor(
    cmd: string,
    message: string,
    kind: CommandErrorKind = "other",
    reampStillOn = false,
  ) {
    super(message);
    this.name = "CommandError";
    this.cmd = cmd;
    this.kind = kind;
    this.reampStillOn = reampStillOn;
  }
}

/**
 * Log a caught failure unless it is a {@link CommandError}, which `invokeCommand` has
 * already logged. The UI never prints a failure's text (it says what happened in plain
 * words), so a failure thrown by the app's own code around a command — post-processing a
 * reply, say — would otherwise reach neither the screen nor the log, and so never a bug
 * report. `where` names the caller in the log line.
 */
export function logUnlessCommandError(where: string, e: unknown): void {
  if (e instanceof CommandError) return;
  const stack = e instanceof Error && e.stack ? `\n${e.stack}` : "";
  logError(`${where} failed: ${errMsg(e)}${stack}`);
}

/**
 * The ONE call path every wrapper below goes through, so a backend command
 * rejection always reaches the on-disk log (`logError`) before it propagates —
 * otherwise a rejection a caller doesn't `.catch()` vanishes with nothing but an
 * in-memory stack trace. Rejects with a {@link CommandError} carrying the
 * backend's own text (see there).
 */
function invokeCommand<K extends CommandName>(
  cmd: K,
  ...args: CommandArgList<K>
): Promise<CommandRet<K>> {
  return tauriInvoke<CommandRet<K>>(cmd, args[0]).catch((e: unknown) => {
    // A bare string (most commands) or a typed `{ kind, message }` (see CommandError).
    const message = errMsg(e);
    const kind = errorKind(e);
    // The 3 s auto-connect poll rejects BY DESIGN while no unit is present —
    // error-logging that class (~1 line/3 s, forever, while unplugged) would
    // churn the size-capped log and evict the diagnostics a bug report needs,
    // so the connect gate's kinds (no unit / unit still booting) stay quiet.
    if (cmd !== "connect_device" || !isConnectGateKind(kind))
      logError(`invoke(${cmd}) failed: ${message}`);
    throw new CommandError(cmd, message, kind, errorReampStillOn(e));
  });
}

// ─── Connection + app ─────────────────────────────────────────────────────────

/** App identity — name + the running build's version (Tauri config). */
export const appInfo = (): Promise<AppInfo> => invokeCommand("app_info");

/** Seize the TMP + combined handshake (firmware + discovery in one
 * burst, mirroring Pro Control). Returns both firmware version and the active
 * preset's signal graph, so the preset list and signal chain can render
 * simultaneously with no second handshake. */
export const connectDevice = (): Promise<ConnectResult> =>
  invokeCommand("connect_device");

/** Device "My Presets"; each `{ slot, name }` (slot = 0-based list index). */
export const listPresets = (): Promise<PresetEntry[]> =>
  invokeCommand("list_presets");

// ─── Loudness leveling ──────────────────────────────────────────────────────

/** Enumerate a preset's level-type block controls (load → discovery). */
export const listLevelBlocks = (slot: number): Promise<LevelBlock[]> =>
  invokeCommand("list_level_blocks", { slot });

/** One-shot open-loop on `presetLevel` (or closed-loop on a block knob
 * when all three `block_*` coords are set). DEVICE WRITE when `job.save` is true. */
export const levelPreset = (job: LevelJob): Promise<LevelResult> =>
  invokeCommand("level_preset", { job });

/** Cooperatively cancel an in-flight {@link levelPreset} run — sets a backend flag the
 *  leveller checks at its seams, so the current item bails before its apply+save. */
export const cancelPresetLeveling = (): Promise<void> =>
  invokeCommand("cancel_preset_leveling");

/** Batched APPLY path. One backend command levels all selected scenes and streams row
 * progress over a Tauri channel. Each job carries its OWN per-scene target, so a
 * mixed-target preset still levels in ONE batch. Resolves with the leveled rows and the
 * run-end re-amp verdict beside them (`BatchLevelRun`). */
export const levelScenesApplyBatched = (
  args: SceneLevelApplyArgs,
  onResult: (item: SceneLevelProgressItem) => void,
): Promise<BatchLevelRun<LevelResult>> => {
  const channel = new Channel<SceneLevelProgressItem>();
  channel.onmessage = onResult;
  return invokeCommand("level_scenes_apply_batched", {
    ...args,
    onResult: channel,
  });
};

/** Cooperatively stop an in-flight batched scene-leveling run. */
export const cancelSceneLeveling = (): Promise<void> =>
  invokeCommand("cancel_scene_leveling");

/** Per-scene handle candidates for the Set-up step's scene control picker — every
 * FS scene's controls in ONE call, so a scene row's picker fetches lazily (on first
 * open) and caches per preset rather than firing per row. No scene is recalled on the
 * unit and nothing is measured — but it is NOT always just one field-8 read: on a
 * truncated body (the `scenes` tail cut off) the backend falls back to a synchronous
 * whole-library device-backup transfer (multi-second) before it can answer. */
export const listSceneLevelHandles = (
  slot: number,
): Promise<SceneHandleRow[]> =>
  invokeCommand("list_scene_level_handles", { slot });

/** Which scenes enable each footswitch of `slot` (D3) — the Set-up step's per-footswitch
 * scene-context picker source. No scene is recalled on the unit and nothing is measured —
 * but it is NOT always just one field-8 read: on a truncated body (the `ftsw` tail cut
 * off) the backend falls back to a synchronous whole-library device-backup transfer
 * (multi-second) before it can answer. */
export const listFootswitchSceneContexts = (
  slot: number,
): Promise<FsSceneContext[]> =>
  invokeCommand("list_footswitch_scene_contexts", { slot });

/** Level one or more block-acting footswitches' engaged states for preset `slot`,
 * streaming a progress row per switch. Each job picks a switch + the block param to
 * solve. Mirrors `levelScenesApplyBatched`, return shape included. */
export const levelFootswitchesApply = (
  args: FootswitchLevelApplyArgs,
  onResult: (item: FootswitchLevelProgressItem) => void,
): Promise<BatchLevelRun<FootswitchLevelResult>> => {
  const channel = new Channel<FootswitchLevelProgressItem>();
  channel.onmessage = onResult;
  return invokeCommand("level_footswitches_apply", {
    ...args,
    onResult: channel,
  });
};

/** Cooperatively stop an in-flight footswitch-leveling run. */
export const cancelFootswitchLeveling = (): Promise<void> =>
  invokeCommand("cancel_footswitch_leveling");

// ─── Doctor (tone diagnosis) ─────────────────────────────────────────────────

/** The Doctor RUN: capture + measure every selected sound (one backend command,
 * ~9 s each, read-only on the unit), streaming a progress row per sound; the
 * per-sound target-deviation diagnoses + per-preset scene consistency ride the return.
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
  return invokeCommand("doctor_check", {
    items,
    restoreListIndex,
    onResult: channel,
  });
};

/** Cooperatively stop an in-flight Doctor check — already-measured sounds keep
 * their results. */
export const cancelDoctorCheck = (): Promise<void> =>
  invokeCommand("cancel_doctor_check");

// ─── Instrument profiles + persisted store (Settings window) ──────────────────

/** Settings — profiles + per-slot assignment + named loudness targets. */
export const getStore = (): Promise<Store> => invokeCommand("get_store");

/** Settings — persist the profile list. */
export const saveProfiles = (profiles: Profile[]): Promise<void> =>
  invokeCommand("save_profiles", { profiles });

/** Settings — persist the editable LUFS targets. */
export const saveTargets = (targets: Target[]): Promise<void> =>
  invokeCommand("save_targets", { targets });

/** Settings — persist the playback level leveling compensates for. */
export const setPlaybackLevel = (level: PlaybackLevel): Promise<void> =>
  invokeCommand("set_playback_level", { level });

/** Settings — persist which output(s) the rig plays (the leveling meter follows). */
export const setPlaybackChannels = (
  channels: PlaybackChannels,
): Promise<void> => invokeCommand("set_playback_channels", { channels });

/** Settings — persist the auto-install-updates preference. */
export const setAutoInstallUpdates = (on: boolean): Promise<void> =>
  invokeCommand("set_auto_install_updates", { on });

/** Remember the instrument chosen for one preset (0-based list index) in the app's own
 * store on this computer — never on the unit. `profileId` is a profile id or
 * `NO_INSTRUMENT` ("none"), an explicit "None" that is saved and counts as chosen; a
 * later choice overwrites it. Read back as `Store.profile_by_slot`. */
export const setPresetInstrument = (
  slot: number,
  profileId: string,
): Promise<void> => invokeCommand("set_preset_instrument", { slot, profileId });

/** Settings — Tier-2 calibration: capture dry instrument for `secs` (clamped
 * 2..30), store K-weighted LUFS, return it + clip/stimulus-ceiling caveats.
 * DEVICE WRITE (persists calibration). Use a countdown, NOT window.confirm. */
export const calibrateProfile = (
  profileId: string,
  secs: number,
): Promise<CalibrateResult> =>
  invokeCommand("calibrate_profile", { profileId, secs });

/** Settings — shipped pickup-topology catalog. */
export const listPickupTopologies = (): Promise<TopologyInfo[]> =>
  invokeCommand("list_pickup_topologies");

/** Settings — write a diagnostics bundle (recent logs, device settings, app info,
 * + the opt-in preset) to a .tar in ~/Downloads; returns its path. All args
 * nullable; every text member is home-path-scrubbed backend-side. */
export const saveSupportBundle = (
  firmware: string | null,
  presetJson: string | null,
  presetName: string | null,
): Promise<SupportBundleResult> =>
  invokeCommand("save_support_bundle", { firmware, presetJson, presetName });

/** Settings — same bundle as `saveSupportBundle`, returned as raw bytes (no
 * Downloads write) for an in-app "send report" attach. */
export const buildSupportBundle = (
  firmware: string | null,
  presetJson: string | null,
  presetName: string | null,
): Promise<ArrayBuffer> =>
  invokeCommand("build_support_bundle", { firmware, presetJson, presetName });

// ─── LevelView — active preset + songs + slot ops ──────────────────────────────

/** LevelView — the active (currently-loaded) preset's signal graph (live read). */
export const readActivePreset = (): Promise<ActiveGraph> =>
  invokeCommand("read_active_preset");

/** LevelView — the monitor's CURRENT cached graph (the snapshot kept current by
 * every field-3 push). A cheap, no-device-I/O read used to seed the hero when a
 * fresh mount has no graph (e.g. a graphless connect + an idle device). Returns
 * null when the cache itself has no graph. */
export const currentGraph = (): Promise<ActiveGraph | null> =>
  invokeCommand("current_graph");

/** Presets two-phase load — read the WHOLE user library (every preset + its scenes)
 * from one device backup (~22 s). DEVICE READ, non-destructive, persists nothing
 * (archive in RAM, temp DB dropped). Emits `tmp://backup-progress` as it streams
 * (subscribe via `onLive("backupProgress", …)` to drive the scan strip). */
export const readLibraryViaBackup = (): Promise<BackupReadResult> =>
  invokeCommand("read_library_via_backup");

/** Copy feature — apply each target preset's staged structural ops (replace / insert /
 * remove) LIVE on a held session, saving a preset only when all its ops confirmed.
 * DEVICE WRITE — only after the backup acknowledgment. Streams a {@link CopyApplyItem}
 * per preset as it goes. There is no cancel: the Copy tab has no Stop button. */
export const copyApply = (
  jobs: CopyJob[],
  save: boolean,
  onResult: (item: CopyApplyItem) => void,
): Promise<CopyApplyItem[]> => {
  const channel = new Channel<CopyApplyItem>();
  channel.onmessage = onResult;
  return invokeCommand("copy_apply", { jobs, save, onResult: channel });
};

/** LevelView — songs as read live from the device (read-only overview). */
export const listSongs = (): Promise<SongRecord[]> =>
  invokeCommand("list_songs");

// ─── Songs & setlists — device-backed CRUD (Songs view) ───────────────────────
// The device is the single source of truth. Every WRITE follows a read-back-after-
// write contract: it returns the fresh authoritative list, because the device's
// positional slots shift on every add/remove (the UI must never predict them).
// `listSongs` is the song read. HW WRITES — gated by the read-only HW
// policy (lifted per-session by explicit authorization + a device backup).

/** Songs page — read every setlist's name live from the device. */
export const readSetlists = (): Promise<SetlistRecord[]> =>
  invokeCommand("read_setlists");

/** Songs page — read a setlist's songs (ordered GLOBAL song slots; trailing empties
 * dropped, a `0` between members kept as an unknown entry). The 1-based index of a
 * slot in THIS list is the position the membership ops address. */
export const listSetlistSongs = (setlistSlot: number): Promise<number[]> =>
  invokeCommand("list_setlist_songs", { setlistSlot });

/** The names of a song or setlist list, in order: what every slot-keyed write sends as
 * `expectList`. Names can repeat, so only the whole list the UI saw confirms a slot. */
interface Named {
  readonly name: string;
}
const names = (list: readonly Named[]): string[] => list.map((r) => r.name);

/** Delete a song by slot → fresh song list. DEVICE WRITE (removeSong). `expectName`
 * and `seen` (the song list the UI saw) guard against a stale-mapping wrong-slot
 * delete. DESTRUCTIVE. */
export const removeSong = (
  slot: number,
  expectName: string,
  seen: readonly Named[],
): Promise<SongRecord[]> =>
  invokeCommand("remove_song", { slot, expectName, expectList: names(seen) });

/** Create a setlist → fresh setlist list. DEVICE WRITE (addSetlist). */
export const addSetlist = (name: string): Promise<SetlistRecord[]> =>
  invokeCommand("add_setlist", { name });

/** Rename a setlist by slot → fresh setlist list. DEVICE WRITE (renameSetlist).
 * `expectName` (the current name) and `seen` (the setlist list the UI saw) guard the
 * slot: setlist slots shift on delete. */
export const renameSetlist = (
  slot: number,
  expectName: string,
  seen: readonly Named[],
  name: string,
): Promise<SetlistRecord[]> =>
  invokeCommand("rename_setlist", {
    slot,
    expectName,
    expectList: names(seen),
    name,
  });

/** Delete a setlist by slot → fresh setlist list. DEVICE WRITE (removeSetlist);
 * the songs themselves are kept. `expectName` and `seen` (the setlist list the UI
 * saw) guard the slot. DESTRUCTIVE. */
export const removeSetlist = (
  slot: number,
  expectName: string,
  seen: readonly Named[],
): Promise<SetlistRecord[]> =>
  invokeCommand("remove_setlist", {
    slot,
    expectName,
    expectList: names(seen),
  });

/** Remove a song from a setlist by its 1-based POSITION within the setlist (NOT the
 * global song slot) → fresh ordered member slots. DEVICE WRITE (removeSetlistSong).
 * Guarded: the setlist must still be `expectName` in the setlist list `seen` and
 * the position must still hold `expectSongSlot`. */
export const removeSetlistSong = (
  setlistSlot: number,
  expectName: string,
  seen: readonly Named[],
  setlistSongSlot: number,
  expectSongSlot: number,
): Promise<number[]> =>
  invokeCommand("remove_setlist_song", {
    setlistSlot,
    expectName,
    expectList: names(seen),
    setlistSongSlot,
    expectSongSlot,
  });

/** Reorder a song within a setlist by 1-based POSITIONS (array-splice semantics) →
 * fresh ordered member slots. DEVICE WRITE (moveSetlistSong). Guarded: the setlist
 * must still be `expectName` in the setlist list `seen` and `oldPos` must still
 * hold `expectSongSlot`. */
export const moveSetlistSong = (
  setlistSlot: number,
  expectName: string,
  seen: readonly Named[],
  oldPos: number,
  newPos: number,
  expectSongSlot: number,
): Promise<number[]> =>
  invokeCommand("move_setlist_song", {
    setlistSlot,
    expectName,
    expectList: names(seen),
    oldPos,
    newPos,
    expectSongSlot,
  });

/** Batched song create: name + optional notes / BPM / setlist membership as ONE
 * device transaction (one seize bookend, one final read — vs one of each PER
 * FIELD on the granular commands). DEVICE WRITE. `addToSetlist` names the setlist
 * by slot AND name; the name and `setlists` (the setlist list the UI saw) guard the
 * slot before the song is created. */
export const createSongFull = (
  name: string,
  notes: string | null,
  bpm: number | null,
  addToSetlist: SetlistRecord | null,
  setlists: readonly Named[],
): Promise<SongSaveOutcome> =>
  invokeCommand("create_song_full", {
    name,
    notes,
    bpm,
    addToSetlist: addToSetlist?.slot ?? null,
    expectSetlistName: addToSetlist?.name ?? null,
    expectSetlistList: addToSetlist ? names(setlists) : null,
  });

/** Batched song update: only the CHANGED fields (null = unchanged) as ONE device
 * transaction. Callers skip the call entirely when nothing changed. DEVICE WRITE.
 * `expectName` (the song's name before the edit) and `seen` (the song list the UI
 * saw) guard the slot. */
export const updateSongFull = (
  slot: number,
  expectName: string,
  seen: readonly Named[],
  name: string | null,
  notes: string | null,
  bpm: number | null,
): Promise<SongSaveOutcome> =>
  invokeCommand("update_song_full", {
    slot,
    expectName,
    expectList: names(seen),
    name,
    notes,
    bpm,
  });

/** Add several songs to a setlist under ONE bookend with ONE final membership read
 * (vs one of each per song) → fresh ordered member slots. DEVICE WRITE
 * (addSetlistSong ×N). The setlist and every song travel with their names, and the
 * setlist and song lists the UI saw; together they guard the slots before any
 * write. */
export const addSetlistSongs = (
  setlist: SetlistRecord,
  setlists: readonly Named[],
  songs: SongRecord[],
  allSongs: readonly Named[],
): Promise<number[]> =>
  invokeCommand("add_setlist_songs", {
    setlistSlot: setlist.slot,
    expectName: setlist.name,
    expectList: names(setlists),
    songSlots: songs.map((s) => s.slot),
    expectSongNames: songs.map((s) => s.name),
    expectSongList: names(allSongs),
  });
