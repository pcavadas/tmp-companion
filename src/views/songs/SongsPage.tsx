// src/views/songs/SongsPage.tsx — the Songs tab's content column (look B): the page
// title row (padding 16/24/12) over a scrolling body (padding 0/24/20, items 12 apart).
// One column, 720 px at most and left-aligned, so a wide window keeps the rows a
// readable length instead of stretching them across the screen.

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { PageHead } from "../../ui/PageHead";

/** The widest the Songs content column grows (Settings uses 640). */
const SONGS_COLUMN_MAX = 720;

export interface SongsPageProps {
  title: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
}

export function SongsPage({
  title,
  subtitle,
  actions,
  children,
}: SongsPageProps) {
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
        <div style={{ maxWidth: SONGS_COLUMN_MAX }}>
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
            maxWidth: SONGS_COLUMN_MAX,
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
