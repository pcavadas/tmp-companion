// src/lib/useUpdater.ts — the auto-update state machine, called ONCE in AppShell.
//
// Plain useState (AppShell never unmounts, so no module store is needed).
// On mount (in-app only): read the app version + the persisted auto-install
// preference, then check the update endpoint in the background — silent on any
// failure. A found update either waits as the "available" toast or (auto-install
// on) starts downloading immediately. No auto-dismiss timers anywhere.

import { useCallback, useEffect, useState } from "react";

import { appInfo, getStore, isTauri, setAutoInstallUpdates } from "./invoke";
import { checkForUpdate, relaunchApp } from "./updater";
import type { FoundUpdate } from "./updater";

export type UpdatePhase =
  "idle" | "available" | "reviewing" | "downloading" | "ready" | "error";

export interface UpdaterApi {
  phase: UpdatePhase;
  /** The NEW version (set once an update is found). */
  version: string | null;
  /** The update's release notes (raw markdown-ish text). */
  notes: string | null;
  /** 0–100 while downloading. */
  percent: number;
  autoInstall: boolean;
  /** The running app's version (from `appInfo()`). */
  currentVersion: string | null;
  setAutoInstall: (on: boolean) => void;
  /** Manual check (the Settings button). Never throws. */
  check: () => Promise<"found" | "none" | "error">;
  /** available → reviewing (the release-notes modal). */
  review: () => void;
  /** reviewing → available (the modal's "Later" — back to the toast). */
  cancelReview: () => void;
  startDownload: () => void;
  /** Any dismissible state → idle. */
  dismiss: () => void;
  /** error → downloading again (or a fresh check when the update ref is gone). */
  retry: () => void;
  restart: () => void;
}

/** Release-notes markdown → plain bullet lines (the modal adds its own "•  "
 * prefix). Strips heading #s, [text](url) links, and leading bullets; drops
 * blank lines; caps at 10 lines. */
export function formatReleaseNotes(notes: string): string[] {
  const out: string[] = [];
  for (const raw of notes.split(/\r?\n/)) {
    const line = raw
      .trim()
      .replace(/^#+\s*/, "")
      .replace(/^[*\-•]\s*/, "")
      .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
      .trim();
    if (line === "") continue;
    out.push(line);
    if (out.length === 10) break;
  }
  return out;
}

/** Dev/e2e builds carry this placeholder version — never check for updates. */
const DEV_VERSION = "0.0.0-development";

export function useUpdater(): UpdaterApi {
  const [phase, setPhase] = useState<UpdatePhase>("idle");
  const [found, setFound] = useState<FoundUpdate | null>(null);
  const [percent, setPercent] = useState(0);
  const [autoInstall, setAutoInstallState] = useState(false);
  const [currentVersion, setCurrentVersion] = useState<string | null>(null);

  const start = useCallback((u: FoundUpdate) => {
    setPhase("downloading");
    setPercent(0);
    u.download(setPercent).then(
      () => {
        setPhase("ready");
      },
      () => {
        setPhase("error");
      },
    );
  }, []);

  const onFound = useCallback(
    (u: FoundUpdate, auto: boolean) => {
      setFound(u);
      if (auto) start(u);
      else setPhase("available");
    },
    [start],
  );

  const startDownload = useCallback(() => {
    if (found) start(found);
  }, [found, start]);

  // ── Startup: version + preference + one background check (all silent) ─────
  useEffect(() => {
    if (!isTauri()) return;
    let cancelled = false;
    // Read through a call so TS doesn't narrow the flag "always false" across
    // the awaits (the cleanup's `cancelled = true` is invisible to its CFA).
    const gone = () => cancelled;
    const run = async () => {
      try {
        const info = await appInfo();
        if (gone()) return;
        setCurrentVersion(info.version);
        let auto = false;
        try {
          const store = await getStore();
          auto = store.auto_install_updates;
          if (!gone()) setAutoInstallState(auto);
        } catch {
          // keep the default — the preference is cosmetic here
        }
        if (gone() || info.version === DEV_VERSION) return;
        const found = await checkForUpdate();
        if (!gone() && found) onFound(found, auto);
      } catch {
        // background check — stay idle, no error surface
      }
    };
    void run();
    return () => {
      cancelled = true;
    };
  }, [onFound]);

  const check = useCallback(async (): Promise<"found" | "none" | "error"> => {
    try {
      const found = await checkForUpdate();
      if (!found) return "none";
      onFound(found, autoInstall);
      return "found";
    } catch {
      return "error";
    }
  }, [onFound, autoInstall]);

  const setAutoInstall = useCallback((on: boolean) => {
    setAutoInstallState(on); // optimistic — persistence is best-effort
    void setAutoInstallUpdates(on).catch(() => undefined);
  }, []);

  const review = useCallback(() => {
    setPhase("reviewing");
  }, []);
  const cancelReview = useCallback(() => {
    setPhase("available");
  }, []);
  const dismiss = useCallback(() => {
    setPhase("idle");
  }, []);
  const retry = useCallback(() => {
    if (found) start(found);
    else void check();
  }, [found, start, check]);
  const restart = useCallback(() => {
    void relaunchApp();
  }, []);

  return {
    phase,
    version: found ? found.version : null,
    notes: found ? found.notes : null,
    percent,
    autoInstall,
    currentVersion,
    setAutoInstall,
    check,
    review,
    cancelReview,
    startDownload,
    dismiss,
    retry,
    restart,
  };
}
