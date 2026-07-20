// Pins presetWorstSev's tinting so the header/badge severity never contradicts
// presetLookCount (which counts a scene-consistency finding).

import { describe, it, expect } from "vitest";

import { diagKicker, presetWorstSev } from "../views/doctor/severity";
import type {
  DoctorDiag,
  DoctorPresetResult,
  DoctorSceneConsistency,
} from "../lib/types";

function preset(
  sceneConsistency: DoctorSceneConsistency | null,
): DoctorPresetResult {
  return { listIndex: 0, sounds: [], sceneConsistency };
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
    bands: [],
    detail: "",
    explain: "",
    rx: [],
    fromLevel: "rehearsal",
  };
}

describe("diagKicker", () => {
  it("reads 'Worth a look' for a near-threshold ('possible') high-sev diag", () => {
    // severity < POSSIBLE_MAX_SEVERITY (1.0) — the muted "Possible X" chip
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
