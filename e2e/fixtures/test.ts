import { test as base } from "@playwright/test";
import { fileURLToPath } from "node:url";

// Every spec gets the bridge-client injected before app code (so `isTauri()` is true and
// invoke is forwarded to the e2e_server), and a fresh device fake per test.
const BRIDGE = fileURLToPath(new URL("../bridge-client.js", import.meta.url));
// Per-worktree bridge port (scripts/e2e.sh exports TMP_E2E_PORT); default 7600.
const PORT = process.env.TMP_E2E_PORT ?? "7600";

export const test = base.extend({
  page: async ({ page }, use) => {
    await fetch(`http://127.0.0.1:${PORT}/sim/reset`, { method: "POST" }).catch(
      () => {
        // best-effort: the online server no-ops /sim/reset; a truly missing server surfaces
        // on the first real invoke, not here.
      },
    );
    // Hand the bridge-client its port BEFORE it loads (init scripts run in order).
    await page.addInitScript(`window.__E2E_PORT__ = ${JSON.stringify(PORT)};`);
    await page.addInitScript({ path: BRIDGE });
    await use(page);
  },
});

export { expect } from "@playwright/test";
