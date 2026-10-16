// src/device/libraryScan.ts — module-scoped controller for the ONE-SHOT
// background scene read (the whole library via one ~22 s device backup).
//
// It lives at MODULE scope (not in a component) so it runs exactly ONCE per device
// connection and SURVIVES LevelView unmount/remount — switching tabs must never
// re-trigger the scan. Reset only on disconnect, so the next connection scans fresh.
//
// Consumers subscribe via useSyncExternalStore (see usePresetData); the device call
// + progress listen are owned here, idempotently. It lives under src/device/ (not a
// view folder) because Level, Doctor, Copy, Songs and the bug report all read it.
//
// Two stores, on purpose: the scan DATA (`subscribeLibraryScan`/`getLibraryScan`) and
// the transfer PERCENT (`useLibraryScanPercent`). The backup emits dozens of progress
// ticks, and a data subscriber re-rendering on each was wasted work, so only the few
// progress readouts subscribe to the percent.
//
// A failed scan is not an empty library: it settles `ready` (so nothing waits on it
// forever) with `error` set, and `retryLibraryScan` runs it again. `error` is the
// failure's text for the log and a bug report only; the app-wide notice never shows it.

import { useSyncExternalStore } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";

import { logUnlessCommandError, readLibraryViaBackup } from "../lib/invoke";
import { errMsg } from "../lib/format";
import { onLive } from "../lib/liveEvents";
import type {
  ActiveGraph,
  AmpCandidate,
  FootswitchInfo,
  FsSceneContext,
  NodeOverride,
  SceneDrive,
  SceneHandleCandidate,
  SceneHandleRow,
  SceneInfo,
  SilenceHint,
  SongRecord,
  SetlistRecord,
} from "../lib/types";

