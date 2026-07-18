import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  type Preset,
  clearScenario,
  ensureScenario,
  expectReampBalanced,
  invoke,
  isOnline,
  reampCounters,
  reampOff,
  simEvents,
} from "../fixtures/scenario";

// Consecutive-runs idempotency gate — the PR #74 requirement ("2 consecutive leveling runs
// must produce the same result") that lived only in a session prompt, never as executable
// infrastructure. Split oracles by mode:
//   OFFLINE = events-equality (run 2's device-write sequence must equal run 1's — no drift).
//   ONLINE  = the true skip-branch gate (run 2 makes ZERO new writes for in-tolerance sounds).
// Mode is read from the server (/health) at runtime, NOT process.env — the Playwright process
// does not inherit TMP_E2E_ONLINE (only the server subprocess does), so a describe-level env
// check misfires (offline test runs online, online tests skip). Each test skips on the wrong
// mode instead.

interface LevelResult {
  saved: boolean;
  clamped: boolean;
}
interface FootswitchLevelResult {
  switch: number;
  saved: boolean;
  clamped: boolean;
  predicted_lufs: number;
  clamp_reason: string | null;
}

// A plain preset's base-leveling job (no scenes/footswitches → whole-preset = Base only).
const baseJob = (slot: number, target: number) => ({
  slot,
  target_lufs: target,
  save: true,
  topology_id: "guitar-humbucker",
  calibration_lufs: null,
  stimulus_path: null,
  profile_id: null,
  block_group_id: null,
  block_node_id: null,
  block_parameter_id: null,
  block_value: null,
});

// Real re-amp (measure + verify captures + save) runs well past the invoke helper's 30 s
// default, so every ONLINE leveling invoke gets a long request timeout.
const T = 280_000;

// ───────────────────────────── OFFLINE: events-equality ─────────────────────────────
test.describe("Level re-run — offline events-equality (no drift)", () => {
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });

  // OFFLINE the `level_unchanged` skip branch is STRUCTURALLY UNREACHABLE: it needs the
  // pre-run `presetLevel` from a field-8 preset read that the SimDevice does not model, so
  // `previous_level` is always None and BOTH runs write the full sequence. That is exactly
  // why offline only asserts events-EQUALITY (the write sequence never drifts) — the
  // skip-branch itself is an online-only oracle (below). This is a harness limitation, not
  // a product bug: the fake models device wire ops, not the field-8 read the skip depends on.
  test("two identical base runs emit the same device-write sequence", async ({
    page,
  }) => {
    test.skip(await isOnline(page), "offline-only events-equality oracle");
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    const runBaseLevel = async (preset: Preset) => {
      await page.goto("/");
      const disclaimer = page.getByRole("button", { name: /backed up/i });
      if (await disclaimer.isVisible().catch(() => false))
        await disclaimer.click(); // only shows on the first load (localStorage-gated)
      await expect(page.getByText(/connected · \d+\.\d+/)).toBeVisible({
        timeout: 20_000,
      });
      const filter = page.getByPlaceholder(/Filter by name or slot/i);
      await filter.fill(preset.name);
      await page.getByTitle("Select preset to level").first().click();
      await filter.fill("");
      await page.getByRole("button", { name: /Level 1 preset/ }).click();
      await page.getByText(/I.ve backed up with Pro Control/i).click();
      await page.locator(`[data-pick="target:${preset.name}"]`).click();
      await page.getByText(/Crunch/).click();
      await page.getByRole("button", { name: /Level 1 sound/ }).click();
      await expect(page.getByRole("button", { name: "Done" })).toBeVisible({
        timeout: 240_000,
      });
    };

    // /sim/reset (test fixture) cleared the event log, so run 1's log is the whole prefix.
    await runBaseLevel(SCENARIO[1]);
    const afterRun1 = await simEvents(page);
    expect(afterRun1.length).toBeGreaterThan(0);

    // page.goto resets the UI (selection cleared) but NOT the SimDevice — events accumulate.
    await runBaseLevel(SCENARIO[1]);
    const afterRun2 = await simEvents(page);

    // Run 2's DELTA must byte-equal run 1's whole log: same loads, same solved presetLevel,
    // same save — a re-run that drifted (re-clamped, or wrote a different value) fails here.
    const run2Delta = afterRun2.slice(afterRun1.length);
    expect(run2Delta).toEqual(afterRun1);

    await expectReampBalanced(page, reampBase);
  });
});

