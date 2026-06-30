import { test, expect } from "../fixtures/test";
import { fileURLToPath } from "node:url";

// Marketing-screenshot tour (NOT a gate): drives the REAL React UI against the curated,
// non-personal showcase library and captures each tab to docs/assets/. Every step waits on
// a CONCRETE populated selector before capturing so a loading skeleton fails the run
// instead of shipping a blank frame. One command: `bun run screenshots`.
//
// The headless capture has no native window chrome, so each captured tab is composited into
// a macOS window (traffic lights + title bar + rounded corners + drop shadow on a
// transparent margin) in a second pass at the end — done in-page via setContent so it needs
// no image-processing dependency. The site's screenshot frame is a transparent passthrough,
// so the window "floats" on the page.
const ASSETS = (name: string) =>
  fileURLToPath(new URL(`../../docs/assets/${name}.png`, import.meta.url));

// One captured tab, awaiting its macOS-window frame.
interface Shot {
  name: string;
  buf: Buffer;
}

// Wrap a captured tab (a data-URL <img>) in a macOS "Liquid Glass" window and screenshot it.
// The 900×680 content is the app's native window size; the 36px frosted title bar + rounded
// corners + soft shadow sit on a transparent 72px margin (captured via omitBackground) so the
// window floats. (If you retune the margin, update the site's 404-fallback aspect-ratio in
// docs/index.html `.shot:has(.placeholder:only-child)`.)
async function frameWindow(
  page: import("@playwright/test").Page,
  buf: Buffer,
): Promise<void> {
  const dataUrl = `data:image/png;base64,${buf.toString("base64")}`;
  // Flat traffic light — solid fill, no gloss or ring.
  const dot = (c: string) =>
    `<span style="width:12px;height:12px;border-radius:50%;background:${c}"></span>`;
  await page.setContent(
    `<!doctype html><meta charset="utf-8">
     <div id="wrap" style="display:inline-block;padding:72px;background:transparent">
       <div style="width:900px;border-radius:18px;overflow:hidden;background:#fff;
                   box-shadow:0 36px 80px -28px rgba(15,17,21,.30),
                              0 12px 28px -14px rgba(15,17,21,.16),
                              0 0 0 .5px rgba(15,17,21,.06),
                              inset 0 .5px 0 rgba(255,255,255,.8)">
         <!-- frosted glass title bar: translucent light gradient + top sheen, unified
              (no hard divider) for the Liquid Glass look -->
         <div style="height:36px;display:flex;align-items:center;gap:9px;padding:0 16px;
                     background:linear-gradient(180deg,rgba(252,252,253,.92),rgba(244,244,247,.78));
                     box-shadow:inset 0 .5px 0 rgba(255,255,255,.9),
                                inset 0 -.5px 0 rgba(15,17,21,.05)">
           ${dot("#ff5f57")}${dot("#febc2e")}${dot("#28c840")}
           <span style="flex:1;text-align:center;margin-left:-39px;
                        font:590 12.5px -apple-system,system-ui,sans-serif;
                        letter-spacing:.01em;color:#9296a0">
             TMP Companion
           </span>
         </div>
         <img id="ss" src="${dataUrl}" style="display:block;width:900px;height:680px">
       </div>
     </div>`,
    { waitUntil: "load" },
  );
  // Guarantee the data-URL image is decoded + paintable before the frame screenshot.
  await page.locator("#ss").evaluate((img: HTMLImageElement) => img.decode());
}

test.describe.configure({ timeout: 120_000 });

test.describe("Marketing screenshots", () => {
  test("capture Level, Copy, Songs, Catalog", async ({ page }) => {
    const shots: Shot[] = [];
    const grab = async (name: string) => {
      shots.push({
        name,
        buf: await page.screenshot({ animations: "disabled" }),
      });
    };

    await page.goto("/");
    await page.getByRole("button", { name: /backed up/i }).click(); // startup disclaimer
    await expect(page.getByText(/connected · \d+\.\d+/)).toBeVisible({
      timeout: 30_000,
    });

    // ── Level: the hero shows the active "Verse — Split" dual-split chain (the hero name
    // is `graph.name`), and the list populates with the curated presets. (The hero strip
    // is non-interactive, so it has no `data-block-tile` hook — gate on the name instead.)
    await expect(page.getByText(/Verse/).first()).toBeVisible({
      timeout: 30_000,
    });
    await expect(
      page.getByText("Stadium Lead", { exact: true }).first(),
    ).toBeVisible();
    await grab("level");

    // ── Copy: pick a reference + two targets, place the blocks, wait for the per-target
    // signal paths to render their tiles.
    await page.getByRole("button", { name: "Copy", exact: true }).click();
    await page.getByText("Stadium Lead", { exact: true }).first().click();
    await page.getByText("Brit Combo 65", { exact: true }).last().click();
    await page.getByText("Worship Swell", { exact: true }).last().click();
    const place = page.getByRole("button", { name: /Place the blocks/i });
    await expect(place).toBeEnabled({ timeout: 60_000 }); // backup scan settles
    await place.click();
    await expect(
      page
        .locator('[data-target-card="Brit Combo 65"] [data-block-tile]')
        .first(),
    ).toBeVisible({ timeout: 30_000 });
    await grab("copy");

    // ── Songs: the curated song list (live read-back from the seeded SimDevice).
    await page.getByRole("button", { name: "Songs", exact: true }).click();
    await expect(page.getByText("Higher Ground", { exact: true })).toBeVisible({
      timeout: 30_000,
    });
    await grab("songs");

    // ── Catalog: device-independent model wall (bundled guide, renders synchronously).
    await page.getByRole("button", { name: "Catalog", exact: true }).click();
    await expect(page.getByPlaceholder(/Search a model/i)).toBeVisible({
      timeout: 30_000,
    });
    await expect(page.locator('[title*="—"]').first()).toBeVisible(); // a model card
    await grab("catalog");

    // ── Frame pass: composite each captured tab into a floating macOS window. Done last
    // because setContent replaces the app page (so it can't run mid-tour).
    for (const s of shots) {
      await frameWindow(page, s.buf);
      await page
        .locator("#wrap")
        .screenshot({ path: ASSETS(s.name), omitBackground: true });
    }
  });
});
