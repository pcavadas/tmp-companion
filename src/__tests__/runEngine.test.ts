// src/__tests__/runEngine.test.ts — the leveling run engine with no React and no store:
// the pure batching rule, the result mappers, and `runEngine` driven straight through
// fake device commands. The engine's contract with its store is that it only ever EMITS
// immutable item patches — a published row is never mutated afterwards — so every case
// here freezes what it publishes and replays the patches the way `levelRunStore` does.

import { describe, expect, it, vi } from "vitest";

import type { LevelResult } from "../lib/types";
import type { RunItem } from "../views/level/leveling";
import {
  applyPatches,
  byEarCause,
  outcomeOf,
  planBatches,
  runEngine,
  valueOf,
  type EngineDeps,
  type EngineStep,
  type LevelOutcomeFields,
} from "../views/level/runEngine";
import { makeFootswitchLevelResult, makeLevelResult } from "./commandMocks";

const row = (key: string, over: Partial<RunItem> = {}): RunItem => ({
  key,
  slot: 0,
  presetName: "Plexi",
  isBase: false,
  sceneSlot: null,
  sceneName: key,
  tag: null,
  instId: "",
  targetName: "Rhythm",
  status: "queued",
  ...over,
});
const base = (key: string, over: Partial<RunItem> = {}) =>
  row(key, { isBase: true, ...over });
const scene = (key: string, sceneSlot: number, over: Partial<RunItem> = {}) =>
  row(key, { sceneSlot, ...over });
const footswitch = (
  key: string,
  switchIndex: number,
  over: Partial<RunItem> = {},
) =>
  row(key, {
    footswitch: {
      switchIndex,
      levGroupId: "G",
      levNodeId: "N",
      levParameterId: "level",
      sceneContext: null,
    },
    ...over,
  });

const outcome = (
  over: Partial<LevelOutcomeFields> = {},
): LevelOutcomeFields => ({
  clamped: false,
  clamp_reason: null,
  verify_lufs: -20,
  predicted_lufs: -21,
  ...over,
});

describe("planBatches", () => {
  it("gives each base row its own call and batches adjacent same-preset, same-instrument rows", () => {
    const work = [
      base("b0"),
      scene("s1", 1),
      scene("s2", 2, { targetName: "Lead" }), // a different target still batches
      scene("s3", 3, { instId: "tele" }), // a different instrument splits
      footswitch("f0", 0),
      footswitch("f1", 1),
      row("whole"), // scene-less: a single preset-lane call
      scene("s1@1", 1, { slot: 1 }), // another preset splits
    ];
    expect(planBatches(work)).toEqual([
      { lane: "preset", start: 0, end: 1 },
      { lane: "scene", start: 1, end: 3 },
      { lane: "scene", start: 3, end: 4 },
      { lane: "footswitch", start: 4, end: 6 },
      { lane: "preset", start: 6, end: 7 },
      { lane: "scene", start: 7, end: 8 },
    ]);
  });

  it("never merges two base rows, and plans nothing for no rows", () => {
    expect(planBatches([base("a"), base("b")])).toHaveLength(2);
    expect(planBatches([])).toEqual([]);
  });
});

describe("result mappers", () => {
  it("outcomeOf: off-branch over clamped over unconverged/unverified over done", () => {
    expect(outcomeOf(outcome({ clamp_reason: "silent", clamped: true }))).toBe(
      "offbranch",
    );
    expect(outcomeOf(outcome({ clamped: true, unconverged: true }))).toBe(
      "clamped",
    );
    expect(outcomeOf(outcome({ unconverged: true }))).toBe("unconverged");
    expect(outcomeOf(outcome({ verification: "verify_failed" }))).toBe(
      "unconverged",
    );
    expect(outcomeOf(outcome())).toBe("done");
  });

  it("valueOf prefers the verify capture over the prediction", () => {
    expect(valueOf(outcome())).toBe(-20);
    expect(valueOf(outcome({ verify_lufs: null }))).toBe(-21);
  });

  it("byEarCause resolves one cause: wet floor, else rebalance, else none", () => {
    expect(byEarCause(outcome({ wet_floor: true, verify_by_ear: true }))).toBe(
      "wet_floor",
    );
    expect(byEarCause(outcome({ verify_by_ear: true }))).toBe("rebalance");
    expect(byEarCause(outcome())).toBeUndefined();
  });
});

/** Fake deps: every command resolves a plain success unless overridden. */
function fakeDeps(over: Partial<EngineDeps> = {}): EngineDeps {
  return {
    levelPreset: vi.fn<EngineDeps["levelPreset"]>(() =>
      Promise.resolve(makeLevelResult({ verify_lufs: -26 })),
    ),
    listLevelBlocks: vi.fn<EngineDeps["listLevelBlocks"]>(() =>
      Promise.resolve([]),
    ),
    levelScenesApplyBatched: vi.fn<EngineDeps["levelScenesApplyBatched"]>(
      (args, onResult) => {
        for (const job of args.jobs) {
          onResult({
            sceneSlot: job.sceneSlot,
            status: "active",
            result: null,
            message: "measuring",
          });
          onResult({
            sceneSlot: job.sceneSlot,
            status: "done",
            result: makeLevelResult({ verify_lufs: job.targetLufs }),
            message: null,
          });
        }
        return Promise.resolve([]);
      },
    ),
    levelFootswitchesApply: vi.fn<EngineDeps["levelFootswitchesApply"]>(
      (args, onResult) => {
        for (const job of args.jobs)
          onResult({
            switch: job.switch,
            status: "done",
            result: makeFootswitchLevelResult({ switch: job.switch }),
            message: null,
          });
        return Promise.resolve([]);
      },
    ),
    isCancelled: () => false,
    profileById: () => null,
    targetLufsByName: (name) => (name === "Lead" ? -20 : -26),
    ampCandidates: new Map([
      [
        0,
        [
          {
            groupId: "G",
            nodeId: "amp",
            parameterId: "outputLevel",
            value: 0.5,
          },
        ],
      ],
    ]),
    blocksByIndex: new Map(),
    rebalance: false,
    ...over,
  };
}

