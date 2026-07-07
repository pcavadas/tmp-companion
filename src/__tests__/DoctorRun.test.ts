// src/__tests__/DoctorRun.test.ts — pure-function coverage for the Doctor run
// screen's live "about Ns left" estimate. No component rendering, no fake
// timers: estimateSecsLeft is a plain function of (remaining, avgMs, elapsed).

import { describe, expect, it } from "vitest";

import { estimateSecsLeft, avgSoundMs } from "../views/doctor/estimateSecsLeft";

describe("estimateSecsLeft", () => {
  it("full-queue start: no time elapsed on the current sound", () => {
    expect(estimateSecsLeft(5, 9000, 0)).toBe(45);
  });

  it("mid-sound countdown: counts down within the current sound", () => {
    expect(estimateSecsLeft(5, 9000, 4000)).toBe(41);
  });

  it("overrun holds at the floor instead of racing to 0", () => {
    expect(estimateSecsLeft(2, 9000, 15000)).toBe(9);
  });

  it("last sound overrun floors at 0", () => {
    expect(estimateSecsLeft(1, 9000, 20000)).toBe(0);
  });

  it("nothing remaining is always 0", () => {
    expect(estimateSecsLeft(0, 9000, 0)).toBe(0);
  });
});

describe("avgSoundMs", () => {
  it("no completions: returns the prior unchanged", () => {
    expect(avgSoundMs(15000, [0])).toBe(15000);
  });

  it("one 20s completion with a 15s prior nudges toward the measurement", () => {
    expect(avgSoundMs(15000, [0, 20000])).toBe(17500);
  });

  it("zero-length gaps (batched events) are skipped, not counted as 0s", () => {
    expect(avgSoundMs(15000, [0, 5000, 5000])).toBe((15000 + 5000) / 2);
  });

  it("several completions converge toward the measured mean", () => {
    // Five 10s completions against a 15s prior: pulls steadily toward 10s
    // rather than snapping to it (the prior still counts as one observation).
    const doneAts = [0, 10000, 20000, 30000, 40000, 50000];
    expect(avgSoundMs(15000, doneAts)).toBeCloseTo((15000 + 5 * 10000) / 6, 6);
  });
});

describe("estimateSecsLeft + avgSoundMs replay (HW scenario, preset 024)", () => {
  // The real measured hardware timeline: 3 sounds, prior 15 s, actual
  // durations 29.5 s / 13 s / 13 s — a slow first sound (per-preset read +
  // connect retries) followed by two normal ones. Under the OLD
  // mean-from-run-start model (avg = elapsed / completions, snapped only at
  // each completion) this made the on-screen label teleport 24s -> 59s the
  // moment the first sound finished — a +35 single-tick jump. The new
  // pseudo-count running-rate model bounds that same transition to +14 (see
  // the pinned assertion below), comfortably under the required <=15.
  it("bounds the upward jump, starts at 45, and ends at 0", () => {
    const prior = 15000;
    const total = 3;
    const durations = [29500, 13000, 13000];

    const completionTimes: number[] = [];
    let cursor = 0;
    for (const d of durations) {
      cursor += d;
      completionTimes.push(cursor);
    }
    const lastCompletion =
      completionTimes.length > 0
        ? completionTimes[completionTimes.length - 1]
        : 0;

    const series: number[] = [];
    for (let t = 0; t <= lastCompletion + 5000; t += 1000) {
      const doneAts = [0, ...completionTimes.filter((c) => c <= t)];
      const completions = doneAts.length - 1;
      const remaining = total - completions;
      const avgMs = avgSoundMs(prior, doneAts);
      const lastDoneAt = doneAts.length > 0 ? doneAts[doneAts.length - 1] : 0;
      series.push(estimateSecsLeft(remaining, avgMs, t - lastDoneAt));
    }

    expect(series.length > 0 ? series[0] : -1).toBe(45);
    expect(series.length > 0 ? series[series.length - 1] : -1).toBe(0);

    let maxJump = -Infinity;
    for (let i = 1; i < series.length; i++) {
      const jump = series[i] - series[i - 1];
      if (jump > maxJump) maxJump = jump;
    }
    // The only upward jump in the whole series happens at t=30000, the first
    // tick after the slow sound 1 completes (30 -> 44) — the old
    // mean-from-start model jumped +35 (24 -> 59) on this same timeline.
    expect(maxJump).toBeLessThanOrEqual(15);
  });
});
