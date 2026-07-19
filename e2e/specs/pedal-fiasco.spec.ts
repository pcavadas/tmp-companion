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
  simEvents,
} from "../fixtures/scenario";

// PEDAL-FIASCO regression gates (the complainant's "items failing / it changed everything" class).
// The load-bearing safety invariant: a SILENT capture (no signal on USB 1/2) must NEVER write a
// near-zero level — the floor guard skips it, so an off-branch / faulted sound is reported, not
// saved-over with garbage. Asserted through the SimDevice event log (`/sim/events`): the faulted
// preset produces ZERO write ops (no PresetLevel, no Saved), while its healthy sibling saves.
//
// SCOPE (offline): the FORCED-BYPASS-then-reload-before-save half of the pedal-fiasco (footswitch
// measurement isolation never persisting its forced bypasses) is NOT drivable offline — footswitch
// leveling needs a field-8 preset read the SimDevice doesn't model (out of PR3 scope; see the plan
// + sim_device.rs). That invariant is enforced + covered by the leveller's `write_footswitch_values`
// (reload at leveller.rs before the single save) and the `copy_apply_one` op-order Rust tests
// (never-save-on-presetError / retry-the-dropped-first-edit). Offline this gate covers the
// silent-capture-writes-nothing half, which IS the "items failing" symptom the user reported.

const savedSlots = (ev: unknown[]): number[] =>
  ev
    .map((e) => (e as { Saved?: number }).Saved)
    .filter((s): s is number => typeof s === "number");

test.describe("Pedal fiasco — a silent capture writes nothing", () => {
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  test("capture-faulted preset writes ZERO level ops; its sibling still saves", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline-only: /sim/fault is a SimDevice injection",
    );
    await ensureScenario(page);
    const reampBase = await reampCounters(page);
    await page.goto("/");
    const disclaimer = page.getByRole("button", { name: /backed up/i });
    if (await disclaimer.isVisible().catch(() => false))
      await disclaimer.click();
    await expect(page.getByText(/connected · \d+\.\d+/)).toBeVisible({
      timeout: 20_000,
    });

    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    for (const p of [SCENARIO[1], SCENARIO[2]]) {
      await filter.fill(p.name);
      await page.getByTitle("Select preset to level").first().click();
    }
    await filter.fill("");

    // Silence 402's next capture → its base measure hits NO_SIGNAL → off-branch, no write.
    await armCaptureFault(page, SCENARIO[2].slot);

    await page.getByRole("button", { name: /Level 2 preset/ }).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();
    await page.getByRole("button", { name: /Level \d+ sound/ }).click();
    await expect(
      page.getByRole("button", { name: /^(Done|Accept)$/ }),
    ).toBeVisible({
      timeout: 240_000,
    });

    // /sim/reset (test fixture) cleared the log at test start, so these are THIS run's writes.
    const saved = savedSlots(await simEvents(page));
    expect(saved, "the healthy sibling (401) leveled and saved").toContain(
      SCENARIO[1].slot,
    );
    expect(
      saved,
      "the faulted preset (402) must NOT be saved — a silent capture writes nothing",
    ).not.toContain(SCENARIO[2].slot);

    await expectReampBalanced(page, reampBase);
  });
});
