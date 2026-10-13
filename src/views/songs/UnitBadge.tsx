// src/views/songs/UnitBadge.tsx — the "on unit" lock badge of the Songs tab.

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";

/** "on unit" lock badge — presets (and their song assignments) live on the device. */
export interface UnitBadgeProps {
  note?: string;
}

export function UnitBadge({ note = "on unit" }: UnitBadgeProps) {
  const { t } = useTheme();
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: t.space2,
        fontFamily: t.mono,
        fontSize: t.fsTag,
        letterSpacing: "0.08em",
        textTransform: "uppercase",
        color: t.faint,
      }}
    >
      <Icon name="lock" size={10} stroke={t.faint} />
      {note}
    </span>
  );
}
