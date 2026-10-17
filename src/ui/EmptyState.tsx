// src/ui/EmptyState.tsx — the DS empty state (look B). Device and app states are a
// `PageState`; this is the in-pane "nothing here" message (it was `PaneEmpty`).
//
// Says what's missing and offers the one thing to do about it, centred in its pane: a
// 24 px outline icon in `textDisabled`, a 13/600 title (h2), an optional one-line body
// in secondary text, and an optional single action (the caller composes the Button).
// `size="page"` fills a tab: 32 px icon, 15 px title.

import type { ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";
import { Icon } from "./Icon";
import type { IconName } from "./iconNames";

export interface EmptyStateProps {
  icon: IconName;
  title: string;
  body?: ReactNode;
  /** optional action rendered under the body (caller composes the Button). */
  cta?: ReactNode;
  /** "pane" (default) sits in a list; "page" fills a tab. */
  size?: "pane" | "page";
}

export function EmptyState({
  icon,
  title,
  body,
  cta,
  size = "pane",
}: EmptyStateProps) {
  const { t } = useTheme();
  const page = size === "page";
  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: t.sp8,
        padding: `${String(t.sp24)}px ${String(t.sp16)}px`,
        textAlign: "center",
        fontFamily: t.fontSans,
      }}
    >
      <Icon
        name={icon}
        size={page ? 32 : 24}
        stroke={t.textDisabled}
        strokeWidth={1.5}
      />
      <h2
        style={{
          margin: 0,
          fontSize: page ? t.fontLarge : t.fontBody,
          lineHeight: `${String(page ? t.lineLarge : t.lineBody)}px`,
          fontWeight: t.weightSemibold,
          color: t.text,
        }}
      >
        {title}
      </h2>
      {body != null && (
        <div
          style={{
            maxWidth: 360,
            fontSize: t.fontBody,
            lineHeight: `${String(t.lineBody)}px`,
            color: t.textSecondary,
          }}
        >
          {body}
        </div>
      )}
      {cta != null && <div style={{ marginTop: t.sp6 }}>{cta}</div>}
    </div>
  );
}
