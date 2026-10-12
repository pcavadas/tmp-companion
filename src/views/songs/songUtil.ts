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

/** One setlist entry as the device holds it: its 1-based `position` (what remove and
 *  move address), the global song slot there, and that song — `null` when the slot
 *  resolves to no song on the unit (a gap, or an edit made on the unit since the
 *  read). Unresolved entries are SHOWN, never filtered: filtering one would shift
 *  every later row onto another entry's position. */
export interface SetlistEntry {
  position: number;
  songSlot: number;
  song: SongRecord | null;
}

/** Where setlist `slot` sits after the setlist deletes in `removed`, in the order they
 *  ran: each delete above it shifts it down one slot. `null` when it was deleted. */
export function slotAfterDeletes(
  slot: number,
  removed: readonly number[],
): number | null {
  let at = slot;
  for (const r of removed) {
    if (r === at) return null;
    if (r < at) at -= 1;
  }
  return at;
}

/** Re-key a membership cache after setlist `removed` is deleted: every later setlist
 *  shifts down one slot, so its cached songs move with it. */
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
