import { test as base } from "@playwright/test";
import { fileURLToPath } from "node:url";

// Every spec gets the bridge-client injected before app code (so `isTauri()` is true and
// invoke is forwarded to the e2e_server), and a fresh device fake per test.
const BRIDGE = fileURLToPath(new URL("../bridge-client.js", import.meta.url));

export const test = base.extend({
  page: async ({ page }, use) => {
    await fetch("http://127.0.0.1:7600/sim/reset", { method: "POST" }).catch(() => {
      // best-effort: the online server no-ops /sim/reset; a truly missing server surfaces
      // on the first real invoke, not here.
    });
    await page.addInitScript({ path: BRIDGE });
    await use(page);
  },
});

export { expect } from "@playwright/test";
