// `measureLaneBracket`/`LaneBracketCap` (SignalChainView.tsx) draw the bracket
// connecting a split/join point's two lanes from DOM-measured rects rather than
// fixed constants — this is the exact mechanism this session's fix touched
// (ForkTail/JoinHead adopted SplitGroup's pre-existing measured-bracket
// technique). jsdom has no real layout, so `getBoundingClientRect` always
// returns an all-zero rect unless stubbed (same technique BlockArt.test.tsx /
// ActiveSignalChainView.test.tsx use for SVGElement.getBBox). These tests mock
// concrete rects and assert the bracket lands at the exact computed pixel
// geometry, not just "renders without crashing".

import { describe, it, expect, vi } from "vitest";
import { render } from "@testing-library/react";

import { SignalChainView } from "../views/SignalChainView";
import { ThemeProvider } from "../theme/ThemeProvider";

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
  // sibling border-box cap — see SignalChainView.tsx) to avoid picking that one.
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
    const { container } = render(
      <ThemeProvider>
        <SignalChainView
          graph={{
            template: "gtrSplit",
            stages: [{ kind: "series", blocks: [b("G1")] }],
            outputs: {
              a: { type: "out1", blocks: [b("G2"), b("G3")] },
              b: { type: "out2", blocks: [b("G5")] },
            },
          }}
        />
      </ThemeProvider>,
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
    const { container } = render(
      <ThemeProvider>
        <SignalChainView
          graph={{
            template: "gtrMicSeries",
            inputs: {
              a: { type: "guitar", blocks: [b("G1"), b("G2")] },
              b: { type: "mic", blocks: [b("M1")] },
            },
            stages: [{ kind: "series", blocks: [b("XO")] }],
          }}
        />
      </ThemeProvider>,
    );
    const cap = bracketCapOf(container);
    expect(cap.style.top).toBe("39px");
    expect(cap.style.height).toBe("82px");
    expect(cap.style.right).toBe("-9px"); // JoinHead mirrors ForkTail onto the join side.
    expect(cap.style.left).toBe("");
  });
});