// ───────────────────────── ONLINE: the true skip-branch gate ─────────────────────────
test.describe("Level re-run — online skip-branch idempotency", () => {
  // Slots stay seeded across both tests (base uses 401, footswitch uses 400) and are cleared
  // ONCE — clearing between tests would force the next ensureScenario into the flaky
  // in-process re-seed (the 0xe00002c5 open-lockout), mirroring level.spec's teardown shape.
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  // Base lane: run 1 levels + saves; run 2 reads the just-saved presetLevel as
  // `previous_level`, solves within tolerance, and SKIPS (zero new PresetLevel/save). This
  // is the gate the always-`None` regression (Part A) would fail — offline it can't run.
  test("base: run 2 makes zero new writes (level_unchanged skip)", async ({
    page,
  }) => {
    test.skip(!(await isOnline(page)), "online-only: needs the field-8 read");
    test.setTimeout(300_000);
    await ensureScenario(page);
    const reampBase = await reampCounters(page);
    const job = baseJob(SCENARIO[1].slot, -30);

    const r1 = (await invoke(page, "level_preset", { job }, T)) as LevelResult;
    // Non-vacuous: run 1 must actually level + write (else the skip below proves nothing).
    // -30 LUFS is well under a guitar preset's reachable ceiling, so it should not clamp.
    expect(r1.clamped, "run 1 should reach target, not clamp").toBe(false);
    expect(r1.saved, "run 1 levels and saves").toBe(true);

    const r2 = (await invoke(page, "level_preset", { job }, T)) as LevelResult;
    // A real idempotency skip, not a clamp: run 2 must reach target unclamped AND write
    // nothing. Without the unclamped guard a headroom clamp (which also skips the save)
    // would green this test without exercising the level_unchanged branch.
    expect(r2.clamped, "run 2 must reach target unclamped (a real skip)").toBe(
      false,
    );
    expect(
      r2.saved,
      "run 2 solved the same value already saved → must skip the write",
    ).toBe(false);

    await expectReampBalanced(page, reampBase);
  });

  // Footswitch lane: the switch_at_target idempotency fix (this PR). Run 1 levels the
  // switch's engaged state (Assign path — E2E Reference has scenes); run 2 probes the
  // stored valueA, finds it on target, and rewrites nothing. WITHOUT the fix this is RED
  // (the lane re-solved in-tolerance switches every run — the PR #74 deferred gap).
  test("footswitch: run 2 rewrites nothing in-tolerance (switch_at_target)", async ({
    page,
  }) => {
    test.skip(!(await isOnline(page)), "online-only: needs the field-8 read");
    test.setTimeout(300_000);
    await ensureScenario(page);
    const reampBase = await reampCounters(page);
    // E2E Reference switch 1 (GREENBOX) toggles ACD_TubeScreamer; level its `level` param.
    // The switch's block controls only a slice of the chain loudness, so a fixed LUFS target
    // can be out of its reach (→ clamp, nothing to skip). Learn the reachable value with a
    // dry run and target THAT — then run 1 always truly levels and the skip is real.
    const apply = (targetLufs: number, save: boolean) =>
      invoke(
        page,
        "level_footswitches_apply",
        {
          slot: SCENARIO[0].slot,
          jobs: [
            {
              switch: 1,
              levGroupId: "G1",
              levNodeId: "ACD_TubeScreamer",
              levParameterId: "level",
              targetLufs,
            },
          ],
          save,
          topologyId: "guitar-humbucker",
          calibrationLufs: null,
          profileId: null,
          onResult: "__CHANNEL__:1", // Channel arg → no-op sink over the bridge
        },
        T,
      ) as Promise<FootswitchLevelResult[]>;

    // Dry run: measure the switch's achievable engaged loudness (predicted_lufs) with no write.
    const dry = await apply(-22, false);
    const measurable = dry.find((r) => r.clamp_reason == null); // signal on USB, not off-branch
    expect(
      measurable,
      "the switch's engaged state must be measurable",
    ).toBeTruthy();
    const target = measurable?.predicted_lufs ?? -22; // reachable by construction

    const r1 = await apply(target, true);
    // Which switches actually leveled + wrote (so the idempotency claim is non-vacuous).
    const leveled = new Set(
      r1.filter((r) => r.saved && !r.clamped).map((r) => r.switch),
    );
    expect(
      leveled.size,
      "run 1 must level at least one switch at its reachable target",
    ).toBeGreaterThan(0);

    const r2 = await apply(target, true);
    // For EVERY switch run 1 leveled, run 2 must return a measurable (error-free),
    // unclamped result that skips the write — not merely "no result" or a clamp, either
    // of which would vacuously satisfy a bare saved === false check.
    for (const sw of leveled) {
      const r = r2.find((x) => x.switch === sw);
      expect(
        r,
        `switch ${String(sw)} must return a result in run 2`,
      ).toBeTruthy();
      expect(
        r?.clamp_reason,
        `switch ${String(sw)} run 2 must be measurable (no clamp reason)`,
      ).toBeNull();
      expect(
        r?.clamped,
        `switch ${String(sw)} run 2 must reach target unclamped`,
      ).toBe(false);
      expect(
        r?.saved,
        `switch ${String(sw)} was in tolerance → run 2 must skip (zero rewrites)`,
      ).toBe(false);
    }

    await expectReampBalanced(page, reampBase);
  });
});
