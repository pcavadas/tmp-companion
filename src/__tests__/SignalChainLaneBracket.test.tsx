// `useLaneBracket`/`LaneBracketCap` (views/signalChain/) draw the bracket
// connecting a split/join point's two lanes from DOM-measured rects rather than
// fixed constants — this is the exact mechanism this session's fix touched
// (ForkTail/JoinHead adopted SplitGroup's pre-existing measured-bracket
// technique). jsdom has no real layout, so `getBoundingClientRect` always
// returns an all-zero rect unless stubbed (the same technique setup.ts uses for
// SVGElement.getBBox). These tests mock
// concrete rects and assert the bracket lands at the exact computed pixel
// geometry, not just "renders without crashing".

import { describe, it, expect, vi } from "vitest";

import { SignalChainView } from "../views/SignalChainView";
import { renderWithTheme } from "./testKit";

const b = (name: string) => ({ name });

// `measureLaneBracket` calls getBoundingClientRect exactly 3 times per mount,
// in fixed order: the column wrapper, then lane A, then lane B.
function mockLaneRects(rects: Partial<DOMRect>[]) {
  let call = 0;
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    () => rects[call++ % rects.length] as DOMRect,
  );
}

function bracketCapOf(container: HTMLElement): HTMLElement {
  // DiamondNode's SPLIT/MIX/JOIN caption is ALSO an absolutely-positioned div,
  // so also match LaneBracketCap's distinctive `width: 9` (its `[side]: -9`
  // sibling border-box cap — see views/signalChain/LaneBracketCap.tsx) to avoid picking that one.
  const cap = [...container.querySelectorAll("div")].find(
    (el) => el.style.position === "absolute" && el.style.width === "9px",
  );
  if (!cap) throw new Error("expected an absolutely-positioned bracket div");
  return cap;
}

describe("SignalChainView — lane bracket geometry", () => {
  it("ForkTail (gtrSplit): brackets the asymmetric multi-block lane exactly", () => {
    mockLaneRects([
      { top: 100, height: 0 }, // column wrapper — only .top used as origin
      { top: 110, height: 76 }, // lane A (out1, 2-block lane)
      { top: 210, height: 40 }, // lane B (out2, 1-block lane)
    ]);
    const { container } = renderWithTheme(
      <SignalChainView
        graph={{
          stages: [{ kind: "series", blocks: [b("G1")] }],
          outputs: {
            a: { type: "out1", blocks: [b("G2"), b("G3")] },
            b: { type: "out2", blocks: [b("G5")] },
          },
        }}
      />,
    );
    const cap = bracketCapOf(container);
    // aC = (110-100) + (76-18)/2 = 39 ; bC = (210-100) + (40-18)/2 = 121
    // height = bC - aC = 82. STRIP_LBL = 18 (SignalChainView.tsx).
    expect(cap.style.top).toBe("39px");
    expect(cap.style.height).toBe("82px");
    expect(cap.style.left).toBe("-9px"); // ForkTail's bracket sits on the split side.
    expect(cap.style.right).toBe("");
  });

  it("JoinHead (gtrMicSeries): brackets on the join side (right), not the split side", () => {
    mockLaneRects([
      { top: 50, height: 0 },
      { top: 60, height: 76 },
      { top: 160, height: 40 },
    ]);
    const { container } = renderWithTheme(
      <SignalChainView
        graph={{
          inputs: {
            a: { type: "guitar", blocks: [b("G1"), b("G2")] },
            b: { type: "mic", blocks: [b("M1")] },
          },
          stages: [{ kind: "series", blocks: [b("XO")] }],
        }}
      />,
    );
    const cap = bracketCapOf(container);
    expect(cap.style.top).toBe("39px");
    expect(cap.style.height).toBe("82px");
    expect(cap.style.right).toBe("-9px"); // JoinHead mirrors ForkTail onto the join side.
    expect(cap.style.left).toBe("");
  });
});

describe("regression: #313 same-count bracket re-measure", () => {
  it("re-measures on a same-count change, not only a count change", () => {
    const graph = (name: string) => ({
      stages: [{ kind: "series" as const, blocks: [b("G1")] }],
      outputs: {
        a: { type: "out1" as const, blocks: [b(name), b("G3")] },
        b: { type: "out2" as const, blocks: [b("G5")] },
      },
    });
    mockLaneRects([
      { top: 100, height: 0 },
      { top: 110, height: 76 },
      { top: 210, height: 40 },
    ]);
    const { container, rerender } = renderWithTheme(
      <SignalChainView graph={graph("G2")} />,
    );
    expect(bracketCapOf(container).style.height).toBe("82px");
    // Same block counts, but the lanes now lay out differently (e.g. a longer label
    // or the skeleton→real swap) — the old count-keyed signature never re-measured.
    vi.restoreAllMocks();
    mockLaneRects([
      { top: 100, height: 0 },
      { top: 110, height: 76 },
      { top: 250, height: 40 },
    ]);
    rerender(<SignalChainView graph={graph("Renamed")} />);
    // bC = (250-100) + (40-18)/2 = 161 ; height = 161 - 39 = 122.
    expect(bracketCapOf(container).style.height).toBe("122px");
  });
});

describe("regression: a scaled-down diagram draws its bracket to the lower lane", () => {
  it("divides the ancestor's CSS scale back out of the measured rects", () => {
    // The Level list fits a long chain to the window with a transform, so the screen
    // rects are half size here while the bracket is placed in layout px. The column
    // lays out 200 px tall but measures 100 on screen: a scale of 0.5.
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(200);
    mockLaneRects([
      { top: 100, height: 100 }, // column, at half size
      { top: 105, height: 38 }, // lane A: the unscaled 110 / 76, halved
      { top: 155, height: 20 }, // lane B: the unscaled 210 / 40, halved
    ]);
    const { container } = renderWithTheme(
      <SignalChainView
        graph={{
          stages: [{ kind: "series", blocks: [b("G1")] }],
          outputs: {
            a: { type: "out1", blocks: [b("G2"), b("G3")] },
            b: { type: "out2", blocks: [b("G5")] },
          },
        }}
      />,
    );
    const cap = bracketCapOf(container);
    // The same geometry as the unscaled ForkTail case above, not half of it.
    expect(cap.style.top).toBe("39px");
    expect(cap.style.height).toBe("82px");
    vi.restoreAllMocks();
  });
});
