// src/views/doctor/severity.ts — how bad a Doctor finding looks, and the one order every
// list of findings follows.
//
// A finding shows in one of two tones: `danger` for a confident high-severity finding,
// `caution` for a medium one AND for a borderline one ("Muddy (borderline)": a high finding
// fired only just past its limit, `z` under `POSSIBLE_MAX_Z`). The tone is the row's dot,
// the opened block's colours and the sort key, so a borderline finding never sorts above a
// row that shows real danger: what the row shows and where it sits always agree.

import type {
  DoctorDiag,
  DoctorSceneConsistency,
  DoctorSoundResult,
} from "../../lib/types";

/** A finding's tone on screen. */
export type FindingTone = "danger" | "caution";

/** A fired card fewer than this many run-to-run standard deviations past its
 *  threshold (`DoctorDiag.z`, unit-free so it reads the same for dB, dB/oct
 *  and peak-height rules) is a near-threshold "borderline" verdict (caution,
 *  ranked lower). 2.5σ keeps a band rule's old 1.0 dB cutoff (σ 0.4 dB). */
export const POSSIBLE_MAX_Z = 2.5;

/** True when a diagnosis is a low-confidence borderline verdict. */
export function isPossible(diag: DoctorDiag): boolean {
  return diag.z < POSSIBLE_MAX_Z;
}

/** The finding's name as shown: "Muddy (borderline)" when `isPossible`, else the bare
 *  name. The one place that builds this string — the row caption and the opened
 *  block's heading both route through it, so the hedge is never dropped. */
export function possibleLabel(diag: DoctorDiag): string {
  return isPossible(diag) ? `${diag.label} (borderline)` : diag.label;
}

/** The tone a finding shows: danger for a confident high finding, caution for a medium
 *  one or a borderline one. */
export function findingTone(diag: DoctorDiag): FindingTone {
  return diag.sev === "high" && !isPossible(diag) ? "danger" : "caution";
}

/** Sort key for worst-first ordering (danger > caution). */
export function toneRank(tone: FindingTone): number {
  return tone === "danger" ? 2 : 1;
}

/** A sound's findings worst-first: the tone they show first, then confidence `z`
 *  (descending) — so a confident finding sorts above a borderline one, whatever unit
 *  each rule measures in. */
export function sortedDiags(diags: readonly DoctorDiag[]): DoctorDiag[] {
  return [...diags].sort((a, b) => {
    const r = toneRank(findingTone(b)) - toneRank(findingTone(a));
    return r !== 0 ? r : b.z - a.z;
  });
}

/** The worst tone across a sound's findings; null when it has none. */
export function soundTone(sound: DoctorSoundResult): FindingTone | null {
  if (sound.diags.length === 0) return null;
  return sound.diags.some((d) => findingTone(d) === "danger")
    ? "danger"
    : "caution";
}

/** A scene-loudness jump beyond this (dB) shows as danger — a big volume leap between
 *  scenes is a headline problem. */
export const SCENE_JUMP_HIGH_DB = 5;

/** A jump within this (dB) is fine: the backend's advice aims every scene under it
 *  ("until the jump is 3 dB or less"). */
export const SCENE_JUMP_OK_DB = 3;

/** The tone of a preset's "Scenes" row: danger past `SCENE_JUMP_HIGH_DB`, else caution. */
export function sceneLoudnessTone(sc: DoctorSceneConsistency): FindingTone {
  return Math.abs(sc.worstDeltaDb) > SCENE_JUMP_HIGH_DB ? "danger" : "caution";
}

/** The tone of one sound's bar in the opened "Scenes" row, on the row's own scale:
 *  danger past `SCENE_JUMP_HIGH_DB`, caution past `SCENE_JUMP_OK_DB`, null when fine. */
export function sceneBarTone(deltaDb: number): FindingTone | null {
  const size = Math.abs(deltaDb);
  if (size > SCENE_JUMP_HIGH_DB) return "danger";
  return size > SCENE_JUMP_OK_DB ? "caution" : null;
}
