import type { FullConfig } from "@playwright/test";

// globalSetup for both lanes (#193): runs AFTER the webServers, so it also catches a server
// adopted by `reuseExistingServer` in the wrong mode — an online one under the offline
// config (device writes), or an offline one under the online config (fake-online green).
// Each config declares `metadata: { expectOnline, healthUrls }`, plus `expectRemote` for a
// remote lane. A remote server reports `online` (device state, not SimDevice) AND `remote`,
// so without the `remote` check the online lane would adopt one and pass green with no unit
// attached. A remote lane also declares `expectRemoteAudio`: whether the server it adopts
// captures from the remote device.
export default async function assertMode(config: FullConfig): Promise<void> {
  const { expectOnline, expectRemote, expectRemoteAudio, healthUrls } =
    config.metadata as {
      expectOnline: boolean;
      expectRemote?: boolean;
      expectRemoteAudio?: boolean;
      healthUrls: string[];
    };
  for (const url of healthUrls) {
    const { online, remote, remoteAudio } = (await (
      await fetch(url)
    ).json()) as {
      online?: boolean;
      remote?: boolean;
      remoteAudio?: boolean;
    };
    if (
      expectRemoteAudio !== undefined &&
      Boolean(remoteAudio) !== expectRemoteAudio
    ) {
      throw new Error(
        `e2e mode mismatch: ${url} reports remoteAudio:${String(remoteAudio)}, this lane ` +
          `expects remoteAudio:${String(expectRemoteAudio)} (TMP_E2E_REMOTE_AUDIO). ` +
          `Kill the stale server and re-run scripts/e2e-remote.sh.`,
      );
    }
    if (
      Boolean(online) !== expectOnline ||
      Boolean(remote) !== Boolean(expectRemote)
    ) {
      throw new Error(
        `e2e mode mismatch: ${url} reports online:${String(online)} remote:${String(remote)}, ` +
          `this lane expects online:${String(expectOnline)} remote:${String(Boolean(expectRemote))}. ` +
          `Kill the stale server (scripts/e2e.sh clears the port range) and re-run.`,
      );
    }
  }
}