/** Run the engine the way `levelRunStore` does — replaying every step's patches onto
 *  the published rows — freezing each publication so any later mutation throws, and
 *  keeping a deep copy of it to prove nothing published ever changed. */
async function drive(items: RunItem[], deps: EngineDeps) {
  const steps: EngineStep[] = [];
  const published: { rows: readonly RunItem[]; copy: string }[] = [];
  let rows: readonly RunItem[] = Object.freeze(
    items.map((it) => Object.freeze(it)),
  );
  published.push({ rows, copy: JSON.stringify(rows) });
  await runEngine(rows, deps, (step) => {
    steps.push(step);
    rows = Object.freeze(
      applyPatches(rows, step.patches).map((it) => Object.freeze(it)),
    );
    published.push({ rows, copy: JSON.stringify(rows) });
  });
  for (const p of published) expect(JSON.stringify(p.rows)).toBe(p.copy);
  return { steps, rows };
}

describe("runEngine", () => {
  it("levels base first, then one scene batch, and emits immutable patches", async () => {
    const deps = fakeDeps();
    const { steps, rows } = await drive(
      [base("b"), scene("s1", 1), scene("s2", 2, { targetName: "Lead" })],
      deps,
    );

    expect(deps.levelPreset).toHaveBeenCalledTimes(1);
    expect(deps.levelScenesApplyBatched).toHaveBeenCalledTimes(1);
    const [args] = vi.mocked(deps.levelScenesApplyBatched).mock.calls[0];
    expect(args).toMatchObject({
      slot: 0,
      jobs: [
        { sceneSlot: 1, targetLufs: -26 },
        { sceneSlot: 2, targetLufs: -20 },
      ],
      save: true,
      rebalance: false,
      baseAnchor: { targetLufs: -26 },
    });
    expect(deps.listLevelBlocks).not.toHaveBeenCalled();

    expect(rows.map((r) => [r.key, r.status, r.outcome, r.value])).toEqual([
      ["b", "result", "done", -26],
      ["s1", "result", "done", -26],
      ["s2", "result", "done", -20],
    ]);
    expect(steps.map((s) => s.resolvedKey).filter(Boolean)).toEqual([
      "b",
      "s1",
      "s2",
    ]);
    // The step counter only ever moves forward.
    const idx = steps.map((s) => s.currentIndex);
    expect(idx).toEqual([...idx].sort((a, b) => a - b));
  });

  it("keeps at most one row active at every step", async () => {
    const items = [
      scene("s1", 1),
      scene("s2", 2),
      footswitch("f0", 0),
      footswitch("f1", 1),
    ];
    const { steps } = await drive(items, fakeDeps());
    let rows: readonly RunItem[] = items;
    for (const step of steps) {
      rows = applyPatches(rows, step.patches);
      expect(
        rows.filter((r) => r.status === "active").length,
      ).toBeLessThanOrEqual(1);
    }
  });

  it("passes the run's rebalance choice to the scene call", async () => {
    const deps = fakeDeps({ rebalance: true });
    await drive([scene("s1", 1)], deps);
    expect(
      vi.mocked(deps.levelScenesApplyBatched).mock.calls[0][0].rebalance,
    ).toBe(true);
  });

  it("skips a failed call's rows with its reason and carries on", async () => {
    const deps = fakeDeps({
      levelPreset: vi.fn<EngineDeps["levelPreset"]>(() =>
        Promise.reject(new Error("capture failed")),
      ),
    });
    const { rows } = await drive([base("b"), footswitch("f0", 0)], deps);
    expect(rows[0]).toMatchObject({
      status: "result",
      outcome: "skipped",
      skipReason: "capture failed",
    });
    expect(rows[1]).toMatchObject({ status: "result", outcome: "done" });
  });

  it("a stop mid-batch reverts the optimistic active row and dispatches nothing more", async () => {
    let cancelled = false;
    const deps = fakeDeps({
      isCancelled: () => cancelled,
      levelScenesApplyBatched: vi.fn<EngineDeps["levelScenesApplyBatched"]>(
        () => {
          cancelled = true; // Stop pressed while the batch is in flight
          return Promise.resolve([]);
        },
      ),
    });
    const { rows } = await drive(
      [scene("s1", 1), base("b", { slot: 1 })],
      deps,
    );
    expect(rows.map((r) => r.status)).toEqual(["queued", "queued"]);
    expect(rows[0].activeMessage).toBeNull();
    expect(deps.levelPreset).not.toHaveBeenCalled();
  });

  it("stamps a batch's re-amp warning on every row it covered", async () => {
    const deps = fakeDeps({
      levelScenesApplyBatched: vi.fn<EngineDeps["levelScenesApplyBatched"]>(
        (args, onResult) => {
          for (const job of args.jobs)
            onResult({
              sceneSlot: job.sceneSlot,
              status: "done",
              result: makeLevelResult(),
              message: null,
            });
          const warned: LevelResult = makeLevelResult({
            reamp_warning: "re-amp may still be on",
          });
          return Promise.resolve([warned]);
        },
      ),
    });
    const { rows } = await drive([scene("s1", 1), scene("s2", 2)], deps);
    expect(rows.map((r) => r.reampWarning)).toEqual([
      "re-amp may still be on",
      "re-amp may still be on",
    ]);
  });
});
