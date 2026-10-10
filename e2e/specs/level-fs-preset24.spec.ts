import type { Page } from "@playwright/test";
import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  baseLevelJob,
  clearScenario,
  ensureScenario,
  expectReampBalanced,
  invoke,
  isOnline,
  LEVEL_T,
  type LevelBlock,
  reampCounters,
  reampOff,
} from "../fixtures/scenario";

// COVERAGE rows 36, 16 — the stale-load incident fixture, the FS level opt-in BAKE
// lane it runs on, AND (P4-B, the Plumes/BD2/OCD leveling-regression fix) the first proof
// that a base-engaged SaturatedPedal paired with a sub-1.0 amp fader actually reaches
// target on the FIRST run. 405 was amended in place (see e2e/fixtures/COVERAGE.md and
// scenario-loudness.json's own "405" comment): `presetLevel` 1.0→0.27, the Twin's
// `outputLevel` 1.0→0.28, and Rat flipped base-ON (bypass:false, volume:0.62). Pre-fix, base
// leveling at -23 clamped ~5.2 dB short (`SceneCeiling`, `final_level` saved at 1.0) because
// all the loudness lived in Rat's own base-engaged pedal while the Twin's fader sat at 0.28 —
// the Friedman-era changes (#160–#166) removed every remedy for this shape. Post-fix,
// `headroom_trade::plan_level_pair` finds base's gap (`G≈+16.4`) exceeds the pure-presetLevel
// headroom (`P_up≈+11.1`), enters the BOOST regime, pins `presetLevel` at its ceiling (1.0)
// and raises the Twin's fader from 0.28 to ≈0.498 to close the rest — closed-loop verified,
// one save carrying both halves.
// BUG→GATE (2026-08-02 HW incident, fw 1.8.45): the footswitch batch's load 2 s after the
// base save materialized the pre-run presetLevel, so all 4 pedal sweeps ran +5.2 LU hot. On
// fw 1.8.58 a save is durable the moment it returns (tmp-audit Q34), so that load sees it.
//
// This file is the offline, deterministic, sim-layer proof that the WHOLE stack — the base
// BOOST (presetLevel + fader, one save), the footswitch batch right after it, and the
// FS_TOL_LU=0.1 tightened acceptance — lands base AND all 4 pedals within tolerance of their
// targets on the FIRST run, no clamp, both as REPORTED by the solve and as RE-MEASURED from
// the saved (persisted) state afterward, with BOTH halves of the boosted pair (presetLevel
// and the Twin's fader) surviving the footswitch batch's own load → save.
//
// PRESET24 (E2E Preset24, slot 405): 4 drive pedals (Plumes/BluesDriver/ObsessiveDrive/Rat,
// ftsw indices 5-8, matching the real "TR+BD2+BMP"-class preset) feeding a saturated amp
// (Twin) into a cab, no scenes. Each pedal's own knob follows notes/leveling.md's
// silent→cliff→plateau curve (`sim_device::saturated_pedal_lufs`) — EXACT and deterministic
// offline, unlike the stimulus-scaling slack the flat-C sidecar model carries elsewhere, so a
// tight ±0.1 LU assertion is meaningful here (see e2e/fixtures/scenario-loudness.json's "405"
// entry for the C/PT math, including the two measurement regimes P4-B introduced). All 4
// pedals stay on the BAKE path (off-in-base + sole owner + no scenes), so the leveler writes
// straight onto the block and the sim's `SavedDoc` overlay (sim_device.rs)
// round-trips the baked value through a save→load exactly like `presetLevel`.
//
// OFFLINE-ONLY: ±0.1 LU assumes the sim's exact deterministic model; the online strict
// harness (level.online.spec.ts) keeps HW-noise-sized tolerances for the same reason, and
// asserts the BOOST regime/ordering there (T5) without repeating these fixture-derived
// magnitudes.

const PRESET24 = SCENARIO[5]; // E2E Preset24
// P4-B: -23 is Rhythm (profiles.rs::default_targets) — the shipped default this fixture's
// base-engaged Rat now falls ~5.2 dB short of pre-fix, and the BOOST regime closes post-fix.
const BASE_TARGET = -23;

const SWITCH_JOBS = [
  {
    switch: 5,
    levGroupId: "G1",
    levNodeId: "ACD_Plumes",
    levParameterId: "level",
    targetLufs: -23,
  },
  {
    switch: 6,
    levGroupId: "G1",
    levNodeId: "ACD_BluesDriver",
    levParameterId: "level",
    targetLufs: -23,
  },
  {
    switch: 7,
    levGroupId: "G1",
    levNodeId: "ACD_ObsessiveDrive",
    levParameterId: "volume",
    targetLufs: -21,
  },
  {
    switch: 8,
    levGroupId: "G1",
    levNodeId: "ACD_Rat",
    levParameterId: "volume",
    targetLufs: -21,
  },
];

/** The base row's `base_boost` disclosure (mirrors `headroom_trade::BaseBoostSummary` /
 *  `src/lib/types.ts`'s `BaseBoostSummary`, snake_case) — only the fields this file asserts. */
interface BaseBoostResult {
  applied: boolean;
  base_amps: { previous_value: number; value: number | null }[];
}
interface LevelResult {
  saved: boolean;
  clamped: boolean;
  /** The solved `presetLevel` (0..1) — pins at `LEVEL_MAX` (1.0) in the BOOST regime. */
  final_level: number;
  /** Null unless the base pair entered the BOOST regime — see
   *  `src/lib/types.ts`'s `BaseBoostSummary` doc. */
  base_boost: BaseBoostResult | null;
}
interface FootswitchLevelResult {
  switch: number;
  saved: boolean;
  clamped: boolean;
  unconverged: boolean;
  clamp_reason: string | null;
  predicted_lufs: number;
}

