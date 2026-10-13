// #302: setlist songs reorder with clicks alone (⋯ Move up / Move down), and
// #304: the no-device screen's "Connection help" opens real help instead of
// being a dead button.

import { describe, it, expect, vi } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { SetlistDetail } from "../views/songs/SetlistDetail";
import { EmptyState } from "../views/EmptyState";
import type { SetlistRecord, SongRecord } from "../lib/types";
import { renderWithTheme } from "./testKit";

describe("SetlistDetail — click-only reorder (#302)", () => {
  const a: SongRecord = { slot: 1, name: "Song A", notes: "", bpm: 0 };
  const b: SongRecord = { slot: 2, name: "Song B", notes: "", bpm: 0 };
  const setlist: SetlistRecord = { slot: 1, name: "Set 1" };
  const entries = [
    { position: 1, songSlot: 1, song: a },
    { position: 2, songSlot: 2, song: b },
  ];

  function renderDetail(busy = false) {
    const onReorder = vi.fn();
    renderWithTheme(
      <SetlistDetail
        setlist={setlist}
        entries={entries}
        available={[]}
        busy={busy}
        onRename={vi.fn()}
        onDelete={vi.fn()}
        onRemoveSong={vi.fn()}
        onReorder={onReorder}
        onAdd={vi.fn()}
        onCreateAndAdd={vi.fn()}
      />,
    );
    return onReorder;
  }

  it("Move down / Move up reorder by device position, edges offer only the valid move", async () => {
    const onReorder = renderDetail();
    const user = userEvent.setup();

    await user.click(screen.getAllByTitle("Move")[0]);
    expect(screen.queryByText("Move up")).toBeNull();
    await user.click(screen.getByText("Move down"));
    expect(onReorder).toHaveBeenLastCalledWith(entries[0], entries[1]);

    await user.click(screen.getAllByTitle("Move")[1]);
    expect(screen.queryByText("Move down")).toBeNull();
    await user.click(screen.getByText("Move up"));
    expect(onReorder).toHaveBeenLastCalledWith(entries[1], entries[0]);
  });

  it("the move menu stays shut while a device write is busy", async () => {
    const onReorder = renderDetail(true);
    await userEvent.click(screen.getAllByTitle("Move")[0]);
    expect(screen.queryByText("Move down")).toBeNull();
    expect(onReorder).not.toHaveBeenCalled();
  });
});

describe("EmptyState — Connection help works (#304)", () => {
  it("opens the connection help sheet, and Done closes it", async () => {
    renderWithTheme(
      <EmptyState title="No unit" body="Connect it." onScan={vi.fn()} />,
    );
    const user = userEvent.setup();
    await user.click(screen.getByText("Connection help"));
    expect(screen.getByText("Close Pro Control")).toBeInTheDocument();
    await user.click(screen.getByText("Done"));
    expect(screen.queryByText("Close Pro Control")).toBeNull();
  });

  it("renders no Scan button without a handler", () => {
    renderWithTheme(<EmptyState title="No unit" body="Connect it." />);
    expect(screen.queryByText("Scan for device")).toBeNull();
    expect(screen.getByText("Connection help")).toBeInTheDocument();
  });
});
