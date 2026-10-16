// src/ui/Dot.tsx — a status dot (look B): 8 px, solid or hollow, always paired with
// words that say the same thing, so the dot itself is `aria-hidden`.
//
// Tones: `good`, `caution` (`cautionLine`), `danger`, `neutral` (`meter`), and
// `working` (`accent` with a soft halo). Hollow = waiting (a 1.5 px ring). `color` is
// the legacy per-site colour for callers not yet on a tone; `tone` wins over it.

import { useTheme } from "../theme/ThemeContext";
import type { ThemeTokens } from "../theme/tokens";

export type DotTone = "good" | "caution" | "danger" | "neutral" | "working";

export interface DotProps {
  /** default 8. */
  size?: number;
  tone?: DotTone;
  /** Legacy custom colour; ignored when `tone` is given. */
  color?: string;
  /** Outline-only — waiting (a stopped run's tail, a queued row). */
  hollow?: boolean;
}

function toneColor(t: ThemeTokens, tone: DotTone): string {
  switch (tone) {
    case "good":
      return t.good;
    case "caution":
      return t.cautionLine;
    case "danger":
      return t.danger;
    case "neutral":
      return t.meter;
    case "working":
      return t.accent;
  }
}

export function Dot({ size = 8, tone, color, hollow }: DotProps) {
  const { t } = useTheme();
  const c = tone ? toneColor(t, tone) : (color ?? t.meter);
  return (
    <span
      aria-hidden
      style={{
        width: size,
        height: size,
        borderRadius: t.radiusPill,
        background: hollow ? "transparent" : c,
        border: hollow ? `1.5px solid ${c}` : undefined,
        // The halo is the frames' terracotta tint; no B token names it yet.
        boxShadow:
          tone === "working" && !hollow
            ? `0 0 0 3px ${t.accentBadgeSoft}`
            : undefined,
        boxSizing: "border-box",
        flexShrink: 0,
        display: "inline-block",
      }}
    />
  );
}
