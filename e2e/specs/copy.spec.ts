import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  clearScenario,
  ensureScenario,
  sourceLabels,
  tileLabels,
} from "../fixtures/scenario";

// COVERAGE row 38 — import / copy chain.
// Copy scenario — runs identically offline (SimDevice + backup fixture) and online (real
// device). Source = P400; targets = P401 + P402 (all at slots 400/401/402, sharing a
// block so same-block replace always has a candidate). It drives EVERY edit op the look B
// page offers — choose a block on the source strip, then: delete (a block's menu),
// insert-before (a leading "+" gap), insert-after (the trailing "+" gap), replace-different
// and replace-same (a block's menu) — saves edits on TWO presets, then re-opens and
// confirms the edited path carried forward via an in-place cache patch —
// `read_library_via_backup` called exactly once (optimistic update, never a refetch).
const [REF, T1, T2] = SCENARIO;

test.describe("Copy — every edit op, multi-preset save, optimistic cache", () => {
  test.afterEach(async ({ page }) => {
    await clearScenario(page);
  });

  // The startup backup read streams at ~27 KiB/s whatever the library holds, so its length is
  // the archive's, not the preset count's: 2.2 MiB took 85 s (HW 2026-09-02, fw 1.8.45).
  const SCAN_MS = 180_000;

  test("delete/insert/replace across two presets, re-edit shows the saved path, no refetch", async ({
    page,
  }) => {
    test.setTimeout(600_000); // two SCAN_MS waits plus a possible in-test re-seed hold
    await ensureScenario(page);

    let backupCalls = 0;
    page.on("request", (r) => {
      if (
        r.url().endsWith("/invoke") &&
        (r.postData() ?? "").includes("read_library_via_backup")
      ) {
        backupCalls += 1;
      }
    });

    await page.goto("/");
    await page.getByRole("button", { name: /backed up/i }).click(); // startup disclaimer
    await page.getByRole("button", { name: "Copy", exact: true }).click();

    // source = P400 (.first() = the "Copy from" row); targets = P401 + P402 (.last()).
    await page.getByText(REF.name, { exact: true }).first().click();
    await page.getByText(T1.name, { exact: true }).last().click();
    await page.getByText(T2.name, { exact: true }).last().click();
    const place = page.getByRole("button", { name: "Choose blocks" });
    await expect(place).toBeEnabled({ timeout: SCAN_MS }); // real backup scan settles
    await place.click();

    const card1 = `[data-target-card="${T1.name}"]`;
    const tile1 = (i: number) =>
      page.locator(`${card1} [data-block-tile]`).nth(i);
    const menuItem = (name: string | RegExp) =>
      page.getByRole("menuitem", { name });

    // ── Round 1 on P401: replace-different · insert-before · insert-after · delete ──
    // (Tap by position + re-read each step.)
    const before = await tileLabels(page, T1.name);
    expect(before.length).toBeGreaterThanOrEqual(3); // offline fixture = 3; online clone ≥ 3
    const deleted = before[before.length - 1];

    // Choose the block to copy on the source strip: one whose model differs from tile 0
    // (so the replace is visible) and from the block deleted below (so that one stays
    // uniquely labelled). It stays chosen for every placement that follows.
    const sources = await sourceLabels(page);
    const different = sources.find((c) => c !== before[0] && c !== deleted);
    if (!different) throw new Error(`no source block unlike ${before[0]}`);
    await page
      .locator(`[data-source-card] [data-block-tile="${different}"]`)
      .first()
      .click();

    // replace-different: tile 0's menu → "Replace with …" → verify it actually landed
    // (position 0 changed model) BEFORE the inserts shift positions.
    await tile1(0).click();
    await menuItem(/^Replace with /).click();
    await expect
      .poll(() => tileLabels(page, T1.name).then((t) => t[0]))
      .toBe(different);

    // insert-before (the gap ahead of tile 0) then insert-after (the gap after the last
    // block of the chain).
    await page.locator(`${card1} [data-gap="before"]`).first().click();
    await page.locator(`${card1} [data-gap="after"]`).last().click();

    // delete the original last block (still uniquely labelled — no op re-added it)
    await page.locator(`${card1} [data-block-tile="${deleted}"]`).click();
    await menuItem("Remove").click();

    // ── Round 1 on P402: a second edited preset (one delete) ──
    const card2 = `[data-target-card="${T2.name}"]`;
    const t2 = await tileLabels(page, T2.name);
    await page
      .locator(`${card2} [data-block-tile]`)
      .nth(t2.length - 1)
      .click();
    await menuItem("Remove").click();

    // Save both presets.
    await page.getByText(/backed up with Pro Control/i).click();
    await page
      .getByRole("button", { name: "Save to your Tone Master Pro" })
      .click();
    await expect(page.getByText("Saved to your Tone Master Pro")).toBeVisible({
      timeout: 30_000,
    });
    await page.getByRole("button", { name: "Done", exact: true }).click();

    // ── Round 2: re-open; P401 reflects round 1 (cache patched, never refetched) ──
    await expect(place).toBeEnabled({ timeout: SCAN_MS });
    await place.click();
    const after = await tileLabels(page, T1.name);
    expect(after).not.toContain(deleted); // the deleted block stayed gone
    // net block-count change carried forward: −1 delete + 2 inserts = +1 vs the original.
    expect(after.length).toBe(before.length + 1);

    // replace-SAME: choose the source block with tile 0's own model (tile 0 is the block
    // inserted from the source), then tile 0's menu → Replace — a real re-stamp op that
    // leaves the model unchanged.
    const sources2 = await sourceLabels(page);
    const same = sources2.includes(after[0]) ? after[0] : sources2[0];
    await page
      .locator(`[data-source-card] [data-block-tile="${same}"]`)
      .first()
      .click();
    await tile1(0).click();
    await menuItem(/^Replace with /).click();

    await page.getByText(/backed up with Pro Control/i).click();
    await page
      .getByRole("button", { name: "Save to your Tone Master Pro" })
      .click();
    await expect(page.getByText("Saved to your Tone Master Pro")).toBeVisible({
      timeout: 30_000,
    });

    // The heavy library backup was read exactly once — every edit patched the cache.
    expect(backupCalls).toBe(1);

    // NOT IMPLEMENTED: a read-back of the saved presets ships as a separately-designed
    // follow-up (notes/user-journeys.md's Copy registry).

    // ONLINE ONLY: this test's own edits above left T2 (402, E2E Edge) structurally
    // mutated. The next run's pristine-checking seed (e2e_seed.rs's ChainDrift
    // path) restores it; no in-run reseed here — that would duplicate the cost for
    // zero benefit to THIS run.
  });
});
