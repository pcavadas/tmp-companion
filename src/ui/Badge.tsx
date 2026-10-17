// src/ui/Badge.tsx — a word or two of status on a row (look B).
//
// 18 high, `radiusSmall`, 12 px/500, sentence case, no border. Five tones: `neutral`
// (`segmentTrack` fill, `text`), `good`, `caution`, `danger` (their `*Fill` and text
// colour) and `accent` ("Changed" in Copy only; `accentFill` and `link`). Optional 11 px
// leading icon. At most one badge per row. It replaced the old Tag and the mono chips.

import type { ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";
import type { ThemeTokens } from "../theme/tokens";
import { Icon, type IconName } from "./Icon";

export type BadgeTone = "neutral" | "good" | "caution" | "danger" | "accent";

export interface BadgeProps {
  children: ReactNode;
  /** Default "neutral". */
  tone?: BadgeTone;
  /** Optional leading 11 px icon. */
  icon?: IconName;
}

function badgePaint(
  t: ThemeTokens,
  tone: BadgeTone,
): { bg: string; fg: string } {
  switch (tone) {
    case "neutral":
      return { bg: t.segmentTrack, fg: t.text };
    case "good":
      return { bg: t.goodFill, fg: t.good };
    case "caution":
      return { bg: t.cautionFill, fg: t.caution };
    case "danger":
      return { bg: t.dangerFill, fg: t.danger };
    case "accent":
      return { bg: t.accentFill, fg: t.link };
  }
}

export function Badge({ children, tone = "neutral", icon }: BadgeProps) {
  const { t } = useTheme();
  const p = badgePaint(t, tone);
  return (
    <span
      style={{
        height: 18,
        flexShrink: 0,
        boxSizing: "border-box",
        display: "inline-flex",
        alignItems: "center",
        gap: t.sp4,
        padding: `0 ${String(t.sp6)}px`,
        borderRadius: t.radiusSmall,
        background: p.bg,
        color: p.fg,
        fontFamily: t.fontSans,
        fontSize: t.fontCaption,
        fontWeight: t.weightMedium,
        whiteSpace: "nowrap",
      }}
    >
      {icon && <Icon name={icon} size={11} stroke={p.fg} strokeWidth={2} />}
      {children}
    </span>
  );
}
