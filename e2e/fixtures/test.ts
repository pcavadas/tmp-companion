import { expect, test as base } from "@playwright/test";
import { fileURLToPath } from "node:url";
import { PARALLEL_INDEX, PORT, SERVER } from "./port";

// Every spec gets the bridge-client injected before app code (so `isTauri()` is true and
// invoke is forwarded to the e2e_server), and a fresh device fake per test.
const BRIDGE = fileURLToPath(new URL("../bridge-client.js", import.meta.url));

// The worker count the config launched servers for. A CLI `--workers=N` OVERRIDES the
// config value, so a run can be handed a parallel index with no server behind it — which
// surfaces as connection-refused on the first invoke and reads like a product failure.
// Fail loudly and specifically instead.
const WORKERS = Number(process.env.TMP_E2E_WORKERS ?? "3");
if (PARALLEL_INDEX >= WORKERS) {
  const base = PORT - PARALLEL_INDEX;
  throw new Error(
    `e2e worker ${String(PARALLEL_INDEX)} has no server: the config started ` +
      `${String(WORKERS)} e2e_server(s) on ports ${String(base)}..` +
      `${String(base + WORKERS - 1)}. Set TMP_E2E_WORKERS to match --workers rather ` +
      `than overriding only one of them.`,
  );
}

export const test = base.extend({
  page: async ({ page }, use) => {
    // Rebuild this worker's SimDevice from scratch: presets, scenes, songs, setlists, the
    // re-amp latch and any armed capture fault. This — not `clearScenario` — is what
    // isolates one offline test from the next.
    await fetch(`${SERVER}/sim/reset`, { method: "POST" }).catch(() => {
      // best-effort: the online server no-ops /sim/reset; a truly missing server surfaces
      // on the first real invoke, not here.
    });
    // Hand the bridge-client its port BEFORE it loads (init scripts run in order).
    await page.addInitScript(`window.__E2E_PORT__ = ${JSON.stringify(PORT)};`);
    await page.addInitScript({ path: BRIDGE });
    await use(page);
    // #279: a request the offline fake does not model is answered with nothing, so a new
    // product request would otherwise pass here without ever being exercised. Every kind
    // outside the fake's census fails the test that sent it (online: nothing to check).
    // The server answers `/sim/state` in both modes, so a failed read fails the test too.
    const res = await fetch(`${SERVER}/sim/state`);
    expect(res.ok, `GET /sim/state answered ${String(res.status)}`).toBe(true);
    const state = (await res.json()) as {
      online?: boolean;
      unhandled_new?: string[];
    };
    if (state.online === true) return;
    expect(
      state.unhandled_new ?? [],
      "the offline fake answered request kinds it does not model (tms.inner); model them " +
        "in sim_device.rs or add them to UNHANDLED_CENSUS with the reason",
    ).toEqual([]);
  },
});

export { expect };
