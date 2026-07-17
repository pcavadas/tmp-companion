import { test, expect } from "../fixtures/test";
import { SCENARIO, clearScenario, ensureScenario } from "../fixtures/scenario";

// Doctor journey — runs identically offline (fake re-amp: the "capture" is the raw
// stimulus, so every sound measures finite and identical) and online (real re-amp
// captures, ~15 s/sound). The oracle is the FLOW: select → set up → run →
// auto-advance → a Results page that renders every checked preset with either
// diagnosis cards or "All clear". Diagnosis CONTENT is sound-dependent, so the spec
// never asserts a specific tag; the prescription-content regressions (existing-comp
// advisory, presetLevel preservation) are backend-validated in doctor.rs unit tests
// and the probe/HW lane, not here.
//
// ONLINE seeding note: scripts/e2e.sh seeds the scenario via `probe --seed-scenario`
// BEFORE the server's handshake (a post-handshake line serves truncated list reads +
// 0xe00002c5 open lockouts for minutes — HW-observed); `ensureScenario` here is the
// presence check + the fallback for direct playwright runs. If the runner's seed
// fails all attempts with truncated reads/lockouts, the unit is congested from prior
// cycling: power-cycle it, wait a minute, rerun.
test.describe("Doctor — select, check, results", () => {
  test.afterEach(async ({ page }) => {
    await clearScenario(page);
  });

  test("checks three presets end to end and lands on Results", async ({
    page,
  }) => {
    await ensureScenario(page);

    await page.goto("/");
    await page.getByRole("button", { name: /backed up/i }).click(); // startup disclaimer
    await expect(page.getByText(/connected · \d+\.\d+/)).toBeVisible({
      timeout: 20_000,
    });

    await page.getByRole("button", { name: "Doctor" }).click();

    // Select the two PLAIN scenario presets (Base only → 1 sound each) AND the
    // Reference preset (Base + 2 scenes + block-acting footswitches) so the run
    // exercises the scene/footswitch doctor paths too — the sound count is
    // scenario-shape-dependent, so the buttons match on /\d+ sounds/.
    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    const picked = [SCENARIO[0], SCENARIO[1], SCENARIO[2]];
    for (const p of picked) {
      await filter.fill(p.name);
      await page.getByTitle("Select preset to check").first().click();
    }
    await filter.fill("");

    await page.getByRole("button", { name: /Check \d+ sounds/ }).click();

    // Set up: keep the defaults, run.
    await page.getByRole("button", { name: /Run check on \d+ sounds/ }).click();

    // The run auto-advances to Results on a natural finish. Progress events don't
    // stream over the bridge, so the only signal is the terminal Results page.
    await expect(
      page.getByText(/presets? need a look|All clear/).first(),
    ).toBeVisible({ timeout: 240_000 });

    // Every checked preset renders on Results — a card (flagged) or "All clear".
    for (const p of picked) {
      await expect(page.getByText(p.name).first()).toBeVisible();
    }

    // Expanding any measured sound row surfaces the cut-through estimate —
    // `cutThrough` is non-null for every successful guitar capture, so this is
    // deterministic (unlike diagnosis content, which stays unasserted). A plain
    // preset's base row is labeled with the preset name, so the LAST match is
    // the clickable sound row (the first is the card header).
    await page.getByText(picked[1].name).last().click();
    await expect(
      page.getByText("Cut-through (estimated)").first(),
    ).toBeVisible();
  });
});
