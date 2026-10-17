import type { Locator } from "@playwright/test";

import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  armCaptureDelay,
  clearScenario,
  ensureScenario,
  expectReampBalanced,
  isOnline,
  openLevel,
  reampCounters,
  reampOff,
  runDoctorCheck,
  selectPresetsForCheck,
  waitForCaptureInFlight,
} from "../fixtures/scenario";

// Doctor journey — runs identically offline (fake re-amp: the sim's physics model
// scales the stimulus by each sound's sidecar C, so every sound measures finite —
// levels differ per scene, spectra don't) and online (real re-amp captures,
// ~15 s/sound). The oracle is the FLOW: select → (instrument, when a preset has none
// saved) → check → auto-advance → a Results page that lists every checked preset's sounds,
// with findings or "Sounds good". Diagnosis CONTENT is sound-dependent, so the spec never
// asserts a specific finding; the fix-content regressions (existing-comp advisory, the
// plain-words fixes) are backend-validated in doctor.rs unit tests and the probe/HW lane,
// not here. Doctor never writes to the unit, so there is nothing to read back.
//
// ONLINE e2e suite consolidation (8→4 files): this file's first test below now runs
// OFFLINE ONLY (test.skip'd online) — its online-specific assertions (the E2E Edge
// EQ-ring finding + its notch prescription) moved to `doctor.online.spec.ts`, which runs
// Edge alone online; the proof that the ring stays on its own preset's rows is Vitest's
// (`DoctorResults.test.tsx`, "keeps a ringing finding on its own preset's rows"). The
// offline FLOW proof here (select → check → Results, three presets' rows render) is
// unaffected. It also reads E2E Rig's two footswitch advisories
// off the same Results page — zero device captures (a backup-scan read only), so they need
// no Doctor run of their own.
//
// ONLINE seeding note (kept for the file's own online-tier assertions, if any are ever
// re-added): scripts/e2e.sh seeds the scenario via `probe --seed-scenario` BEFORE the
// server starts (so the server's handshake snapshots the seeded presets) and POSTs
// `e2e_mark_seeded`, which arms the server's verified-seed flag; `ensureScenario` here
// always calls `e2e_seed_scenario` online, which fast-no-ops on that flag and only pays
// the full ownership-verified in-process seed on direct playwright runs (or after a
// clear). If the runner's seed fails all attempts, check nothing else holds the device
// (Pro Control, a stale server/app), then rerun.
/** Open a Results row whichever way the window lays it out: under the row on a narrow
 *  window (`aria-expanded`), or in the panel beside the list on a wide one
 *  (`aria-current`). Playwright's Desktop Chrome viewport is 1280 wide, past
 *  `WIDE_WINDOW`, so this spec runs the wide layout. */
