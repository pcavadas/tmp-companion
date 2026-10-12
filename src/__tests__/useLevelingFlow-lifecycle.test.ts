// src/__tests__/useLevelingFlow-lifecycle.test.ts — BUG→GATE for the run's lifecycle.
//
// #283: a leveling run's state used to live in `useLevelingFlow`'s component state. A tab
// switch unmounts `LevelView`, so the run kept writing to the unit with no Stop the user
// could reach, and a second run could start beside it. The run now lives in the module
// store (`levelRunStore`): a remounted hook shows the same run, its Stop ends it, and a
// second start is refused while it is in flight.
//
// #285: the scene lane's fallback `listLevelBlocks` read sat outside any try, so a throw
// left the wizard on "Saving the last one…" forever with the run still marked running.
// It now skips that group with the error as the row's reason, and the run ends.
//
// Because the flow now outlives the view, a device detach closes it: its rows are keyed by
// list index, which the next connection can re-map, so a stale Summary's Re-level could
// write the wrong preset. A run in flight resets once it has wound down.

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { LevelResult } from "../lib/types";
import type { SetupChoice, SetupOption } from "../views/level/leveling";

const h = vi.hoisted(() => ({
  levelPreset: vi.fn(),
  listLevelBlocks: vi.fn(),
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
import {
  isLevelRunning,
  onDeviceDetached,
  resetLevelFlow,
} from "../views/level/levelRunStore";

const deps: UseLevelingFlowDeps = {
  rows: [{ slot: 0, name: "Plexi", empty: false }],
  store: null,
  sceneInfo: new Map(),
  footswitchInfo: new Map(),
  // No backup candidates for slot 0, so a scene row needs the live block read.
  ampCandidates: new Map(),
  blocksByIndex: new Map(),
  silenceHintByIndex: new Map(),
  targetLufsByName: () => -20,
  deselectKeys: () => {
    /* Not exercised here. */
  },
  refresh: () => Promise.resolve(),
};

function option(lane: "base" | "scene", index = 0): SetupOption {
  return {
    key: `${lane}:${String(index)}`,
    slot: 0,
    presetName: "Plexi",
    isBase: lane === "base",
    sceneSlot: lane === "scene" ? index : null,
    sceneName: lane,
    tag: null,
    hasScenes: true,
  };
}
const choices = (options: SetupOption[]): SetupChoice[] =>
  options.map((o) => ({ option: o, instId: "none", targetName: "Default" }));

const leveled = {
  clamped: false,
  clamp_reason: null,
  verify_lufs: -20,
  predicted_lufs: -20,
  dynamic_spread_lu: null,
  constant_c: -10,
  true_peak_dbtp: null,
  base_boost: null,
  reamp_warning: null,
} as unknown as LevelResult;

beforeEach(() => {
  resetLevelFlow();
  for (const mock of Object.values(h)) mock.mockReset();
  h.cancelPresetLeveling.mockResolvedValue(undefined);
  h.cancelSceneLeveling.mockResolvedValue(undefined);
  h.cancelFootswitchLeveling.mockResolvedValue(undefined);
  h.levelScenesApplyBatched.mockResolvedValue([]);
});

describe("a leveling run outlives the Level tab (#283)", () => {
  it("shows the same run after a remount, and its Stop ends it", async () => {
    let finish: (r: LevelResult) => void = () => undefined;
    h.levelPreset.mockImplementation(
      () =>
        new Promise<LevelResult>((res) => {
          finish = res;
        }),
    );

    const first = renderHook(() => useLevelingFlow(deps));
    act(() => {
      first.result.current.onSetupStart(choices([option("base")]));
    });
    await waitFor(() => {
      expect(h.levelPreset).toHaveBeenCalledTimes(1);
    });
    // The user switches tab: App renders only the active tab, so the view unmounts.
    first.unmount();

    // …and comes back. The fresh mount shows the in-flight run, not a closed wizard.
    const second = renderHook(() => useLevelingFlow(deps));
    expect(second.result.current.stage).toBe("run");
    expect(second.result.current.run.done).toBe(false);
    expect(second.result.current.run.items[0].status).toBe("active");

    // Its Stop reaches the run: every lane is cancelled and the run reports stopped.
    act(() => {
      second.result.current.onRunCancel();
    });
    expect(h.cancelPresetLeveling).toHaveBeenCalledTimes(1);
    expect(second.result.current.run.stopping).toBe(true);
    act(() => {
      finish(leveled);
    });
    await waitFor(() => {
      expect(second.result.current.run.done).toBe(true);
    });
    expect(second.result.current.run.stopped).toBe(true);
    expect(isLevelRunning()).toBe(false);
  });

  it("refuses a second run while one is in flight", async () => {
    let finish: (r: LevelResult) => void = () => undefined;
    h.levelPreset.mockImplementation(
      () =>
        new Promise<LevelResult>((res) => {
          finish = res;
        }),
    );
    const { result, unmount } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(choices([option("base")]));
    });
    await waitFor(() => {
      expect(h.levelPreset).toHaveBeenCalledTimes(1);
    });
    unmount();

    const again = renderHook(() => useLevelingFlow(deps));
    act(() => {
      again.result.current.onSetupStart(choices([option("base")]));
    });
    expect(h.levelPreset).toHaveBeenCalledTimes(1);
    // Closing the wizard is refused mid-run too, so the run can't be hidden.
    act(() => {
      again.result.current.onCancel();
    });
    expect(again.result.current.stage).toBe("run");

    act(() => {
      finish(leveled);
    });
    await waitFor(() => {
      expect(again.result.current.run.done).toBe(true);
    });
    expect(again.result.current.run.stopped).toBe(false);
  });
});

describe("a throwing live block read ends the run (#285)", () => {
  it("skips the scene group with the error and releases the run", async () => {
    h.listLevelBlocks.mockRejectedValue(new Error("Device not connected"));
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(
        choices([option("scene", 0), option("scene", 1)]),
      );
    });
    await waitFor(() => {
      expect(result.current.run.done).toBe(true);
    });
    expect(result.current.run.stopped).toBe(false);
    expect(result.current.run.stopping).toBe(false);
    for (const row of result.current.run.items) {
      expect(row).toMatchObject({
        status: "result",
        outcome: "skipped",
        skipReason: "Device not connected",
      });
    }
    expect(h.levelScenesApplyBatched).not.toHaveBeenCalled();
    expect(isLevelRunning()).toBe(false);

    // The run is released: a new one starts.
    h.levelPreset.mockResolvedValue(leveled);
    act(() => {
      result.current.onSetupStart(choices([option("base")]));
    });
    await waitFor(() => {
      expect(h.levelPreset).toHaveBeenCalledTimes(1);
    });
  });
});

