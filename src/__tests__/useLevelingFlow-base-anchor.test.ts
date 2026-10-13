// src/__tests__/useLevelingFlow-base-anchor.test.ts — A7/A8: the scene-group dispatch
// keeps the wizard's Base job alive through the batch as a `baseAnchor` so the
// headroom trade (`headroom_trade.rs`, PR #144) can fire on a wizard run — it was
// previously unreachable because the wizard levels Base via the separate `levelPreset`
// lane and the batch never carried a base job at all.
//
// Drives `useLevelingFlow` with `level_preset` + `level_scenes_apply_batched` mocked and
// asserts the batch call:
//   - carries `baseAnchor: { targetLufs }` set to the SAME preset's base RunItem's own
//     target (not the scene's) when the run includes a base row;
//   - OMITS the `baseAnchor` key entirely (not `null`) when the run has no base row.

import { beforeEach, describe, expect, it } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";

import { useLevelingFlow } from "../views/level/useLevelingFlow";
import type { SetupChoice } from "../views/level/leveling";
import { resetLevelFlow } from "../views/level/levelRunStore";
import {
  callsOf,
  fired,
  makeLevelingFlowDeps,
  makeLevelResult,
  makeSceneOption,
  makeSetupOption,
  mockCommands,
} from "./commandMocks";

// The leveling wizard lives in a module store that outlives a mount; start each case clean.
beforeEach(() => {
  resetLevelFlow();
  mockCommands({
    level_preset: () =>
      makeLevelResult({
        ref_level: 1,
        measured_lufs: -18,
        constant_c: -15,
        final_level: 0.5,
        target_lufs: -18,
        predicted_lufs: -18,
        verify_lufs: null,
        dynamic_spread_lu: null,
        previous_level: 0.3,
        true_peak_dbtp: -3,
      }),
  });
});

const PRESET = { slot: 0, presetName: "Friedman HBE" };

const deps = makeLevelingFlowDeps({
  rows: [{ slot: 0, name: "Friedman HBE", empty: false }],
  // Distinguishes the base row's own target from the scene row's — proves the anchor
  // reads the BASE item's target, not the scene's.
  targetLufsByName: (name: string | null) =>
    name === "BaseTarget" ? -18 : -24,
});

const sceneChoice: SetupChoice = {
  option: makeSceneOption(0, { ...PRESET, sceneName: "Rhythm" }),
  instId: "",
  targetName: "SceneTarget",
};

describe("useLevelingFlow — scene-group dispatch carries baseAnchor (issue 1 / A7/A8)", () => {
  it("includes baseAnchor with the base row's own target when the run has a base row", async () => {
    const { result } = renderHook(() => useLevelingFlow(deps));

    const choices: SetupChoice[] = [
      {
        option: makeSetupOption(PRESET),
        instId: "",
        targetName: "BaseTarget",
      },
      sceneChoice,
    ];

    act(() => {
      result.current.onSetupStart(choices);
    });
    await waitFor(() => {
      expect(result.current.run.done).toBe(true);
    });

    const batches = callsOf("level_scenes_apply_batched");
    expect(batches).toHaveLength(1);
    expect(batches[0].baseAnchor).toEqual({ targetLufs: -18 });
  });

  it("omits the baseAnchor key entirely when the run has no base row", async () => {
    const { result } = renderHook(() => useLevelingFlow(deps));

    act(() => {
      result.current.onSetupStart([sceneChoice]);
    });
    await waitFor(() => {
      expect(result.current.run.done).toBe(true);
    });

    expect(fired("level_preset")).toBe(false);
    const batches = callsOf("level_scenes_apply_batched");
    expect(batches).toHaveLength(1);
    expect(batches[0]).not.toHaveProperty("baseAnchor");
  });
});
