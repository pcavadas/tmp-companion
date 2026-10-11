// src/__tests__/SummaryReampWarning.test.tsx — a run whose run-end re-amp OFF could not be
// read back (`LevelResult.reamp_warning`) shows ONE banner in the backend's own words, and a
// run without it shows none.

import { describe, it, expect } from "vitest";
import { screen } from "@testing-library/react";

import { base, renderSummary } from "./summaryTestUtils";

const WARNING = "Re-amp may still be on. Unplug and replug the unit.";

describe("Summary re-amp warning", () => {
  it("shows one banner when any row carries the warning", () => {
    renderSummary([
      base({ key: "p3", reampWarning: WARNING }),
      base({ key: "p4", slot: 4, presetName: "Clean", reampWarning: WARNING }),
    ]);
    expect(screen.getByText("Re-amp may still be on")).toBeInTheDocument();
    expect(screen.getAllByText(WARNING)).toHaveLength(1);
  });

  it("shows nothing when the OFF read back", () => {
    renderSummary([base({ key: "p3" })]);
    expect(screen.queryByText("Re-amp may still be on")).toBeNull();
  });
});