describe("a device detach closes the flow", () => {
  it("closes an idle Summary at once", async () => {
    h.levelPreset.mockResolvedValue(leveled);
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(choices([option("base")]));
    });
    await waitFor(() => {
      expect(result.current.run.done).toBe(true);
    });
    act(() => {
      result.current.onRunComplete();
    });
    expect(result.current.stage).toBe("summary");

    act(() => {
      onDeviceDetached();
    });
    expect(result.current.stage).toBe("closed");
    expect(result.current.run.items).toHaveLength(0);
  });

  it("lets a run in flight end, then closes instead of showing its Summary", async () => {
    let finish: (r: LevelResult) => void = () => undefined;
    h.levelPreset.mockImplementation(
      () =>
        new Promise<LevelResult>((res) => {
          finish = res;
        }),
    );
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(choices([option("base")]));
    });
    await waitFor(() => {
      expect(h.levelPreset).toHaveBeenCalledTimes(1);
    });

    act(() => {
      onDeviceDetached();
    });
    // The run is not torn down under the in-flight command.
    expect(result.current.stage).toBe("run");
    expect(isLevelRunning()).toBe(true);

    act(() => {
      finish(leveled);
    });
    await waitFor(() => {
      expect(result.current.stage).toBe("closed");
    });
    expect(result.current.run.done).toBe(false);
    expect(isLevelRunning()).toBe(false);
  });
});
