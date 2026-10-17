// src/__tests__/useLevelingFlow-reamp-warning.test.ts — the run-end re-amp verdict a
// command returns reaches every run item of its call, on all three lanes — the typed
// flag (what the screen reads) and the warning text (the log's): the base row's own
// result, and a scene/footswitch batch's RETURN, where the verdict rides beside the rows
// (its per-row results stream over the channel before the run-end OFF, and a stopped or
// all-failed batch returns no rows at all).

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import type { SetupOption } from "../views/level/leveling";
import { useLevelingFlow } from "../views/level/useLevelingFlow";
import {
  isRunOver,
  requestStop,
  resetLevelFlow,
} from "../views/level/levelRunStore";
import {
  buildSummary,
  SUMMARY_TEXT,
  type SummaryContext,
} from "../views/level/summaryModel";
import {
  makeBatchRun,
  makeChoices,
  makeFootswitchLevelResult,
  makeFootswitchOption,
  makeLevelingFlowDeps,
  makeLevelResult,
  makeSceneOption,
  makeSetupOption,
  mockCommands,
} from "./commandMocks";

const WARNING = "Re-amp may still be on. Restart your Tone Master Pro.";

const deps = makeLevelingFlowDeps();

const summaryCtx: SummaryContext = {
  stopped: true,
  targetLufsByName: () => -26,
  sceneNamesOf: () => [],
  enablingScenesOf: () => null,
  sceneOverrides: {},
  hookKeyOf: (it) => it.key,
};

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
          reamp_still_on: true,
        }),
    });
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(makeChoices([option("base")]));
    });
    await waitFor(() => {
      expect(isRunOver(result.current.run.phase)).toBe(true);
    });
    expect(result.current.run.items[0].reampWarning).toBe(WARNING);
    expect(result.current.run.items[0].reampStillOn).toBe(true);
  });

  it.each(["scene", "footswitch"] as const)(
    "stamps a %s batch's returned warning on every row of the batch",
    async (lane) => {
      mockCommands(
        lane === "scene"
          ? {
              level_scenes_apply_batched: () =>
                makeBatchRun([makeLevelResult()], {
                  reampWarning: WARNING,
                  reampStillOn: true,
                }),
            }
          : {
              level_footswitches_apply: () =>
                makeBatchRun([makeFootswitchLevelResult()], {
                  reampWarning: WARNING,
                  reampStillOn: true,
                }),
            },
      );
      const { result } = renderHook(() => useLevelingFlow(deps));
      act(() => {
        result.current.onSetupStart(
          makeChoices([option(lane, 0), option(lane, 1)]),
        );
      });
      await waitFor(() => {
        expect(isRunOver(result.current.run.phase)).toBe(true);
      });
      for (const row of result.current.run.items) {
        expect(row.reampWarning).toBe(WARNING);
        expect(row.reampStillOn).toBe(true);
      }
    },
  );

  // A Stop during the ceiling prepass returns no rows; the OFF that never read back OFF must
  // still reach the Summary as the restart Notice.
  it.each(["scene", "footswitch"] as const)(
    "a %s batch stopped before any row finished still reaches the Summary's re-amp Notice",
    async (lane) => {
      const stopped = () => {
        requestStop();
        return makeBatchRun<never>([], {
          reampWarning: WARNING,
          reampStillOn: true,
        });
      };
      mockCommands(
        lane === "scene"
          ? { level_scenes_apply_batched: stopped }
          : { level_footswitches_apply: stopped },
      );
      const { result } = renderHook(() => useLevelingFlow(deps));
      act(() => {
        result.current.onSetupStart(
          makeChoices([option(lane, 0), option(lane, 1)]),
        );
      });
      await waitFor(() => {
        expect(result.current.run.phase).toBe("stopped");
      });
      expect(result.current.stage).toBe("summary");
      for (const row of result.current.results)
        expect(row.reampStillOn).toBe(true);
      expect(buildSummary(result.current.results, summaryCtx).notice).toEqual({
        tone: "caution",
        title: null,
        text: SUMMARY_TEXT.reampStillOn,
      });
    },
  );

  it("leaves rows unmarked when the OFF read back", async () => {
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(makeChoices([option("scene", 0)]));
    });
    await waitFor(() => {
      expect(isRunOver(result.current.run.phase)).toBe(true);
    });
    expect(result.current.run.items[0].reampWarning).toBeUndefined();
    expect(result.current.run.items[0].reampStillOn).toBeUndefined();
  });
});
