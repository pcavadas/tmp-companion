// src/ui/SplitView.tsx — a sidebar beside the content (look B): Songs, Catalog and
// Settings. The sidebar is a `nav` (190–210 px, `window` fill, a 1 px `border` on the
// right, padding 12/10, items 2 px apart) holding RailItems and section labels; the
// content takes the rest of the width on the same `window` grey and owns its own scroll.

import type { ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";

export interface SplitViewProps {
  /** The sidebar's contents (RailItems, section labels, a bottom action). */
  sidebar: ReactNode;
  /** Accessible name of the sidebar `nav` ("Settings categories"). */
  sidebarLabel: string;
  /** Sidebar width in px (190–210). Default 200. */
  sidebarWidth?: number;
  children: ReactNode;
}

export function SplitView({
  sidebar,
  sidebarLabel,
  sidebarWidth = 200,
  children,
}: SplitViewProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        display: "flex",
        background: t.window,
        fontFamily: t.fontSans,
      }}
    >
      <nav
        aria-label={sidebarLabel}
        style={{
          width: sidebarWidth,
          flexShrink: 0,
          boxSizing: "border-box",
          display: "flex",
          flexDirection: "column",
          gap: t.sp2,
          padding: `${String(t.sp12)}px ${String(t.sp8 + 2)}px`,
          borderRight: `1px solid ${t.border}`,
          background: t.window,
          overflowY: "auto",
        }}
      >
        {sidebar}
      </nav>
      <div
        style={{
          flex: 1,
          minWidth: 0,
          minHeight: 0,
          display: "flex",
          flexDirection: "column",
        }}
      >
        {children}
      </div>
    </div>
  );
}
