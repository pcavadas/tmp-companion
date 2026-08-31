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
  reampCounters,
  reampOff,
  runBaseLevel,
} from "../fixtures/scenario";

// ONLINE Level rework (P4, the Plumes/BD2/OCD leveling-regression fix). Budget rationale:
// the online LEVELING suite is capped at ~25 min; exhaustive coverage lives offline in the
// sim (which now models both real-preset shapes), and this file keeps only the minimal
// device-truth journeys — one consolidated arc per shape, back-to-back in one session so a
// 27↔28 see-saw regression surfaces in a single run. Retired T1 (the Hiwatt-3S (404) full arc,
// ~14 min: base + 4 scenes + 4 footswitches + 9 strict ffmpeg re-measures) and T3 (the
// backup-scan enumeration of Hiwatt's 9 child rows) and T4 (the two-preset Base UI flow on
// Pedalboard/Edge). In their place:
//
// T1′ (below, describe #1's first test) — a CONSOLIDATED Friedman-shape (410) arc: base + all
// 3 scenes + 2 footswitches, re-measured strict for base+scenes only (4 ffmpeg rows, not 9).
// 410's own base pair is a plain TRADE solve (`headroom_trade::plan_level_pair`'s
// `G≈+1.0 <= P_up≈+6.0`), so it must NEVER enter BOOST — this is the "see-saw" CONTROL this
// arc exists to run, back-to-back with T5's Plumes-shape (405) BOOST case below, so a
// regression that widened BOOST's trigger condition and started moving 410's fader too would
// be caught in the SAME online session. One of the 2 footswitches is 410's own TubeScreamer
// row; the other is HIWATT (404) switch 12 (`ACD_UniVibe.volume`), CARRIED VERBATIM from the
// retired arc's own `SWITCH_JOBS` so its modulated-response solve (an LFO'd knob, the one
// class of assertion the retired arc uniquely exercised) keeps a named carrier per the plan's
// own budget table — touching 404 costs nothing extra since it stays resident regardless.
//
// T2 (below, describe #1's second test) — the idempotency addendum, KEPT and RETARGETED from
// Hiwatt (404) to 410, on the SAME saved state T1′ just wrote (no reseed, `.serial` ordering).
// It also now carries the retired arc's own SKIP-LEGITIMACY guard (its lane-3 loop's
// `if (!r.saved) …` check on the FIRST run's own footswitch result — a `saved:false` first-run
// result is legitimate ONLY when the stored value already measured within FS tolerance, never
// a silent no-op): folded in here rather than left in T1′ because idempotency (a re-run
// making zero writes) and this skip-legitimacy proof are the same "was a no-write outcome
// actually correct" property, on 410's much smaller footswitch surface (1 row, not 4).
//
// T3 (enumeration) is GONE — moved offline as an `e2e_server_tests.rs` gate (E9); a pure list
// read has negligible device truth to verify. T4 (two-preset Base UI flow) is GONE — its
// "drives the real UI" claim is subsumed by T1′ (this file only levels 410 via raw invoke,
// see below) plus T5 (drives the wizard UI on 405). Session-budget arithmetic: the plan's own
// table lands this file at ~8 min (T1′) + ~2 min (T2) = ~10 min, replacing the retired
// ~14+2+~2+~2 ≈ 20 min; T5 (below, describe #2) adds ~5 min. Total ≈ 15 min for this file
// (down from the old suite's ≈ 20 min for the SAME two describes' worth of coverage), leaving
// margin under the leveling suite's ratified ≤ 25 min online budget.
//
// COVERAGE rows 37, 45 — row 37 is Hiwatt's own scene/footswitch enumeration, row 45 is
// 410's structural-readiness pin; see e2e/fixtures/COVERAGE.md for the "where it went"
// notes on every retired test.