export interface LibraryScan {
  scanning: boolean;
  /** Settled — success OR failure. Releases the caret + the Level gate. */
  ready: boolean;
  /** Why the last scan failed, or null. Set only once `ready`: a failed scan leaves
   *  every map empty, and this is what tells that apart from an empty library. */
  error: string | null;
  /** Per-preset scenes keyed by 0-based LIST INDEX (device slot − 1). */
  sceneInfo: Map<number, SceneInfo[]>;
  /** Per-preset amp `outputLevel` candidates keyed by 0-based LIST INDEX — read
   *  from the same backup, so per-scene leveling skips live block discovery. */
  ampCandidates: Map<number, AmpCandidate[]>;
  /** Per-preset LEVELABLE block-acting footswitches (those with ≥1 level candidate)
   *  keyed by 0-based LIST INDEX — read from the SAME backup, so the list shows the
   *  footswitch count + the Level flow levels them with no extra device read.
   *  Doctor's own SELECT list is built off this filtered map too (a non-levelable FS
   *  row has nothing to diagnose as its own sound — see `DoctorView.tsx`'s
   *  `handleCheck`). The Level list itself now needs the FULL roster instead (BUG 1 —
   *  a switch with no level candidate must still show up, disabled with a reason, not
   *  vanish): `usePresetData`'s `footswitchRoster: "all"` option (Level opts in;
   *  Doctor does not) swaps its OWN `footswitchInfo` for `allFootswitchesByIndex`
   *  below rather than this field being widened — widening THIS field silently
   *  changed what Doctor offers for diagnosis once (caught in review), so don't
   *  repeat that: this stays the levelable-only source of truth. */
  footswitchesPerIndex: Map<number, FootswitchInfo[]>;
  /** Every per-preset block-acting footswitch — INCLUDING one with zero level
   *  candidates (e.g. an Other-class-only param, or a plain on/off toggle) — keyed by
   *  0-based LIST INDEX, from the SAME backup. Doctor's offline force-bypass isolation
   *  derivation needs every switch that can toggle a block, not just the levelable
   *  subset; `footswitchesPerIndex` hid an Other-class-only switch from it entirely.
   *  Also the source `usePresetData`'s `footswitchRoster: "all"` option threads to
   *  Level, so the Level list can show a non-levelable switch disabled (BUG 1). */
  allFootswitchesByIndex: Map<number, FootswitchInfo[]>;
  /** Per-preset block roster (exact node FenderIds) keyed by 0-based LIST INDEX —
   *  read from the same backup. Drives the per-preset CPU total. */
  blocksByIndex: Map<number, string[]>;
  /** Per-preset routed signal graph keyed by 0-based LIST INDEX — read from the same
   *  backup. Drives the Copy feature's per-preset signal-path render. */
  graphByIndex: Map<number, ActiveGraph>;
  /** Per-preset silence hint keyed by 0-based LIST INDEX — only flagged presets get an
   *  entry. Refines the Level flow's offbranch ("not on USB 1/2") row status. */
  silenceHintByIndex: Map<number, SilenceHint>;
  /** Per-preset scene leveling-handle candidates keyed by 0-based LIST INDEX — read from
   *  the SAME backup, so the Set-up step's scene control picker (`useSceneHandles`)
   *  resolves instantly instead of firing `list_scene_level_handles` on first open. Every
   *  scanned preset gets an entry (possibly `[]` — a scene-less or unparseable preset);
   *  `useSceneHandles` falls back to the device command only when this MAP has no entry
   *  for the slot at all (an empty array here is a legitimate answer, not "no data"). */
  sceneHandlesByIndex: Map<number, SceneHandleRow[]>;
  /** Per-preset BASE leveling-handle candidates (guitar-only) keyed by 0-based LIST INDEX
   *  — same backup sourcing as `sceneHandlesByIndex`, for `useLevelBlocks`'s instant-first
   *  path. Same discriminator as `sceneHandlesByIndex` too: every scanned preset gets an
   *  entry (possibly `[]`); `useLevelBlocks` falls back to the device command only when
   *  this MAP has no entry for the slot at all. */
  baseHandlesByIndex: Map<number, SceneHandleCandidate[]>;
  /** Per-preset scene overlays as Doctor node overrides (indexed by scene wire index)
   *  keyed by 0-based LIST INDEX — what a Doctor scene sound is diagnosed on. */
  sceneOverridesByIndex: Map<number, NodeOverride[][]>;
  /** Per-preset footswitch scene contexts keyed by 0-based LIST INDEX — same backup
   *  sourcing and map-presence discriminator as `sceneHandlesByIndex`, for
   *  `useFootswitchSceneContexts`'s instant-first path. */
  fsSceneContextsByIndex: Map<number, FsSceneContext[]>;
  /** Per-preset, per-scene "what a run drives" keyed by 0-based LIST INDEX — the amp(s)
   *  whose output level a scene row's DEFAULT control turns, or why it turns none
   *  (`BackupPresetRow.scene_drives`, from the run's own planner), so Set up's Block shows
   *  exactly what will be used with no device read. Every scanned preset gets an entry
   *  (`[]` for a scene-less or unparseable one). */
  sceneDrivesByIndex: Map<number, SceneDrive[]>;
  /** The 0-based LIST INDICES of the presets the rebalance flow ("Balance its two amps
   *  first") can act on: a scene drives two or more lane amps that re-merge into one
   *  output (`BackupPresetRow.rebalanceable`). */
  rebalanceableIndices: Set<number>;
  /** The unit's presets — `{ slot: 0-based LIST INDEX, name }` — for the Songs tab's
   *  Presets axis rail. Read from the same backup, so the axis needs no device call. */
  presets: { slot: number; name: string }[];
  /** Song slot → the preset LIST INDICES that song uses (`SongPresets` table), for the
   *  Songs tab's read-only "songs per preset" view. The song slot is the live song
   *  list's 1-based positional slot. Read from the same backup. `null` = UNKNOWN: a
   *  song add/remove shifted every song slot since the scan, so the join can't be
   *  trusted until a reconnect rescans (`invalidateSongPresetJoin`). */
  songPresetSlots: Map<number, number[]> | null;
  /** Full song list (`Songs` table) — for the Songs tab's INSTANT first paint in steady
   *  state. Live reads stay the read-back-after-write source for CRUD, and SongsView
   *  writes its post-edit state back here (`patchLibrarySongs`), so a tab switch
   *  repaints what the unit holds now, not the startup scan. */
  songs: SongRecord[];
  /** Full setlist list (`Setlists` table) — instant first paint. Patched like `songs`. */
  setlists: SetlistRecord[];
  /** Setlist slot → global song slots by device position (`SetlistSongs`; index `i` is
   *  position `i + 1`, a gap is `0`) — instant rail counts. Every scanned setlist gets
   *  an entry, an empty one `[]`. Patched like `songs`; a missing entry = not known. */
  setlistSongs: Map<number, number[]>;
}

