// src/__tests__/useLevelingFlow-skip-reason.test.ts — a failed command's own reason
// reaches the rows it skipped, on all three lanes; a cancel or a client-side skip
// invents none.

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import type { SetupOption } from "../views/level/leveling";
import { useLevelingFlow } from "../views/level/useLevelingFlow";
import { resetLevelFlow } from "../views/level/levelRunStore";
import {
  fired,
  makeChoices,
  makeFootswitchOption,
  makeLevelingFlowDeps,
  makeSceneOption,
  makeSetupOption,
  mockCommands,
  rejectWith,
} from "./commandMocks";

const deps = makeLevelingFlowDeps();

function option(lane: "base" | "scene" | "footswitch", index = 0): SetupOption {
  if (lane === "base") return makeSetupOption();
  return lane === "scene"
    ? makeSceneOption(index)
    : makeFootswitchOption(index);
}
const choices = makeChoices;

// The leveling wizard lives in a module store that outlives a mount; start each case clean.
beforeEach(() => {
  resetLevelFlow();
  mockCommands();
});

describe("leveling failure reasons", () => {
  it.each([
    "cannot hold endpoint at unity",
    new Error("cannot hold endpoint at unity"),
  ])("retains a base command rejection: %s", async (error) => {
    mockCommands({ level_preset: () => rejectWith(error) });
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(choices([option("base")]));
    });
    await waitFor(() => {
      expect(result.current.run.done).toBe(true);
    });
    expect(result.current.run.items[0]).toMatchObject({
      outcome: "skipped",
      skipReason: "cannot hold endpoint at unity",
    });
  });

  it.each(["scene", "footswitch"] as const)(
    "retains a whole %s batch rejection on unresolved rows",
    async (lane) => {
      const fail = () =>
        rejectWith(new Error("device disconnected during setup"));
      mockCommands(
        lane === "scene"
          ? { level_scenes_apply_batched: fail }
          : { level_footswitches_apply: fail },
      );
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
        expect(row).toMatchObject({
          outcome: "skipped",
          skipReason: "device disconnected during setup",
        });
      }
    },
  );

  it("preserves a resolved row's own reason when the rest of its batch fails", async () => {
    mockCommands({
      level_scenes_apply_batched: ({ onResult }) => {
        onResult.onmessage({
          sceneSlot: 0,
          status: "error",
          result: null,
          message: "scene shares base knobs",
        });
        return rejectWith(new Error("batch interrupted"));
      },
    });
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(
        choices([option("scene", 0), option("scene", 1)]),
      );
    });
    await waitFor(() => {
      expect(result.current.run.done).toBe(true);
    });
    expect(result.current.run.items.map((row) => row.skipReason)).toEqual([
      "scene shares base knobs",
      "batch interrupted",
    ]);
  });

  it("keeps cancelled unresolved rows queued without inventing a failure reason", async () => {
    let reject: (reason: Error) => void = () => {
      throw new Error("batch not started");
    };
    mockCommands({
      level_scenes_apply_batched: () =>
        new Promise<never>((_resolve, fail) => {
          reject = fail;
        }),
    });
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(
        choices([option("scene", 0), option("scene", 1)]),
      );
    });
    act(() => {
      result.current.onRunCancel();
      reject(new Error("cancelled"));
    });
    await waitFor(() => {
      expect(result.current.run.done).toBe(true);
    });
    expect(result.current.run.stopped).toBe(true);
    for (const row of result.current.run.items) {
      expect(row.status).toBe("queued");
      expect(row.skipReason).toBeUndefined();
    }
  });

  it.each(["missing scene slot", "no amp candidate"])(
    "keeps client-side %s skips generic",
    async (scenario) => {
      const selected = option("scene");
      if (scenario === "missing scene slot") selected.sceneSlot = null;
      const { result } = renderHook(() =>
        useLevelingFlow({ ...deps, ampCandidates: new Map([[0, []]]) }),
      );
      act(() => {
        result.current.onSetupStart(choices([selected]));
      });
      await waitFor(() => {
        expect(result.current.run.done).toBe(true);
      });
      expect(result.current.run.items[0].outcome).toBe("skipped");
      expect(result.current.run.items[0].skipReason).toBeUndefined();
      expect(fired("level_scenes_apply_batched")).toBe(false);
    },
  );
});
