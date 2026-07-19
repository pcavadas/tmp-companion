import type { Page } from "@playwright/test";
import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  armCaptureFault,
  clearScenario,
  ensureScenario,
  expectReampBalanced,
  isOnline,
  reampCounters,
  reampOff,
} from "../fixtures/scenario";

// First-session DEFAULTS + the physics-outcome gates (the real user's complaint set), driven
// through the REAL UI so the SummaryBody banner copy is asserted verbatim. Outcomes come from
// the offline physics-faithful capture model (sim_device.rs) + e2e/fixtures/scenario-loudness.json
// (the hand-authored C table), so these are OFFLINE-ONLY: online the real device's ceilings are
// whatever the seeded presets actually give, not the authored sidecar values. Online still
// exercises 403's seed + net-zero teardown via scenario_spec / SCENARIO. Fixture "slot" numbers
// are 0-based LIST INDICES (the unit shows userSlot = index + 1: 403 → 404).
//
// HARNESS LIMIT (asserted at the command level instead — see e2e_server_tests.rs
// `level_defaults_403_scenes_solve_and_offbranch` + `..._base_clamps_and_footswitch_is_offbranch`):
// per-SCENE and per-FOOTSWITCH results STREAM via a `Channel` the offline HTTP bridge no-ops, so
// the UI can't render those outcomes offline (doctor/level specs work around the same limit).
// The UI here asserts the BASE-leveling outcomes (level_preset returns directly, no Channel): the
// mass-clamp (a), the re-level-clamped loop (b), and an off-branch + its remediation banner (c).
// The (c) off-branch is a capture-FAULT (silent capture), a faithful proxy for the routing
// off-branch ONLY while both share the clamp_reason "no signal on USB 1/2" → the same banner; if
// the code ever splits transient-silent vs structurally-dead into different banners, revisit (c).

/** Open the Level tab on a fresh page and wait for the connected header. Dismisses the
 *  one-shot startup backup disclaimer when present (localStorage-gated — only the first load). */
async function openLevel(page: Page): Promise<void> {
  await page.goto("/");
  const disclaimer = page.getByRole("button", { name: /backed up/i });
  if (await disclaimer.isVisible().catch(() => false)) await disclaimer.click();
  await expect(page.getByText(/connected · \d+\.\d+/)).toBeVisible({
    timeout: 20_000,
  });
}

/** Tick a preset's whole-preset checkbox (selecting Base + every scene + footswitch). */
async function selectWholePreset(page: Page, name: string): Promise<void> {
  const filter = page.getByPlaceholder(/Filter by name or slot/i);
  await filter.fill(name);
  const caret = page.getByTitle(/Show Base/).first(); // reveal children so the tick sweeps them in
  if (await caret.isVisible().catch(() => false)) await caret.click();
  await page.getByTitle("Select preset to level").first().click();
  await filter.fill("");
}