async function openResultRow(button: Locator): Promise<void> {
  if ((await button.getAttribute("aria-expanded")) === "true") return;
  if ((await button.getAttribute("aria-current")) === "true") return;
  await button.click();
}

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
    // This server's store starts empty every run, so Doctor must ask which instrument
    // plays these three presets, and the spec drives the page's "Instrument for all".
    await runDoctorCheck(page, { expectInstrumentPage: true });

    // The check moves on to Results by itself on a natural finish. Progress events don't
    // stream over the bridge, so the only signal is the Results page — but a rejected run
    // (backend/IPC failure) shows "The check couldn't finish" on the Check page within
    // seconds instead (DoctorRun.tsx). That failure is a fast-fail: match it in the wait so
    // a dropped device reports in seconds, then assert it wasn't the branch we took.
    await expect(
      page
        .getByRole("heading", {
          name: /sounds? needs? a look|^Nothing to change in|check couldn.t finish/,
        })
        .first(),
    ).toBeVisible({ timeout: 900_000 });
    await expect(page.getByText(/check couldn.t finish/)).toHaveCount(0);

    // Clean sounds fold into one "N more sound good" row when there are findings above
    // them: open it, so every checked preset's rows show.
    const fold = page.getByRole("button", { name: /^\d+ more sounds? good$/ });
    if (await fold.isVisible().catch(() => false)) {
      await fold.click();
    }

    // Every checked preset renders on Results — a row per sound, flagged or clean.
    for (const p of picked) {
      await expect(
        page.locator("[data-result-row]").filter({ hasText: p.name }).first(),
      ).toBeVisible();
    }

    // An opened measured sound offers "Use as reference" — `cutThrough` is non-null for
    // every successful guitar capture, so this is deterministic (unlike diagnosis content,
    // which stays unasserted). Open E2E Pedalboard's main sound.
    await openResultRow(
      page
        .locator(`[data-result-row^="${String(picked[1].slot)}|"]`)
        .filter({ hasText: "Main sound" })
        .getByRole("button"),
    );
    await expect(
      page.getByRole("button", { name: "Use as reference" }).first(),
    ).toBeVisible();

    // COVERAGE row 21 — E2E Edge's OUT2 footswitch routes off USB 1/2, so its isolated
    // capture is silent. Doctor reports that as the typed no-signal kind, worded by the
    // preset's silence hint, never the generic "Couldn't check this sound." (an untyped
    // analysis error used to land there).
    await expect(
      page
        .locator(`[data-result-row^="${String(picked[2].slot)}|"]`)
        .filter({ hasText: "OUT2" }),
    ).toContainText(
      "Nothing came out of USB 1/2, so there was nothing to hear.",
    );

    // COVERAGE rows 32, 33 (the footswitch advisories), on the same Results page — they
    // used to pay a whole Doctor run of their own on E2E Rig. Deterministic offline: the
    // "Footswitches" row's hints are a BACKUP-SCAN read (zero device captures), not
    // diagnosis content, so this is an exception to this file's "diagnosis content is
    // sound-dependent, never asserted directly" rule. E2E Rig's row says how many switches,
    // and — once opened — both signatures' plain words (VERB KILL's deletedEffect, WAH
    // SWEEP's sweptOther).
    const rigRow = page.locator(
      `[data-result-row="${String(picked[0].slot)}|leveling-damage"]`,
    );
    await expect(
      rigRow.getByText("Footswitches", { exact: true }),
    ).toBeVisible();
    await expect(
      rigRow.getByText("2 footswitches worth checking"),
    ).toBeVisible();
    await openResultRow(rigRow.getByRole("button"));
    const rigDetail = page.locator(
      `[data-result-detail="${String(picked[0].slot)}|leveling-damage"]`,
    );
    await expect(
      rigDetail.getByText(/VERB KILL: Turns the effect almost silent/),
    ).toBeVisible();
    await expect(
      rigDetail.getByText(/WAH SWEEP: Changes the tone, not only the volume/),
    ).toBeVisible();

    // Standing re-amp-OFF safety gate — the doctor capture path must disengage re-amp
    // at least as often as it engages (checked before any teardown rescue).
    await expectReampBalanced(page, reampBase);
  });

  // MATRIX MISMATCH (row 35, the SNR "some checks skipped" badge): COVERAGE.md lists it
  // Offline, on the premise that 402's "Quiet" scene (sidecar C=-48) reads quiet enough
  // offline to trip the coverage gate. Empirically it doesn't — an actual offline run
  // here renders a normal diagnosis on the Quiet scene ("Gets lost in the mix"), no
  // coverage-gated badge. The cause is NOT a missing C model (the offline capture DOES
  // ride scenario-loudness.json via the same audio::reamp_capture seam): coverage
  // compares the body PSD against the SAME capture's pre-onset floor, and the offline
  // scale_stimulus scales body and preamble uniformly, so coverage is level-invariant
  // offline however quiet the scene. This row is backend/online only.

  // COVERAGE row 47 — a mid-run Stop (#190). Every capture is held for a minute, so the
  // only way to reach Results inside the 15 s below is a cancel served WHILE
  // `doctor_check` is still running — the bridge's side lane (`e2e_server.rs`
  // `e2e_is_side_lane`); queued behind the run it would land minutes later. The sounds
  // the check never reached must then read as not checked, never as a 0.0 LUFS
  // measurement, and the page must never claim there is nothing to change in them.
  test("a mid-run Stop reports the unreached sounds as not checked (#190)", async ({
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
    await selectPresetsForCheck(page, [SCENARIO[0]]); // E2E Rig: base + scenes + switches
    await runDoctorCheck(page);
    await waitForCaptureInFlight(page, reampBase);

    await page.getByRole("button", { name: "Stop", exact: true }).click();
    await page
      .getByRole("dialog", { name: "Stop checking?" })
      .getByRole("button", { name: "Stop", exact: true })
      .click();
    // Results open by themselves once the sound in flight winds down — no extra click.
    await expect(
      page.getByRole("heading", {
        name: /^Stopped(\.| before)|sounds? needs? a look/,
      }),
    ).toBeVisible({ timeout: 15_000 });

    await expect(page.getByText("Not checked").first()).toBeVisible();
    await expect(page.locator("body")).not.toContainText(
      /(^|[^\d.])-?0\.0\s*LUFS/,
    );
    // B's all-clear title ("Nothing to change in these N sounds" / "in this sound") is
    // never claimed while a sound went unchecked; the stopped title says it stopped.
    await expect(
      page.getByRole("heading", { name: /^Nothing to change in th(ese|is)/ }),
    ).toHaveCount(0);

    await expectReampBalanced(page, reampBase);
  });
});
