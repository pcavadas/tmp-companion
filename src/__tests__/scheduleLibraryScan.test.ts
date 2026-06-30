// src/__tests__/scheduleLibraryScan.test.ts — ordering contract for the startup
// signal-path-before-backup fix.
//
// Regression: when the connect snapshot arrives with NO active graph (graph=none),
// the monitor schedules a graph-retry ~3 s out — but the ~22 s library backup scan
// pauses the monitor the instant it starts, preempting that retry, so the hero
// signal path stays blank for the WHOLE backup (~26 s observed on HW). The fix:
// hold the backup scan until the active graph is in hand (bounded), so the path
// paints first. This pins that ordering with an injected clock (no real timers).

import { describe, it, expect, vi } from "vitest";

import { startScanAfterGraph } from "../lib/scheduleLibraryScan";
import type { ActiveGraph } from "../lib/types";

const graph: ActiveGraph = {
  name: "Plexi",
  slot: 7,
  template: null,
  split_mix: null,
  nodes: [],
  stages: [],
};

const immediateSleep = () => Promise.resolve();

describe("startScanAfterGraph — signal path before the backup scan", () => {
  it("scans immediately when the connect snapshot already has a graph", async () => {
    const getGraph = vi.fn<() => Promise<ActiveGraph | null>>();
    const startScan = vi.fn();
    await startScanAfterGraph({
      initialGraph: graph,
      getGraph,
      startScan,
      sleep: immediateSleep,
    });
    expect(startScan).toHaveBeenCalledTimes(1);
    expect(getGraph).not.toHaveBeenCalled(); // already have it — no poll
  });

  it("waits for the monitor's graph before starting the backup scan", async () => {
    // null, null, then the retry delivers the graph.
    const getGraph = vi
      .fn<() => Promise<ActiveGraph | null>>()
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(graph);
    const startScan = vi.fn(() => {
      // ORDERING INVARIANT: the scan must not start until a graph is in hand.
      expect(getGraph).toHaveBeenCalled();
    });
    const onGraph = vi.fn();

    await startScanAfterGraph({
      initialGraph: null,
      getGraph,
      startScan,
      onGraph,
      sleep: immediateSleep,
    });

    expect(onGraph).toHaveBeenCalledWith(graph); // hero gets the path…
    expect(startScan).toHaveBeenCalledTimes(1); // …then the backup runs
    expect(getGraph).toHaveBeenCalledTimes(3);
  });

  it("still scans (timeout) when no graph ever arrives — an idle/graphless unit", async () => {
    const getGraph = vi
      .fn<() => Promise<ActiveGraph | null>>()
      .mockResolvedValue(null);
    const startScan = vi.fn();
    const onGraph = vi.fn();

    await startScanAfterGraph({
      initialGraph: null,
      getGraph,
      startScan,
      onGraph,
      tries: 4,
      sleep: immediateSleep,
    });

    expect(onGraph).not.toHaveBeenCalled();
    expect(startScan).toHaveBeenCalledTimes(1); // backup still runs
    expect(getGraph).toHaveBeenCalledTimes(4); // bounded by `tries`
  });

  it("treats a getGraph error as 'not yet' and keeps polling, then scans", async () => {
    const getGraph = vi
      .fn<() => Promise<ActiveGraph | null>>()
      .mockRejectedValueOnce(new Error("device busy"))
      .mockResolvedValueOnce(graph);
    const startScan = vi.fn();
    const onGraph = vi.fn();

    await startScanAfterGraph({
      initialGraph: null,
      getGraph,
      startScan,
      onGraph,
      sleep: immediateSleep,
    });

    expect(onGraph).toHaveBeenCalledWith(graph);
    expect(startScan).toHaveBeenCalledTimes(1);
  });
});
