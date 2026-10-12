import { test, expect } from "../fixtures/test";
import { clearScenario, invoke } from "../fixtures/scenario";

// Songs scenario — full CRUD plus BPM, runs identically offline (SimDevice models the song/setlist
// protocol) and online (real device). Self-cleaning: the song and setlist it creates are
// both deleted, so the unit's song DB is net-zero. Every mutation is verified by the
// read-after-write the Songs tab performs.
const SONG = "E2E Song";
const SONG2 = "E2E Song Renamed";
// Setlist names carry a per-run tag, so the cleanup below can only ever match setlists
// this run created — never one already on the unit that happens to share a fixed name.
const RUN = Date.now().toString(36).slice(-4);
const SETLIST = `E2E Set ${RUN}`;
const SETLIST_NEW = `E2E Set ${RUN} R2`;
// BPM has no setter: it is the tap tempo, which lands in the active song only once its
// BPM flag is on (fw 1.8.58). A new song has no footswitch and its flag off, so the
// create exercises the fresh-assign path; the edit, the existing-binding path.
const BPM = "97";
const BPM2 = "132";

// This run's setlists — swept in afterEach, so a failed assertion can't strand one on
// the unit (`clearScenario` doesn't touch setlists). Removed highest slot first, each
// guarded by its name and the list's size, so a removal never shifts a slot still to come.
const TEST_SETLISTS = [SETLIST, SETLIST_NEW];
async function removeTestSetlists(
  page: import("@playwright/test").Page,
): Promise<void> {
  const lists = (await invoke(page, "read_setlists")) as {
    slot: number;
    name: string;
  }[];
  const ours = lists
    .filter((l) => TEST_SETLISTS.includes(l.name))
    .sort((a, b) => b.slot - a.slot);
  // Each delete is guarded by the whole list as it then reads, in order.
  const seen = lists.map((l) => l.name);
  for (const l of ours) {
    await invoke(page, "remove_setlist", {
      slot: l.slot,
      expectName: l.name,
      expectList: [...seen],
    });
    seen.splice(l.slot - 1, 1);
  }
}

const songRow = (page: import("@playwright/test").Page, name: string) =>
  page
    .locator("div")
    .filter({ has: page.getByText(name, { exact: true }) })
    .filter({ has: page.getByTitle("More") })
    .last();

