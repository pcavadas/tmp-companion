// Guards the REAL per-block DSP-cost layer (models/cpu.ts + model-cpu.json) — the
// values baked from tm-stomp-server's utilizationBudget/utilizationPercentage blob.
// A drift here means model-cpu.json was regenerated against different firmware or
// the lookup regressed (suffix stripping, null handling, preset summing).

import { describe, it, expect } from "vitest";
import { CPU_BUDGET, cpuForBid, cpuStr, presetCpu } from "../models/cpu";
import { MODELS } from "../models/catalog";
import type { ActiveGraph, GraphNode } from "../lib/types";

const node = (model: string, bypassed = false): GraphNode => ({
  group_id: "G1",
  node_id: model,
  model,
  bypassed,
  params: {},
});

const graphOf = (models: string[]): ActiveGraph => ({
  name: "T",
  slot: 0,
  template: "gtrSeries",
  split_mix: null,
  nodes: models.map((m) => node(m)),
  stages: [{ kind: "series", blocks: models.map((m) => node(m)) }],
});

describe("CPU cost layer", () => {
  it("exposes the device's per-preset budget", () => {
    expect(CPU_BUDGET).toBe(76.5);
  });

  it("returns the real per-block cost by FenderId", () => {
    // Values verified against the tm-stomp-server 1.8.45 utilization blob.
    expect(cpuForBid("ACD_GuitarSynth")).toBe(36);
    expect(cpuForBid("ACD_UserIRTMS")).toBe(4.1);
  });

  it("no single block exceeds the budget (the cap binds on combinations)", () => {
    const max = Math.max(
      ...MODELS.map((m) => m.cpu ?? 0).filter((n) => Number.isFinite(n)),
    );
    expect(max).toBeLessThan(CPU_BUDGET);
  });

  it("strips merged cab/IR/conv suffixes CHECK-FIRST", () => {
    // A live audioGraph amp id can carry a CabIR suffix the costed base lacks.
    const base = cpuForBid("ACD_GuitarSynth");
    expect(cpuForBid("ACD_GuitarSynthCabIR")).toBe(base);
    // …but an id catalogued WITH the suffix matches directly (never over-stripped).
    expect(cpuForBid("ACD_TwinReverb65VibratoCabIRConvRvb")).toBe(31.8);
  });

  it("returns null for non-DSP ids (mics / FX-loop markers / unknown)", () => {
    expect(cpuForBid("ACD_FxLoop1")).toBeNull();
    expect(cpuForBid(null)).toBeNull();
    expect(cpuForBid("ACD_NotAReal_Block")).toBeNull();
  });

  it("formats costs, dashing nulls", () => {
    expect(cpuStr(13.8)).toBe("13.8%");
    expect(cpuStr(18)).toBe("18.0%");
    expect(cpuStr(null)).toBe("—");
  });

  it("sums a preset's blocks (uncosted blocks contribute 0)", () => {
    const a = cpuForBid("ACD_GuitarSynth");
    const b = cpuForBid("ACD_UserIRTMS");
    if (a === null) throw new Error("expected a cost for ACD_GuitarSynth");
    if (b === null) throw new Error("expected a cost for ACD_UserIRTMS");
    expect(
      presetCpu(graphOf(["ACD_GuitarSynth", "ACD_UserIRTMS", "ACD_FxLoop1"])),
    ).toBe(Math.round((a + b) * 10) / 10);
    expect(presetCpu(null)).toBeNull();
  });

  it("attaches a real cost to most catalog models, null to mics/loops", () => {
    const costed = MODELS.filter((m) => m.cpu != null);
    expect(costed.length).toBeGreaterThan(330);
    const mic = MODELS.find((m) => m.cat === "Microphones");
    expect(mic?.cpu).toBeNull();
  });
});
