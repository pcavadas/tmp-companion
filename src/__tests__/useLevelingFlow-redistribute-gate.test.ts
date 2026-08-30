// src/__tests__/useLevelingFlow-redistribute-gate.test.ts — BUG→GATE: the
// redistribute "single-amp" gate must use `baseActiveAmpCountByIndex` (bypassed-in-base
// amps excluded), never the raw `ampCandidates` node count.
//
// Real incident (Friedman HBE): the preset carries a live amp PLUS a bypassed Twin
// Reverb. `ampCandidates` legitimately lists both (a global bypass filter there would
// starve amp-flip presets that need the bypassed-in-base amp as a candidate for the
// scene where it IS live — see `filter_amp_candidates`'s doc), so a raw node-count gate
// would read this as a 2-amp preset and refuse to offer redistribution even though only
// ONE amp is actually active in the base graph.
//
// This drives `useLevelingFlow` directly (no device calls — `redistribution.plan` is a
// pure filter over already-resolved `RunItem`s) and proves the gate honors
// `baseActiveAmpCountByIndex` exclusively, including when the map has no entry for the
// slot at all.

import { describe, it, expect } from "vitest";
import { renderHook } from "@testing-library/react";

import { useLevelingFlow } from "../views/level/useLevelingFlow";
import type { RunItem } from "../views/level/leveling";

function baseItem(slot: number): RunItem {
  return {
    key: `p${String(slot)}`,
    slot,
    presetName: "Friedman HBE",
    isBase: true,
    sceneSlot: null,
    sceneName: "Base Preset",
    tag: null,
    instId: "",
    targetName: "Standard",
    status: "result",
    outcome: "done",
  };
}

function clampedSceneItem(slot: number, sceneSlot: number): RunItem {
  return {
    key: `s${String(slot)}:${String(sceneSlot)}`,
    slot,
    presetName: "Friedman HBE",
    isBase: false,
    sceneSlot,
    sceneName: `FS${String(sceneSlot + 1)}`,
    tag: `FS${String(sceneSlot + 1)}`,
    instId: "",
    targetName: "Standard",
    status: "result",
    outcome: "clamped",
  };
}

const AMP_A = {
  groupId: "G1",
  nodeId: "ampA",
  parameterId: "outputLevel",
  value: 0.5,
};
const AMP_B = {
  groupId: "G1",
  nodeId: "ampB",
  parameterId: "outputLevel",
  value: 0.3,
};

const baseDeps = {
  rows: [{ slot: 0, name: "Friedman HBE", empty: false }],
  store: null,
  sceneInfo: new Map(),
  footswitchInfo: new Map(),
  blocksByIndex: new Map(),
  silenceHintByIndex: new Map(),
  targetLufsByName: () => -20,
  deselectKeys: () => {
    /* no-op */
  },
  refresh: () => Promise.resolve(),
};

describe("useLevelingFlow — redistribute single-amp gate (issue 1 / A7)", () => {
  it("offers redistribution when baseActiveAmpCountByIndex reads 1, even with 2 amp candidates", () => {
    const { result } = renderHook(() =>
      useLevelingFlow({
        ...baseDeps,
        ampCandidates: new Map([[0, [AMP_A, AMP_B]]]),
        baseActiveAmpCountByIndex: new Map([[0, 1]]),
      }),
    );

    const items = [baseItem(0), clampedSceneItem(0, 0)];
    expect(result.current.redistribution.plan(items)).toEqual({
      presets: 1,
      scenes: 1,
    });
  });

  it("refuses redistribution when baseActiveAmpCountByIndex reads 2, even with 1 amp candidate", () => {
    const { result } = renderHook(() =>
      useLevelingFlow({
        ...baseDeps,
        ampCandidates: new Map([[0, [AMP_A]]]),
        baseActiveAmpCountByIndex: new Map([[0, 2]]),
      }),
    );

    const items = [baseItem(0), clampedSceneItem(0, 0)];
    expect(result.current.redistribution.plan(items)).toBeNull();
  });

  it("refuses redistribution when baseActiveAmpCountByIndex has no entry for the slot", () => {
    const { result } = renderHook(() =>
      useLevelingFlow({
        ...baseDeps,
        ampCandidates: new Map([[0, [AMP_A]]]),
        baseActiveAmpCountByIndex: new Map(),
      }),
    );
    const items = [baseItem(0), clampedSceneItem(0, 0)];
    expect(result.current.redistribution.plan(items)).toBeNull();
  });
});
