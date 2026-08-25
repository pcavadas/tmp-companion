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

import { describe, it, expect, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";

import type { SceneLevelProgressItem } from "../lib/invoke";

const h = vi.hoisted(() => ({
  levelScenesApplyBatched: vi.fn(),
}));

vi.mock("../lib/invoke", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/invoke")>();
  return {
    ...actual,
    levelScenesApplyBatched: h.levelScenesApplyBatched,
  };
});

// Imported AFTER the mock so the hook picks up the mocked seam.
import { useLevelingFlow } from "../views/level/useLevelingFlow";
import type { SetupChoice, SetupOption } from "../views/level/leveling";

function sceneOption(sceneSlot: number, name: string): SetupOption {
  return {
    key: `s0:${String(sceneSlot)}`,
    slot: 0,
    presetName: "Plexi",
    isBase: false,
    sceneSlot,
    sceneName: name,
    tag: `FS${String(sceneSlot + 1)}`,
    hasScenes: true,
  };
}

describe("useLevelingFlow — at most one active row at a time (BUG 2)", () => {
  it("clears the previous active row when the channel names a new one active", () => {
    let onResult: ((item: SceneLevelProgressItem) => void) | undefined;
    h.levelScenesApplyBatched.mockImplementation(
      (
        _args: unknown,
        cb: (item: SceneLevelProgressItem) => void,
      ): Promise<never> => {
        onResult = cb;
        return new Promise<never>(() => {
          /* held open — never resolves, mirrors an in-flight batch */
        });
      },
    );

    const { result } = renderHook(() =>
      useLevelingFlow({
        rows: [{ slot: 0, name: "Plexi", empty: false }],
        store: null,
        sceneInfo: new Map(),
        footswitchInfo: new Map(),
        ampCandidates: new Map([
          [
            0,
            [
              {
                groupId: "g",
                nodeId: "n",
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
          /* no-op */
        },
        refresh: () => Promise.resolve(),
      }),
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
