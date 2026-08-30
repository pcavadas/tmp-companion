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
  openLevel,
  reampCounters,
  reampOff,
  runBaseLevel,
} from "../fixtures/scenario";

// ONLINE Level consolidation (8-file → 4-file suite shrink). One file, one Hiwatt (404)
// arc plus three smaller tests, so the online lane pays ONE Playwright boot/rest for Level
// instead of four (level.spec.ts's online half + the now-deleted level-strict.spec.ts +
// the now-deleted level-rerun.spec.ts's online half — level.spec.ts's own offline-only
// tests, level-rerun.spec.ts's merged into it, are unaffected — see that file's header).
//
// Test 1 is level-strict.spec.ts moved essentially verbatim (COVERAGE row 37's online half
// — base + every scene + every footswitch, re-measured from the SAVED state with the
// ffmpeg-validated `measure()` calls, including the 141103a skip-legitimacy guard for an
// already-in-tolerance bake row). Test 2, run next in this `.serial` block ON THE SAME
// SAVED STATE test 1 just wrote (no reseed, no reconnect in between), is the idempotency
// addendum absorbed from level-rerun.spec.ts's retired online tests: (i) a second
// `level_preset` run on the base lane must solve unclamped and skip the write
// (`level_unchanged`); (ii) a second `level_footswitches_apply` batch, same targets, must
// skip every write lane 3 actually made. Reusing the Hiwatt state test 1 already saved
// (rather than leveling a second preset from scratch, as level-rerun.spec.ts's online
// tests used to) is the whole point: one save, two independent proofs against it (does it
// measure right, does re-running it write nothing) instead of two separate online leveling
// runs. It is its own test, not inline in test 1, because its acceptance band is narrower
// than test 1's own noise tolerance — see that test's own header comment.
//
// Tests 3 and 4 are level.spec.ts's online-only content, moved here: the backup-scan
// enumeration of Hiwatt's 9 child rows (this file's own preset, so no extra fixture is
// touched), and the two-preset per-target UI flow (E2E Pedalboard/Edge). level.spec.ts's
// own whole-preset UI run (base + scenes + footswitches on E2E Rig) stays OFFLINE ONLY
// there (trade T2 — see that file's own header comment).

const HIWATT = SCENARIO[4]; // E2E Hiwatt 3S — the sanitized user-reported preset
// PR2 re-baseline: +3 (2-ch BS.1770 over the processed pair) from the mono-era
// HW-proven -20 — same physical operating point, pending hardware re-validation
// (deferred; device offline as of this PR — see notes/leveling.md).
const TARGET = -17; // HW-proven reachable for base + all 4 scenes + all 4 switches
const DELTA = 0.5; // base/scenes: ~0.12 LU run-to-run noise + the one-shot/secant residual
// Footswitch sounds get the product's own contract, not a tighter one: the FS lane
// accepts a solve within KNOB_TOL_LU (0.3 LU, leveller.rs), and even the bracket-aware
// secant can legitimately stop `unconverged` on a noisy/modulated response (a UniVibe's
// LFO alone wobbles repeat measures ~0.1 LU) — that acceptance band + the re-measure's
// own run-to-run noise compound, so the gate allows the solver's honest band plus
// margin rather than re-deriving a tighter one it can't guarantee.
const DELTA_FS = 1.0;

// The 4 block-acting switches with the wizard's tone-safe default param each
// (`defaultParamIndex`: first LOUDNESS_PARAMS hit) — fixture facts of the Hiwatt.
const SWITCH_JOBS = [
  {
    switch: 2,
    levGroupId: "G1",
    levNodeId: "ACD_MythicDrive",
    levParameterId: "output",
  },
  {
    switch: 3,
    levGroupId: "G1",
    levNodeId: "ACD_Lightspeed",
    levParameterId: "loudness",
  },
  {
    switch: 11,
    levGroupId: "G1",
    levNodeId: "ACD_TremoloBias",
    levParameterId: "level",
  },
  {
    switch: 12,
    levGroupId: "G4",
    levNodeId: "ACD_UniVibe",
    levParameterId: "volume",
  },
];

