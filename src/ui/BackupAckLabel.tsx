// src/ui/BackupAckLabel.tsx — the shared "I've backed up with Pro Control"
// acknowledgment (look B): a real Checkbox and its words in one <label>, 13 px, so a
// click on the box or the words ticks it. It gates a destructive write-to-unit action
// and sits in the footer just left of the action it gates (Start leveling, Save to your
// Tone Master Pro), so the wording and the gate read identically across the app.

import type { CSSProperties } from "react";

import { Checkbox } from "./primitives";

export interface BackupAckLabelProps {
  checked: boolean;
  /** Called with the toggled value. */
  onChange: (checked: boolean) => void;
  /** Extra layout for the label (e.g. a footer's padding). */
  style?: CSSProperties;
}

export function BackupAckLabel({
  checked,
  onChange,
  style,
}: BackupAckLabelProps) {
  return (
    <Checkbox
      checked={checked}
      onChange={(next) => {
        onChange(next);
      }}
      label="I've backed up with Pro Control"
      style={{ whiteSpace: "nowrap", ...style }}
    />
  );
}
