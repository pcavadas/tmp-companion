// Tests for the device-backed Songs page (SongsView).
//
// The page is now a live editor of the connected unit: it reads songs + setlists
// on the connected rising edge, and every CRUD action fires a device command and
// re-renders from the returned authoritative list (read-back-after-write). These
// tests assert: connection gating, the load-state-machine (loading→ready, error→
// retry without a hook-order crash), that writes call the right command and apply
// the read-back, that destructive deletes go through a confirm Modal whose backdrop
// does nothing, that setlist membership is read on select and addressed BY POSITION
// (1-based) for remove/reorder from each row's ⋯ menu, and that a failed write
// surfaces a plain-words Toast WITHOUT mutating state (the backend text stays in the
// log). Save outcomes are chosen by kind: a taken name and a tempo that didn't land
// each get their own message.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { screen, waitFor, fireEvent, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { SongsView } from "../views/songs";
import {
  READ_FAILED,
  TEMPO_FAILED,
  WRITE_FAILED,
  nameTaken,
} from "../views/songs/useSongsDevice";
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
import { SAVING_NOTE } from "../views/songs/SongsPage";

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

/** A save outcome with nothing to report. */
const saved = (songs: SongRecord[], members: number[] | null = null) => ({
  songs,
  members,
  duplicate_name: false,
  bpm_warning: null,
});

/** Open a song row's ⋯ menu and pick one of its items. */
async function rowMenu(
  user: ReturnType<typeof userEvent.setup>,
  songName: string,
  item: string,
): Promise<void> {
  await user.click(
    screen.getByRole("button", { name: `More for ${songName}` }),
  );
  await user.click(screen.getByRole("menuitem", { name: item }));
}

/** Open the shown setlist's ⋯ (beside its title) and pick one of its items. */
async function setlistMenu(
  user: ReturnType<typeof userEvent.setup>,
  item: string,
): Promise<void> {
  await user.click(screen.getByRole("button", { name: "Setlist options" }));
  await user.click(screen.getByRole("menuitem", { name: item }));
}

