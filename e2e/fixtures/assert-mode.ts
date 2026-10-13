import type { FullConfig } from "@playwright/test";

// globalSetup for both lanes (#193): runs AFTER the webServers, so it also catches a server
// adopted by `reuseExistingServer` in the wrong mode — an online one under the offline
// config (device writes), or an offline one under the online config (fake-online green).
// Each config declares `metadata: { expectOnline, healthUrls }`, plus `expectTwin` for a
// twin lane. A twin server reports `online` (real firmware) AND `twin`, so without the
// `twin` check the online lane would adopt one and pass green with no unit attached.
export default async function assertMode(config: FullConfig): Promise<void> {
  const { expectOnline, expectTwin, healthUrls } = config.metadata as {
    expectOnline: boolean;
    expectTwin?: boolean;
    healthUrls: string[];
  };
  for (const url of healthUrls) {
    const { online, twin } = (await (await fetch(url)).json()) as {
      online?: boolean;
      twin?: boolean;
    };
    if (
      Boolean(online) !== expectOnline ||
      Boolean(twin) !== Boolean(expectTwin)
    ) {
      throw new Error(
        `e2e mode mismatch: ${url} reports online:${String(online)} twin:${String(twin)}, ` +
          `this lane expects online:${String(expectOnline)} twin:${String(Boolean(expectTwin))}. ` +
          `Kill the stale server (scripts/e2e.sh clears the port range) and re-run.`,
      );
    }
  }
}
