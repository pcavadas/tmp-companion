// src/__tests__/useLevelingFlow-run-store.test.ts — BUG→GATEs for the leveling run's
// LIFECYCLE, now that the wizard + run engine live in a module store.
//
// #283: switching tabs mid-run unmounted LevelView and orphaned the run — it kept writing
// to the device while the remounted tab showed a closed wizard with no Stop, and a second
// run could start alongside it. A remounted hook must bind back to the SAME run, its Stop
// must end it, and nothing may start a second run while one is in flight.
// #285: the run loop had no try/finally, so a thrown live read stranded the wizard on
// "Saving the last one…" with `running` held forever.
// #284: "Run again" / "Re-run off target" reopened Set up with every sound at the first
// target + first instrument, dropping what each sound was leveled with.

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { SetupChoice, SetupOption } from "../views/level/leveling";

const h = vi.hoisted(() => ({
  levelScenesApplyBatched: vi.fn(),
  listLevelBlocks: vi.fn(),
  cancelPresetLeveling: vi.fn(),
  cancelSceneLeveling: vi.fn(),
  cancelFootswitchLeveling: vi.fn(),
}));
vi.mock("../lib/invoke", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/invoke")>()),
  ...h,
}));

import {
  isLevelRunInFlight,
  resetLevelRun,
  useLevelingFlow,
  type UseLevelingFlowDeps,
} from "../views/level/useLevelingFlow";

const AMP = {
  groupId: "G1",
  nodeId: "amp",
  parameterId: "outputLevel",
  value: 0.5,
};

const deps: UseLevelingFlowDeps = {
  rows: [{ slot: 0, name: "Plexi", empty: false }],
  store: null,
  sceneInfo: new Map(),
  footswitchInfo: new Map(),
  ampCandidates: new Map([[0, [AMP]]]),
  blocksByIndex: new Map(),
  silenceHintByIndex: new Map(),
  targetLufsByName: () => -20,
  deselectKeys: () => {
    /* not under test */
  },
  refresh: () => Promise.resolve(),
};

function sceneOption(sceneSlot: number): SetupOption {
  return {
    key: `s0:${String(sceneSlot)}`,
    slot: 0,
    presetName: "Plexi",
    isBase: false,
    sceneSlot,
    sceneName: `Scene ${String(sceneSlot)}`,
    tag: `FS${String(sceneSlot + 1)}`,
    hasScenes: true,
  };
}
const choice = (
  option: SetupOption,
  instId = "",
  targetName = "Rhythm",
): SetupChoice => ({ option, instId, targetName });

/** A held-open batch whose caller settles it — the in-flight run. */
function holdBatch() {
  let settle: () => void = () => {
    throw new Error("batch not started");
  };
  h.levelScenesApplyBatched.mockImplementation(
    () =>
      new Promise<never[]>((resolve) => {
        settle = () => {
          resolve([]);
        };
      }),
  );
  return () => {
    settle();
  };
}

beforeEach(() => {
  resetLevelRun();
  for (const mock of Object.values(h)) mock.mockReset();
  h.cancelPresetLeveling.mockResolvedValue(undefined);
  h.cancelSceneLeveling.mockResolvedValue(undefined);
  h.cancelFootswitchLeveling.mockResolvedValue(undefined);
});

