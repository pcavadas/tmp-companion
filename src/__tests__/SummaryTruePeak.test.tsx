// Summary "may clip" chip — a Base row whose PREDICTED true peak (an estimate from
// the one-shot presetLevel solve, never a re-measurement) lands above −1 dBTP gets a
// warn chip + the run gets one explanatory footnote.

import { describe, it, expect } from "vitest";
import { screen } from "@testing-library/react";

import { base, renderSummary } from "./summaryTestUtils";

describe("Summary true-peak warn chip", () => {
  it("flags a row predicted to clip", () => {
    renderSummary([base({ truePeakDbtp: -0.2 })]);
    // One chip on the row + one in the footnote's leading icon.
    expect(screen.getAllByText("may clip").length).toBe(2);
    expect(screen.getByText(/estimated to peak above −1 dBTP/i)).toBeTruthy();
  });

  it("stays quiet for a row safely under the threshold", () => {
    renderSummary([base({ truePeakDbtp: -3 })]);
    expect(screen.queryByText("may clip")).toBeNull();
  });

  it("stays quiet when no prediction was made (non-Base / scene paths)", () => {
    renderSummary([base({ truePeakDbtp: null })]);
    expect(screen.queryByText("may clip")).toBeNull();
  });
});