interface LevelResult {
  saved: boolean;
  clamped: boolean;
  /** 0-based `scenes[]` wire index on a scene row; null elsewhere. Identity, not
   * position: `level_scenes_apply_batched` filters failed scenes out of the array it
   * returns, so index i is NOT scene i once anything fails. */
  scene_slot: number | null;
  persist_mismatch: boolean | null;
}
interface FootswitchLevelResult {
  switch: number;
  saved: boolean;
  clamped: boolean;
  clamp_reason: string | null;
  predicted_lufs: number;
  /** "baked" | "assigned" — which arm the switch resolved to (footswitch.rs's assign
   * gate). Only read where the idempotency addendum pins a row's arm. */
  method: string;
}
/** P5 external validation: what the run PROMISED for the sound about to be re-measured.
 * The spec owns this because the spec is what drove the leveling lane — the server keeps
 * no cross-command memory. Inert unless `TMP_E2E_VALIDATE_LOG` is set in the SERVER's
 * environment (`scripts/e2e.sh` does that when ffmpeg is present), in which case the
 * re-measure also dumps its WAV and appends one row for `scripts/level-validate.sh`. */
interface ValidateArg {
  targetLufs: number;
  clamped: boolean;
  persistMismatch: boolean | null;
}
interface LevelBlock {
  group_id: string;
  node_id: string;
  model_id: string;
  parameter_id: string;
  value: number;
}

const T = LEVEL_T;

