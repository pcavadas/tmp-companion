// src/__tests__/TradeDisclosure.test.tsx — the headroom-trade disclosure (D4/D5), rendered
// through SummaryBody exactly as production does (`tradesBySlot` → one <TradeDisclosure>
// per traded slot). This is component-level coverage on purpose: `level_scenes_apply_batched`
// streams its per-row outcome over a Tauri Channel the offline e2e HTTP bridge no-ops
// (`.claude/rules/e2e.md`'s Channel-streaming seam), so the RENDERED Summary a trade lands on
// is not reachable through the Playwright wizard flow offline — `e2e/specs/level-trade.spec.ts`
// proves the WIRE shape (raw invoke, the seam's sanctioned twin); this file proves the
// RENDERING, including the `applied:false` "would trade" preview form nothing else covers.

import { describe, it, expect } from "vitest";
import { screen } from "@testing-library/react";

import { base, renderSummary } from "./summaryTestUtils";
import type { TradeSummary } from "../lib/types";

const landedTrade: TradeSummary = {
  applied: true,
  raise_db: 4.4,
  previous_preset_level: 0.6,
  preset_level: 1.0,
  base_amps: [
    {
      group_id: "G1",
      node_id: "ACD_HiwattDR103CanMod",
      parameter_id: "outputLevel",
      previous_value: 0.69,
      value: 0.42,
    },
  ],
  cap: "preset_level_max",
  benefiting: [{ kind: "scene", sceneSlot: 2 }],
};

const advisoryTrade: TradeSummary = {
  ...landedTrade,
  applied: false,
  base_amps: [{ ...landedTrade.base_amps[0], value: null }],
};

describe("TradeDisclosure — rendered through SummaryBody", () => {
  it("a LANDED trade reads 'Headroom traded', shows the solved fader move and the cap note", () => {
    renderSummary([
      base({ key: "p404", slot: 404, trade: landedTrade }),
      base({
        key: "s404:2",
        slot: 404,
        isBase: false,
        sceneSlot: 2,
        sceneName: "Lead",
        outcome: "clamped",
        trade: landedTrade,
      }),
    ]);
    expect(screen.getByText(/Headroom traded/)).toBeInTheDocument();
    // preset level move: previous → raised, both linear (fmtLinear, 3 decimals).
    expect(screen.getByText(/0\.600.*→.*1\.000/)).toBeInTheDocument();
    // the solved fader move — a real number, never the advisory's "?" placeholder.
    expect(screen.getByText(/0\.690.*→.*0\.420/)).toBeInTheDocument();
    // benefiting sound resolved to its sibling's OWN name, not a positional fallback.
    expect(screen.getByText(/for Lead/)).toBeInTheDocument();
    expect(
      screen.getByText(/capped at the preset level.s maximum/),
    ).toBeInTheDocument();
  });

  it("an ADVISORY (applied:false) trade reads 'Would trade headroom' and never invents a solved fader value", () => {
    renderSummary([
      base({ key: "p404", slot: 404, trade: advisoryTrade }),
      base({
        key: "s404:2",
        slot: 404,
        isBase: false,
        sceneSlot: 2,
        sceneName: "Lead",
        outcome: "clamped",
        trade: advisoryTrade,
      }),
    ]);
    expect(screen.getByText(/Would trade headroom/)).toBeInTheDocument();
    expect(screen.queryByText(/Headroom traded/)).not.toBeInTheDocument();
    // the fader's PREVIOUS value is still known (the restore anchor); the SOLVED half
    // renders "?" — an advisory solved nothing (TradeDisclosure.tsx's own contract).
    expect(screen.getByText(/0\.690.*→.*\?/)).toBeInTheDocument();
  });

  it("de-dupes to ONE disclosure per slot even though every row of the batch carries the same trade", () => {
    renderSummary([
      base({ key: "p404", slot: 404, trade: landedTrade }),
      base({
        key: "s404:2",
        slot: 404,
        isBase: false,
        sceneSlot: 2,
        sceneName: "Lead",
        outcome: "clamped",
        trade: landedTrade,
      }),
    ]);
    expect(screen.getAllByText(/Headroom traded/).length).toBe(1);
  });
});
