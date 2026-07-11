// buildLevelJob threads the chosen instrument profile's id onto the wire job so the
// backend can resolve its Tier-2 calibration capture as the re-amp stimulus.

import { describe, expect, it } from "vitest";

import { buildLevelJob } from "../views/level/leveling";
import type { Profile } from "../lib/types";

const profile: Profile = {
  id: "p1",
  name: "Strat",
  topology_id: "guitar-single",
  calibration_lufs: -20,
};

describe("buildLevelJob", () => {
  it("carries the profile's id as profile_id", () => {
    const job = buildLevelJob(3, -18, profile, true);
    expect(job.profile_id).toBe("p1");
  });

  it("emits profile_id: null when no profile is chosen", () => {
    const job = buildLevelJob(3, -18, null, true);
    expect(job.profile_id).toBeNull();
  });
});
