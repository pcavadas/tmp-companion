// src/views/level/RowCpu.tsx — compact per-preset CPU readout for a base preset row.
//
// A thin usage bar + percentage, shown on the right of each BASE preset row (never on
// the scene/footswitch sub-rows). The value is the preset's REAL DSP load (sum of its
// blocks' costs, from the startup backup graph), against the device's per-preset cap.
// Turns warn-coloured once a preset crosses the cap. Echoes the hero strip's CPU meter.

import { useTheme } from "../../theme/ThemeContext";
import { Meter } from "../../ui/Meter";
import { CPU_BUDGET, cpuStr } from "../../models/cpu";

export interface RowCpuProps {
  value: number;
}

export function RowCpu({ value }: RowCpuProps) {
  const { t } = useTheme();
  const over = value > CPU_BUDGET;
  const pct = Math.min(100, (value / CPU_BUDGET) * 100);
  return (
    <span
      title={`Preset CPU — ${cpuStr(value)} of ${String(CPU_BUDGET)}%`}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        flexShrink: 0,
      }}
    >
      <Meter
        pct={pct}
        width={34}
        height={4}
        trackColor={t.hairline}
        fillColor={over ? t.warn : t.accent}
      />
      <span
        style={{
          fontFamily: t.mono,
          // = 9.5, the same micro-mono size as the adjacent row meta (metaStyle).
          fontSize: t.fsMicro,
          letterSpacing: "0.02em",
          color: over ? t.warn : t.mutedInk,
          fontVariantNumeric: "tabular-nums",
          whiteSpace: "nowrap",
          width: 36,
          textAlign: "right",
        }}
      >
        {cpuStr(value)}
      </span>
    </span>
  );
}

export default RowCpu;
