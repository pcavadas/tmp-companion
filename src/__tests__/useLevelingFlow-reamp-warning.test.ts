// src/__tests__/useLevelingFlow-reamp-warning.test.ts — the run-end re-amp warning a
// command returns (`reamp_warning`) reaches every run item of its call, on all three lanes:
// the base row's own result, and a scene/footswitch batch's RETURNED rows (its per-row
// results stream over the channel before the run-end OFF, so the return value is the only
// carrier).

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import type { SetupOption } from "../views/level/leveling";
import { useLevelingFlow } from "../views/level/useLevelingFlow";
import { resetLevelFlow } from "../views/level/levelRunStore";
import {
  makeChoices,
  makeFootswitchLevelResult,
  makeFootswitchOption,
  makeLevelingFlowDeps,
  makeLevelResult,
  makeSceneOption,
  makeSetupOption,
  mockCommands,
} from "./commandMocks";

const WARNING = "Re-amp may still be on. Unplug and replug the unit.";

const deps = makeLevelingFlowDeps();

function option(lane: "base" | "scene" | "footswitch", index = 0): SetupOption {
  if (lane === "base") return makeSetupOption();
  return lane === "scene"
    ? makeSceneOption(index)
    : makeFootswitchOption(index);
}

// The leveling wizard lives in a module store that outlives a mount; start each case clean.
beforeEach(() => {
  resetLevelFlow();
  mockCommands();
});

describe("run-end re-amp warning", () => {
  it("carries a base row's own warning", async () => {
    mockCommands({
      level_preset: () =>
        makeLevelResult({
          predicted_lufs: -20,
          verify_lufs: -20,
          constant_c: -10,
          dynamic_spread_lu: 1,
          reamp_warning: WARNING,
        }),
    });
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(makeChoices([option("base")]));
    });
    await waitFor(() => {
      expect(result.current.run.done).toBe(true);
    });
    expect(result.current.run.items[0].reampWarning).toBe(WARNING);
  });

  it.each(["scene", "footswitch"] as const)(
    "stamps a %s batch's returned warning on every row of the batch",
    async (lane) => {
      mockCommands(
        lane === "scene"
          ? {
              level_scenes_apply_batched: () => [
                makeLevelResult({ reamp_warning: WARNING }),
              ],
            }
          : {
              level_footswitches_apply: () => [
                makeFootswitchLevelResult({ reamp_warning: WARNING }),
              ],
            },
      );
      const { result } = renderHook(() => useLevelingFlow(deps));
      act(() => {
        result.current.onSetupStart(
          makeChoices([option(lane, 0), option(lane, 1)]),
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
      result.current.onSetupStart(makeChoices([option("scene", 0)]));
    });
    await waitFor(() => {
      expect(result.current.run.done).toBe(true);
    });
    expect(result.current.run.items[0].reampWarning).toBeUndefined();
  });
});
