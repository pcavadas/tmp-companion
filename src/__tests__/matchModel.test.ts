// Pins the "Match reference" EQ-move math: pure delta → EQ-10 move mapping,
// no device I/O. Derivation notes sit inline per case (see matchModel.ts for
// the verified band-center table + the EQ10_BANDS mirror of doctor.rs).

import { describe, it, expect } from "vitest";

import {
  matchDeltas,
  eqMovesFor,
  matchResidualLarge,
  lastGuitarGroup,
} from "../views/doctor/matchModel";
import type { GraphNode } from "../lib/types";

const GUITAR_LABELS = ["Lows", "Low-mids", "Mids", "High-mids", "Highs", "Air"];

describe("matchDeltas", () => {
  it("is ref minus sound, per band", () => {
    expect(matchDeltas([1, 2, 3], [0, 0, 0])).toEqual([1, 2, 3]);
    expect(matchDeltas([-2, 5], [3, 1])).toEqual([-5, 4]);
  });
});

describe("eqMovesFor", () => {
  // Hand-derived: each 6-band guitar center's log-nearest EQ-10 band (see
  // matchModel.ts's BAND_CENTER_HZ/EQ10_BANDS) is Lows→62Hz, Low-mids→250Hz,
  // Mids→500Hz, High-mids→2kHz, Highs→4kHz, Air→8kHz — all distinct, so this
  // case exercises straight clamp/round/drop with no band collisions.
  it("maps each family band to its log-nearest EQ-10 band, clamped + rounded + drop-filtered", () => {
    const deltas = [3, -8, 0.5, 6, -2, 1];
    const moves = eqMovesFor(deltas, GUITAR_LABELS);
    expect(moves).toEqual([
      { controlId: "gain62hz", gainDb: 3 }, // Lows: 3 → unclamped, exact
      { controlId: "gain250hz", gainDb: -6 }, // Low-mids: -8 clamped to -6
      { controlId: "gain2khz", gainDb: 6 }, // High-mids: 6 → unclamped
      { controlId: "gain4khz", gainDb: -2 }, // Highs: -2 → unclamped
      // Mids (0.5) and Air (1) are both < 1.5 dB → dropped.
    ]);
  });

  it("averages multiple family bands that land on the same EQ-10 band", () => {
    // Two hand-picked bands both nearest gain62hz (62 Hz) — the averaging
    // path, independent of any real family layout.
    const moves = eqMovesFor([4, 2], ["Lows", "Lows"]);
    expect(moves).toEqual([{ controlId: "gain62hz", gainDb: 3 }]);
  });

  it("clamps to +/-6 dB", () => {
    expect(eqMovesFor([10], ["Lows"])).toEqual([
      { controlId: "gain62hz", gainDb: 6 },
    ]);
    expect(eqMovesFor([-10], ["Lows"])).toEqual([
      { controlId: "gain62hz", gainDb: -6 },
    ]);
  });

  it("rounds to the nearest 0.5 dB", () => {
    expect(eqMovesFor([3.24], ["Lows"])).toEqual([
      { controlId: "gain62hz", gainDb: 3 },
    ]);
    expect(eqMovesFor([3.26], ["Lows"])).toEqual([
      { controlId: "gain62hz", gainDb: 3.5 },
    ]);
  });

  it("drops moves under 1.5 dB (post-rounding), keeps a move at exactly 1.5 dB", () => {
    // 1.2 rounds to the nearest 0.5 = 1.0, which is < 1.5 → dropped. The drop
    // check runs on the ROUNDED value (what the player would actually see).
    expect(eqMovesFor([1.2], ["Lows"])).toEqual([]);
    expect(eqMovesFor([1.5], ["Lows"])).toEqual([
      { controlId: "gain62hz", gainDb: 1.5 },
    ]);
  });

  it("ignores a band with no known center (defensive — shouldn't occur on real data)", () => {
    expect(eqMovesFor([5, 5], ["Lows", "Bogus"])).toEqual([
      { controlId: "gain62hz", gainDb: 5 },
    ]);
  });
});

function node(groupId: string, nodeId: string, model: string): GraphNode {
  return {
    group_id: groupId,
    node_id: nodeId,
    model,
    bypassed: false,
    params: {},
  };
}

describe("lastGuitarGroup", () => {
  it("is the guitar group when it's the chain tail", () => {
    const nodes = [
      node("G1", "n1", "ACD_TweedDeluxe"),
      node("G1", "n2", "ACD_CabSimTMS"),
    ];
    expect(lastGuitarGroup(nodes)).toBe("G1");
  });

  it("is still the guitar group when a mic group trails it", () => {
    const nodes = [
      node("G1", "n1", "ACD_TweedDeluxe"),
      node("G1", "n2", "ACD_CabSimTMS"),
      node("M1", "n3", "ACD_Mic1"),
    ];
    expect(lastGuitarGroup(nodes)).toBe("G1");
  });

  it("picks the LAST guitar group when there are several", () => {
    const nodes = [
      node("G1", "n1", "ACD_TweedDeluxe"),
      node("G2", "n2", "ACD_Overdrive"),
      node("G2", "n3", "ACD_CabSimTMS"),
    ];
    expect(lastGuitarGroup(nodes)).toBe("G2");
  });

  it("is null when the chain has no guitar nodes", () => {
    const nodes = [node("M1", "n1", "ACD_Mic1")];
    expect(lastGuitarGroup(nodes)).toBeNull();
  });

  it("is null for an empty chain", () => {
    expect(lastGuitarGroup([])).toBeNull();
  });
});

describe("matchResidualLarge", () => {
  it("is true when any band delta exceeds the +/-6 dB clamp", () => {
    expect(matchResidualLarge([1, 2, 7])).toBe(true);
    expect(matchResidualLarge([1, 2, -6.5])).toBe(true);
  });

  it("is false when every delta is within +/-6 dB (exactly 6 is not 'large')", () => {
    expect(matchResidualLarge([1, 2, 6])).toBe(false);
    expect(matchResidualLarge([-6, 0, 3])).toBe(false);
  });
});
