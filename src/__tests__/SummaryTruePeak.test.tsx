// Summary "may clip" chip — a Base row whose PREDICTED true peak (an estimate from
// the one-shot presetLevel solve, never a re-measurement) lands above −1 dBTP gets a
// warn chip + the run gets one explanatory footnote.

import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";

import { ThemeProvider } from "../theme/ThemeProvider";
import { SummaryBody } from "../views/overlays/SummaryBody";
import type { RunItem } from "../views/level/leveling";

const base = (over: Partial<RunItem>): RunItem => ({
  key: "p3",
  slot: 3,
  presetName: "Guitar",
  isBase: true,
  sceneSlot: null,
  sceneName: "",
  tag: null,
  footswitch: null,
  instId: "none",
  targetName: "Lead",
  label: "Guitar",
  status: "result",
  outcome: "done",
  value: -22,
  ...over,
});

const renderSummary = (items: RunItem[]) =>
  render(
    <ThemeProvider>
      <SummaryBody
        items={items}
        stopped={false}
        onAccept={() => undefined}
        onRelevel={() => undefined}
      />
    </ThemeProvider>,
  );

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
