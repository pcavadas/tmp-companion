// src/views/level/libraryScan.ts — module-scoped controller for the ONE-SHOT
// background scene read (the whole library via one ~22 s device backup).
//
// It lives at MODULE scope (not in a component) so it runs exactly ONCE per device
// connection and SURVIVES LevelView unmount/remount — switching tabs must never
// re-trigger the scan. Reset only on disconnect, so the next connection scans fresh.
//
// Consumers subscribe via useSyncExternalStore (see usePresetData); the device call
// + progress listen are owned here, idempotently.

import type { UnlistenFn } from "@tauri-apps/api/event";

import { readLibraryViaBackup } from "../../lib/invoke";
import { onBackupProgress } from "../../lib/liveEvents";
import type {
  ActiveGraph,
  AmpCandidate,
  FootswitchInfo,
  SceneInfo,
  SongRecord,
  SetlistRecord,
} from "../../lib/types";

export interface LibraryScan {
  scanning: boolean;
  /** Determinate transfer % (exact, from `tmp://backup-progress`). */
  percent: number;
  /** Settled — success OR failure. Releases the caret + the Level gate. */
  ready: boolean;
  /** Per-preset scenes keyed by 0-based LIST INDEX (device slot − 1). */
  sceneInfo: Map<number, SceneInfo[]>;
  /** Per-preset amp `outputLevel` candidates keyed by 0-based LIST INDEX — read
   *  from the same backup, so per-scene leveling skips live block discovery. */
  ampCandidates: Map<number, AmpCandidate[]>;
  /** Per-preset LEVELABLE block-acting footswitches (those with ≥1 level candidate)
   *  keyed by 0-based LIST INDEX — read from the SAME backup, so the list shows the
   *  footswitch count + the flow levels them with no extra device read. */
  footswitchesPerIndex: Map<number, FootswitchInfo[]>;
  /** Per-preset block roster (exact node FenderIds) keyed by 0-based LIST INDEX —
   *  read from the same backup. Drives the per-preset CPU total. */
  blocksByIndex: Map<number, string[]>;
  /** Per-preset routed signal graph keyed by 0-based LIST INDEX — read from the same
   *  backup. Drives the Copy feature's per-preset signal-path render. */
  graphByIndex: Map<number, ActiveGraph>;
  /** The unit's presets — `{ slot: 0-based LIST INDEX, name }` — for the Songs tab's
   *  Presets axis rail. Read from the same backup, so the axis needs no device call. */
  presets: { slot: number; name: string }[];
  /** Song slot → the preset LIST INDICES that song uses (`SongPresets` table), for the
   *  Songs tab's read-only "songs per preset" view. The song slot is the live song
   *  list's 1-based positional slot. Read from the same backup. */
  songPresetSlots: Map<number, number[]>;
  /** Full song list (`Songs` table) — for the Songs tab's INSTANT first paint in steady
   *  state. After first paint SongsView holds its own state; live reads stay the
   *  read-back-after-write source for CRUD. */
  songs: SongRecord[];
  /** Full setlist list (`Setlists` table) — instant first paint. */
  setlists: SetlistRecord[];
  /** Setlist slot → ordered global song slots (`SetlistSongs`) — instant rail counts. */
  setlistSongs: Map<number, number[]>;
}

