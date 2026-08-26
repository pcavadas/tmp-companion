// src/views/doctor/PresetFindingRow.tsx — shared chrome for a synthetic
// preset-level finding row: severity dot + serif title + one chip slot +
// caret, expanding into a padded body. One home for the shape so
// SceneConsistency's "Level jumps" and LevelingDamageRow's "Leveling damage"
// rows (byte-for-byte identical before this extraction, sev/title/chip and
// body content aside) can't drift apart.

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { ROW_MIN_HEIGHT, SevDot } from "./SoundRow";
import { sevTone, type Sev } from "./severity";

export interface PresetFindingRowProps {
  sev: Sev;
  title: string;
  /** The middle-row chip, e.g. a `DiagnosisChip` — callers build their own
   *  label, this component only lays it out. */
  chip: ReactNode;
  open: boolean;
  onToggle: () => void;
  /** The expanded body, rendered only while `open`. */
  children: ReactNode;
}

export function PresetFindingRow({
  sev,
  title,
  chip,
  open,
  onToggle,
  children,
}: PresetFindingRowProps) {
  const { t } = useTheme();
  const tone = sevTone(t, sev);

  return (
    <div style={{ borderTop: `0.5px solid ${t.hairline}` }}>
      <div
        onClick={onToggle}
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.space5,
          minHeight: ROW_MIN_HEIGHT,
          padding: `${String(t.space3)}px ${String(t.space4)}px ${String(t.space3)}px ${String(t.space3)}px`,
          cursor: "pointer",
          background: open ? t.rowSel : "transparent",
        }}
      >
        <SevDot sev={sev} />
        <span
          style={{
            fontFamily: t.serif,
            fontSize: 14,
            color: t.ink,
            whiteSpace: "nowrap",
            minWidth: 96,
            flexShrink: 0,
          }}
        >
          {title}
        </span>
        <span
          style={{
            flex: 1,
            minWidth: 0,
            display: "flex",
            alignItems: "center",
            gap: t.space3,
            overflow: "hidden",
          }}
        >
          {chip}
        </span>
        <span
          style={{
            width: 14,
            flexShrink: 0,
            display: "inline-flex",
            justifyContent: "center",
            opacity: 0.6,
          }}
        >
          <span
            style={{
              display: "inline-flex",
              transform: open ? "rotate(90deg)" : "none",
              transition: "transform 0.12s",
            }}
          >
            <Icon name="chev-right" size={13} stroke={tone.fg} />
          </span>
        </span>
      </div>

      {open && (
        <div
          style={{
            padding: `${String(t.space1)}px ${String(t.space5)}px ${String(t.space7)}px ${String(t.space11)}px`,
          }}
        >
          {children}
        </div>
      )}
    </div>
  );
}

export default PresetFindingRow;
