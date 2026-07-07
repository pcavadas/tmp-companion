// src/__tests__/DoctorRun.test.ts — pure-function coverage for the Doctor run
// screen's live "about Ns left" estimate. No component rendering, no fake
// timers: estimateSecsLeft is a plain function of (remaining, avgMs, elapsed).

import { describe, expect, it } from "vitest";

import { estimateSecsLeft } from "../views/doctor/estimateSecsLeft";

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
