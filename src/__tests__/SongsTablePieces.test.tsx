// Smoke tests for the extracted Songs-table pieces (ListHeader / SongRow) and the
// static CPU-meter bar (ui/Meter). Real timers (RTL default).

import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { ThemeProvider } from "../theme/ThemeProvider";
import { ListHeader } from "../views/songs/ListHeader";
import { SongRow } from "../views/songs/SongRow";
import { Meter } from "../ui/Meter";
import type { SongRecord } from "../lib/types";

function under(node: ReactNode) {
  return render(<ThemeProvider>{node}</ThemeProvider>);
}

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
    under(
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
    under(<SongRow song={song()} idx={0} gridCols="34px 1fr 78px" />);
    expect(screen.getByText("Song A")).toBeTruthy();
    expect(screen.getByText("120")).toBeTruthy();
  });

  it("renders '<n> bpm' + right-aligns when bpmAlign + bpmSuffix are set", () => {
    under(
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
    under(
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
    under(
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

describe("Meter — static track+fill bar", () => {
  it("clamps pct to 0–100", () => {
    const { container: high } = under(
      <Meter pct={150} width={96} height={6} />,
    );
    const { container: low } = under(<Meter pct={-10} width={96} height={6} />);
    const { container: mid } = under(<Meter pct={42} width={96} height={6} />);
    expect(fill(high).style.width).toBe("100%");
    expect(fill(low).style.width).toBe("0%");
    expect(fill(mid).style.width).toBe("42%");
  });

  it("renders the marker only when given", () => {
    const { container: none } = under(<Meter pct={50} width={96} height={6} />);
    const { container: withM } = under(
      <Meter pct={50} width={96} height={6} marker={76.5} />,
    );
    expect(track(none).childElementCount).toBe(1);
    expect(track(withM).childElementCount).toBe(2);
    // Marker sits at its budget position.
    const marker = track(withM).children[1] as HTMLElement | undefined;
    expect(marker?.style.left).toBe("76.5%");
  });

  it("has no transition (paints instantly)", () => {
    const { container } = under(<Meter pct={50} width={96} height={6} />);
    expect(track(container).style.transition).toBe("");
    expect(fill(container).style.transition).toBe("");
  });
});

function track(container: HTMLElement): HTMLElement {
  return container.firstElementChild as HTMLElement;
}
function fill(container: HTMLElement): HTMLElement {
  return track(container).children[0] as HTMLElement;
}
