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
// -15, 401 (E2E Pedalboard) -20, 402 (E2E Edge) -13, 403 (E2E Parallel) -20. -23/-21 both solve
// on every preset. The tests below select ONLY the Base row (never the whole preset) for the
// clamp/boost cases: sweeping in a preset's footswitches too risks shifting the terminal
// Done-vs-Accept summary text for reasons unrelated to the base outcome under test.
//
// P4 (the Plumes/BD2/OCD leveling-regression fix, `headroom_trade::plan_level_pair`) SPLIT
// what "the loud shipped default" (Lead, -19) does at a preset's ~-20 ceiling, by whether a
// SECOND control exists to close the gap: 401 has exactly one active, non-maxed amp candidate
// (`ACD_MarshallPlexi`, `outputLevel` authored 0.6) with headroom above 0.6, so it now BOOSTS
// past its old clamp and reaches Lead — see test (a) for the exact math. 403 has TWO active
// amps (`gtrParallel1`, both already at `outputLevel` 1.0), which the boost candidate
// derivation refuses outright (Phase 1's own refusal list: "≥2 amp knobs (parallel —
// danger.md OPEN distrust)"; a maxed fader would also have zero room even if it were
// considered) — so 403 still clamps at Lead exactly as before, matching
// `e2e_server_tests.rs`'s `level_defaults_base_clamps_and_the_split_lane_footswitch_is_offbranch`
// ("Base at Lead (-19) on 403 -> CLAMP at its ceiling (-20)"). Test (b) (the re-level-clamped
// loop) now runs on 403 for exactly this reason — 401 no longer has a LOUD-enough named
// target to demonstrate a clamp with.
//
// HARNESS LIMIT: per-SCENE/per-FOOTSWITCH outcomes hit the Channel-streaming seam offline
// (asserted at the command level instead — see e2e_server_tests.rs
// `level_defaults_403_scenes_solve_and_offbranch` + `..._base_clamps_and_footswitch_is_offbranch`;
// rationale in .claude/rules/e2e.md's "The Channel-streaming seam"). The UI here asserts only
// the BASE-leveling outcomes (level_preset returns directly, no Channel): the base-boost
// reaching a loud default (a), the re-level-clamped loop (b), and a mid-run off-branch
// capture fault (c).

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

  // COVERAGE row 2 — base solve that USED to CLAMP, now BOOSTS.
  // (a) 401's Base at the LOUD shipped default (Lead, -19) used to clamp at its ~-20 ceiling
  // (PR2 re-baseline math lives in scenario-loudness.json's "401" entry) — pure `presetLevel`
  // maxing out short of target, nothing else to give. Post-P4, `headroom_trade::
  // plan_level_pair` finds a SECOND control: `ACD_MarshallPlexi`'s own `outputLevel`, stored
  // 0.6 (COVERAGE.md's cab-rule table), well below its 1.0 ceiling. The math (`g = target -
  // base_asis_lufs`, `p_up`/`f_up` = presetLevel/fader headroom in dB — see
  // `headroom_trade.rs`'s own doc comments):
  //   - presetLevel raises from its authored 0.32 to its ceiling 1.0: raise_db = p_up =
  //     20·log10(1/0.32) ≈ 9.90 dB — this alone reaches exactly C = -20 (today's old ceiling).
  //   - `g > p_up` (BOOST's own trigger) reduces algebraically to `target > C`: -19 > -20, so
  //     BOOST fires. The remaining gap the fader must close is exactly `target - C` = -19 -
  //     (-20) = +1.00 dB — well inside the amp's own `f_up` = 20·log10(1/0.6) ≈ 4.44 dB of
  //     headroom, so this REACHES target rather than clamping the fader too (the "clamped:true
  //     WITH base_boost.applied:true" shape only fires when the ask exceeds `f_up`).
  //   - solved fader = 0.6 · 10^(1/20) ≈ 0.6732 → 0.67 to 2 decimals (`seed_fader_target`).
  //     Zero secant corrections expected: 401 declares no `leveledParams`, so the sim's
  //     `ol_term` is exactly log-linear (same zero-correction precedent as 405's Twin fader).
  //   - the UI always saves (`useLevelingFlow.ts`: "Leveling always WRITES (save:true)"), so
  //     `base_boost.applied` is `true`, not the `save:false` advisory shape.
  test("base at the loud default now reaches it via the amp-fader boost", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline-only: the boost math is sidecar-authored",
    );
    await ensureScenario(page);
    const reampBase = await reampCounters(page);
    await openLevel(page);
    await selectBaseOnly(page, SCENARIO[1].name); // E2E Pedalboard

    await page.getByRole("button", { name: /Level 1 preset/ }).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click(); // the inline commit gate
    await pickBaseTarget(page, SCENARIO[1].slot, "Lead"); // the loud default — now REACHED via boost
    await page.getByRole("button", { name: /Start.*1 sound/ }).click();
    // allGood (SummaryPage.tsx): no clamp/unconverged/offbranch/skip rows this run ⇒ "Done",
    // not "Accept" — the row solved, it didn't need acknowledgement.
    await expect(page.getByRole("button", { name: "Done" })).toBeVisible({
      timeout: 240_000,
    });
    // `useGroupOpen`'s `badSlots` auto-open list is built from non-"done" rows only
    // (SummaryPage.tsx) — an all-good run's preset group starts COLLAPSED, unlike the
    // off-branch case in test (c) below. Expand it before reading the row detail.
    await page
      .locator(`[data-preset-group="${String(SCENARIO[1].slot)}"]`)
      .click();

    // The base_boost disclosure (SummaryPage.tsx's `baseBoostSentence`). Copied VERBATIM from
    // that function's template — never re-word it here if the copy changes; update this
    // string instead.
    await expect(
      page.getByText(
        "Turned this preset up as far as it goes and raised the amp’s output from 0.60 to 0.67 to reach the target.",
      ),
    ).toBeVisible();
    // No clamp banner remains — this row is done, not clamped.
    await expect(page.getByText(/Clamped .* already maxed/)).toHaveCount(0);
    // The row's own achieved reading, at Lead (-19).
    await expect(page.getByText(/[−-]19\.\d/)).toBeVisible();

    await expectReampBalanced(page, reampBase);
  });

  // (b) Re-level-clamped loop: a sound clamped at a LOUDER target resolves when re-leveled at a
  // quieter one. RETARGETED (405-era 401 → 403): 401's Base at Lead no longer clamps at all
  // (test (a) above) since it now has a fader to boost with, so it can't demonstrate this loop
  // any more with a NAMED target (Rhythm/Crunch/Lead are the only picker options). 403 (E2E
  // Parallel, ceiling ~-20) is UNAFFECTED by the boost feature — its base pair carries TWO
  // active amps (`gtrParallel1`, both already at `outputLevel` 1.0), which the boost candidate
  // derivation refuses outright (Phase 1's "≥2 amp knobs" refusal; a maxed fader would have
  // zero headroom regardless) — so it clamps at Lead (-19) and resolves at Crunch (-21) exactly
  // as before. Summary → "Re-level clamped…" → lower target → run 2 → the row is done (no
  // clamp banner remains).
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
    await selectBaseOnly(page, SCENARIO[3].name); // E2E Parallel, Base only

    await page.getByRole("button", { name: /Level 1 preset/ }).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();
    await pickBaseTarget(page, SCENARIO[3].slot, "Lead"); // pick the LOUD target
    await page.getByRole("button", { name: /Start.*1 sound/ }).click();
    await expect(page.getByRole("button", { name: "Accept" })).toBeVisible({
      timeout: 240_000,
    });
    await expect(page.getByText(/Clamped .* already maxed/)).toBeVisible();

    await page.getByRole("button", { name: /Re-level clamped/ }).click();
    await pickBaseTarget(page, SCENARIO[3].slot, "Crunch"); // QUIETER target
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
