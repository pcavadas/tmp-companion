// src/views/level/RowCpu.tsx — compact per-preset CPU readout for a preset row.
//
// The look B CPU meter (ui/Meter), shown at the right of each preset row (never on the
// sound sub-rows). The value is the preset's REAL DSP load (sum of its blocks' costs, from
// the startup backup graph), against the device's per-preset budget; the meter turns
// danger and says "Over" by the device's own budget rule. No hover title: the figure says
// it.

import { Meter } from "../../ui/Meter";

export interface RowCpuProps {
  value: number;
}

export function RowCpu({ value }: RowCpuProps) {
  return <Meter value={value} />;
}
