import type { Page } from "@playwright/test";
import { test, expect } from "../fixtures/test";
import { SERVER } from "../fixtures/port";
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
  runBaseLevel,
} from "../fixtures/scenario";

// ONLINE Level rework (P4, the Plumes/BD2/OCD leveling-regression fix). Two arcs, run
// back-to-back in one session so a 27↔28 see-saw regression surfaces in a single run: T1′
// (below) — Friedman-shape (410)'s consolidated base+footswitches+scenes arc + idempotency,
// the TRADE/see-saw CONTROL, lane order base → footswitches → scenes matching production's own
// `runRank` (notes/gotchas.md's "A scene renders the base chain's ON blocks" entry). T5 —
// Plumes-shape (405)'s first-run BOOST journey via the wizard UI.
// SELF-CALIBRATE: every target/precondition below is derived at RUNTIME from the device's own
// as-is reading or solved `constant_c`, never a fixture constant — a sim-model magnitude (e.g.
// a `scenario-loudness.json` ceiling) is a sim-model truth, not a device one, and asserting it
// directly has already failed against real hardware once.
// Measured real-device run: ~26-27 min, ~5 min of it two fixed 150 s T5 stalls for a lazy save
// commit that fw 1.8.58 does not have — both are gone.
// Rows this file emits per run: T1′'s 1 base + `SCENES_410.length`, T5's 1 base +
// `SWITCH_SPECS_405.length` — `scripts/e2e.sh` greps the line below and compares by EQUALITY.
// STRICT_VALIDATE_ROWS=6
// The same rows by identity (`validate_log`'s label: list index + scene/switch). Each must be
// MEASURED and PASS under the ffmpeg judge, or the run does not certify.
// STRICT_VALIDATE_LABELS=base:slot410 scene:slot410:scene0 scene:slot410:scene1 base:slot405 footswitch:slot405:switch5 footswitch:slot405:switch8
// The REMOTE twin runs only the two re-measures that guard an ORDERING regression (`isRemote`
// below): 410's scene 0 (the see-saw row) and 405's Rat (the base-ON pedal). Its audio is not
// calibrated against the unit and `scripts/e2e-remote.sh` runs no ffmpeg judge, so the other
// four only re-check a solve the run already reported. The unit runs all six.

// COVERAGE rows 37, 45
const FRIEDMAN = SCENARIO[10]; // E2E Friedman 3S — the P4 leveling-regression fixture
const PRESET24 = SCENARIO[5]; // E2E Preset24 — the Plumes-shape first-run journey (T5)

// 410's base pair is a plain TRADE solve at -23 (G≈+1.0 <= P_up≈+6.0, plan physics section):
// presetLevel alone closes the gap, so the fader must NEVER move — this is the see-saw
// control target, deliberately the SAME numeric target as T5's Plumes-shape BOOST case below
// so the two runs are directly comparable in one online session. This is a DEFAULT ATTEMPT,
// not a guarantee — the real device's own ceiling wins if -23 turns out unreachable (T1′
// falls back to the run's own solved `constant_c - 1` when the first attempt clamps; see the
// file header's SELF-CALIBRATE note and T1′'s own comment).
const BASE_TARGET_410 = -23;
// The scene rows 410's arc levels. Its three overlays are byte-identical apart from
// `sceneName`, so the third only costs a capture pair. Scene 2 is the droppable one: 0 is the
// see-saw guard row (its strict re-measure below), 1 is `lastLoadedScene` (danger.md's
// batched-save revert class). The FIXTURE keeps all three — E9 pins `scene_count == 3`.
const SCENES_410 = [0, 1];
// Scene and footswitch targets are no longer fixture constants — see T1′'s own AS-IS
// derivation (file header's SELF-CALIBRATE note: a fixed sim-model ceiling doesn't hold as device
// physics). `SCENE_TARGET_410`/`FS_TARGET_410`/`FS_TARGET_UNIVIBE` used to live here.

