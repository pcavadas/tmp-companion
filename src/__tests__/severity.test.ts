// Pins presetWorstSev's tinting so the header/badge severity never contradicts
// presetLookCount (which counts a scene-consistency finding).

import { describe, it, expect } from "vitest";

import { presetWorstSev } from "../views/doctor/severity";
import type { DoctorPresetResult, DoctorSceneConsistency } from "../lib/types";

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
