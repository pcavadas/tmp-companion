// Smoke tests for the extracted Songs-table pieces (ListHeader / SongRow). Real timers
// (RTL default). The static CPU-meter bar's tests moved to Meter.test.tsx with its look B
// rewrite (ui/Meter is now the whole CPU meter).

import { describe, it, expect } from "vitest";
import { screen } from "@testing-library/react";
import { ListHeader } from "../views/songs/ListHeader";
import { SongRow } from "../views/songs/SongRow";
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

describe("ListHeader — the shared uppercase column-header row", () => {
  it("renders each cell label and right-aligns the flagged one", () => {
    renderWithTheme(
      <ListHeader
        cols="34px 1fr 78px"
        cells={[
          { label: "№" },
          { label: "song" },
          { label: "bpm", align: "right" },
        ]}
      />,
    );
    expect(screen.getByText("№")).toBeTruthy();
    expect(screen.getByText("song")).toBeTruthy();
    const bpm = screen.getByText("bpm");
    expect(bpm.style.textAlign).toBe("right");
    // A non-flagged cell carries no explicit alignment.
    expect(screen.getByText("song").style.textAlign).toBe("");
  });
});

describe("SongRow — one song row", () => {
  it("renders the name and bpm (bare number by default)", () => {
    renderWithTheme(<SongRow song={song()} idx={0} gridCols="34px 1fr 78px" />);
    expect(screen.getByText("Song A")).toBeTruthy();
    expect(screen.getByText("120")).toBeTruthy();
  });

  it("renders '<n> bpm' + right-aligns when bpmAlign + bpmSuffix are set", () => {
    renderWithTheme(
      <SongRow
        song={song()}
        idx={0}
        gridCols="34px 1fr 78px"
        bpmAlign="right"
        bpmSuffix
      />,
    );
    const bpm = screen.getByText("120 bpm");
    expect(bpm.style.textAlign).toBe("right");
  });

  it("spreads rootProps onto the row root", () => {
    renderWithTheme(
      <SongRow
        song={song()}
        idx={0}
        gridCols="34px 1fr 78px"
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
        gridCols="26px 34px 1fr 70px 30px"
        leading={<span title="lead-slot">grip</span>}
        trailing={<span title="trail-slot">x</span>}
      />,
    );
    expect(screen.getByTitle("lead-slot")).toBeTruthy();
    expect(screen.getByTitle("trail-slot")).toBeTruthy();
  });
});
