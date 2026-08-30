import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  armCaptureFault,
  clearScenario,
  ensureScenario,
  expectReampBalanced,
  isOnline,
  openLevel,
  pickBaseTarget,
  reampCounters,
  reampOff,
  selectBaseOnly,
} from "../fixtures/scenario";

// First-session DEFAULTS + the physics-outcome gates (the real user's complaint set), driven
// through the REAL UI so the SummaryBody banner copy is asserted verbatim. Outcomes come from
// the offline physics-faithful capture model (sim_device.rs) + e2e/fixtures/scenario-loudness.json
// (the hand-authored C table), so these are OFFLINE-ONLY: online the real device's ceilings are
// whatever the seeded presets actually give, not the authored sidecar values. Online still
// exercises the seed + net-zero teardown via scenario_spec / SCENARIO. Fixture "slot" numbers
// are 0-based LIST INDICES (the unit shows userSlot = index + 1: 403 → 404).
//
// P4-B fixture rebuild: EVERY scenario preset now carries footswitches and/or scenes (none is
// "plain" Base-only any more — see e2e/fixtures/COVERAGE.md). Base C ceilings: 400 (E2E Rig)
// -15, 401 (E2E Pedalboard) -20, 402 (E2E Edge) -13, 403 (E2E Parallel) -20. So 401/403's base
// CLAMPS only at the LOUD shipped default (Lead, -19) — -23/-21 both solve — matching
// `e2e_server_tests.rs`'s `level_defaults_base_clamps_and_the_split_lane_footswitch_is_offbranch`
// ("Base at Lead (-19) on 403 -> CLAMP at its ceiling (-20)"). The tests below therefore select
// ONLY the Base row (never the whole preset) for the clamp cases: sweeping in a preset's
// footswitches too would (a) collide on the shared `data-pick="target:NAME"` locator every
// selected row of one preset carries, and (b) risks shifting the terminal Done-vs-Accept summary
// text for reasons unrelated to the base clamp under test.
//
// HARNESS LIMIT: per-SCENE/per-FOOTSWITCH outcomes hit the Channel-streaming seam offline
// (asserted at the command level instead — see e2e_server_tests.rs
// `level_defaults_403_scenes_solve_and_offbranch` + `..._base_clamps_and_footswitch_is_offbranch`;
// rationale in .claude/rules/e2e.md's "The Channel-streaming seam"). The UI here asserts only
// the BASE-leveling outcomes (level_preset returns directly, no Channel): the mass-clamp (a),
// the re-level-clamped loop (b), and a mid-run off-branch capture fault (c).

// openLevel / selectBaseOnly / pickBaseTarget now live in ../fixtures/scenario.ts (shared
// with level-setup.spec.ts, pedal-fiasco.spec.ts and level.spec.ts).

