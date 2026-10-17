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
// The toast a save shows when its tempo didn't land. Matched EXACTLY: a pattern for
// wording the app no longer uses would match nothing and pass without testing anything.
// It is counted ONCE, with no retry, right after the song's BPM row shows: the toast and
// the row render together, and the toast hides itself after about 4 s, so a retrying
// `toHaveCount(0)` would wait it out and pass even when it showed.
const TEMPO_TOAST = "Song saved, but its tempo didn't. Set it again.";

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

// A song's row line: the innermost div holding its ⋯ ("More for {name}").
const moreFor = (page: import("@playwright/test").Page, name: string) =>
  page.getByRole("button", { name: `More for ${name}`, exact: true });
const songRow = (page: import("@playwright/test").Page, name: string) =>
  page
    .locator("div")
    .filter({ has: moreFor(page, name) })
    .last();

test.describe("Songs — full CRUD (self-cleaning)", () => {
  test.afterEach(async ({ page }) => {
    await removeTestSetlists(page);
    await clearScenario(page); // leaves the unit on preset 001
  });

  // #295 rides along: tabs unmount on switch, and the Songs tab repaints from the startup
  // library scan. A setlist renamed here must come back under its NEW name after a tab
  // round-trip — a stale old name sits on a slot that every later setlist write would then
  // address. (It was its own test, which paid a second page load and library scan.)
  test("create / rename / delete a song; create / rename / delete a setlist that keeps its new name across a tab switch", async ({
    page,
  }) => {
    await page.goto("/");
    await page.getByRole("button", { name: /backed up/i }).click(); // startup disclaimer
    await page.getByRole("button", { name: "Songs", exact: true }).click();

    // CREATE song with a BPM → read-back shows the BPM, with no "tempo didn't" toast.
    await page.getByRole("button", { name: "New song" }).click();
    await page.getByLabel("Song name").fill(SONG);
    await page.getByLabel("Tempo").fill(BPM);
    await page.getByRole("button", { name: "Save song" }).click();
    await expect(page.getByText(SONG, { exact: true })).toBeVisible({
      timeout: 30_000,
    });
    await expect(
      songRow(page, SONG).getByText(`${BPM} BPM`, { exact: true }),
    ).toBeVisible({ timeout: 30_000 });
    expect(await page.getByText(TEMPO_TOAST, { exact: true }).count()).toBe(0);

    // UPDATE (rename + BPM) → read-back shows the new name and BPM, old name gone.
    await moreFor(page, SONG).click();
    await page.getByRole("menuitem", { name: "Edit song" }).click();
    await page.getByLabel("Song name").fill(SONG2);
    await page.getByLabel("Tempo").fill(BPM2);
    await page.getByRole("button", { name: "Save song" }).click();
    await expect(page.getByText(SONG2, { exact: true })).toBeVisible({
      timeout: 30_000,
    });
    await expect(
      songRow(page, SONG2).getByText(`${BPM2} BPM`, { exact: true }),
    ).toBeVisible({ timeout: 30_000 });
    expect(await page.getByText(TEMPO_TOAST, { exact: true }).count()).toBe(0);
    // …and the old name is GONE — a rename that appended instead of replacing would leave
    // both visible (and break net-zero), yet pass a new-name-visible check alone.
    await expect(page.getByText(SONG, { exact: true })).toHaveCount(0, {
      timeout: 30_000,
    });

    // CREATE setlist → read-back.
    await page
      .getByRole("button", { name: "New setlist", exact: true })
      .click();
    await page.getByPlaceholder("Setlist name").fill(SETLIST);
    await page.getByRole("button", { name: "Create setlist" }).click();
    await expect(page.getByText(SETLIST, { exact: true })).toBeVisible({
      timeout: 30_000,
    });

    // RENAME the setlist from its page title (a click on it renames). Selecting reads
    // the setlist's membership ("reading this setlist…" = busy) — wait for it first.
    await page.getByText(SETLIST, { exact: true }).click();
    await expect(page.getByText(/reading this setlist/i)).toHaveCount(0, {
      timeout: 30_000,
    });
    await page
      .getByRole("heading", { name: SETLIST, exact: true })
      .getByRole("button")
      .click();
    // The title editor opens focused on the current name.
    const title = page.locator("input:focus");
    await title.fill(SETLIST_NEW);
    await title.press("Enter");
    await expect(
      page.getByText(SETLIST_NEW, { exact: true }).first(),
    ).toBeVisible({
      timeout: 30_000,
    });

    // Leave the tab and come back: the rail shows the new name, not the scan's (#295).
    await page.getByRole("button", { name: "Copy", exact: true }).click();
    await page.getByRole("button", { name: "Songs", exact: true }).click();
    await expect(page.getByText(SETLIST_NEW, { exact: true })).toBeVisible({
      timeout: 30_000,
    });
    await expect(page.getByText(SETLIST, { exact: true })).toHaveCount(0);

    // DELETE setlist (open it → options → Delete setlist → confirm) — cleanup, and the
    // guarded delete must accept the CURRENT name.
    await page.getByText(SETLIST_NEW, { exact: true }).click();
    // The options button is disabled while the membership read is busy.
    await expect(page.getByText(/reading this setlist/i)).toHaveCount(0, {
      timeout: 30_000,
    });
    await page.getByRole("button", { name: "Setlist options" }).click();
    await page.getByRole("menuitem", { name: "Delete setlist" }).click();
    // The confirm names the action; its backdrop does nothing.
    await page.getByRole("button", { name: "Delete setlist" }).click();
    await expect(page.getByText(SETLIST_NEW, { exact: true })).toHaveCount(0, {
      timeout: 30_000,
    });

    // DELETE song — cleanup → read-back gone (net-zero).
    await moreFor(page, SONG2).click();
    await page.getByRole("menuitem", { name: "Delete song" }).click();
    await page.getByRole("button", { name: "Delete song" }).click();
    await expect(page.getByText(SONG2, { exact: true })).toHaveCount(0, {
      timeout: 30_000,
    });
  });
});
