// src/lib/platform.ts — which OS this build runs on, as the backend reports it
// (`app_info().os`, the compile-time target), never sniffed from the user agent.
//
// It words two things: the "unit busy" page (Fender ships Pro Control for macOS and
// Windows only, so on Linux the only other app that can hold the unit is a second TMP
// Companion window), and Settings › About's note on where the user's data is kept ("this
// Mac" or "this computer"). Module-scoped, like the library scan: App loads
// it once at startup (`loadPlatform`) and any view reads it (`usePlatform`) without its
// own device-independent round trip. `null` until the read lands (or if it fails).

import { useSyncExternalStore } from "react";

import { appInfo } from "./invoke";

export type Platform = "macos" | "windows" | "linux" | "other";

/** `std::env::consts::OS` → a {@link Platform}. */
export function platformOf(os: string): Platform {
  switch (os) {
    case "macos":
    case "windows":
    case "linux":
      return os;
    default:
      return "other";
  }
}

let current: Platform | null = null;
const subs = new Set<() => void>();

function subscribe(cb: () => void): () => void {
  subs.add(cb);
  return () => {
    subs.delete(cb);
  };
}

function getPlatform(): Platform | null {
  return current;
}

/** Read the platform from the backend (App calls it once at startup). A failed read
 * leaves it unknown; the failure is already logged by the invoke wrapper. */
export async function loadPlatform(): Promise<void> {
  try {
    const next = platformOf((await appInfo()).os);
    if (next === current) return;
    current = next;
    subs.forEach((f) => {
      f();
    });
  } catch {
    // Unknown platform: callers fall back to their default wording.
  }
}

/** The platform this build runs on, or `null` while unknown. */
export function usePlatform(): Platform | null {
  return useSyncExternalStore(subscribe, getPlatform);
}
