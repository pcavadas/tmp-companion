// src/views/copy/CpuMeter.tsx — a target card's CPU readout: the caption "CPU" and the
// look B CPU meter (ui/Meter).
//
// The meter's bar is budget-relative and turns danger with "Over" by the SAME rule the
// card's validation uses (`overCpuBudget`), so the meter and the save gate can never
// disagree about a preset at exactly the budget. The budget + per-block costs are the
// device's REAL figures (`models/cpu`).

import { useTheme } from "../../theme/ThemeContext";
import { Meter } from "../../ui/Meter";
import { COPY_TEXT } from "./copyText";

export interface CpuMeterProps {
  value: number;
}

export function CpuMeter({ value }: CpuMeterProps) {
  const { t } = useTheme();
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: t.sp8,
        flexShrink: 0,
        fontFamily: t.fontSans,
        fontSize: t.fontCaption,
        color: t.textSecondary,
        whiteSpace: "nowrap",
      }}
    >
      {COPY_TEXT.cpu}
      <Meter value={value} />
    </span>
  );
}
