// src/__tests__/severity.test.ts — the tone a Doctor finding shows and the one order
// every list follows. A borderline ("possible") finding shows caution and sorts below
// every confident danger finding, so where a row sits always agrees with what it shows;
// the hedge and the ranking key on `z`, never on a rule's raw margin (#253).

import { describe, it, expect } from "vitest";

import {
  findingTone,
  isPossible,
  possibleLabel,
  sceneBarTone,
  sceneLoudnessTone,
  sortedDiags,
  soundTone,
} from "../views/doctor/severity";
import type {
  DoctorDiag,
  DoctorSceneConsistency,
  DoctorSoundResult,
} from "../lib/types";

function diag(
  sev: "high" | "med",
  severity: number,
  key = "muddy",
): DoctorDiag {
  return {
    key,
    label: key === "muddy" ? "Muddy" : key,
    sev,
    severity,
    // A band rule: σ 0.4 dB.
    z: severity / 0.4,
    bands: [],
    direction: "strong",
    detail: "",
    explain: "",
    rx: [],
    fromLevel: "rehearsal",
  };
}

function sceneJump(worstDeltaDb: number): DoctorSceneConsistency {
  return { rows: [], worstName: "Lead", worstDeltaDb, rx: [] };
}

describe("findingTone and the borderline label", () => {
  it("shows a confident high finding as danger", () => {
    expect(findingTone(diag("high", 4.2))).toBe("danger");
    expect(possibleLabel(diag("high", 4.2))).toBe("Muddy");
  });

  it("shows a borderline high finding as caution, named '(borderline)'", () => {
    // z < POSSIBLE_MAX_Z (2.5σ, 1.0 dB on a band rule).
    const d = diag("high", 0.4);
    expect(isPossible(d)).toBe(true);
    expect(findingTone(d)).toBe("caution");
    expect(possibleLabel(d)).toBe("Muddy (borderline)");
  });

  it("shows a medium finding as caution", () => {
    expect(findingTone(diag("med", 6))).toBe("caution");
  });
});

describe("worst first agrees with what the row shows", () => {
  it("never sorts a borderline high finding above a confident danger one", () => {
    const borderline = { ...diag("high", 0.4, "boomy"), z: 2.4 };
    const real = diag("high", 4.2, "muddy");
    expect(sortedDiags([borderline, real]).map((d) => d.key)).toEqual([
      "muddy",
      "boomy",
    ]);
  });

  it("sorts a borderline high finding with the caution findings, by confidence", () => {
    const borderline = { ...diag("high", 0.4, "boomy"), z: 1.0 };
    const med = { ...diag("med", 3, "dark"), z: 7.5 };
    expect(sortedDiags([borderline, med]).map((d) => d.key)).toEqual([
      "dark",
      "boomy",
    ]);
  });

  it("gives a sound the worst tone its findings show", () => {
    const sound = (diags: DoctorDiag[]) =>
      ({ diags }) as unknown as DoctorSoundResult;
    expect(soundTone(sound([]))).toBeNull();
    expect(soundTone(sound([diag("high", 0.4)]))).toBe("caution");
    expect(soundTone(sound([diag("med", 6), diag("high", 4.2)]))).toBe(
      "danger",
    );
  });
});

describe("sceneLoudnessTone", () => {
  it("is caution for a moderate jump and danger past 5 dB", () => {
    expect(sceneLoudnessTone(sceneJump(3))).toBe("caution");
    expect(sceneLoudnessTone(sceneJump(-4))).toBe("caution");
    expect(sceneLoudnessTone(sceneJump(6))).toBe("danger");
    expect(sceneLoudnessTone(sceneJump(-6))).toBe("danger");
  });

  it("colours each bar on the row's own scale, so the worst bar matches the row's dot", () => {
    expect(sceneBarTone(0.5)).toBeNull();
    expect(sceneBarTone(-3)).toBeNull();
    expect(sceneBarTone(4)).toBe("caution");
    expect(sceneBarTone(-35)).toBe("danger");
    for (const worst of [4, -4.5, 6, -35]) {
      expect(sceneBarTone(worst)).toBe(sceneLoudnessTone(sceneJump(worst)));
    }
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

  it("ranks the more certain finding first within a tone", () => {
    expect(sortedDiags([band, tilt]).map((d) => d.key)).toEqual([
      "dark",
      "muddy",
    ]);
  });
});
