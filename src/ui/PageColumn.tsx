// src/ui/PageColumn.tsx — a SplitView page's content column (look B; Songs and Settings):
// the page title row (padding 16/24/12) over a body that scrolls on its own (padding
// 0/24/20, items 12 apart). One column, `maxWidth` at most and left-aligned, so a wide
// window keeps the rows a readable length instead of stretching them across the screen
// (Songs 720, Settings 640).

import type { ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";
import { PageHead } from "./PageHead";

export interface PageColumnProps {
  title: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
  /** The widest the column grows, in px. */
  maxWidth: number;
  children: ReactNode;
}

export function PageColumn({
  title,
  subtitle,
  actions,
  maxWidth,
  children,
}: PageColumnProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
      }}
    >
      <div
        style={{
          padding: `${String(t.sp16)}px ${String(t.sp24)}px ${String(t.sp12)}px`,
        }}
      >
        <div style={{ maxWidth }}>
          <PageHead title={title} subtitle={subtitle} actions={actions} />
        </div>
      </div>
      <div
        style={{
          flex: 1,
          minHeight: 0,
          overflowY: "auto",
          padding: `0 ${String(t.sp24)}px ${String(t.sp16 + 4)}px`,
        }}
      >
        <div
          style={{
            maxWidth,
            display: "flex",
            flexDirection: "column",
            gap: t.sp12,
          }}
        >
          {children}
        </div>
      </div>
    </div>
  );
}