const T = LEVEL_T;
// Matches the leveller's own tightened FS lane acceptance (`FS_TOL_LU = 0.1`) — the sim's
// leveled-param curve is exact, so there is no HW-noise margin to add on top.
const TOL = 0.1;

const measureSound = (
  page: Page,
  job?: (typeof SWITCH_JOBS)[number],
): Promise<number> =>
  invoke(
    page,
    "e2e_measure_sound",
    {
      slot: PRESET24.slot,
      scene: null,
      footswitch: job?.switch ?? null,
      topologyId: "guitar-humbucker",
      lev: job
        ? {
            groupId: job.levGroupId,
            nodeId: job.levNodeId,
            parameterId: job.levParameterId,
          }
        : null,
    },
    T,
  ) as Promise<number>;

test.describe("Level — footswitch stale-load fixture (offline deterministic model)", () => {
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  test("base + 4 pedals solve to target and re-measure at target from the saved state", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline-only: ±0.1 LU assumes the sim's exact deterministic model",
    );
    // The FS batch's 10-11 secant iterations per switch × 4 switches are CPU-bound against
    // SimDevice (a debug-build `e2e_server` is the slow case).
    test.setTimeout(240_000);
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    // Base: presetLevel BOOST (P4-B) — presetLevel pins at its ceiling and the Twin's fader
    // is solved and saved alongside it, one save carrying both halves.
    const base = (await invoke(
      page,
      "level_preset",
      { job: baseLevelJob(PRESET24.slot, BASE_TARGET) },
      T,
    )) as LevelResult;
    expect(base.clamped, "base must reach target, not clamp").toBe(false);
    expect(base.saved, "base must level and save").toBe(true);
    expect(
      base.final_level,
      "presetLevel pins at LEVEL_MAX in the BOOST regime",
    ).toBeCloseTo(1.0, 5);
    const boost = base.base_boost;
    if (!boost) {
      throw new Error(
        "the Plumes shape (405) must enter the BOOST regime: G≈+16.4 exceeds P_up≈+11.1",
      );
    }
    expect(
      boost.applied,
      "the fader raise must be solved AND persisted (save:true)",
    ).toBe(true);
    const amp = boost.base_amps[0];
    expect(amp, "exactly one base amp candidate (the Twin)").toBeDefined();
    expect(
      amp.previous_value,
      "the Twin's fader started at its authored 0.28",
    ).toBeCloseTo(0.28, 2);
    expect(
      amp.value,
      "the boost solved (not merely planned) the fader",
    ).not.toBeNull();
    if (amp.value !== null) {
      expect(
        amp.value,
        "the Twin's fader solved to ≈0.498 to close the rest of the gap",
      ).toBeCloseTo(0.498, 2);
    }

    // Footswitch batch: all 4 pedals in one call, save — its own load comes right after the
    // base save (the incident shape).
    const fs = (await invoke(
      page,
      "level_footswitches_apply",
      {
        slot: PRESET24.slot,
        jobs: SWITCH_JOBS,
        save: true,
        topologyId: "guitar-humbucker",
        calibrationLufs: null,
        profileId: null,
        onResult: "__CHANNEL__:1",
      },
      T * 2,
    )) as FootswitchLevelResult[];

    for (const [i, r] of fs.entries()) {
      const job = SWITCH_JOBS[i];
      expect(
        r.clamp_reason,
        `switch ${String(r.switch)} must have signal`,
      ).toBeNull();
      expect(
        r.clamped,
        `switch ${String(r.switch)} must reach target, not clamp`,
      ).toBe(false);
      expect(
        r.unconverged,
        `switch ${String(r.switch)} must converge, not stop short — the incident's signature`,
      ).toBe(false);
      expect(r.saved, `switch ${String(r.switch)} must level and save`).toBe(
        true,
      );
      expect(
        Math.abs(r.predicted_lufs - job.targetLufs),
        `switch ${String(r.switch)} solved-vs-target`,
      ).toBeLessThanOrEqual(TOL);
    }

    // Re-measure the PERSISTED sim state from a FRESH load (the strict harness's own seam,
    // `e2e_measure_sound` — see level.online.spec.ts) — proves the SAVED preset actually
    // sounds at target, not merely that the run reported it.
    // The base re-measure catches the boosted PAIR (either half wrong moves the sound off
    // target); the block read below confirms the FADER half by its own persisted value.
    const heardBase = await measureSound(page);
    expect(
      Math.abs(heardBase - BASE_TARGET),
      "base re-measures at target from the saved state",
    ).toBeLessThanOrEqual(TOL);
    const blocks = (await invoke(
      page,
      "list_level_blocks",
      { slot: PRESET24.slot },
      T,
    )) as LevelBlock[];
    const twinFader = blocks.find(
      (b) =>
        b.node_id === "ACD_TwinReverb65NoFx" &&
        b.parameter_id === "outputLevel",
    );
    expect(
      twinFader,
      "the Twin's outputLevel candidate must be discoverable",
    ).toBeDefined();
    expect(
      twinFader?.value ?? Number.NaN,
      "the Twin's fader half survived the footswitch batch's save at its boosted ≈0.498",
    ).toBeCloseTo(0.498, 2);

    for (const job of SWITCH_JOBS) {
      const heard = await measureSound(page, job);
      expect(
        Math.abs(heard - job.targetLufs),
        `switch ${String(job.switch)} re-measures at target from the saved state`,
      ).toBeLessThanOrEqual(TOL);
    }

    await expectReampBalanced(page, reampBase);
  });
});
