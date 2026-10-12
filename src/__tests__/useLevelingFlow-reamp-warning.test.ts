// src/__tests__/useLevelingFlow-reamp-warning.test.ts — the run-end re-amp warning a
// command returns (`reamp_warning`) reaches every run item of its call, on all three lanes:
// the base row's own result, and a scene/footswitch batch's RETURNED rows (its per-row
// results stream over the channel before the run-end OFF, so the return value is the only
// carrier).

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { SetupChoice, SetupOption } from "../views/level/leveling";

const WARNING = "Re-amp may still be on. Unplug and replug the unit.";

const h = vi.hoisted(() => ({
  levelPreset: vi.fn(),
  levelScenesApplyBatched: vi.fn(),
  levelFootswitchesApply: vi.fn(),
  cancelPresetLeveling: vi.fn(),
  cancelSceneLeveling: vi.fn(),
  cancelFootswitchLeveling: vi.fn(),
}));
vi.mock("../lib/invoke", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/invoke")>()),
  ...h,
}));

import {
  useLevelingFlow,
  type UseLevelingFlowDeps,
} from "../views/level/useLevelingFlow";
import { resetLevelFlow } from "../views/level/levelRunStore";

// The leveling wizard lives in a module store that outlives a mount; start each case clean.
beforeEach(() => {
  resetLevelFlow();
});

const deps: UseLevelingFlowDeps = {
  rows: [{ slot: 0, name: "Plexi", empty: false }],
  store: null,
  sceneInfo: new Map(),
  footswitchInfo: new Map(),
  ampCandidates: new Map([
    [
      0,
      [
        {
          groupId: "G1",
          nodeId: "amp",
          parameterId: "outputLevel",
          value: 0.5,
        },
      ],
    ],
  ]),
  blocksByIndex: new Map(),
  silenceHintByIndex: new Map(),
  targetLufsByName: () => -20,
  deselectKeys: () => {
    /* This test does not close the wizard. */
  },
  refresh: () => Promise.resolve(),
};

function option(lane: "base" | "scene" | "footswitch", index = 0): SetupOption {
  return {
    key: `${lane}:${String(index)}`,
    slot: 0,
    presetName: "Plexi",
    isBase: lane === "base",
    sceneSlot: lane === "scene" ? index : null,
    sceneName: lane,
    tag: null,
    hasScenes: true,
    ...(lane === "footswitch"
      ? {
          footswitch: {
            switchIndex: index,
            levGroupId: "G1",
            levNodeId: "drive",
            levParameterId: "level",
            sceneContext: null,
          },
        }
      : {}),
  };
}

const choices = (options: SetupOption[]): SetupChoice[] =>
  options.map((o) => ({ option: o, instId: "", targetName: "Default" }));

beforeEach(() => {
  for (const mock of Object.values(h)) mock.mockReset();
  h.levelScenesApplyBatched.mockResolvedValue([]);
  h.levelFootswitchesApply.mockResolvedValue([]);
  h.cancelPresetLeveling.mockResolvedValue(undefined);
  h.cancelSceneLeveling.mockResolvedValue(undefined);
  h.cancelFootswitchLeveling.mockResolvedValue(undefined);
});

describe("run-end re-amp warning", () => {
  it("carries a base row's own warning", async () => {
    h.levelPreset.mockResolvedValue({
      slot: 0,
      clamped: false,
      clamp_reason: null,
      predicted_lufs: -20,
      verify_lufs: -20,
      constant_c: -10,
      dynamic_spread_lu: 1,
      true_peak_dbtp: null,
      base_boost: null,
      reamp_warning: WARNING,
    });
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(choices([option("base")]));
    });
    await waitFor(() => {
      expect(result.current.run.done).toBe(true);
    });
    expect(result.current.run.items[0].reampWarning).toBe(WARNING);
  });

  it.each(["scene", "footswitch"] as const)(
    "stamps a %s batch's returned warning on every row of the batch",
    async (lane) => {
      const mock =
        lane === "scene" ? h.levelScenesApplyBatched : h.levelFootswitchesApply;
      mock.mockResolvedValue([{ reamp_warning: WARNING }]);
      const { result } = renderHook(() => useLevelingFlow(deps));
      act(() => {
        result.current.onSetupStart(
          choices([option(lane, 0), option(lane, 1)]),
        );
      });
      await waitFor(() => {
        expect(result.current.run.done).toBe(true);
      });
      for (const row of result.current.run.items) {
        expect(row.reampWarning).toBe(WARNING);
      }
    },
  );

  it("leaves rows unmarked when the OFF read back", async () => {
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(choices([option("scene", 0)]));
    });
    await waitFor(() => {
      expect(result.current.run.done).toBe(true);
    });
    expect(result.current.run.items[0].reampWarning).toBeUndefined();
  });
});
