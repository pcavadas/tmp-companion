import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  candidateLabels,
  clearScenario,
  ensureScenario,
  tileLabels,
} from "../fixtures/scenario";

// Copy scenario — runs identically offline (SimDevice + backup fixture) and online (real
// device). Reference = P400; targets = P401 + P402 (all at slots 400/401/402, sharing a
// block so same-block replace always has a candidate). It drives EVERY edit op (delete,
// insert-before, insert-after, replace-different, replace-same), saves edits on TWO presets,
// then re-opens and confirms the edited path carried forward via an in-place cache patch —
// `read_library_via_backup` called exactly once (optimistic update, never a refetch).
const [REF, T1, T2] = SCENARIO;

test.describe("Copy — every edit op, multi-preset save, optimistic cache", () => {
  test.afterEach(async ({ page }) => {
    await clearScenario(page);
  });

  test("delete/insert/replace across two presets, re-edit shows the saved path, no refetch", async ({
    page,
  }) => {
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

    // reference = P400 (.first() = the "Copy from" row); targets = P401 + P402 (.last()).
    await page.getByText(REF.name, { exact: true }).first().click();
    await page.getByText(T1.name, { exact: true }).last().click();
    await page.getByText(T2.name, { exact: true }).last().click();
    const place = page.getByRole("button", { name: /Place the blocks/i });
    await expect(place).toBeEnabled({ timeout: 60_000 }); // real backup scan settles
    await place.click();

    const card1 = `[data-target-card="${T1.name}"]`;
    const tile1 = (i: number) =>
      page.locator(`${card1} [data-block-tile]`).nth(i);

    // ── Round 1 on P401: replace-different · insert-before · insert-after · delete ──
    // (Tap by position + re-read each step; the editor opens in Replace mode by default.)
    const before = await tileLabels(page, T1.name);
    expect(before.length).toBeGreaterThanOrEqual(3); // offline fixture = 3; online clone ≥ 3
    const deleted = before[before.length - 1];

    // replace-different: tile 0 → a candidate whose label differs from tile 0 → verify it
    // actually landed (position 0 changed model) BEFORE the inserts shift positions.
    await tile1(0).click();
    const cands = await candidateLabels(page, T1.name);
    const different = cands.find((c) => c !== before[0]);
    if (!different) throw new Error(`no different candidate vs ${before[0]}`);
    await page.locator(`${card1} [data-candidate="${different}"]`).click();
    await expect
      .poll(() => tileLabels(page, T1.name).then((t) => t[0]))
      .toBe(different);

    // insert-before then insert-after the (new) tile 0
    await tile1(0).click();
    await page
      .getByRole("radio", { name: "Insert before", exact: true })
      .click();
    await page.locator(`${card1} [data-candidate]`).first().click();
    await tile1(0).click();
    await page
      .getByRole("radio", { name: "Insert after", exact: true })
      .click();
    await page.locator(`${card1} [data-candidate]`).first().click();

    // delete the original last block (still uniquely labelled — no op re-added it)
    await page.locator(`${card1} [data-block-tile="${deleted}"]`).click();
    await page.getByRole("button", { name: "Remove", exact: true }).click();

    // ── Round 1 on P402: a second edited preset (one delete) ──
    const card2 = `[data-target-card="${T2.name}"]`;
    const t2 = await tileLabels(page, T2.name);
    await page
      .locator(`${card2} [data-block-tile]`)
      .nth(t2.length - 1)
      .click();
    await page.getByRole("button", { name: "Remove", exact: true }).click();

    // Save both presets.
    await page.getByText(/backed up with Pro Control/i).click();
    await page.getByRole("button", { name: "Save to the unit" }).click();
    await expect(page.getByText("Saved to the unit.")).toBeVisible({
      timeout: 30_000,
    });
    await page.getByRole("button", { name: "Done" }).click();

    // ── Round 2: re-open; P401 reflects round 1 (cache patched, never refetched) ──
    await expect(place).toBeEnabled({ timeout: 60_000 });
    await place.click();
    const after = await tileLabels(page, T1.name);
    expect(after).not.toContain(deleted); // the deleted block stayed gone
    // net block-count change carried forward: −1 delete + 2 inserts = +1 vs the original.
    expect(after.length).toBe(before.length + 1);

    // replace-SAME: tile 0 (an inserted candidate) → the candidate with its own label — a
    // real re-stamp op that leaves the model unchanged.
    await tile1(0).click();
    const cands2 = await candidateLabels(page, T1.name);
    const same = cands2.includes(after[0]) ? after[0] : cands2[0];
    await page.locator(`${card1} [data-candidate="${same}"]`).click();

    await page.getByText(/backed up with Pro Control/i).click();
    await page.getByRole("button", { name: "Save to the unit" }).click();
    await expect(page.getByText("Saved to the unit.")).toBeVisible({
      timeout: 30_000,
    });

    // The heavy library backup was read exactly once — every edit patched the cache.
    expect(backupCalls).toBe(1);
  });
});
