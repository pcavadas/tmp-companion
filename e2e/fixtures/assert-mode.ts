import type { FullConfig } from "@playwright/test";

// globalSetup for both lanes (#193): runs AFTER the webServers, so it also catches a server
// adopted by `reuseExistingServer` in the wrong mode — an online one under the offline
// config (device writes), or an offline one under the online config (fake-online green).
// Each config declares `metadata: { expectOnline, healthUrls }`, plus `expectTwin` for a
// twin lane. A twin server reports `online` (real firmware) AND `twin`, so without the
// `twin` check the online lane would adopt one and pass green with no unit attached. A twin
// lane also declares `expectTwinAudio`: whether the server it adopts captures from the twin.
export default async function assertMode(config: FullConfig): Promise<void> {
  const { expectOnline, expectTwin, expectTwinAudio, healthUrls } =
    config.metadata as {
      expectOnline: boolean;
      expectTwin?: boolean;
      expectTwinAudio?: boolean;
      healthUrls: string[];
    };
  for (const url of healthUrls) {
    const { online, twin, twinAudio } = (await (await fetch(url)).json()) as {
      online?: boolean;
      twin?: boolean;
      twinAudio?: boolean;
    };
    if (
      expectTwinAudio !== undefined &&
      Boolean(twinAudio) !== expectTwinAudio
    ) {
      throw new Error(
        `e2e mode mismatch: ${url} reports twinAudio:${String(twinAudio)}, this lane ` +
          `expects twinAudio:${String(expectTwinAudio)} (TMP_E2E_TWIN_AUDIO). ` +
          `Kill the stale server and re-run scripts/e2e-twin.sh.`,
      );
    }
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