test.describe("Songs — full CRUD (self-cleaning)", () => {
  test.afterEach(async ({ page }) => {
    await removeTestSetlists(page);
    await clearScenario(page); // leaves the unit on preset 001
  });

  test("create / rename / delete a song and create / delete a setlist", async ({
    page,
  }) => {
    await page.goto("/");
    await page.getByRole("button", { name: /backed up/i }).click(); // startup disclaimer
    await page.getByRole("button", { name: "Songs", exact: true }).click();

    // CREATE song with a BPM → read-back shows the BPM, with no "didn't stick" warning.
    await page.getByRole("button", { name: "New song" }).click();
    await page.getByPlaceholder("Song name").fill(SONG);
    await page.getByPlaceholder("—", { exact: true }).fill(BPM);
    await page.getByTitle("Save").click();
    await expect(page.getByText(SONG, { exact: true })).toBeVisible({
      timeout: 30_000,
    });
    await expect(
      songRow(page, SONG).getByText(BPM, { exact: true }),
    ).toBeVisible({ timeout: 30_000 });
    await expect(page.getByText(/BPM didn't stick/)).toHaveCount(0);

    // UPDATE (rename + BPM) → read-back shows the new name and BPM, old name gone.
    await songRow(page, SONG).getByTitle("More").click();
    await page.getByText("Edit song…").click();
    await page.getByPlaceholder("Song name").fill(SONG2);
    await page.getByPlaceholder("—", { exact: true }).fill(BPM2);
    await page.getByTitle("Save").click();
    await expect(page.getByText(SONG2, { exact: true })).toBeVisible({
      timeout: 30_000,
    });
    await expect(
      songRow(page, SONG2).getByText(BPM2, { exact: true }),
    ).toBeVisible({ timeout: 30_000 });
    await expect(page.getByText(/BPM didn't stick/)).toHaveCount(0);
    // …and the old name is GONE — a rename that appended instead of replacing would leave
    // both visible (and break net-zero), yet pass a new-name-visible check alone.
    await expect(page.getByText(SONG, { exact: true })).toHaveCount(0, {
      timeout: 30_000,
    });

    // CREATE setlist → read-back.
    await page.getByRole("button", { name: "New setlist" }).click();
    await page.getByPlaceholder("Name this setlist").fill(SETLIST);
    await page.getByTitle("Create").click();
    await expect(page.getByText(SETLIST, { exact: true })).toBeVisible({
      timeout: 30_000,
    });

    // DELETE setlist (open it → options → Delete setlist → confirm) — cleanup.
    await page.getByText(SETLIST, { exact: true }).click();
    // Selecting reads the setlist's membership ("reading this setlist…" = busy, which
    // disables the options button) — wait for it to settle before opening the menu.
    await expect(page.getByText(/reading this setlist/i)).toHaveCount(0, {
      timeout: 30_000,
    });
    await page.getByTitle("Setlist options").click();
    await page.getByText(/Delete setlist/).click();
    await page.getByRole("button", { name: "Delete", exact: true }).click();
    await expect(page.getByText(SETLIST, { exact: true })).toHaveCount(0, {
      timeout: 30_000,
    });

    // DELETE song — cleanup → read-back gone (net-zero).
    await songRow(page, SONG2).getByTitle("More").click();
    await page.getByText(/Delete song/).click();
    await page.getByRole("button", { name: "Delete", exact: true }).click();
    await expect(page.getByText(SONG2, { exact: true })).toHaveCount(0, {
      timeout: 30_000,
    });
  });
});

// #295: tabs unmount on switch, and the Songs tab repaints from the startup library scan.
// A setlist renamed here must come back under its NEW name after a tab round-trip — a
// stale old name sits on a slot that every later setlist write would then address.

test.describe("Songs — edits survive a tab switch (self-cleaning)", () => {
  test.afterEach(async ({ page }) => {
    await removeTestSetlists(page);
    await clearScenario(page);
  });

  test("a renamed setlist keeps its new name after leaving and returning to Songs", async ({
    page,
  }) => {
    await page.goto("/");
    await page.getByRole("button", { name: /backed up/i }).click(); // startup disclaimer
    await page.getByRole("button", { name: "Songs", exact: true }).click();

    await page.getByRole("button", { name: "New setlist" }).click();
    await page.getByPlaceholder("Name this setlist").fill(SETLIST);
    await page.getByTitle("Create").click();
    await expect(page.getByText(SETLIST, { exact: true })).toBeVisible({
      timeout: 30_000,
    });

    // RENAME it from its detail title.
    await page.getByText(SETLIST, { exact: true }).click();
    await expect(page.getByText(/reading this setlist/i)).toHaveCount(0, {
      timeout: 30_000,
    });
    await page.getByTitle("Rename").click();
    // The title editor opens focused on the current name.
    const title = page.locator("input:focus");
    await title.fill(SETLIST_NEW);
    await title.press("Enter");
    await expect(
      page.getByText(SETLIST_NEW, { exact: true }).first(),
    ).toBeVisible({
      timeout: 30_000,
    });

    // Leave the tab and come back: the rail shows the new name, not the scan's.
    await page.getByRole("button", { name: "Copy", exact: true }).click();
    await page.getByRole("button", { name: "Songs", exact: true }).click();
    await expect(page.getByText(SETLIST_NEW, { exact: true })).toBeVisible({
      timeout: 30_000,
    });
    await expect(page.getByText(SETLIST, { exact: true })).toHaveCount(0);

    // DELETE it — cleanup, and the guarded delete must accept the CURRENT name.
    await page.getByText(SETLIST_NEW, { exact: true }).click();
    await expect(page.getByText(/reading this setlist/i)).toHaveCount(0, {
      timeout: 30_000,
    });
    await page.getByTitle("Setlist options").click();
    await page.getByText(/Delete setlist/).click();
    await page.getByRole("button", { name: "Delete", exact: true }).click();
    await expect(page.getByText(SETLIST_NEW, { exact: true })).toHaveCount(0, {
      timeout: 30_000,
    });
  });
});