const emptyScan = (): LibraryScan => ({
  scanning: false,
  ready: false,
  error: null,
  sceneInfo: new Map(),
  ampCandidates: new Map(),
  footswitchesPerIndex: new Map(),
  allFootswitchesByIndex: new Map(),
  blocksByIndex: new Map(),
  graphByIndex: new Map(),
  silenceHintByIndex: new Map(),
  sceneHandlesByIndex: new Map(),
  baseHandlesByIndex: new Map(),
  sceneOverridesByIndex: new Map(),
  fsSceneContextsByIndex: new Map(),
  sceneDrivesByIndex: new Map(),
  rebalanceableIndices: new Set(),
  presets: [],
  songPresetSlots: new Map(),
  songs: [],
  setlists: [],
  setlistSongs: new Map(),
});

let state: LibraryScan = emptyScan();
let inFlight = false;
// Bumped by resetLibraryScan (a device detach). An in-flight scan captures this at entry
// and abandons — never settles `ready`, never touches the listener — if it changed by the
// time an await resolves, so a detach mid-scan can't resurrect stale state or double-free
// the progress listener (the finally used to crash on a nulled unlistenProgress()).
let generation = 0;
let unlistenProgress: UnlistenFn | null = null;
// Songs-tab edits made BEFORE the scan settles (the Songs tab fell back to a live read
// and the user edited). The scan's backup may predate them, so they are applied on top
// of its result when it settles instead of being dropped. Cleared on reset (detach).
type SongsSeed = Pick<LibraryScan, "songs" | "setlists" | "setlistSongs">;
let pendingSongs: SongsSeed | null = null;
let pendingJoinUnknown = false;
const subs = new Set<() => void>();

function set(next: Partial<LibraryScan>): void {
  state = { ...state, ...next }; // new ref so useSyncExternalStore re-renders
  subs.forEach((f) => {
    f();
  });
}

// ── The transfer-percent store (exact, from `tmp://backup-progress`) ──
let percent = 0;
const percentSubs = new Set<() => void>();

function setPercent(next: number): void {
  if (next === percent) return;
  percent = next;
  percentSubs.forEach((f) => {
    f();
  });
}

export function subscribeLibraryScanPercent(cb: () => void): () => void {
  percentSubs.add(cb);
  return () => {
    percentSubs.delete(cb);
  };
}

export function getLibraryScanPercent(): number {
  return percent;
}

/** The backup's determinate transfer %, for progress readouts only. */
export function useLibraryScanPercent(): number {
  return useSyncExternalStore(
    subscribeLibraryScanPercent,
    getLibraryScanPercent,
  );
}

export function subscribeLibraryScan(cb: () => void): () => void {
  subs.add(cb);
  return () => {
    subs.delete(cb);
  };
}

export function getLibraryScan(): LibraryScan {
  return state;
}

/** Start the one-shot backup scene read. Idempotent: a call while a scan is in
 *  flight, or after one has succeeded for this connection, is a no-op (so a tab
 *  switch back to Presets never re-scans). After a FAILED scan it runs again. */
