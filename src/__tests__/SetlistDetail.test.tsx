// src/__tests__/SetlistDetail.test.tsx — the setlist detail pane.

import { describe, it, expect, vi } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { SetlistDetail } from "../views/songs/SetlistDetail";
import type { SetlistRecord, SongRecord } from "../lib/types";
import { renderWithTheme } from "./testKit";

/** Nearest ancestor matching `sel` — avoids the lint-forbidden non-null `!`. */
function ancestor(el: HTMLElement, sel: string): HTMLElement {
  const found = el.closest(sel);
  if (!found) throw new Error(`no ${sel} ancestor`);
  return found as HTMLElement;
}

describe("regression: BUG-5 a setlist row's reorder + remove are gated while a device write is busy", () => {
  const song: SongRecord = { slot: 1, name: "Song A", notes: "", bpm: 0 };
  const setlist: SetlistRecord = { slot: 1, name: "Set 1" };
  const baseProps = {
    setlist,
    entries: [{ position: 1, songSlot: 1, song }],
    available: [],
    onRename: vi.fn(),
    onDelete: vi.fn(),
    onReorder: vi.fn(),
    onAdd: vi.fn(),
    onCreateAndAdd: vi.fn(),
  };

  it("does not fire remove while busy, does when idle; the row isn't draggable while busy", async () => {
    const onRemoveSong = vi.fn();
    const props = { ...baseProps, onRemoveSong };
    const { rerender } = renderWithTheme(
      <SetlistDetail {...props} busy={true} />,
    );
    const user = userEvent.setup();
    // The row is non-draggable while a write is in flight.
    expect(
      ancestor(screen.getByText("Song A"), "[draggable]").getAttribute(
        "draggable",
      ),
    ).toBe("false");
    // Remove lives in the row's ⋯, which is disabled (its menu stays shut).
    const more = screen.getByRole("button", { name: "More for Song A" });
    expect(more).toBeDisabled();
    await user.click(more);
    expect(
      screen.queryByRole("menuitem", { name: "Remove from setlist" }),
    ).toBeNull();
    expect(onRemoveSong).not.toHaveBeenCalled();

    rerender(<SetlistDetail {...props} busy={false} />);
    expect(
      ancestor(screen.getByText("Song A"), "[draggable]").getAttribute(
        "draggable",
      ),
    ).toBe("true");
    await user.click(screen.getByRole("button", { name: "More for Song A" }));
    await user.click(
      screen.getByRole("menuitem", { name: "Remove from setlist" }),
    );
    expect(onRemoveSong).toHaveBeenCalledWith({
      position: 1, // 1-based device position
      songSlot: 1,
      song,
    });
  });

  it("the grip names the move and isn't a button (dragging is its one use)", () => {
    renderWithTheme(
      <SetlistDetail {...baseProps} onRemoveSong={vi.fn()} busy={false} />,
    );
    expect(screen.getByRole("img", { name: "Move Song A" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Move Song A" })).toBeNull();
  });
});
