import { defineConfig, devices } from "@playwright/test";

// Ports default to 7600/1421 but scripts/e2e.sh derives a per-worktree pair (exported as
// TMP_E2E_PORT / TMP_E2E_VITE_PORT) so parallel runs in sibling worktrees don't collide.
const PORT = Number(process.env.TMP_E2E_PORT ?? "7600");
const VITE = process.env.TMP_E2E_VITE_PORT ?? "1421";

// One `e2e_server` PROCESS per worker, on PORT+i — `fixtures/port.ts` picks the matching
// one from TEST_PARALLEL_INDEX. `workers: 1` was never about the fake: it is the exclusive
// -seize DEVICE, an ONLINE-only constraint (the online config keeps it). Offline the only
// shared state is one server's `SimDevice`, and the remaining work is CPU-bound, so extra
// processes convert directly into wall-clock.
const WORKERS = Number(process.env.TMP_E2E_WORKERS ?? "3");

// SOTA dual-mode e2e: the same specs drive the REAL React UI in Chromium against the
// real Rust backend. Offline (here) fakes only the USB transport (SimDevice) + the
// startup snapshot via the windowless `e2e_server`. Online (Slice 2) points the bridge
// at a server opened against the real device. Device is exclusive-seize → `workers: 1`.
export default defineConfig({
  testDir: "./specs",
  // Two spec files are online-only ORACLES in their entirety — `*.online.spec.ts` needs a
  // real device, and level-strict needs real audio (the offline capture is a stimulus
  // passthrough). Offline they can only ever `test.skip(...)`, but the skip is decided
  // INSIDE the test body, so Playwright had already run the file's fixtures and hooks —
  // ~57 s of CI time to reach a foregone skip. Not running them is the same coverage.
  // OFFLINE ONLY: the online config must keep collecting them, they are its whole point.
  testIgnore: ["**/*.online.spec.ts", "**/level-strict.spec.ts"],
  // File-level parallelism only: tests inside one file keep running in order, in one
  // worker, because several describes share state through `afterAll`.
  fullyParallel: false,
  workers: WORKERS,
  forbidOnly: !!process.env.CI,
  reporter: process.env.CI ? "line" : [["list"]],
  // The default per-test timeout is 30 s, but the songs/level CRUD flows run ~20-25 s locally
  // and comfortably exceed 30 s on a loaded CI runner (2-4x slower), so the whole test is
  // killed mid-flight ("Test timeout of 30000ms exceeded") even though every assertion set a
  // 30-240 s timeout — those are dead under a tighter test cap.
  //
  // KEEP THIS EQUAL TO THE ONLINE CAP. `testDir` is the SAME ./specs in both configs, so one
  // spec set cannot carry two budgets: the eleven 240 s waits the shared specs declare are
  // only reachable under a cap ≥ 300 s, and a tighter cap silently truncates them instead of
  // failing the assertion they belong to. A 120 s cap did exactly that to the heaviest offline
  // test (level-defaults.spec.ts "reachable-common-target fallback" — TWO full 2-preset level
  // runs), which measured 133 s → 150 s begin-to-begin across CI runners and so died mid-run
  // on whichever runner drew the slow straw, reporting a timeout on unrelated PRs.
  timeout: 300_000,
  use: {
    baseURL: `http://localhost:${VITE}`,
    trace: "on-first-retry",
  },
  projects: [{ name: "offline", use: { ...devices["Desktop Chrome"] } }],
  // Vite serves the real frontend (ONE instance — it is a static asset server, workers just
  // fetch from it); each worker gets its own Rust e2e_server running the real commands.
  webServer: [
    {
      command: "bun run dev",
      url: `http://localhost:${VITE}`,
      reuseExistingServer: !process.env.CI,
      timeout: 120_000,
    },
    // The PREBUILT binary, not `cargo run`: N concurrent `cargo run`s serialize on the
    // cargo target-dir lock at startup, and even one paid a ~17 s recompile inside the
    // timed server-start window. `scripts/e2e.sh` always builds first (so does CI).
    // Running `bunx playwright test` directly after a Rust edit therefore uses the last
    // build — run `scripts/e2e.sh` (or `cargo build --features e2e --bin e2e_server`) to
    // pick up backend changes.
    ...Array.from({ length: WORKERS }, (_, i) => ({
      command: "../src-tauri/target/debug/e2e_server",
      env: { TMP_E2E_PORT: String(PORT + i) },
      url: `http://127.0.0.1:${String(PORT + i)}/health`,
      reuseExistingServer: !process.env.CI,
      timeout: 180_000,
    })),
  ],
});