test.describe("Level — first-run defaults + physics outcomes (offline, sidecar-authored)", () => {
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  // (a) Mass-clamp: whole-preset 403 at the UNTOUCHED default target (Rhythm −26) → its Base
  // CLAMPS at its ceiling (−28). This is the first-session reality the user hit — the shipped
  // defaults are quieter than the preset's max, so it can't reach them. Faithful on the
  // presetLevel path; the scene/footswitch outcomes are the command-level gates (see header).
  test("whole-preset defaults → base clamps at its ceiling", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline-only: the clamp ceiling is sidecar-authored",
    );
    await ensureScenario(page);
    const reampBase = await reampCounters(page);
    await openLevel(page);
    await selectWholePreset(page, "E2E Realistic");

    await page.getByRole("button", { name: /Level 1 preset/ }).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click(); // the inline commit gate
    await page.getByRole("button", { name: /Level \d+ sound/ }).click(); // untouched Rhythm (−26)
    await expect(
      page.getByRole("button", { name: /^(Done|Accept)$/ }),
    ).toBeVisible({
      timeout: 240_000,
    });

    // The headroom clamp: its remediation banner + the exact clamped ceiling on the Base row.
    await expect(page.getByText(/Clamped .* already maxed/)).toBeVisible();
    await expect(page.getByText(/1 clamped/)).toBeVisible();
    await expect(page.getByText(/clamped · [−-]28\.\d/)).toBeVisible();

    await expectReampBalanced(page, reampBase);
  });

  // (b) Re-level-clamped loop: a sound clamped at a LOUDER target resolves when re-leveled at a
  // quieter one. 401 (ceiling −23) clamps at Lead (−22) and resolves at Crunch (−24). Summary →
  // "Re-level clamped…" → lower target → run 2 → the row is done (no clamp banner remains).
  test("re-level-clamped: clamped at Lead resolves at Crunch", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline-only: authored ceiling via the sidecar",
    );
    await ensureScenario(page);
    const reampBase = await reampCounters(page);
    await openLevel(page);
    await selectWholePreset(page, SCENARIO[1].name); // E2E Target 1 (plain → Base only)

    await page.getByRole("button", { name: /Level 1 preset/ }).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();
    await page.locator(`[data-pick="target:${SCENARIO[1].name}"]`).click(); // pick the LOUD target
    await page.getByText(/Lead/).click();
    await page.getByRole("button", { name: /Level 1 sound/ }).click();
    await expect(page.getByRole("button", { name: "Accept" })).toBeVisible({
      timeout: 240_000,
    });
    await expect(page.getByText(/Clamped .* already maxed/)).toBeVisible();

    await page.getByRole("button", { name: /Re-level clamped/ }).click();
    await page.locator(`[data-pick="target:${SCENARIO[1].name}"]`).click(); // QUIETER target
    await page.getByText(/Crunch/).click();
    await page.getByRole("button", { name: /Level 1 sound/ }).click();
    await expect(page.getByRole("button", { name: "Done" })).toBeVisible({
      timeout: 240_000,
    });
    await expect(page.getByText(/Clamped .* already maxed/)).toHaveCount(0);

    await expectReampBalanced(page, reampBase);
  });

  // (c) Mid-run failure + no-signal banner: a 2-preset run with /sim/fault silencing 402's first
  // capture → 402 goes OFF-BRANCH (no signal) with its "needs routing" remediation banner, while
  // its sibling 401 still levels to the target. Proves skip-and-continue AND the offbranch banner
  // + the displayed solvable LUFS through the (base-leveling, Channel-free) UI path.
  test("mid-run capture fault: one item off-branch (routing banner), the sibling levels", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline-only: /sim/fault is a SimDevice injection",
    );
    await ensureScenario(page);
    const reampBase = await reampCounters(page);
    await openLevel(page);

    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    for (const p of [SCENARIO[1], SCENARIO[2]]) {
      await filter.fill(p.name);
      await page.getByTitle("Select preset to level").first().click();
    }
    await filter.fill("");

    await armCaptureFault(page, SCENARIO[2].slot); // silence 402's next capture (one-shot)

    await page.getByRole("button", { name: /Level 2 preset/ }).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();
    await page.getByRole("button", { name: /Level \d+ sound/ }).click(); // defaults; 401 solves, 402 faults
    await expect(
      page.getByRole("button", { name: /^(Done|Accept)$/ }),
    ).toBeVisible({
      timeout: 240_000,
    });

    // 402 off-branch: the sub-tally, the row status, and the routing remediation banner.
    await expect(page.getByText(/1 silent/)).toBeVisible();
    await expect(page.getByText("not on USB 1/2")).toBeVisible();
    await expect(page.getByText("Needs routing on the unit")).toBeVisible();
    // 401 still leveled — its displayed final LUFS near the default target (−26).
    await expect(page.getByText(/[−-]2[567]\.\d LUFS/).first()).toBeVisible();

    await expectReampBalanced(page, reampBase);
  });

  // (d) Reachable-common-target fallback (QUIET-preset clamp class): a 2-preset run where 403's
  // Base clamps because its ceiling (−28) sits below every shipped default target, while 401
  // (ceiling −23) levels fine at −26. The Summary names the measured ceiling and offers
  // "Re-level to a reachable target", which derives min(ceiling) − headroom = −29 from the
  // ALREADY-measured ceilings (zero re-capture) and re-levels EVERY base to it → the once-clamped
  // 403 lands done and both presets sit at one common loudness (no on-stage jump). Base-only
  // (scene outcomes stream via the Channel the offline bridge no-ops); the offset-space derivation
  // is Rust-unit-gated (`common_reachable_target_is_min_of_offset_adjusted_ceilings`).
  test("reachable-common-target fallback: clamped base re-levels to a reachable common target", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline-only: the ceilings are sidecar-authored",
    );
    await ensureScenario(page);
    const reampBase = await reampCounters(page);
    await openLevel(page);

    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    for (const p of [SCENARIO[3], SCENARIO[1]]) {
      // 403 (ceiling −28), 401 (ceiling −23)
      await filter.fill(p.name);
      await page.getByTitle("Select preset to level").first().click();
    }
    await filter.fill("");

    await page.getByRole("button", { name: /Level 2 preset/ }).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();
    await page.getByRole("button", { name: /Level \d+ sound/ }).click(); // default Rhythm −26
    await expect(
      page.getByRole("button", { name: /^(Done|Accept)$/ }),
    ).toBeVisible({ timeout: 240_000 });

    // 403's Base clamped at its ceiling — the banner NAMES the measured ceiling.
    await expect(page.getByText(/Clamped .* already maxed/)).toBeVisible();
    await expect(page.getByText(/ceiling [−-]28\.\d LUFS/)).toBeVisible();

    // The fallback re-levels every measured sound to the reachable common target (−29).
    await page
      .getByRole("button", { name: /Re-level to a reachable target/ })
      .click();
    // Wait for the re-run to actually START (the RunBody replaces the summary) so the
    // asserts below don't race the stale pre-fallback summary, then for it to FINISH
    // (auto-advance back to a summary with Done/Accept).
    await expect(page.getByText(/Step \d+ of \d+/)).toBeVisible({
      timeout: 30_000,
    });
    await expect(
      page.getByRole("button", { name: /^(Done|Accept)$/ }),
    ).toBeVisible({ timeout: 240_000 });
    // No clamp remains, and both bases landed near the derived common target (−29).
    await expect(page.getByText(/Clamped .* already maxed/)).toHaveCount(0);
    await expect(page.getByText(/[−-]29\.\d LUFS/).first()).toBeVisible();

    await expectReampBalanced(page, reampBase);
  });
});
