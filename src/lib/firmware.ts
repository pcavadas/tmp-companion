// src/lib/firmware.ts — tested-firmware floor + the per-tab gate predicate.
//
// TMP Companion has only been validated against unit firmware FW_MIN and later.
// Older units still connect, but the unit-driven pages can't be trusted, so a
// full-page "untested firmware" notice gates them (with a "use it anyway"
// escape). Catalog (shipped reference data) and Settings (local config) work
// without a trusted unit, so they're never gated.

/** Lowest unit firmware the app has been tested against. */
export const FW_MIN = "1.7";

/** Tabs that work without a trusted unit — never gated by the firmware notice. */
export const FW_UNGATED_TABS = new Set<string>(["catalog", "settings"]);

/** One dotted segment → integer; non-numeric / missing → 0. */
function seg(s: string): number {
  const n = Number(s);
  return Number.isNaN(n) ? 0 : n;
}

/**
 * `a >= b` by numeric version segments, so "1.10" > "1.7" (a plain string
 * compare would get that backwards). Missing trailing segments read as 0, so
 * "1.7" == "1.7.0".
 */
export function semverGte(a: string, b: string): boolean {
  const pa = a.split(".").map(seg);
  const pb = b.split(".").map(seg);
  const n = Math.max(pa.length, pb.length);
  for (let i = 0; i < n; i++) {
    const x = i < pa.length ? pa[i] : 0;
    const y = i < pb.length ? pb[i] : 0;
    if (x !== y) return x > y;
  }
  return true; // equal
}

/** A null version is "unknown" (no unit / not read yet) — treated as supported. */
export function firmwareSupported(version: string | null): boolean {
  return version == null || semverGte(version, FW_MIN);
}

/**
 * Whether the untested-firmware notice should replace the routed tab. True only
 * when a known version is below the floor, the user hasn't proceeded, and the
 * tab is unit-driven (Level / Copy / Songs).
 */
export function firmwareGateActive(o: {
  firmware: string | null;
  tab: string;
  proceeded: boolean;
}): boolean {
  if (o.firmware == null || firmwareSupported(o.firmware) || o.proceeded)
    return false;
  return !FW_UNGATED_TABS.has(o.tab);
}
