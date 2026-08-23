import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  clearScenario,
  ensureScenario,
  expectReampBalanced,
  isOnline,
  openLevel,
  reampCounters,
  reampOff,
  runDoctorCheck,
  selectPresetsForCheck,
} from "../fixtures/scenario";

// Doctor journey — runs identically offline (fake re-amp: the sim's physics model
// scales the stimulus by each sound's sidecar C, so every sound measures finite —
// levels differ per scene, spectra don't) and online (real re-amp captures,
// ~15 s/sound). The oracle is the FLOW: select → set up → run →
// auto-advance → a Results page that renders every checked preset with either
// diagnosis cards or "All clear". Diagnosis CONTENT is sound-dependent, so the spec
// never asserts a specific tag; the prescription-content regressions (existing-comp
// advisory, presetLevel preservation) are backend-validated in doctor.rs unit tests
// and the probe/HW lane, not here.
//
// ONLINE e2e suite consolidation (8→4 files): this file's first test below now runs
// OFFLINE ONLY (test.skip'd online) — its online-specific assertions (the E2E Edge
// EQ-ring chip + "cut the 2 kHz band" prescription, plus the cross-card ring-chip
// scoping proof on the other two presets) moved to `doctor.online.spec.ts`, which runs
// the same 3-preset selection online — see that file's own header for why. The offline
// FLOW proof here (select → check → run → Results, three presets' cards render) is
// unaffected and keeps running here exactly as before. The second test below
// (leveling-damage advisories) is now explicitly offline-only too — it performs zero
// device captures (a backup-scan read only), so an online run of it proved nothing extra.
//
// ONLINE seeding note (kept for the file's own online-tier assertions, if any are ever
// re-added): scripts/e2e.sh seeds the scenario via `probe --seed-scenario` BEFORE the
// server starts (fresh-process seeding dodges the in-process 0xe00002c5 open lockout
// that aborted in-spec seeds) and POSTs `e2e_mark_seeded`, which arms the server's
// verified-seed flag; `ensureScenario` here always calls `e2e_seed_scenario` online,
// which fast-no-ops on that flag and only pays the full ownership-verified in-process
// seed on direct playwright runs (or after a clear). If the runner's seed fails all
// attempts, check nothing else holds the device (Pro Control, a stale server/app),
// rest a minute, rerun.
test.describe("Doctor — select, check, results", () => {
  test.afterEach(async ({ page }) => {
    // Re-amp OFF rescue FIRST — a mid-test failure before the balance gate must not strand
    // the unit input-muted (clearScenario's own reampOff is last, after minutes of clears).
    await reampOff(page);
    await clearScenario(page);
  });

  // COVERAGE row 28 — Doctor's as-played scenes (400/401/402 all selected, exercising
  // the scene/footswitch doctor paths).
  test("checks three presets end to end and lands on Results", async ({
    page,
  }) => {
    // OFFLINE ONLY (trade, ONLINE e2e consolidation): the online-only assertions this
    // test used to carry now live in `doctor.online.spec.ts`, which runs the identical
    // 3-preset selection — see this file's own header. Running this flow online too
    // would just duplicate that run's device time for no extra proof; the FLOW itself
    // is identical in both tiers, so nothing online-specific is lost.
    test.skip(
      await isOnline(page),
      "online half moved to doctor.online.spec.ts",
    );
    // ~29 real sounds across the three intact fixtures (E2E Rig + Pedalboard + Edge, base +
    // scenes + footswitches) at ~12-18 s/capture online, plus the pristine-check reseed and
    // backup scan before the run even starts — worst case ≈ 900 s, matching the terminal
    // wait below; the budget here adds headroom on top.
    test.setTimeout(1_200_000);
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    await openLevel(page);

    // Select E2E Pedalboard + E2E Edge (401/402) AND E2E Rig (400, base + 4 scenes +
    // block-acting footswitches) so the run exercises the scene/footswitch doctor paths
    // too — the sound count is scenario-shape-dependent, so the buttons match on
    // /\d+ sounds/.
    const picked = [SCENARIO[0], SCENARIO[1], SCENARIO[2]];
    await selectPresetsForCheck(page, picked);
    await runDoctorCheck(page);

    // The run auto-advances to Results on a natural finish. Progress events don't stream
    // over the bridge, so the only signal is the terminal Results page — but a rejected run
    // (backend/IPC failure) renders LoadErrorPane's "The check couldn't finish: …" within
    // seconds instead (DoctorView.tsx). The error pane is a fast-fail: match it in the wait
    // so a dropped device reports in seconds, then assert it wasn't the branch we took.
    await expect(
      page
        .getByText(/presets? need a look|All clear|check couldn.t finish/)
        .first(),
    ).toBeVisible({ timeout: 900_000 });
    await expect(page.getByText(/check couldn.t finish/)).toHaveCount(0);

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

    // Expanding any measured sound row surfaces the cut-through estimate —
    // `cutThrough` is non-null for every successful guitar capture, so this is
    // deterministic (unlike diagnosis content, which stays unasserted). Scope the
    // click inside the card's first [data-sound-row] header — a page-wide
    // getByText(name) can resolve into advisory-panel body text (see the Edge
    // ring-row click above for the incident).
    await page
      .locator(`[data-preset-card="${picked[1].name}"] [data-sound-row]`)
      .first()
      .getByText(picked[1].name)
      .first()
      .click();
    await expect(
      page.getByText("Cut-through (estimated)").first(),
    ).toBeVisible();

    // Standing re-amp-OFF safety gate — the doctor capture path must disengage re-amp
    // at least as often as it engages (checked before any teardown rescue).
    await expectReampBalanced(page, reampBase);
  });

  // COVERAGE rows 32, 33 (leveling-damage advisories). Deterministic offline:
  // `LevelingDamageRow`'s hints are a BACKUP-SCAN read (zero device captures — see
  // src/views/doctor/LevelingDamageRow.tsx's own header), not diagnosis content, so this is
  // an exception to this file's "diagnosis content is sound-dependent, never asserted
  // directly" rule.
  //
  // MATRIX MISMATCH (row 35, the SNR "some checks skipped" badge): COVERAGE.md lists it
  // Offline, on the premise that 402's "Quiet" scene (sidecar C=-48) reads quiet enough
  // offline to trip the coverage gate. Empirically it doesn't — an actual offline run
  // here renders a normal diagnosis on the Quiet scene ("Gets lost in the mix"), no
  // coverage-gated badge. The cause is NOT a missing C model (the offline capture DOES
  // ride scenario-loudness.json via the same audio::reamp_capture seam): coverage
  // compares the body PSD against the SAME capture's pre-onset floor, and the offline
  // scale_stimulus scales body and preamble uniformly, so coverage is level-invariant
  // offline however quiet the scene. This row is backend/online only.
  test("400 surfaces its two leveling-damage advisories (backup-scan-only, zero captures)", async ({
    page,
  }) => {
    // OFFLINE ONLY (ONLINE e2e consolidation): this row is a backup-scan read with zero
    // device captures (see the header above), so an online run proves nothing an offline
    // run doesn't already — it was only ever incidentally online because this file had
    // no per-test mode split before.
    test.skip(
      await isOnline(page),
      "zero captures — offline proves this identically",
    );
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    await openLevel(page);
    await selectPresetsForCheck(page, [SCENARIO[0]]); // E2E Rig
    await runDoctorCheck(page);
    await expect(
      page.getByText(/presets? need a look|All clear/).first(),
    ).toBeVisible({ timeout: 240_000 });

    const everything = page.getByRole("radio", { name: "Everything" });
    if (await everything.isVisible().catch(() => false)) {
      await everything.click();
    }

    // E2E Rig's card: the "Leveling damage" finding row, its 2-assignment chip, and — once
    // expanded — both signatures' factual copy (VERB KILL's deletedEffect, WAH SWEEP's
    // sweptOther). This is read straight from the backup scan, not from any capture.
    await expect(page.getByText("Leveling damage").first()).toBeVisible();
    await expect(
      page.getByText(/2 assignments worth checking/).first(),
    ).toBeVisible();
    await page.getByText("Leveling damage").first().click();
    await expect(
      page.getByText(/drops to ~0 when engaged — the effect goes silent/),
    ).toBeVisible();
    await expect(
      page.getByText(/isn.t a level control — engaging it changes tone/),
    ).toBeVisible();

    await expectReampBalanced(page, reampBase);
  });
});
