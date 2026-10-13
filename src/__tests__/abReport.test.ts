// Pins the applied-prescription A/B helpers: the level match (#257) and the
// before → after finding line (#256).

import { describe, it, expect } from "vitest";

import { findingCheckLine, levelMatch } from "../views/doctor/abReport";

describe("levelMatch", () => {
  it("turns the louder clip down to the quieter one and says the real gap", () => {
    // After is 3 dB quieter: Before plays at −3 dB, After as captured.
    const m = levelMatch({ beforeLufs: -18, afterLufs: -21 });
    expect(m?.afterVolume).toBe(1);
    expect(m?.beforeVolume).toBeCloseTo(10 ** (-3 / 20), 12);
    expect(m?.line).toBe(
      "After is 3.0 dB quieter. Levels matched for comparison.",
    );
    // And the other way round.
    const louder = levelMatch({ beforeLufs: -21, afterLufs: -19.8 });
    expect(louder?.beforeVolume).toBe(1);
    expect(louder?.afterVolume).toBeCloseTo(10 ** (-1.2 / 20), 12);
    expect(louder?.line).toBe(
      "After is 1.2 dB louder. Levels matched for comparison.",
    );
  });

  it("says nothing is different under half a dB, and still matches", () => {
    const m = levelMatch({ beforeLufs: -18, afterLufs: -18.2 });
    expect(m?.line).toBe("Both clips play at the same level.");
    expect(m?.beforeVolume).toBeCloseTo(10 ** (-0.2 / 20), 12);
  });

  it("is null without both loudnesses", () => {
    expect(levelMatch(null)).toBeNull();
    expect(levelMatch(undefined)).toBeNull();
    expect(levelMatch({ beforeLufs: -18, afterLufs: Number.NaN })).toBeNull();
  });
});

describe("findingCheckLine", () => {
  it("reports a cleared finding", () => {
    expect(
      findingCheckLine("Muddy", { key: "muddy", before: 2.46, after: null }),
    ).toBe("Muddy: +2.5 dB over → cleared by the fix.");
  });

  it("reports a partial fix", () => {
    expect(
      findingCheckLine("Muddy", { key: "muddy", before: 6.3, after: 1.1 }),
    ).toBe("Muddy: +6.3 → +1.1 dB over. Better, not fully cleared.");
  });

  it("flags a fix that made no measurable difference", () => {
    expect(
      findingCheckLine("Harsh", { key: "harsh", before: 2.5, after: 2.4 }),
    ).toBe(
      "Harsh: +2.5 → +2.4 dB over. No measurable change, so the fix may not have landed. Discard and try again.",
    );
  });

  it("flags a fix that made the finding worse", () => {
    expect(
      findingCheckLine("Muddy", { key: "muddy", before: 2.0, after: 2.5 }),
    ).toBe("Muddy: +2.0 → +2.5 dB over. Worse after the fix. Discard it.");
  });

  it("stays quiet when there is nothing to compare", () => {
    expect(findingCheckLine("Muddy", null)).toBeNull();
    expect(findingCheckLine("Muddy", undefined)).toBeNull();
    expect(
      findingCheckLine("Muddy", { key: "muddy", before: null, after: null }),
    ).toBeNull();
  });
});
