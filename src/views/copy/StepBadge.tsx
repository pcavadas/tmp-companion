// src/views/copy/StepBadge.tsx — the numbered step pill (① / ②) of the Copy steps.
// A private feature-shared module (NOT in the barrel; never imports a sub-component).

import { useTheme } from "../../theme/ThemeContext";

export interface StepBadgeProps {
  n: number;
}

/** The numbered step pill (① / ②). */
export function StepBadge({ n }: StepBadgeProps) {
  const { t } = useTheme();
  return (
    <span
      style={{
        width: 20,
        height: 20,
        borderRadius: t.rPill,
        background: t.accent,
        color: t.onInk,
        fontFamily: t.mono,
        fontSize: t.fsData,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        flexShrink: 0,
      }}
    >
      {n}
    </span>
  );
}
