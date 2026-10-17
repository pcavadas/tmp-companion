// Pins the "To sound like …" EQ-move math: pure delta → EQ-10 move mapping,
// no device I/O. Derivation notes sit inline per case (see matchModel.ts for
// the verified band-center table + the EQ10_BANDS mirror of doctor.rs).

import { describe, it, expect } from "vitest";

import {
  eqMovesFor,
  existingEq10,
  matchDeltas,
  matchResidualLarge,
} from "../views/doctor/matchModel";
import type { GraphNode } from "../lib/types";

const GUITAR_LABELS = ["Lows", "Low mids", "Mids", "High mids", "Highs", "Air"];

/** `balanceDb` as the backend builds it: each band's dB minus the mean of ALL
 *  bands (`doctor.rs`'s `balance`). */
function balance(db: number[]): number[] {
  const mean = db.reduce((a, b) => a + b, 0) / db.length;
  return db.map((d) => d - mean);
}

describe("matchDeltas", () => {
  // BUG→GATE (#254): two sounds identical except the reference has 30 dB more
  // Air. The all-band mean moved 5 dB, so every other band read a −5 dB delta
  // and the card prescribed five cuts nobody asked for. Only the Air move (and
  // the too-big-for-EQ advisory) may remain.
  it("prescribes only the Air move when only Air differs", () => {
    const body = [-5, -2, -5, 13, 13.5];
    const sound = balance([...body, -40]);
    const reference = balance([...body, -10]);
    const deltas = matchDeltas(reference, sound, GUITAR_LABELS);
    expect(deltas.slice(0, 5).every((d) => Math.abs(d) < 1e-9)).toBe(true);
    expect(deltas[5]).toBeCloseTo(30, 9);
    expect(eqMovesFor(deltas, GUITAR_LABELS)).toEqual([
      { controlId: "gain8khz", gainDb: 6 },
    ]);
    expect(matchResidualLarge(deltas)).toBe(true);
  });

  it("keeps a body-band difference intact whatever Air does", () => {
    const sound = balance([0, 0, 0, 0, 0, -40]);
    const reference = balance([0, 4, 0, 0, 0, -10]);
    const deltas = matchDeltas(reference, sound, GUITAR_LABELS);
    expect(deltas[1]).toBeCloseTo(4, 9);
    expect(deltas[0]).toBeCloseTo(0, 9);
  });

  it("keeps Bass VI's Sub band out of the centring", () => {
    const labels = ["Sub", ...GUITAR_LABELS];
    const sound = balance([-30, 0, 0, 0, 0, 0, -20]);
    const reference = balance([0, 0, 0, 0, 0, 0, -20]);
    const deltas = matchDeltas(reference, sound, labels);
    expect(deltas[0]).toBeCloseTo(30, 9);
    expect(deltas.slice(1).every((d) => Math.abs(d) < 1e-9)).toBe(true);
  });

  it("centres an even body count on the mean of its two middle deltas", () => {
    // Labels outside the known set count as body; four body bands here.
    const labels = ["Lows", "Low mids", "Mids", "High mids"];
    expect(matchDeltas([1, 2, 3, 10], [0, 0, 0, 0], labels)).toEqual([
      -1.5, -0.5, 0.5, 7.5,
    ]);
  });
});

describe("eqMovesFor", () => {
  // Hand-derived: each 6-band guitar center's log-nearest EQ-10 band (see
  // matchModel.ts's BAND_CENTER_HZ/EQ10_BANDS) is Lows→62Hz, Low mids→250Hz,
  // Mids→500Hz, High mids→2kHz, Highs→4kHz, Air→8kHz — all distinct, so this
  // case exercises straight clamp/round/drop with no band collisions.
  it("maps each family band to its log-nearest EQ-10 band, clamped + rounded + drop-filtered", () => {
    const deltas = [3, -8, 0.5, 6, -2, 1];
    const moves = eqMovesFor(deltas, GUITAR_LABELS);
    expect(moves).toEqual([
      { controlId: "gain62hz", gainDb: 3 }, // Lows: 3 → unclamped, exact
      { controlId: "gain250hz", gainDb: -6 }, // Low mids: -8 clamped to -6
      { controlId: "gain2khz", gainDb: 6 }, // High mids: 6 → unclamped
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

  it("skips the bass-vi Sub band entirely (no known center), leaving Lows undiluted", () => {
    // Sub has no entry in BAND_CENTER_HZ (deleted — its own log-nearest EQ-10
    // band is gain31hz, HW-unverified/no-op-prone), so it never joins the
    // sums/counts map at all — it does NOT get averaged into Lows' gain62hz
    // move, which would happen if Sub's delta (10, wildly different from
    // Lows' 3) were silently folded in. Distinct deltas make that provable:
    // if Sub were included, the result would differ from the plain 3.
    expect(eqMovesFor([10, 3], ["Sub", "Lows"])).toEqual([
      { controlId: "gain62hz", gainDb: 3 },
    ]);
  });
});

function node(
  groupId: string,
  nodeId: string,
  model: string,
  overrides: Partial<GraphNode> = {},
): GraphNode {
  return {
    group_id: groupId,
    node_id: nodeId,
    model,
    bypassed: false,
    params: {},
    ...overrides,
  };
}

const EQ10 = "ACD_TenBandEQStereo";

describe("existingEq10", () => {
  it("finds a non-bypassed EQ-10", () => {
    const eq = node("G1", "eq1", EQ10, { params: { gain62hz: 4 } });
    expect(existingEq10([node("G1", "amp1", "ACD_TweedDeluxe"), eq])).toBe(eq);
  });

  it("ignores a bypassed EQ-10", () => {
    const nodes = [
      node("G1", "amp1", "ACD_TweedDeluxe"),
      node("G1", "eq1", EQ10, { bypassed: true }),
    ];
    expect(existingEq10(nodes)).toBeNull();
  });

  it("ignores other models", () => {
    const nodes = [
      node("G1", "amp1", "ACD_TweedDeluxe"),
      node("G1", "cab1", "ACD_CabSimTMS"),
    ];
    expect(existingEq10(nodes)).toBeNull();
  });

  it("is null when there's no EQ-10 at all", () => {
    expect(existingEq10([])).toBeNull();
  });

  it("picks the LAST non-bypassed EQ-10 when several exist (mirrors doctor.rs's graph_facts, which has no is_none() guard on f.eq10)", () => {
    const last = node("G2", "eq2", EQ10);
    const nodes = [node("G1", "eq1", EQ10), last];
    expect(existingEq10(nodes)).toBe(last);
  });
});
