// src/__tests__/useLevelingFlow-run-order.test.ts — BUG→GATE: each preset's Base row
// reaches the backend BEFORE that preset's scenes, whatever order the selection arrived in.
//
// Base levels `presetLevel`, a global multiplier over every scene, so a scene leveled
// before its own Base is pushed off target by the Base write that follows. The run used
// to sort with a comparator that returned 0 across slots — not a consistent order — and
// on an interleaved selection (`[scene@0, base@1, base@0]`) it left slot 0's scene ahead
// of slot 0's base. This drives `useLevelingFlow` with the device calls mocked and pins the
// order the two backend lanes are actually called in.

import { beforeEach, describe, expect, it } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";

import { useLevelingFlow } from "../views/level/useLevelingFlow";
import type { SetupChoice, SetupOption } from "../views/level/leveling";
import { resetLevelFlow } from "../views/level/levelRunStore";
import {
  makeLevelingFlowDeps,
  makeSceneOption,
  makeSetupOption,
  mockCommands,
  rejectWith,
} from "./commandMocks";

// The leveling wizard lives in a module store that outlives a mount; start each case clean.
beforeEach(() => {
  resetLevelFlow();
});

const baseOption = (slot: number): SetupOption =>
  makeSetupOption({ slot, presetName: `P${String(slot)}` });

const sceneOption = (slot: number, sceneSlot: number): SetupOption =>
  makeSceneOption(sceneSlot, { slot, presetName: `P${String(slot)}` });

const choice = (option: SetupOption): SetupChoice => ({
  option,
  instId: "",
  targetName: "Rhythm",
});

describe("useLevelingFlow — base before scenes at the backend", () => {
  it("levels each preset's base before its scenes on a scenes-first selection", async () => {
    const calls: string[] = [];
    mockCommands({
      level_preset: ({ job }) => {
        calls.push(`base@${String(job.slot)}`);
        // The result is not under test — a rejection resolves the row as skipped.
        return rejectWith(new Error("not under test"));
      },
      level_scenes_apply_batched: ({ slot }) => {
        calls.push(`scenes@${String(slot)}`);
        return [];
      },
    });

    const candidate = {
      groupId: "g",
      nodeId: "n",
      parameterId: "outputLevel",
      value: 0.5,
    };
    const { result } = renderHook(() =>
      useLevelingFlow(
        makeLevelingFlowDeps({
          rows: [
            { slot: 0, name: "P0", empty: false },
            { slot: 1, name: "P1", empty: false },
          ],
          ampCandidates: new Map([
            [0, [candidate]],
            [1, [candidate]],
          ]),
        }),
      ),
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
    expect(calls).toEqual(["base@0", "scenes@0", "base@1"]);
  });
});
