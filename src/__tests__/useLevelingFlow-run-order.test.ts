// src/__tests__/useLevelingFlow-run-order.test.ts — BUG→GATE: each preset's Base row
// reaches the backend BEFORE that preset's scenes, whatever order the selection arrived in.
//
// Base levels `presetLevel`, a global multiplier over every scene, so a scene leveled
// before its own Base is pushed off target by the Base write that follows. The run used
// to sort with a comparator that returned 0 across slots — not a consistent order — and
// on an interleaved selection (`[scene@0, base@1, base@0]`) it left slot 0's scene ahead
// of slot 0's base. This drives `useLevelingFlow` with the device calls mocked and pins the
// order the two backend lanes are actually called in.

import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";

const h = vi.hoisted(() => ({
  calls: [] as string[],
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
import { baseKey, sceneKeyOf } from "../views/level/leveling";
import type { SetupChoice, SetupOption } from "../views/level/leveling";
import { resetLevelFlow } from "../views/level/levelRunStore";

// The leveling wizard lives in a module store that outlives a mount; start each case clean.
beforeEach(() => {
  resetLevelFlow();
});

function baseOption(slot: number): SetupOption {
  return {
    key: baseKey(slot),
    slot,
    presetName: `P${String(slot)}`,
    isBase: true,
    sceneSlot: null,
    sceneName: "Base Preset",
    tag: "BASE",
    hasScenes: true,
  };
}

function sceneOption(slot: number, sceneSlot: number): SetupOption {
  return {
    key: sceneKeyOf(slot, sceneSlot),
    slot,
    presetName: `P${String(slot)}`,
    isBase: false,
    sceneSlot,
    sceneName: `Scene ${String(sceneSlot)}`,
    tag: `FS${String(sceneSlot + 1)}`,
    hasScenes: true,
  };
}

const choice = (option: SetupOption): SetupChoice => ({
  option,
  instId: "",
  targetName: "Rhythm",
});

describe("useLevelingFlow — base before scenes at the backend", () => {
  it("levels each preset's base before its scenes on a scenes-first selection", async () => {
    h.levelPreset.mockImplementation((job: { slot: number }) => {
      h.calls.push(`base@${String(job.slot)}`);
      // The result is not under test — a rejection resolves the row as skipped.
      return Promise.reject(new Error("not under test"));
    });
    h.levelScenesApplyBatched.mockImplementation((args: { slot: number }) => {
      h.calls.push(`scenes@${String(args.slot)}`);
      return Promise.resolve([]);
    });

    const candidate = {
      groupId: "g",
      nodeId: "n",
      parameterId: "outputLevel",
      value: 0.5,
    };
    const { result } = renderHook(() =>
      useLevelingFlow({
        rows: [
          { slot: 0, name: "P0", empty: false },
          { slot: 1, name: "P1", empty: false },
        ],
        store: null,
        sceneInfo: new Map(),
        footswitchInfo: new Map(),
        ampCandidates: new Map([
          [0, [candidate]],
          [1, [candidate]],
        ]),
        blocksByIndex: new Map(),
        silenceHintByIndex: new Map(),
        targetLufsByName: () => -20,
        deselectKeys: () => {
          /* no-op */
        },
        refresh: () => Promise.resolve(),
      }),
    );

    // Scenes first, and another preset's base between slot 0's scene and its base.
    act(() => {
      result.current.onSetupStart([
        choice(sceneOption(0, 0)),
        choice(baseOption(1)),
        choice(baseOption(0)),
      ]);
    });

    await waitFor(() => {
      expect(result.current.run.done).toBe(true);
    });
    expect(h.calls).toEqual(["base@0", "scenes@0", "base@1"]);
  });
});
