// src/views/doctor/findingRow.ts — the header shape every Doctor finding row shares:
// SoundRow's per-sound rows and PresetFindingRow's synthetic preset-level rows
// (SceneConsistency's "Level jumps", LevelingDamageRow's "Leveling damage"). One home so
// the two row kinds line up and can't drift apart.

import type { CSSProperties } from "react";

import type { ThemeTokens } from "../../theme/tokens";

/** Diagnosis-row header min-height (SevDot + serif label baseline) — off the
 *  t.spaceN scale, so it's a role-named const shared by every finding row rather than
 *  a bare literal in each file. */
export const ROW_MIN_HEIGHT = 38;

/** The finding-row header: severity dot · title · middle slot · caret, tinted while
 *  the row is open. */
export function findingRowHeaderStyle(
  t: ThemeTokens,
  open: boolean,
): CSSProperties {
  return {
    display: "flex",
    alignItems: "center",
    gap: t.space5,
    minHeight: ROW_MIN_HEIGHT,
    padding: `${String(t.space3)}px ${String(t.space4)}px ${String(t.space3)}px ${String(t.space3)}px`,
    background: open ? t.rowSel : "transparent",
  };
}