export async function ensureLibraryScan(): Promise<void> {
  if (inFlight || (state.ready && state.error == null)) return;
  inFlight = true;
  const gen = generation;
  set({ ...emptyScan(), scanning: true });
  setPercent(0);
  let failure: string | null = null;
  try {
    // Progress is cosmetic: a listen that fails leaves the bar at 0 % and the scan
    // runs anyway. Inside the try so it can never strand `inFlight`.
    try {
      const unlisten = await onLive("backupProgress", (p) => {
        if (gen === generation) setPercent(p.percent);
      });
      // A reset (detach) during the listen await → clean up this listener and bail
      // before publishing it or touching state.
      if (gen !== generation) {
        unlisten();
        return;
      }
      unlistenProgress = unlisten;
    } catch {
      // No progress events this scan; the read below still runs.
    }
    if (gen !== generation) return; // reset during the listen → no stale read
    const res = await readLibraryViaBackup();
    if (gen !== generation) return; // reset mid-scan → drop the stale results
    const m = new Map<number, SceneInfo[]>();
    const amps = new Map<number, AmpCandidate[]>();
    const fsw = new Map<number, FootswitchInfo[]>();
    const allFsw = new Map<number, FootswitchInfo[]>();
    const blocks = new Map<number, string[]>();
    const graphs = new Map<number, ActiveGraph>();
    const silence = new Map<number, SilenceHint>();
    const sceneHandles = new Map<number, SceneHandleRow[]>();
    const baseHandles = new Map<number, SceneHandleCandidate[]>();
    const sceneOverrides = new Map<number, NodeOverride[][]>();
    const fsContexts = new Map<number, FsSceneContext[]>();
    const sceneDrives = new Map<number, SceneDrive[]>();
    const rebalanceable = new Set<number>();
    // backup slot is the 1-based device slot; the list is 0-based → −1.
    const presets: { slot: number; name: string }[] = [];
    res.presets.forEach((p) => {
      m.set(p.slot - 1, p.scenes);
      amps.set(p.slot - 1, p.amp_candidates);
      // Doctor's SELECT list is built off `footswitchesPerIndex` too (see its own
      // doc) — keep it levelable-only here; `allFootswitchesByIndex` carries the full
      // roster for Doctor's damage detector AND (via `usePresetData`'s
      // `footswitchRoster: "all"` option) for Level's own list (BUG 1).
      if (p.footswitches.length > 0) allFsw.set(p.slot - 1, p.footswitches);
      const levelable = p.footswitches.filter((f) => f.level_params.length > 0);
      if (levelable.length > 0) fsw.set(p.slot - 1, levelable);
      blocks.set(
        p.slot - 1,
        p.blocks.map((b) => b.fender_id),
      );
      graphs.set(p.slot - 1, p.graph);
      if (p.silence_hint != null) silence.set(p.slot - 1, p.silence_hint);
      sceneHandles.set(p.slot - 1, p.scene_handles);
      baseHandles.set(p.slot - 1, p.base_handles);
      sceneOverrides.set(p.slot - 1, p.scene_overrides);
      fsContexts.set(p.slot - 1, p.fs_scene_contexts);
      sceneDrives.set(p.slot - 1, p.scene_drives);
      if (p.rebalanceable) rebalanceable.add(p.slot - 1);
      presets.push({ slot: p.slot - 1, name: p.name });
    });
    // Song slot → preset LIST INDICES (preset device slot − 1), deduped per song.
    const songPresetSlots = new Map<number, number[]>();
    res.song_presets.forEach((b) => {
      const idx = b.preset_slot - 1;
      const cur = songPresetSlots.get(b.song_slot) ?? [];
      if (!cur.includes(idx)) cur.push(idx);
      songPresetSlots.set(b.song_slot, cur);
    });
    // Songs/setlists for the Songs tab's instant first paint — the backup already
    // emits the live read types (SongRecord/SetlistRecord), so no conversion.
    const songs = res.songs;
    const setlists = res.setlists;
    // setlist slot → song slots BY POSITION, the shape the live read returns: the
    // membership edits address a song by position, so a gap stays a `0` rather than
    // shifting every later song onto the wrong position. Every setlist gets an entry —
    // an empty setlist is a known `[]`, not "unread".
    const setlistSongs = new Map<number, number[]>(
      setlists.map((l) => [l.slot, []]),
    );
    res.setlist_songs.forEach((r) => {
      const cur = setlistSongs.get(r.setlist_slot) ?? [];
      while (cur.length < r.position - 1) cur.push(0);
      cur[r.position - 1] = r.song_slot;
      setlistSongs.set(r.setlist_slot, cur);
    });
    set({
      sceneInfo: m,
      ampCandidates: amps,
      footswitchesPerIndex: fsw,
      allFootswitchesByIndex: allFsw,
      blocksByIndex: blocks,
      graphByIndex: graphs,
      silenceHintByIndex: silence,
      sceneHandlesByIndex: sceneHandles,
      baseHandlesByIndex: baseHandles,
      sceneOverridesByIndex: sceneOverrides,
      fsSceneContextsByIndex: fsContexts,
      sceneDrivesByIndex: sceneDrives,
      rebalanceableIndices: rebalanceable,
      presets,
      songPresetSlots: pendingJoinUnknown ? null : songPresetSlots,
      ...(pendingSongs ?? { songs, setlists, setlistSongs }),
    });
    pendingSongs = null;
    pendingJoinUnknown = false;
  } catch (e) {
    // Scene details stay unknown and whole-preset ticks degrade to Base-only, but
    // say so: `error` keeps this from reading as an empty library. The screen says it
    // in plain words only, so the text must reach the log: a backup-read rejection is
    // already there (the central invoke-failure logger), a throw from the processing
    // above is logged here.
    logUnlessCommandError("library scan", e);
    failure = errMsg(e);
  } finally {
    // Only the CURRENT scan settles. A reset (detach) already cleared state + the
    // listener; settling `ready` here would strand a stale scan and double-free the
    // listener (the old bug: unlistenProgress() on a nulled ref threw).
    if (gen === generation) {
      if (unlistenProgress) {
        unlistenProgress();
        unlistenProgress = null;
      }
      inFlight = false;
      if (failure == null) setPercent(100);
      set({ scanning: false, ready: true, error: failure });
    }
  }
}

