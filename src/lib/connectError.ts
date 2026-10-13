// Classifies a `connect_device` failure into "show the red banner" vs "show the
// friendly please-connect gate", by the backend's typed error KIND (`error::AppError`,
// `{ kind, message }` on the wire) — never by matching the message text.
//
// Two suppressed (gate, not error) kinds:
//   • `not_connected` — no TMP enumerated: nothing plugged in yet.
//   • `not_ready` — the unit is present and seized, but its USB stack refused a report
//     write during the handshake. This is the COLD-BOOT window: the HID interface
//     enumerates (so the hotplug watcher fires + the open/seize succeeds) seconds
//     before tm-stomp-server is ready, so the first handshake send fails. It also
//     covers the unit being pulled mid-handshake. Either way the fix is "wait / power
//     it on" — exactly what the gate says — not a red error, and the retry reconnects
//     once the unit is up.
//
// Everything else surfaces (red banner) — most importantly `busy`, the actionable
// "close Fender Pro Control" hint when Pro Control holds the exclusive seize.
//
// Dependency-free on purpose (types only): `invoke.ts` imports this for its
// quiet-logging rule, so importing `invoke.ts` back would close a module cycle.

import type { CommandErrorKind } from "./types";

const KINDS: readonly CommandErrorKind[] = [
  "not_connected",
  "busy",
  "not_ready",
  "other",
];

/** Whether `k` is a {@link CommandErrorKind} the backend sends. */
export function isCommandErrorKind(k: unknown): k is CommandErrorKind {
  return typeof k === "string" && (KINDS as readonly string[]).includes(k);
}

/** The typed kind a caught error carries (`CommandError.kind`, or a raw
 * `{ kind, message }` rejection); `"other"` for anything without one. */
export function errorKind(e: unknown): CommandErrorKind {
  if (typeof e === "object" && e !== null && "kind" in e) {
    const k = (e as { kind?: unknown }).kind;
    if (isCommandErrorKind(k)) return k;
  }
  return "other";
}

/** Whether a connect failure of this kind is the friendly gate's case (no unit, or
 * a unit still booting) rather than an error worth a banner or a log line. */
export function isConnectGateKind(kind: CommandErrorKind): boolean {
  return kind === "not_connected" || kind === "not_ready";
}

/** The message to surface in the red AlertBanner for a connect failure, or `null`
 * to suppress it (the gate + the 3 s retry loop handle that kind). */
export function connectBannerMessage(
  kind: CommandErrorKind,
  message: string,
): string | null {
  return isConnectGateKind(kind) ? null : message;
}
