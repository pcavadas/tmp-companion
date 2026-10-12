// src/views/songs/songUtil.ts — pure helpers for the Songs view, split from
// ./shared so that file exports only the IconBtn component (React Fast Refresh's
// component-boundary rule) without disabling it. PRIVATE to the songs view.
import { DASH } from "../../lib/format";
import type { SongRecord } from "../../lib/types";

/** The editable BPM of a device song: the numeric value when active, else null. */
export function songBpm(rec: SongRecord): number | null {
  return rec.bpm_active ? rec.bpm : null;
}

export function bpmStr(b: number | null): string {
  return b != null ? String(b) : DASH;
}

/** Re-key a setlist-membership cache after setlist `removed` is deleted: its entry
 *  goes, and every later setlist shifts down one slot (slots are list index + 1). */
export function shiftMembershipAfterDelete(
  m: Map<number, number[]>,
  removed: number,
): Map<number, number[]> {
  const n = new Map<number, number[]>();
  m.forEach((v, k) => {
    if (k < removed) n.set(k, v);
    else if (k > removed) n.set(k - 1, v);
  });
  return n;
}
