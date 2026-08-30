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
    sceneContext: null,
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
    // The row's own short status + its reading — two separate cells, not one
    // concatenated label.
    expect(screen.getByText("off target")).toBeTruthy();
    expect(screen.getByText("−24.3")).toBeTruthy();
    // The clamp copy must not leak onto a row that still has knob room.
    expect(screen.queryByText("as loud as it goes")).toBeNull();
  });

  it("carries its own remedy message and inline fix link, distinct from clamped", () => {
    renderSummary([fsRow({})]);
    expect(
      screen.getByText(
        /ran out of tries. Running it again usually finishes the job/,
      ),
    ).toBeTruthy();
    expect(screen.queryByText(/already all the way up/)).toBeNull();
    // The row's own inline fix link — "another run", not "a lower target".
    expect(screen.getByText("Run again")).toBeTruthy();
    expect(screen.queryByText("Level lower")).toBeNull();
  });

  it("offers the batch re-run control, not the clamp remedy", () => {
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

  it("still shows a genuinely clamped row as clamped, not off target", () => {
    renderSummary([fsRow({ outcome: "clamped", value: -28.1 })]);
    expect(screen.getByText("as loud as it goes")).toBeTruthy();
    expect(screen.getByText("−28.1")).toBeTruthy();
    expect(screen.queryByText("off target")).toBeNull();
  });
});
