// src/__tests__/useLevelingFlow-tail-caption.test.ts — issue 6b: a batched scene
// channel item can carry a `tail` caption ("Saving…" / "Verifying…") for the
// deferred-save / persist-verify phase AFTER every scene in the group already
// resolved — at that point the item carries no valid row key of its own. `batchResolve`
// must read `tail` BEFORE its "unknown key" guard drops the item, publish it as
// `RunState.tailMessage`, and clear it once the run completes.

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
    presetName: "Friedman HBE",
    isBase: false,
    sceneSlot,
    sceneName: name,
    tag: `FS${String(sceneSlot + 1)}`,
    hasScenes: true,
  };
}

const deps = {
  rows: [{ slot: 0, name: "Friedman HBE", empty: false }],
  store: null,
  sceneInfo: new Map(),
  footswitchInfo: new Map(),
  ampCandidates: new Map([
    [
      0,
      [
        {
          groupId: "g",
          nodeId: "ampA",
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
};

describe("useLevelingFlow — batch tail caption (issue 6b)", () => {
  it("a tail-only item sets tailMessage and touches no row", () => {
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

    const { result } = renderHook(() => useLevelingFlow(deps));

    const choices: SetupChoice[] = [
      { option: sceneOption(0, "Rhythm"), instId: "", targetName: "Rhythm" },
    ];

    act(() => {
      result.current.onSetupStart(choices);
    });

    expect(result.current.run.tailMessage).toBeNull();
    const itemsBefore = result.current.run.items;

    // A tail item riding on a key that names no real row (its `sceneSlot` never
    // appeared in the dispatched jobs) — exactly the shape the backend emits once
    // every scene in the group already resolved.
    act(() => {
      onResult?.({
        sceneSlot: 999,
        status: "active",
        result: null,
        message: null,
        tail: "Saving…",
      });
    });

    expect(result.current.run.tailMessage).toBe("Saving…");
    // No row was created or reassigned — same items, same statuses.
    expect(result.current.run.items).toHaveLength(itemsBefore.length);
    expect(result.current.run.items.some((it) => it.sceneSlot === 999)).toBe(
      false,
    );
  });

  it("clears tailMessage once the run completes", async () => {
    let onResult: ((item: SceneLevelProgressItem) => void) | undefined;
    h.levelScenesApplyBatched.mockReset();
    // A DEFERRED promise — held open until this test explicitly resolves it, so the
    // run can't race to "done" before the tail + done events are delivered in order
    // (an already-resolved mock promise settles on its own schedule, independent of
    // when `onResult` is invoked).
    let resolveBatch: ((v: never[]) => void) | undefined;
    h.levelScenesApplyBatched.mockImplementation(
      (
        _args: unknown,
        cb: (item: SceneLevelProgressItem) => void,
      ): Promise<never[]> => {
        onResult = cb;
        return new Promise<never[]>((resolve) => {
          resolveBatch = resolve;
        });
      },
    );

    const { result } = renderHook(() => useLevelingFlow(deps));

    const choices: SetupChoice[] = [
      { option: sceneOption(0, "Rhythm"), instId: "", targetName: "Rhythm" },
    ];

    act(() => {
      result.current.onSetupStart(choices);
    });

    // The real sequencing: every scene's own "done" arrives FIRST, and only THEN
    // does the batch's deferred-save/persist-verify tail caption ride in — this is
    // the gap the "tail" name describes.
    act(() => {
      onResult?.({
        sceneSlot: 0,
        status: "done",
        result: {
          slot: 0,
          scene_slot: 0,
          ref_level: 1,
          measured_lufs: -20,
          constant_c: -18,
          final_level: 0.62,
          target_lufs: -20,
          predicted_lufs: -20,
          clamped: false,
          saved: true,
          verify_lufs: null,
          iterations: 1,
          dynamic_spread_lu: null,
          clamp_reason: null,
          verify_by_ear: false,
          previous_level: null,
          true_peak_dbtp: null,
          persist_mismatch: null,
          clamp_kind: null,
          trade: null,
          base_boost: null,
        },
        message: null,
      });
    });
    expect(result.current.run.tailMessage).toBeNull();

    act(() => {
      onResult?.({
        sceneSlot: 999,
        status: "active",
        result: null,
        message: null,
        tail: "Verifying…",
      });
    });
    // The batch call itself hasn't returned yet — the tail caption rides alone
    // until the run's own completion.
    expect(result.current.run.tailMessage).toBe("Verifying…");

    // Let the batch call return, then flush the microtask chain to the run's own
    // completion publish (sweep → `await refresh()` → final `publish(total, true, …)`).
    await act(async () => {
      resolveBatch?.([]);
      for (let i = 0; i < 8; i += 1) {
        await Promise.resolve();
      }
    });

    expect(result.current.run.done).toBe(true);
    expect(result.current.run.tailMessage).toBeNull();
  });
});
