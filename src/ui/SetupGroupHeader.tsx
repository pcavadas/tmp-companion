// src/ui/SetupGroupHeader.tsx — the per-preset group header for a setup list.
//
// Slot number (via the DS SlotLabel — mono/fsData/mutedInk canon) + preset name
// (serif 15). Used by Doctor's DoctorSetup, which groups its option rows by preset
// (the Leveling wizard's own per-preset header is `PresetGroupRow`, design 1a).

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
        gap: t.space4,
        marginBottom: t.space3,
      }}
    >
      <SlotLabel index={slot} />
      <span style={{ fontFamily: t.serif, fontSize: t.fsName, color: t.ink }}>
        {name}
      </span>
    </div>
  );
}
