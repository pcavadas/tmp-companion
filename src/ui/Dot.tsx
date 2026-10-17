// src/ui/Dot.tsx — a status dot (look B): 8 px, solid or hollow, always paired with
// words that say the same thing, so the dot itself is `aria-hidden`.
//
// Tones: `good`, `caution` (`cautionLine`), `danger`, `neutral` (`meter`), and
// `working` (`accent` with a soft `accentFill` halo). Hollow = waiting (a 1.5 px ring).

import { useTheme } from "../theme/ThemeContext";
import type { ThemeTokens } from "../theme/tokens";

export type DotTone = "good" | "caution" | "danger" | "neutral" | "working";

export interface DotProps {
  /** default 8. */
  size?: number;
  tone: DotTone;
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

export function Dot({ size = 8, tone, hollow }: DotProps) {
  const { t } = useTheme();
  const c = toneColor(t, tone);
  return (
    <span
      aria-hidden
      style={{
        width: size,
        height: size,
        borderRadius: t.radiusPill,
        background: hollow ? "transparent" : c,
        border: hollow ? `1.5px solid ${c}` : undefined,
        boxShadow:
          tone === "working" && !hollow
            ? `0 0 0 3px ${t.accentFill}`
            : undefined,
        boxSizing: "border-box",
        flexShrink: 0,
        display: "inline-block",
      }}
    />
  );
}
