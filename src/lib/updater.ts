// src/lib/updater.ts — thin seam over the Tauri updater/process plugins.
//
// The ONLY module allowed to import @tauri-apps/plugin-updater and
// @tauri-apps/plugin-process — everything else (the hook, tests) goes through
// this seam so the plugin JS never loads under Vitest/jsdom.

import { check } from "@tauri-apps/plugin-updater";

/** Exit + relaunch the app (finishes an installed update). */
export { relaunch as relaunchApp } from "@tauri-apps/plugin-process";

/** A found update, narrowed to what the UI needs. `download` downloads AND
 * installs, reporting whole-number percent progress. */
export interface FoundUpdate {
  version: string;
  notes: string | null;
  download(onPercent: (pct: number) => void): Promise<void>;
}

/** Ask the update endpoint; null when already on the latest version. */
export async function checkForUpdate(): Promise<FoundUpdate | null> {
  const update = await check();
  if (!update) return null;
  return {
    version: update.version,
    notes: update.body ?? null,
    download(onPercent) {
      let total = 0;
      let downloaded = 0;
      return update.downloadAndInstall((ev) => {
        if (ev.event === "Started") {
          total = ev.data.contentLength ?? 0;
        } else if (ev.event === "Progress") {
          downloaded += ev.data.chunkLength;
          if (total > 0) onPercent(Math.round((downloaded / total) * 100));
        } else {
          onPercent(100);
        }
      });
    },
  };
}
