import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  clearScenario,
  ensureScenario,
  expectReampBalanced,
  reampCounters,
  reampOff,
} from "../fixtures/scenario";

// Level scenarios — run identically offline (fake re-amp) and online (real re-amp).
// SCENARIO[0] "E2E Reference" has BOTH footswitch scenes AND block-acting footswitches
// (and an amp); SCENARIO[1]/[2] "E2E Target 1/2" are PLAIN (no scenes, no footswitches).
// Loudness accuracy is the device's job; these prove the multi-preset, per-preset-target
// flow AND the base+scene+footswitch flow end to end through the real backend.
test.describe("Level — plain presets + a scenes-and-footswitches preset", () => {
  // Between tests: SAFETY only (re-amp off, so an aborted capture can't strand the
  // unit input-muted for the next test). Slot cleanup happens ONCE in afterAll —
  // clearing between tests would force the next test's ensureScenario down the flaky
  // in-process re-seed (the runner seeds once per spec FILE; HW-observed: test 1
  // passed, its clear forced test 2's re-seed into the 0xe00002c5 open lockout).
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });

  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  test("levels two PLAIN presets to different targets, end to end", async ({
    page,
  }) => {
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    await page.goto("/");
    await page.getByRole("button", { name: /backed up/i }).click(); // startup disclaimer
    await expect(page.getByText(/connected · \d+\.\d+/)).toBeVisible({
      timeout: 20_000,
    });

    // Target 1 + Target 2 are PLAIN (no scenes, no footswitches), so the whole-preset
    // CHECKBOX selects exactly their Base — the simplest, most common preset shape. The
    // filter narrows the list to each in turn; the selection persists across filters.
    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    const plain = [SCENARIO[1], SCENARIO[2]];
    for (const p of plain) {
      await filter.fill(p.name);
      await page.getByTitle("Select preset to level").first().click();
    }
    await filter.fill("");

    await page.getByRole("button", { name: /Level 2 preset/ }).click();

    // The wizard opens directly at Set up; tick the inline footer ack that gates the
    // commit (there is no separate Back-up step).
    await page.getByText(/I.ve backed up with Pro Control/i).click();

    // Two DIFFERENT per-preset targets (each plain preset = one Base row → its
    // `target:NAME` trigger is unique, no collision).
    const targets = [
      { name: SCENARIO[1].name, label: "Crunch" },
      { name: SCENARIO[2].name, label: "Lead" },
    ];
    for (const { name, label } of targets) {
      await page.locator(`[data-pick="target:${name}"]`).click();
      await page.getByText(new RegExp(label)).click();
    }
    // The picks must actually BIND — assert each row's trigger now reads its target
    // (guards a silent display-vs-value no-op the always-solving fake re-amp would hide).
    for (const { name, label } of targets) {
      await expect(page.locator(`[data-pick="target:${name}"]`)).toContainText(
        label,
      );
    }

    await page.getByRole("button", { name: /Level 2 sound/ }).click();
    await expect(page.getByRole("button", { name: "Done" })).toBeVisible({
      timeout: 240_000,
    });

    // Standing safety gate: the app disengaged re-amp at least as often as it engaged,
    // checked BEFORE the afterEach reampOff rescue (so a stranded engage fails here).
    await expectReampBalanced(page, reampBase);
  });

  // The mandatory "both scenes and footswitches" case: E2E Reference carries a Base, 2
  // footswitch SCENES (Rhythm/Lead, amp outputLevel) AND block-acting FOOTSWITCHES. Ticking
  // the whole preset sweeps in ALL of them, so the run exercises base (level_preset) +
  // scene (level_scenes_apply_batched) + footswitch (level_footswitches_apply) leveling in
  // one preset. Oracle: Set up shows all three row kinds (asserted via their distinct
  // sub-text), the bake/assign mechanism never leaks, and the run reaches a terminal
  // Summary. Offline the fake re-amp may clamp scenes/footswitches — that's expected; the
  // base still solves and the flow completes.
  test("levels a preset with base + scenes + footswitches end to end", async ({
    page,
  }) => {
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    await page.goto("/");
    await page.getByRole("button", { name: /backed up/i }).click(); // startup disclaimer
    await expect(page.getByText(/connected · \d+\.\d+/)).toBeVisible({
      timeout: 20_000,
    });

    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    await filter.fill(SCENARIO[0].name); // E2E Reference

    // Reveal its children (Base + scene rows + footswitch rows), then tick the WHOLE
    // preset → every child selected.
    await page.getByTitle(/Show Base/).click();
    await page.getByTitle("Select preset to level").first().click();
    await filter.fill("");

    await page.getByRole("button", { name: /Level 1 preset/ }).click();
    // The wizard opens directly at Set up; tick the inline footer ack that gates the commit.
    await page.getByText(/I.ve backed up with Pro Control/i).click();

    // Set up must show all THREE row kinds — proven by their distinct sub-text copy.
    await expect(
      page.getByText(/levels this preset against the others/),
    ).toBeVisible(); // Base
    await expect(
      page.getByText(/levels this scene against/).first(),
    ).toBeVisible(); // a footswitch SCENE
    await expect(
      page.getByText(/evens this footswitch out to your target/).first(),
    ).toBeVisible(); // a block-acting FOOTSWITCH
    // The bake/assign mechanism is never surfaced.
    await expect(page.getByText(/baked|assigned/i)).toHaveCount(0);

    // Run base + scenes + footswitches → a terminal Summary (Done OR Accept; offline
    // clamps on scenes/footswitches are fine).
    await page.getByRole("button", { name: /Level \d+ sound/ }).click();
    await expect(
      page.getByRole("button", { name: /^(Done|Accept)$/ }),
    ).toBeVisible({ timeout: 240_000 });

    await expectReampBalanced(page, reampBase);
  });

  // BUG→GATE (2026-07-27 report — the corruption-class preset's SHAPE). SCENARIO[4]
  // "E2E Hiwatt 3S" is a real unit's preset: 3 tone scenes + a 4th literally named
  // "Base Scene" (a real overlay, NOT the base sentinel) and 4 block-acting footswitches,
  // saved `lastLoadedScene = 3`. Its outcomes are command-level gates (the per-scene/
  // per-footswitch Channel is a no-op offline — see level-defaults.spec.ts's header); what
  // ONLY the UI can prove is that the backup scan ENUMERATES this shape: 4 scene children
  // (not 3 + a swallowed "Base Scene", and not 5 with base double-counted) plus 4 footswitch
  // children. No leveling run — list rendering only, so it costs seconds.
  test("enumerates the 3-scene + Base-Scene + 4-footswitch preset in the list", async ({
    page,
  }) => {
    await ensureScenario(page);
    await page.goto("/");
    await page.getByRole("button", { name: /backed up/i }).click(); // startup disclaimer
    await expect(page.getByText(/connected · \d+\.\d+/)).toBeVisible({
      timeout: 20_000,
    });

    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    await filter.fill(SCENARIO[4].name);
    // The collapsed breakdown — the scan's own count, before any expansion.
    await expect(page.getByText("4 scenes · 4 footswitches")).toBeVisible({
      timeout: 60_000,
    });

    await page.getByTitle(/Show Base/).click();
    // Base + 4 footswitch scenes + 4 block-acting footswitches = 9 child rows.
    await expect(page.getByText("main preset sound")).toHaveCount(1);
    await expect(page.getByText("footswitch scene")).toHaveCount(4);
    await expect(page.getByText("footswitch", { exact: true })).toHaveCount(4);
    // The 4th scene is a real overlay named "Base Scene" — it must appear as its OWN row,
    // distinct from the "Base Preset" row (the sentinel).
    await expect(page.getByText("Base Scene", { exact: true })).toHaveCount(1);
    await expect(page.getByText("Base Preset", { exact: true })).toHaveCount(1);
  });
});