const FRIEDMAN = SCENARIO[10]; // E2E Friedman 3S — the P4 leveling-regression fixture
const HIWATT = SCENARIO[4]; // E2E Hiwatt 3S — carries only the one carried-over modulated row
const PRESET24 = SCENARIO[5]; // E2E Preset24 — the Plumes-shape first-run journey (T5)

// 410's base pair is a plain TRADE solve at -23 (G≈+1.0 <= P_up≈+6.0, plan physics section):
// presetLevel alone closes the gap, so the fader must NEVER move — this is the see-saw
// control target, deliberately the SAME numeric target as T5's Plumes-shape BOOST case below
// so the two runs are directly comparable in one online session.
const BASE_TARGET_410 = -23;
// Below every scene's own ceiling (Rhythm -17 / Lead -16 / "Base Scene" -19,
// scenario-loudness.json's "410" entry) so all three solve unclamped with headroom to spare.
const SCENE_TARGET_410 = -20;
// TubeScreamer's own isolated capture rides the same amp/cab chain as base.
const FS_TARGET_410 = -23;
// Carried verbatim from the retired Hiwatt arc's own SWITCH_JOBS entry for switch 12.
const FS_TARGET_UNIVIBE = -17;

const DELTA = 0.5; // base/scene: run-to-run noise + the one-shot/secant residual (unchanged)
const DELTA_FS = 1.0; // footswitch re-measure: KNOB_TOL_LU (0.3, leveller.rs) + capture noise
// The footswitch SOLVE's own acceptance band (not a re-measure) — matches leveller.rs's
// KNOB_TOL_LU exactly, no extra margin needed since this checks the solver's own report.
const KNOB_TOL_LU = 0.3;

interface LevelResult {
  saved: boolean;
  clamped: boolean;
  final_level: number;
  scene_slot: number | null;
  persist_mismatch: boolean | null;
  /** Null unless the base pair entered the BOOST regime — see `src/lib/types.ts`'s
   *  `BaseBoostSummary` doc. 410's base pair is TRADE, so this must stay null throughout. */
  base_boost: { applied: boolean; regime: string } | null;
}
interface FootswitchLevelResult {
  switch: number;
  saved: boolean;
  clamped: boolean;
  clamp_reason: string | null;
  predicted_lufs: number;
}
/** P5 external validation: what the run PROMISED for the sound about to be re-measured —
 *  see level-fs-preset24.spec.ts / .claude/rules/e2e.md's "External validation" section. */
interface ValidateArg {
  targetLufs: number;
  clamped: boolean;
  persistMismatch: boolean | null;
}
interface LevelBlock {
  group_id: string;
  node_id: string;
  parameter_id: string;
  value: number;
}

const T = LEVEL_T;

