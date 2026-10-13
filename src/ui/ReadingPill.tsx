// src/ui/ReadingPill.tsx — the disabled "Reading presets…" spinner pill.
//
// Shown in a bottom-bar's primary-action slot while the background preset detail load
// (the ~22 s device backup) runs and gates the real action. Shared by the Presets-tab
// selection footer (Level) and the Copy step (Place the blocks).

import { useTheme } from "../theme/ThemeContext";
import { Spinner } from "./Spinner";

export interface ReadingPillProps {
  /** The pill text (the Copy step appends the determinate "… N%"). */
  label?: string;
}

export function ReadingPill({ label = "Reading presets…" }: ReadingPillProps) {
  const { t } = useTheme();
  return (
    <span
      title="Available once preset details finish loading"
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: t.space4,
        height: 28,
        boxSizing: "border-box",
        fontFamily: t.sans,
        fontSize: t.fsControl,
        fontWeight: 500,
        color: t.faint,
        background: "transparent",
        border: `0.5px solid ${t.hairline}`,
        borderRadius: t.rMd,
        padding: `0 ${String(t.space6)}px`,
        whiteSpace: "nowrap",
        cursor: "default",
      }}
    >
      <Spinner size={13} stroke={t.faint} />
      {label}
    </span>
  );
}

export default ReadingPill;
