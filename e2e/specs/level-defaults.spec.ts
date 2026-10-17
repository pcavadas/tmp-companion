import type { Page } from "@playwright/test";

import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  armCaptureDelay,
  armCaptureFault,
  clearScenario,
  ensureScenario,
  expectReampBalanced,
  isOnline,
  levelButton,
  openLevel,
  openPreset,
  pickBaseTarget,
  reampCounters,
  reampOff,
  selectBaseOnly,
  selectPresetsForLevel,
  simEvents,
  soundBox,
  waitForCaptureInFlight,
  startLeveling,
  summaryDone,
  tryAgainButton,
} from "../fixtures/scenario";

// First-session DEFAULTS + the physics-outcome gates (the real user's complaint set), driven
// through the REAL UI so the Summary's words are asserted verbatim. Outcomes come from
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
// clamp/boost cases: sweeping in a preset's footswitches too risks changing the Summary's
// outcome for reasons unrelated to the base outcome under test.
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
// ("Base at Lead (-19) on 403 -> CLAMP at its ceiling (-20)"). Test (b), the clamp loop, runs
// on 403 for exactly this reason — 401 no longer has a LOUD-enough named target to
// demonstrate a clamp with.
//
// Per-SCENE/per-FOOTSWITCH outcomes are asserted at the command level (e2e_server_tests.rs
// `level_defaults_403_scenes_solve_and_offbranch` + `..._base_clamps_and_footswitch_is_offbranch`);
// their streamed rows reach the page through the UI bridge (.claude/rules/e2e.md's "Events
// and Channels reach the page"), but this file reads none of them. The UI here asserts only
// the BASE-leveling outcomes (level_preset returns directly, no Channel): the base-boost
// reaching a loud default (a), the clamp loop — clamped at Lead, then leveled at Crunch
// (b) — and a mid-run off-branch capture fault (c). The 403 Lead clamp is also pinned at
// the command layer (`level_defaults_base_clamps_and_the_split_lane_footswitch_is_offbranch`).
//
// The tests are independent (each starts from the fixture's `/sim/reset`, each worker owns
// its own e2e_server), so the file runs them in parallel: as one serial file it was the
// offline lane's wall-clock floor.

// openLevel / selectBaseOnly / pickBaseTarget now live in ../fixtures/scenario.ts (shared
// with level-setup.spec.ts and level.spec.ts).

/** The run page's Stop, confirmed in its "Stop leveling?" dialog (the page behind the
 *  dialog is inert while it is open, so the confirm is the dialog's own button). */
async function stopRun(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Stop leveling?" });
  await expect(dialog).toContainText("Everything matched so far stays saved.");
  await dialog.getByRole("button", { name: "Stop", exact: true }).click();
}

