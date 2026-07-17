// Tests for the device-backed Songs page (SongsView).
//
// The page is now a live editor of the connected unit: it reads songs + setlists
// on the connected rising edge, and every CRUD action fires a device command and
// re-renders from the returned authoritative list (read-back-after-write). These
// tests assert: connection gating, the load-state-machine (loading→ready, error→
// retry without a hook-order crash), that writes call the right command and apply
// the read-back, that destructive deletes go through a confirm Modal, that setlist
// membership is read on select and addressed BY POSITION (1-based) for remove/
// reorder, and that a failed write surfaces a Toast WITHOUT mutating state.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ThemeProvider } from "../theme/ThemeProvider";
import { SongsView } from "../views/songs";
import type {
  ActiveGraph,
  BackupReadResult,
  SetlistRecord,
  SongRecord,
} from "../lib/types";
import {
  ensureLibraryScan,
  resetLibraryScan,
} from "../views/level/libraryScan";

function renderMgr(connected: boolean) {
  return render(
    <ThemeProvider>
      <SongsView connected={connected} />
    </ThemeProvider>,
  );
}

const connectedTree = (
  <ThemeProvider>
    <SongsView connected={true} />
  </ThemeProvider>
);

const song = (
  slot: number,
  name: string,
  extra: Partial<SongRecord> = {},
): SongRecord => ({
  slot,
  name,
  notes: "",
  bpm: 120,
  bpm_active: false,
  ...extra,
});

interface DeviceState {
  songs?: SongRecord[];
  setlists?: SetlistRecord[];
  members?: Record<number, number[]>;
}

/** Install a persistent invoke implementation: reads return the seeded state,
 *  writes default to an empty array (override per-call with mockImplementationOnce
 *  to simulate a specific read-back). */
function baseMock(state: DeviceState = {}) {
  const songs = state.songs ?? [];
  const setlists = state.setlists ?? [];
  const members = state.members ?? {};
  vi.mocked(invoke).mockImplementation((cmd, args) => {
    switch (cmd) {
      case "list_songs":
        return Promise.resolve(songs);
      case "read_setlists":
        return Promise.resolve(setlists);
      case "list_setlist_songs":
        return Promise.resolve(
          members[
            Number((args as { setlistSlot?: number } | undefined)?.setlistSlot)
          ] ?? [],
        );
      default:
        return Promise.resolve([]);
    }
  });
}

const callsTo = (name: string) =>
  vi.mocked(invoke).mock.calls.filter((c) => c[0] === name).length;
const lastArgsTo = (name: string) =>
  [...vi.mocked(invoke).mock.calls].reverse().find((c) => c[0] === name)?.[1];

/** Resolve the draggable row wrapping a song's label, throwing if absent
 *  (a missing row is a test failure, not a silent non-null assertion). */
function draggableRow(songName: string): Element {
  const row = screen.getByText(songName).closest("div[draggable]");
  if (!row) throw new Error(`no draggable row for "${songName}"`);
  return row;
}