const emptyScan = (): LibraryScan => ({
  scanning: false,
  percent: 0,
  ready: false,
  sceneInfo: new Map(),
  ampCandidates: new Map(),
  footswitchesPerIndex: new Map(),
  blocksByIndex: new Map(),
  graphByIndex: new Map(),
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
const subs = new Set<() => void>();

function set(next: Partial<LibraryScan>): void {
  state = { ...state, ...next }; // new ref so useSyncExternalStore re-renders
  subs.forEach((f) => {
    f();
  });
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
 *  flight, or after one has completed for this connection, is a no-op (so a tab
 *  switch back to Presets never re-scans). */
export async function ensureLibraryScan(): Promise<void> {
  if (inFlight || state.ready) return;
  inFlight = true;
  const gen = generation;
  set({ ...emptyScan(), scanning: true });
  const unlisten = await onBackupProgress((p) => {
    if (gen === generation) set({ percent: p.percent });
  });
  // A reset (detach) during the listen await → clean up this listener and bail before
  // publishing it or touching state.
  if (gen !== generation) {
    unlisten();
    return;
  }
  unlistenProgress = unlisten;
  try {
    const res = await readLibraryViaBackup();
    if (gen !== generation) return; // reset mid-scan → drop the stale results
    const m = new Map<number, SceneInfo[]>();
    const amps = new Map<number, AmpCandidate[]>();
    const fsw = new Map<number, FootswitchInfo[]>();
    const blocks = new Map<number, string[]>();
    const graphs = new Map<number, ActiveGraph>();
    // backup slot is the 1-based device slot; the list is 0-based → −1.
    const presets: { slot: number; name: string }[] = [];
    res.presets.forEach((p) => {
      m.set(p.slot - 1, p.scenes);
      amps.set(p.slot - 1, p.amp_candidates);
      // Only LEVELABLE footswitches (≥1 level candidate) become rows — one without a
      // candidate has nothing to solve and would always skip.
      const levelable = p.footswitches.filter((f) => f.level_params.length > 0);
      if (levelable.length > 0) fsw.set(p.slot - 1, levelable);
      blocks.set(
        p.slot - 1,
        p.blocks.map((b) => b.fender_id),
      );
      graphs.set(p.slot - 1, p.graph);
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
    // setlist slot → ordered song slots (backend already ORDER BY setlist, position).
    const setlistSongs = new Map<number, number[]>();
    res.setlist_songs.forEach((r) => {
      const cur = setlistSongs.get(r.setlist_slot) ?? [];
      cur.push(r.song_slot);
      setlistSongs.set(r.setlist_slot, cur);
    });
    set({
      sceneInfo: m,
      ampCandidates: amps,
      footswitchesPerIndex: fsw,
      blocksByIndex: blocks,
      graphByIndex: graphs,
      presets,
      songPresetSlots,
      songs,
      setlists,
      setlistSongs,
    });
  } catch (e) {
    // Non-fatal: scene details stay unknown; whole-preset ticks degrade to Base-only.
    console.warn("library backup scene read failed:", e);
  } finally {
    // Only the CURRENT scan settles. A reset (detach) already cleared state + the
    // listener; settling `ready` here would strand a stale scan and double-free the
    // listener (the old bug: unlistenProgress() on a nulled ref threw).
    if (gen === generation) {
      // Provably non-null here: reset (the only nuller) bumps `generation`, so
      // gen === generation means it hasn't run since we published the listener.
      unlistenProgress();
      unlistenProgress = null;
      inFlight = false;
      set({ scanning: false, percent: 100, ready: true });
    }
  }
}

/** Patch ONE preset's cached graph in place (0-based list index) — used after a Copy
 *  save writes new blocks to a slot, so a second edit reads the just-saved path without
 *  the ~22 s re-scan. No-op until the backup has settled (`ready`); the next full scan
 *  would otherwise overwrite it anyway. */
export function patchLibraryGraph(listIndex: number, graph: ActiveGraph): void {
  if (!state.ready) return;
  const next = new Map(state.graphByIndex);
  next.set(listIndex, graph);
  set({ graphByIndex: next });
}

/** Invalidate the backup-sourced SONG data after a slot-shifting song CRUD (add/remove
 *  bumps every song slot, so `songPresetSlots` — keyed by song slot — would join the
 *  Presets axis against WRONG bindings, and the song/setlist seed is now stale). Clears
 *  it to empty (honest "unknown after edit") rather than confidently-wrong; the preset
 *  list itself is unaffected by song edits, so `presets`/`ready` are kept.
 *  ponytail: clear-all; a precise old→new slot remap is the upgrade path if the Presets
 *  axis needs to survive edits without a reconnect re-scan. */
export function invalidateLibrarySongs(): void {
  if (!state.ready) return;
  set({
    songs: [],
    setlists: [],
    setlistSongs: new Map(),
    songPresetSlots: new Map(),
  });
}

/** Reset on disconnect so the NEXT connection scans fresh. */
export function resetLibraryScan(): void {
  generation++; // invalidate any in-flight scan so its continuation can't resurrect state
  if (unlistenProgress) {
    unlistenProgress();
    unlistenProgress = null;
  }
  inFlight = false;
  set(emptyScan());
}
