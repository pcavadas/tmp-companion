import { defineConfig, devices } from "@playwright/test";

// Ports default to 7600/1421; scripts/e2e.sh exports a per-worktree pair (TMP_E2E_PORT /
// TMP_E2E_VITE_PORT) so sibling-worktree runs don't collide.
const PORT = process.env.TMP_E2E_PORT ?? "7600";
const VITE = process.env.TMP_E2E_VITE_PORT ?? "1421";

// ONLINE e2e config — runs the SAME specs as the offline config (./specs), but starts the
// e2e_server with TMP_E2E_ONLINE=1 so it does ONE real USB handshake to seed the snapshot
// and every command opens the seized device (no SimDevice). `ensureScenario` clones the
// three working presets into 400/401/402 at setup; each spec clears them afterwards.
// Attended only: the device must be plugged in and Pro Control closed.
//
//   bunx playwright test --config e2e/playwright.online.config.ts
export default defineConfig({
  testDir: "./specs",
  fullyParallel: false,
  workers: 1, // the device is exclusive-seize
  reporter: [["list"]],
  // Real device: the ~22 s backup, real re-amp (×3 for the Level scenario), and the
  // per-test scratch seed/clear all add up — give each test generous room.
  timeout: 300_000,
  use: {
    baseURL: `http://localhost:${VITE}`,
    trace: "on-first-retry",
  },
  projects: [{ name: "online", use: { ...devices["Desktop Chrome"] } }],
  webServer: [
    {
      command: "bun run dev",
      url: `http://localhost:${VITE}`,
      reuseExistingServer: true,
      timeout: 120_000,
    },
    {
      command:
        "cargo run --manifest-path ../src-tauri/Cargo.toml --features e2e --bin e2e_server",
      url: `http://127.0.0.1:${PORT}/health`,
      reuseExistingServer: true,
      timeout: 180_000,
      env: { TMP_E2E_ONLINE: "1" },
    },
  ],
});
