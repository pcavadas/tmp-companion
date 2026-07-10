// src/ui/SetupGroupHeader.tsx — the per-preset group header for a setup list.
//
// Slot number (via the DS SlotLabel — mono/fsData/mutedInk canon) + preset name
// (serif 15). Shared by the Leveling SetupBody and the Doctor DoctorSetup, which
// both group their option rows by preset.

import { useTheme } from "../theme/ThemeContext";
import { SlotLabel } from "./SlotLabel";

export interface SetupGroupHeaderProps {
  /** 0-based list index — rendered through SlotLabel/slotLabel(). */
  slot: number;
  name: string;
}

export function SetupGroupHeader({ slot, name }: SetupGroupHeaderProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        display: "flex",
        alignItems: "baseline",
        gap: 9,
        marginBottom: 6,
      }}
    >
      <SlotLabel index={slot} />
      <span style={{ fontFamily: t.serif, fontSize: 15, color: t.ink }}>
        {name}
      </span>
    </div>
  );
}
