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
// through the REAL UI so the SummaryPage banner copy is asserted verbatim. Outcomes come from
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
// footswitches too risks shifting the terminal Done-vs-Accept summary text for reasons
// unrelated to the base clamp under test.
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
    await pickBaseTarget(page, SCENARIO[1].slot, "Lead"); // the loud default — this is what clamps
    await page.getByRole("button", { name: /Start.*1 sound/ }).click();
    await expect(page.getByRole("button", { name: "Accept" })).toBeVisible({
      timeout: 240_000,
    });

    // The headroom clamp: its remediation banner, the row's own clamped status, and the
    // exact clamped ceiling on the Base row.
    await expect(page.getByText(/Clamped .* already maxed/)).toBeVisible();
    await expect(page.getByText("as loud as it goes")).toBeVisible();
    await expect(page.getByText(/[−-]20\.\d/)).toBeVisible();
    // design 1a: every clamped row shows ONE generic message regardless of clamp cause
    // (the old per-`ClampKind` backend wording — trade/floor disclosures included — is
    // gone from the wizard entirely). Copied VERBATIM from `SummaryPage.tsx`'s
    // `PROBLEM.clamped.msg` — never re-word it here if the copy changes; update this
    // string instead.
    await expect(
      page.getByText(
        "The knob is already all the way up. A quieter target would let this one match.",
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
    await pickBaseTarget(page, SCENARIO[1].slot, "Lead"); // pick the LOUD target
    await page.getByRole("button", { name: /Start.*1 sound/ }).click();
    await expect(page.getByRole("button", { name: "Accept" })).toBeVisible({
      timeout: 240_000,
    });
    await expect(page.getByText(/Clamped .* already maxed/)).toBeVisible();

    await page.getByRole("button", { name: /Re-level clamped/ }).click();
    await pickBaseTarget(page, SCENARIO[1].slot, "Crunch"); // QUIETER target
    await page.getByRole("button", { name: /Start.*1 sound/ }).click();
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
    await page.getByRole("button", { name: /Start.*\d+ sound/ }).click(); // defaults; 401 solves, 402 faults
    await expect(
      page.getByRole("button", { name: /^(Done|Accept)$/ }),
    ).toBeVisible({
      timeout: 240_000,
    });

    // 402 off-branch: the row's own status, its specific no-signal diagnosis, and the
    // routing remediation banner (design 1a dropped the old per-category sub-tally count —
    // the row status + banner are the evidence now).
    await expect(page.getByText("can’t hear it")).toBeVisible();
    await expect(
      page.getByText(
        "Nothing came through USB 1/2, so we couldn’t hear this one.",
      ),
    ).toBeVisible();
    await expect(page.getByText("Needs routing on the unit")).toBeVisible();
    // 401 still leveled — its displayed final LUFS near the default target (−23). No "LUFS"
    // suffix on Summary (that's a RunPage-only unit label) — just the bare reading.
    await expect(page.getByText(/[−-]2[234]\.\d/).first()).toBeVisible();

    await expectReampBalanced(page, reampBase);
  });
});
