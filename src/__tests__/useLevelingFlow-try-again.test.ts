// src/__tests__/useLevelingFlow-try-again.test.ts — the Summary's one "Try again (N)"
// (look B): it goes straight to the run with the same settings — no Set up, no backup
// tick — and comes back to the same Summary with those sounds' rows updated.
//
// BUG→GATE #284, retargeted: "Run again" / "Re-run off target…" reopened Set up with every
// row reset to the FIRST target and FIRST instrument, so one Start re-leveled a Lead sound
// at Rhythm. Try again never reopens Set up: each sound runs again with exactly the
// instrument, target, knob, scene and two-amp choice the first run used.
//
// Also pinned here: a Stop lands on the Summary by itself (no "Leveling stopped" screen and
// no Continue), the Summary's footswitch scene pick rides the Try again, a scene tried
// again without its main sound still anchors on it, and Done deselects every sound any run
// of the flow resolved (BUG-4, accumulating across Try again rounds).

import { act, renderHook, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { LevelResult } from "../lib/types";
import type { SetupChoice } from "../views/level/leveling";
import { useLevelingFlow } from "../views/level/useLevelingFlow";
import {
  isLevelRunning,
  isRunOver,
  resetLevelFlow,
} from "../views/level/levelRunStore";
import {
  callsOf,
  makeBatchRun,
  makeFootswitchOption,
  makeFootswitchLevelResult,
  makeLevelResult,
  makeLevelingFlowDeps,
  makeSceneOption,
  makeSetupOption,
  makeStore,
  mockCommands,
  rejectWith,
  type Handler,
} from "./commandMocks";

const TELE = {
  id: "tele",
  name: "Telecaster",
  topology_id: "sc",
  calibration_lufs: -22,
};

let deselected: string[] = [];
const deps = makeLevelingFlowDeps({
  store: makeStore({ profiles: [TELE] }),
  targetLufsByName: (name) => (name === "Lead" ? -19 : -23),
  deselectKeys: (keys) => {
    deselected = keys;
  },
});

const LEAD_KNOB = { groupId: "G2", nodeId: "boost", parameterId: "level" };

/** Base (Lead, Telecaster), a scene with its own knob (Lead), a footswitch measured in
 *  scene 1 (Lead), all of list index 0, with "Balance its two amps first" on. */
function firstRunChoices(): SetupChoice[] {
  return [
    { option: makeSetupOption(), instId: "tele", targetName: "Lead" },
    {
      option: makeSceneOption(0, { sceneHandle: LEAD_KNOB }),
      instId: "tele",
      targetName: "Lead",
    },
    {
      option: makeFootswitchOption(2, {
        footswitch: {
          switchIndex: 2,
          levGroupId: "G1",
          levNodeId: "drive",
          levParameterId: "level",
          sceneContext: 1,
        },
      }),
      instId: "tele",
      targetName: "Lead",
    },
  ];
}

/** A scene batch that streams the given status for every job. */
const scenes =
  (status: "done" | "error"): Handler<"level_scenes_apply_batched"> =>
  ({ jobs, onResult }) => {
    jobs.forEach((j) => {
      onResult.onmessage({
        sceneSlot: j.sceneSlot,
        status,
        result:
          status === "done"
            ? makeLevelResult({ scene_slot: j.sceneSlot })
            : null,
        message: status === "error" ? "read failed" : null,
        skipKind: status === "error" ? "preset_unreadable" : null,
      });
    });
    return makeBatchRun();
  };
const footswitches: Handler<"level_footswitches_apply"> = ({
  jobs,
  onResult,
}) => {
  jobs.forEach((j) => {
    onResult.onmessage({
      switch: j.switch,
      status: "done",
      result: makeFootswitchLevelResult({ switch: j.switch }),
      message: null,
      skipKind: null,
    });
  });
  return makeBatchRun();
};

beforeEach(() => {
  resetLevelFlow();
  vi.mocked(invoke).mockClear();
  deselected = [];
});

/** The last call of `command`'s args. */
function lastCall<K extends Parameters<typeof callsOf>[0]>(command: K) {
  const calls = callsOf(command);
  return calls[calls.length - 1];
}

async function settle(result: { current: ReturnType<typeof useLevelingFlow> }) {
  await waitFor(() => {
    expect(isRunOver(result.current.run.phase)).toBe(true);
  });
  await waitFor(() => {
    expect(isLevelRunning()).toBe(false);
  });
}

describe("Try again (#284)", () => {
  it("runs the sounds again straight away with the first run's settings, and updates the same Summary", async () => {
    // First run: the main sound fails, the scene and the footswitch level.
    mockCommands({
      level_preset: () => rejectWith(new Error("capture failed")),
      level_scenes_apply_batched: scenes("done"),
      level_footswitches_apply: footswitches,
    });
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(firstRunChoices(), [0]);
    });
    await settle(result);
    act(() => {
      result.current.onRunComplete();
    });
    expect(result.current.stage).toBe("summary");
    expect(result.current.results.map((r) => [r.key, r.outcome])).toEqual([
      ["p0", "skipped"],
      ["s0:0", "done"],
      ["f0:2", "done"],
    ]);
    const first = {
      preset: callsOf("level_preset")[0],
      scene: callsOf("level_scenes_apply_batched")[0],
      fs: callsOf("level_footswitches_apply")[0],
    };

    // Try again: the main sound and its preset's leveled sounds.
    const done: LevelResult = makeLevelResult({
      target_lufs: -19,
      predicted_lufs: -19,
      verify_lufs: -19,
    });
    mockCommands({
      level_preset: () => done,
      level_scenes_apply_batched: scenes("done"),
      level_footswitches_apply: footswitches,
    });
    act(() => {
      result.current.onTryAgain(["p0", "s0:0", "f0:2"]);
    });
    // Straight to the run: no Set up, no backup tick.
    expect(result.current.stage).toBe("run");
    await settle(result);

    // The same instrument, target, knob, scene and two-amp choice as the first run.
    expect(callsOf("level_preset")).toHaveLength(2);
    const again = {
      preset: lastCall("level_preset"),
      scene: lastCall("level_scenes_apply_batched"),
      fs: lastCall("level_footswitches_apply"),
    };
    expect(again.preset.job).toEqual(first.preset.job);
    expect(again.preset.job).toMatchObject({
      target_lufs: -19,
      profile_id: "tele",
    });
    expect(again.scene.jobs).toEqual(first.scene.jobs);
    expect(again.scene.jobs[0]).toMatchObject({
      targetLufs: -19,
      handle: LEAD_KNOB,
    });
    expect(again.scene).toMatchObject({
      rebalance: true,
      profileId: "tele",
      baseAnchor: { targetLufs: -19 },
    });
    expect(again.fs.jobs).toEqual(first.fs.jobs);
    expect(again.fs.jobs[0]).toMatchObject({
      sceneContext: 1,
      targetLufs: -19,
    });

    // The same Summary, its rows updated in place.
    act(() => {
      result.current.onRunComplete();
    });
    expect(result.current.stage).toBe("summary");
    expect(result.current.results.map((r) => [r.key, r.outcome])).toEqual([
      ["p0", "done"],
      ["s0:0", "done"],
      ["f0:2", "done"],
    ]);
  });

  it("runs only the sounds tried again, and leaves the other rows as they were", async () => {
    mockCommands({
      level_preset: () => makeLevelResult(),
      level_scenes_apply_batched: scenes("error"),
    });
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(firstRunChoices().slice(0, 2));
    });
    await settle(result);
    act(() => {
      result.current.onRunComplete();
    });
    const leveled = result.current.results[0];

    mockCommands({ level_scenes_apply_batched: scenes("done") });
    act(() => {
      result.current.onTryAgain(["s0:0"]);
    });
    // The run page lists only the sound tried again.
    expect(result.current.run.items.map((r) => r.key)).toEqual(["s0:0"]);
    expect(result.current.run.total).toBe(1);
    await settle(result);
    expect(callsOf("level_preset")).toHaveLength(1);
    // Its batch still anchors on the main sound the first run leveled.
    expect(callsOf("level_scenes_apply_batched")).toHaveLength(2);
    expect(lastCall("level_scenes_apply_batched").baseAnchor).toEqual({
      targetLufs: -19,
    });
    expect(result.current.results[0]).toBe(leveled);
    expect(result.current.results[1]).toMatchObject({
      key: "s0:0",
      outcome: "done",
    });
    expect(result.current.results[1].skipKind).toBeUndefined();
  });

  it("levels a footswitch in the scene picked on the Summary", async () => {
    mockCommands({ level_footswitches_apply: footswitches });
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(firstRunChoices().slice(2));
    });
    await settle(result);
    act(() => {
      result.current.onRunComplete();
    });
    act(() => {
      result.current.onSummaryScene("f0:2", 2);
    });
    expect(result.current.summaryScenes).toEqual({ "f0:2": 2 });
    act(() => {
      result.current.onTryAgain(["f0:2"]);
    });
    await settle(result);
    expect(callsOf("level_footswitches_apply")).toHaveLength(2);
    expect(lastCall("level_footswitches_apply").jobs[0].sceneContext).toBe(2);
    // The pick was used up, and the row now says it was leveled in that scene.
    expect(result.current.summaryScenes).toEqual({});
    expect(result.current.results[0].footswitch?.sceneContext).toBe(2);
  });

  it("Done deselects every sound any run of the flow resolved (BUG-4)", async () => {
    mockCommands({
      level_preset: () => rejectWith(new Error("capture failed")),
      level_scenes_apply_batched: scenes("done"),
    });
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(firstRunChoices().slice(0, 2));
    });
    await settle(result);
    mockCommands({ level_preset: () => makeLevelResult() });
    act(() => {
      result.current.onRunComplete();
    });
    act(() => {
      result.current.onTryAgain(["p0"]);
    });
    await settle(result);
    act(() => {
      result.current.onRunComplete();
    });
    act(() => {
      result.current.onDone();
    });
    expect(result.current.stage).toBe("closed");
    expect([...deselected].sort()).toEqual(["p0", "s0:0"]);
    // A closed flow has nothing to try again.
    act(() => {
      result.current.onTryAgain(["p0"]);
    });
    expect(result.current.stage).toBe("closed");
  });
});

// The "Stop mid-run" journey, look B: the page says it is stopping, then the Summary shows
// on its own once the sound in progress has wound down — no "Leveling stopped" screen.
describe("a Stop lands on the Summary", () => {
  it("goes to the Summary as soon as the run has wound down", async () => {
    const held: { finish: (r: LevelResult) => void } = {
      finish: () => undefined,
    };
    mockCommands({
      level_preset: () =>
        new Promise<LevelResult>((res) => {
          held.finish = res;
        }),
    });
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart(firstRunChoices().slice(0, 2));
    });
    await waitFor(() => {
      expect(callsOf("level_preset")).toHaveLength(1);
    });
    act(() => {
      result.current.onRunCancel();
    });
    expect(result.current.run.phase).toBe("stopping");
    expect(result.current.stage).toBe("run");
    act(() => {
      held.finish(makeLevelResult());
    });
    await waitFor(() => {
      expect(result.current.stage).toBe("summary");
    });
    expect(result.current.run.phase).toBe("stopped");
    // The scene the Stop came before is not reached, and the Summary lists it.
    expect(result.current.results.map((r) => [r.key, r.outcome])).toEqual([
      ["p0", "done"],
      ["s0:0", undefined],
    ]);
  });
});