test.describe.configure({ mode: "parallel" });

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

    await levelButton(page, 1).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click(); // the inline commit gate
    await pickBaseTarget(page, SCENARIO[1].slot, "Lead"); // the loud default — now REACHED via boost
    await startLeveling(page).click();
    await expect(summaryDone(page)).toBeVisible({ timeout: 240_000 });
    // The row solved: leveled, on target, nothing needs the user.
    await expect(
      page.getByRole("heading", { name: "1 sound leveled." }),
    ).toBeVisible();
    const row = page.locator(
      `[data-summary-row="p${String(SCENARIO[1].slot)}"]`,
    );
    await expect(row.getByText("On target")).toBeVisible();
    // The base boost, in words only (`summaryModel.ts`'s SUMMARY_TEXT.baseBoost, copied
    // verbatim — update this string if the copy changes), and APPLIED, not just planned.
    await expect(
      row.getByText(
        "Turned the preset up as far as it goes, then raised the amp's output to reach Lead.",
      ),
    ).toBeVisible();
    await expect(row).toHaveAttribute("data-base-boost", "applied");
    // No clamp message — this row is done, not clamped.
    await expect(page.getByText(/Already at full volume/)).toHaveCount(0);

    await expectReampBalanced(page, reampBase);
  });

  // COVERAGE row 2 — the base solve that still CLAMPS (403 at Lead).
  // (b) The clamp loop (BUG→GATE, the 2026-07 "clamp loop unresolvable" report; journey
  // "clamped at a loud target → resolves at a quieter one"). 403's Base at Lead (-19)
  // clamps at its -20 ceiling: the Summary says so and what to change, with no button and
  // no Try again (running it again can't help). The way to a quieter target is Done,
  // the list and Set up — and at Crunch (-21) it levels. Look B removed the Summary's
  // "Re-level clamped…" that used to reopen Set up.
  test("clamp loop: clamped at Lead says so with no button, then levels at Crunch", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline-only: the 403 ceiling is sidecar-authored",
    );
    await ensureScenario(page);
    const reampBase = await reampCounters(page);
    await openLevel(page);
    const parallel = SCENARIO[3]; // E2E Parallel, ceiling -20
    await selectBaseOnly(page, parallel.name);
    await levelButton(page, 1).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();
    await pickBaseTarget(page, parallel.slot, "Lead");
    await startLeveling(page).click();
    await expect(summaryDone(page)).toBeVisible({ timeout: 240_000 });
    await expect(
      page.getByText(
        "Already at full volume, so it can't reach Lead. Choose a quieter target for it.",
      ),
    ).toBeVisible();
    await expect(tryAgainButton(page)).toHaveCount(0);

    // Done → the list (the clamped sound was resolved, so it is unchosen) → choose it
    // again → Crunch → it levels.
    await summaryDone(page).click();
    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    await filter.fill(parallel.name);
    await soundBox(page, "Main sound", parallel.name).click();
    await filter.fill("");
    await levelButton(page, 1).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();
    await pickBaseTarget(page, parallel.slot, "Crunch");
    await startLeveling(page).click();
    await expect(summaryDone(page)).toBeVisible({ timeout: 240_000 });
    await expect(
      page.getByRole("heading", { name: "1 sound leveled." }),
    ).toBeVisible();

    await expectReampBalanced(page, reampBase);
  });

  // (c) Mid-run failure + no-signal message + the PEDAL-FIASCO gate (the complainant's
  // "items failing / it changed everything" class; was pedal-fiasco.spec.ts): /sim/fault
  // silences 402's first capture → 402's Base goes OFF-BRANCH (no signal) with its routing
  // message and writes NOTHING (a silent capture must never be saved over with a near-zero
  // level), while its sibling 401 still levels and saves. Exactly the three rows the
  // assertions read are selected — 401 Base, 402 Base and 402's `OUT2` switch, the second
  // "can't hear it" (its block sits on the split's OUT-2 lane, COVERAGE row 21) — rather than
  // both whole presets, whose 8 scenes and other switches only cost captures.
  test("mid-run capture fault: one item off-branch (routing banner) writes nothing, the sibling levels and saves", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline-only: /sim/fault is a SimDevice injection",
    );
    await ensureScenario(page);
    const reampBase = await reampCounters(page);
    await openLevel(page);

    await selectBaseOnly(page, SCENARIO[1].name); // 401 Base
    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    await filter.fill(SCENARIO[2].name);
    await openPreset(page, SCENARIO[2].name);
    await soundBox(page, "Main sound", SCENARIO[2].name).click(); // 402 Base
    await soundBox(page, "OUT2", SCENARIO[2].name).click(); // 402 switch 2
    await filter.fill("");

    await armCaptureFault(page, SCENARIO[2].slot); // silence 402's next capture (one-shot)

    await levelButton(page, 3).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();
    await startLeveling(page).click(); // defaults; 401 solves, 402 faults
    await expect(summaryDone(page)).toBeVisible({ timeout: 240_000 });

    // 402 off-branch: two sounds can't be heard — the faulted Base, and switch 2 `OUT2`
    // (the second arrives through the UI bridge) — for the same reason, so they share one
    // row ("All 2 sounds") with the routing message, and Try again can't help them.
    await expect(
      page.getByRole("heading", { name: "1 sound leveled. 2 need you." }),
    ).toBeVisible();
    const needYou = page.getByRole("region", { name: "Need you" });
    await expect(needYou.getByText("All 2 sounds")).toBeVisible();
    await expect(
      needYou.getByText(
        "This sound doesn't reach USB 1/2 on your Tone Master Pro, so leveling can't hear it. Route it to USB 1/2, or set its volume by ear.",
      ),
    ).toHaveCount(1);
    await expect(tryAgainButton(page)).toHaveCount(0);
    // 401 still leveled, on target.
    await expect(
      page
        .locator(`[data-summary-row="p${String(SCENARIO[1].slot)}"]`)
        .getByText("On target"),
    ).toBeVisible();

    // /sim/reset (test fixture) cleared the log at test start, so these are THIS run's writes.
    const saved = (await simEvents(page))
      .map((e) => (e as { Saved?: number }).Saved)
      .filter((s): s is number => typeof s === "number");
    expect(saved, "the healthy sibling (401) leveled and saved").toContain(
      SCENARIO[1].slot,
    );
    expect(
      saved,
      "the faulted preset (402) must NOT be saved — a silent capture writes nothing",
    ).not.toContain(SCENARIO[2].slot);

    await expectReampBalanced(page, reampBase);
  });

  // COVERAGE row 46 — a 60 s capture hold makes the stopped Summary inside 15 s reachable
  // only by a cancel served mid-run (`e2e_server.rs` `e2e_is_side_lane`). Look B: Stop
  // asks first in a dialog, and the Summary shows on its own once the run has wound down.
  test("a mid-run Stop aborts the in-flight item and leaves the rest not run", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline-only: /sim/capture-delay is a SimDevice injection",
    );
    await ensureScenario(page);
    const reampBase = await reampCounters(page);
    await armCaptureDelay(page, 60_000);
    await openLevel(page);

    await selectPresetsForLevel(page, [SCENARIO[1], SCENARIO[2]]);

    await levelButton(page, "any").click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();
    await startLeveling(page).click();
    await waitForCaptureInFlight(page, reampBase);

    await stopRun(page);
    await expect(
      page.getByRole("heading", {
        name: /^Stopped\. Nothing was leveled, \d+ not reached\.$/,
      }),
    ).toBeVisible({ timeout: 15_000 });
    await expect(
      page.getByRole("region", { name: "Not reached" }),
    ).toBeVisible();
    await expect(tryAgainButton(page)).toBeVisible();

    await expectReampBalanced(page, reampBase);
  });

  // The fourth leveling scenario (re-run what failed or ran out of tries): after a Stop,
  // the Summary's one "Try again (N)" runs the sounds the Stop didn't reach straight away
  // — no Set up, no second backup tick — and the same Summary comes back with them
  // leveled. Two clicks: Try again · Done.
  test("scenario 4, Try again after a Stop: Try again (N) · Done = 2, the same Summary updated", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline-only: /sim/capture-delay is a SimDevice injection",
    );
    await ensureScenario(page);
    const reampBase = await reampCounters(page);
    await armCaptureDelay(page, 60_000);
    await openLevel(page);
    await selectBaseOnly(page, SCENARIO[1].name);
    await selectBaseOnly(page, SCENARIO[2].name);
    await levelButton(page, 2).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();
    await startLeveling(page).click();
    await waitForCaptureInFlight(page, reampBase);
    await stopRun(page);
    await expect(
      page.getByRole("heading", {
        name: "Stopped. Nothing was leveled, 2 not reached.",
      }),
    ).toBeVisible({ timeout: 15_000 });
    await armCaptureDelay(page, 0);

    let clicks = 0;
    await tryAgainButton(page, 2).click(); // 1 Try again (2)
    clicks++;
    // Straight to the run of just those sounds: no Set up, no backup tick.
    await expect(
      page.getByRole("heading", { name: "Leveling 2 sounds" }),
    ).toBeVisible();
    await expect(startLeveling(page)).toHaveCount(0);
    await expect(summaryDone(page)).toBeVisible({ timeout: 240_000 });
    // The same Summary, both rows now leveled (a main sound already at its target skips
    // its save, so the Summary, not the save log, is the evidence).
    await expect(
      page.getByRole("heading", { name: "All 2 sounds leveled." }),
    ).toBeVisible();
    await expect(
      page.getByRole("region", { name: "Leveled" }).getByText("On target"),
    ).toHaveCount(2);
    await summaryDone(page).click(); // 2 Done
    clicks++;
    await expect(
      page.getByText("Choose presets or sounds to level"),
    ).toBeVisible();
    expect(clicks).toBe(2);

    await expectReampBalanced(page, reampBase);
  });

  // #283: a tab switch used to unmount the run with it, leaving the loop writing to the
  // unit with no reachable Stop. The run lives in a module store now, so leaving the Level
  // tab and coming back shows the same run, and its Stop still ends it.
  //
  // Harness note: the remounted view's reloads queue on the e2e server behind the run, and
  // Chromium's six-connections-per-host pool fills with them, so the Stop's cancel invoke
  // only leaves the page once the in-flight item returns. The run itself sees the Stop at
  // once (the loop's own flag), so it ends after that item instead of aborting it; the
  // capture delay is short for that reason — every capture of that item pays it in full.
  test("a run survives a tab switch, and its Stop still ends it", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline-only: /sim/capture-delay is a SimDevice injection",
    );
    await ensureScenario(page);
    const reampBase = await reampCounters(page);
    await armCaptureDelay(page, 3_000);
    await openLevel(page);

    await selectPresetsForLevel(page, [SCENARIO[1], SCENARIO[2]]);

    await levelButton(page, "any").click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();
    await startLeveling(page).click();
    await waitForCaptureInFlight(page, reampBase);

    await page.getByRole("button", { name: "Settings", exact: true }).click();
    await page.getByRole("button", { name: "Level", exact: true }).click();

    // Back on Level: the run is still on screen, with a working Stop.
    await stopRun(page);
    await expect(
      page.getByRole("heading", { name: /^Stopped\. .* not reached\.$/ }),
    ).toBeVisible({ timeout: 60_000 });
    // Only the item in flight at the Stop ran; the rest of the run did not.
    await expect(
      page.getByRole("region", { name: "Not reached" }),
    ).toBeVisible();

    await expectReampBalanced(page, reampBase);
  });
});
