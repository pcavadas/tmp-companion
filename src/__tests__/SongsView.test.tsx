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
import { screen, waitFor, fireEvent, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { SongsView } from "../views/songs";
import { DASH } from "../lib/format";
import type { CommandName } from "../lib/commands";
import type { BackupReadResult, SetlistRecord, SongRecord } from "../lib/types";
import {
  ensureLibraryScan,
  getLibraryScan,
  resetLibraryScan,
} from "../device/libraryScan";
import {
  callsOf,
  makeBackupPresetRow,
  makeBackupRead,
  mockCommands,
  rejectWith,
  type Handlers,
} from "./commandMocks";
import { renderWithTheme } from "./testKit";

function renderMgr(connected: boolean) {
  return renderWithTheme(<SongsView connected={connected} />);
}

const connectedTree = <SongsView connected={true} />;

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

/** Route the Songs reads to the seeded state; every write falls through to its typed
 *  default (an empty read-back) unless `overrides` names a specific one. */
function baseMock(state: DeviceState = {}, overrides: Partial<Handlers> = {}) {
  const songs = state.songs ?? [];
  const setlists = state.setlists ?? [];
  const members = state.members ?? {};
  mockCommands({
    list_songs: () => songs,
    read_setlists: () => setlists,
    list_setlist_songs: ({ setlistSlot }) => members[setlistSlot] ?? [],
    ...overrides,
  });
}

const callsTo = (name: CommandName) => callsOf(name).length;
const lastArgsTo = <K extends CommandName>(name: K) => {
  const calls = callsOf(name);
  return calls.length ? calls[calls.length - 1] : undefined;
};

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
    let offline = true; // the first songs read fails; Try again succeeds
    baseMock(
      {},
      {
        list_songs: () => {
          if (!offline) return [];
          offline = false;
          throw new Error("device offline");
        },
      },
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
    baseMock(
      { songs: [] },
      {
        create_song_full: () => ({
          songs: [song(1, "Toto")],
          members: null,
          bpm_warning: null,
        }),
      },
    );
    await user.keyboard("{Enter}");

    await waitFor(() => {
      expect(lastArgsTo("create_song_full")).toEqual({
        name: "Toto",
        notes: null,
        bpm: null,
        addToSetlist: null,
        expectSetlistName: null,
        expectSetlistList: null,
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

    baseMock(
      { songs: [song(2, "Toto")] },
      {
        update_song_full: () => ({
          songs: [song(2, "Toto Toto", { bpm: 102, bpm_active: true })],
          members: null,
          bpm_warning: null,
        }),
      },
    );
    await user.keyboard("{Enter}");

    await waitFor(() => {
      expect(callsTo("update_song_full")).toBe(1);
    });
    // Notes unchanged → null (the backend writes only the changed fields).
    // `expectName` is the name BEFORE the edit: the backend refuses if slot 2 moved.
    expect(lastArgsTo("update_song_full")).toEqual({
      slot: 2,
      expectName: "Toto",
      expectList: ["Toto"],
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
        expectList: ["Doomed"],
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
    baseMock(
      { setlists: [{ slot: 1, name: "Set A" }], members: { 1: [] } },
      { remove_setlist: () => [] },
    );
    await user.click(screen.getByRole("button", { name: /^delete$/i }));

    await waitFor(() => {
      expect(lastArgsTo("remove_setlist")).toEqual({
        slot: 1,
        expectName: "Set A",
        expectList: ["Set A"],
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
        expectName: "Set A",
        expectList: ["Set A"],
        setlistSongSlot: 1,
        expectSongSlot: 3,
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
        expectName: "Set A",
        expectList: ["Set A"],
        oldPos: 1,
        newPos: 2,
        expectSongSlot: 3,
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
    baseMock(
      { setlists: [{ slot: 1, name: "Set A" }], members: { 1: [] } },
      {
        create_song_full: () => ({
          songs: [song(1, "Fresh")],
          members: [1],
          bpm_warning: null,
        }),
      },
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
      expectSetlistName: "Set A",
      expectSetlistList: ["Set A"],
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

    baseMock(
      { songs: [song(2, "Keeper")] },
      { update_song_full: () => rejectWith(new Error("rename refused")) },
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

const pRow = (slot: number, name: string) =>
  makeBackupPresetRow({ slot, name });

// Presets at device slots 8 / 58 / 100 (→ list indices 7 / 57 / 99). Bindings:
// Song A (slot 1) → presets 8 + 58 ; Song B (slot 2) → preset 58 ; preset 100 unused.
const PRESET_BACKUP: BackupReadResult = makeBackupRead({
  total_rows: 3,
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
});

describe("SongsView — Presets axis", () => {
  beforeEach(async () => {
    vi.mocked(invoke).mockReset();
    resetLibraryScan();
    mockCommands({
      list_songs: () => [song(1, "Song A"), song(2, "Song B")],
      read_setlists: () => [],
      read_library_via_backup: () => PRESET_BACKUP,
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

// ── Stale-slot regressions (#295–#299) ───────────────────────────────────────────
// Setlist and song slots are list index + 1 and shift on every delete/add, so every
// cache keyed on them goes stale after an edit. These run against a small fake unit
// that applies each write and returns the fresh lists, like the backend does.

interface FakeUnit {
  songs: SongRecord[];
  setlists: SetlistRecord[];
  members: Map<number, number[]>;
}

/** Route every Songs command to `unit`, mutating it like the device would. With
 *  `held`, the FIRST call of `held.cmd` replies only once `held.until` resolves. */
function fakeUnit(
  unit: FakeUnit,
  held?: { cmd: CommandName; until: Promise<void> },
) {
  let holding = held;
  const renumber = () => {
    unit.setlists = unit.setlists.map((l, i) => ({ ...l, slot: i + 1 }));
  };
  /** `reply()` now — or, for the first call of the held command, once it opens. */
  const answer = <T,>(cmd: CommandName, reply: () => T): T | Promise<T> => {
    if (holding?.cmd !== cmd) return reply();
    const wait = holding.until;
    holding = undefined;
    return wait.then(reply);
  };
  mockCommands({
    read_library_via_backup: () =>
      answer("read_library_via_backup", () => ({
        ...PRESET_BACKUP,
        songs: unit.songs,
        setlists: unit.setlists,
        setlist_songs: [...unit.members].flatMap(([sl, m]) =>
          m.map((song_slot, i) => ({
            setlist_slot: sl,
            song_slot,
            position: i + 1,
          })),
        ),
      })),
    list_songs: () => answer("list_songs", () => unit.songs),
    read_setlists: () => answer("read_setlists", () => unit.setlists),
    list_setlist_songs: ({ setlistSlot }) =>
      answer("list_setlist_songs", () => unit.members.get(setlistSlot) ?? []),
    rename_setlist: ({ slot, name }) =>
      answer("rename_setlist", () => {
        unit.setlists = unit.setlists.map((l) =>
          l.slot === slot ? { ...l, name } : l,
        );
        return unit.setlists;
      }),
    add_setlist: ({ name }) =>
      answer("add_setlist", () => {
        unit.setlists = [
          ...unit.setlists,
          { slot: unit.setlists.length + 1, name },
        ];
        return unit.setlists;
      }),
    remove_setlist: ({ slot }) =>
      answer("remove_setlist", () => {
        unit.setlists = unit.setlists.filter((l) => l.slot !== slot);
        const shifted = new Map<number, number[]>();
        unit.members.forEach((m, k) => {
          if (k !== slot) shifted.set(k > slot ? k - 1 : k, m);
        });
        unit.members = shifted;
        renumber();
        return unit.setlists;
      }),
    remove_setlist_song: ({ setlistSlot, setlistSongSlot }) =>
      answer("remove_setlist_song", () => {
        const m = [...(unit.members.get(setlistSlot) ?? [])];
        m.splice(setlistSongSlot - 1, 1);
        unit.members.set(setlistSlot, m);
        return m;
      }),
    remove_song: ({ slot }) =>
      answer("remove_song", () => {
        unit.songs = unit.songs
          .filter((x) => x.slot !== slot)
          .map((x) => (x.slot > slot ? { ...x, slot: x.slot - 1 } : x));
        return unit.songs;
      }),
    create_song_full: ({ name }) =>
      answer("create_song_full", () => {
        unit.songs = [
          song(1, name),
          ...unit.songs.map((x) => ({ ...x, slot: x.slot + 1 })),
        ];
        return { songs: unit.songs, members: null, bpm_warning: null };
      }),
  });
}

/** A promise plus the function that resolves it. */
function gate(): { until: Promise<void>; open: () => void } {
  let resolve: (() => void) | undefined;
  const until = new Promise<void>((r) => {
    resolve = r;
  });
  return {
    until,
    open: () => {
      resolve?.();
    },
  };
}

/** The rail row for `name` (rows are role="tab"). */
const railRow = (name: string): HTMLElement => {
  const row = screen
    .getAllByRole("tab")
    .find((t) => within(t).queryByText(name, { exact: true }) != null);
  if (!row) throw new Error(`no rail row "${name}"`);
  return row;
};

describe("SongsView — stale slots after edits (#295–#299)", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    resetLibraryScan();
  });

  it("#295: after a rename and a delete, leaving and returning repaints the post-edit setlists", async () => {
    const unit: FakeUnit = {
      songs: [song(1, "Song A")],
      setlists: [
        { slot: 1, name: "Set A" },
        { slot: 2, name: "Set B" },
        { slot: 3, name: "Set C" },
      ],
      members: new Map([
        [1, [1]],
        [2, []],
        [3, []],
      ]),
    };
    fakeUnit(unit);
    await ensureLibraryScan();

    const first = renderMgr(true);
    const user = userEvent.setup();
    await screen.findByText("Set B");

    // Delete B.
    await user.click(screen.getByText("Set B"));
    await screen.findByText(/No songs in this setlist yet/);
    await user.click(screen.getByTitle("Setlist options"));
    await user.click(screen.getByText("Delete setlist"));
    await user.click(screen.getByRole("button", { name: /^delete$/i }));
    await waitFor(() => {
      expect(screen.queryByText("Set B")).not.toBeInTheDocument();
    });

    // Rename A.
    await user.click(screen.getByText("Set A"));
    await user.click(await screen.findByTitle("Rename"));
    const input = screen.getByDisplayValue("Set A");
    await user.clear(input);
    await user.type(input, "Openers{Enter}");
    await screen.findAllByText("Openers");
    expect(lastArgsTo("rename_setlist")).toEqual({
      slot: 1,
      expectName: "Set A",
      expectList: ["Set A", "Set C"],
      name: "Openers",
    });

    // Leave the tab (unmount) and come back: the rail shows the unit's lists now.
    first.unmount();
    renderMgr(true);
    await screen.findByText("Openers");
    expect(screen.queryByText("Set A")).not.toBeInTheDocument();
    expect(screen.queryByText("Set B")).not.toBeInTheDocument();
    expect(screen.getByText("Set C")).toBeInTheDocument();
  });

  it("#296: a second setlist click during the first one's read still loads the second", async () => {
    const unit: FakeUnit = {
      songs: [song(1, "Alpha"), song(2, "Beta")],
      setlists: [
        { slot: 1, name: "Set A" },
        { slot: 2, name: "Set B" },
      ],
      members: new Map([
        [1, [1]],
        [2, [2]],
      ]),
    };
    // No scan: both setlists' memberships are unknown until read live.
    const first = gate();
    fakeUnit(unit, { cmd: "list_setlist_songs", until: first.until });
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByText("Set B");

    await user.click(screen.getByText("Set A")); // read held
    await user.click(screen.getByText("Set B")); // queued behind it — never dropped
    expect(callsTo("list_setlist_songs")).toBe(1);
    // Let A's read land; B's read then runs and B's song shows.
    first.open();
    expect(await screen.findByText("Beta")).toBeInTheDocument();
    await waitFor(() => {
      expect(callsTo("list_setlist_songs")).toBe(2);
    });
    expect(lastArgsTo("list_setlist_songs")).toEqual({ setlistSlot: 2 });
  });

  it("#296: a setlist rename committed while another op runs is applied, not lost", async () => {
    const unit: FakeUnit = {
      songs: [],
      setlists: [{ slot: 1, name: "Set A" }],
      members: new Map([[1, []]]),
    };
    const read = gate();
    fakeUnit(unit, { cmd: "list_setlist_songs", until: read.until });
    await ensureLibraryScan();
    renderMgr(true);
    const user = userEvent.setup();

    // Selecting re-reads the membership — that read is held, so the view is busy.
    await user.click(await screen.findByText("Set A"));
    await user.click(await screen.findByTitle("Rename"));
    const input = screen.getByDisplayValue("Set A");
    await user.clear(input);
    await user.type(input, "Set Z{Enter}");
    expect(callsTo("rename_setlist")).toBe(0); // queued behind the read

    read.open();
    await waitFor(() => {
      expect(callsTo("rename_setlist")).toBe(1);
    });
    expect(await screen.findAllByText("Set Z")).not.toHaveLength(0);
  });

  it("a membership READ that finishes after leaving the tab keeps the cached lists", async () => {
    const unit: FakeUnit = {
      songs: [song(1, "Alpha")],
      setlists: [{ slot: 1, name: "Set A" }],
      members: new Map([[1, [1]]]),
    };
    const read = gate();
    fakeUnit(unit, { cmd: "list_setlist_songs", until: read.until });
    await ensureLibraryScan();
    const view = renderMgr(true);
    const user = userEvent.setup();
    await user.click(await screen.findByText("Set A")); // read held
    view.unmount();
    read.open();
    await waitFor(() => {
      expect(callsTo("list_setlist_songs")).toBe(1);
    });
    await Promise.resolve();
    // A read changes nothing on the unit, so the seed for the next visit stays.
    expect(getLibraryScan().setlists).toEqual([{ slot: 1, name: "Set A" }]);
  });

  it("a song delete that finishes after leaving the tab leaves the song↔preset join unknown", async () => {
    const unit: FakeUnit = {
      songs: [song(1, "Alpha"), song(2, "Beta")],
      setlists: [],
      members: new Map(),
    };
    const del = gate();
    fakeUnit(unit, { cmd: "remove_song", until: del.until });
    await ensureLibraryScan();
    expect(getLibraryScan().songPresetSlots).not.toBeNull();
    const view = renderMgr(true);
    const user = userEvent.setup();
    await screen.findByText("Alpha");
    await user.click(screen.getAllByTitle("More")[0]);
    await user.click(screen.getByText("Delete song"));
    await user.click(screen.getByRole("button", { name: /^delete$/i }));
    await waitFor(() => {
      expect(callsTo("remove_song")).toBe(1);
    });
    view.unmount(); // the delete is still running
    del.open();
    // The song slots shifted under the backup's song↔preset join: the next visit must
    // not count presets against the wrong songs.
    await waitFor(() => {
      expect(getLibraryScan().songPresetSlots).toBeNull();
    });
    expect(getLibraryScan().songs).toEqual([]);
  });

  it("#297: deleting a middle setlist keeps each remaining rail row on its own count", async () => {
    const unit: FakeUnit = {
      songs: [song(1, "S1"), song(2, "S2"), song(3, "S3"), song(4, "S4")],
      setlists: [
        { slot: 1, name: "Set A" },
        { slot: 2, name: "Set B" },
        { slot: 3, name: "Set C" },
        { slot: 4, name: "Set D" },
      ],
      members: new Map([
        [1, [1]],
        [2, [1, 2]],
        [3, [1, 2, 3]],
        [4, [1, 2, 3, 4]],
      ]),
    };
    fakeUnit(unit);
    await ensureLibraryScan();
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByText("Set B");

    // Open B's options from its detail and delete it.
    await user.click(screen.getByText("Set B"));
    await user.click(screen.getByTitle("Setlist options"));
    await user.click(screen.getByText("Delete setlist"));
    await user.click(screen.getByRole("button", { name: /^delete$/i }));
    await waitFor(() => {
      expect(screen.queryByText("Set B")).not.toBeInTheDocument();
    });

    expect(within(railRow("Set A")).getByText("1")).toBeInTheDocument();
    expect(within(railRow("Set C")).getByText("3")).toBeInTheDocument();
    expect(within(railRow("Set D")).getByText("4")).toBeInTheDocument();
  });

  it("#297: a setlist below a deleted one keeps its own songs under its new slot", async () => {
    const unit: FakeUnit = {
      songs: [song(1, "Only Song")],
      setlists: [
        { slot: 1, name: "Set A" },
        { slot: 2, name: "Set B" },
      ],
      members: new Map([
        [1, []],
        [2, [1]],
      ]),
    };
    fakeUnit(unit);
    await ensureLibraryScan();
    renderMgr(true);
    const user = userEvent.setup();
    await user.click(await screen.findByText("Set A"));
    await user.click(screen.getByTitle("Setlist options"));
    await user.click(screen.getByText("Delete setlist"));
    await user.click(screen.getByRole("button", { name: /^delete$/i }));
    await waitFor(() => {
      expect(screen.queryByText("Set A")).not.toBeInTheDocument();
    });
    // B is now slot 1 and still carries its own song.
    await user.click(screen.getByText("Set B"));
    expect(await screen.findAllByText("Only Song")).not.toHaveLength(0);
    expect(lastArgsTo("list_setlist_songs")).toEqual({ setlistSlot: 1 });
  });

  it("#297: a setlist picked while an earlier delete runs is read at its shifted slot", async () => {
    const unit: FakeUnit = {
      songs: [song(1, "Only Song")],
      setlists: [
        { slot: 1, name: "Set A" },
        { slot: 2, name: "Set B" },
        { slot: 3, name: "Set C" },
      ],
      members: new Map([
        [1, []],
        [2, []],
        [3, [1]],
      ]),
    };
    const del = gate();
    fakeUnit(unit, { cmd: "remove_setlist", until: del.until });
    await ensureLibraryScan();
    renderMgr(true);
    const user = userEvent.setup();
    await user.click(await screen.findByText("Set A"));
    await user.click(screen.getByTitle("Setlist options"));
    await user.click(screen.getByText("Delete setlist"));
    await user.click(screen.getByRole("button", { name: /^delete$/i }));
    await waitFor(() => {
      expect(callsTo("remove_setlist")).toBe(1);
    });
    // C is picked at slot 3 while A's delete is still running; the read queues.
    await user.click(screen.getByText("Set C"));
    const readsBefore = callsTo("list_setlist_songs");
    del.open();
    // The delete moved C to slot 2: the read follows it there, not to the old slot 3.
    await waitFor(() => {
      expect(callsTo("list_setlist_songs")).toBe(readsBefore + 1);
    });
    expect(lastArgsTo("list_setlist_songs")).toEqual({ setlistSlot: 2 });
    expect(await screen.findAllByText("Only Song")).not.toHaveLength(0);
    expect(within(railRow("Set C")).getByText("1")).toBeInTheDocument();
  });

  it("#298: an unresolvable member is shown, and a later row is removed at its real device position", async () => {
    const unit: FakeUnit = {
      songs: [song(3, "Song C"), song(5, "Song E")],
      setlists: [{ slot: 1, name: "Set A" }],
      // Position 2 holds a song slot the app's song list doesn't have.
      members: new Map([[1, [3, 99, 5]]]),
    };
    fakeUnit(unit);
    renderMgr(true);
    const user = userEvent.setup();
    await user.click(await screen.findByText("Set A"));
    await screen.findByText("Song E");
    expect(screen.getByText("Unknown song")).toBeInTheDocument();

    // Song E is row 3, so its device position is 3 — not 2, as filtering would make it.
    await user.click(screen.getAllByTitle("Remove from setlist")[2]);
    await waitFor(() => {
      expect(lastArgsTo("remove_setlist_song")).toEqual({
        setlistSlot: 1,
        expectName: "Set A",
        expectList: ["Set A"],
        setlistSongSlot: 3,
        expectSongSlot: 5,
      });
    });
  });

  it("#299: after a song add, the Presets axis shows the song counts as unknown, not 0", async () => {
    const unit: FakeUnit = {
      songs: [song(1, "Song A"), song(2, "Song B")],
      setlists: [],
      members: new Map(),
    };
    fakeUnit(unit);
    await ensureLibraryScan();
    renderMgr(true);
    const user = userEvent.setup();

    await user.click(await screen.findByRole("button", { name: /new song/i }));
    await user.type(screen.getByPlaceholderText("Song name"), "Fresh{Enter}");
    await screen.findByText("Fresh");

    await user.click(screen.getByRole("radio", { name: /presets/i }));
    expect(within(railRow("Stadium Lead")).getByText(DASH)).toBeInTheDocument();
    await user.click(screen.getByText("Stadium Lead"));
    expect(
      await screen.findByText(/songs using this preset: unknown/i),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(/no songs use this preset/i),
    ).not.toBeInTheDocument();
  });
});