// .serial: the idempotency test below genuinely depends on device state the strict-arc
// test writes (Hiwatt's lane-1/lane-3 saves) — `fullyParallel:false` only gives ORDERING,
// not failure-gating, so without `.serial` a lane-3 failure would still let the idempotency
// test run against a half-leveled preset and report a second, confusing failure on top of
// the real one. `.serial` also skips the two UI tests below on an earlier failure, which is
// the right call on an attended online run where the device state is already suspect.
test.describe
  .serial("Level online — strict output + idempotency (Hiwatt 3S, 404)", () => {
  // Carried from the strict-arc test into the idempotency test below (same describe,
  // same worker — `fullyParallel:false` + `.serial` guarantee the write-then-read order).
  // The idempotency test guards its own read with an `expect`, not a `test.skip`, so a
  // predecessor that never set this fails loudly instead of silently no-op-passing.
  let laneFs: FootswitchLevelResult[] | undefined;

  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  test("base + every scene + every footswitch re-measure at target after save; both lanes then prove idempotent on the same saved state", async ({
    page,
  }) => {
    test.skip(!(await isOnline(page)), "online-only: needs real audio");
    // level-strict.spec.ts's own budget: 3 leveling lanes + 9 re-measures of ~6 s captures,
    // checked against `ensure_fresh_load`'s worst case (danger.md): COMMIT_WINDOW_SECS =
    // 150 s, at most 2 same-slot loads that could race a prior save = 300 s of pure barrier
    // stall worst case. 1_800_000 ms covers that with headroom. (The idempotency addendum's
    // own budget — a second `level_preset` solve + a second `level_footswitches_apply`
    // batch — now lives in test 2's own `test.setTimeout`, not here.)
    test.setTimeout(1_800_000);
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    // For a footswitch sound the leveled-param triple rides along so the bridge
    // command replays an ASSIGN switch's saved `valueA` on the SAME param the
    // leveling lane wrote — the spec is the single owner of those coordinates
    // (no second picker server-side to diverge from the wizard's choice).
    const measure = (args: {
      scene?: number;
      footswitch?: (typeof SWITCH_JOBS)[number];
      validate?: ValidateArg;
    }): Promise<number> =>
      invoke(
        page,
        "e2e_measure_sound",
        {
          slot: HIWATT.slot,
          scene: args.scene ?? null,
          footswitch: args.footswitch?.switch ?? null,
          topologyId: "guitar-humbucker",
          lev: args.footswitch
            ? {
                groupId: args.footswitch.levGroupId,
                nodeId: args.footswitch.levNodeId,
                parameterId: args.footswitch.levParameterId,
              }
            : null,
          validate: args.validate ?? null,
        },
        T,
      ) as Promise<number>;

    // ── Lane 1: base (presetLevel one-shot, save) ─────────────────────────────
    const base = (await invoke(
      page,
      "level_preset",
      { job: baseLevelJob(HIWATT.slot, TARGET) },
      T,
    )) as LevelResult;
    expect(base.clamped, "base must reach target, not clamp").toBe(false);
    expect(base.saved, "base must level and save").toBe(true);

    // ── Lane 2: all 4 scenes (amp outputLevel, one batch, save) ───────────────
    const blocks = (await invoke(
      page,
      "list_level_blocks",
      { slot: HIWATT.slot },
      T,
    )) as LevelBlock[];
    const candidates = blocks
      .filter((b) => b.parameter_id === "outputLevel")
      .map((b) => ({
        groupId: b.group_id,
        nodeId: b.node_id,
        parameterId: b.parameter_id,
        value: b.value,
      }));
    expect(
      candidates.length,
      "the Hiwatt amp candidate must be discoverable",
    ).toBeGreaterThan(0);
    const scenes = (await invoke(
      page,
      "level_scenes_apply_batched",
      {
        slot: HIWATT.slot,
        jobs: [0, 1, 2, 3].map((sceneSlot) => ({
          sceneSlot,
          targetLufs: TARGET,
        })),
        candidates,
        save: true,
        rebalance: false,
        topologyId: "guitar-humbucker",
        calibrationLufs: null,
        profileId: null,
        onResult: "__CHANNEL__:1",
      },
      T * 3,
    )) as LevelResult[];
    // Row identity comes off `scene_slot`, not the array index — a mid-batch failure
    // shortens this array, so index i is not scene i.
    expect(
      scenes.map((r) => r.scene_slot).sort((a, b) => Number(a) - Number(b)),
      "every requested scene must come back (no silent mid-batch drop)",
    ).toEqual([0, 1, 2, 3]);
    for (const r of scenes) {
      const id = String(r.scene_slot);
      expect(r.clamped, `scene ${id} must reach target, not clamp`).toBe(false);
      expect(r.saved, `scene ${id} must level and save`).toBe(true);
    }

    // ── Lane 3: all 4 footswitches (one batch, save) ──────────────────────────
    const fs = (await invoke(
      page,
      "level_footswitches_apply",
      {
        slot: HIWATT.slot,
        jobs: SWITCH_JOBS.map((j) => ({ ...j, targetLufs: TARGET })),
        save: true,
        topologyId: "guitar-humbucker",
        calibrationLufs: null,
        profileId: null,
        onResult: "__CHANNEL__:1",
      },
      T * 3,
    )) as FootswitchLevelResult[];
    for (const r of fs) {
      expect(
        r.clamp_reason,
        `switch ${String(r.switch)} must have signal (no off-branch clamp)`,
      ).toBeNull();
      expect(r.clamped, `switch ${String(r.switch)} must reach target`).toBe(
        false,
      );
      // `saved: false` is legitimate ONLY as the bake lane's in-tolerance skip
      // (cb7cb60): lanes 1/2 just leveled the whole preset to TARGET, so a
      // low-authority switch's engaged sound can already sit at TARGET — the
      // idempotency probe then writes nothing, and on THIS preset that shape is
      // deterministic (switch 11 skips every run). Accept it only with the
      // probe's own proof: `predicted_lufs` IS the measurement that passed the
      // FS_TOL_LU (0.1 LU) gate, so it must sit within it. A save that merely
      // failed reports off-target or clamped and still dies here. The strict
      // ffmpeg re-measure below remains the real arbiter for every switch,
      // skipped or saved.
      if (!r.saved) {
        expect(
          Math.abs(r.predicted_lufs - TARGET),
          `switch ${String(r.switch)} skipped its save, which is only legitimate when its stored value measured within tolerance of target`,
        ).toBeLessThanOrEqual(0.11);
      }
      // Fixture-drift alarm: every one of this file's SWITCH_JOBS carries a bare
      // `func: "on-off"` row in the Hiwatt's own `ftsw` (fixtures/scenario-presets.json
      // 404) with no existing `func: "param"` entry on its leveled (node, param) — so
      // `footswitch.rs`'s assign gate (`existing_param_fn_index`) resolves every one of
      // them to the Bake arm, deterministically, on every run. Pin that here
      // unconditionally so a future fixture edit that adds a `param` function to any of
      // these rows (which would flip it to Assign) fails loudly instead of silently
      // testing the wrong arm.
      expect(
        r.method,
        `switch ${String(r.switch)} must resolve to the Bake arm — no SWITCH_JOBS row carries an existing param function`,
      ).toBe("baked");
    }

    // ── The strict gate: re-measure EVERY sound from the SAVED state ──────────
    // Each re-measure also carries the run's own promise for that sound (`validate`),
    // which the server writes to the P5 expectation log alongside the WAV it just
    // captured — so `scripts/level-validate.sh` can judge the SAME audio with ffmpeg's
    // ebur128 afterwards. A scene's promise is looked up BY `scene_slot`, never by index:
    // the batch filters failed scenes out of the array it returns.
    const sceneRow = (slot: number): LevelResult | undefined =>
      scenes.find((r) => r.scene_slot === slot);
    const heard: Record<string, number> = {};
    heard.base = await measure({
      validate: {
        targetLufs: TARGET,
        clamped: base.clamped,
        persistMismatch: base.persist_mismatch,
      },
    });
    for (const scene of [0, 1, 2, 3]) {
      const row = sceneRow(scene);
      expect(
        row,
        `scene ${String(scene)} must be present in the batch results`,
      ).toBeDefined();
      heard[`scene${String(scene)}`] = await measure({
        scene,
        validate: {
          targetLufs: TARGET,
          clamped: row?.clamped ?? false,
          persistMismatch: row?.persist_mismatch ?? null,
        },
      });
    }
    for (const j of SWITCH_JOBS) {
      const row = fs.find((r) => r.switch === j.switch);
      heard[`fs${String(j.switch)}`] = await measure({
        footswitch: j,
        validate: {
          targetLufs: TARGET,
          clamped: row?.clamped ?? false,
          persistMismatch: null,
        },
      });
    }
    for (const [sound, lufs] of Object.entries(heard)) {
      const delta = sound.startsWith("fs") ? DELTA_FS : DELTA;
      expect(
        Math.abs(lufs - TARGET),
        `${sound} re-measures at ${lufs.toFixed(2)} LUFS from the saved state — ` +
          `must be within ${String(delta)} LU of the ${String(TARGET)} LUFS target`,
      ).toBeLessThanOrEqual(delta);
    }

    // Carry lane 3's results to the idempotency test below (same describe, runs next —
    // see the describe-level comment on `laneFs` and the `.serial` note above it).
    laneFs = fs;

    await expectReampBalanced(page, reampBase);
  });

  // ── Idempotency addendum, ON THE SAME SAVED STATE the test above just wrote ──────────
  // Split into its own test (was inline in the strict-arc test above) because its
  // acceptance band is NARROWER than that test's own DELTA (0.5): a real skip requires the
  // solver to land within `KNOB_TOL_LU` (0.3 LU, leveller.rs) of the prior save, tighter
  // than the strict re-measure's accepted noise band — so a legitimate idempotency flake
  // must not report as a failure of the (unrelated) strict output-accuracy property, and
  // soak (`scripts/e2e.sh soak N`, which loops this file) needs per-property attribution
  // rather than one monolithic pass/fail.
  test("idempotency: base and footswitch skip-branch re-runs make zero new writes", async ({
    page,
  }) => {
    test.skip(!(await isOnline(page)), "online-only: needs real audio");
    // 1 base solve (fast — no capture, just solve+skip) + 1 footswitch batch (up to 4
    // measurable switches, each a capture) + its own possible `ensure_fresh_load` stall
    // (COMMIT_WINDOW_SECS = 150 s, danger.md) = 300_000 (base + fs headroom) + 300_000
    // (commit-window stall) = 600_000 ms.
    test.setTimeout(600_000);
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    // Non-vacuous dependency check: the strict-arc test above must have run and set this
    // (an `expect`, not a `test.skip`, so a predecessor that silently never got here fails
    // loudly instead of this test no-op-passing — the exact G-class trap this file's own
    // `scripts/e2e.sh` guard exists to catch at the spec-selection level). Narrows via a
    // throw, not a non-null assertion, so the check is real at runtime too.
    if (!laneFs) {
      throw new Error(
        "the strict-arc test above must run first and set laneFs (same describe.serial block)",
      );
    }
    const fs = laneFs;

    // ── Base skip-branch (ports level-rerun.spec.ts's retired online "base: run 2 makes
    // zero new writes" test — reusing Hiwatt's lane-1 save instead of leveling a second
    // preset from scratch) ──
    const baseRerun = (await invoke(
      page,
      "level_preset",
      { job: baseLevelJob(HIWATT.slot, TARGET) },
      T,
    )) as LevelResult;
    expect(
      baseRerun.clamped,
      "base re-run must reach target unclamped (a real skip, not a clamp)",
    ).toBe(false);
    expect(
      baseRerun.saved,
      "base re-run solved the same value lane 1 already saved → must skip the write (level_unchanged)",
    ).toBe(false);

    // ── Footswitch skip-branch (ports level-rerun.spec.ts's retired online "footswitch:
    // run 2 rewrites nothing in-tolerance" test — reusing Hiwatt's lane-3 save instead of a
    // fresh switch on a different preset) ──
    const fs2 = (await invoke(
      page,
      "level_footswitches_apply",
      {
        slot: HIWATT.slot,
        jobs: SWITCH_JOBS.map((j) => ({ ...j, targetLufs: TARGET })),
        save: true,
        topologyId: "guitar-humbucker",
        calibrationLufs: null,
        profileId: null,
        onResult: "__CHANNEL__:1",
      },
      T * 3,
    )) as FootswitchLevelResult[];

    // The non-vacuity witness for the skip proof below: at least one switch lane 3
    // actually WROTE (mirrors the retired test's own `leveled.size > 0` guard) — without
    // it, "every write skips on re-run" would be vacuously true of an empty set.
    const wroteInLane3 = fs.filter((r) => r.saved).map((r) => r.switch);
    expect(
      wroteInLane3.length,
      "lane 3 (the strict arc's fs pass) must have written at least one switch, or the skip proof below is vacuous",
    ).toBeGreaterThan(0);

    for (const j of SWITCH_JOBS) {
      const r2 = fs2.find((r) => r.switch === j.switch);
      expect(
        r2,
        `switch ${String(j.switch)} must return a result in the re-run`,
      ).toBeTruthy();
      expect(
        r2?.clamp_reason,
        `switch ${String(j.switch)} re-run must be measurable (no clamp reason)`,
      ).toBeNull();
      expect(
        r2?.clamped,
        `switch ${String(j.switch)} re-run must reach target unclamped`,
      ).toBe(false);
      // Same fixture-drift alarm as the strict-arc test's lane 3 — unconditional, since
      // every SWITCH_JOBS row is deterministically Bake in this fixture.
      expect(
        r2?.method,
        `switch ${String(j.switch)} re-run must still resolve to the Bake arm`,
      ).toBe("baked");
      expect(
        Math.abs((r2?.predicted_lufs ?? NaN) - TARGET),
        `switch ${String(j.switch)} re-run must land on target`,
      ).toBeLessThanOrEqual(0.11);
      // The idempotency-skip property itself: any switch lane 3 actually WROTE must make
      // ZERO further writes on this immediate re-run at the same target. Switches lane 3
      // did NOT write (already in-tolerance there, e.g. switch 11's documented
      // low-authority skip) get no save-strictness assertion here — lane 3's own
      // in-tolerance guard (the strict-arc test, above) already proves those.
      if (wroteInLane3.includes(j.switch)) {
        expect(
          r2?.saved,
          `switch ${String(j.switch)} lane 3 wrote a real value at TARGET → this re-run at the same target must skip the write (level_unchanged / switch_at_target)`,
        ).toBe(false);
      }
    }

    await expectReampBalanced(page, reampBase);
  });

  // COVERAGE row 37 — scene wipe / bake / conformance oracle (moved from level.spec.ts;
  // this file already carries 404's slot, so no extra fixture is touched by adding it here).
  test("enumerates the 3-scene + Base-Scene + 4-footswitch preset in the list", async ({
    page,
  }) => {
    test.skip(!(await isOnline(page)), "online-only tier of this file's arc");
    await ensureScenario(page);
    await openLevel(page);

    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    await filter.fill(HIWATT.name);
    // The collapsed breakdown — the scan's own count, before any expansion.
    await expect(page.getByText("4 scenes · 4 footswitches")).toBeVisible({
      timeout: 60_000,
    });

    await page.getByTitle(/Show Base/).click();
    // Base + 4 footswitch scenes + 4 block-acting footswitches = 9 child rows.
    await expect(page.getByText("main preset sound")).toHaveCount(1);
    await expect(page.getByText("footswitch scene")).toHaveCount(4);
    await expect(page.getByText("footswitch", { exact: true })).toHaveCount(4);
    // The 4th scene is a real overlay named "Base Scene" — it must appear as its OWN row,
    // distinct from the "Base Preset" row (the sentinel).
    await expect(page.getByText("Base Scene", { exact: true })).toHaveCount(1);
    await expect(page.getByText("Base Preset", { exact: true })).toHaveCount(1);
  });

  // Moved from level.spec.ts — E2E Pedalboard + E2E Edge, unrelated to the Hiwatt arc
  // above; kept in this file so the online lane pays one Level Playwright boot, not two.
  test("levels two presets' Base to different targets, end to end", async ({
    page,
  }) => {
    test.skip(!(await isOnline(page)), "online-only tier of this file's arc");
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    // Each of the two presets carries footswitches/scenes of its own, so a whole-preset
    // tick would sweep those in too and shift the terminal summary's Done-vs-Accept text —
    // `runBaseLevel` selects each preset's Base row alone.
    await runBaseLevel(page, [
      { preset: SCENARIO[1], label: "Crunch" },
      { preset: SCENARIO[2], label: "Lead" },
    ]);

    // Standing safety gate: the app disengaged re-amp at least as often as it engaged,
    // checked BEFORE the afterEach reampOff rescue (so a stranded engage fails here).
    await expectReampBalanced(page, reampBase);
  });
});
