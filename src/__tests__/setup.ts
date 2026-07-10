// src/__tests__/setup.ts — Vitest global setup (referenced by vitest.config.ts).
//
// 1. jest-dom matchers for RTL.
// 2. A default mock of @tauri-apps/api/core so screen smoke tests don't hit Tauri.
//    `invoke(command, args)` resolves a sensible EMPTY shape per command — enough
//    for a screen to mount + render its empty state without throwing. Individual
//    tests can override with `vi.mocked(invoke).mockResolvedValueOnce(...)`.

import "@testing-library/jest-dom/vitest";
import { vi } from "vitest";

// This jsdom config doesn't provide Web Storage; supply a tiny in-memory shim so
// the gate helpers (gates.ts) and any localStorage reads don't crash under test.
if (typeof globalThis.localStorage === "undefined") {
  const store = new Map<string, string>();
  const mem: Storage = {
    get length() {
      return store.size;
    },
    clear: () => {
      store.clear();
    },
    getItem: (k: string) => store.get(k) ?? null,
    key: (i: number) => [...store.keys()][i] ?? null,
    removeItem: (k: string) => void store.delete(k),
    setItem: (k: string, v: string) => void store.set(k, v),
  };
  Object.defineProperty(globalThis, "localStorage", {
    value: mem,
    configurable: true,
  });
}

class MockChannel {
  onmessage: ((message: unknown) => void) | null = null;
}

// Empty/zero shapes keyed by command name. Anything not listed resolves `null`.
// Shapes mirror each command's return type (their EMPTY / no-data variant).
function emptyResultFor(command: string): unknown {
  switch (command) {
    // connection + app
    case "app_info":
      return { name: "TMP Companion", version: "0.0.0-test" };
    case "connect_device":
      return { firmware: null, graph: null };
    case "list_presets":
      return [];
    // Scenes nested under presets: empty batch (no library imported in tests),
    // so the disclosure tree degrades to no caret / no drawer.
    case "read_preset_scenes":
      return { scenes: [], fs: [] };
    case "read_library_via_backup":
      return {
        members: [],
        db_bytes: 0,
        total_rows: 0,
        scene_mode: "test",
        presets: [],
        song_presets: [],
        songs: [],
        setlists: [],
        setlist_songs: [],
      };
    case "load_scene_on_amp":
      return null;

    // leveling
    case "list_level_blocks":
      return [];
    case "level_scenes":
      return [];
    case "level_setlist":
      return { target_lufs: -14, results: [] };

    // profiles / store
    case "get_store":
      return {
        profiles: [],
        profile_by_slot: {},
        targets: [],
        auto_install_updates: true,
      };
    case "save_profiles":
    case "save_targets":
    case "set_auto_install_updates":
      return null;
    case "calibrate_profile":
      return -20;
    case "list_pickup_topologies":
    case "list_samples":
      return [];

    // library / search / export
    case "import_library":
      return {
        matched: 0,
        unmatched_files: [],
        unmatched_slots: [],
        ambiguous: [],
      };
    case "library_records":
    case "library_filter":
      return [];

    // bulk
    case "bulk_dry_run":
      return [];
    case "bulk_apply":
      return { run_id: "run_test", report: { entries: [] } };
    case "bulk_revert":
    case "list_snapshots":
      return [];

    // rename / variants
    case "bulk_rename":
      return [];
    case "create_variant":
      return "ok";

    // spectral / audition
    case "spectrum_scan":
      return { bands: [], flags: [] };
    case "eq_match":
      return {
        source_bands: [],
        reference_bands: [],
        distance: 0,
        deltas: [],
        matched_bands: [],
      };
    case "rank_candidates":
      return [];
    case "audition_render":
      return "";

    // migration / audit
    case "migration_scan":
    case "audit_loudness":
      return [];
    case "migration_plan":
      return {
        classified: { renamed: [], removed_only: [], added_only: [] },
        plan: [],
      };
    case "migration_apply":
      return [];

    // song-preset assignment
    case "song_assign":
    case "song_clear":
    case "song_move":
    case "song_swap":
      return null;

    // view songs + device-backed song/setlist CRUD — all return lists
    // (SongRecord[] / SetlistRecord[] / member-slot number[]).
    case "list_songs":
    case "read_setlists":
    case "list_setlist_songs":
    case "add_song":
    case "rename_song":
    case "remove_song":
    case "set_song_notes":
    case "set_song_bpm":
    case "add_setlist":
    case "rename_setlist":
    case "remove_setlist":
    case "add_setlist_song":
    case "remove_setlist_song":
    case "move_setlist_song":
      return [];

    default:
      return null;
  }
}

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn((command: string) => Promise.resolve(emptyResultFor(command))),
  Channel: MockChannel,
}));
