// src/views/songs/songUtil.ts — pure helpers and types for the Songs view. PRIVATE to
// the songs view (not re-exported from index.ts).
import type { SongRecord } from "../../lib/types";

/** Inline-edit draft of a song (the create/edit form's working values). */
export interface SongDraft {
  name: string;
  notes: string;
  bpm: number | null;
}

/** The editable BPM of a device song: the numeric value when active, else null. */
export function songBpm(rec: SongRecord): number | null {
  return rec.bpm_active ? rec.bpm : null;
}

/** A song's tempo as a row shows it ("120 BPM"), or null when it has none. */
export function tempoLabel(rec: SongRecord): string | null {
  const bpm = songBpm(rec);
  return bpm != null ? `${String(bpm)} BPM` : null;
}

/** "1 song" / "{n} songs". */
export function songCount(n: number): string {
  return `${String(n)} song${n === 1 ? "" : "s"}`;
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
