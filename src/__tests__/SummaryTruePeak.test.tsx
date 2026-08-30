// Summary "may clip" chip — a Base row whose PREDICTED true peak (an estimate from
// the one-shot presetLevel solve, never a re-measurement) lands above −1 dBTP gets a
// warn chip + the run gets one explanatory footnote.

import { describe, it, expect } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { base, renderSummary } from "./summaryTestUtils";

describe("Summary true-peak warn chip", () => {
  it("flags a row predicted to clip", async () => {
    renderSummary([base({ truePeakDbtp: -0.2 })]);
    // A truePeak-only flag on an otherwise "done" row doesn't make the group a
    // "problem" group, so it stays collapsed — expand it to reach the row's chip.
    await userEvent.click(screen.getByText("Guitar"));
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
