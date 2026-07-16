// src/views/doctor/severity.ts — the Doctor results severity model: the three
// tint levels the cards paint (high / med / ok) mapped onto the theme tokens, plus
// the small pure helpers that derive a preset's worst severity + badge count.
//
// Colors map 1:1 onto theme tokens EXCEPT the med soft tint (see MED_SOFT).

import type { CSSProperties } from "react";

import type { ThemeTokens } from "../../theme/tokens";
import type {
  DoctorDiag,
  DoctorPresetResult,
  DoctorSceneConsistency,
  DoctorSev,
  DoctorSoundResult,
} from "../../lib/types";

/** The two diagnosis severities plus the "all good" green. */
export type Sev = DoctorSev | "ok";

export interface SevTone {
  /** Foreground / accent color — text, bars, rings, borders. */
  fg: string;
  /** Soft header / panel tint. */
  soft: string;
  /** 0.5px chip / card border tint. */
  border: string;
}

// The med soft tint is the ONE color with no exact theme token: the handoff
// severity table specifies rgba(176,125,28,0.10), while t.sevWarnSoft is 0.08.
// Every other value below is a straight token reference.
const MED_SOFT = "rgba(176,125,28,0.10)";

export function sevTone(t: ThemeTokens, sev: Sev): SevTone {
  switch (sev) {
    case "high":
      return { fg: t.warn, soft: t.warnSoft, border: t.warnBorder };
    case "med":
      return { fg: t.sevWarn, soft: MED_SOFT, border: t.sevWarnBorder };
    case "ok":
      return { fg: t.good, soft: t.goodSoft, border: t.goodBorder };
  }
}

const RANK: Record<Sev, number> = { ok: 0, med: 1, high: 2 };

/** Sort key for worst-first ordering (high > med > ok). */
export function sevRank(sev: Sev): number {
  return RANK[sev];
}

/** A fired card within this many units of its threshold (severity = margin past
 *  threshold, in the rule's own dB/LU unit) is a near-threshold "possible"
 *  verdict (muted, ranked lower). No backend confidence anymore — this is a
 *  pure severity threshold. */
export const POSSIBLE_MAX_SEVERITY = 1.0;

/** True when a diagnosis is a low-severity "possible" verdict. */
export function isPossible(diag: DoctorDiag): boolean {
  return diag.severity < POSSIBLE_MAX_SEVERITY;
}

/** The label as shown in the UI: "Possible X" when `isPossible`, else the bare
 *  label. The one place that builds this string — chip + expanded heading + the
 *  LevelIndicator aria-label all route through it so the hedge is never dropped. */
export function possibleLabel(diag: DoctorDiag): string {
  return isPossible(diag) ? `Possible ${diag.label}` : diag.label;
}

/** A sound's diagnoses worst-first: severity tint (high before med) is the
 *  primary key, raw severity magnitude (descending) the tiebreaker — so
 *  confidently-past-threshold findings sort above near-threshold "possible"
 *  ones. */
export function sortedDiags(diags: DoctorDiag[]): DoctorDiag[] {
  return [...diags].sort((a, b) => {
    const r = sevRank(b.sev) - sevRank(a.sev);
    return r !== 0 ? r : b.severity - a.severity;
  });
}

/** A scene-loudness jump beyond this (dB) bumps the preset's worst severity to
 *  "high" — a big volume leap between scenes is a headline problem. */
const SCENE_JUMP_HIGH_DB = 5;

/** The severity of the synthetic "Level jumps" row: a big jump (>5 dB) is a
 *  headline problem, a present-but-moderate one is worth a look. */
export function sceneConsistencySev(sc: DoctorSceneConsistency): Sev {
  return Math.abs(sc.worstDeltaDb) > SCENE_JUMP_HIGH_DB ? "high" : "med";
}

/** The worst severity across one sound's diagnoses ("ok" when it has none). An
 *  errored sound carries no severity (it shows a message, not a diagnosis). */
export function soundSev(sound: DoctorSoundResult): Sev {
  let worst: Sev = "ok";
  for (const diag of sound.diags) {
    if (diag.sev === "high") return "high";
    worst = "med";
  }
  return worst;
}

/** The worst severity across a preset's sounds, bumped by the scene-consistency
 *  finding via `sceneConsistencySev`. A present-but-moderate scene jump still
 *  counts as at least "med" so the header/badge tint matches `presetLookCount`
 *  (which counts the scene finding) instead of painting a flagged preset "all
 *  clear" green. */
export function presetWorstSev(preset: DoctorPresetResult): Sev {
  let worst: Sev = "ok";
  for (const sound of preset.sounds) {
    const s = soundSev(sound);
    if (s === "high") return "high";
    if (s === "med") worst = "med";
  }
  const sc = preset.sceneConsistency;
  if (sc) {
    if (sceneConsistencySev(sc) === "high") return "high";
    if (worst === "ok") worst = "med";
  }
  return worst;
}

/** How many things the card status badge counts: every diagnosis plus the
 *  scene-consistency finding (one, when present). */
export function presetLookCount(preset: DoctorPresetResult): number {
  const diags = preset.sounds.reduce((n, s) => n + s.diags.length, 0);
  return diags + (preset.sceneConsistency ? 1 : 0);
}

/** The uppercase mono severity kicker shown in an expanded diagnosis panel. */
export function diagSevLabel(sev: DoctorSev): string {
  return sev === "high" ? "Needs attention" : "Worth a look";
}

/** The Doctor detail-card chrome (PrescriptionCard / CutThroughCard /
 * MatchCard) — one home for the border/radius/padding so the three cards
 * can't drift; callers override only the tone (border/background). */
export function doctorCard(
  t: ThemeTokens,
  tone?: { border?: string; background?: string },
): CSSProperties {
  return {
    flexShrink: 0,
    border: `0.5px solid ${tone?.border ?? t.hairlineStrong}`,
    borderRadius: 10,
    background: tone?.background ?? t.bg,
    padding: t.space6,
  };
}
