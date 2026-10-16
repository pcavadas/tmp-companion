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
// list index, which the next connection can re-map, so a stale Summary's Try again could
// write the wrong preset. A run in flight resets once it has wound down.

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import type { LevelResult } from "../lib/types";
import type { SetupOption } from "../views/level/leveling";
import { useLevelingFlow } from "../views/level/useLevelingFlow";
import {
  isLevelRunning,
  isRunOver,
  onDeviceDetached,
  resetLevelFlow,
} from "../views/level/levelRunStore";
import {
  callsOf,
  fired,
  makeChoices,
  makeLevelingFlowDeps,
  makeLevelResult,
  makeSceneOption,
  makeSetupOption,
  mockCommands,
  rejectWith,
} from "./commandMocks";

// No backup candidates for slot 0, so a scene row needs the live block read.
const deps = makeLevelingFlowDeps({ ampCandidates: new Map() });

const option = (lane: "base" | "scene", index = 0): SetupOption =>
  lane === "base" ? makeSetupOption() : makeSceneOption(index);
const choices = (options: SetupOption[]) =>
  makeChoices(options, { instId: "none" });

const leveled = makeLevelResult({
  verify_lufs: -20,
  predicted_lufs: -20,
  dynamic_spread_lu: null,
  constant_c: -10,
});

/** A `level_preset` that stays in flight until the test calls `finish`. */
function holdLevelPreset(): { finish: (r: LevelResult) => void } {
  const held = {
    finish: (_r: LevelResult): void => undefined,
  };
  mockCommands({
    level_preset: () =>
      new Promise<LevelResult>((res) => {
        held.finish = res;
      }),
  });
  return held;
}

const levelPresetCalls = () => callsOf("level_preset").length;

beforeEach(() => {
  resetLevelFlow();
  mockCommands();
});

describe("a leveling run outlives the Level tab (#283)", () => {
  it("shows the same run after a remount, and its Stop ends it", async () => {
    const held = holdLevelPreset();

    const first = renderHook(() => useLevelingFlow(deps));
    act(() => {
      first.result.current.onSetupStart(choices([option("base")]));
    });
    await waitFor(() => {
      expect(levelPresetCalls()).toBe(1);
    });
    // The user switches tab: App renders only the active tab, so the view unmounts.
    first.unmount();

    // …and comes back. The fresh mount shows the in-flight run, not a closed wizard.
    const second = renderHook(() => useLevelingFlow(deps));
    expect(second.result.current.stage).toBe("run");
    expect(isRunOver(second.result.current.run.phase)).toBe(false);
    expect(second.result.current.run.items[0].status).toBe("active");

    // Its Stop reaches the run: every lane is cancelled and the run reports stopped.
    act(() => {
      second.result.current.onRunCancel();
    });
    expect(callsOf("cancel_preset_leveling")).toHaveLength(1);
    expect(second.result.current.run.phase === "stopping").toBe(true);
    act(() => {
      held.finish(leveled);
    });
    await waitFor(() => {
      expect(isRunOver(second.result.current.run.phase)).toBe(true);
    });
    expect(second.result.current.run.phase === "stopped").toBe(true);
    expect(isLevelRunning()).toBe(false);
  });

  it("refuses a second run while one is in flight", async () => {
    const held = holdLevelPreset();
    const { result, unmount } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(choices([option("base")]));
    });
    await waitFor(() => {
      expect(levelPresetCalls()).toBe(1);
    });
    unmount();

    const again = renderHook(() => useLevelingFlow(deps));
    act(() => {
      again.result.current.onSetupStart(choices([option("base")]));
    });
    expect(levelPresetCalls()).toBe(1);
    // Closing the wizard is refused mid-run too, so the run can't be hidden.
    act(() => {
      again.result.current.onCancel();
    });
    expect(again.result.current.stage).toBe("run");

    act(() => {
      held.finish(leveled);
    });
    await waitFor(() => {
      expect(isRunOver(again.result.current.run.phase)).toBe(true);
    });
    expect(again.result.current.run.phase === "stopped").toBe(false);
  });
});

describe("a throwing live block read ends the run (#285)", () => {
  it("skips the scene group with the error and releases the run", async () => {
    mockCommands({
      list_level_blocks: () => rejectWith(new Error("Device not connected")),
    });
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(
        choices([option("scene", 0), option("scene", 1)]),
      );
    });
    await waitFor(() => {
      expect(isRunOver(result.current.run.phase)).toBe(true);
    });
    expect(result.current.run.phase === "stopped").toBe(false);
    expect(result.current.run.phase === "stopping").toBe(false);
    for (const row of result.current.run.items) {
      expect(row).toMatchObject({
        status: "result",
        outcome: "skipped",
        skipReason: "Device not connected",
      });
    }
    expect(fired("level_scenes_apply_batched")).toBe(false);
    expect(isLevelRunning()).toBe(false);

    // The run is released: a new one starts.
    mockCommands({ level_preset: () => leveled });
    act(() => {
      result.current.onSetupStart(choices([option("base")]));
    });
    await waitFor(() => {
      expect(levelPresetCalls()).toBe(1);
    });
  });
});

describe("a device detach closes the flow", () => {
  it("closes an idle Summary at once", async () => {
    mockCommands({ level_preset: () => leveled });
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(choices([option("base")]));
    });
    await waitFor(() => {
      expect(isRunOver(result.current.run.phase)).toBe(true);
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
    const held = holdLevelPreset();
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(choices([option("base")]));
    });
    await waitFor(() => {
      expect(levelPresetCalls()).toBe(1);
    });

    act(() => {
      onDeviceDetached();
    });
    // The run is not torn down under the in-flight command.
    expect(result.current.stage).toBe("run");
    expect(isLevelRunning()).toBe(true);

    act(() => {
      held.finish(leveled);
    });
    await waitFor(() => {
      expect(result.current.stage).toBe("closed");
    });
    expect(isRunOver(result.current.run.phase)).toBe(false);
    expect(isLevelRunning()).toBe(false);
  });
});
