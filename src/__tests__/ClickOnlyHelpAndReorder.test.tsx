// #302: setlist songs reorder with clicks alone (⋯ Move up / Move down), and
// #304: the no-device page's "Connection help" (DeviceState, shared by Level, Doctor,
// Copy and Songs) opens real help instead of being a dead button.

import { describe, it, expect, vi } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { SetlistDetail } from "../views/songs/SetlistDetail";
import { DeviceState } from "../views/DeviceState";
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

    // Each row's ⋯ holds its moves, then Remove from setlist.
    await user.click(screen.getByRole("button", { name: "More for Song A" }));
    expect(screen.queryByRole("menuitem", { name: "Move up" })).toBeNull();
    expect(screen.getAllByRole("menuitem").map((m) => m.textContent)).toEqual([
      "Move down",
      "Remove from setlist",
    ]);
    await user.click(screen.getByRole("menuitem", { name: "Move down" }));
    expect(onReorder).toHaveBeenLastCalledWith(entries[0], entries[1]);

    await user.click(screen.getByRole("button", { name: "More for Song B" }));
    expect(screen.queryByRole("menuitem", { name: "Move down" })).toBeNull();
    await user.click(screen.getByRole("menuitem", { name: "Move up" }));
    expect(onReorder).toHaveBeenLastCalledWith(entries[1], entries[0]);
  });

  it("the move menu stays shut while a device write is busy", async () => {
    const onReorder = renderDetail(true);
    await userEvent.click(
      screen.getByRole("button", { name: "More for Song A" }),
    );
    expect(screen.queryByRole("menuitem", { name: "Move down" })).toBeNull();
    expect(onReorder).not.toHaveBeenCalled();
  });
});

describe("DeviceState — Connection help works (#304)", () => {
  it.each(["off", "failed"] as const)(
    "the %s page opens the connection help sheet, and Done closes it",
    async (state) => {
      renderWithTheme(<DeviceState state={state} />);
      const user = userEvent.setup();
      await user.click(screen.getByRole("button", { name: "Connection help" }));
      expect(
        screen.getByRole("dialog", { name: "Connecting your Tone Master Pro" }),
      ).toBeInTheDocument();
      expect(screen.getByText("Quit Pro Control")).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "Done" }));
      expect(screen.queryByRole("dialog")).toBeNull();
    },
  );

  it.each(["connecting", "off", "busy", "failed"] as const)(
    "the %s page offers no Scan or Connect button (connection is automatic)",
    (state) => {
      renderWithTheme(<DeviceState state={state} />);
      expect(screen.queryByText(/scan for device/i)).toBeNull();
      expect(screen.queryByRole("button", { name: /connect$/i })).toBeNull();
    },
  );
});
