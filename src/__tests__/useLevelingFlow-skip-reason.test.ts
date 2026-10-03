import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { SceneLevelProgressItem } from "../lib/invoke";
import type { SetupChoice, SetupOption } from "../views/level/leveling";

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
function choices(options: SetupOption[]): SetupChoice[] {
  return options.map((option) => ({
    option,
    instId: "",
    targetName: "Default",
  }));
}

beforeEach(() => {
  for (const mock of Object.values(h)) mock.mockReset();
  h.levelScenesApplyBatched.mockResolvedValue([]);
  h.levelFootswitchesApply.mockResolvedValue([]);
  h.cancelPresetLeveling.mockResolvedValue(undefined);
  h.cancelSceneLeveling.mockResolvedValue(undefined);
  h.cancelFootswitchLeveling.mockResolvedValue(undefined);
});

describe("leveling failure reasons", () => {
  it.each([
    "cannot hold endpoint at unity",
    new Error("cannot hold endpoint at unity"),
  ])("retains a base command rejection: %s", async (error) => {
    h.levelPreset.mockRejectedValue(error);
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
      const mock =
        lane === "scene" ? h.levelScenesApplyBatched : h.levelFootswitchesApply;
      mock.mockRejectedValue(new Error("device disconnected during setup"));
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
    h.levelScenesApplyBatched.mockImplementation(
      (_args: unknown, cb: (item: SceneLevelProgressItem) => void) => {
        cb({
          sceneSlot: 0,
          status: "error",
          result: null,
          message: "scene shares base knobs",
        });
        return Promise.reject(new Error("batch interrupted"));
      },
    );
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
    h.levelScenesApplyBatched.mockImplementation(
      () =>
        new Promise<never>((_resolve, fail) => {
          reject = fail;
        }),
    );
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
      expect(h.levelScenesApplyBatched).not.toHaveBeenCalled();
    },
  );
});
