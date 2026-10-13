// src/views/level/SoundRowShell.tsx — one sound's line inside an open preset row, shared
// by the Run and Summary pages: a 14px status glyph, the sound's name (ellipsized to a
// fixed column), then the page's own trailing cells.

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";

export interface SoundRowShellProps {
  glyph: ReactNode;
  name: string;
  /** The name's colour (default ink2; the Run page fades a queued sound). */
  nameColor?: string;
  /** The trailing cells (status, LUFS, chips…). */
  children: ReactNode;
}

export function SoundRowShell({
  glyph,
  name,
  nameColor,
  children,
}: SoundRowShellProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: t.space6,
        minHeight: 30,
      }}
    >
      <span
        style={{
          width: 14,
          flexShrink: 0,
          display: "inline-flex",
          justifyContent: "center",
        }}
      >
        {glyph}
      </span>
      <span
        style={{
          fontFamily: t.sans,
          fontSize: 13,
          color: nameColor ?? t.ink2,
          width: 138,
          flexShrink: 0,
          whiteSpace: "nowrap",
          overflow: "hidden",
          textOverflow: "ellipsis",
        }}
      >
        {name}
      </span>
      {children}
    </div>
  );
}
