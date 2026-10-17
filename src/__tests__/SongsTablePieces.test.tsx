// Smoke tests for the Songs list pieces (look B): the All songs header row and the
// shared SongRow. Real timers (RTL default). The static CPU-meter bar's tests moved to
// Meter.test.tsx with its look B rewrite (ui/Meter is now the whole CPU meter).

import { describe, it, expect, vi } from "vitest";
import { screen, within } from "@testing-library/react";
import { SongRow } from "../views/songs/SongRow";
import { SongList } from "../views/songs/SongList";
import type { SongRecord } from "../lib/types";
import { renderWithTheme } from "./testKit";

const song = (over: Partial<SongRecord> = {}): SongRecord => ({
  slot: 1,
  name: "Song A",
  notes: "",
  bpm: 120,
  bpm_active: true,
  ...over,
});

describe("All songs header row — the list's column labels", () => {
  it("names the columns '№' and 'Song' in sentence case, with no tempo column", () => {
    renderWithTheme(
      <SongList
        songs={[song()]}
        busy={false}
        onAddSong={vi.fn()}
        onEditSong={vi.fn()}
        onDeleteSong={vi.fn()}
      />,
    );
    const list = screen.getByRole("group", { name: "Songs" });
    expect(within(list).getByText("№")).toBeTruthy();
    expect(within(list).getByText("Song")).toBeTruthy();
    expect(within(list).queryByText(/^bpm$/i)).toBeNull();
    // No uppercase column labels in look B: DSPrimitives' source scan forbids
    // uppercase outside the diagram.
  });
});

describe("SongRow — one song row", () => {
  it("renders the position, the name and '<n> BPM'", () => {
    renderWithTheme(<SongRow song={song()} idx={0} />);
    expect(screen.getByText("01")).toBeTruthy();
    expect(screen.getByText("Song A")).toBeTruthy();
    expect(screen.getByText("120 BPM")).toBeTruthy();
  });

  it("shows no tempo at all when the song has none", () => {
    renderWithTheme(<SongRow song={song({ bpm_active: false })} idx={4} />);
    expect(screen.getByText("05")).toBeTruthy();
    expect(screen.queryByText(/BPM/)).toBeNull();
    expect(screen.queryByText("120")).toBeNull();
  });

  it("shows the notes on the same line as the name", () => {
    renderWithTheme(<SongRow song={song({ notes: "capo 2" })} idx={0} />);
    const notes = screen.getByText("capo 2");
    expect(notes.parentElement).toBe(screen.getByText("Song A").parentElement);
  });

  it("spreads rootProps onto the row root", () => {
    renderWithTheme(
      <SongRow
        song={song()}
        idx={0}
        rootProps={{ title: "songroot", draggable: true }}
      />,
    );
    const root = screen.getByTitle("songroot");
    expect(root.getAttribute("draggable")).toBe("true");
    // The row content lives inside the same root.
    expect(root.textContent).toContain("Song A");
  });

  it("renders leading + trailing slots", () => {
    renderWithTheme(
      <SongRow
        song={song()}
        idx={0}
        leading={<span title="lead-slot">grip</span>}
        trailing={<span title="trail-slot">x</span>}
      />,
    );
    expect(screen.getByTitle("lead-slot")).toBeTruthy();
    expect(screen.getByTitle("trail-slot")).toBeTruthy();
  });
});
