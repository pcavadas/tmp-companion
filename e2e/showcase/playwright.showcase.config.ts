import { defineConfig, devices } from "@playwright/test";

// Marketing-screenshot tour (NOT a test gate). Boots the SAME offline e2e harness as
// playwright.config.ts, but with TMP_E2E_SHOWCASE=1 so the backend serves the curated,
// non-personal `e2e/fixtures/showcase/` library, and at the app's native 900×680 window
// size so each captured tab fills the site's screenshot frame (aspect 432/326). The cargo
// server is NOT reused (`reuseExistingServer:false`) — a plain gate server lacks the
// showcase env — so stop any stale server on :7600 first. One worker, one project.
export default defineConfig({
  testDir: ".",
  fullyParallel: false,
  workers: 1,
  reporter: [["list"]],
  use: {
    baseURL: "http://localhost:1421",
  },
  // viewport + scale go in the PROJECT use, AFTER the devices spread — otherwise
  // `devices["Desktop Chrome"]`'s 1280×720 (16:9) overrides them and the capture no
  // longer matches the app's native 900×680 window (the site frame is aspect 432/326).
  projects: [
    {
      name: "showcase",
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 900, height: 680 },
        // 3× the native 900×680 window → ~3.1k-wide crisp PNGs (the app is a webview, so a
        // higher scale renders text/vectors at more detail). The output ships at this native
        // resolution — no downscale pass — so the screenshots stay sharp at full size.
        deviceScaleFactor: 3,
      },
    },
  ],
  webServer: [
    {
      command: "bun run dev",
      url: "http://localhost:1421",
      reuseExistingServer: true,
      timeout: 120_000,
    },
    {
      // webServer CWD defaults to this config's dir (`e2e/showcase/`), so the manifest is
      // two levels up — NOT `../src-tauri` like the gate config (which lives in `e2e/`).
      command:
        "cargo run --manifest-path ../../src-tauri/Cargo.toml --features e2e --bin e2e_server",
      url: "http://127.0.0.1:7600/health",
      reuseExistingServer: false,
      timeout: 180_000,
      env: { TMP_E2E_SHOWCASE: "1" },
    },
  ],
});
