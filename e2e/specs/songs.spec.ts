import { test, expect } from "../fixtures/test";
import { clearScenario } from "../fixtures/scenario";

// Songs scenario — full CRUD, runs identically offline (SimDevice models the song/setlist
// protocol) and online (real device). Self-cleaning: the song and setlist it creates are
// both deleted, so the unit's song DB is net-zero. Every mutation is verified by the
// read-after-write the Songs tab performs.
const SONG = "E2E Song";
const SONG2 = "E2E Song Renamed";
const SETLIST = "E2E Setlist";

const songRow = (page: import("@playwright/test").Page, name: string) =>
  page
    .locator("div")
    .filter({ has: page.getByText(name, { exact: true }) })
    .filter({ has: page.getByTitle("More") })
    .last();

test.describe("Songs — full CRUD (self-cleaning)", () => {
  test.afterEach(async ({ page }) => {
    await clearScenario(page); // leaves the unit on preset 001
  });

  test("create / rename / delete a song and create / delete a setlist", async ({
    page,
  }) => {
    await page.goto("/");
    await page.getByRole("button", { name: /backed up/i }).click(); // startup disclaimer
    await page.getByRole("button", { name: "Songs", exact: true }).click();

    // CREATE song → read-back.
    await page.getByRole("button", { name: "New song" }).click();
    await page.getByPlaceholder("Song name").fill(SONG);
    await page.getByTitle("Save").click();
    await expect(page.getByText(SONG, { exact: true })).toBeVisible({
      timeout: 30_000,
    });

    // UPDATE (rename) → read-back shows the new name, old name gone.
    await songRow(page, SONG).getByTitle("More").click();
    await page.getByText("Edit song…").click();
    await page.getByPlaceholder("Song name").fill(SONG2);
    await page.getByTitle("Save").click();
    await expect(page.getByText(SONG2, { exact: true })).toBeVisible({
      timeout: 30_000,
    });
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
