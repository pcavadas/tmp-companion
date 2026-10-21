import { test, expect } from "../fixtures/test";
import {
  BACKUP_SCAN_MS,
  SCENARIO,
  clearScenario,
  ensureScenario,
  invoke,
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

  test("delete/insert/replace across two presets, re-edit shows the saved path, no refetch", async ({
    page,
  }) => {
    test.setTimeout(600_000); // two BACKUP_SCAN_MS waits plus a possible in-test re-seed hold
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
    await expect(place).toBeEnabled({ timeout: BACKUP_SCAN_MS }); // real backup scan settles
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
    await expect(place).toBeEnabled({ timeout: BACKUP_SCAN_MS });
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

    // NOT IMPLEMENTED: a read-back of the saved presets after a reload.

    // ONLINE ONLY: this test's own edits above left T2 (402, E2E Edge) structurally
    // mutated. The next run's pristine-checking seed (e2e_seed.rs's ChainDrift
    // path) restores it; no in-run reseed here — that would duplicate the cost for
    // zero benefit to THIS run.
  });
});

// COVERAGE row 48 — every saved replace reaches the stored copy.
// A replace can be confirmed and show in the working copy while the save stores the old
// block: the unit does that when some other scene has no entry for the replaced node and a
// scene is current [HW, fw 1.8.58]. Copy then makes the replace on base and recalls the
// preset's scene before the save. Each case saves through `copy_apply` and checks the
// STORED copy (field 8), never a reload, since a lost save reloads as defaults.
interface StoredNode {
  FenderId: string;
  nodeId: string;
  dspUnitParameters?: Record<string, unknown>;
}
interface StoredDoc {
  lastLoadedScene?: number;
  audioGraph: { guitarNodes: Record<string, StoredNode[]> };
  scenes?: { guitarNodes?: Record<string, unknown> }[];
}
interface CopyItem {
  outcome: string;
  detail: string;
}

test.describe("Copy — the stored copy shows every saved replace", () => {
  test.afterEach(async ({ page }) => {
    await clearScenario(page);
  });

  const stored = async (page: import("@playwright/test").Page, slot: number) =>
    (
      (await invoke(page, "e2e_read_stored_preset", { slot })) as {
        doc: StoredDoc;
      }
    ).doc;

  const replace = async (
    page: import("@playwright/test").Page,
    slot: number,
    name: string,
    group: string,
    nodeId: string,
    repl: Record<string, unknown>,
  ) => {
    const items = (await invoke(
      page,
      "copy_apply",
      {
        jobs: [
          {
            listIndex: slot,
            name,
            ops: [{ kind: "replace", group, nodeId, repl }],
          },
        ],
        save: true,
        onResult: "__CHANNEL__:1",
      },
      120_000,
    )) as CopyItem[];
    expect(items).toHaveLength(1);
    return items[0];
  };

  test("a replace on a scene preset, regular and through base, is stored", async ({
    page,
  }) => {
    test.setTimeout(600_000);
    await ensureScenario(page);
    const [friedman, hiwatt] = [SCENARIO[10], SCENARIO[4]];
    expect(friedman.name).toBe("E2E Friedman 3S");
    expect(hiwatt.name).toBe("E2E Hiwatt 3S");

    // (1) Regular path: the Plexi has an entry in every scene, so the replace is made in
    // the preset's own scene (1) and stored.
    const one = await replace(
      page,
      friedman.slot,
      friedman.name,
      "G1",
      "ACD_MarshallPlexi",
      {
        kind: "model",
        fenderId: "ACD_TweedDeluxe",
      },
    );
    expect(one.outcome, one.detail).toBe("updated");
    let doc = await stored(page, friedman.slot);
    expect(doc.audioGraph.guitarNodes.G1.map((n) => n.FenderId)).toContain(
      "ACD_TweedDeluxe",
    );
    expect(doc.lastLoadedScene).toBe(1);

    // (2) Through base, on a device-made preset: scene 0 has no entry for the G3 cab
    // (it files it under G4), and the preset opens on scene 3. The replacement is a
    // single cab: the preset already holds two, so a dual one would be over the cap.
    const before = await stored(page, hiwatt.slot);
    expect(before.lastLoadedScene).toBe(3);
    const two = await replace(
      page,
      hiwatt.slot,
      hiwatt.name,
      "G3",
      "ACD_MarG12H30BB",
      {
        kind: "model",
        fenderId: "ACD_Mar412G1265",
      },
    );
    expect(two.outcome, two.detail).toBe("updated");
    doc = await stored(page, hiwatt.slot);
    expect(doc.audioGraph.guitarNodes.G3.map((n) => n.FenderId)).toEqual([
      "ACD_Mar412G1265",
    ]);
    expect(doc.lastLoadedScene).toBe(3);
    expect(
      doc.scenes,
      "the other scenes' entries are left as they were",
    ).toEqual(before.scenes);

    // (3) A saved block through base: cab1 has no entry in scenes 0 and 2. The built-in
    // index-0 cabinet block is on every unit; its cab differs from the fixture's.
    const block = (await invoke(page, "e2e_saved_block", {
      fenderId: "ACD_CabSimTMS",
      index: 0,
    })) as { cab1_id: string } | null;
    expect(
      block,
      "the unit's built-in ACD_CabSimTMS block (index 0) is missing",
    ).not.toBeNull();
    const three = await replace(
      page,
      friedman.slot,
      friedman.name,
      "G1",
      "cab1",
      {
        kind: "saved",
        fenderId: "ACD_CabSimTMS",
        index: 0,
        dualCab: true,
      },
    );
    expect(three.outcome, three.detail).toBe("updated");
    doc = await stored(page, friedman.slot);
    const cab = doc.audioGraph.guitarNodes.G1.find(
      (n) => n.FenderId === "ACD_CabSimTMS",
    );
    expect(cab, "the stored G1 holds the saved cabinet block").toBeDefined();
    const params = cab?.dspUnitParameters ?? {};
    expect(params.cabsimid).toBe(block?.cab1_id);
    expect(params.cabsimid).not.toBe("ACD_Twin212");
    expect(
      typeof params.cabsim2enabled,
      "the stored node carries its dual state",
    ).toBe("boolean");
    expect(doc.lastLoadedScene).toBe(1);
  });
});
