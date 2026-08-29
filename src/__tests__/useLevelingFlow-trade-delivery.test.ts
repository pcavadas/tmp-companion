// src/__tests__/useLevelingFlow-trade-delivery.test.ts — A8/F5: a headroom-trade
// summary stamped on a batched scene channel's "done" item must reach the RunItem the
// wizard renders. `batchResolve` (useLevelingFlow.ts) already reads `result.trade ??
// null` — this proves the delivery path end-to-end rather than trusting the read exists.

import { describe, it, expect, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";

import type { SceneLevelProgressItem } from "../lib/invoke";
import type { TradeSummary } from "../lib/types";

const h = vi.hoisted(() => ({
  levelScenesApplyBatched: vi.fn(),
}));

vi.mock("../lib/invoke", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/invoke")>();
  return {
    ...actual,
    levelScenesApplyBatched: h.levelScenesApplyBatched,
  };
});

// Imported AFTER the mock so the hook picks up the mocked seam.
import { useLevelingFlow } from "../views/level/useLevelingFlow";
import type { SetupChoice, SetupOption } from "../views/level/leveling";

function sceneOption(sceneSlot: number, name: string): SetupOption {
  return {
    key: `s0:${String(sceneSlot)}`,
    slot: 0,
    presetName: "Friedman HBE",
    isBase: false,
    sceneSlot,
    sceneName: name,
    tag: `FS${String(sceneSlot + 1)}`,
    hasScenes: true,
  };
}

const TRADE: TradeSummary = {
  applied: true,
  raise_db: 4.437,
  previous_preset_level: 0.3672,
  preset_level: 0.612,
  base_amps: [
    {
      group_id: "G1",
      node_id: "ampA",
      parameter_id: "outputLevel",
      previous_value: 1.0,
      value: 0.62,
    },
  ],
  cap: null,
  benefiting: [{ kind: "scene", sceneSlot: 0 }],
};

describe("useLevelingFlow — headroom-trade summary reaches the RunItem (A8/F5)", () => {
  it("stamps the channel done item's trade onto the resolved row", () => {
    let onResult: ((item: SceneLevelProgressItem) => void) | undefined;
    h.levelScenesApplyBatched.mockImplementation(
      (
        _args: unknown,
        cb: (item: SceneLevelProgressItem) => void,
      ): Promise<never[]> => {
        onResult = cb;
        return Promise.resolve([]);
      },
    );

    const { result } = renderHook(() =>
      useLevelingFlow({
        rows: [{ slot: 0, name: "Friedman HBE", empty: false }],
        store: null,
        sceneInfo: new Map(),
        footswitchInfo: new Map(),
        ampCandidates: new Map([
          [
            0,
            [
              {
                groupId: "G1",
                nodeId: "ampA",
                parameterId: "outputLevel",
                value: 0.5,
              },
            ],
          ],
        ]),
        baseActiveAmpCountByIndex: new Map([[0, 1]]),
        blocksByIndex: new Map(),
        silenceHintByIndex: new Map(),
        targetLufsByName: () => -20,
        deselectKeys: () => {
          /* no-op */
        },
        refresh: () => Promise.resolve(),
      }),
    );

    const choices: SetupChoice[] = [
      { option: sceneOption(0, "Rhythm"), instId: "", targetName: "Rhythm" },
    ];

    act(() => {
      result.current.onSetupStart(choices);
    });

    act(() => {
      onResult?.({
        sceneSlot: 0,
        status: "done",
        result: {
          slot: 0,
          scene_slot: 0,
          ref_level: 1,
          measured_lufs: -20,
          constant_c: -18,
          final_level: 0.62,
          target_lufs: -20,
          predicted_lufs: -20,
          clamped: false,
          saved: true,
          verify_lufs: null,
          iterations: 1,
          dynamic_spread_lu: null,
          clamp_reason: null,
          verify_by_ear: false,
          previous_level: null,
          true_peak_dbtp: null,
          persist_mismatch: null,
          clamp_kind: null,
          trade: TRADE,
        },
        message: null,
      });
    });

    const row = result.current.run.items.find((it) => it.sceneSlot === 0);
    expect(row?.trade).toEqual(TRADE);
    expect(row?.outcome).toBe("done");
  });
});
