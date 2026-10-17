// src/__tests__/runEngine.test.ts — the leveling run engine with no React and no store:
// the pure batching rule, the result mappers, and `runEngine` driven straight through
// fake device commands. The engine's contract with its store is that it only ever EMITS
// immutable item patches — a published row is never mutated afterwards — so every case
// here freezes what it publishes and replays the patches the way `levelRunStore` does.

import { describe, expect, it, vi } from "vitest";

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
import {
  buildSummary,
  SUMMARY_TEXT,
  type SummaryContext,
} from "../views/level/summaryModel";
import {
  makeBatchRun,
  makeFootswitchLevelResult,
  makeLevelResult,
} from "./commandMocks";

const row = (key: string, over: Partial<RunItem> = {}): RunItem => ({
  key,
  slot: 0,
  presetName: "Plexi",
  isBase: false,
  sceneSlot: null,
  sceneName: key,
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

const summaryCtx = (over: Partial<SummaryContext> = {}): SummaryContext => ({
  stopped: false,
  reampStillOn: false,
  targetLufsByName: () => -26,
  sceneNamesOf: () => [],
  enablingScenesOf: () => null,
  sceneOverrides: {},
  hookKeyOf: (it) => it.key,
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
            skipKind: null,
          });
          onResult({
            sceneSlot: job.sceneSlot,
            status: "done",
            result: makeLevelResult({ verify_lufs: job.targetLufs }),
            message: null,
            skipKind: null,
          });
        }
        return Promise.resolve(makeBatchRun());
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
            skipKind: null,
          });
        return Promise.resolve(makeBatchRun());
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
    rebalance: new Set(),
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
  const verdict = await runEngine(rows, deps, (step) => {
    steps.push(step);
    rows = Object.freeze(
      applyPatches(rows, step.patches).map((it) => Object.freeze(it)),
    );
    published.push({ rows, copy: JSON.stringify(rows) });
  });
  for (const p of published) expect(JSON.stringify(p.rows)).toBe(p.copy);
  return { steps, rows, verdict };
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

  // The Summary's Try again: a scene tried again without its preset's
  // main sound still anchors its batch on that main sound, at the main sound's target,
  // exactly as the first run's batch did; a run's own base row wins over an earlier one.
  it("anchors a Try again scene batch on the main sound an earlier run carried", async () => {
    const earlier = base("b", { targetName: "Lead", status: "result" });
    const deps = fakeDeps({ earlierBaseRows: [earlier] });
    await drive([scene("s1", 1)], deps);
    expect(deps.levelPreset).not.toHaveBeenCalled();
    const [args] = vi.mocked(deps.levelScenesApplyBatched).mock.calls[0];
    expect(args.baseAnchor).toEqual({ targetLufs: -20 });

    const own = fakeDeps({ earlierBaseRows: [earlier] });
    await drive([base("b"), scene("s1", 1)], own);
    expect(
      vi.mocked(own.levelScenesApplyBatched).mock.calls[0][0].baseAnchor,
    ).toEqual({ targetLufs: -26 });

    const other = fakeDeps({
      earlierBaseRows: [base("x", { slot: 5, targetName: "Lead" })],
    });
    await drive([scene("s1", 1)], other);
    expect(
      vi.mocked(other.levelScenesApplyBatched).mock.calls[0][0],
    ).not.toHaveProperty("baseAnchor");
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

  it("passes each preset's own rebalance choice to its scene call", async () => {
    // "Balance its two amps first" is per preset: preset 0 ticked it, preset 1 did not.
    const amp = { groupId: "G", nodeId: "amp", parameterId: "outputLevel" };
    const deps = fakeDeps({
      rebalance: new Set([0]),
      ampCandidates: new Map([
        [0, [{ ...amp, value: 0.5 }]],
        [1, [{ ...amp, value: 0.5 }]],
      ]),
    });
    await drive([scene("s1", 1), scene("t1", 1, { slot: 1 })], deps);
    const calls = vi.mocked(deps.levelScenesApplyBatched).mock.calls;
    expect(calls.map(([job]) => [job.slot, job.rebalance])).toEqual([
      [0, true],
      [1, false],
    ]);
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
          return Promise.resolve(makeBatchRun());
        },
      ),
    });
    const { rows } = await drive(
      [scene("s1", 1), base("b", { slot: 1 })],
      deps,
    );
    expect(rows.map((r) => r.status)).toEqual(["queued", "queued"]);
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
              skipKind: null,
            });
          return Promise.resolve(
            makeBatchRun([makeLevelResult()], {
              reampWarning: "re-amp may still be on",
            }),
          );
        },
      ),
    });
    const { rows } = await drive([scene("s1", 1), scene("s2", 2)], deps);
    expect(rows.map((r) => r.reampWarning)).toEqual([
      "re-amp may still be on",
      "re-amp may still be on",
    ]);
  });

  // The screen reads the TYPED re-amp flag, never the warning text.
  it("stamps the typed re-amp flag on every row of a batch, from its return or its error", async () => {
    const deps = fakeDeps({
      levelFootswitchesApply: vi.fn<EngineDeps["levelFootswitchesApply"]>(
        (args, onResult) => {
          for (const job of args.jobs)
            onResult({
              switch: job.switch,
              status: "done",
              result: makeFootswitchLevelResult({ switch: job.switch }),
              message: null,
              skipKind: null,
            });
          return Promise.resolve(
            makeBatchRun([makeFootswitchLevelResult({ switch: 0 })], {
              reampStillOn: true,
            }),
          );
        },
      ),
      levelPreset: vi.fn<EngineDeps["levelPreset"]>(() =>
        Promise.reject(
          Object.assign(new Error("no signal captured"), {
            kind: "no_signal",
            reampStillOn: true,
          }),
        ),
      ),
    });
    const { rows } = await drive(
      [footswitch("f0", 0), footswitch("f1", 1), base("b", { slot: 1 })],
      deps,
    );
    expect(rows.map((r) => r.reampStillOn)).toEqual([true, true, true]);
    expect(rows[2]).toMatchObject({
      outcome: "skipped",
      skipKind: "no_signal",
    });
    // The text is never what decided it: no warning text was sent at all.
    expect(rows.map((r) => r.reampWarning)).toEqual([
      undefined,
      undefined,
      undefined,
    ]);
  });

  // A target deleted in Settings has no loudness: the run refuses its sounds before
  // anything reaches the unit, with a typed kind, and never levels them at a stand-in.
  it("refuses a sound whose target is not configured, before any command", async () => {
    const deps = fakeDeps({
      targetLufsByName: (name) =>
        name === "Gone" || name === "" ? null : name === "Lead" ? -20 : -26,
    });
    const { rows, verdict } = await drive(
      [
        base("b", { targetName: "Gone" }),
        scene("s1", 1),
        scene("s2", 2, { targetName: "" }),
        footswitch("f0", 0, { targetName: "Gone" }),
      ],
      deps,
    );
    expect(rows.map((r) => [r.key, r.outcome, r.skipKind ?? null])).toEqual([
      ["b", "skipped", "no_target"],
      ["s1", "done", null],
      ["s2", "skipped", "no_target"],
      ["f0", "skipped", "no_target"],
    ]);
    expect(deps.levelPreset).not.toHaveBeenCalled();
    expect(deps.levelFootswitchesApply).not.toHaveBeenCalled();
    // The scene batch carries only the sound it can level, at its own target, and no
    // anchor for a main sound that has none.
    const [args] = vi.mocked(deps.levelScenesApplyBatched).mock.calls[0];
    expect(args.jobs.map((j) => [j.sceneSlot, j.targetLufs])).toEqual([
      [1, -26],
    ]);
    expect(args.baseAnchor).toBeUndefined();
    expect(rows[1].targetLufs).toBe(-26);
    expect(verdict.reampStillOn).toBe(false);
  });

  it("knows nothing new about re-amp when no command ran", async () => {
    const deps = fakeDeps({ targetLufsByName: () => null });
    const { verdict } = await drive([base("b"), scene("s1", 1)], deps);
    expect(deps.levelPreset).not.toHaveBeenCalled();
    expect(deps.levelScenesApplyBatched).not.toHaveBeenCalled();
    expect(verdict.reampStillOn).toBeNull();
  });

  // A command that returned read re-amp back; a rejection without the flag may have been
  // refused before anything engaged, so it reads nothing.
  it("takes a rejection's still-on, and nothing from a rejection without it", async () => {
    const rejecting = (reampStillOn: boolean) =>
      fakeDeps({
        levelPreset: vi.fn<EngineDeps["levelPreset"]>(() =>
          Promise.reject(
            Object.assign(new Error("refused"), {
              kind: "other",
              reampStillOn,
            }),
          ),
        ),
      });
    expect((await drive([base("b")], rejecting(false))).verdict).toEqual({
      reampStillOn: null,
    });
    expect((await drive([base("b")], rejecting(true))).verdict).toEqual({
      reampStillOn: true,
    });
    // A later command of the same run that reads it back off doesn't undo an earlier
    // still-on.
    const both = fakeDeps({
      levelPreset: vi.fn<EngineDeps["levelPreset"]>(() =>
        Promise.resolve(makeLevelResult({ reamp_still_on: true })),
      ),
    });
    expect(
      (await drive([base("b"), scene("s1", 1)], both)).verdict.reampStillOn,
    ).toBe(true);
  });

  it("leaves the re-amp flag unset when the run-end OFF read back", async () => {
    const { rows } = await drive([base("b"), scene("s1", 1)], fakeDeps());
    expect(rows[0].reampStillOn).toBe(false);
    expect(rows[1].reampStillOn).toBeUndefined();
  });

  // A batch can return no rows at all and still have left re-amp on: the verdict comes back
  // beside the rows, so the Summary still says to restart the unit.
  it("stamps the re-amp flag from a scene batch stopped before any row finished", async () => {
    let cancelled = false;
    const deps = fakeDeps({
      isCancelled: () => cancelled,
      levelScenesApplyBatched: vi.fn<EngineDeps["levelScenesApplyBatched"]>(
        (args, onResult) => {
          // The ceiling prepass: every row goes active, then the Stop lands.
          for (const job of args.jobs)
            onResult({
              sceneSlot: job.sceneSlot,
              status: "active",
              result: null,
              message: "measuring",
              skipKind: null,
            });
          cancelled = true;
          return Promise.resolve(makeBatchRun([], { reampStillOn: true }));
        },
      ),
    });
    const { rows, verdict } = await drive(
      [scene("s1", 1), scene("s2", 2)],
      deps,
    );
    expect(rows.map((r) => [r.status, r.reampStillOn])).toEqual([
      ["queued", true],
      ["queued", true],
    ]);
    expect(verdict.reampStillOn).toBe(true);
    expect(
      buildSummary(
        rows,
        summaryCtx({
          stopped: true,
          reampStillOn: verdict.reampStillOn === true,
        }),
      ).notice,
    ).toEqual({
      tone: "caution",
      title: null,
      text: SUMMARY_TEXT.reampStillOn,
    });
  });

  it("stamps the re-amp flag from a footswitch batch with no solved switch", async () => {
    const deps = fakeDeps({
      levelFootswitchesApply: vi.fn<EngineDeps["levelFootswitchesApply"]>(
        (args, onResult) => {
          for (const job of args.jobs)
            onResult({
              switch: job.switch,
              status: "error",
              result: null,
              message: "no signal captured",
              skipKind: "no_signal",
            });
          return Promise.resolve(makeBatchRun([], { reampStillOn: true }));
        },
      ),
    });
    const { rows, verdict } = await drive(
      [footswitch("f0", 0), footswitch("f1", 1)],
      deps,
    );
    expect(rows.map((r) => [r.outcome, r.skipKind, r.reampStillOn])).toEqual([
      ["skipped", "no_signal", true],
      ["skipped", "no_signal", true],
    ]);
    expect(verdict.reampStillOn).toBe(true);
    expect(
      buildSummary(
        rows,
        summaryCtx({ reampStillOn: verdict.reampStillOn === true }),
      ).notice?.text,
    ).toBe(SUMMARY_TEXT.reampStillOn);
  });

  // The Summary must hear when a save didn't hold. The save's verdict is only known once
  // the command returns — the streamed rows predate the save — so it is merged from the
  // RETURNED rows by identity.
  it("merges the save's verdict from the returned rows onto the rows they name", async () => {
    const deps = fakeDeps({
      levelPreset: vi.fn<EngineDeps["levelPreset"]>(() =>
        Promise.resolve(makeLevelResult({ persist_mismatch: true })),
      ),
      levelScenesApplyBatched: vi.fn<EngineDeps["levelScenesApplyBatched"]>(
        (args, onResult) => {
          for (const job of args.jobs)
            onResult({
              sceneSlot: job.sceneSlot,
              status: "done",
              result: makeLevelResult({ scene_slot: job.sceneSlot }),
              message: null,
              skipKind: null,
            });
          // Returned out of order: identity, never position, picks the row.
          return Promise.resolve(
            makeBatchRun([
              makeLevelResult({ scene_slot: 2, persist_mismatch: true }),
              makeLevelResult({ scene_slot: 1, persist_mismatch: false }),
            ]),
          );
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
              skipKind: null,
            });
          return Promise.resolve(
            makeBatchRun([
              makeFootswitchLevelResult({ switch: 1, persist_mismatch: true }),
            ]),
          );
        },
      ),
    });
    const { rows } = await drive(
      [
        base("b"),
        scene("s1", 1),
        scene("s2", 2),
        footswitch("f0", 0),
        footswitch("f1", 1),
      ],
      deps,
    );
    expect(rows.map((r) => [r.key, r.outcome, r.persistMismatch])).toEqual([
      ["b", "done", true],
      ["s1", "done", false],
      ["s2", "done", true],
      // f0's save was never verified (no returned verdict): nothing is claimed.
      ["f0", "done", undefined],
      ["f1", "done", true],
    ]);
  });

  it("reports a stopped batch's save verdict on the rows it leveled", async () => {
    let cancelled = false;
    const deps = fakeDeps({
      isCancelled: () => cancelled,
      levelScenesApplyBatched: vi.fn<EngineDeps["levelScenesApplyBatched"]>(
        (_args, onResult) => {
          onResult({
            sceneSlot: 1,
            status: "done",
            result: makeLevelResult({ scene_slot: 1 }),
            message: null,
            skipKind: null,
          });
          cancelled = true; // Stop pressed; the batch still saves what it leveled
          return Promise.resolve(
            makeBatchRun([
              makeLevelResult({ scene_slot: 1, persist_mismatch: true }),
            ]),
          );
        },
      ),
    });
    const { rows } = await drive([scene("s1", 1), scene("s2", 2)], deps);
    expect(rows.map((r) => [r.status, r.persistMismatch])).toEqual([
      ["result", true],
      ["queued", undefined],
    ]);
  });

  // The screen picks a skipped row's words by kind; the text is for the log only.
  it("gives every skipped row a typed kind: the streamed one, the error's, or other", async () => {
    const deps = fakeDeps({
      levelScenesApplyBatched: vi.fn<EngineDeps["levelScenesApplyBatched"]>(
        (args, onResult) => {
          onResult({
            sceneSlot: args.jobs[0].sceneSlot,
            status: "error",
            result: null,
            message: "no active guitar amp in scene",
            skipKind: "no_active_amp",
          });
          return Promise.reject(
            Object.assign(new Error("scene 2 could not be read"), {
              kind: "preset_unreadable",
            }),
          );
        },
      ),
      levelPreset: vi.fn<EngineDeps["levelPreset"]>(() =>
        Promise.reject(new Error("no active amp")),
      ),
    });
    const { rows } = await drive(
      [base("b", { slot: 1 }), scene("s1", 1), scene("s2", 2)],
      deps,
    );
    expect(rows.map((r) => [r.key, r.outcome, r.skipKind])).toEqual([
      // Untyped text never picks a kind, whatever it says.
      ["b", "skipped", "other"],
      ["s1", "skipped", "no_active_amp"],
      ["s2", "skipped", "preset_unreadable"],
    ]);
    expect(rows[1].skipReason).toBe("no active guitar amp in scene");
  });

  it("names a client-side skip's kind: no amp to drive, or a preset it couldn't read", async () => {
    const noAmp = await drive(
      [scene("s1", 1)],
      fakeDeps({ ampCandidates: new Map([[0, []]]) }),
    );
    expect(noAmp.rows[0]).toMatchObject({
      outcome: "skipped",
      skipKind: "no_active_amp",
    });
    const unread = await drive(
      [scene("s1", 1)],
      fakeDeps({
        ampCandidates: new Map(),
        listLevelBlocks: vi.fn<EngineDeps["listLevelBlocks"]>(() =>
          Promise.reject(new Error("read timed out")),
        ),
      }),
    );
    expect(unread.rows[0]).toMatchObject({
      outcome: "skipped",
      skipKind: "preset_unreadable",
      skipReason: "read timed out",
    });
  });

  it("carries the result's clamp cause onto the row", async () => {
    const deps = fakeDeps({
      levelFootswitchesApply: vi.fn<EngineDeps["levelFootswitchesApply"]>(
        (args, onResult) => {
          onResult({
            switch: args.jobs[0].switch,
            status: "done",
            result: makeFootswitchLevelResult({
              clamped: true,
              wet_floor: true,
              clamp_kind: "wet_floor",
            }),
            message: null,
            skipKind: null,
          });
          return Promise.resolve(makeBatchRun());
        },
      ),
      levelPreset: vi.fn<EngineDeps["levelPreset"]>(() =>
        Promise.resolve(
          makeLevelResult({ clamped: true, clamp_kind: "scene_ceiling" }),
        ),
      ),
    });
    const { rows } = await drive(
      [base("b", { slot: 1 }), footswitch("f0", 0), scene("s1", 1)],
      deps,
    );
    expect(rows.map((r) => [r.key, r.outcome, r.clampKind])).toEqual([
      ["b", "clamped", "scene_ceiling"],
      ["f0", "clamped", "wet_floor"],
      ["s1", "done", null],
    ]);
  });
});
