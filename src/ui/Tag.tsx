// src/ui/Tag.tsx — the DS chip/tag.
//
// One tokenized mono chip replacing the ~14 hand-rolled inline spans (scene
// badges, FS tags, "by ear" caveats, status pips). Renders as an inline-block
// span so it flows inside the existing flex rows unchanged. Tone picks the
// text/border/background triplet from the token set; `fg` is the custom-color
// escape (a per-site accent) that WINS over tone.

import type { CSSProperties, ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";

export type TagTone = "accent" | "good" | "warn" | "neutral" | "neutralFill";
export type TagSize = "sm" | "md";

export interface TagProps {
  children: ReactNode;
  /** default "neutral". */
  tone?: TagTone;
  /** default "sm". */
  size?: TagSize;
  /** CSS textTransform only — never edits the child string. */
  uppercase?: boolean;
  /** custom-color mode: color=fg, border=`${fg}66`, transparent bg; WINS over tone. */
  fg?: string;
  /** merged last (rare per-site escape). */
  style?: CSSProperties;
}

export function Tag({
  children,
  tone = "neutral",
  size = "sm",
  uppercase,
  fg,
  style,
}: TagProps) {
  const { t } = useTheme();

  const sized: CSSProperties =
    size === "md"
      ? {
          fontSize: t.fsMeta,
          padding: "2px 7px",
          borderRadius: t.rMenuItem,
        }
      : {
          fontSize: t.fsTag,
          letterSpacing: t.lsTag,
          padding: "1px 5px",
          borderRadius: t.rSm,
        };

  // fg wins over tone; else the per-tone text/border/background triplet.
  // "warn" keeps a transparent border for layout parity with bordered chips.
  const tones: Record<TagTone, { c: string; b: string; bg: string }> = {
    accent: { c: t.accentDeep, b: t.accentBorder, bg: t.accentSoft },
    good: { c: t.good, b: t.goodBorder, bg: t.goodSoft },
    warn: { c: t.onInk, b: "transparent", bg: t.sevWarn },
    neutral: { c: t.mutedInk, b: t.hairlineStrong, bg: "transparent" },
    neutralFill: { c: t.ink2, b: t.hairline, bg: t.inset },
  };
  const p = fg ? { c: fg, b: `${fg}66`, bg: "transparent" } : tones[tone];
  const paint: CSSProperties = {
    color: p.c,
    border: `0.5px solid ${p.b}`,
    background: p.bg,
  };

  return (
    <span
      style={{
        fontFamily: t.mono,
        whiteSpace: "nowrap",
        flexShrink: 0,
        display: "inline-block",
        ...(uppercase ? { textTransform: "uppercase" } : {}),
        ...sized,
        ...paint,
        ...style,
      }}
    >
      {children}
    </span>
  );
}
