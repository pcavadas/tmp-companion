// src/__tests__/useLevelingFlow-base-anchor.test.ts — A7/A8: the scene-group dispatch
// keeps the wizard's Base job alive through the batch as a `baseAnchor` so the
// headroom trade (`headroom_trade.rs`, PR #144) can fire on a wizard run — it was
// previously unreachable because the wizard levels Base via the separate `levelPreset`
// lane and the batch never carried a base job at all.
//
// Drives `useLevelingFlow` with `levelPreset` + `levelScenesApplyBatched` mocked and
// asserts the batch call:
//   - carries `baseAnchor: { targetLufs }` set to the SAME preset's base RunItem's own
//     target (not the scene's) when the run includes a base row;
//   - OMITS the `baseAnchor` key entirely (not `null`) when the run has no base row.

import { describe, it, expect, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";

import type { SceneLevelProgressItem } from "../lib/invoke";

const h = vi.hoisted(() => ({
  levelPreset: vi.fn(),
  levelScenesApplyBatched: vi.fn(),
}));

vi.mock("../lib/invoke", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/invoke")>();
  return {
    ...actual,
    levelPreset: h.levelPreset,
    levelScenesApplyBatched: h.levelScenesApplyBatched,
  };
});

// Imported AFTER the mock so the hook picks up the mocked seam.
import { useLevelingFlow } from "../views/level/useLevelingFlow";
import type { SetupChoice, SetupOption } from "../views/level/leveling";

function baseOption(): SetupOption {
  return {
    key: "p0",
    slot: 0,
    presetName: "Friedman HBE",
    isBase: true,
    sceneSlot: null,
    sceneName: "Base Preset",
    tag: "BASE",
    hasScenes: true,
  };
}

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

const BASE_RESULT = {
  slot: 0,
  scene_slot: null,
  ref_level: 1,
  measured_lufs: -18,
  constant_c: -15,
  final_level: 0.5,
  target_lufs: -18,
  predicted_lufs: -18,
  clamped: false,
  saved: true,
  verify_lufs: null,
  iterations: 1,
  dynamic_spread_lu: null,
  clamp_reason: null,
  verify_by_ear: false,
  previous_level: 0.3,
  true_peak_dbtp: -3,
  persist_mismatch: null,
  clamp_kind: null,
  trade: null,
};

const deps = {
  rows: [{ slot: 0, name: "Friedman HBE", empty: false }],
  store: null,
  sceneInfo: new Map(),
  footswitchInfo: new Map(),
  ampCandidates: new Map([
    [
      0,
      [
        {
          groupId: "g",
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
  // Distinguishes the base row's own target from the scene row's — proves the anchor
  // reads the BASE item's target, not the scene's.
  targetLufsByName: (name: string | null) =>
    name === "BaseTarget" ? -18 : -24,
  deselectKeys: () => {
    /* no-op */
  },
  refresh: () => Promise.resolve(),
};

describe("useLevelingFlow — scene-group dispatch carries baseAnchor (issue 1 / A7/A8)", () => {
  it("includes baseAnchor with the base row's own target when the run has a base row", async () => {
    h.levelPreset.mockResolvedValue(BASE_RESULT);
    h.levelScenesApplyBatched.mockImplementation(
      (
        _args: unknown,
        _cb: (item: SceneLevelProgressItem) => void,
      ): Promise<never[]> => Promise.resolve([]),
    );

    const { result } = renderHook(() => useLevelingFlow(deps));

    const choices: SetupChoice[] = [
      { option: baseOption(), instId: "", targetName: "BaseTarget" },
      {
        option: sceneOption(0, "Rhythm"),
        instId: "",
        targetName: "SceneTarget",
      },
    ];

    await act(async () => {
      result.current.onSetupStart(choices);
      // Flush the base row's await + the microtask queue up to the scene dispatch.
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(h.levelScenesApplyBatched).toHaveBeenCalledTimes(1);
    const [calledArgs] = h.levelScenesApplyBatched.mock.calls[0] as [
      { baseAnchor?: unknown },
    ];
    expect(calledArgs.baseAnchor).toEqual({ targetLufs: -18 });
  });

  it("omits the baseAnchor key entirely when the run has no base row", async () => {
    h.levelPreset.mockReset();
    h.levelScenesApplyBatched.mockReset();
    h.levelScenesApplyBatched.mockImplementation(
      (
        _args: unknown,
        _cb: (item: SceneLevelProgressItem) => void,
      ): Promise<never[]> => Promise.resolve([]),
    );

    const { result } = renderHook(() => useLevelingFlow(deps));

    const choices: SetupChoice[] = [
      {
        option: sceneOption(0, "Rhythm"),
        instId: "",
        targetName: "SceneTarget",
      },
    ];

    await act(async () => {
      result.current.onSetupStart(choices);
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(h.levelPreset).not.toHaveBeenCalled();
    expect(h.levelScenesApplyBatched).toHaveBeenCalledTimes(1);
    const [calledArgs] = h.levelScenesApplyBatched.mock.calls[0] as [object];
    expect(calledArgs).not.toHaveProperty("baseAnchor");
  });
});
