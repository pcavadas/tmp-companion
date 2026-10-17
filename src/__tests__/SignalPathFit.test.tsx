// SignalPathFit — the Level list's signal path always shows whole: a chain wider
// than the window scales DOWN to fit, a short one keeps its natural size, and the box
// below never jumps (the wrapper takes the scaled height). jsdom has no layout, so the
// widths are stubbed on the prototype and a fake ResizeObserver replays a resize.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, screen } from "@testing-library/react";

import { SignalPathFit } from "../views/level/SignalPathFit";
import { renderWithTheme } from "./testKit";

// The space the window gives (outer clientWidth) and the diagram's natural width and
// height (inner offsetWidth / offsetHeight).
const size = { available: 800, natural: 1000, height: 240 };

let observers: (() => void)[] = [];
class FakeResizeObserver {
  private readonly cb: () => void;
  constructor(cb: () => void) {
    this.cb = cb;
    observers.push(cb);
  }
  observe() {
    /* the test replays resizes by hand */
  }
  disconnect() {
    observers = observers.filter((o) => o !== this.cb);
  }
}

beforeEach(() => {
  observers = [];
  vi.stubGlobal("ResizeObserver", FakeResizeObserver);
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(
    () => size.available,
  );
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockImplementation(
    () => size.natural,
  );
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(
    () => size.height,
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

function renderFit() {
  renderWithTheme(
    <SignalPathFit label="Signal path of Verse — Split, the preset loaded on your Tone Master Pro">
      <div data-testid="diagram">diagram</div>
    </SignalPathFit>,
  );
  const figure = screen.getByRole("figure", {
    name: "Signal path of Verse — Split, the preset loaded on your Tone Master Pro",
  });
  const inner = screen.getByTestId("diagram").parentElement;
  if (!inner) throw new Error("no inner box");
  return { figure, inner };
}

describe("SignalPathFit", () => {
  it("scales a chain wider than the window down to fit, and takes the scaled height", () => {
    size.available = 800;
    size.natural = 1000;
    const { figure, inner } = renderFit();
    expect(inner.style.transform).toBe("scale(0.8)");
    // 240 × 0.8, less the 2 px that hide the diagram's own bottom hairline.
    expect(figure.style.height).toBe("190px");
  });

  it("keeps a chain that fits at its natural size", () => {
    size.available = 1200;
    size.natural = 1200; // the inner box is at least the full width
    const { figure, inner } = renderFit();
    expect(inner.style.transform).toBe("");
    expect(figure.style.height).toBe("238px");
  });

  it("re-fits when the window resizes", () => {
    size.available = 900;
    size.natural = 900;
    const { figure, inner } = renderFit();
    expect(inner.style.transform).toBe("");
    size.available = 600;
    act(() => {
      observers.forEach((o) => {
        o();
      });
    });
    expect(inner.style.transform).toBe("scale(0.6666666666666666)");
    expect(figure.style.height).toBe("158px");
  });
});
