// src/__tests__/liveMeterStore.test.tsx — the live meter's ~5 Hz ticks re-render only
// its reader (RunPage), never a LevelView-level consumer of `useLevelingFlow`: the meter
// used to ride the wizard store, so every tick re-rendered LevelView, the preset list's
// shell and the footer. Also pins the hook's return as stable across a re-render, so
// LevelView's `handleLevel` callback is not rebuilt each time.

import { act, render, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import {
  IDLE_METER,
  pushLiveMeter,
  setLiveMeter,
  useLiveMeter,
} from "../views/level/liveMeterStore";
import { resetLevelFlow } from "../views/level/levelRunStore";
import { useLevelingFlow } from "../views/level/useLevelingFlow";
import { makeLevelingFlowDeps } from "./commandMocks";

beforeEach(() => {
  resetLevelFlow();
  setLiveMeter(IDLE_METER);
});

describe("live meter store", () => {
  it("a meter tick re-renders the meter's reader and not a useLevelingFlow consumer", () => {
    const deps = makeLevelingFlowDeps();
    let flowRenders = 0;
    let meterRenders = 0;
    function FlowConsumer() {
      useLevelingFlow(deps);
      flowRenders++;
      return null;
    }
    function MeterReader() {
      useLiveMeter();
      meterRenders++;
      return null;
    }
    render(
      <>
        <FlowConsumer />
        <MeterReader />
      </>,
    );
    const flowBefore = flowRenders;
    const meterBefore = meterRenders;

    act(() => {
      pushLiveMeter(-23.1, -22);
      pushLiveMeter(-22.8, -21);
    });

    expect(meterRenders).toBeGreaterThan(meterBefore);
    expect(flowRenders).toBe(flowBefore);
  });

  it("keeps the last 24 momentary hops and clears to idle", () => {
    const { result } = renderHook(() => useLiveMeter());
    act(() => {
      for (let i = 0; i < 30; i++) pushLiveMeter(-20, i);
    });
    expect(result.current.lufs).toBe(-20);
    expect(result.current.trace).toHaveLength(24);
    expect(result.current.trace[0]).toBe(6);
    act(() => {
      setLiveMeter(IDLE_METER);
    });
    expect(result.current).toBe(IDLE_METER);
  });

  it("useLevelingFlow's return is the same object across a re-render with unchanged inputs", () => {
    const deps = makeLevelingFlowDeps();
    const { result, rerender } = renderHook(() => useLevelingFlow(deps));
    const first = result.current;
    rerender();
    expect(result.current).toBe(first);
    expect(result.current.openFlow).toBe(first.openFlow);
  });
});
