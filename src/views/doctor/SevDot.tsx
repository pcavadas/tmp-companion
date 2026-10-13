// src/views/doctor/SevDot.tsx — the severity dot that leads every Doctor finding row
// (filled + halo for high, filled for med, ring for low/ok).

import { useTheme } from "../../theme/ThemeContext";
import { sevTone, type Sev } from "./severity";

export interface SevDotProps {
  sev: Sev;
}

export function SevDot({ sev }: SevDotProps) {
  const { t } = useTheme();
  const tone = sevTone(t, sev);
  if (sev === "high") {
    return (
      <span
        style={{
          width: 9,
          height: 9,
          borderRadius: 999,
          background: tone.fg,
          boxShadow: `0 0 0 2.5px ${tone.soft}`,
          flexShrink: 0,
        }}
      />
    );
  }
  if (sev === "med") {
    return (
      <span
        style={{
          width: 8,
          height: 8,
          borderRadius: 999,
          background: tone.fg,
          flexShrink: 0,
        }}
      />
    );
  }
  return (
    <span
      style={{
        width: 7,
        height: 7,
        borderRadius: 999,
        border: `1.5px solid ${tone.fg}`,
        boxSizing: "border-box",
        flexShrink: 0,
      }}
    />
  );
}
