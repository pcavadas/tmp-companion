// src/__tests__/useLevelingFlow-reamp-warning.test.ts — the run-end re-amp verdict a
// command returns reaches every run item of its call, on all three lanes — the typed
// flag (what the screen reads) and the warning text (the log's): the base row's own
// result, and a scene/footswitch batch's RETURN, where the verdict rides beside the rows
// (its per-row results stream over the channel before the run-end OFF, and a stopped or
// all-failed batch returns no rows at all). The Summary's restart Notice reads the FLOW's
// latest verdict, not a row's flag: a Try again whose commands read re-amp back off clears
// it, and one that never reached the unit leaves it as it was.

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import type { SetupOption } from "../views/level/leveling";
import { useLevelingFlow } from "../views/level/useLevelingFlow";
import {
  getLevelFlow,
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
  rejectWith,
} from "./commandMocks";

const WARNING = "Re-amp may still be on. Restart your Tone Master Pro.";

const deps = makeLevelingFlowDeps();

const summaryCtx: SummaryContext = {
  stopped: true,
  reampStillOn: false,
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
      expect(result.current.reampStillOn).toBe(true);
      expect(
        buildSummary(result.current.results, {
          ...summaryCtx,
          reampStillOn: result.current.reampStillOn,
        }).notice,
      ).toEqual({
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

// The restart Notice follows the unit's state now, not a row an earlier run marked: every
// leveling command ends by reading re-amp back, so the latest run that sent one decides.
describe("the restart Notice after a Try again", () => {
  /** The Summary's Notice for the flow as it stands. */
  const noticeNow = () => {
    const flow = getLevelFlow();
    return buildSummary(flow.results, {
      ...summaryCtx,
      stopped: false,
      reampStillOn: flow.reampStillOn,
    }).notice;
  };
  const restart = {
    tone: "caution",
    title: null,
    text: SUMMARY_TEXT.reampStillOn,
  };

  it("goes once a Try again reads re-amp back off", async () => {
    let presetCalls = 0;
    mockCommands({
      // The main sound's run leaves re-amp on; the scene's read it back off.
      level_preset: () => {
        presetCalls++;
        return makeLevelResult({
          reamp_still_on: true,
          reamp_warning: WARNING,
        });
      },
      level_scenes_apply_batched: () =>
        makeBatchRun([makeLevelResult({ scene_slot: 0 })]),
    });
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(
        makeChoices([option("base"), option("scene", 0)]),
      );
    });
    await waitFor(() => {
      expect(isRunOver(result.current.run.phase)).toBe(true);
    });
    expect(result.current.reampStillOn).toBe(true);
    expect(noticeNow()).toEqual(restart);

    // Try again on the scene alone: its command reads re-amp back off. The main sound's
    // row keeps its own flag from the first run, and the Notice still goes.
    act(() => {
      result.current.onTryAgain(["s0:0"]);
    });
    await waitFor(() => {
      expect(result.current.run.items.map((r) => r.key)).toEqual(["s0:0"]);
      expect(isRunOver(result.current.run.phase)).toBe(true);
    });
    expect(presetCalls).toBe(1);
    expect(
      result.current.results.find((r) => r.key === "p0")?.reampStillOn,
    ).toBe(true);
    expect(result.current.reampStillOn).toBe(false);
    expect(noticeNow()).toBeNull();
  });

  it("stays when a Try again never reaches the unit", async () => {
    mockCommands({
      level_preset: () =>
        makeLevelResult({ reamp_still_on: true, reamp_warning: WARNING }),
      // A scene of a preset the backup scan missed: its live block read fails, so the
      // run skips it before any leveling command, then and on the Try again.
      list_level_blocks: () => rejectWith(new Error("Device not connected")),
    });
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(
        makeChoices([option("base"), makeSceneOption(0, { slot: 1 })]),
      );
    });
    await waitFor(() => {
      expect(isRunOver(result.current.run.phase)).toBe(true);
    });
    expect(noticeNow()).toEqual(restart);

    act(() => {
      result.current.onTryAgain(["s1:0"]);
    });
    await waitFor(() => {
      expect(result.current.run.items.map((r) => r.key)).toEqual(["s1:0"]);
      expect(isRunOver(result.current.run.phase)).toBe(true);
    });
    expect(result.current.run.items[0].skipKind).toBe("preset_unreadable");
    expect(result.current.reampStillOn).toBe(true);
    expect(noticeNow()).toEqual(restart);
  });
});
