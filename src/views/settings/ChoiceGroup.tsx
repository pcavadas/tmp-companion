// src/views/settings/ChoiceGroup.tsx — one Playback setting as a white Group (look B):
// a section label, the choices as a SegmentedControl (named by the same words), and
// one 13/19 body line under them. A choice saves the moment it is clicked.

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { sectionLabel } from "../../theme/tokens";
import { Group } from "../../ui/List";
import { SegmentedControl, type SegmentedOption } from "../../ui/primitives";

export interface ChoiceGroupProps<T extends string> {
  /** The section label, also the choices' accessible name ("How loud you play"). */
  label: string;
  options: SegmentedOption<T>[];
  value: T;
  onChange: (value: T) => void;
  /** The one line that explains it (it may change with the choice). */
  body: ReactNode;
}

export function ChoiceGroup<T extends string>({
  label,
  options,
  value,
  onChange,
  body,
}: ChoiceGroupProps<T>) {
  const { t } = useTheme();
  return (
    <Group>
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          gap: t.sp8 + 2,
          padding: `${String(t.sp12 + 2)}px ${String(t.sp16)}px`,
        }}
      >
        <span style={sectionLabel(t)}>{label}</span>
        <SegmentedControl
          options={options}
          value={value}
          onChange={onChange}
          ariaLabel={label}
        />
        <p
          style={{
            margin: 0,
            fontSize: t.fontBody,
            lineHeight: `${String(t.lineBody)}px`,
            color: t.text,
          }}
        >
          {body}
        </p>
      </div>
    </Group>
  );
}
