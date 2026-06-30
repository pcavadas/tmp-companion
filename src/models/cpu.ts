// src/models/cpu.ts — REAL per-block DSP cost, baked from the firmware binary.
//
// The Tone Master Pro caps each preset at a fixed share of its audio-core DSP
// budget; every block in the signal path draws against it. These figures are the
// device's OWN per-module costs — extracted from the `utilizationPercentage` /
// `utilizationBudget` JSON blob embedded in `tm-stomp-server` (fw 1.8.45, 468
// modules), NOT synthesized. Regenerate `model-cpu.json` when firmware revs (see
// that file's header). Keyed by ACD_ FenderId
// (= a catalog `bid`, or a device audioGraph node's model id).
import cpuData from "./model-cpu.json";
import { resolveDeviceId } from "./blockArt";
import { DASH } from "../lib/format";
import type { ActiveGraph } from "../lib/types";

/** Per-preset DSP budget (% of the audio core) — the device's `utilizationBudget`. */
export const CPU_BUDGET = cpuData.budget;

const BY_BID = cpuData.cpuByBid as Record<string, number>;
const has = (id: string) => Object.prototype.hasOwnProperty.call(BY_BID, id);

/** Real DSP cost (% of preset budget) for a block id, or `null` when the id is not
 *  a costed DSP module — mics and the FX-loop placement markers carry no separate
 *  cost. Strips merged cab/IR/convolution suffixes CHECK-FIRST (shared
 *  {@link resolveDeviceId}), so a live audioGraph node id such as
 *  `ACD_HiwattDR103CanModCabIR` resolves to its costed base. */
export function cpuForBid(bid: string | null | undefined): number | null {
  if (!bid) return null;
  const id = resolveDeviceId(bid, has);
  return has(id) ? BY_BID[id] : null;
}

/** Format a CPU cost as the UI string (e.g. `13.8%`); `null` renders as the
 *  shared no-fabricate {@link DASH}. */
export function cpuStr(cpu: number | null, dash: string = DASH): string {
  return cpu == null ? dash : `${cpu.toFixed(1)}%`;
}

/** Total DSP cost of an active preset = sum of every block's real cost. `nodes` is
 *  the device's complete flat block list (the structured stage/lane views are
 *  derived from it — see session::active_graph_from_value), so summing it counts
 *  each block exactly once. Bypassed blocks still occupy their DSP slot, so they
 *  count too. Returns null when there's no active graph. */
export function presetCpu(graph: ActiveGraph | null): number | null {
  if (!graph) return null;
  const total = graph.nodes.reduce(
    (sum, n) => sum + (cpuForBid(n.model) ?? 0),
    0,
  );
  return Math.round(total * 10) / 10;
}
