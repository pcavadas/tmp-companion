// src/views/doctor/DiagnosisChip.tsx — a pure, non-interactive severity pill shown
// in a sound row's middle. The whole ROW expands now (owned by DoctorResults), so
// this chip only labels a diagnosis at its severity color; the explanation, the
// frequency picture, and the fixes live in the row's expanded region.

import { useTheme } from "../../theme/ThemeContext";
import { sevTone, type Sev } from "./severity";

export interface DiagnosisChipProps {
  /** Already possible-aware (see `severity.ts::possibleLabel`) — this component
   *  renders it verbatim, it doesn't build the "Possible …" prefix itself. */
  label: string;
  sev: Sev;
  /** A near-threshold, low-confidence verdict: rendered muted, hollow chip. */
  possible?: boolean;
}

export function DiagnosisChip({
  label,
  sev,
  possible = false,
}: DiagnosisChipProps) {
  const { t } = useTheme();
  const tone = sevTone(t, sev);
  return (
    <span
      style={{
        fontFamily: t.sans,
        fontSize: 11,
        fontWeight: 500,
        // Possible = muted: drop the severity tint to a neutral, hollow chip so a
        // near-threshold guess doesn't read as a firm finding.
        color: possible ? t.mutedInk : tone.fg,
        background: possible ? "transparent" : tone.soft,
        border: `0.5px solid ${possible ? t.hairlineStrong : tone.border}`,
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
