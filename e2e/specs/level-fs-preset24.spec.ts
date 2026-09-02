import type { Page } from "@playwright/test";
import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  armCommitLatency,
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

// COVERAGE rows 36, 16 — the lazy-save (stale-load) incident, the FS level opt-in BAKE
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
// BUG→GATE (2026-08-02 HW incident, lazy-save half): `saveCurrentPreset` commits LAZILY
// (T+45-100s on the real unit) — a same-slot `loadPreset` inside that window materializes the
// PRE-save preset. The incident: base saved presetLevel 0.4377, the footswitch batch's own
// load 2s later materialized the pre-run ~0.798, so all 4 pedal sweeps ran +5.2 LU hot and the
// solved values persisted ~5 dB low. `leveller::ensure_fresh_load` + the per-slot save
// registry (danger.md) are the fix.
//
// This file is the offline, deterministic, sim-layer proof that the WHOLE stack — the base
// BOOST (presetLevel + fader, one save), the footswitch batch, the freshness barrier, and the
// FS_TOL_LU=0.1 tightened acceptance — lands base AND all 4 pedals within tolerance of their
// targets on the FIRST run, no clamp, both as REPORTED by the solve and as RE-MEASURED from
// the saved (persisted) state afterward. The second test below additionally proves the
// TWO-HALVES save barrier (danger.md's Phase 2 guard (b)): a same-slot load induced between
// the base save and the footswitch batch's own load must not let ONE half of the boosted pair
// (presetLevel OR the amp fader) revert while the other survives — a half-reverted pair reads
// materially off target, which the base re-measure below would catch even though neither
// half is independently zero.
//
// PRESET24 (E2E Preset24, slot 405): 4 drive pedals (Plumes/BluesDriver/ObsessiveDrive/Rat,
// ftsw indices 5-8, matching the real "TR+BD2+BMP"-class preset) feeding a saturated amp
// (Twin) into a cab, no scenes. Each pedal's own knob follows notes/leveling.md's
// silent→cliff→plateau curve (`sim_device::saturated_pedal_lufs`) — EXACT and deterministic
// offline, unlike the stimulus-scaling slack the flat-C sidecar model carries elsewhere, so a
// tight ±0.1 LU assertion is meaningful here (see e2e/fixtures/scenario-loudness.json's "405"
// entry for the C/PT math, including the two measurement regimes P4-B introduced). All 4
// pedals stay on the BAKE path (off-in-base + sole owner + no scenes), so the leveler writes
// straight onto the block and the sim's lazy-commit `SavedDoc` overlay (sim_device.rs)
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
    // The FS batch alone costs ~100 s: the cliff's steep slope needs 10-11 secant
    // iterations per switch × 4 switches, each paying the leveller's own REAL
    // (non-shortened) settle/reconnect sleeps even against SimDevice — measured via a
    // direct curl rehearsal against the offline server, not a guess.
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

    // Footswitch batch: all 4 pedals in one call, save. `ensure_fresh_load` gates this
    // batch's own load against the base save's commit window (default 0 ms latency here —
    // the second test below arms a real gap).
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
    const heardBase = await measureSound(page);
    expect(
      Math.abs(heardBase - BASE_TARGET),
      "base re-measures at target from the saved state",
    ).toBeLessThanOrEqual(TOL);

    for (const job of SWITCH_JOBS) {
      const heard = await measureSound(page, job);
      expect(
        Math.abs(heard - job.targetLufs),
        `switch ${String(job.switch)} re-measures at target from the saved state`,
      ).toBeLessThanOrEqual(TOL);
    }

    await expectReampBalanced(page, reampBase);
  });

  // The incident replay: span the base-save→FS-batch-load gap with a real commit latency.
  // Pre-fix (no freshness barrier), the FS batch's own load would materialize the PRE-save
  // presetLevel and every pedal sweep would run off by the resulting preset_term error —
  // several dB, not the ±0.1 this run demands. With `leveller::ensure_fresh_load` +
  // `register_slot_save` (danger.md), the batch WAITS for the base save to commit before
  // reading/leveling, so the run still lands within TOL despite the induced lazy-commit gap.
  // P4-B ADDITION: since the base row now boosts a PAIR (presetLevel + the Twin's fader) in
  // one save, this test also proves the TWO-HALVES save barrier (danger.md's Phase 2 guard
  // (b)) — after the induced gap, BOTH the presetLevel half (via a base re-measure) AND the
  // fader half (via a direct block read) must have survived, not just one of them.
  test("survives a lazy-commit gap between the base save and the footswitch batch", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline-only: TMP_SIM_COMMIT_LATENCY_MS / /sim/commit-latency is a SimDevice-only knob",
    );
    // The ~100 s FS-batch solve cost (see the sibling test's comment) PLUS whatever
    // `ensure_fresh_load` retry cadence (10 s/iteration) the barrier pays waiting out the
    // 15 s induced gap — budget well past both (measured ~107 s end to end via a direct
    // curl rehearsal with the same 15 s latency armed).
    test.setTimeout(300_000);
    await ensureScenario(page);
    const reampBase = await reampCounters(page);
    // Comfortably larger than the gap between this run's base save and its footswitch
    // batch's own load — pins the incident mechanism at the full-stack level (sim_device.rs's
    // own red-pin test pins it at the sim layer alone; this pins the barrier that consumes it).
    await armCommitLatency(page, 15_000);

    const base = (await invoke(
      page,
      "level_preset",
      { job: baseLevelJob(PRESET24.slot, BASE_TARGET) },
      T,
    )) as LevelResult;
    expect(base.clamped, "base must reach target, not clamp").toBe(false);
    expect(base.saved, "base must level and save").toBe(true);

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
        r.unconverged,
        `switch ${String(r.switch)} must converge despite the induced commit gap`,
      ).toBe(false);
      expect(r.clamped, `switch ${String(r.switch)} must not clamp`).toBe(
        false,
      );
      expect(r.saved, `switch ${String(r.switch)} must level and save`).toBe(
        true,
      );
      expect(
        Math.abs(r.predicted_lufs - job.targetLufs),
        `switch ${String(r.switch)} solved-vs-target despite the lazy-commit gap — RED \
pre-fix (no barrier): the batch's load would materialize the pre-save presetLevel and every \
pedal would land several dB off target, exactly the reported incident`,
      ).toBeLessThanOrEqual(TOL);
    }

    // The TWO-HALVES save barrier (danger.md's Phase 2 guard (b)): the base run above
    // boosted BOTH presetLevel and the Twin's own fader in one save. A half-reverted pair
    // (e.g. the fader write predating the pre-save base recall and getting silently
    // reverted while presetLevel survives, or vice versa) would NOT read as a clean 0 —
    // it reads several dB off target, exactly like the lazy-commit incident itself. Prove
    // BOTH halves independently: the base re-measure catches the PAIR (either half wrong
    // moves the sound off target), and a direct block read confirms the FADER half by its
    // own persisted value.
    const heardBaseAfterGap = await measureSound(page);
    expect(
      Math.abs(heardBaseAfterGap - BASE_TARGET),
      "base re-measures at target after the induced gap — a half-reverted presetLevel/fader \
pair would read materially off target here even though neither half alone is silence",
    ).toBeLessThanOrEqual(TOL);

    const blocksAfterGap = (await invoke(
      page,
      "list_level_blocks",
      { slot: PRESET24.slot },
      T,
    )) as LevelBlock[];
    const twinFader = blocksAfterGap.find(
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
      "the Twin's fader half survived the gap at its boosted ≈0.498 value",
    ).toBeCloseTo(0.498, 2);

    await expectReampBalanced(page, reampBase);
  });
});
