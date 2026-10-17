// src/ui/Footer.tsx — the ONE bottom bar every flow screen ends in (look B).
//
// 56 high, `window` fill, a 1 px `border` rule on top, padding 0 16 px. Left: the
// secondary, back or cancel actions. Right: a caption or the reason a main action is
// greyed, then the main action, always visible (pushed right even when the left is
// empty). It replaced the old ActionBar and the wizard's own footer.

import type { ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";

export interface FooterProps {
  /** Secondary / back / cancel actions, or a status line. */
  left?: ReactNode;
  /** A caption or reason, then the main action. */
  right?: ReactNode;
}

export function Footer({ left, right }: FooterProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        flexShrink: 0,
        minHeight: t.footer,
        boxSizing: "border-box",
        display: "flex",
        alignItems: "center",
        gap: t.sp12,
        padding: `0 ${String(t.sp16)}px`,
        borderTop: `1px solid ${t.border}`,
        background: t.window,
        fontFamily: t.fontSans,
        fontSize: t.fontBody,
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.sp12,
          minWidth: 0,
        }}
      >
        {left}
      </div>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.sp12,
          marginLeft: "auto",
          minWidth: 0,
        }}
      >
        {right}
      </div>
    </div>
  );
}
