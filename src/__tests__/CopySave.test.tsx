// src/__tests__/CopySave.test.tsx — saving to your Tone Master Pro (look B): the rows'
// words while it saves, the line naming the unchanged presets, no button and no way out
// until it settles, and the three outcomes' words.

import { describe, expect, it, vi } from "vitest";
import { screen, within } from "@testing-library/react";

import { SaveOverlay } from "../views/copy/SaveOverlay";
import type { CopyApplyItem } from "../lib/types";
import { renderWithTheme } from "./testKit";

const NAMES: Record<number, string> = {
  4: "Scooped Verse",
  7: "Tweed Warm",
  9: "Clean DI",
};
const nameOf = (s: number): string => NAMES[s] ?? "?";

const item = (
  slot: number,
  outcome: CopyApplyItem["outcome"],
): CopyApplyItem => ({ slot, name: nameOf(slot), outcome, detail: "x" });

function renderSave(over: Partial<Parameters<typeof SaveOverlay>[0]> = {}) {
  const props = {
    slots: [4, 9],
    nameOf,
    results: new Map<number, CopyApplyItem>(),
    activeSlot: 4,
    done: false,
    unchanged: [] as string[],
    onDone: vi.fn(),
    ...over,
  };
  renderWithTheme(<SaveOverlay {...props} />);
  return props;
}

const rowWord = (name: string): string | null => {
  const row = document.querySelector<HTMLElement>(`[data-save-row="${name}"]`);
  if (!row) throw new Error(`no row ${name}`);
  return within(row).getAllByText(/./).slice(-1)[0].textContent;
};

describe("SaveOverlay — while saving", () => {
  it("shows progress, each row's word, the unchanged line and no button", () => {
    renderSave({
      results: new Map([[4, item(4, "updated")]]),
      activeSlot: 9,
      unchanged: ["Tweed Warm"],
    });
    expect(
      screen.getByRole("dialog", { name: "Saving to your Tone Master Pro" }),
    ).toBeTruthy();
    expect(screen.getByText("Saving 2 presets")).toBeTruthy();
    expect(rowWord("Scooped Verse")).toBe("Saved");
    expect(rowWord("Clean DI")).toBe("Saving");
    expect(
      screen.getByText("Tweed Warm is unchanged, so it isn't saved."),
    ).toBeTruthy();
    expect(
      screen.getByText("Keep your Tone Master Pro connected."),
    ).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("says Waiting for a queued preset, and counts several unchanged ones", () => {
    renderSave({ unchanged: ["Tweed Warm", "Clean DI"], slots: [4, 7] });
    expect(rowWord("Scooped Verse")).toBe("Saving");
    expect(rowWord("Tweed Warm")).toBe("Waiting");
    expect(
      screen.getByText("2 presets are unchanged, so they aren't saved."),
    ).toBeTruthy();
  });

  it("never closes on a backdrop click", () => {
    const props = renderSave();
    const backdrop = document.querySelector<HTMLElement>("[data-backdrop]");
    backdrop?.click();
    expect(props.onDone).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toBeTruthy();
  });
});

describe("SaveOverlay — done", () => {
  it("all saved: says so and how to undo it", () => {
    const props = renderSave({
      done: true,
      activeSlot: null,
      results: new Map([
        [4, item(4, "updated")],
        [9, item(9, "skipped")],
      ]),
    });
    expect(screen.getByText("Saved to your Tone Master Pro")).toBeTruthy();
    expect(
      screen.getByText("Restore your Pro Control backup to undo this."),
    ).toBeTruthy();
    expect(rowWord("Clean DI")).toBe("No change");
    expect(
      screen.queryByText("Keep your Tone Master Pro connected."),
    ).toBeNull();
    screen.getByRole("button", { name: "Done" }).click();
    expect(props.onDone).toHaveBeenCalled();
  });

  it("some saved: counts both and says the others keep their changes", () => {
    renderSave({
      done: true,
      activeSlot: null,
      slots: [4, 7, 9],
      results: new Map([
        [4, item(4, "updated")],
        [7, item(7, "error")],
      ]),
    });
    expect(screen.getByText("Some presets weren't saved")).toBeTruthy();
    expect(
      screen.getByText(
        "1 saved. 2 couldn't be saved, and they keep your changes so you can try again. Restore your Pro Control backup to undo the ones that saved.",
      ),
    ).toBeTruthy();
    expect(rowWord("Tweed Warm")).toBe("Couldn't save");
    // The backend's detail is for the log only.
    expect(screen.queryByText("x")).toBeNull();
  });

  it("none saved: says nothing was saved and the changes are kept", () => {
    renderSave({
      done: true,
      activeSlot: null,
      results: new Map([[4, item(4, "error")]]),
    });
    expect(screen.getByText("Nothing was saved")).toBeTruthy();
    expect(
      screen.getByText("Your changes are kept, so you can try again."),
    ).toBeTruthy();
    expect(screen.queryByText(/undo/)).toBeNull();
  });
});
