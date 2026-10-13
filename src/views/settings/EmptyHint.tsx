// src/views/settings/EmptyHint.tsx — the italic muted empty-state bar of the Settings
// panes (Loudness targets, Instruments, the initial read).

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";

export interface EmptyHintProps {
  children: ReactNode;
}

// Italic muted empty-state bar shared by the Targets and Instruments panes.
export function EmptyHint({ children }: EmptyHintProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        fontFamily: t.sans,
        fontSize: t.fsUi,
        color: t.faint,
        padding: `${String(t.space7)}px 0`,
        fontStyle: "italic",
      }}
    >
      {children}
    </div>
  );
}