// .serial: T2 genuinely depends on device state T1′ writes (410's base + footswitch saves).
test.describe
  .serial("Level online — Friedman-shape consolidated arc + idempotency (410)", () => {
  // Carried from T1′ into T2 below (same describe, same worker — `.serial` guarantees the
  // write-then-read order). T2 guards its own read with a thrown error, not a silent
  // `test.skip`, so a predecessor that never got here fails loudly instead of no-op-passing.
  let laneFs410: FootswitchLevelResult[] | undefined;

  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  test("base + 3 scenes + 2 footswitches re-measure at target after save; base stays TRADE, never BOOST (see-saw guard)", async ({
    page,
  }) => {
    test.skip(!(await isOnline(page)), "online-only: needs real audio");
    // Base (3 conns/1 engage) + 3 scenes (one batch) + 2 footswitch batches + 4 strict
    // ffmpeg re-measures, checked against `ensure_fresh_load`'s worst case (danger.md:
    // COMMIT_WINDOW_SECS = 150 s) — generous headroom over the plan's own ~8 min estimate.
    test.setTimeout(900_000);
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    const measure410 = (args: {
      scene?: number;
      validate?: ValidateArg;
    }): Promise<number> =>
      invoke(
        page,
        "e2e_measure_sound",
        {
          slot: FRIEDMAN.slot,
          scene: args.scene ?? null,
          footswitch: null,
          topologyId: "guitar-humbucker",
          lev: null,
          validate: args.validate ?? null,
        },
        T,
      ) as Promise<number>;

    // ── Lane 1: base (presetLevel one-shot, save) — the see-saw control ───────
    const base = (await invoke(
      page,
      "level_preset",
      { job: baseLevelJob(FRIEDMAN.slot, BASE_TARGET_410) },
      T,
    )) as LevelResult;
    expect(base.clamped, "base must reach target, not clamp").toBe(false);
    expect(base.saved, "base must level and save").toBe(true);
    expect(
      base.base_boost,
      "410's base pair is a plain TRADE solve (G <= P_up) — it must never enter BOOST, or a \
regression has widened BOOST's trigger and would start moving a fader nothing asked it to move",
    ).toBeNull();

    // ── Lane 2: all 3 scenes (amp outputLevel, one batch, save) ───────────────
    const blocks = (await invoke(
      page,
      "list_level_blocks",
      { slot: FRIEDMAN.slot },
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
      "the Friedman amp candidate must be discoverable",
    ).toBeGreaterThan(0);
    const scenes = (await invoke(
      page,
      "level_scenes_apply_batched",
      {
        slot: FRIEDMAN.slot,
        jobs: [0, 1, 2].map((sceneSlot) => ({
          sceneSlot,
          targetLufs: SCENE_TARGET_410,
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
    // Row identity comes off `scene_slot`, not array index (a mid-batch failure shortens
    // this array).
    expect(
      scenes.map((r) => r.scene_slot).sort((a, b) => Number(a) - Number(b)),
      "every requested scene must come back (no silent mid-batch drop)",
    ).toEqual([0, 1, 2]);
    for (const r of scenes) {
      const id = String(r.scene_slot);
      expect(r.clamped, `scene ${id} must reach target, not clamp`).toBe(false);
      expect(r.saved, `scene ${id} must level and save`).toBe(true);
    }

    // ── Lane 3: two footswitches — 410's own TubeScreamer row, plus one MODULATED
    // row (UniVibe, 404) carried verbatim from the retired Hiwatt arc so its own
    // modulated-solve assertion keeps a named carrier (plan's online-budget table).
    // Separate batches: the two rows live on different slots.
    const fs410 = (await invoke(
      page,
      "level_footswitches_apply",
      {
        slot: FRIEDMAN.slot,
        jobs: [
          {
            switch: 1,
            levGroupId: "G1",
            levNodeId: "ACD_TubeScreamer",
            levParameterId: "level",
            targetLufs: FS_TARGET_410,
          },
        ],
        save: true,
        topologyId: "guitar-humbucker",
        calibrationLufs: null,
        profileId: null,
        onResult: "__CHANNEL__:1",
      },
      T * 2,
    )) as FootswitchLevelResult[];
    for (const r of fs410) {
      expect(
        r.clamp_reason,
        `switch ${String(r.switch)} must have signal`,
      ).toBeNull();
      expect(
        r.clamped,
        `switch ${String(r.switch)} must reach target, not clamp`,
      ).toBe(false);
      expect(
        Math.abs(r.predicted_lufs - FS_TARGET_410),
        `switch ${String(r.switch)} solved-vs-target`,
      ).toBeLessThanOrEqual(KNOB_TOL_LU);
    }

    const fsUniVibe = (await invoke(
      page,
      "level_footswitches_apply",
      {
        slot: HIWATT.slot,
        jobs: [
          {
            switch: 12,
            levGroupId: "G4",
            levNodeId: "ACD_UniVibe",
            levParameterId: "volume",
            targetLufs: FS_TARGET_UNIVIBE,
          },
        ],
        save: true,
        topologyId: "guitar-humbucker",
        calibrationLufs: null,
        profileId: null,
        onResult: "__CHANNEL__:1",
      },
      T * 2,
    )) as FootswitchLevelResult[];
    for (const r of fsUniVibe) {
      expect(
        r.clamp_reason,
        `switch ${String(r.switch)} must have signal`,
      ).toBeNull();
      expect(
        r.clamped,
        `switch ${String(r.switch)} (modulated UniVibe response) must reach target, not clamp`,
      ).toBe(false);
      expect(
        Math.abs(r.predicted_lufs - FS_TARGET_UNIVIBE),
        `switch ${String(r.switch)} (modulated UniVibe response) solved-vs-target — a \
LFO'd knob can legitimately need the full KNOB_TOL_LU band`,
      ).toBeLessThanOrEqual(KNOB_TOL_LU);
    }

    // ── The strict gate: re-measure base + all 3 scenes from the SAVED state (4 rows).
    // The 2 footswitch rows above are judged by their OWN solve result, not a second
    // strict re-measure — the plan's online-budget table deliberately caps this
    // consolidated arc's ffmpeg-validated rows at 4 (vs the retired arc's 9) to keep
    // its cost down.
    const sceneRow = (slot: number): LevelResult | undefined =>
      scenes.find((r) => r.scene_slot === slot);
    const heard: Record<string, number> = {};
    heard.base = await measure410({
      validate: {
        targetLufs: BASE_TARGET_410,
        clamped: base.clamped,
        persistMismatch: base.persist_mismatch,
      },
    });
    for (const scene of [0, 1, 2]) {
      const row = sceneRow(scene);
      expect(
        row,
        `scene ${String(scene)} must be present in the batch results`,
      ).toBeDefined();
      heard[`scene${String(scene)}`] = await measure410({
        scene,
        validate: {
          targetLufs: SCENE_TARGET_410,
          clamped: row?.clamped ?? false,
          persistMismatch: row?.persist_mismatch ?? null,
        },
      });
    }
    for (const [sound, lufs] of Object.entries(heard)) {
      const target = sound === "base" ? BASE_TARGET_410 : SCENE_TARGET_410;
      expect(
        Math.abs(lufs - target),
        `${sound} re-measures at ${lufs.toFixed(2)} LUFS from the saved state`,
      ).toBeLessThanOrEqual(DELTA);
    }

    // Carry lane 3's 410 result to the idempotency test below (same describe, same
    // worker — `.serial` guarantees ordering).
    laneFs410 = fs410;

    await expectReampBalanced(page, reampBase);
  });

  // ── Idempotency addendum, ON THE SAME SAVED STATE T1′ just wrote ────────────────────
  test("idempotency: base and footswitch skip-branch re-runs make zero new writes (410)", async ({
    page,
  }) => {
    test.skip(!(await isOnline(page)), "online-only: needs real audio");
    test.setTimeout(600_000);
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    if (!laneFs410) {
      throw new Error(
        "the arc test above must run first and set laneFs410 (same describe.serial block)",
      );
    }
    const fs = laneFs410;

    // The retired Hiwatt arc's own SKIP-LEGITIMACY guard, folded in here: a `saved:false`
    // result on the FIRST run (above) is legitimate ONLY when the stored value already
    // measured within FS tolerance of target — never a silent "did nothing" masquerading
    // as success.
    for (const r of fs) {
      if (!r.saved) {
        expect(
          Math.abs(r.predicted_lufs - FS_TARGET_410),
          `switch ${String(r.switch)} skipped its save on the first run, legitimate only \
when the stored value already measured within tolerance of target`,
        ).toBeLessThanOrEqual(0.11);
      }
    }

    // ── Base skip-branch ──
    const baseRerun = (await invoke(
      page,
      "level_preset",
      { job: baseLevelJob(FRIEDMAN.slot, BASE_TARGET_410) },
      T,
    )) as LevelResult;
    expect(
      baseRerun.clamped,
      "base re-run must reach target unclamped (a real skip, not a clamp)",
    ).toBe(false);
    expect(
      baseRerun.saved,
      "base re-run solved the same value the arc test already saved → must skip the write (level_unchanged)",
    ).toBe(false);
    expect(
      baseRerun.base_boost,
      "the see-saw guard holds on re-run too: still TRADE, never BOOST",
    ).toBeNull();

    // ── Footswitch skip-branch (410's own TubeScreamer row only — the carried-over
    // UniVibe row belongs to T1′'s modulated-solve coverage, not this idempotency proof) ──
    const wroteInLane3 = fs.filter((r) => r.saved).map((r) => r.switch);
    const fs2 = (await invoke(
      page,
      "level_footswitches_apply",
      {
        slot: FRIEDMAN.slot,
        jobs: [
          {
            switch: 1,
            levGroupId: "G1",
            levNodeId: "ACD_TubeScreamer",
            levParameterId: "level",
            targetLufs: FS_TARGET_410,
          },
        ],
        save: true,
        topologyId: "guitar-humbucker",
        calibrationLufs: null,
        profileId: null,
        onResult: "__CHANNEL__:1",
      },
      T * 2,
    )) as FootswitchLevelResult[];

    for (const r2 of fs2) {
      expect(
        r2.clamp_reason,
        `switch ${String(r2.switch)} re-run must be measurable (no clamp reason)`,
      ).toBeNull();
      expect(
        r2.clamped,
        `switch ${String(r2.switch)} re-run must reach target unclamped`,
      ).toBe(false);
      expect(
        Math.abs(r2.predicted_lufs - FS_TARGET_410),
        `switch ${String(r2.switch)} re-run must land on target`,
      ).toBeLessThanOrEqual(0.11);
      if (wroteInLane3.includes(r2.switch)) {
        expect(
          r2.saved,
          `switch ${String(r2.switch)} wrote a real value at target in the arc test → this re-run at the same target must skip the write`,
        ).toBe(false);
      }
    }

    await expectReampBalanced(page, reampBase);
  });
});

// ── T5: Plumes-shape (405) first-run journey — the BOOST case paired with T1′'s TRADE
// control above, run back-to-back in the same online session (see-saw surfaces in one run) ──
test.describe("Level online — Plumes-shape first-run journey (405)", () => {
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  // 4 drive pedals, retargeted with the offline gate (level-fs-preset24.spec.ts) to -23/-23/
  // -21/-21 — kept local to this file rather than imported, since cross-spec imports of test
  // fixtures aren't this codebase's convention (each spec owns its own job literals).
  const SWITCH_JOBS_405 = [
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
      levParameterId: "level",
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
  const BASE_TARGET_405 = -23; // Rhythm (profiles.rs::default_targets)

  test("first-run UI journey: base BOOST via the wizard, 4 footswitch rows, strict re-measure", async ({
    page,
  }) => {
    test.skip(!(await isOnline(page)), "online-only: needs real audio");
    // Base UI drive (3 conns/1 engage via the wizard) + a same-target confirm read + a
    // 4-switch footswitch batch + 5 strict ffmpeg re-measures — the plan's own ~5 min
    // estimate, generously padded.
    test.setTimeout(600_000);
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    // Cheap probe read BEFORE the run (a single quick capture, not a full leveling cycle):
    // confirm the Plumes shape's pedals-off base is genuinely well short of target — the
    // precondition the BOOST regime exists to close. Guards against silently asserting a
    // BOOST outcome against a fixture a prior run already maxed out.
    const asIs = (await invoke(
      page,
      "e2e_measure_sound",
      {
        slot: PRESET24.slot,
        scene: null,
        footswitch: null,
        topologyId: "guitar-humbucker",
        lev: null,
      },
      T,
    )) as number;
    expect(
      asIs,
      "the Plumes shape's pedals-off base must start well short of target — the precondition \
this run's BOOST regime exists to close",
    ).toBeLessThan(BASE_TARGET_405 - 3);

    // The UI journey itself: the wizard end to end, proving the base_boost summary-row
    // disclosure (SummaryPage.tsx's `baseBoostSentence`) renders in the real app, not just on
    // the wire. `Rhythm` = -23 LUFS (profiles.rs::default_targets), matching BASE_TARGET_405.
    await runBaseLevel(page, [{ preset: PRESET24, label: "Rhythm" }]);
    // `useGroupOpen`'s `badSlots` auto-open list is built from non-"done" rows only
    // (SummaryPage.tsx) — an all-good run's preset group starts COLLAPSED on Summary, same as
    // level-defaults.spec.ts's own boost-sentence test (a). Expand it before reading the row
    // detail, or the sentence below is never in the DOM to match against.
    await page
      .locator(`[data-preset-group="${String(PRESET24.slot)}"]`)
      .click();
    await expect(
      page.getByText(
        /Turned this preset up as far as it goes and raised the amp/,
      ),
    ).toBeVisible();

    // Assert the PERSISTED pair directly rather than re-invoking `level_preset` a second
    // time: a same-target re-run's OWN result is unreliable evidence of what the UI run above
    // actually solved and saved, on every realistic branch — the idempotency skip hard-codes
    // `base_boost: null` (the solve is skipped entirely), a hairline re-measure can flip
    // `clamped: true`, and a within-tolerance re-plan that doesn't re-enter BOOST also reports
    // `base_boost: null` (`leveller::level_preset_impl`'s routing). `list_level_blocks` reads
    // the SAVED state instead — same seam and the SAME fixture (405) as
    // level-fs-preset24.spec.ts's own lazy-commit-gap read, which pins the Twin's fader at its
    // solved ≈0.498 after this exact boost; a looser tolerance here (vs that file's exact
    // offline model) accounts for real-HW measurement noise in the closed-loop fader solve.
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
      "the Twin's fader persisted at its boosted ≈0.498 value",
    ).toBeCloseTo(0.498, 1);

    // 4 footswitch rows (base MUST run first — the pl-context trap 405's own fixture
    // ordering pins, e2e/fixtures/COVERAGE.md row 36) — raw invoke, the channel-streaming
    // seam (.claude/rules/e2e.md).
    const fs = (await invoke(
      page,
      "level_footswitches_apply",
      {
        slot: PRESET24.slot,
        jobs: SWITCH_JOBS_405,
        save: true,
        topologyId: "guitar-humbucker",
        calibrationLufs: null,
        profileId: null,
        onResult: "__CHANNEL__:1",
      },
      T * 3,
    )) as FootswitchLevelResult[];
    for (const [i, r] of fs.entries()) {
      const job = SWITCH_JOBS_405[i];
      expect(
        r.clamp_reason,
        `switch ${String(r.switch)} must have signal`,
      ).toBeNull();
      expect(
        r.clamped,
        `switch ${String(r.switch)} must reach target, not clamp`,
      ).toBe(false);
      expect(
        Math.abs(r.predicted_lufs - job.targetLufs),
        `switch ${String(r.switch)} solved-vs-target`,
      ).toBeLessThanOrEqual(KNOB_TOL_LU);
    }

    // 5 strict ffmpeg re-measures: base + all 4 pedals, from the saved state.
    const measure405 = (
      job?: (typeof SWITCH_JOBS_405)[number],
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
          validate: {
            targetLufs: job ? job.targetLufs : BASE_TARGET_405,
            clamped: false,
            persistMismatch: null,
          },
        },
        T,
      ) as Promise<number>;

    const heardBase = await measure405();
    expect(
      Math.abs(heardBase - BASE_TARGET_405),
      `base re-measures at ${heardBase.toFixed(2)} LUFS from the saved state`,
    ).toBeLessThanOrEqual(DELTA);
    for (const job of SWITCH_JOBS_405) {
      const heard = await measure405(job);
      expect(
        Math.abs(heard - job.targetLufs),
        `switch ${String(job.switch)} re-measures at ${heard.toFixed(2)} LUFS from the saved state`,
      ).toBeLessThanOrEqual(DELTA_FS);
    }

    await expectReampBalanced(page, reampBase);
  });
});