const DELTA = 0.5; // base/scene: run-to-run noise + the one-shot/secant residual (unchanged)
const DELTA_FS = 1.0; // footswitch re-measure: KNOB_TOL_LU (0.3, leveller.rs) + capture noise
// The footswitch SOLVE's own acceptance band (not a re-measure) — matches leveller.rs's
// KNOB_TOL_LU exactly, no extra margin needed since this checks the solver's own report.
const KNOB_TOL_LU = 0.3;
// "On target" in every lane (leveller::ACCEPT_TOL_LU): the band inside which the footswitch
// solver accepts a reading and an idempotent re-run skips its write. About 2 sigma of the
// ~0.12 LU capture noise; the old 0.1 band sat below it.
const ACCEPT_TOL_LU = 0.25;

interface LevelResult {
  saved: boolean;
  clamped: boolean;
  final_level: number;
  scene_slot: number | null;
  persist_mismatch: boolean | null;
  /** Null unless the base pair entered the BOOST regime — see `src/lib/types.ts`'s
   *  `BaseBoostSummary` doc. 410's base pair is TRADE, so this must stay null throughout. */
  base_boost: { applied: boolean } | null;
  /** Solved constant `C` in `LUFS = 20*log10(level) + C` (max reachable LUFS) — leveller.rs's
   *  `LevelResult::constant_c` doc. Used by T1′'s base fallback and T5's self-calibration to
   *  read the run's own measured ceiling instead of trusting a fixture number. */
  constant_c: number;
}
/** A batched leveling command's return: the rows, with the run-end re-amp verdict beside them. */
interface BatchLevelRun<R> {
  rows: R[];
  reampStillOn: boolean;
}

interface FootswitchLevelResult {
  switch: number;
  saved: boolean;
  clamped: boolean;
  clamp_reason: string | null;
  predicted_lufs: number;
  /** The field-8 read after the save (`verify_fs_persisted_writes`): true when the stored
   *  row does not hold what the batch wrote. Null when nothing was saved. */
  persist_mismatch: boolean | null;
}
/** P5 external validation: what the run PROMISED for the sound about to be re-measured —
 *  see .claude/rules/e2e.md's "External validation" section. */
interface ValidateArg {
  targetLufs: number;
  clamped: boolean;
  persistMismatch: boolean | null;
}

const T = LEVEL_T;

/** Whether the server drives the REMOTE twin (`/health`'s `remote`), not the unit. */
async function isRemote(page: Page): Promise<boolean> {
  const res = await page.request.get(`${SERVER}/health`);
  return ((await res.json()) as { remote?: boolean }).remote === true;
}

/** The shared clamp_reason/clamped/predicted_lufs-within-tolerance triple every footswitch
 *  batch result is checked against. `targetOf` reads each row's own target (by result + its
 *  index in the batch, matching the jobs array's order); `tol` defaults to the solver's own
 *  KNOB_TOL_LU acceptance band. */
function assertFsSolved(
  results: FootswitchLevelResult[],
  targetOf: (r: FootswitchLevelResult, i: number) => number,
  tol: number = KNOB_TOL_LU,
): void {
  for (const [i, r] of results.entries()) {
    expect(
      r.clamp_reason,
      `switch ${String(r.switch)} must have signal`,
    ).toBeNull();
    expect(
      r.clamped,
      `switch ${String(r.switch)} must reach target, not clamp`,
    ).toBe(false);
    expect(
      Math.abs(r.predicted_lufs - targetOf(r, i)),
      `switch ${String(r.switch)} solved-vs-target`,
    ).toBeLessThanOrEqual(tol);
    // danger.md's "a load before `save_deferred_scene_writes` wipes the writes" class: the
    // run reads the stored row back after its save and publishes the verdict here. A saved
    // row must carry a real verdict: `null` there is an unverifiable re-read, not a pass.
    expect(
      r.saved ? r.persist_mismatch : false,
      `switch ${String(r.switch)} must persist what it solved`,
    ).toBe(false);
  }
}

/** Every requested switch must come back. `level_footswitches_apply` reports a row that
 *  errors (an audio overrun mid-solve, say) as a streamed Error item and leaves it out of the
 *  returned array, so `assertFsSolved` alone passes over it, and a later test that builds on
 *  the row then measures something else (a dropped switch-1 solve turned the idempotency
 *  re-run into a fresh solve on the unit, 2026-10-14). */
function expectEverySwitch(
  results: FootswitchLevelResult[],
  jobs: readonly { switch: number }[],
): void {
  expect(
    results.map((r) => r.switch).sort((a, b) => a - b),
    "every requested switch must come back (no silent mid-batch drop)",
  ).toEqual(jobs.map((j) => j.switch).sort((a, b) => a - b));
}

