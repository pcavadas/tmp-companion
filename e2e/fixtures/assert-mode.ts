import type { FullConfig } from "@playwright/test";

// globalSetup for both lanes (#193): runs AFTER the webServers, so it also catches a server
// adopted by `reuseExistingServer` in the wrong mode — an online one under the offline
// config (device writes), or an offline one under the online config (fake-online green).
// Each config declares `metadata: { expectOnline, healthUrls }`.
export default async function assertMode(config: FullConfig): Promise<void> {
  const { expectOnline, healthUrls } = config.metadata as {
    expectOnline: boolean;
    healthUrls: string[];
  };
  for (const url of healthUrls) {
    const { online } = (await (await fetch(url)).json()) as {
      online?: boolean;
    };
    if (Boolean(online) !== expectOnline) {
      throw new Error(
        `e2e mode mismatch: ${url} reports online:${String(online)}, this lane expects ` +
          `online:${String(expectOnline)}. Kill the stale server (scripts/e2e.sh clears ` +
          `the port range) and re-run.`,
      );
    }
  }
}
