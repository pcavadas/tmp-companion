// src/views/doctor/abReport.ts — pure helpers for the applied-prescription A/B:
// the playback gains that level-match the two clips (#257) and the one-line
// before → after report on the treated finding (#256). No device I/O; the
// backend measures both clips (`doctor_apply`'s `levels` / `finding`).

import type { AbLevels, FindingCheck } from "../../lib/types";

/** Under this loudness gap (dB) the clips already play at the same level. */
const SAME_LEVEL_DB = 0.5;
/** An AFTER severity within this many dB of BEFORE is "no measurable change"
 *  (about the band rules' run-to-run repeatability). */
const NO_CHANGE_DB = 0.5;

export interface LevelMatch {
  /** `HTMLAudioElement.volume` for each clip, in [0, 1]: the louder clip is
   *  turned down to the quieter one's loudness, the quieter plays as is. */
  beforeVolume: number;
  afterVolume: number;
  /** The real level difference, said once. */
  line: string;
}

function fmt(db: number): string {
  return db.toFixed(1);
}

/** The level match for a measured pair; `null` when either clip's loudness is
 *  unknown, so the audition plays both as captured. */
export function levelMatch(
  levels: AbLevels | null | undefined,
): LevelMatch | null {
  if (levels == null) return null;
  const diff = levels.afterLufs - levels.beforeLufs;
  if (!Number.isFinite(diff)) return null;
  const gain = 10 ** (-Math.abs(diff) / 20);
  if (Math.abs(diff) < SAME_LEVEL_DB) {
    return {
      beforeVolume: diff < 0 ? gain : 1,
      afterVolume: diff > 0 ? gain : 1,
      line: "Both clips play at the same level.",
    };
  }
  return {
    beforeVolume: diff < 0 ? gain : 1,
    afterVolume: diff > 0 ? gain : 1,
    line: `After is ${fmt(Math.abs(diff))} dB ${diff < 0 ? "quieter" : "louder"}. Levels matched for comparison.`,
  };
}

/** The treated finding re-measured on both clips, as one line; `null` when
 *  there is nothing to report (no check, or the finding didn't show on the
 *  BEFORE clip either). Severities are dB past the finding's limit. */
export function findingCheckLine(
  label: string,
  check: FindingCheck | null | undefined,
): string | null {
  if (check?.before == null) return null;
  const before = check.before;
  if (check.after == null) {
    return `${label}: +${fmt(before)} dB over → cleared by the fix.`;
  }
  const span = `${label}: +${fmt(before)} → +${fmt(check.after)} dB over.`;
  if (check.after - before >= NO_CHANGE_DB) {
    return `${span} Worse after the fix. Discard it.`;
  }
  if (before - check.after < NO_CHANGE_DB) {
    return `${span} No measurable change, so the fix may not have landed. Discard and try again.`;
  }
  return `${span} Better, not fully cleared.`;
}
