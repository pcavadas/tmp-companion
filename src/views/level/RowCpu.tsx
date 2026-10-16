// src/views/level/RowCpu.tsx — compact per-preset CPU readout for a base preset row.
//
// The look B CPU meter (ui/Meter), shown on the right of each BASE preset row (never on
// the scene/footswitch sub-rows). The value is the preset's REAL DSP load (sum of its
// blocks' costs, from the startup backup graph), against the device's per-preset budget;
// the meter turns danger and says "Over" by the device's own budget rule.

import { Meter } from "../../ui/Meter";
import { CPU_BUDGET, cpuStr } from "../../models/cpu";

export interface RowCpuProps {
  value: number;
}

export function RowCpu({ value }: RowCpuProps) {
  return (
    <Meter
      value={value}
      title={`Preset CPU — ${cpuStr(value)} of ${String(CPU_BUDGET)}%`}
    />
  );
}