describe("SongsView (device-backed)", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    baseMock();
  });

  it("does not touch the device while disconnected, then loads songs + setlists on the rising edge", async () => {
    const { rerender } = renderMgr(false);
    expect(callsTo("list_songs")).toBe(0);
    expect(callsTo("read_setlists")).toBe(0);

    rerender(connectedTree);
    // refresh reads songs then setlists SEQUENTIALLY (single-connection device),
    // so wait for both rather than assuming they fire together.
    await waitFor(() => {
      expect(callsTo("list_songs")).toBe(1);
    });
    await waitFor(() => {
      expect(callsTo("read_setlists")).toBe(1);
    });
  });

  it("renders the library once ready", async () => {
    baseMock({ songs: [song(1, "Song 1")] });
    renderMgr(true);
    expect(
      await screen.findByRole("button", { name: /new song/i }),
    ).toBeInTheDocument();
    expect(screen.getByText(/1 song on the unit/)).toBeInTheDocument();
    expect(screen.getByText("Song 1")).toBeInTheDocument();
  });

  it("survives the error→ready transition via Try again (guards the hook-order rule)", async () => {
    baseMock();
    vi.mocked(invoke).mockImplementationOnce(() =>
      Promise.reject(new Error("device offline")),
    );
    renderMgr(true);
    const user = userEvent.setup();

    expect(await screen.findByText("device offline")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /try again/i }));
    await waitFor(() =>
      expect(screen.queryByText("device offline")).not.toBeInTheDocument(),
    );
    expect(
      await screen.findByRole("button", { name: /new song/i }),
    ).toBeInTheDocument();
  });

  it("add song calls create_song_full (one batched transaction) and renders the read-back list", async () => {
    baseMock({ songs: [] });
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByRole("button", { name: /new song/i });

    await user.click(screen.getByRole("button", { name: /new song/i }));
    await user.type(screen.getByPlaceholderText("Song name"), "Toto");

    // The create_song_full read-back returns the new song at protocol slot 1.
    vi.mocked(invoke).mockImplementationOnce(() =>
      Promise.resolve({
        songs: [song(1, "Toto")],
        members: null,
        bpm_warning: null,
      }),
    );
    await user.keyboard("{Enter}");

    await waitFor(() => {
      expect(lastArgsTo("create_song_full")).toEqual({
        name: "Toto",
        notes: null,
        bpm: null,
        addToSetlist: null,
      });
    });
    expect(await screen.findByText("Toto")).toBeInTheDocument();
  });

  it("edit song sends ONE update_song_full carrying only the changed fields", async () => {
    baseMock({ songs: [song(2, "Toto")] });
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByText("Toto");

    await user.click(screen.getByTitle("More"));
    await user.click(screen.getByText("Edit song…"));
    const nameInput = screen.getByPlaceholderText("Song name");
    await user.clear(nameInput);
    await user.type(nameInput, "Toto Toto");
    await user.type(screen.getByPlaceholderText("—"), "102");

    vi.mocked(invoke).mockImplementation(() =>
      Promise.resolve({
        songs: [song(2, "Toto Toto", { bpm: 102, bpm_active: true })],
        members: null,
        bpm_warning: null,
      }),
    );
    await user.keyboard("{Enter}");

    await waitFor(() => {
      expect(callsTo("update_song_full")).toBe(1);
    });
    // Notes unchanged → null (the backend writes only the changed fields).
    expect(lastArgsTo("update_song_full")).toEqual({
      slot: 2,
      name: "Toto Toto",
      notes: null,
      bpm: 102,
    });
  });

  it("delete song requires the confirm Modal (Cancel does not write, Delete does)", async () => {
    baseMock({ songs: [song(5, "Doomed")] });
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByText("Doomed");

    await user.click(screen.getByTitle("More"));
    await user.click(screen.getByText("Delete song"));
    // Modal up, nothing written yet.
    expect(
      screen.getByText(/WRITES TO THE TONE MASTER PRO/),
    ).toBeInTheDocument();
    expect(callsTo("remove_song")).toBe(0);

    // Cancel → still no write.
    await user.click(screen.getByRole("button", { name: /cancel/i }));
    expect(callsTo("remove_song")).toBe(0);

    // Re-open and confirm Delete → guarded remove with slot + expectName.
    await user.click(screen.getByTitle("More"));
    await user.click(screen.getByText("Delete song"));
    await user.click(screen.getByRole("button", { name: /^delete$/i }));
    await waitFor(() => {
      expect(lastArgsTo("remove_song")).toEqual({
        slot: 5,
        expectName: "Doomed",
      });
    });
  });

  it("deleting the viewed setlist confirms then falls back to All songs", async () => {
    baseMock({ setlists: [{ slot: 1, name: "Set A" }], members: { 1: [] } });
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByRole("button", { name: /new song/i });

    await user.click(screen.getByText("Set A"));
    await screen.findByText(/No songs in this setlist yet/);

    await user.click(screen.getByTitle("Setlist options"));
    await user.click(screen.getByText("Delete setlist"));
    // remove_setlist returns the fresh (now empty) list; view falls back.
    vi.mocked(invoke).mockImplementationOnce(() => Promise.resolve([]));
    await user.click(screen.getByRole("button", { name: /^delete$/i }));

    await waitFor(() => {
      expect(lastArgsTo("remove_setlist")).toEqual({
        slot: 1,
        expectName: "Set A",
      });
    });
    // View falls back to the library: the deleted setlist is gone from the rail.
    await waitFor(() =>
      expect(screen.queryByText("Set A")).not.toBeInTheDocument(),
    );
    expect(
      screen.getByRole("button", { name: /new song/i }),
    ).toBeInTheDocument();
  });

  it("selecting a setlist reads its membership and resolves member songs", async () => {
    baseMock({
      songs: [song(3, "Song C"), song(5, "Song E")],
      setlists: [{ slot: 1, name: "Set A" }],
      members: { 1: [3, 5] },
    });
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByRole("button", { name: /new song/i });

    await user.click(screen.getByText("Set A"));
    await waitFor(() => {
      expect(lastArgsTo("list_setlist_songs")).toEqual({ setlistSlot: 1 });
    });
    // Member songs render in device order.
    expect(await screen.findByText("Song C")).toBeInTheDocument();
    expect(screen.getByText("Song E")).toBeInTheDocument();
  });

  it("removing a setlist song addresses it by 1-based POSITION", async () => {
    baseMock({
      songs: [song(3, "Song C"), song(5, "Song E")],
      setlists: [{ slot: 1, name: "Set A" }],
      members: { 1: [3, 5] },
    });
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByRole("button", { name: /new song/i });
    await user.click(screen.getByText("Set A"));
    await screen.findByText("Song C");

    await user.click(screen.getAllByTitle("Remove from setlist")[0]);
    await waitFor(() => {
      expect(lastArgsTo("remove_setlist_song")).toEqual({
        setlistSlot: 1,
        setlistSongSlot: 1,
      });
    });
  });

  it("reordering a setlist song uses 1-based positions (drag row 1 onto row 2)", async () => {
    baseMock({
      songs: [song(3, "Song C"), song(5, "Song E")],
      setlists: [{ slot: 1, name: "Set A" }],
      members: { 1: [3, 5] },
    });
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByRole("button", { name: /new song/i });
    await user.click(screen.getByText("Set A"));
    await screen.findByText("Song C");

    const rowC = draggableRow("Song C");
    const rowE = draggableRow("Song E");
    fireEvent.dragStart(rowC);
    fireEvent.drop(rowE);
    await waitFor(() => {
      expect(lastArgsTo("move_setlist_song")).toEqual({
        setlistSlot: 1,
        oldPos: 1,
        newPos: 2,
      });
    });
  });

  it("create-and-add from the picker is ONE create_song_full with addToSetlist", async () => {
    baseMock({ setlists: [{ slot: 1, name: "Set A" }], members: { 1: [] } });
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByRole("button", { name: /new song/i });
    await user.click(screen.getByText("Set A"));
    await screen.findByText(/No songs in this setlist yet/);

    // Open the picker, then its "New song" (the library isn't rendered in setlist
    // view, so this "New song" is unambiguously the picker's).
    await user.click(screen.getAllByRole("button", { name: /add songs/i })[0]);
    await screen.findByText("Add songs from library");
    await user.click(screen.getByRole("button", { name: /new song/i }));

    await user.type(screen.getByPlaceholderText("Song name"), "Fresh");
    // The batched transaction returns the fresh songs AND the fresh membership.
    vi.mocked(invoke).mockImplementationOnce(() =>
      Promise.resolve({
        songs: [song(1, "Fresh")],
        members: [1],
        bpm_warning: null,
      }),
    );
    await user.keyboard("{Enter}");

    await waitFor(() => {
      expect(callsTo("create_song_full")).toBe(1);
    });
    expect(lastArgsTo("create_song_full")).toEqual({
      name: "Fresh",
      notes: null,
      bpm: null,
      addToSetlist: 1,
    });
    expect(await screen.findByText("Fresh")).toBeInTheDocument();
  });

  it("a failed write surfaces a Toast and does NOT mutate the list (no optimistic drift)", async () => {
    baseMock({ songs: [song(2, "Keeper")] });
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByText("Keeper");

    await user.click(screen.getByTitle("More"));
    await user.click(screen.getByText("Edit song…"));
    const nameInput = screen.getByPlaceholderText("Song name");
    await user.clear(nameInput);
    await user.type(nameInput, "Renamed");

    vi.mocked(invoke).mockImplementationOnce(() =>
      Promise.reject(new Error("rename refused")),
    );
    await user.keyboard("{Enter}");

    expect(await screen.findByText("rename refused")).toBeInTheDocument();
    // State unchanged: the original name still shows, the failed rename did not apply.
    expect(screen.getByText("Keeper")).toBeInTheDocument();
    expect(screen.queryByText("Renamed")).not.toBeInTheDocument();
  });
});

