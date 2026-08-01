import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  clearScenario,
  ensureScenario,
  expectReampBalanced,
  isOnline,
  reampCounters,
  reampOff,
} from "../fixtures/scenario";

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
// BEFORE the server starts (fresh-process seeding dodges the in-process 0xe00002c5
// open lockout that aborted in-spec seeds) and POSTs `e2e_mark_seeded`, which arms the
// server's verified-seed flag; `ensureScenario` here always calls `e2e_seed_scenario`
// online, which fast-no-ops on that flag and only pays the full ownership-verified
// in-process seed on direct playwright runs (or after a clear). If the runner's seed
// fails all attempts, check nothing else holds the device (Pro Control, a stale
// server/app), rest a minute, rerun.
test.describe("Doctor — select, check, results", () => {
  test.afterEach(async ({ page }) => {
    // Re-amp OFF rescue FIRST — a mid-test failure before the balance gate must not strand
    // the unit input-muted (clearScenario's own reampOff is last, after minutes of clears).
    await reampOff(page);
    await clearScenario(page);
  });

  test("checks three presets end to end and lands on Results", async ({
    page,
  }) => {
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

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

    // The default "Needs a look" filter HIDES fully-clean presets (DoctorResults
    // `shown`), so flip to "Everything" first when the filter strip is present (it
    // only renders when there is a clean preset to hide; on all-clear every card
    // already shows). The pill is a SegmentedControl → role="radio", not "button".
    const everything = page.getByRole("radio", { name: "Everything" });
    if (await everything.isVisible().catch(() => false)) {
      await everything.click();
    }

    // Every checked preset renders on Results — a card, flagged or clean.
    for (const p of picked) {
      await expect(page.getByText(p.name).first()).toBeVisible();
    }

    // STRICT diagnosis oracle (online-only — the offline fake capture is a
    // stimulus passthrough, so no real spectrum exists to diagnose): the checked
    // set carries one KNOWN defect and two known-healthy controls, and the
    // Doctor must call both sides correctly.
    //  * "E2E Target 2" ships a fixture-injected post-cab EQ-5 parametric with
    //    two stacked +12 dB Q14 peaks at 2.6 kHz — the calibration suite's
    //    `resonant_peq` DefectRecipe (its Q14 saturation ceiling ≈17 dB clears
    //    the resonant height gate on any chain) — and MUST be flagged with the
    //    localized "Rings at N kHz" resonant chip. Resonant is the tilt-robust
    //    oracle: it fires on the transfer's LOCAL octave-median-envelope excess,
    //    so the bright/scooped broadband sweep every scenario chain measures
    //    (which silences broadband verdicts like Muddy) cannot mask it. The
    //    anchored regex tolerates PSD peak-fit wobble in the decimal and skips
    //    the Rx title (which embeds the same phrase); broadband side-effects of
    //    the +12 dB stack (harsh/fizzy) may co-fire and stay unasserted.
    //  * The two healthy presets must produce NO resonant chip anywhere, so
    //    exactly ONE renders in Everything view (HW sweep: resonant fired on
    //    Target 2 alone — 22 dB, Q 24 — across all five scenario presets).
    //  * Opening the defect preset's own sound row must surface the RIGHT
    //    prescription — the cut at the EQ-10 band nearest the MEASURED ring
    //    (2.5–2.7 kHz all map log-nearest to the 2 kHz band; the chain owns a
    //    parametric EQ, so the Rx is the point-at-your-EQ advisory).
    if (await isOnline(page)) {
      await expect(page.getByText(/^Rings at 2\.\d kHz$/)).toHaveCount(1);
      await page.getByText(picked[2].name).last().click();
      await expect(
        page.getByText(/Rings at 2\.\d kHz — cut the 2 kHz band/).first(),
      ).toBeVisible();
      // Collapse the defect row again so the cut-through assertion below can only
      // resolve against picked[1]'s freshly expanded row, never this one.
      await page.getByText(picked[2].name).last().click();
      await expect(
        page.getByText(/Rings at 2\.\d kHz — cut the 2 kHz band/),
      ).toHaveCount(0);
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

    // Standing re-amp-OFF safety gate — the doctor capture path must disengage re-amp
    // at least as often as it engages (checked before any teardown rescue).
    await expectReampBalanced(page, reampBase);
  });
});