/** Start renaming the shown setlist: its title is the rename button. */
async function startRename(
  user: ReturnType<typeof userEvent.setup>,
  name: string,
): Promise<void> {
  const title = await screen.findByRole("heading", { name });
  await user.click(within(title).getByRole("button"));
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
    expect(
      screen.getByText("1 song on your Tone Master Pro"),
    ).toBeInTheDocument();
    expect(screen.getByText("Song 1")).toBeInTheDocument();
    // The list's header row names its columns in sentence case.
    const list = screen.getByRole("group", { name: "Songs" });
    expect(within(list).getByText("№")).toBeInTheDocument();
    expect(within(list).getByText("Song")).toBeInTheDocument();
  });

  it("a song's tempo shows as '{bpm} BPM', and nothing when it has none", async () => {
    baseMock({
      songs: [
        song(1, "Fast", { bpm: 142, bpm_active: true, notes: "capo 2" }),
        song(2, "Free", { bpm: 99, bpm_active: false }),
      ],
    });
    renderMgr(true);
    expect(await screen.findByText("142 BPM")).toBeInTheDocument();
    expect(screen.getByText("capo 2")).toBeInTheDocument();
    expect(screen.queryByText(/99/)).not.toBeInTheDocument();
    expect(screen.queryByText(DASH)).not.toBeInTheDocument();
  });

  it("New song opens the form above the list and stays in place, disabled; Save song waits for a name", async () => {
    baseMock({ songs: [song(1, "Song 1")] });
    renderMgr(true);
    const user = userEvent.setup();
    const newSong = await screen.findByRole("button", { name: "New song" });

    await user.click(newSong);
    expect(screen.getByRole("button", { name: "New song" })).toBeDisabled();
    const save = screen.getByRole("button", { name: "Save song" });
    expect(save).toBeDisabled();
    await user.type(screen.getByLabelText("Song name"), "Toto");
    expect(save).toBeEnabled();

    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByLabelText("Song name")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "New song" })).toBeEnabled();
    expect(callsTo("create_song_full")).toBe(0);
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

    // The error page says it in plain words; the backend text stays in the log.
    const failed = "Couldn't read your songs from your Tone Master Pro";
    expect(await screen.findByText(failed)).toBeInTheDocument();
    expect(screen.queryByText("device offline")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /try again/i }));
    await waitFor(() =>
      expect(screen.queryByText(failed)).not.toBeInTheDocument(),
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
    await user.type(screen.getByLabelText("Song name"), "Toto");

    // The create_song_full read-back returns the new song at protocol slot 1.
    baseMock(
      { songs: [] },
      { create_song_full: () => saved([song(1, "Toto")]) },
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

  it("says it is saving while a new song is written to the unit, then stops", async () => {
    let finish: () => void = () => undefined;
    const written = new Promise<void>((resolve) => {
      finish = resolve;
    });
    baseMock(
      { songs: [song(1, "Song 1")] },
      {
        create_song_full: async () => {
          await written;
          return {
            songs: [song(1, "Song 1"), song(2, "Toto")],
            members: null,
            duplicate_name: false,
            bpm_warning: null,
          };
        },
      },
    );
    renderMgr(true);
    const user = userEvent.setup();
    expect(await screen.findByText("Song 1")).toBeInTheDocument();
    expect(screen.queryByText(SAVING_NOTE)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "New song" }));
    await user.type(screen.getByLabelText("Song name"), "Toto");
    await user.click(screen.getByRole("button", { name: "Save song" }));
    // The form has closed, but the write is still running: the title row says so.
    expect(await screen.findByText(SAVING_NOTE)).toBeInTheDocument();
    finish();
    expect(await screen.findByText("Toto")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.queryByText(SAVING_NOTE)).not.toBeInTheDocument();
    });
  });

  it("a create on a name already taken says so by kind, not the tempo message", async () => {
    baseMock(
      { songs: [song(1, "Toto")] },
      {
        create_song_full: () => ({
          songs: [song(1, "Toto"), song(2, "Toto")],
          members: null,
          duplicate_name: true,
          bpm_warning: null,
        }),
      },
    );
    renderMgr(true);
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "New song" }));
    await user.type(screen.getByLabelText("Song name"), "Toto");
    await user.type(screen.getByLabelText("Tempo"), "120");
    await user.click(screen.getByRole("button", { name: "Save song" }));

    expect(await screen.findByText(nameTaken("Toto"))).toBeInTheDocument();
    expect(
      screen.getByText("You already have a song called Toto."),
    ).toBeInTheDocument();
    expect(screen.queryByText(TEMPO_FAILED)).not.toBeInTheDocument();
  });

  it("a tempo that didn't land gets its own message; the backend text stays off screen", async () => {
    const warning = "BPM didn't land at 97 (read-back=0)";
    baseMock(
      { songs: [] },
      {
        create_song_full: () => ({
          songs: [song(1, "Toto")],
          members: null,
          duplicate_name: false,
          bpm_warning: warning,
        }),
      },
    );
    const logged = vi.spyOn(console, "error").mockImplementation(() => {
      /* the log sink's console half */
    });
    renderMgr(true);
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "New song" }));
    await user.type(screen.getByLabelText("Song name"), "Toto");
    await user.type(screen.getByLabelText("Tempo"), "97{Enter}");

    expect(
      await screen.findByText(
        "Song saved, but its tempo didn't. Set it again.",
      ),
    ).toBeInTheDocument();
    // A literal substring match: the warning's parentheses would make a RegExp built
    // from it a group that never matches the text itself.
    expect(
      screen.queryByText(warning, { exact: false }),
    ).not.toBeInTheDocument();
    expect(logged).toHaveBeenCalledWith(expect.stringContaining(warning));
    logged.mockRestore();
  });

  it("edit song sends ONE update_song_full carrying only the changed fields", async () => {
    baseMock({ songs: [song(2, "Toto")] });
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByText("Toto");

    await rowMenu(user, "Toto", "Edit song");
    // The form takes the row's place, filled in.
    const nameInput = screen.getByLabelText("Song name");
    expect(nameInput).toHaveValue("Toto");
    await user.clear(nameInput);
    await user.type(nameInput, "Toto Toto");
    await user.type(screen.getByLabelText("Tempo"), "102");

    baseMock(
      { songs: [song(2, "Toto")] },
      {
        update_song_full: () =>
          saved([song(2, "Toto Toto", { bpm: 102, bpm_active: true })]),
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

  it("delete song requires the confirm Modal (backdrop and Cancel do not write, Delete song does)", async () => {
    baseMock({ songs: [song(5, "Doomed")] });
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByText("Doomed");

    await rowMenu(user, "Doomed", "Delete song");
    // Modal up, nothing written yet.
    const dialog = screen.getByRole("dialog", { name: "Delete Doomed?" });
    expect(
      within(dialog).getByText(
        "It's removed from your Tone Master Pro and from every setlist that has it. This can't be undone here.",
      ),
    ).toBeInTheDocument();
    expect(callsTo("remove_song")).toBe(0);

    // A click on the backdrop does nothing: only the dialog's own buttons close it.
    const backdrop = document.querySelector("[data-backdrop]");
    if (!(backdrop instanceof HTMLElement)) throw new Error("no backdrop");
    await user.click(backdrop);
    expect(
      screen.getByRole("dialog", { name: "Delete Doomed?" }),
    ).toBeVisible();

    // Cancel → still no write.
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(callsTo("remove_song")).toBe(0);

    // Re-open and confirm Delete → guarded remove with slot + expectName.
    await rowMenu(user, "Doomed", "Delete song");
    await user.click(screen.getByRole("button", { name: "Delete song" }));
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

    await setlistMenu(user, "Delete setlist");
    const dialog = screen.getByRole("dialog", { name: "Delete Set A?" });
    expect(
      within(dialog).getByText(
        "It's removed from your Tone Master Pro. Its songs stay in All songs.",
      ),
    ).toBeInTheDocument();
    // remove_setlist returns the fresh (now empty) list; view falls back.
    baseMock(
      { setlists: [{ slot: 1, name: "Set A" }], members: { 1: [] } },
      { remove_setlist: () => [] },
    );
    await user.click(screen.getByRole("button", { name: "Delete setlist" }));

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
    // Member songs render in device order, under the setlist's title and count.
    expect(await screen.findByText("Song C")).toBeInTheDocument();
    expect(screen.getByText("Song E")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Set A" })).toBeInTheDocument();
    expect(screen.getByText("2 songs")).toBeInTheDocument();
  });

  it("a failed setlist read says so in plain words, and opening it again reads it again", async () => {
    let fail = true;
    baseMock(
      {
        songs: [song(3, "Song C")],
        setlists: [{ slot: 1, name: "Set A" }],
      },
      {
        list_setlist_songs: () => {
          if (!fail) return [3];
          fail = false;
          throw new Error("membership read timed out");
        },
      },
    );
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByRole("button", { name: /new song/i });

    await user.click(screen.getByText("Set A"));
    expect(await screen.findByText(READ_FAILED)).toBeInTheDocument();
    expect(screen.queryByText(/timed out/)).not.toBeInTheDocument();
    await user.click(railRow("Set A"));
    expect(await screen.findByText("Song C")).toBeInTheDocument();
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

    // Remove sits in the row's ⋯, not on a one-click button.
    expect(
      screen.queryByRole("button", { name: "Remove from setlist" }),
    ).not.toBeInTheDocument();
    await rowMenu(user, "Song C", "Remove from setlist");
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

    // Open the picker (the empty page's own "Add songs"), then its "New song" (the
    // library isn't rendered in setlist view, so this "New song" is the picker's).
    await user.click(screen.getByRole("button", { name: "Add songs" }));
    await screen.findByRole("dialog", { name: "Add songs to Set A" });
    await user.click(screen.getByRole("button", { name: /new song/i }));

    await user.type(screen.getByLabelText("Song name"), "Fresh");
    // The batched transaction returns the fresh songs AND the fresh membership.
    baseMock(
      { setlists: [{ slot: 1, name: "Set A" }], members: { 1: [] } },
      { create_song_full: () => saved([song(1, "Fresh")], [1]) },
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

  it("the Add songs picker stays open on an outside click; Cancel closes it without adding", async () => {
    baseMock({
      songs: [song(3, "Song C"), song(5, "Song E"), song(7, "Song G")],
      setlists: [{ slot: 1, name: "Set A" }],
      members: { 1: [3] },
    });
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByRole("button", { name: /new song/i });
    await user.click(screen.getByText("Set A"));
    await screen.findByText("Song C");

    await user.click(screen.getByRole("button", { name: "Add songs" }));
    const picker = await screen.findByRole("dialog", {
      name: "Add songs to Set A",
    });
    // Only the songs not yet in the setlist; nothing ticked → a disabled "Add".
    expect(within(picker).queryByText("Song C")).not.toBeInTheDocument();
    expect(within(picker).getByRole("button", { name: "Add" })).toBeDisabled();
    // A click on the name ticks the box too; ticked rows count into "Add {n}".
    await user.click(within(picker).getByText("Song E"));
    await user.click(
      within(picker).getByRole("checkbox", { name: "Add Song G" }),
    );
    expect(within(picker).getByRole("button", { name: "Add 2" })).toBeEnabled();

    // An outside click leaves it open, with the choice kept.
    await user.click(screen.getByRole("heading", { name: "Set A" }));
    expect(
      screen.getByRole("dialog", { name: "Add songs to Set A" }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(
      screen.queryByRole("dialog", { name: "Add songs to Set A" }),
    ).not.toBeInTheDocument();
    expect(callsTo("add_setlist_songs")).toBe(0);

    // Add {n} adds the ticked songs in one op and closes.
    await user.click(screen.getByRole("button", { name: "Add songs" }));
    await user.click(screen.getByRole("checkbox", { name: "Add Song E" }));
    await user.click(screen.getByRole("button", { name: "Add 1" }));
    await waitFor(() => {
      expect(callsTo("add_setlist_songs")).toBe(1);
    });
    expect(
      screen.queryByRole("dialog", { name: "Add songs to Set A" }),
    ).not.toBeInTheDocument();
  });

  it("the Add songs picker closes when another setlist is shown, so its ticks never carry over", async () => {
    baseMock({
      songs: [song(3, "Song C"), song(5, "Song E"), song(7, "Song G")],
      setlists: [
        { slot: 1, name: "Set A" },
        { slot: 2, name: "Set B" },
      ],
      members: { 1: [3], 2: [5] },
    });
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByRole("button", { name: /new song/i });
    await user.click(screen.getByText("Set A"));
    await screen.findByText("Song C");

    await user.click(screen.getByRole("button", { name: "Add songs" }));
    const picker = await screen.findByRole("dialog", {
      name: "Add songs to Set A",
    });
    await user.click(
      within(picker).getByRole("checkbox", { name: "Add Song E" }),
    );
    await user.click(
      within(picker).getByRole("checkbox", { name: "Add Song G" }),
    );
    expect(within(picker).getByRole("button", { name: "Add 2" })).toBeEnabled();

    // A rename of this setlist keeps the picker open, with its ticks.
    baseMock(
      {
        songs: [song(3, "Song C"), song(5, "Song E"), song(7, "Song G")],
        setlists: [
          { slot: 1, name: "Openers" },
          { slot: 2, name: "Set B" },
        ],
        members: { 1: [3], 2: [5] },
      },
      {
        rename_setlist: () => [
          { slot: 1, name: "Openers" },
          { slot: 2, name: "Set B" },
        ],
      },
    );
    await startRename(user, "Set A");
    const input = screen.getByDisplayValue("Set A");
    await user.clear(input);
    await user.type(input, "Openers{Enter}");
    const renamed = await screen.findByRole("dialog", {
      name: "Add songs to Openers",
    });
    expect(
      within(renamed).getByRole("button", { name: "Add 2" }),
    ).toBeEnabled();

    // Another setlist in the sidebar: the picker closes, with nothing written.
    await user.click(screen.getByText("Set B"));
    await screen.findByRole("heading", { name: "Set B" });
    await screen.findByText("Song E");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    // Opened again on Set B, it starts with nothing ticked.
    await user.click(screen.getByRole("button", { name: "Add songs" }));
    const pickerB = await screen.findByRole("dialog", {
      name: "Add songs to Set B",
    });
    expect(within(pickerB).getByRole("button", { name: "Add" })).toBeDisabled();
    for (const box of within(pickerB).getAllByRole("checkbox"))
      expect(box).not.toBeChecked();
    expect(callsTo("add_setlist_songs")).toBe(0);
  });

  it("Add {n} counts the ticks the picker shows, which are the songs Add writes", async () => {
    baseMock({
      songs: [song(3, "Song C"), song(5, "Song E"), song(7, "Song G")],
      setlists: [{ slot: 1, name: "Set A" }],
      members: { 1: [3] },
    });
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByRole("button", { name: /new song/i });
    await user.click(screen.getByText("Set A"));
    await screen.findByText("Song C");

    await user.click(screen.getByRole("button", { name: "Add songs" }));
    const picker = await screen.findByRole("dialog", {
      name: "Add songs to Set A",
    });
    /** The label Add must carry for the ticks on screen. */
    const addLabelForTicks = () => {
      const n = within(picker)
        .queryAllByRole("checkbox")
        .filter((c) => c.getAttribute("aria-checked") === "true").length;
      return n > 0 ? `Add ${String(n)}` : "Add";
    };
    await user.click(
      within(picker).getByRole("checkbox", { name: "Add Song E" }),
    );
    await user.click(
      within(picker).getByRole("checkbox", { name: "Add Song G" }),
    );
    expect(addLabelForTicks()).toBe("Add 2");
    expect(within(picker).getByRole("button", { name: "Add 2" })).toBeEnabled();

    // A song made from the picker's "New song" goes in at slot 1 and moves every
    // other song up one slot, so no song is still under the slots that were ticked.
    await user.click(within(picker).getByRole("button", { name: /new song/i }));
    await user.type(screen.getByLabelText("Song name"), "Fresh");
    baseMock(
      {
        songs: [
          song(1, "Fresh"),
          song(4, "Song C"),
          song(6, "Song E"),
          song(8, "Song G"),
        ],
        setlists: [{ slot: 1, name: "Set A" }],
        members: { 1: [4, 1] },
      },
      {
        create_song_full: () =>
          saved(
            [
              song(1, "Fresh"),
              song(4, "Song C"),
              song(6, "Song E"),
              song(8, "Song G"),
            ],
            [4, 1],
          ),
      },
    );
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(callsTo("create_song_full")).toBe(1);
    });
    await within(picker).findByRole("checkbox", { name: "Add Song E" });
    const label = addLabelForTicks();
    const add = within(picker).getByRole("button", { name: label });
    if (label === "Add") expect(add).toBeDisabled();
    else expect(add).toBeEnabled();
  });

  it("a failed write surfaces a Toast and does NOT mutate the list (no optimistic drift)", async () => {
    baseMock({ songs: [song(2, "Keeper")] });
    renderMgr(true);
    const user = userEvent.setup();
    await screen.findByText("Keeper");

    await rowMenu(user, "Keeper", "Edit song");
    const nameInput = screen.getByLabelText("Song name");
    await user.clear(nameInput);
    await user.type(nameInput, "Renamed");

    baseMock(
      { songs: [song(2, "Keeper")] },
      { update_song_full: () => rejectWith(new Error("rename refused")) },
    );
    await user.keyboard("{Enter}");

    // Plain words on screen; the backend's text goes to the log, not the toast.
    expect(await screen.findByText(WRITE_FAILED)).toBeInTheDocument();
    expect(screen.queryByText("rename refused")).not.toBeInTheDocument();
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
    expect(await screen.findByText("Used by 2 songs")).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Stadium Lead" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Song A")).toBeInTheDocument();
    expect(screen.getByText("Song B")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Which songs use a preset is set on your Tone Master Pro.",
      ),
    ).toBeInTheDocument();
    // Read-only: no ⋯ on the song rows.
    expect(
      screen.queryByRole("button", { name: /^More for/ }),
    ).not.toBeInTheDocument();
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
    // The empty state says it once: no "Used by 0 songs" subtitle, and the caption
    // under a list is not drawn.
    expect(screen.queryByText(/^Used by/)).not.toBeInTheDocument();
    expect(
      screen.getAllByText(
        "Which songs use a preset is set on your Tone Master Pro.",
      ),
    ).toHaveLength(1);
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
        return saved(unit.songs);
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

/** The rail row for `name` (rows are navigation buttons). */
const railRow = (name: string): HTMLElement => {
  const row = screen
    .getAllByRole("button")
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
    await setlistMenu(user, "Delete setlist");
    await user.click(screen.getByRole("button", { name: "Delete setlist" }));
    await waitFor(() => {
      expect(screen.queryByText("Set B")).not.toBeInTheDocument();
    });

    // Rename A.
    await user.click(screen.getByText("Set A"));
    await startRename(user, "Set A");
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
    await startRename(user, "Set A");
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
    await rowMenu(user, "Alpha", "Delete song");
    await user.click(screen.getByRole("button", { name: "Delete song" }));
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
    await setlistMenu(user, "Delete setlist");
    await user.click(screen.getByRole("button", { name: "Delete setlist" }));
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
    await setlistMenu(user, "Delete setlist");
    await user.click(screen.getByRole("button", { name: "Delete setlist" }));
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
    await setlistMenu(user, "Delete setlist");
    await user.click(screen.getByRole("button", { name: "Delete setlist" }));
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
    expect(
      screen.getByText(
        "Not found on your Tone Master Pro. Reconnect it, or remove this song.",
      ),
    ).toBeInTheDocument();

    // Song E is row 3, so its device position is 3 — not 2, as filtering would make it.
    await rowMenu(user, "Song E", "Remove from setlist");
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
    await user.type(screen.getByLabelText("Song name"), "Fresh{Enter}");
    await screen.findByText("Fresh");

    await user.click(screen.getByRole("radio", { name: /presets/i }));
    expect(within(railRow("Stadium Lead")).getByText(DASH)).toBeInTheDocument();
    await user.click(screen.getByText("Stadium Lead"));
    expect(await screen.findByText("Not known right now")).toBeInTheDocument();
    // The count is unknown, so the subtitle isn't drawn at all.
    expect(screen.queryByText(/^Used by/)).not.toBeInTheDocument();
    expect(
      screen.queryByText(/no songs use this preset/i),
    ).not.toBeInTheDocument();
  });
});
