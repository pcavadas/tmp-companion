// Summary base-boost disclosure (Phase 2, the plumes/BD2/OCD-class regression fix): a
// base row whose pair plan entered the `Boost` regime gets its own sentence — distinct
// from the removed trade disclosure, this one reports a PERMANENT change to the saved
// preset (the amp's own output level, not just presetLevel), so it renders regardless
// of the row's outcome.

import { describe, it, expect } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { base, renderSummary } from "./summaryTestUtils";
import type { BaseBoostSummary } from "../lib/types";

const boost = (over: Partial<BaseBoostSummary>): BaseBoostSummary => ({
  applied: false,
  preset_level: 1.0,
  base_amps: [
    {
      group_id: "G1",
      node_id: "amp1",
      parameter_id: "outputLevel",
      previous_value: 0.28,
      // The planner's SEED (`LevelPairPlan.fader_target`) — present even on an
      // unapplied plan, matching the plan doc's own worked example (slot 27: 0.28 → 0.51).
      value: 0.51,
    },
  ],
  ...over,
});

describe("Summary base-boost disclosure", () => {
  it("states the before/after amplitude, past tense, when the boost was applied and saved", async () => {
    renderSummary([
      base({
        outcome: "done",
        baseBoost: boost({ applied: true }),
      }),
    ]);
    // An applied boost on an otherwise "done" row doesn't make the group a "problem"
    // group, so it stays collapsed — expand it to reach the row's own footnote.
    await userEvent.click(screen.getByText("Guitar"));
    expect(
      screen.getByText(
        /Turned this preset up as far as it goes and raised the amp.s output from 0\.28 to 0\.51 to reach the target\./,
      ),
    ).toBeInTheDocument();
  });

  it("keeps 'turned' but swaps to 'would raise' for the amp half when the boost is only advisory", () => {
    renderSummary([base({ outcome: "clamped", baseBoost: boost({}) })]);
    // A clamped row's group opens by default — no expand needed.
    expect(
      screen.getByText(
        /Turned this preset up as far as it goes and would raise the amp.s output from 0\.28 to 0\.51 to reach the target\./,
      ),
    ).toBeInTheDocument();
  });

  it("stays quiet for a row with no base-pair boost", () => {
    renderSummary([base({ outcome: "done", baseBoost: null })]);
    expect(screen.queryByText(/turn this preset up/i)).toBeNull();
  });
});