describe("#283 — a tab switch never orphans the run", () => {
  it("a remounted Level tab shows the live run, and its Stop ends it", async () => {
    const settle = holdBatch();
    const first = renderHook(() => useLevelingFlow(deps));
    act(() => {
      first.result.current.onSetupStart([choice(sceneOption(0))]);
    });
    expect(first.result.current.stage).toBe("run");
    // The user clicks another tab: LevelView unmounts mid-run…
    first.unmount();
    expect(isLevelRunInFlight()).toBe(true);
    // …and comes back. The fresh mount binds to the SAME run — run page, not a closed
    // wizard, and not done.
    const back = renderHook(() => useLevelingFlow(deps));
    expect(back.result.current.stage).toBe("run");
    expect(back.result.current.run.done).toBe(false);
    expect(back.result.current.run.items.map((r) => r.key)).toEqual(["s0:0"]);

    // Its Stop reaches the in-flight run: every lane's backend cancel fires and the
    // header flips to "Stopping…" at once.
    act(() => {
      back.result.current.onRunCancel();
    });
    expect(h.cancelSceneLeveling).toHaveBeenCalledTimes(1);
    expect(h.cancelPresetLeveling).toHaveBeenCalledTimes(1);
    expect(h.cancelFootswitchLeveling).toHaveBeenCalledTimes(1);
    expect(back.result.current.run.stopping).toBe(true);

    // The in-flight command winds down → the run ends STOPPED, on the remounted page.
    await act(async () => {
      settle();
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(back.result.current.run.done).toBe(true);
    });
    expect(back.result.current.run.stopped).toBe(true);
    expect(back.result.current.run.stopping).toBe(false);
    expect(isLevelRunInFlight()).toBe(false);
  });

  it("a second run cannot start while one is in flight", async () => {
    const settle = holdBatch();
    const first = renderHook(() => useLevelingFlow(deps));
    act(() => {
      first.result.current.onSetupStart([choice(sceneOption(0))]);
    });
    first.unmount();
    const back = renderHook(() => useLevelingFlow(deps));
    // The list's "Level…" entry and a stray Start are both refused mid-run.
    act(() => {
      back.result.current.openFlow(new Set(["p0"]));
      back.result.current.onSetupStart([choice(sceneOption(1))]);
    });
    expect(back.result.current.stage).toBe("run");
    expect(h.levelScenesApplyBatched).toHaveBeenCalledTimes(1);
    expect(back.result.current.run.items.map((r) => r.key)).toEqual(["s0:0"]);
    // Nor can the run page be closed out from under the run.
    act(() => {
      back.result.current.onCancel();
    });
    expect(back.result.current.stage).toBe("run");

    await act(async () => {
      settle();
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(back.result.current.run.done).toBe(true);
    });
    // Once it ends, a new run may start.
    holdBatch();
    act(() => {
      back.result.current.onSetupStart([choice(sceneOption(1))]);
    });
    expect(h.levelScenesApplyBatched).toHaveBeenCalledTimes(2);
  });

  it("the ended run's list reload goes to the MOUNTED view", async () => {
    const settle = holdBatch();
    const staleRefresh = vi.fn(() => Promise.resolve());
    const first = renderHook(() =>
      useLevelingFlow({ ...deps, refresh: staleRefresh }),
    );
    act(() => {
      first.result.current.onSetupStart([choice(sceneOption(0))]);
    });
    first.unmount();
    const liveRefresh = vi.fn(() => Promise.resolve());
    const back = renderHook(() =>
      useLevelingFlow({ ...deps, refresh: liveRefresh }),
    );
    await act(async () => {
      settle();
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(back.result.current.run.done).toBe(true);
    });
    expect(liveRefresh).toHaveBeenCalledTimes(1);
    expect(staleRefresh).not.toHaveBeenCalled();
  });
});

describe("#285 — the run always reaches a terminal state", () => {
  it("a throwing live block read ends the run with the error shown, and releases it", async () => {
    // The backup scan has no candidates for this preset (e.g. a detach emptied it), so
    // the run falls back to a live read — which fails.
    h.listLevelBlocks.mockRejectedValue(new Error("device not connected"));
    const { result } = renderHook(() =>
      useLevelingFlow({ ...deps, ampCandidates: new Map() }),
    );
    act(() => {
      result.current.onSetupStart([
        choice(sceneOption(0)),
        choice(sceneOption(1)),
      ]);
    });
    await waitFor(() => {
      expect(result.current.run.done).toBe(true);
    });
    expect(result.current.run.stopping).toBe(false);
    for (const row of result.current.run.items) {
      expect(row).toMatchObject({
        status: "result",
        outcome: "skipped",
        skipReason: "device not connected",
      });
    }
    expect(h.levelScenesApplyBatched).not.toHaveBeenCalled();
    // `running` was released: the next run starts.
    expect(isLevelRunInFlight()).toBe(false);
    holdBatch();
    act(() => {
      result.current.onSetupStart([choice(sceneOption(0))]);
    });
    expect(result.current.run.done).toBe(false);
    expect(isLevelRunInFlight()).toBe(true);
  });

  it("an unexpected throw ends the run STOPPED, with the reason on the row it hit", async () => {
    // A throw from outside every per-item try — here the deps' own target lookup.
    const { result } = renderHook(() =>
      useLevelingFlow({
        ...deps,
        targetLufsByName: () => {
          throw new Error("store went away");
        },
      }),
    );
    act(() => {
      result.current.onSetupStart([
        choice(sceneOption(0)),
        choice(sceneOption(1)),
      ]);
    });
    await waitFor(() => {
      expect(result.current.run.done).toBe(true);
    });
    expect(result.current.run.stopped).toBe(true);
    expect(result.current.run.stopping).toBe(false);
    expect(result.current.run.items.some((r) => r.status === "active")).toBe(
      false,
    );
    expect(result.current.run.items[0]).toMatchObject({
      outcome: "skipped",
      skipReason: "store went away",
    });
    expect(isLevelRunInFlight()).toBe(false);
  });
});

describe("#284 — a re-run keeps each sound's own instrument + target", () => {
  it("onRelevel reopens Set up carrying the row's instId and targetName", async () => {
    h.levelScenesApplyBatched.mockResolvedValue([]);
    const { result } = renderHook(() => useLevelingFlow(deps));
    act(() => {
      result.current.onSetupStart([
        choice(sceneOption(0), "none", "Lead"),
        choice(sceneOption(1), "tele", "Rhythm"),
      ]);
    });
    await waitFor(() => {
      expect(result.current.run.done).toBe(true);
    });
    act(() => {
      result.current.onRelevel(result.current.run.items);
    });
    expect(result.current.stage).toBe("setup");
    expect(result.current.isRelevel).toBe(true);
    expect(
      result.current.chosen.map((o) => [o.key, o.instId, o.targetName]),
    ).toEqual([
      ["s0:0", "none", "Lead"],
      ["s0:1", "tele", "Rhythm"],
    ]);
  });
});
