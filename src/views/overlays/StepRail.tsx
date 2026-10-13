// src/views/overlays/StepRail.tsx — the wizard's numbered step rail (Set up · Level ·
// Summary by default), drawn inside `WizardHeader`.

import { Fragment } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { WIZ_STEPS, type RailStep } from "./wizardContext";

// ---- step rail (header) ----------------------------------------------------
// Exported so the full-page Level wizard (`LevelPage`) reuses the identical rail. `steps`
// defaults to the leveling rail.
export interface StepRailProps {
  current: number;
  steps?: readonly RailStep[];
}

export function StepRail({ current, steps = WIZ_STEPS }: StepRailProps) {
  const { t } = useTheme();
  return (
    <div style={{ display: "flex", alignItems: "center" }}>
      {steps.map((s, i) => {
        const done = i < current;
        const active = i === current;
        const filled = done || active;
        return (
          <Fragment key={s.key}>
            <div
              style={{ display: "flex", alignItems: "center", gap: t.space4 }}
            >
              <span
                style={{
                  width: 22,
                  height: 22,
                  borderRadius: 999,
                  flexShrink: 0,
                  display: "inline-flex",
                  alignItems: "center",
                  justifyContent: "center",
                  background: filled ? t.accent : "transparent",
                  border: `1px solid ${filled ? t.accent : t.hairlineStrong}`,
                  fontFamily: t.mono,
                  fontSize: 11,
                  fontWeight: 500,
                  color: filled ? t.onInk : t.faint,
                }}
              >
                {done ? (
                  <Icon
                    name="check"
                    size={12}
                    stroke={t.onInk}
                    strokeWidth={2.4}
                  />
                ) : (
                  i + 1
                )}
              </span>
              <span
                style={{
                  fontFamily: t.mono,
                  fontSize: 10.5,
                  letterSpacing: "0.1em",
                  textTransform: "uppercase",
                  color: active ? t.ink : done ? t.mutedInk : t.faint,
                  whiteSpace: "nowrap",
                }}
              >
                {s.label}
              </span>
            </div>
            {i < steps.length - 1 && (
              <span
                style={{
                  flex: 1,
                  height: 1,
                  margin: `0 ${String(t.space6)}px`,
                  background: i < current ? t.accent : t.hairlineStrong,
                  minWidth: 16,
                }}
              />
            )}
          </Fragment>
        );
      })}
    </div>
  );
}
