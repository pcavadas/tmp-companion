// src/lib/scheduleLibraryScan.ts — hold the ~22 s library backup scan until the
// active signal path is in hand, so the hero paints right away.
//
// Why: the connect snapshot can arrive with graph=none (a flooded first
// handshake). The monitor then schedules a graph-retry ~3 s out — but the backup
// scan pauses the monitor the instant it starts, preempting that retry, so the
// hero signal path stays blank for the WHOLE backup (~26 s observed on HW). So we
// poll the monitor's cached graph (cheap, no device I/O) and only kick the backup
// once a graph lands — or after a bounded timeout, so an idle/graphless unit still
// scans. When the snapshot already carries a graph, we scan immediately.

import type { ActiveGraph } from "./types";

const defaultSleep = (ms: number): Promise<void> =>
  new Promise((r) => setTimeout(r, ms));

export interface ScanAfterGraphDeps {
  /** The active graph from the connect snapshot (null = the graph=none case). */
  initialGraph: ActiveGraph | null;
  /** Poll the monitor's currently-cached graph (no device op). */
  getGraph: () => Promise<ActiveGraph | null>;
  /** Start the backup scan (e.g. `ensureLibraryScan`, which is async). */
  startScan: () => void | Promise<void>;
  /** Forward a freshly-arrived graph to the hero (e.g. `setInitialGraph`). */
  onGraph?: (g: ActiveGraph) => void;
  /** Poll attempts before giving up and scanning anyway. tries×intervalMs ≈ the
   *  8 s connect deadline, so a unit that never reports a graph isn't stuck. */
  tries?: number;
  intervalMs?: number;
  sleep?: (ms: number) => Promise<void>;
}

export async function startScanAfterGraph(
  deps: ScanAfterGraphDeps,
): Promise<void> {
  const {
    initialGraph,
    getGraph,
    startScan,
    onGraph,
    tries = 16,
    intervalMs = 500,
    sleep = defaultSleep,
  } = deps;

  if (initialGraph == null) {
    for (let i = 0; i < tries; i++) {
      const g = await getGraph().catch(() => null); // an error = "not yet"
      if (g != null) {
        onGraph?.(g);
        break;
      }
      await sleep(intervalMs);
    }
  }
  void startScan(); // fire-and-forget — the scan drives its own store
}