/** Run the scan again after it failed (the consumers' Retry). A no-op while one is
 *  in flight or once one has succeeded. */
export function retryLibraryScan(): void {
  void ensureLibraryScan();
}

/** Patch ONE preset's cached graph in place (0-based list index) — used after a Copy
 *  save writes new blocks to a slot, so a second edit reads the just-saved path without
 *  the ~22 s re-scan. No-op until the backup has settled (`ready`); the next full scan
 *  would otherwise overwrite it anyway. */
export function patchLibraryGraph(listIndex: number, graph: ActiveGraph): void {
  if (!state.ready) return;
  const next = new Map(state.graphByIndex);
  next.set(listIndex, graph);
  // Scene overrides are kept: a Copy save edits blocks, not scene overlays, and
  // Doctor's `effective_nodes` ignores an override whose node the save removed.
  set({ graphByIndex: next });
}

/** Mark the song↔preset join UNKNOWN after a slot-shifting song CRUD: add/remove bumps
 *  every song slot, so `songPresetSlots` (keyed by song slot) would join the Presets axis
 *  against WRONG bindings. `null` renders as "unknown, reconnect to refresh", never as a
 *  confident zero. The preset list itself is unaffected, so `presets`/`ready` are kept.
 *  ponytail: a precise old→new slot remap is the upgrade path if the Presets axis needs
 *  to survive edits without a reconnect re-scan. */
export function invalidateSongPresetJoin(): void {
  // Also held after a FAILED scan, so a retry's result can't drop it.
  if (!state.ready || state.error != null) pendingJoinUnknown = true;
  if (!state.ready) return; // applied when the scan settles
  set({ songPresetSlots: null });
}

/** Write the Songs tab's post-edit lists back into the snapshot, so a tab switch (which
 *  unmounts the view) repaints what the unit holds now instead of the startup scan.
 *  Before the scan settles (or after it failed, ahead of a retry) the lists are held
 *  and applied over its result, since its backup may predate the edit. */
export function patchLibrarySongs(next: SongsSeed): void {
  // Also held after a FAILED scan, so a retry's (older) backup can't drop it.
  if (!state.ready || state.error != null) pendingSongs = next;
  if (!state.ready) return;
  set(next);
}

/** Drop the Songs-tab seed (songs, setlists, membership) so the next Songs mount reads
 *  them live: a Songs write that finished after the view unmounted changed the unit,
 *  but there was no view left to patch the snapshot with its reply. */
export function forgetLibrarySongs(): void {
  const empty: SongsSeed = { songs: [], setlists: [], setlistSongs: new Map() };
  if (!state.ready || state.error != null) pendingSongs = empty;
  if (!state.ready) return;
  set(empty);
}

/** Reset on disconnect so the NEXT connection scans fresh. */
export function resetLibraryScan(): void {
  generation++; // invalidate any in-flight scan so its continuation can't resurrect state
  if (unlistenProgress) {
    unlistenProgress();
    unlistenProgress = null;
  }
  inFlight = false;
  pendingSongs = null;
  pendingJoinUnknown = false;
  setPercent(0);
  set(emptyScan());
}
