// Summary: an UNCONVERGED row must present distinctly from a CLAMPED one. The two miss
// states differ in the user's next action — a clamped sound is at the end of its knob and
// re-running it is pointless, while an unconverged one still had knob room and simply ran
// out of measurement captures, so the same target re-run improves it. They were one flag
// (`clamped`) until the backend split them (`FootswitchLevelResult.unconverged`); this
// pins the split all the way to the copy, since an if-chain that falls through to "done"
// would silently report an unconverged row as leveled.

import { describe, it, expect } from "vitest";
import { screen } from "@testing-library/react";

import type { RunItem } from "../views/level/leveling";
import { renderSummary } from "./summaryTestUtils";

// A block-acting FOOTSWITCH row — the only lane that reports `unconverged` today.
const fsRow = (over: Partial<RunItem>): RunItem => ({
  key: "f3:0",
  slot: 3,
  presetName: "Guitar",
  isBase: false,
  sceneSlot: null,
  sceneName: "Boost",
  tag: "FS1",
  footswitch: {
    switchIndex: 0,
    levGroupId: "G1",
    levNodeId: "amp",
    levParameterId: "outputLevel",
  },
  instId: "none",
  targetName: "Lead",
  status: "result",
  outcome: "unconverged",
  value: -24.3,
  ...over,
});

describe("Summary unconverged row", () => {
  it("reports it off target, never as clamped or done", () => {
    renderSummary([fsRow({})]);
    // Exact copy pin — matches RunBody's phrase for the same state; the remedy
    // wording lives on the footer's re-run control, not this 104px cell.
    expect(screen.getByText("off target · −24.3")).toBeTruthy();
    // The clamp copy must not leak onto a row that still has knob room.
    expect(screen.queryByText(/clamped · /)).toBeNull();
  });

  it("groups and tallies it apart from clamped, and does not count as leveled", () => {
    renderSummary([fsRow({})]);
    expect(screen.getByText("Off target")).toBeTruthy();
    expect(screen.queryByText("Clamped")).toBeNull();
    expect(screen.getByText(/1 off target/)).toBeTruthy();
    // Not an all-clear: the row needs a follow-up run.
    expect(screen.getByText(/0 of 1 leveled/)).toBeTruthy();
  });

  it("offers the re-run control, not the clamp remedy", () => {
    renderSummary([fsRow({})]);
    // The advertised follow-up: another run at the same target.
    expect(
      screen.getByRole("button", { name: /Re-run off target/ }),
    ).toBeTruthy();
    // No clamp remedy — the fix is another run, not a lower target.
    expect(
      screen.queryByRole("button", { name: /Re-level clamped/ }),
    ).toBeNull();
  });

  it("still shows a genuinely clamped row as clamped", () => {
    renderSummary([fsRow({ outcome: "clamped", value: -28.1 })]);
    expect(screen.getByText(/clamped · −28\.1/)).toBeTruthy();
    expect(screen.queryByText("Off target")).toBeNull();
  });
});
