import { test, expect } from "../fixtures/test";
import { SCENARIO, clearScenario, ensureScenario } from "../fixtures/scenario";

// Level scenarios — run identically offline (fake re-amp) and online (real re-amp).
// SCENARIO[0] "E2E Reference" has BOTH footswitch scenes AND block-acting footswitches
// (and an amp); SCENARIO[1]/[2] "E2E Target 1/2" are PLAIN (no scenes, no footswitches).
// Loudness accuracy is the device's job; these prove the multi-preset, per-preset-target
// flow AND the base+scene+footswitch flow end to end through the real backend.
test.describe("Level — plain presets + a scenes-and-footswitches preset", () => {
  test.afterEach(async ({ page }) => {
    await clearScenario(page);
  });

  test("levels two PLAIN presets to different targets, end to end", async ({
    page,
  }) => {
    await ensureScenario(page);

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
  });
});