/** The "measure as-is, target = as-is minus 2 dB" idiom shared by every self-calibrated lane
 *  (file header's SELF-CALIBRATE note) — always reachable, since the handle only needs to go
 *  quieter and the floor is ~0.01 ≈ -34 dB of room. */
async function measureAsIsTarget(
  page: import("@playwright/test").Page,
  params: {
    slot: number;
    scene?: number | null;
    footswitch?: number | null;
    lev?: { groupId: string; nodeId: string; parameterId: string } | null;
  },
): Promise<{ asIs: number; target: number }> {
  const asIs = (await invoke(
    page,
    "e2e_measure_sound",
    {
      slot: params.slot,
      scene: params.scene ?? null,
      footswitch: params.footswitch ?? null,
      topologyId: "guitar-humbucker",
      lev: params.lev ?? null,
      validate: null,
    },
    T,
  )) as number;
  return { asIs, target: asIs - 2.0 };
}

test.describe("Level online — Friedman-shape consolidated arc + idempotency (410)", () => {
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  test("base + 2 scenes + a footswitch re-measure at target after save, an identical base re-run writes nothing; base stays TRADE, never BOOST (see-saw guard)", async ({
    page,
  }) => {
    test.skip(!(await isOnline(page)), "online-only: needs real audio");
    // Base (3 conns/1 engage) + 2 scenes (one batch) + 1 footswitch batch + 3 strict
    // ffmpeg re-measures — generous headroom over the plan's own ~8 min estimate.
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
    // -23 is a DEFAULT ATTEMPT, not a guarantee (file header's SELF-CALIBRATE note): the sim model's
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

    // ── Lane 2: the footswitch, run BEFORE scenes (production order: `runRank` in
    // src/views/level/leveling.ts ranks footswitch=1 ahead of scene=2, both after base=0) —
    // 410's own TubeScreamer row. The as-is probe is taken AFTER the base save above, since
    // base leveling just moved `presetLevel`, which scales every capture on this same slot.
    // (The modulated UniVibe row on 404 this lane once carried is gone: a footswitch solve on
    // a LFO'd knob is the same solver path as this row, and it cost a probe, a solve, a save
    // and the next run's re-import of 404.)
    const { asIs: asIsFs410, target: fsTargetUsed410 } =
      await measureAsIsTarget(page, {
        slot: FRIEDMAN.slot,
        footswitch: 1,
      });

    const fs410Run = (await invoke(
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
    )) as BatchLevelRun<FootswitchLevelResult>;
    const fs410 = fs410Run.rows;
    expectEverySwitch(fs410, [{ switch: 1 }]);
    assertFsSolved(fs410, () => fsTargetUsed410);

    // ── Lane 3: all 3 scenes (amp outputLevel, one batch, save) — run AFTER footswitches,
    // per the same production `runRank`. This is the fix: 410's TubeScreamer is base-ON and
    // footswitch-owned, and all 3 scene overlays render through it (their overlays carry only
    // `ACD_MarshallPlexi`), so the scene as-is probes below MUST be taken after lane 2's
    // footswitch save — otherwise the derived scene target reflects a TubeScreamer value lane 2
    // is about to change out from under it. HW proof this was the bug: with footswitches
    // running AFTER scenes (the old order), lane 2's 2.0 dB cut to TubeScreamer (as-is -21.31 →
    // target -23.31) silently re-moved all 3 already-on-target scenes — scene 0 read exactly on
    // target (-29.33) right after its own batch save, then drifted to -31.34 by the end of the
    // arc.
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

    // Self-calibration (file header's SELF-CALIBRATE note): probe each scene's CURRENT as-is loudness
    // — taken after BOTH base and the footswitch lane above, so it reflects the FINAL
    // TubeScreamer value nothing downstream will move again — and derive the target as
    // (as-is - 2 dB). A target quieter than what's already achieved is ALWAYS reachable (the
    // per-scene overlay only needs to go DOWN, and the floor is ~0.01 ≈ -34 dB of room), so
    // this can never clamp the way a fixed sim-model ceiling did on real hardware.
    const asIsScenes: number[] = [];
    for (const sceneSlot of SCENES_410) {
      asIsScenes.push(await measure410({ scene: sceneSlot }));
    }
    const sceneTarget410 = Math.min(...asIsScenes) - 2.0;

    const scenesRun = (await invoke(
      page,
      "level_scenes_apply_batched",
      {
        slot: FRIEDMAN.slot,
        jobs: SCENES_410.map((sceneSlot) => ({
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
    )) as BatchLevelRun<LevelResult>;
    const scenes = scenesRun.rows;
    // Row identity comes off `scene_slot`, not array index (a mid-batch failure shortens
    // this array).
    expect(
      scenes.map((r) => r.scene_slot).sort((a, b) => Number(a) - Number(b)),
      "every requested scene must come back (no silent mid-batch drop)",
    ).toEqual(SCENES_410);
    for (const r of scenes) {
      const id = String(r.scene_slot);
      expect(r.clamped, `scene ${id} must reach target, not clamp`).toBe(false);
      expect(r.saved, `scene ${id} must level and save`).toBe(true);
      // danger.md's batched-save revert class ("a batched scene-leveling save can revert the
      // ONE scene it just leveled, if that scene is also `restore_scene`") is DETECTED, not
      // silent — 410's `lastLoadedScene` is 1, so scene 1 is exactly the row that class fires
      // on. The run publishes its own verdict here; a spec that re-measures without reading it
      // ignores the run's own warning and reports the revert as an unexplained level miss.
      // Every scene here saved, so `null` (an unverifiable re-read) is not a pass either.
      expect(
        r.persist_mismatch,
        `scene ${id} must persist what it solved (danger.md's batched-save revert class)`,
      ).toBe(false);
    }

    // ── The strict gate: re-measure base + every scene in SCENES_410 from the SAVED state.
    // Scene 0's own row is this arc's see-saw guard — with lanes in the correct order (base →
    // footswitches → scenes) nothing downstream still touches the shared base-ON TubeScreamer,
    // so a future ordering regression would show up here as a scene 0 miss.
    // The footswitch row above is judged by its OWN solve result, not a second
    // strict re-measure — this arc caps its ffmpeg-validated rows to keep its cost down. It
    // emits 1 + SCENES_410.length; STRICT_VALIDATE_ROWS at the top of this file counts them.
    // The remote twin re-measures scene 0 only (file header).
    const remote = await isRemote(page);
    const sceneRow = (slot: number): LevelResult | undefined =>
      scenes.find((r) => r.scene_slot === slot);
    const heard: Record<string, number> = {};
    if (!remote) {
      heard.base = await measure410({
        validate: {
          targetLufs: baseTargetUsed,
          clamped: base.clamped,
          persistMismatch: base.persist_mismatch,
        },
      });
    }
    for (const scene of SCENES_410) {
      if (remote && scene !== 0) continue;
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

    // ── Idempotency, ON THE SAME SAVED STATE this arc just wrote (was its own test, T2) ──
    // The SKIP-LEGITIMACY guard: a `saved:false` footswitch result above is legitimate ONLY
    // when the stored value already measured within tolerance of target — never a silent
    // "did nothing" masquerading as success.
    for (const r of fs410) {
      if (!r.saved) {
        expect(
          Math.abs(r.predicted_lufs - fsTargetUsed410),
          `switch ${String(r.switch)} skipped its save, legitimate only when the stored \
value already measured within tolerance of target`,
        ).toBeLessThanOrEqual(ACCEPT_TOL_LU);
      }
    }
    // The base skip-branch: a re-run at the same target solves the value already saved, so
    // it must write nothing (`level_unchanged`). The footswitch skip-branch is not re-run
    // here: the same skip rule is pinned offline (`bake_path_footswitch_rerun_skips_the_
    // persist_when_already_at_target`), and the device adds only capture noise to it.
    const baseRerun = (await invoke(
      page,
      "level_preset",
      { job: baseLevelJob(FRIEDMAN.slot, baseTargetUsed) },
      T,
    )) as LevelResult;
    expect(
      baseRerun.clamped,
      "base re-run must reach target unclamped (a real skip, not a clamp)",
    ).toBe(false);
    expect(
      baseRerun.saved,
      "base re-run solved the same value the arc already saved → must skip the write (level_unchanged)",
    ).toBe(false);
    expect(
      baseRerun.base_boost,
      "the see-saw guard holds on re-run too: still TRADE, never BOOST",
    ).toBeNull();

    await expectReampBalanced(page, reampBase);
  });
});

// COVERAGE row 36 (online half: 405's base BOOST, then its footswitch rows after the save)
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

  // TWO of 405's four drive pedals — kept local to this file rather than imported, since
  // cross-spec imports of test fixtures aren't this codebase's convention (each spec owns its
  // own job literals). Identity only: NOT a fixture-derived magnitude (file header's
  // SELF-CALIBRATE note bans that class of online target). Each row's `targetLufs` is self-calibrated at
  // runtime inside the test, the same way T1′ derives its own footswitch target — see the
  // probe loop below.
  //
  // WHY TWO AND NOT FOUR (the ratified ≤25 min online leveling budget). Every device capture
  // costs ~22 s and a four-row journey spent ~45 of them, putting this file at 27.7 min. The
  // two rows kept are the two that carry DISTINCT classes:
  //   • switch 5 `ACD_Plumes.level`  — a `level`-named handle, bypassed in base.
  //   • switch 8 `ACD_Rat.volume`    — a `volume`-named handle, and the one pedal this fixture
  //     leaves ON in base (`bypass:false` + `isActive:true`), so it is also the row that
  //     exercises the base-ON interaction the run-order fix exists for.
  // Switches 6 (`ACD_BluesDriver.level`) and 7 (`ACD_ObsessiveDrive.volume`) are the same two
  // classes over again and are dropped from the ONLINE journey only. They stay covered where
  // the coverage is free: gate E5
  // (`e2e_server_tests.rs`) proves all four converge after a base boost, and — the reason
  // switch 7 mattered — `fixture_gates` now pins all four pedals' parameter NAMES against the
  // names the hardware exposes, which is a stronger guard than one online row was.
  const SWITCH_SPECS_405 = [
    {
      switch: 5,
      levGroupId: "G1",
      levNodeId: "ACD_Plumes",
      levParameterId: "level",
    },
    {
      switch: 8,
      levGroupId: "G1",
      levNodeId: "ACD_Rat",
      levParameterId: "volume",
    },
  ];
  const BASE_TARGET_405 = -23; // Rhythm (profiles.rs::default_targets)

  test("first-run UI journey: base BOOST via the wizard, 2 footswitch rows, strict re-measure", async ({
    page,
  }) => {
    test.skip(!(await isOnline(page)), "online-only: needs real audio");
    // Base UI drive (3 conns/1 engage via the wizard), a fallback calibration only when the
    // seeded fixture does not boost (up to ~4 engages + a second UI drive), a 2-switch
    // footswitch batch + 3 strict ffmpeg re-measures (not on the remote twin).
    test.setTimeout(1_800_000);
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    // Verify by IDENTITY (the Twin): a discovery that answered with another preset's graph
    // must fail loudly here. Its value is the fader's baseline for the boost assertions below.
    const blocksPre = (await invoke(
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
    const twinCandidatePre = findTwin(blocksPre);
    expect(
      twinCandidatePre,
      `the Twin's outputLevel candidate must be discoverable before the run — \
list_level_blocks(${String(PRESET24.slot)}) answered with \
[${blocksPre.map((b) => `${b.node_id}.${b.parameter_id}`).join(", ")}], which is another \
preset's graph if the Twin is absent (a cross-slot load that did not take)`,
    ).toBeDefined();
    if (!twinCandidatePre) {
      throw new Error(
        "the Twin's outputLevel candidate must be discoverable before the run",
      );
    }

    // The UI journey itself: the flow end to end, proving the base boost's Summary
    // sentence renders in the real app, not just on the wire. `Rhythm` = -23 LUFS
    // (profiles.rs::default_targets), matching BASE_TARGET_405. Look B states the boost in
    // words only, and only once it was applied; the row's `data-base-boost` hook says
    // whether the run applied it or only planned it (`summaryModel.ts`).
    const boostRow = page.locator(
      `[data-summary-row="p${String(PRESET24.slot)}"][data-base-boost]`,
    );
    const runAndReadBoost = async (): Promise<boolean> => {
      await runBaseLevel(page, [{ preset: PRESET24, label: "Rhythm" }]);
      const shown = await boostRow
        .waitFor({ timeout: 5_000 })
        .then(() => true)
        .catch(() => false);
      if (!shown) return false;
      // A boost the run planned but did not apply is a failure of its own, not a reason to
      // calibrate and run again.
      expect(
        await boostRow.getAttribute("data-base-boost"),
        "the base boost must be APPLIED, not only planned",
      ).toBe("applied");
      await expect(
        boostRow.getByText(
          "Turned the preset up as far as it goes, then raised the amp's output to reach Rhythm.",
        ),
      ).toBeVisible();
      return true;
    };

    // OPTIMISTIC FIRST on the unit (the T1′ base-clamp fallback's pattern): the seed
    // re-imports 405 pristine before every run, and its authored pair (presetLevel 0.27,
    // Twin 0.28, the Rat base-ON) is meant to put G = target - as_is past P_up (headroom to
    // presetLevel 1.0), so the run boosts with no setup. The as-is read, the dry preview and
    // the fader calibration (file header's SELF-CALIBRATE note: a sim magnitude is not device
    // truth) run only when the device proves that wrong — the fallback below. The REMOTE twin
    // is deterministic and its pristine 405 does NOT boost (G <= P_up on every twin run so
    // far), so it goes straight to the calibration: trying first would cost it a second
    // wizard drive every run.
    const remote = await isRemote(page);
    let fCalibrated = twinCandidatePre.value;
    // The minimum excess of G over P_up the boost run saw — known only when the fallback
    // measured it; the optimistic path proves BOOST by the disclosure sentence instead.
    let guaranteedExcessDb: number | null = null;
    if (remote || !(await runAndReadBoost())) {
      // Self-calibration: guarantee the BOOST precondition directly — `G > P_up` — by
      // reading the pedals-off ceiling and lowering the Twin's fader (never `presetLevel`)
      // via `level_preset`'s BLOCK-KNOB arm, the only seam here that can write+persist a
      // block parameter (a closed-loop TARGET solve, so the change is a raw LUFS target).
      //
      // Raw pedals-off reading at the CURRENT presetLevel/fader. `e2e_measure_sound`'s base
      // branch isolates every footswitch-owned block off — the SAME isolation
      // `level_preset`'s own base arm uses — so this raw reading and the `constant_c` below
      // are commensurable.
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
      // A DRY preview (save:false): never writes, just reports the SOLVED `constant_c` — the
      // pedals-off ceiling extrapolated to presetLevel=1, which is presetLevel-INDEPENDENT.
      const preview = (await invoke(
        page,
        "level_preset",
        {
          job: { ...baseLevelJob(PRESET24.slot, BASE_TARGET_405), save: false },
        },
        T,
      )) as LevelResult;
      // `P_up` is INVARIANT under a fader move — both `constant_c` and `asIsRaw` shift by the
      // same d dB — so it still describes the state after calibration below.
      const pUp = preview.constant_c - asIsRaw;
      const BOOST_MARGIN_DB = 2.0; // slack over the bare G > P_up boundary
      const twinNow = findTwin(
        (await invoke(
          page,
          "list_level_blocks",
          { slot: PRESET24.slot },
          T,
        )) as LevelBlock[],
      );
      if (!twinNow) throw new Error("the Twin vanished after the first run");
      fCalibrated = twinNow.value;
      guaranteedExcessDb = BASE_TARGET_405 - asIsRaw - pUp;
      if (guaranteedExcessDb <= 0) {
        // Aim the closed loop at the raw LUFS value that makes the new G exceed P_up by the
        // margin — commensurable with `asIsRaw`'s own isolated measurement basis.
        const calibration = (await invoke(
          page,
          "level_preset",
          {
            job: {
              slot: PRESET24.slot,
              target_lufs: BASE_TARGET_405 - pUp - BOOST_MARGIN_DB,
              save: true,
              topology_id: "guitar-humbucker",
              calibration_lufs: null,
              stimulus_path: null,
              profile_id: null,
              block_group_id: twinNow.group_id,
              block_node_id: twinNow.node_id,
              block_parameter_id: twinNow.parameter_id,
              block_value: twinNow.value,
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
        guaranteedExcessDb = BOOST_MARGIN_DB;
      }
      expect(
        await runAndReadBoost(),
        "with G > P_up guaranteed, the wizard run must disclose the base boost",
      ).toBe(true);
    }

    // Assert the PERSISTED pair directly rather than re-invoking `level_preset` a second
    // time: a same-target re-run's OWN result is unreliable evidence of what the UI run above
    // actually solved and saved, on every realistic branch — the idempotency skip hard-codes
    // `base_boost: null` (the solve is skipped entirely), a hairline re-measure can flip
    // `clamped: true`, and a within-tolerance re-plan that doesn't re-enter BOOST also reports
    // `base_boost: null` (`leveller::level_preset_impl`'s routing). `list_level_blocks` reads
    // the SAVED state instead — same seam as `the_footswitch_rows_still_converge_after_a_base_boost`'s persisted-Twin
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
    // A derived BAND off our OWN baseline, never a hard-coded model number (the old
    // `toBeCloseTo(0.498, 1)` was the offline sim model's own number, not a real-HW invariant
    // — file header's SELF-CALIBRATE note). Boost maxes `presetLevel` FIRST (the disclosure
    // sentence above just confirmed it fired), so the solved fader must be strictly louder
    // than its baseline. When the fallback measured the excess of G over P_up, the fader's
    // own share of closing it is at least that excess (inflated further by the drive pedals'
    // base-ON contribution), so it must also clear `(guaranteedExcessDb - 1)` dB over the
    // baseline (1 dB slack for solve noise/secant residual).
    expect(
      twinFader?.value ?? Number.NaN,
      "the boosted fader must be LOUDER than its baseline (a real boost, not a no-op)",
    ).toBeGreaterThan(fCalibrated);
    if (guaranteedExcessDb !== null) {
      expect(
        twinFader?.value ?? Number.NaN,
        `the boosted fader must close at least the ~${guaranteedExcessDb.toFixed(2)} dB (minus \
1 dB solve-noise slack) deficit off the calibrated baseline ${fCalibrated.toFixed(4)}`,
      ).toBeGreaterThanOrEqual(
        fCalibrated * 10 ** ((guaranteedExcessDb - 1) / 20),
      );
    }

    // Self-calibration (file header's SELF-CALIBRATE note): taken AFTER the base boost above
    // (which scales every footswitch capture on this slot), with the SAME `lev` coordinates the
    // leveling lane below is fed. One probe per switch — these cost device time.
    const SWITCH_JOBS_405: ((typeof SWITCH_SPECS_405)[number] & {
      targetLufs: number;
    })[] = [];
    for (const spec of SWITCH_SPECS_405) {
      const { target } = await measureAsIsTarget(page, {
        slot: PRESET24.slot,
        footswitch: spec.switch,
        lev: {
          groupId: spec.levGroupId,
          nodeId: spec.levNodeId,
          parameterId: spec.levParameterId,
        },
      });
      SWITCH_JOBS_405.push({ ...spec, targetLufs: target });
    }

    // 2 footswitch rows (base MUST run first — the pl-context trap 405's own fixture
    // ordering pins, e2e/fixtures/COVERAGE.md row 36) — raw invoke, the channel-streaming
    // seam (.claude/rules/e2e.md).
    const fsRun = (await invoke(
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
    )) as BatchLevelRun<FootswitchLevelResult>;
    const fs = fsRun.rows;
    expectEverySwitch(fs, SWITCH_JOBS_405);
    assertFsSolved(fs, (r) => {
      const job = SWITCH_JOBS_405.find((j) => j.switch === r.switch);
      if (!job) throw new Error(`unexpected switch ${String(r.switch)}`);
      return job.targetLufs;
    });

    // 3 strict ffmpeg re-measures: base + both leveled pedals, from the saved state. The
    // remote twin re-measures the Rat only (file header).
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

    if (!remote) {
      const heardBase = await measure405();
      expect(
        Math.abs(heardBase - BASE_TARGET_405),
        `base re-measures at ${heardBase.toFixed(2)} LUFS from the saved state`,
      ).toBeLessThanOrEqual(DELTA);
    }
    for (const job of SWITCH_JOBS_405) {
      // The Rat (switch 8) is the base-ON pedal the run-order fix exists for.
      if (remote && job.switch !== 8) continue;
      const heard = await measure405(job);
      expect(
        Math.abs(heard - job.targetLufs),
        `switch ${String(job.switch)} re-measures at ${heard.toFixed(2)} LUFS from the saved state`,
      ).toBeLessThanOrEqual(DELTA_FS);
    }

    await expectReampBalanced(page, reampBase);
  });
});
