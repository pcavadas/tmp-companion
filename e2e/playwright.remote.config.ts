import { defineConfig, devices } from "@playwright/test";
import path from "node:path";
import { fileURLToPath } from "node:url";

// REMOTE e2e config — the same specs as the other two configs, against a remote device
// reached over a socket (`TMP_E2E_REMOTE`, `hid::remote`). Run it through `scripts/e2e-remote.sh`, which
// builds, empties the remote device's scratch zone, seeds, and starts the e2e_server this config only
// ADOPTS: its e2e_server entry refuses to start one itself, so a run can never fall back to
// a server of another mode. Re-amp captures go to the remote device's own audio when
// TMP_E2E_REMOTE_AUDIO is set beside the remote device's address; without it they are refused, and only
// the control-plane specs pass (the runner's default set). This lane never stamps.
const PORT = process.env.TMP_E2E_PORT ?? "12000";
const VITE = process.env.TMP_E2E_VITE_PORT ?? "1821";

export default defineConfig({
  testDir: "./specs",
  // Refuses any adopted server that is not a remote server (#193's mode check).
  globalSetup: "./fixtures/assert-mode.ts",
  metadata: {
    expectOnline: true,
    expectRemote: true,
    expectRemoteAudio: Boolean(process.env.TMP_E2E_REMOTE_AUDIO?.trim()),
    healthUrls: [`http://127.0.0.1:${PORT}/health`],
  },
  fullyParallel: false,
  workers: 1, // one remote device, one exclusive seize
  forbidOnly: true,
  // The JSON report lets `scripts/e2e-remote.sh` fail a run in which no test ran: a dual-mode
  // spec that skips online skips here too, and Playwright exits 0 when every test skipped.
  reporter: [
    ["list"],
    [
      "json",
      {
        outputFile:
          process.env.TMP_E2E_REMOTE_REPORT ??
          path.join(
            process.env.TMPDIR ?? "/tmp",
            "tmp-companion-remote-report.json",
          ),
      },
    ],
  ],
  // Equal to the other configs' caps: one spec set, written for the slowest mode.
  timeout: 300_000,
  use: {
    baseURL: `http://localhost:${VITE}`,
    trace: "on-first-retry",
  },
  projects: [{ name: "remote", use: { ...devices["Desktop Chrome"] } }],
  webServer: [
    {
      command: "bun run dev",
      cwd: path.resolve(path.dirname(fileURLToPath(import.meta.url)), ".."),
      url: `http://localhost:${VITE}`,
      reuseExistingServer: true,
      timeout: 120_000,
    },
    {
      command:
        "node -e \"console.error('start the remote device e2e_server with scripts/e2e-remote.sh'); process.exit(1)\"",
      url: `http://127.0.0.1:${PORT}/health`,
      reuseExistingServer: true,
      timeout: 10_000,
    },
  ],
});
