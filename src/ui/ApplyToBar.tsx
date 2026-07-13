// src/ui/ApplyToBar.tsx — the "apply to all / to the ticked" brush band.
//
// The tinted header band above a setup list: a mono kicker label (kickerWide), a
// "Clear ticks" affordance shown only while some rows are ticked, and the bulk
// pickers as children. Shared by the Leveling SetupBody and the Doctor DoctorSetup.
//
// The kicker color and the Clear affordance both key off `somePicked` (the frozen
// {label,onClear,children} shape can't carry the conditional color/visibility, so
// this one optional flag is added — see the lane report).

import type { ReactNode } from "react";

import { useTheme, useStyles } from "../theme/ThemeContext";

export interface ApplyToBarProps {
  /** Full kicker text ("Apply to all 3 sounds" / "Instrument for the 2 ticked"). */
  label: string;
  onClear: () => void;
  /** Some rows ticked → accentDeep kicker + a visible "Clear ticks" link. */
  somePicked?: boolean;
  /** The bulk pickers (caller owns their wrapper layout). */
  children?: ReactNode;
}

export function ApplyToBar({
  label,
  onClear,
  somePicked = false,
  children,
}: ApplyToBarProps) {
  const { t } = useTheme();
  const s = useStyles();
  return (
    <div
      style={{
        flexShrink: 0,
        padding: `${String(t.space6)}px ${String(t.space10)}px ${String(t.space7)}px`,
        background: t.bgAlt,
        borderBottom: `0.5px solid ${t.hairline}`,
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          marginBottom: t.space4,
        }}
      >
        <span style={s.kickerWide(somePicked ? t.accentDeep : t.faint)}>
          {label}
        </span>
        {somePicked && (
          <span
            onClick={onClear}
            style={{
              fontFamily: t.sans,
              fontSize: t.fsLabel,
              color: t.accentDeep,
              cursor: "pointer",
              whiteSpace: "nowrap",
              flexShrink: 0,
              paddingLeft: t.space6,
            }}
          >
            Clear ticks
          </span>
        )}
      </div>
      {children}
    </div>
  );
}
