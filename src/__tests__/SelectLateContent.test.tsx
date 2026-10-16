// src/__tests__/SelectLateContent.test.tsx — a Select's open list must RE-PLACE itself
// when its rows arrive after the open. Ported from usePickAnchorContentKey.test.tsx to
// the look B `Select` (whose list is the DS `Menu`, placed from the trigger's rect).
//
// BUG→GATE (the original): the picker's placement effect depended only on values fixed at
// open time while what it measures is the list's height. A list whose body lands LATE —
// opening fires a lazy per-preset read, so the first paint is one "Reading…" line and the
// real rows appear a moment later — kept the placement computed for the one-line list: a
// tall list rendered straight off the bottom of the window, never clamped and never
// flipped above its trigger, with its rows unreachable.
//
// The proxy this asserts is the list's own `top`, which IS the placement's only output.
// The trigger is placed low in a short window on purpose: the grown list cannot fit below
// it and must flip above. So `top` moving to the flipped value proves the placement re-ran
// against the GROWN list. Removing Menu's `contentKey` re-placement leaves `top` at the
// one-line list's value and fails this test.

import { describe, it, expect, vi } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { Select } from "../ui/Select";
import type { SelectOption } from "../ui/Select";
import { renderWithTheme } from "./testKit";

const WIN_H = 400;
const WIN_W = 400;
const TRIGGER_TOP = 300;
const TRIGGER_BOTTOM = 326;

/** jsdom lays nothing out: every rect is zero and every `offsetHeight` is 0, so the
 *  clamp/flip arithmetic has nothing to bite on. Give it the measurements it reads — the
 *  window size, the anchor's rect, and a list height DERIVED FROM THE RENDERED ROWS so
 *  "the list grew" is a real consequence of the new props rather than a poked value. */
function stubLayout() {
  const rect = (
    left: number,
    top: number,
    width: number,
    height: number,
  ): DOMRect => ({
    left,
    top,
    right: left + width,
    bottom: top + height,
    width,
    height,
    x: left,
    y: top,
    toJSON: () => ({}),
  });

  const origRect = Object.getOwnPropertyDescriptor(
    Element.prototype,
    "getBoundingClientRect",
  );
  const origH = Object.getOwnPropertyDescriptor(
    HTMLElement.prototype,
    "offsetHeight",
  );
  const origW = Object.getOwnPropertyDescriptor(
    HTMLElement.prototype,
    "offsetWidth",
  );
  const origInnerH = Object.getOwnPropertyDescriptor(window, "innerHeight");
  const origInnerW = Object.getOwnPropertyDescriptor(window, "innerWidth");

  Element.prototype.getBoundingClientRect = function (this: Element): DOMRect {
    // The anchor box: the Select's wrapper, whose first child is the trigger.
    if (
      this.firstElementChild?.getAttribute("aria-haspopup") === "listbox" ||
      this.getAttribute("aria-haspopup") === "listbox"
    )
      return rect(20, TRIGGER_TOP, 200, TRIGGER_BOTTOM - TRIGGER_TOP);
    return rect(0, 0, 0, 0);
  };
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
    configurable: true,
    get(this: HTMLElement) {
      // Chrome plus one 60 px slot per rendered option.
      const rows = this.querySelectorAll('[role="option"]').length;
      return 120 + rows * 60;
    },
  });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", {
    configurable: true,
    get: () => 280,
  });
  Object.defineProperty(window, "innerHeight", {
    configurable: true,
    value: WIN_H,
  });
  Object.defineProperty(window, "innerWidth", {
    configurable: true,
    value: WIN_W,
  });

  return () => {
    if (origRect)
      Object.defineProperty(
        Element.prototype,
        "getBoundingClientRect",
        origRect,
      );
    if (origH)
      Object.defineProperty(HTMLElement.prototype, "offsetHeight", origH);
    if (origW)
      Object.defineProperty(HTMLElement.prototype, "offsetWidth", origW);
    if (origInnerH) Object.defineProperty(window, "innerHeight", origInnerH);
    if (origInnerW) Object.defineProperty(window, "innerWidth", origInnerW);
  };
}

const knobs = (n: number): SelectOption[] =>
  Array.from({ length: n }, (_, i) => ({
    value: `k${String(i)}`,
    label: `Knob ${String(i)}`,
  }));

function listTop(): number {
  return parseFloat(screen.getByRole("listbox").style.top);
}

describe("Select — a list whose rows arrive after the open", () => {
  it("re-measures and flips when the lazy option list lands", async () => {
    const restore = stubLayout();
    try {
      const user = userEvent.setup();
      const ui = (loading: boolean, options: SelectOption[]) => (
        <Select
          label="Knob for Plexi Crunch, Crunch"
          value="k0"
          staleLabel="Output level"
          loading={loading}
          options={options}
          onOpen={vi.fn()}
          onChange={vi.fn()}
        />
      );

      const { rerender } = renderWithTheme(ui(true, []));
      await user.click(screen.getByRole("button", { name: /^Knob for/ }));
      await screen.findByText("Reading…");

      // One line: 120 px against a trigger at y 300-326 in a 400 px window does not fit
      // below (330 + 120 > 392), so it flips above at `max(8, 300 - 120 - 4)` = 176.
      const placedForOneLine = listTop();
      expect(placedForOneLine).toBe(176);

      // The read lands with four knobs → a 360 px list, so the flip point moves to
      // `max(8, 300 - 360 - 4)` = 8. Stale placement would leave it at 176, where
      // 176 + 360 = 536 puts 136 px of the list past the window's bottom edge.
      rerender(ui(false, knobs(4)));
      await waitFor(() => {
        expect(screen.getAllByRole("option")).toHaveLength(4);
      });
      const placedForList = listTop();

      expect(
        placedForList,
        "the grown list must be re-placed — without Menu's contentKey re-placement " +
          "this stays at the one-line list's placement and runs off the window",
      ).not.toBe(placedForOneLine);
      expect(placedForList + 360).toBeLessThanOrEqual(WIN_H);
      expect(placedForList).toBeGreaterThanOrEqual(8);
    } finally {
      restore();
    }
  });

  it("drops below a trigger with room under it, left edges flush", async () => {
    const restore = stubLayout();
    try {
      const user = userEvent.setup();
      Object.defineProperty(window, "innerHeight", {
        configurable: true,
        value: 1000,
      });
      renderWithTheme(
        <Select
          label="Target"
          value="k0"
          options={knobs(2)}
          onChange={vi.fn()}
        />,
      );
      await user.click(screen.getByRole("button", { name: /^Target/ }));
      // Below the trigger: bottom 326 + the 4 px gap.
      expect(listTop()).toBe(TRIGGER_BOTTOM + 4);
      expect(parseFloat(screen.getByRole("listbox").style.left)).toBe(20);
    } finally {
      restore();
    }
  });
});
