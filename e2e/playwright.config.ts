import { defineConfig, devices } from "@playwright/test";

// Ports default to 7600/1421 but scripts/e2e.sh derives a per-worktree pair (exported as
// TMP_E2E_PORT / TMP_E2E_VITE_PORT) so parallel runs in sibling worktrees don't collide.
const PORT = process.env.TMP_E2E_PORT ?? "7600";
const VITE = process.env.TMP_E2E_VITE_PORT ?? "1421";

// SOTA dual-mode e2e: the same specs drive the REAL React UI in Chromium against the
// real Rust backend. Offline (here) fakes only the USB transport (SimDevice) + the
// startup snapshot via the windowless `e2e_server`. Online (Slice 2) points the bridge
// at a server opened against the real device. Device is exclusive-seize → `workers: 1`.
export default defineConfig({
  testDir: "./specs",
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  reporter: process.env.CI ? "line" : [["list"]],
  // The default per-test timeout is 30 s, but the songs/level CRUD flows run ~20-25 s locally
  // and comfortably exceed 30 s on a loaded CI runner (2-4x slower), so the whole test is
  // killed mid-flight ("Test timeout of 30000ms exceeded") even though every assertion set a
  // 30-240 s timeout — those are dead under a tighter test cap. Grant the same generous room
  // the online config already does (it uses 300 s); SimDevice is fast, so 120 s is ample.
  timeout: 120_000,
  use: {
    baseURL: `http://localhost:${VITE}`,
    trace: "on-first-retry",
  },
  projects: [{ name: "offline", use: { ...devices["Desktop Chrome"] } }],
  // Vite serves the real frontend; the Rust e2e_server runs the real commands.
  webServer: [
    {
      command: "bun run dev",
      url: `http://localhost:${VITE}`,
      reuseExistingServer: !process.env.CI,
      timeout: 120_000,
    },
    {
      command:
        "cargo run --manifest-path ../src-tauri/Cargo.toml --features e2e --bin e2e_server",
      url: `http://127.0.0.1:${PORT}/health`,
      reuseExistingServer: !process.env.CI,
      timeout: 180_000,
    },
  ],
});
