// src/ui/Toolbar.tsx — the app's one top bar (look B): `size.toolbar` (52) high on the
// `window` grey with a `border` rule below, padding 0 16. A three-column grid: the left
// column stays empty, the tabs sit centred, and the status (the device readout) sits at
// the right. The tabs are a segmented-style bar — a `segmentTrack` track with 2 px padding
// (`radiusPanel`), each tab 24 high (13 px text on a 16 px line); the current tab is white with `shadowSegment` and semibold — but each
// tab is a plain button with `aria-current="page"`, not a radio: it navigates. The tabs
// never stretch, whatever the window width. Click-only: no keyboard shortcuts.

import type { ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";

export interface ToolbarTab<T extends string> {
  id: T;
  label: string;
}

export interface ToolbarProps<T extends string> {
  tabs: readonly ToolbarTab<T>[];
  current: T;
  onSelect: (id: T) => void;
  /** Right-aligned readout (the device status). */
  status: ReactNode;
}

export function Toolbar<T extends string>({
  tabs,
  current,
  onSelect,
  status,
}: ToolbarProps<T>) {
  const { t } = useTheme();
  return (
    <header
      style={{
        height: t.toolbar,
        flexShrink: 0,
        boxSizing: "border-box",
        display: "grid",
        // Equal side columns keep the tabs centred; the status column never shrinks
        // below its words, so in a narrow window the tabs give way (by a few px at
        // 800) instead of the status running into them.
        gridTemplateColumns: "minmax(0, 1fr) auto minmax(max-content, 1fr)",
        alignItems: "center",
        gap: t.sp12,
        padding: `0 ${String(t.sp16)}px`,
        borderBottom: `1px solid ${t.border}`,
        background: t.window,
        fontFamily: t.fontSans,
      }}
    >
      <span />
      <nav
        aria-label="Tabs"
        style={{
          display: "inline-flex",
          padding: t.sp2,
          gap: t.sp2,
          borderRadius: t.radiusPanel,
          background: t.segmentTrack,
        }}
      >
        {tabs.map((tab) => {
          const on = tab.id === current;
          return (
            <button
              key={tab.id}
              type="button"
              aria-current={on ? "page" : undefined}
              onClick={() => {
                onSelect(tab.id);
              }}
              style={{
                border: "none",
                cursor: "pointer",
                fontFamily: t.fontSans,
                fontSize: t.fontBody,
                lineHeight: `${String(t.lineCaption)}px`,
                fontWeight: on ? t.weightSemibold : t.weightMedium,
                color: t.text,
                padding: `${String(t.sp4)}px ${String(t.sp12)}px`,
                borderRadius: t.radiusControl,
                background: on ? t.surface : "transparent",
                boxShadow: on ? t.shadowSegment : "none",
              }}
            >
              {tab.label}
            </button>
          );
        })}
      </nav>
      <div style={{ justifySelf: "end", minWidth: 0, display: "flex" }}>
        {status}
      </div>
    </header>
  );
}
