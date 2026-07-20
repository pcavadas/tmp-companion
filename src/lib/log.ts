// TMP Companion — frontend log sink.
//
// Routes frontend errors to BOTH the dev console and the Tauri log file (via
// tauri-plugin-log → ~/Library/Logs/dev.tmpcompanion.app/). The plugin is
// only available inside the WKWebView, so every call is guarded by `isTauri()`
// — under plain `vite` / Vitest the helpers degrade to console output. Used by
// the global error handlers in main.tsx and by the top-level ErrorBoundary, so
// a render crash (the class that once blanked the whole window) lands on disk.

import { error as pluginError } from "@tauri-apps/plugin-log";

/**
 * True when running inside Tauri's WKWebView (the global injected by the
 * runtime). Screens may use this to render a "not in app" notice during a plain
 * `vite` browser session; the wrappers themselves always call `invoke`
 * (`invoke` rejects gracefully off-host). Defined HERE (not in ./invoke, its
 * historical home — it stays re-exported from there) so the invoke→log import
 * edge is one-directional: a definition in ./invoke would close an invoke⇄log
 * module cycle, the module-init TDZ-crash class.
 */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Persist an error to the dev console and the on-disk Tauri log (when in-app). */
export function logError(msg: string): void {
  console.error(msg);
  if (isTauri())
    void pluginError(msg).catch(() => {
      /* no-op */
    });
}

let globalHandlersInstalled = false;

/** Install once at startup: forward uncaught errors + unhandled rejections.
 * Idempotent — a second call (e.g. on a Vite HMR re-eval) is a no-op, so error
 * listeners never stack and log the same failure twice. */
export function installGlobalErrorLogging(): void {
  if (typeof window === "undefined" || globalHandlersInstalled) return;
  globalHandlersInstalled = true;
  window.addEventListener("error", (e) => {
    logError(
      `window.onerror: ${e.message} @ ${e.filename}:${String(e.lineno)}:${String(e.colno)}`,
    );
  });
  window.addEventListener("unhandledrejection", (e) => {
    logError(`unhandledrejection: ${String(e.reason)}`);
  });
}
