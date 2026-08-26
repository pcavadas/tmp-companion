// src/__tests__/usePickAnchorContentKey.test.tsx — the card-portaled menu must RE-PLACE
// itself when its content arrives after the open.
//
// BUG→GATE. `usePickAnchor`'s measure/place layout effect depended only on `[open, anchor]`,
// both fixed at `openMenu` time, while what it measures is `menuRef.current.offsetHeight`.
// `BlockLevelPick`'s BLOCK dropdown (Scene/Base rows' first-stage picker, D2/Part C) is the
// one whose body lands LATE: opening fires the lazy per-preset candidate read, so the first
// paint is a one-line "Loading controls…" and the real block list appears a moment later.
// The placement therefore stayed the skeleton's — a tall block list rendered straight off
// the bottom edge of the wizard card, never clamped and never flipped above the trigger,
// with its rows unreachable.
//
// The proxy this asserts is the menu's own `top`, which IS the effect's only output. The
// trigger is placed low in the card on purpose: a short menu fits below it (no flip), a
// tall one does not (flip above). So `top` moving from "below" to "above" proves the effect
// re-ran against the GROWN menu. Removing `contentKey` from the effect's deps leaves `top`
// at the below-value and fails this test.

import { describe, it, expect, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { ThemeProvider } from "../theme/ThemeProvider";
import { BlockLevelPick } from "../views/overlays/BlockLevelPick";
import { WithCard } from "./pickCardTestUtils";
import type {
  BlockLevelFetch,
  BlockLevelCandidate,
} from "../views/overlays/BlockLevelPick";

const CARD_H = 400;
const CARD_W = 400;
const TRIGGER_TOP = 300;
const TRIGGER_BOTTOM = 326;

/** jsdom lays nothing out: every rect is zero and every `offsetHeight` is 0, so the hook's
 *  clamp/flip arithmetic has nothing to bite on. Give it the two measurements it actually
 *  reads — the card/trigger rects, and a menu height DERIVED FROM THE RENDERED ROWS so the
 *  "menu grew" condition is a real consequence of the new props rather than a value the
 *  test pokes in by hand. */
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

  // Saved as a DESCRIPTOR rather than as a bare `Element.prototype.getBoundingClientRect`
  // reference: reading a prototype method into a variable is an `unbound-method` error, and
  // `defineProperty` restores the original shape exactly anyway (same as the two below).
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

  Element.prototype.getBoundingClientRect = function (this: Element): DOMRect {
    // The card: the `WithCard` harness's positioned 400x400 div.
    if (this instanceof HTMLElement && this.style.position === "relative") {
      // The trigger is `position: relative` too — disambiguate on the card's fixed size.
      if (this.style.width === "400px") return rect(0, 0, CARD_W, CARD_H);
      return rect(20, TRIGGER_TOP, 200, TRIGGER_BOTTOM - TRIGGER_TOP);
    }
    return rect(0, 0, 0, 0);
  };
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
    configurable: true,
    get(this: HTMLElement) {
      // Chrome (the pseudo-option row) plus one row per rendered BLOCK — the menu
      // measured here is the BLOCK dropdown's, whose rows carry `data-block-pick`
      // (one per block, not one per candidate).
      const rows = this.querySelectorAll("[data-block-pick]").length;
      return 120 + rows * 60;
    },
  });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", {
    configurable: true,
    get: () => 280,
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
  };
}

const candidate = (nodeId: string): BlockLevelCandidate => ({
  groupId: "G1",
  nodeId,
  fenderId: "ACD_TwinReverb65NoFx",
  parameterId: "outputLevel",
  paramClass: "level_linear",
});

/** The portaled menu is the only `position: absolute` div carrying a `min-width`. */
function menuTop(): number {
  const menu = document.querySelector<HTMLElement>(
    '[data-pick-backdrop] + div[style*="position: absolute"]',
  );
  if (!menu) throw new Error("the portaled menu is not open");
  return parseFloat(menu.style.top);
}

describe("usePickAnchor — a menu whose content arrives after the open", () => {
  it("re-measures and flips when the lazy candidate list lands", async () => {
    const restore = stubLayout();
    try {
      const user = userEvent.setup();
      const props = (candidates: BlockLevelFetch) => (
        <ThemeProvider>
          <WithCard>
            <BlockLevelPick
              pseudoLabel="Amp output level"
              handle={null}
              onHandleChange={vi.fn()}
              candidates={candidates}
              onOpen={vi.fn()}
            />
          </WithCard>
        </ThemeProvider>
      );

      const { rerender } = render(props({ status: "loading" }));
      await user.click(screen.getByText("Amp output level"));
      await screen.findByText("Loading controls…");

      // Skeleton: 120px tall against a trigger at y 300-326 in a 400px card, so the hook
      // flips it above at `max(8, 300 - 120 - 4)` = 176.
      const placedForSkeleton = menuTop();
      expect(placedForSkeleton).toBe(176);

      // The fetch lands with four candidates ON FOUR DIFFERENT BLOCKS (distinct
      // `nodeId`s) → the block dropdown renders four `BlockPickRow`s, a 360px-tall
      // menu, so the flip point moves to `max(8, 300 - 360 - 4)` = 8. Stale placement
      // would leave it at 176, where 176 + 360 = 536 puts 136px of the list past the
      // card's 400px bottom edge.
      rerender(
        props({
          status: "resolved",
          list: ["a", "b", "c", "d"].map(candidate),
        }),
      );
      await waitFor(() => {
        expect(document.querySelectorAll("[data-block-pick]")).toHaveLength(4);
      });
      const placedForList = menuTop();

      expect(
        placedForList,
        "the grown menu must be re-placed — with `contentKey` out of the effect's deps " +
          "this stays at the skeleton's placement and the list renders off the card",
      ).not.toBe(placedForSkeleton);
      // And it must be re-placed CORRECTLY: flipped above the trigger, never off the
      // bottom of the card.
      expect(placedForList + 360).toBeLessThanOrEqual(CARD_H);
      expect(placedForList).toBeGreaterThanOrEqual(8);
    } finally {
      restore();
    }
  });
});
