// src/views/doctor/severity.ts — the Doctor results severity model: the three
// tint levels the cards paint (high / med / ok) mapped onto the theme tokens, plus
// the small pure helpers that derive a preset's worst severity + badge count.
//
// Colors map 1:1 onto theme tokens EXCEPT the med soft tint (see MED_SOFT).

import type { ThemeTokens } from "../../theme/tokens";
import type { DoctorPresetResult, DoctorSev } from "../../lib/types";

/** The two diagnosis severities plus the "all good" green. */
export type Sev = DoctorSev | "ok";

export interface SevTone {
  /** Foreground / accent color — text, bars, rings, borders. */
  fg: string;
  /** Soft header / panel tint. */
  soft: string;
}

// The med soft tint is the ONE color with no exact theme token: the handoff
// severity table specifies rgba(176,125,28,0.10), while t.sevWarnSoft is 0.08.
// Every other value below is a straight token reference.
const MED_SOFT = "rgba(176,125,28,0.10)";

export function sevTone(t: ThemeTokens, sev: Sev): SevTone {
  switch (sev) {
    case "high":
      return { fg: t.warn, soft: t.warnSoft };
    case "med":
      return { fg: t.sevWarn, soft: MED_SOFT };
    case "ok":
      return { fg: t.good, soft: t.goodSoft };
  }
}

const RANK: Record<Sev, number> = { ok: 0, med: 1, high: 2 };

/** Sort key for worst-first ordering (high > med > ok). */
export function sevRank(sev: Sev): number {
  return RANK[sev];
}

/** A scene-loudness jump beyond this (dB) bumps the preset's worst severity to
 *  "high" — a big volume leap between scenes is a headline problem. */
const SCENE_JUMP_HIGH_DB = 5;

/** The worst severity across a preset's sounds, bumped to "high" by a big
 *  scene-consistency jump (>5 dB). An errored sound carries no severity (it shows
 *  a message, not a diagnosis). */
export function presetWorstSev(preset: DoctorPresetResult): Sev {
  let worst: Sev = "ok";
  for (const sound of preset.sounds) {
    for (const diag of sound.diags) {
      if (diag.sev === "high") return "high";
      worst = "med";
    }
  }
  const sc = preset.sceneConsistency;
  if (sc && Math.abs(sc.worstDeltaDb) > SCENE_JUMP_HIGH_DB) return "high";
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
