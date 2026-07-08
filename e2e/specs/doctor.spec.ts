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
// ONLINE flake note: `ensureScenario`'s seed is the first fresh HID open after the
// server-start handshake and can hit the device's open LOCKOUT (0xe00002c5) even
// through hid.rs's retry ladder — scripts/e2e.sh now rests 60 s before the first
// spec, which improves the odds but does not fully cure it (HW-observed). On a
// lockout failure, wait a minute or two of true quiet and rerun.
test.describe("Doctor — select, check, results", () => {
  test.afterEach(async ({ page }) => {
    await clearScenario(page);
  });

  test("checks two presets end to end and lands on Results", async ({
    page,
  }) => {
    await ensureScenario(page);

    await page.goto("/");
    await page.getByRole("button", { name: /backed up/i }).click(); // startup disclaimer
    await expect(page.getByText(/connected · \d+\.\d+/)).toBeVisible({
      timeout: 20_000,
    });

    await page.getByRole("button", { name: "Doctor" }).click();

    // Select the two PLAIN scenario presets (Base only → 2 sounds).
    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    const plain = [SCENARIO[1], SCENARIO[2]];
    for (const p of plain) {
      await filter.fill(p.name);
      await page.getByTitle("Select preset to check").first().click();
    }
    await filter.fill("");

    await page.getByRole("button", { name: /Check 2 sounds/ }).click();

    // Set up: keep the defaults, run.
    await page.getByRole("button", { name: /Run check on 2 sounds/ }).click();

    // The run auto-advances to Results on a natural finish. Progress events don't
    // stream over the bridge, so the only signal is the terminal Results page.
    await expect(
      page.getByText(/presets? need a look|All clear/).first(),
    ).toBeVisible({ timeout: 240_000 });

    // Every checked preset renders on Results — a card (flagged) or "All clear".
    for (const p of plain) {
      await expect(page.getByText(p.name).first()).toBeVisible();
    }
  });
});