test.describe("Level — first-run defaults + physics outcomes (offline, sidecar-authored)", () => {
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  // COVERAGE row 2 — base solve that CLAMPS.
  // (a) Mass-clamp: 401's Base at the LOUD shipped default (Lead, -19) clamps at its ~-20
  // ceiling (PR2 re-baseline math lives in scenario-loudness.json's "401" entry). This is the
  // first-session reality the user hit — a shipped default can sit above a preset's max.
  test("base at the loud default clamps at its ceiling", async ({ page }) => {
    test.skip(
      await isOnline(page),
      "offline-only: the clamp ceiling is sidecar-authored",
    );
    await ensureScenario(page);
    const reampBase = await reampCounters(page);
    await openLevel(page);
    await selectBaseOnly(page, SCENARIO[1].name); // E2E Pedalboard

    await page.getByRole("button", { name: /Level 1 preset/ }).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click(); // the inline commit gate
    await pickBaseTarget(page, SCENARIO[1].name, "Lead"); // the loud default — this is what clamps
    await page.getByRole("button", { name: /Level 1 sound/ }).click();
    await expect(page.getByRole("button", { name: "Accept" })).toBeVisible({
      timeout: 240_000,
    });

    // The headroom clamp: its remediation banner + the exact clamped ceiling on the Base row.
    await expect(page.getByText(/Clamped .* already maxed/)).toBeVisible();
    await expect(page.getByText(/1 clamped/)).toBeVisible();
    await expect(page.getByText(/clamped · [−-]20\.\d/)).toBeVisible();
    // NEW COVERAGE (PR #144's ClampKind taxonomy): a plain headroom clamp on a Base row
    // (no off-branch clamp_reason, no wet floor) reports `ClampKind::SceneCeiling` — the
    // ONE ClampKind whose message is UI-observable offline through a real run: Base
    // levels via `level_preset`'s direct return, not the per-scene/per-footswitch Channel
    // the offline bridge no-ops (`.claude/rules/e2e.md`). Copied VERBATIM from
    // `headroom_trade::ClampKind::message()` / `CLAMP_MESSAGES.scene_ceiling` — never
    // re-word it here if the backend's wording changes; update this string instead.
    await expect(
      page.getByText(
        "this sound can’t reach the target because its level control is already maxed out",
      ),
    ).toBeVisible();

    await expectReampBalanced(page, reampBase);
  });

  // (b) Re-level-clamped loop: a sound clamped at a LOUDER target resolves when re-leveled at a
  // quieter one. 401 (ceiling ~-20) clamps at Lead (-19) and resolves at Crunch (-21). Summary →
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
    await selectBaseOnly(page, SCENARIO[1].name); // E2E Pedalboard, Base only

    await page.getByRole("button", { name: /Level 1 preset/ }).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();
    await pickBaseTarget(page, SCENARIO[1].name, "Lead"); // pick the LOUD target
    await page.getByRole("button", { name: /Level 1 sound/ }).click();
    await expect(page.getByRole("button", { name: "Accept" })).toBeVisible({
      timeout: 240_000,
    });
    await expect(page.getByText(/Clamped .* already maxed/)).toBeVisible();

    await page.getByRole("button", { name: /Re-level clamped/ }).click();
    await pickBaseTarget(page, SCENARIO[1].name, "Crunch"); // QUIETER target
    await page.getByRole("button", { name: /Level 1 sound/ }).click();
    await expect(page.getByRole("button", { name: "Done" })).toBeVisible({
      timeout: 240_000,
    });
    await expect(page.getByText(/Clamped .* already maxed/)).toHaveCount(0);

    await expectReampBalanced(page, reampBase);
  });

  // (c) Mid-run failure + no-signal banner: a 2-preset run with /sim/fault silencing 402's first
  // capture → 402 goes OFF-BRANCH (no signal) with its "needs routing" remediation banner, while
  // its sibling 401 still levels to the target. Whole-preset selection here (both presets now
  // carry footswitches too) is fine — the test asserts base-leveling outcomes only, and no
  // data-pick target is picked, so the collision risk from mixed row types never applies.
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
    // 401 still leveled — its displayed final LUFS near the default target (−23).
    await expect(page.getByText(/[−-]2[234]\.\d LUFS/).first()).toBeVisible();

    await expectReampBalanced(page, reampBase);
  });

  // (d) Reachable-common-target fallback (QUIET-preset clamp class): a 2-preset run where 403's
  // Base clamps at Lead (-19, ceiling ~-20) while 400 (ceiling ~-15) reaches Lead fine — both
  // targeted explicitly since the shipped default (Rhythm, -23) no longer clamps EITHER preset
  // with the rebuilt fixtures (401 and 403 share the same base ceiling now, so a Rhythm-default
  // pairing can no longer produce "one clamps, one doesn't"). The Summary names the measured
  // ceiling and offers "Re-level to a reachable target", which derives
  // min(measured ceilings) − headroom from the ALREADY-measured ceilings (zero re-capture) and
  // re-levels every base to it. Base-only selection on both presets (scene outcomes stream via
  // the Channel the offline bridge no-ops); the offset-space derivation is Rust-unit-gated
  // (`common_reachable_target_returns_min_ceiling_minus_headroom`).
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

    await selectBaseOnly(page, SCENARIO[0].name); // E2E Rig, ceiling ~-15 (solves at Lead)
    await selectBaseOnly(page, SCENARIO[3].name); // E2E Parallel, ceiling ~-20 (clamps at Lead)

    await page.getByRole("button", { name: /Level 2 preset/ }).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();
    await pickBaseTarget(page, SCENARIO[0].name, "Lead");
    await pickBaseTarget(page, SCENARIO[3].name, "Lead");
    await page.getByRole("button", { name: /Level \d+ sound/ }).click();
    await expect(
      page.getByRole("button", { name: /^(Done|Accept)$/ }),
    ).toBeVisible({ timeout: 240_000 });

    // 403's Base clamped at its ceiling — the banner NAMES the measured ceiling.
    await expect(page.getByText(/Clamped .* already maxed/)).toBeVisible();
    await expect(page.getByText(/Ceiling: [−-]20\.\d LUFS/)).toBeVisible();

    // The fallback re-levels every measured sound to the derived reachable common target
    // (min of the two MEASURED ceilings minus 1 LU headroom — a loose band, not a literal
    // number, since the exact figure is a run-to-run measured value, not sidecar-authored).
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
    // No clamp remains, and both bases landed near the derived common target.
    await expect(page.getByText(/Clamped .* already maxed/)).toHaveCount(0);
    await expect(page.getByText(/[−-]2[01]\.\d LUFS/).first()).toBeVisible();

    await expectReampBalanced(page, reampBase);
  });
});
