// Classifies a `connect_device` failure by the backend's typed error KIND
// (`error::AppError`, `{ kind, message }` on the wire) — never by matching the message
// text. The kind picks the page a device tab shows while no unit is connected; the
// message itself only reaches the log (and so the bug report), never the screen.
//
//   • `not_connected` — no TMP enumerated: nothing plugged in yet.
//   • `not_ready` — the unit is present and seized, but its USB stack refused a report
//     write during the handshake. This is the COLD-BOOT window: the HID interface
//     enumerates (so the hotplug watcher fires + the open/seize succeeds) seconds
//     before tm-stomp-server is ready, so the first handshake send fails. It also
//     covers the unit being pulled mid-handshake. Either way the fix is "wait / switch
//     it on", so it shows the same page as nothing plugged in, and the retry reconnects
//     once the unit is up.
//   • `busy` — another app holds the unit (Pro Control, or another TMP Companion
//     window): "Another app is using your Tone Master Pro".
//   • `other` — anything else: "Couldn't connect to your Tone Master Pro".
//
// The three leveling commands reject with the same typed shape, their kind a `SkipKind`
// (why the run was not leveled) plus `reampStillOn`; `errorSkipKind` and
// `errorReampStillOn` read those for the run engine, again never from the text.
//
// Dependency-free on purpose (types only): `invoke.ts` imports this for its
// quiet-logging rule, so importing `invoke.ts` back would close a module cycle.

import type { CommandErrorKind, SkipKind } from "./types";

const KINDS: readonly CommandErrorKind[] = [
  "not_connected",
  "busy",
  "not_ready",
  // The leveling commands' kinds (`SkipKind`): why a run or a sound was not leveled.
  "preset_unreadable",
  "footswitches_unreadable",
  "no_active_amp",
  "level_manually",
  "scene_write_refused",
  "unsupported_knob",
  "knob_missing",
  "block_off",
  "scene_change_discards",
  "no_stimulus",
  "clipped_capture",
  "no_signal",
  "connection_lost",
  "cancelled",
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

/** WHY a caught leveling failure was not leveled: the typed kind a leveling command
 * rejected with (`CommandError.kind`), or `"other"` for anything that carries none — a
 * bare-string rejection, a JS throw, or a connect kind no leveling command sends. */
export function errorSkipKind(e: unknown): SkipKind {
  const k = errorKind(e);
  return k === "not_connected" || k === "busy" || k === "not_ready"
    ? "other"
    : k;
}

/** Whether a rejected LEVELING command says re-amp may still be on (`reampStillOn` on
 * the typed `{ kind, message, reampStillOn }` rejection): the run failed AND its run-end
 * re-amp OFF never read back OFF. `false` for anything without the flag. */
export function errorReampStillOn(e: unknown): boolean {
  if (typeof e === "object" && e !== null && "reampStillOn" in e)
    return (e as { reampStillOn?: unknown }).reampStillOn === true;
  return false;
}

/** Whether a connect failure of this kind is the plain "plug it in" case (no unit, or a
 * unit still booting) — the steady state while unplugged, so not worth a log line. */
export function isConnectGateKind(kind: CommandErrorKind): boolean {
  return kind === "not_connected" || kind === "not_ready";
}

/** What a device tab (Level, Doctor, Copy, Songs) shows while no unit is connected. */
export type NoUnitState = "connecting" | "off" | "busy" | "failed";

/** The no-unit page: "connecting" until the first attempt settles, then the last failed
 * connect's kind (`null` = none since a detach or launch) decides it. */
export function noUnitState(
  connecting: boolean,
  kind: CommandErrorKind | null,
): NoUnitState {
  if (connecting) return "connecting";
  if (kind === "busy") return "busy";
  if (kind === "other") return "failed";
  return "off";
}
