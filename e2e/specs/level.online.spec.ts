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
// ── ROOT LESSON (P4-B real-device run, 2026-08-31): SELF-CALIBRATE, never assert a fixture's
// numbers directly. A first pass at T1′/T5 hard-coded the offline sim sidecar's own numbers
// (`scenario-loudness.json`'s per-scene/base ceilings) as ONLINE targets and preconditions.
// Both failed on real hardware: T1′'s scene target sat above a scene's REAL ceiling (the sim
// ceiling doesn't hold as device physics — clamped instead of leveling), and T5's boost
// precondition assumed a sim-model pedals-off ceiling of -28 when the real unit measured
// -23.47 (only 0.47 dB short of the -23 target, not the >3 dB the BOOST regime needs — the
// resident scenario state, e2e.md, means a PRIOR run's own leveling can already sit near the
// real ceiling). Fixture-engineered magnitudes are SIM-MODEL truths, not device truths. Every
// target/precondition below is now derived at runtime from the device's OWN as-is reading (or
// its own solved `constant_c`), and every assertion is a regime/convergence/tolerance check
// against that derived value — never a fixture constant.
//
// T1′ (below, describe #1's first test) — a CONSOLIDATED Friedman-shape (410) arc: base + all
// 3 scenes + 2 footswitches, re-measured strict for base+scenes only (4 ffmpeg rows, not 9).
// 410's own base pair is a plain TRADE solve (`headroom_trade::plan_level_pair`'s
// `G≈+1.0 <= P_up≈+6.0`), so it must NEVER enter BOOST — this is the "see-saw" CONTROL this
// arc exists to run, back-to-back with T5's Plumes-shape (405) BOOST case below, so a
// regression that widened BOOST's trigger condition and started moving 410's fader too would
// be caught in the SAME online session. Scene and footswitch targets are DERIVED (as-is minus
// 2 dB — always reachable, since the per-scene/per-switch handle only needs to go QUIETER, and
// the floor is ~0.01 ≈ -34 dB of room); the AS-IS probes run AFTER the base lane's own save,
// not before — base leveling moves the preset's `presetLevel`, which scales every scene/FS
// capture on the SAME slot, so a probe taken before base would derive a target that no longer
// matches the post-base ceiling. The base target itself defaults to -23 and only falls back
// (to the run's own solved `constant_c - 1`, re-using the FIRST attempt's own result — no
// extra device round trip on the common non-clamping path) if -23 turns out unreachable on
// this unit. One of the 2 footswitches is 410's own TubeScreamer row; the other is HIWATT
// (404) switch 12 (`ACD_UniVibe.volume`), CARRIED VERBATIM from the retired arc's own
// `SWITCH_JOBS` so its modulated-response solve (an LFO'd knob, the one class of assertion the
// retired arc uniquely exercised) keeps a named carrier per the plan's own budget table —
// touching 404 costs nothing extra since it stays resident regardless.
//
// T2 (below, describe #1's second test) — the idempotency addendum, KEPT and RETARGETED from
// Hiwatt (404) to 410, on the SAME saved state T1′ just wrote (no reseed, `.serial` ordering).
// It also now carries the retired arc's own SKIP-LEGITIMACY guard (its lane-3 loop's
// `if (!r.saved) …` check on the FIRST run's own footswitch result — a `saved:false` first-run
// result is legitimate ONLY when the stored value already measured within FS tolerance, never
// a silent no-op): folded in here rather than left in T1′ because idempotency (a re-run
// making zero writes) and this skip-legitimacy proof are the same "was a no-write outcome
// actually correct" property, on 410's much smaller footswitch surface (1 row, not 4). T2 now
// reads its targets off T1′'s own DERIVED values (carried the same way `laneFs410` already
// was), since there is no longer a fixture constant to import.
//
// T3 (enumeration) is GONE — moved offline as an `e2e_server_tests.rs` gate (E9); a pure list
// read has negligible device truth to verify. T4 (two-preset Base UI flow) is GONE — its
// "drives the real UI" claim is subsumed by T1′ (this file only levels 410 via raw invoke,
// see below) plus T5 (drives the wizard UI on 405). Session-budget arithmetic (revised for
// self-calibration's extra reads): T1′ adds 5 as-is probes (~2 min) to its prior ~8 min ≈
// 10 min; T2 stays ~2 min. T5 adds a pedals-off preview + a conditional fader-calibration
// write + (when calibration ran) the 150 s lazy-commit wait its OWN save isn't witnessed for
// (danger.md: the block-knob save carries no `PresetLevel` reassert, so `ensure_fresh_load`
// has nothing to wait on automatically — see T5's own comment) on top of its prior ~5 min ≈
// 10-12 min. Total ≈ 22-24 min for this file, against the leveling suite's ratified ≤ 25 min
// online budget — tight enough that `test.setTimeout` on both T1′ and T5 is bumped to 900 s
// (from T5's prior 600 s) to keep margin over the per-test estimate, matching this file's
// existing convention of padding generously past the plan's own numbers.
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
// so the two runs are directly comparable in one online session. This is a DEFAULT ATTEMPT,
// not a guarantee — the real device's own ceiling wins if -23 turns out unreachable (T1′
// falls back to the run's own solved `constant_c - 1` when the first attempt clamps; see the
// file header's ROOT LESSON and T1′'s own comment).
const BASE_TARGET_410 = -23;
// Scene and footswitch targets are no longer fixture constants — see T1′'s own AS-IS
// derivation (file header's ROOT LESSON: a fixed sim-model ceiling doesn't hold as device
// physics). `SCENE_TARGET_410`/`FS_TARGET_410`/`FS_TARGET_UNIVIBE` used to live here.

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
  /** Solved constant `C` in `LUFS = 20*log10(level) + C` (max reachable LUFS) — leveller.rs's
   *  `LevelResult::constant_c` doc. Used by T1′'s base fallback and T5's self-calibration to
   *  read the run's own measured ceiling instead of trusting a fixture number. */
  constant_c: number;
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
  // The self-calibrated targets T1′ actually used (see the file header's ROOT LESSON) —
  // there is no longer a fixture constant for T2 to re-import, so these ride the same
  // carry-across-tests pattern `laneFs410` already uses.
  let baseTarget410: number | undefined;
  let fsTarget410: number | undefined;

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
    // -23 is a DEFAULT ATTEMPT, not a guarantee (file header's ROOT LESSON): the sim model's
    // own base ceiling (-18) doesn't bind the real device. If -23 clamps, fall back to THIS
    // attempt's own solved `constant_c` (the real measured ceiling) minus 1 dB — always
    // reachable moving down, still a plain presetLevel TRADE — and re-run once. This costs
    // nothing extra on the common (non-clamping) path.
    let baseTargetUsed = BASE_TARGET_410;
    let base = (await invoke(
      page,
      "level_preset",
      { job: baseLevelJob(FRIEDMAN.slot, baseTargetUsed) },
      T,
    )) as LevelResult;
    if (base.clamped) {
      baseTargetUsed = base.constant_c - 1;
      base = (await invoke(
        page,
        "level_preset",
        { job: baseLevelJob(FRIEDMAN.slot, baseTargetUsed) },
        T,
      )) as LevelResult;
    }
    expect(
      base.clamped,
      `base must reach target ${baseTargetUsed.toFixed(2)} (originally ${String(BASE_TARGET_410)}), not clamp`,
    ).toBe(false);
    expect(base.saved, "base must level and save").toBe(true);
    expect(
      base.base_boost,
      "410's base pair is a plain TRADE solve (G <= P_up) — it must never enter BOOST, or a \
regression has widened BOOST's trigger and would start moving a fader nothing asked it to move",
    ).toBeNull();
    baseTarget410 = baseTargetUsed;

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

    // ── Self-calibration (file header's ROOT LESSON): probe each scene's and each
    // footswitch's CURRENT as-is loudness — taken AFTER the base save above, since base
    // leveling just moved `presetLevel`, which scales every capture on this same slot — and
    // derive each target as (as-is - 2 dB). A target quieter than what's already achieved is
    // ALWAYS reachable (the per-scene overlay / per-switch knob only needs to go DOWN, and
    // the floor is ~0.01 ≈ -34 dB of room), so this can never clamp the way a fixed sim-model
    // ceiling did on real hardware. HIWATT (404) is a different slot with its own unrelated
    // `presetLevel`, so its probe is unaffected by 410's base save timing, but it's read here
    // too for a single "5 probes" batch (plan's own budget accounting).
    const asIsScenes: number[] = [];
    for (const sceneSlot of [0, 1, 2]) {
      asIsScenes.push(await measure410({ scene: sceneSlot }));
    }
    const sceneTarget410 = Math.min(...asIsScenes) - 2.0;
    const asIsFs410 = (await invoke(
      page,
      "e2e_measure_sound",
      {
        slot: FRIEDMAN.slot,
        scene: null,
        footswitch: 1,
        topologyId: "guitar-humbucker",
        lev: null,
        validate: null,
      },
      T,
    )) as number;
    const fsTargetUsed410 = asIsFs410 - 2.0;
    const asIsUniVibe = (await invoke(
      page,
      "e2e_measure_sound",
      {
        slot: HIWATT.slot,
        scene: null,
        footswitch: 12,
        topologyId: "guitar-humbucker",
        lev: null,
        validate: null,
      },
      T,
    )) as number;
    const fsTargetUniVibe = asIsUniVibe - 2.0;
    fsTarget410 = fsTargetUsed410;

    const scenes = (await invoke(
      page,
      "level_scenes_apply_batched",
      {
        slot: FRIEDMAN.slot,
        jobs: [0, 1, 2].map((sceneSlot) => ({
          sceneSlot,
          targetLufs: sceneTarget410,
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
      // danger.md's batched-save revert class ("a batched scene-leveling save can revert the
      // ONE scene it just leveled, if that scene is also `restore_scene`") is DETECTED, not
      // silent — 410's `lastLoadedScene` is 1, so scene 1 is exactly the row that class fires
      // on. The run publishes its own verdict here; a spec that re-measures without reading it
      // ignores the run's own warning and reports the revert as an unexplained level miss.
      expect(
        r.persist_mismatch ?? false,
        `scene ${id} must persist what it solved (danger.md's batched-save revert class)`,
      ).toBe(false);
    }

    // ── The FS-lane isolation check: one scene re-measured RIGHT NOW, before lane 3 writes
    // anything. 410's TubeScreamer is base-ON and footswitch-owned, so lane 3's solve moves a
    // block every scene's capture renders through unless the scenes' own overlays shadow it.
    // Re-measuring here and again at the end brackets lane 3: a row that is on target now and
    // off target later was moved BY lane 3 (a real see-saw regression — one lane undoing
    // another's work), while a row that is already off target here was mis-solved by its own
    // batch. Without this bracket the two are indistinguishable at the end of the arc.
    const scene0Immediate = await measure410({ scene: 0 });
    expect(
      Math.abs(scene0Immediate - sceneTarget410),
      `scene 0 re-measures at ${scene0Immediate.toFixed(2)} LUFS immediately after its own \
batch save (target ${sceneTarget410.toFixed(2)}, solved overlay \
${String(scenes.find((r) => r.scene_slot === 0)?.final_level)}, as-is readings \
${asIsScenes.map((v) => v.toFixed(2)).join("/")}) — before lane 3 touches the shared \
base-ON TubeScreamer`,
    ).toBeLessThanOrEqual(DELTA);

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
            targetLufs: fsTargetUsed410,
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
        Math.abs(r.predicted_lufs - fsTargetUsed410),
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
            targetLufs: fsTargetUniVibe,
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
        Math.abs(r.predicted_lufs - fsTargetUniVibe),
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
        targetLufs: baseTargetUsed,
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
          targetLufs: sceneTarget410,
          clamped: row?.clamped ?? false,
          persistMismatch: row?.persist_mismatch ?? null,
        },
      });
    }
    // Every solved value the arc wrote, quoted into any failure below: a level miss is only
    // diagnosable against what the lane BELIEVED it wrote (a mis-solve moves `final_level`; a
    // later lane clobbering a correct solve does not), and the online server log that would
    // otherwise carry these numbers is overwritten by whichever spec runs next.
    const solvedTrace = [
      `base final_level=${base.final_level.toFixed(4)} C=${base.constant_c.toFixed(2)}`,
      ...scenes.map(
        (r) =>
          `scene${String(r.scene_slot)} final_level=${r.final_level.toFixed(4)}`,
      ),
      `scene0 immediately after its batch=${scene0Immediate.toFixed(2)}`,
      `scene as-is=${asIsScenes.map((v) => v.toFixed(2)).join("/")}`,
      `fs410 as-is=${asIsFs410.toFixed(2)} target=${fsTargetUsed410.toFixed(2)}`,
    ].join(", ");
    for (const [sound, lufs] of Object.entries(heard)) {
      const target = sound === "base" ? baseTargetUsed : sceneTarget410;
      expect(
        Math.abs(lufs - target),
        `${sound} re-measures at ${lufs.toFixed(2)} LUFS from the saved state \
(target ${target.toFixed(2)}) — ${solvedTrace}`,
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

    if (
      !laneFs410 ||
      baseTarget410 === undefined ||
      fsTarget410 === undefined
    ) {
      throw new Error(
        "the arc test above must run first and set laneFs410/baseTarget410/fsTarget410 \
(same describe.serial block)",
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
          Math.abs(r.predicted_lufs - fsTarget410),
          `switch ${String(r.switch)} skipped its save on the first run, legitimate only \
when the stored value already measured within tolerance of target`,
        ).toBeLessThanOrEqual(0.11);
      }
    }

    // ── Base skip-branch ──
    const baseRerun = (await invoke(
      page,
      "level_preset",
      { job: baseLevelJob(FRIEDMAN.slot, baseTarget410) },
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
            targetLufs: fsTarget410,
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
        Math.abs(r2.predicted_lufs - fsTarget410),
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
    // Preview read + conditional fader calibration (up to ~4 engages) + the 150 s lazy-commit
    // wait (only paid when calibration actually wrote something) + base UI drive (3 conns/1
    // engage via the wizard) + a same-target confirm read + a 4-switch footswitch batch + 5
    // strict ffmpeg re-measures. See the file header's revised budget note.
    test.setTimeout(900_000);
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    // ── Self-calibration (file header's ROOT LESSON; P4-B real-device evidence 2026-08-31):
    // a fixed "pedals-off base starts >3 dB short of target" precondition is a SIM-MODEL
    // fact, not a device one — the resident scenario state (e2e.md) means a prior run's own
    // leveling can already sit the real ceiling within a fraction of a dB of target (HW: C
    // measured -23.47 against a -23 target, only 0.47 dB short). Rather than assert that
    // number, GUARANTEE the BOOST precondition: read the current pedals-off ceiling and, if
    // it's within 5 dB of target, lower the Twin's own fader (never `presetLevel`, which the
    // real run below must be free to raise on its own) until pedals-off sits comfortably
    // ~5 dB short.
    //
    // There is no dedicated "write one block parameter + save" command in this codebase (the
    // e2e command surface only re-measures; `copy_apply`'s ops are structural
    // replace/insert/remove, not a numeric param setter) — the only invoke seam that can write
    // a block parameter AND persist it is `level_preset`'s own BLOCK-KNOB arm (the same one
    // `list_level_blocks`/`baseLevelJob`'s `block_*` fields already expose), a closed-loop
    // TARGET solve, not a literal value write. So the desired fader change is expressed as a
    // RAW LUFS target for that closed loop rather than a value to set directly.
    // `list_level_blocks` is a LIVE load-then-discover read (commands/level_preset.rs's
    // `load_then_discover_blocks`): it loads the slot and then reads back the ACTIVE graph. A
    // cross-slot load issued while ANOTHER slot's save is still committing can be ignored by
    // the device, and discovery then reads the previous preset's graph and answers confidently
    // with the wrong preset's blocks — this test runs straight after the 410 arc above, so it
    // is the first device op after that arc's saves. Verify the answer by IDENTITY (the Twin
    // this fixture is built around) and re-read once after a settle rather than trusting the
    // first reply; a green predecessor's own minutes of re-measures usually clear the window,
    // which is exactly why this must not be left to depend on the predecessor's timing.
    let blocksPre = (await invoke(
      page,
      "list_level_blocks",
      { slot: PRESET24.slot },
      T,
    )) as LevelBlock[];
    const findTwin = (bs: LevelBlock[]): LevelBlock | undefined =>
      bs.find(
        (b) =>
          b.node_id === "ACD_TwinReverb65NoFx" &&
          b.parameter_id === "outputLevel",
      );
    let twinCandidatePre = findTwin(blocksPre);
    if (!twinCandidatePre) {
      await page.waitForTimeout(150_000); // danger.md's COMMIT_WINDOW_SECS
      blocksPre = (await invoke(
        page,
        "list_level_blocks",
        { slot: PRESET24.slot },
        T,
      )) as LevelBlock[];
      twinCandidatePre = findTwin(blocksPre);
    }
    expect(
      twinCandidatePre,
      `the Twin's outputLevel candidate must be discoverable before calibration — \
list_level_blocks(${String(PRESET24.slot)}) answered with \
[${blocksPre.map((b) => `${b.node_id}.${b.parameter_id}`).join(", ")}], which is another \
preset's graph if the Twin is absent (a cross-slot load that did not take)`,
    ).toBeDefined();
    if (!twinCandidatePre) {
      throw new Error(
        "the Twin's outputLevel candidate must be discoverable before calibration",
      );
    }

    // Raw pedals-off reading at the CURRENT presetLevel/fader. `e2e_measure_sound`'s base
    // branch (scene=null, footswitch=null) isolates every footswitch-owned block off
    // (e2e_server.rs's `doctor_force_bypass(&saved["ftsw"], &saved, footswitch)` with
    // `footswitch: None`) — the SAME isolation `level_preset`'s own base arm uses
    // (`commands/level_preset.rs`'s `base_isolation_or_refuse`), so this raw reading and the
    // `constant_c` below are commensurable (same measurement context, different math on top).
    const asIsRaw = (await invoke(
      page,
      "e2e_measure_sound",
      {
        slot: PRESET24.slot,
        scene: null,
        footswitch: null,
        topologyId: "guitar-humbucker",
        lev: null,
        validate: null,
      },
      T,
    )) as number;

    // A DRY preview (save:false) of the ordinary presetLevel-only solve: never writes, just
    // reports the SOLVED `constant_c` — the pedals-off ceiling extrapolated to presetLevel=1
    // (leveller.rs's own doc on `LevelResult::constant_c`), which is presetLevel-INDEPENDENT
    // (unlike `asIsRaw`, which reads at whatever presetLevel happens to be currently stored —
    // resident state across runs, per e2e.md, so it can't be assumed to be 1.0).
    const preview = (await invoke(
      page,
      "level_preset",
      { job: { ...baseLevelJob(PRESET24.slot, BASE_TARGET_405), save: false } },
      T,
    )) as LevelResult;

    const deficitTarget = BASE_TARGET_405 - 5; // guaranteed >=5 dB short of target
    const shortfall = preview.constant_c - deficitTarget; // dB the ceiling must still drop
    let fCalibrated = twinCandidatePre.value;
    if (shortfall > 0) {
      // Anchor the fader-lowering target in RAW units (asIsRaw minus the SAME shortfall) so
      // the current (unknown, resident) presetLevel CANCELS OUT of the derivation: both
      // `asIsRaw` and `constant_c` were measured at the same presetLevel, so subtracting the
      // same shortfall from the raw reading yields the raw target that corresponds to
      // `constant_c` dropping by `shortfall` — regardless of what presetLevel actually is.
      // (This is the log-formula `f_new = fader * 10^(-shortfall/20)` restated as a LUFS
      // target for the closed loop to converge to, since there is no direct value-write seam
      // to hand `f_new` to — see the comment above.)
      const blockTarget = asIsRaw - shortfall;
      const calibration = (await invoke(
        page,
        "level_preset",
        {
          job: {
            slot: PRESET24.slot,
            target_lufs: blockTarget,
            save: true,
            topology_id: "guitar-humbucker",
            calibration_lufs: null,
            stimulus_path: null,
            profile_id: null,
            block_group_id: twinCandidatePre.group_id,
            block_node_id: twinCandidatePre.node_id,
            block_parameter_id: twinCandidatePre.parameter_id,
            block_value: twinCandidatePre.value,
          },
        },
        T,
      )) as LevelResult;
      expect(
        calibration.clamped,
        "the calibration fader-lower must reach its (quieter) target, not clamp",
      ).toBe(false);
      expect(calibration.saved, "the calibration write must save").toBe(true);
      fCalibrated = calibration.final_level;

      // The block-knob arm measures AS-SAVED (with whatever footswitches are currently
      // engaged — it does NOT isolate like the presetLevel arm does), and its save carries no
      // `PresetLevel` reassert, so `derive_save_witness` registers NOTHING
      // (leveller.rs's `apply_levels` → `recall_reassert_save`): this write is NOT covered by
      // `ensure_fresh_load`'s barrier. Any same-slot 405 touch below (the wizard's own load)
      // must wait out the lazy-commit window itself or risk racing pre-save bytes (danger.md:
      // `saveCurrentPreset` commits LAZILY, observed T+45-100 s, COMMIT_WINDOW_SECS=150 s).
      await page.waitForTimeout(150_000);
    }

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
    // the SAVED state instead — same seam as level-fs-preset24.spec.ts's own lazy-commit-gap
    // read on the SAME fixture (405).
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
    // A derived BAND off our OWN calibrated baseline, never a hard-coded model number (the
    // old `toBeCloseTo(0.498, 1)` was the offline sim model's own number, not a real-HW
    // invariant — file header's ROOT LESSON). Boost maxes `presetLevel` FIRST (the disclosure
    // sentence above just confirmed it fired), so the fader's OWN share of closing the
    // >=5 dB deficit this test engineered is itself >=5 dB, inflated further by whatever the
    // drive pedals' base-ON contribution already was — so the solved fader must be strictly
    // louder than the calibrated baseline, by at least (5 - 1) dB (1 dB slack for solve
    // noise/secant residual).
    expect(
      twinFader?.value ?? Number.NaN,
      "the boosted fader must be LOUDER than the calibrated baseline (a real boost, not a no-op)",
    ).toBeGreaterThan(fCalibrated);
    expect(
      twinFader?.value ?? Number.NaN,
      `the boosted fader must close at least the ~5 dB (minus 1 dB solve-noise slack) deficit \
off the calibrated baseline ${fCalibrated.toFixed(4)}`,
    ).toBeGreaterThanOrEqual(fCalibrated * 10 ** ((5 - 1) / 20));

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
