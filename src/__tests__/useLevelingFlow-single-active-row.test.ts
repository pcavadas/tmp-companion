// src/__tests__/useLevelingFlow-single-active-row.test.ts — BUG→GATE: at most ONE
// run row is ever "active" at a time.
//
// Real incident: a batch measuring several scenes/footswitches streams per-row
// `active` progress over a Tauri Channel, but the old `batchResolve` only ever SET a
// row active and never cleared the PREVIOUS one — so once the backend named a new
// row active before the prior one resolved, both rows (then more) stayed "active"
// forever. `RunBody` renders `leveling · <liveLufs>` for EVERY active row off ONE
// shared `liveLufs` value, so the screenshot showed four rows reading the identical
// live LUFS while the header said it was measuring only one.
//
// This test drives `useLevelingFlow` directly (device calls mocked) and proves the
// invariant at the STATE layer: exactly one row carries `status === "active"` after
// the backend names a second row active, and it's the row the backend actually named.

import { beforeEach, describe, expect, it } from "vitest";
import { renderHook, act } from "@testing-library/react";

import type { SceneLevelProgressItem } from "../lib/types";
import { useLevelingFlow } from "../views/level/useLevelingFlow";
import type { SetupChoice } from "../views/level/leveling";
import { resetLevelFlow } from "../views/level/levelRunStore";
import {
  makeLevelingFlowDeps,
  makeSceneOption,
  mockCommands,
} from "./commandMocks";

// The leveling wizard lives in a module store that outlives a mount; start each case clean.
beforeEach(() => {
  resetLevelFlow();
});

const sceneOption = (sceneSlot: number, name: string) =>
  makeSceneOption(sceneSlot, { presetName: "Plexi", sceneName: name });

describe("useLevelingFlow — at most one active row at a time (BUG 2)", () => {
  it("clears the previous active row when the channel names a new one active", () => {
    let onResult: ((item: SceneLevelProgressItem) => void) | undefined;
    mockCommands({
      level_scenes_apply_batched: (args) => {
        onResult = (item) => {
          args.onResult.onmessage(item);
        };
        return new Promise<never>(() => {
          /* held open — never resolves, mirrors an in-flight batch */
        });
      },
    });

    const { result } = renderHook(() =>
      useLevelingFlow(makeLevelingFlowDeps()),
    );

    const choices: SetupChoice[] = [
      { option: sceneOption(0, "Rhythm"), instId: "", targetName: "Rhythm" },
      { option: sceneOption(1, "Lead"), instId: "", targetName: "Rhythm" },
    ];

    act(() => {
      result.current.onSetupStart(choices);
    });

    // `markGroupActive`'s optimistic pre-flip: the first row goes active up front.
    expect(
      result.current.run.items.filter((it) => it.status === "active"),
    ).toHaveLength(1);
    expect(
      result.current.run.items.find((it) => it.status === "active")?.sceneSlot,
    ).toBe(0);

    // The backend's first REAL progress item names the SECOND scene active before the
    // first ever resolved — the exact shape from the bug report.
    act(() => {
      onResult?.({
        sceneSlot: 1,
        status: "active",
        result: null,
        message: null,
      });
    });

    const activeRows = result.current.run.items.filter(
      (it) => it.status === "active",
    );
    expect(activeRows).toHaveLength(1);
    expect(activeRows[0]?.sceneSlot).toBe(1);
    // The optimistically-flipped first row must be released cleanly, not left
    // dangling in "active" (it isn't resolved either — it's back to queued).
    expect(
      result.current.run.items.find((it) => it.sceneSlot === 0)?.status,
    ).toBe("queued");
  });
});
