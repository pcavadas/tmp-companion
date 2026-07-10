// src/views/copy/CpuMeter.tsx — the per-target-card DSP-load meter.
//
// A 96×6 track with a fill (accent, or amber when over) whose width is the value as a
// % of 100, plus a 1.5px cap marker at the device's per-preset budget. The mono readout
// reads "<value> / <budget>"; over budget turns amber + bold and shows an "over budget"
// chip. The budget + per-block costs are the device's REAL figures (`models/cpu`).

import { useTheme } from "../../theme/ThemeContext";
import { Meter } from "../../ui/Meter";
import { Tag } from "../../ui/Tag";
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
      <Meter
        pct={pct}
        width={96}
        height={6}
        fillColor={over ? t.sevWarn : t.accent}
        marker={CPU_BUDGET}
        markerColor={over ? t.warn : t.faint}
      />
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
        <Tag tone="warn" uppercase>
          over budget
        </Tag>
      )}
    </div>
  );
}

export default CpuMeter;
