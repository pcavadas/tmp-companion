// src/__tests__/libraryScan.test.ts — the song↔preset data the Songs tab's Presets
// axis reads off the ONE startup backup scan (no extra device read). Asserts the
// module store derives a preset list (0-based list index + name) and a song→preset
// map from a single `read_library_via_backup` payload.

import { describe, it, expect, beforeEach, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

import type { ActiveGraph, BackupReadResult } from "../lib/types";
import {
  ensureLibraryScan,
  getLibraryScan,
  invalidateLibrarySongs,
  resetLibraryScan,
} from "../views/level/libraryScan";

vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(() => undefined),
}));

const emptyGraph: ActiveGraph = {
  name: null,
  slot: null,
  template: null,
  split_mix: null,
  nodes: [],
  stages: [],
};

const row = (slot: number, name: string) => ({
  slot,
  name,
  scene_count: 0,
  scenes: [],
  amp_candidates: [],
  blocks: [],
  graph: emptyGraph,
  footswitches: [],
});

// device slots 8 / 58 → list indices 7 / 57; three song→preset bindings.
const BACKUP: BackupReadResult = {
  members: [],
  db_bytes: 0,
  total_rows: 2,
  scene_mode: "test",
  presets: [row(8, "Plexi Crunch"), row(58, "Stadium Lead")],
  song_presets: [
    { song_slot: 1, preset_slot: 8 },
    { song_slot: 1, preset_slot: 58 },
    { song_slot: 2, preset_slot: 58 },
  ],
  songs: [],
  setlists: [],
  setlist_songs: [],
};

describe("libraryScan — songs↔presets axis data", () => {
  beforeEach(() => {
    resetLibraryScan();
    vi.mocked(invoke).mockReset();
  });

  it("derives the preset list + song→preset map from one backup", async () => {
    vi.mocked(invoke).mockImplementation((cmd: string) =>
      cmd === "read_library_via_backup"
        ? Promise.resolve(BACKUP)
        : Promise.resolve(null),
    );

    await ensureLibraryScan();
    const lib = getLibraryScan();

    expect(lib.presets).toEqual([
      { slot: 7, name: "Plexi Crunch" },
      { slot: 57, name: "Stadium Lead" },
    ]);
    expect(lib.songPresetSlots.get(1)).toEqual([7, 57]);
    expect(lib.songPresetSlots.get(2)).toEqual([57]);
  });

  it("derives songs + setlists + ordered membership for the Songs tab first paint", async () => {
    const backup: BackupReadResult = {
      ...BACKUP,
      songs: [
        { slot: 1, name: "Intro", notes: "n1", bpm_active: true, bpm: 120 },
        { slot: 2, name: "Outro", notes: "", bpm_active: false, bpm: 0 },
      ],
      setlists: [{ slot: 1, name: "Main set" }],
      // out of position order on purpose — backend ORDER BY position, so the store
      // must preserve the arriving order (3 before 7).
      setlist_songs: [
        { setlist_slot: 1, song_slot: 2, position: 1 },
        { setlist_slot: 1, song_slot: 1, position: 2 },
      ],
    };
    vi.mocked(invoke).mockImplementation((cmd: string) =>
      cmd === "read_library_via_backup"
        ? Promise.resolve(backup)
        : Promise.resolve(null),
    );

    await ensureLibraryScan();
    const lib = getLibraryScan();

    expect(lib.songs).toEqual([
      { slot: 1, name: "Intro", notes: "n1", bpm: 120, bpm_active: true },
      { slot: 2, name: "Outro", notes: "", bpm: 0, bpm_active: false },
    ]);
    expect(lib.setlists).toEqual([{ slot: 1, name: "Main set" }]);
    expect(lib.setlistSongs.get(1)).toEqual([2, 1]);
  });

  it("invalidates the stale song↔preset join after a slot-shifting CRUD, keeps presets", async () => {
    vi.mocked(invoke).mockImplementation((cmd: string) =>
      cmd === "read_library_via_backup"
        ? Promise.resolve(BACKUP)
        : Promise.resolve(null),
    );
    await ensureLibraryScan();

    invalidateLibrarySongs();
    const lib = getLibraryScan();

    // The preset list is unaffected by song edits — kept.
    expect(lib.presets).toEqual([
      { slot: 7, name: "Plexi Crunch" },
      { slot: 57, name: "Stadium Lead" },
    ]);
    // The slot-keyed song→preset map is cleared (would otherwise join wrong slots).
    expect(lib.songPresetSlots.size).toBe(0);
    expect(lib.songs).toEqual([]);
  });

  it("caches only LEVELABLE footswitches per index (filters empty level_params)", async () => {
    const levelable = {
      switch: 4,
      label: "Solo",
      link_group: null,
      functions: [],
      level_params: [
        {
          group_id: "G1",
          node_id: "N4",
          fender_id: "ACD_BluesDriver",
          parameter_id: "gain",
          current: 0.5,
        },
      ],
    };
    const bare = {
      switch: 5,
      label: "Tuner",
      link_group: null,
      functions: [],
      level_params: [], // not levelable → dropped
    };
    const backup: BackupReadResult = {
      ...BACKUP,
      presets: [
        { ...row(8, "Plexi Crunch"), footswitches: [levelable, bare] },
        row(58, "Stadium Lead"),
      ],
    };
    vi.mocked(invoke).mockImplementation((cmd: string) =>
      cmd === "read_library_via_backup"
        ? Promise.resolve(backup)
        : Promise.resolve(null),
    );

    await ensureLibraryScan();
    const lib = getLibraryScan();

    // slot 8 → list index 7: only the levelable footswitch survives the filter.
    expect(lib.footswitchesPerIndex.get(7)?.map((f) => f.label)).toEqual([
      "Solo",
    ]);
    // slot 58 → index 57: no footswitches → no map entry at all.
    expect(lib.footswitchesPerIndex.has(57)).toBe(false);
  });
});
