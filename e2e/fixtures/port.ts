// The bridge port THIS Playwright worker talks to — the single source of truth, imported
// by `fixtures/test.ts` (which injects it into the page) and `fixtures/scenario.ts` (which
// drives the bridge from Node).
//
// Offline runs N `e2e_server` processes, one per worker: `workers: 1` is for the
// exclusive-seize DEVICE, an ONLINE-only constraint. Offline the only shared thing is one
// server holding one `SimDevice`, so each worker gets its own process.
//
// The index comes from `TEST_PARALLEL_INDEX`, which Playwright sets in every worker process
// (see `playwright/lib/worker/workerProcessEntry.js`). Deliberately NOT a page->port map:
// five specs build teardown pages with `browser.newPage()`, outside the `page` fixture, and
// a per-page map would leave those with no port. A worker-scoped env var is correct for all
// of them, in hooks and test bodies alike.
//
// ONLINE keeps `workers: 1`, so the index is always 0 and the port is exactly TMP_E2E_PORT.
const BASE_PORT = Number(process.env.TMP_E2E_PORT ?? "7600");

/** 0-based index of the Playwright worker running this code. */
export const PARALLEL_INDEX = Number(process.env.TEST_PARALLEL_INDEX ?? "0");

/** Bridge port for this worker's own `e2e_server`. */
export const PORT = BASE_PORT + PARALLEL_INDEX;

/** Bridge base URL for this worker's own `e2e_server`. */
export const SERVER = `http://127.0.0.1:${String(PORT)}`;
