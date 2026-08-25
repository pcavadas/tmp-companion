// src/__tests__/SummaryWetFloor.test.tsx — a footswitch clamped at its wet/mix floor
// (25% of its authored mix) earns the "by ear" chip + its own footnote line, distinct
// from a dynamics-spread flag. Split out of the deleted SummaryVerifyRow.test.tsx
// (whose verify-only-mode and scene target-mode-offset coverage pinned P2-removed
// behavior) — this cause is orthogonal to that removal and still applies.

import { describe, it, expect } from "vitest";
import { screen } from "@testing-library/react";

import { base, renderSummary } from "./summaryTestUtils";

describe("Summary wet_floor by-ear cause", () => {
  it("flags a wet-floored row with the by-ear chip and its own footnote line", () => {
    renderSummary([
      base({
        key: "f5:0",
        footswitch: {
          switchIndex: 0,
          levGroupId: "G1",
          levNodeId: "ped",
          levParameterId: "mix",
          sceneContext: null,
        },
        outcome: "clamped",
        value: -21,
        verifyByEar: "wet_floor",
      }),
    ]);
    // Two "by ear" chips render (the row's own + the footnote's icon), so assert
    // presence via getAllByText rather than a uniqueness-assuming getByText.
    expect(screen.getAllByText("by ear").length).toBeGreaterThan(0);
    expect(
      screen.getByText(/floored at 25% of its designed mix/),
    ).toBeInTheDocument();
  });
});
