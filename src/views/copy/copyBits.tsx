// src/views/copy/copyBits.tsx — small presentational bits shared by the Copy steps.
// A private feature-shared module (NOT in the barrel; never imports a sub-component).

import type { ReactNode } from "react";
import { useTheme } from "../../theme/ThemeContext";
import { Tag } from "../../ui/Tag";

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

/** The green "ON UNIT" chip — the preset currently loaded on the device. */
export function OnUnitChip() {
  return <Tag tone="good">ON UNIT</Tag>;
}

export interface MiniLinkProps {
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
}

/** A small text action ("Select all" / "Clear"). */
export function MiniLink({ children, onClick, disabled }: MiniLinkProps) {
  const { t } = useTheme();
  return (
    <span
      role="button"
      onClick={
        disabled
          ? undefined
          : () => {
              onClick();
            }
      }
      style={{
        fontFamily: t.sans,
        fontSize: t.fsUi,
        color: disabled ? t.faint : t.accentDeep,
        cursor: disabled ? "default" : "pointer",
        whiteSpace: "nowrap",
      }}
    >
      {children}
    </span>
  );
}
