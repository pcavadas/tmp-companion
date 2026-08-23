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
} from "../fixtures/scenario";

// COVERAGE row 37 — scene wipe / bake / conformance oracle (the online half; see
// level.spec.ts for the offline enumeration half).
// STRICT HARNESS (online-only): leveling must be judged by what the player HEARS,
// not by the run reporting success. Level the corruption-class preset (E2E Hiwatt
// 3S — 4 scenes incl. a real "Base Scene" overlay + 4 block-acting footswitches)
// through the real app commands with save, then RE-MEASURE every sound's actual
// audio output from the SAVED state (`e2e_measure_sound` — the production capture
// path with the leveling lanes' exact contexts) and assert each lands on the
// hard-coded target within a small delta. Offline has no audio path (the fake
// capture is a stimulus passthrough), so this file is an online-only oracle.

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

test.describe("Level — strict output harness (Hiwatt corruption-class preset)", () => {
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  test("base + every scene + every footswitch re-measure at target after save", async ({
    page,
  }) => {
    test.skip(!(await isOnline(page)), "online-only: needs real audio");
    // 3 leveling lanes + 9 re-measures of ~6 s captures. Checked against the freshness
    // barrier's own worst case (`ensure_fresh_load`, danger.md): a single wait is bounded by
    // `COMMIT_WINDOW_SECS` (150 s, leveller.rs), and this run makes at most 2 same-slot loads
    // that could race a prior save (the scene lane's prepass, the FS batch's own load) — 300 s
    // of pure barrier stall in the worst case, comfortably inside this 1_800_000 ms budget.
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

    await expectReampBalanced(page, reampBase);
  });
});