// ── Presets axis (the rail "Setlists ⇄ Presets" pivot) ──────────────────────────
// The Presets axis reads the unit's presets + each song's preset bindings from the ONE
// startup backup scan (libraryScan store), NOT a live device read. Selecting a preset
// shows a READ-ONLY list of the songs that use it.

const emptyGraph: ActiveGraph = {
  name: null,
  slot: null,
  template: null,
  split_mix: null,
  nodes: [],
  stages: [],
};
const pRow = (slot: number, name: string) => ({
  slot,
  name,
  scene_count: 0,
  scenes: [],
  amp_candidates: [],
  blocks: [],
  graph: emptyGraph,
  footswitches: [],
  silence_hint: null,
});

// Presets at device slots 8 / 58 / 100 (→ list indices 7 / 57 / 99). Bindings:
// Song A (slot 1) → presets 8 + 58 ; Song B (slot 2) → preset 58 ; preset 100 unused.
const PRESET_BACKUP: BackupReadResult = {
  members: [],
  db_bytes: 0,
  total_rows: 3,
  scene_mode: "test",
  presets: [
    pRow(8, "Plexi Crunch"),
    pRow(58, "Stadium Lead"),
    pRow(100, "Lonely Patch"),
  ],
  song_presets: [
    { song_slot: 1, preset_slot: 8 },
    { song_slot: 1, preset_slot: 58 },
    { song_slot: 2, preset_slot: 58 },
  ],
  songs: [],
  setlists: [],
  setlist_songs: [],
};

