// src/ui/BackupAckLabel.tsx — the shared "I've backed up with Pro Control"
// acknowledgment label (inline checkbox + copy) that gates a destructive
// write-to-unit action. Used by the Leveling Set-up footer and the Copy save bar so
// the wording, the title hint, and the gate read identically across the app.

import type { CSSProperties } from "react";

import { useTheme } from "../theme/ThemeContext";
import { Checkbox } from "./primitives";

export interface BackupAckLabelProps {
  checked: boolean;
  /** Called with the toggled value (clicking the whole label flips it). */
  onChange: (checked: boolean) => void;
  /** Extra label styles (e.g. the Set-up footer's userSelect / paddingRight). */
  style?: CSSProperties;
}

export function BackupAckLabel({
  checked,
  onChange,
  style,
}: BackupAckLabelProps) {
  const { t } = useTheme();
  return (
    <label
      onClick={() => {
        onChange(!checked);
      }}
      title="Confirm you have a backup before writing to the unit"
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: t.space4,
        cursor: "pointer",
        ...style,
      }}
    >
      <Checkbox checked={checked} />
      <span
        style={{
          fontFamily: t.sans,
          fontSize: t.fsControl,
          color: t.ink2,
          whiteSpace: "nowrap",
        }}
      >
        I&apos;ve backed up with Pro Control
      </span>
    </label>
  );
}

export default BackupAckLabel;
