// src/views/copy/CpuMeter.tsx — the per-target-card DSP-load meter.
//
// The look B CPU meter (ui/Meter): a budget-relative bar and the figure, turning danger
// with "Over" by the SAME rule the card's validation uses (`overCpuBudget`), so the meter
// and the save gate can never disagree about a preset at exactly the budget. The budget
// + per-block costs are the device's REAL figures (`models/cpu`).

import { Meter } from "../../ui/Meter";

export interface CpuMeterProps {
  value: number;
}

export function CpuMeter({ value }: CpuMeterProps) {
  return <Meter value={value} />;
}
