// src/ui/PageHead.tsx — a page's title row (look B): a 20/26 semibold title with the
// title tracking (`trackingTitle`, the one letter-spacing look B keeps), an optional
// 13 px secondary subtitle only when it adds a fact (a count), and the page's actions —
// or its `Steps` — at the right.

import type { ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";

export interface PageHeadProps {
  title: ReactNode;
  /** A fact the title doesn't give ("10 songs on your Tone Master Pro"). */
  subtitle?: ReactNode;
  /** Right side: the page's buttons, or its Steps. */
  actions?: ReactNode;
}

export function PageHead({ title, subtitle, actions }: PageHeadProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        display: "flex",
        alignItems: subtitle != null ? "flex-start" : "baseline",
        justifyContent: "space-between",
        gap: t.sp12,
        fontFamily: t.fontSans,
        minWidth: 0,
      }}
    >
      <div style={{ minWidth: 0 }}>
        <h1
          style={{
            margin: 0,
            fontSize: t.fontTitle,
            lineHeight: `${String(t.lineTitle)}px`,
            fontWeight: t.weightSemibold,
            letterSpacing: t.trackingTitle,
            color: t.text,
          }}
        >
          {title}
        </h1>
        {subtitle != null && (
          <p
            style={{
              margin: `${String(t.sp2)}px 0 0`,
              fontSize: t.fontBody,
              lineHeight: `${String(t.lineBody)}px`,
              color: t.textSecondary,
            }}
          >
            {subtitle}
          </p>
        )}
      </div>
      {actions != null && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: t.sp8,
            flexShrink: 0,
          }}
        >
          {actions}
        </div>
      )}
    </div>
  );
}
