// src/lib/format.ts — tiny shared formatting helpers used across screens.
//
// Single source of truth for two patterns each isolated screen had re-rolled.

/** The no-fabricate placeholder. Shown wherever a value has no real backing
 * (the project "never fabricate data" rule). Was redeclared in ~11 screens
 * under three different names (`DASH` / `EMDASH` / `EM_DASH`). */
export const DASH = "—";

/** Best-effort human string for a caught error/rejection — the `message` field
 * when present, else the value stringified. Shared by the screens' device-op
 * catch handlers (was re-declared verbatim in LevelView + SongsView). */
export function errMsg(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (typeof e === "string") return e;
  if (typeof e === "object" && e !== null && "message" in e) {
    const m = (e as { message?: unknown }).message;
    if (typeof m === "string") return m;
  }
  // Fall back to a compact, best-effort label that never yields the useless
  // "[object Object]" the bare String(obj) coercion would produce.
  if (typeof e === "object" && e !== null) return JSON.stringify(e);
  // Primitives (number / boolean / bigint / symbol / null / undefined).
  return String(e);
}

/** Two-digit zero-pad (e.g. 5 → "05"). */
export function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

/** Three-digit zero-pad (e.g. 5 → "005"). */
export function pad3(n: number): string {
  return String(n).padStart(3, "0");
}

/** A 0-based list index rendered as the device's 1-based 3-digit slot label
 * (e.g. index 0 → "001"). The device userSlot = list index + 1; this is the
 * single source of truth for that display convention. */
export function slotLabel(index: number): string {
  return pad3(index + 1);
}

/** A loudness value rendered with the real minus icon and one decimal (e.g.
 * -24 → "−24.0"); the no-fabricate em-dash when unknown. Bare number — callers
 * add the "LUFS" unit only in headers/legends. */
export function fmtLufs(v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v)) return DASH;
  return `${v < 0 ? "−" : ""}${Math.abs(v).toFixed(1)}`;
}

/** A dB DELTA with an explicit sign + the real minus glyph, one decimal (e.g.
 * +6 → "+6.0", -3 → "−3.0") — unlike `fmtLufs` (a bare reading, minus-only),
 * a delta/contrast is always signed so "+" vs "−" reads as unambiguous
 * direction at a glance. */
export function signedDb(db: number): string {
  return `${db < 0 ? "−" : "+"}${Math.abs(db).toFixed(1)}`;
}
