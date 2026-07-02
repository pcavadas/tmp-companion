import { defineConfig, devices } from "@playwright/test";

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
  use: {
    baseURL: "http://localhost:1421",
    trace: "on-first-retry",
  },
  projects: [{ name: "offline", use: { ...devices["Desktop Chrome"] } }],
  // Vite serves the real frontend; the Rust e2e_server runs the real commands.
  webServer: [
    {
      command: "bun run dev",
      url: "http://localhost:1421",
      reuseExistingServer: !process.env.CI,
      timeout: 120_000,
    },
    {
      command:
        "cargo run --manifest-path ../src-tauri/Cargo.toml --features e2e --bin e2e_server",
      url: "http://127.0.0.1:7600/health",
      reuseExistingServer: !process.env.CI,
      timeout: 180_000,
    },
  ],
});
