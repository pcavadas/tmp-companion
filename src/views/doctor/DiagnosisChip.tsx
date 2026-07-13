// src/views/doctor/DiagnosisChip.tsx — a pure, non-interactive severity pill shown
// in a sound row's middle. The whole ROW expands now (owned by DoctorResults), so
// this chip only labels a diagnosis at its severity color; the explanation, the
// frequency picture, and the fixes live in the row's expanded region.

import { useTheme } from "../../theme/ThemeContext";
import { sevTone, type Sev } from "./severity";

export interface DiagnosisChipProps {
  label: string;
  sev: Sev;
}

export function DiagnosisChip({ label, sev }: DiagnosisChipProps) {
  const { t } = useTheme();
  const tone = sevTone(t, sev);
  return (
    <span
      style={{
        fontFamily: t.sans,
        fontSize: 11,
        fontWeight: 500,
        color: tone.fg,
        background: tone.soft,
        border: `0.5px solid ${tone.border}`,
        borderRadius: 5,
        padding: `${String(t.space1)}px ${String(t.space3)}px`,
        whiteSpace: "nowrap",
        flexShrink: 0,
      }}
    >
      {label}
    </span>
  );
}

export default DiagnosisChip;
