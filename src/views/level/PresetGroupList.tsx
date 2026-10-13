// src/views/level/PresetGroupList.tsx — the Level wizard's preset-list frame: a mono
// kicker over a scrollable body of `PresetGroupRow`s. Shared by all three stages.

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";

export interface PresetGroupListProps {
  label: string;
  children: ReactNode;
}

/** The list frame: a mono kicker over a scrollable body of `PresetGroupRow`s. */
export function PresetGroupList({ label, children }: PresetGroupListProps) {
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
          flexShrink: 0,
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          paddingBottom: t.space3,
        }}
      >
        <span
          style={{
            fontFamily: t.mono,
            fontSize: 10,
            letterSpacing: "0.12em",
            textTransform: "uppercase",
            color: t.faint,
          }}
        >
          {label}
        </span>
      </div>
      <div
        style={{
          flex: 1,
          minHeight: 0,
          overflowY: "auto",
          scrollbarGutter: "stable",
        }}
      >
        {children}
      </div>
    </div>
  );
}
