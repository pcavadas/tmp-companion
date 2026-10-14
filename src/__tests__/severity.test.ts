// Pins presetWorstSev's tinting so the header/badge severity never contradicts
// presetLookCount (which counts a scene-consistency finding).

import { describe, it, expect } from "vitest";

import {
  diagKicker,
  isPossible,
  presetWorstSev,
  sortedDiags,
} from "../views/doctor/severity";
import type {
  DoctorDiag,
  DoctorPresetResult,
  DoctorSceneConsistency,
} from "../lib/types";

function preset(
  sceneConsistency: DoctorSceneConsistency | null,
): DoctorPresetResult {
  return { listIndex: 0, sounds: [], sceneConsistency, levelingDamage: [] };
}

function sceneJump(worstDeltaDb: number): DoctorSceneConsistency {
  return { rows: [], worstName: "Lead", worstDeltaDb, rx: [] };
}

describe("presetWorstSev", () => {
  it("is 'ok' with no diagnoses and no scene jump", () => {
    expect(presetWorstSev(preset(null))).toBe("ok");
  });

  it("tints a MODERATE scene jump (≤5 dB) as 'med', not 'ok'", () => {
    // The badge counts the scene finding, so the tint must not read "all clear".
    expect(presetWorstSev(preset(sceneJump(3)))).toBe("med");
    expect(presetWorstSev(preset(sceneJump(-4)))).toBe("med");
  });

  it("bumps a BIG scene jump (>5 dB) to 'high'", () => {
    expect(presetWorstSev(preset(sceneJump(6)))).toBe("high");
  });
});

function diag(sev: "high" | "med", severity: number): DoctorDiag {
  return {
    key: "muddy",
    label: "Muddy",
    sev,
    severity,
    // A band rule: σ 0.4 dB.
    z: severity / 0.4,
    bands: [],
    detail: "",
    explain: "",
    rx: [],
    fromLevel: "rehearsal",
  };
}

describe("diagKicker", () => {
  it("reads 'Worth a look' for a near-threshold ('possible') high-sev diag", () => {
    // z < POSSIBLE_MAX_Z (2.5σ, 1.0 dB on a band rule) — the muted "Possible X" chip
    // must not sit beside a bold "NEEDS ATTENTION" kicker.
    expect(diagKicker(diag("high", 0.4))).toBe("Worth a look");
  });

  it("reads 'Needs attention' for a confidently-past-threshold high-sev diag", () => {
    expect(diagKicker(diag("high", 1.0))).toBe("Needs attention");
    expect(diagKicker(diag("high", 4.2))).toBe("Needs attention");
  });

  it("reads 'Worth a look' for a med-sev diag regardless of severity", () => {
    expect(diagKicker(diag("med", 6))).toBe("Worth a look");
  });
});

describe("hedging and ranking across rule units (#253)", () => {
  // A 0.8 dB/oct tilt is 16σ (σ 0.05 dB/oct); a 0.9 dB band margin is 2.25σ
  // (σ 0.4 dB). Raw severity would rank and hedge them the other way round.
  const tilt: DoctorDiag = {
    ...diag("med", 0.8),
    key: "dark",
    label: "Dark",
    z: 16,
  };
  const band: DoctorDiag = { ...diag("med", 0.9), z: 2.25 };

  it("hedges on z, not on the raw margin", () => {
    expect(isPossible(tilt)).toBe(false);
    expect(isPossible(band)).toBe(true);
  });

  it("ranks the more certain finding first within a tint", () => {
    expect(sortedDiags([band, tilt]).map((d) => d.key)).toEqual([
      "dark",
      "muddy",
    ]);
  });
});
