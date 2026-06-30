// src/views/copy/CpuMeter.tsx — the per-target-card DSP-load meter.
//
// A 96×6 track with a fill (accent, or amber when over) whose width is the value as a
// % of 100, plus a 1.5px cap marker at the device's per-preset budget. The mono readout
// reads "<value> / <budget>"; over budget turns amber + bold and shows an "over budget"
// chip. The budget + per-block costs are the device's REAL figures (`models/cpu`).

import { useTheme } from "../../theme/ThemeContext";
import { CPU_BUDGET } from "../../models/cpu";

export interface CpuMeterProps {
  value: number;
}

export function CpuMeter({ value }: CpuMeterProps) {
  const { t } = useTheme();
  const over = value > CPU_BUDGET;
  const pct = Math.min(100, value);
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 9,
        flexShrink: 0,
      }}
    >
      <div
        style={{
          position: "relative",
          width: 96,
          height: 6,
          borderRadius: t.rPill,
          background: t.track,
          overflow: "hidden",
        }}
      >
        <div
          style={{
            position: "absolute",
            left: 0,
            top: 0,
            bottom: 0,
            width: `${String(pct)}%`,
            background: over ? t.sevWarn : t.accent,
            borderRadius: t.rPill,
          }}
        />
        <div
          style={{
            position: "absolute",
            left: `${String(CPU_BUDGET)}%`,
            top: -1,
            bottom: -1,
            width: 1.5,
            background: over ? t.warn : t.faint,
          }}
        />
      </div>
      <span
        style={{
          fontFamily: t.mono,
          fontSize: t.fsMeta,
          color: over ? t.sevWarn : t.ink2,
          fontWeight: over ? 600 : 500,
          whiteSpace: "nowrap",
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {value.toFixed(1)}
        <span style={{ color: t.faint, fontWeight: 400 }}>
          {" / "}
          {CPU_BUDGET}
        </span>
      </span>
      {over && (
        <span
          style={{
            fontFamily: t.mono,
            fontSize: t.fsTag,
            letterSpacing: t.lsCaption,
            textTransform: "uppercase",
            color: t.onInk,
            background: t.sevWarn,
            borderRadius: t.rSm,
            padding: "2px 6px",
            whiteSpace: "nowrap",
          }}
        >
          over budget
        </span>
      )}
    </div>
  );
}

export default CpuMeter;