describe("SongsView — Presets axis", () => {
  beforeEach(async () => {
    vi.mocked(invoke).mockReset();
    resetLibraryScan();
    vi.mocked(invoke).mockImplementation((cmd) => {
      switch (cmd) {
        case "list_songs":
          return Promise.resolve([song(1, "Song A"), song(2, "Song B")]);
        case "read_setlists":
          return Promise.resolve([]);
        case "read_library_via_backup":
          return Promise.resolve(PRESET_BACKUP);
        default:
          return Promise.resolve([]);
      }
    });
    // Seed the shared backup store (App drives this on connect in production).
    await ensureLibraryScan();
  });

  it("flips to the Presets axis and lists the unit's presets with per-preset song counts", async () => {
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByRole("button", { name: /new song/i });

    await user.click(screen.getByRole("radio", { name: /presets/i }));

    // All three presets appear in the rail; no create control (presets are unit-owned).
    expect(await screen.findByText("Plexi Crunch")).toBeInTheDocument();
    expect(screen.getByText("Stadium Lead")).toBeInTheDocument();
    expect(screen.getByText("Lonely Patch")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /new setlist/i }),
    ).not.toBeInTheDocument();
  });

  it("a selected preset shows the read-only list of songs that use it", async () => {
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByRole("button", { name: /new song/i });
    await user.click(screen.getByRole("radio", { name: /presets/i }));

    await user.click(await screen.findByText("Stadium Lead"));
    // Two songs use slot 58.
    expect(
      await screen.findByText(/2 songs use this preset/i),
    ).toBeInTheDocument();
    expect(screen.getByText("Song A")).toBeInTheDocument();
    expect(screen.getByText("Song B")).toBeInTheDocument();
    expect(screen.getByText(/managed on the unit/i)).toBeInTheDocument();
    // Read-only: no remove/edit affordance on the song rows.
    expect(screen.queryByTitle("Remove from setlist")).not.toBeInTheDocument();
  });

  it("a preset used by no song shows the empty state", async () => {
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByRole("button", { name: /new song/i });
    await user.click(screen.getByRole("radio", { name: /presets/i }));

    await user.click(await screen.findByText("Lonely Patch"));
    expect(
      await screen.findByText(/no songs use this preset/i),
    ).toBeInTheDocument();
  });
});
