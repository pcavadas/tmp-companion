// src/views/level/PresetGroupRow.tsx — the redesigned Level wizard's per-preset
// collapsible row (design handoff 1a): the list counts PRESETS, not sounds — one row
// per preset, its sounds nested inside when open. Shared by all three stages (Set up
// / Level / Summary); each stage supplies its own glyph/tag/note/value for the
// collapsed row and its own children for the expanded body.
//
// Open/closed state is owned by the caller (`useGroupOpen`) — this component is a
// pure renderer plus the click target that calls `onToggle`.

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { SlotLabel } from "../../ui/SlotLabel";

export interface PresetGroup<T> {
  slot: number;
  name: string;
  items: T[];
}

export interface PresetGroupListProps {
  label: string;
  action?: string;
  onAction?: () => void;
  children: ReactNode;
}

/** The list frame: a mono kicker + optional accent action link, over a scrollable
 *  body of `PresetGroupRow`s. */
export function PresetGroupList({
  label,
  action,
  onAction,
  children,
}: PresetGroupListProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
      }}
    >
      <div
        style={{
          flexShrink: 0,
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          paddingBottom: t.space3,
        }}
      >
        <span
          style={{
            fontFamily: t.mono,
            fontSize: 10,
            letterSpacing: "0.12em",
            textTransform: "uppercase",
            color: t.faint,
          }}
        >
          {label}
        </span>
        {action && (
          <span
            onClick={onAction}
            style={{
              fontFamily: t.sans,
              fontSize: 12.5,
              color: t.accentDeep,
              cursor: "pointer",
            }}
          >
            {action}
          </span>
        )}
      </div>
      <div
        style={{
          flex: 1,
          minHeight: 0,
          overflowY: "auto",
          scrollbarGutter: "stable",
        }}
      >
        {children}
      </div>
    </div>
  );
}

export interface PresetGroupRowProps {
  slot: number;
  name: string;
  open: boolean;
  onToggle: () => void;
  /** Leading glyph (16px slot) — a status dot/check/spinner/warning, or nothing. */
  glyph?: ReactNode;
  tag?: ReactNode;
  /** Trailing note (mono, faint) — hidden while the row is open (the children speak
   *  for themselves once expanded). */
  note?: string;
  value?: ReactNode;
  valueColor?: string;
  children: ReactNode;
}

/** One preset row + its sounds — the design-1a `Lv1PresetRow`. */
export function PresetGroupRow({
  slot,
  name,
  open,
  onToggle,
  glyph,
  tag,
  note,
  value,
  valueColor,
  children,
}: PresetGroupRowProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        borderTop: `0.5px solid ${t.hairline}`,
        background: open ? t.accentSoft : "transparent",
      }}
    >
      <div
        onClick={onToggle}
        data-preset-group={slot}
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.space6,
          height: 44,
          padding: `0 ${String(t.space5)}px`,
          cursor: "pointer",
        }}
      >
        <span
          style={{
            width: 16,
            flexShrink: 0,
            display: "inline-flex",
            justifyContent: "center",
          }}
        >
          {glyph}
        </span>
        <span
          style={{
            fontFamily: t.serif,
            fontSize: 16,
            color: t.ink,
            width: 150,
            flexShrink: 0,
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {name}
        </span>
        <SlotLabel index={slot} faint style={{ width: 58, flexShrink: 0 }} />
        {tag}
        <span
          style={{
            flex: 1,
            minWidth: 0,
            fontFamily: t.mono,
            fontSize: 10.5,
            color: t.faint,
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {open ? "" : note}
        </span>
        <span
          style={{
            fontFamily: t.mono,
            fontSize: 12,
            color: valueColor ?? t.faint,
            fontVariantNumeric: "tabular-nums",
            flexShrink: 0,
          }}
        >
          {value}
        </span>
        <span style={{ flexShrink: 0, display: "inline-flex" }}>
          <Icon
            name={open ? "chev-down" : "chev-right"}
            size={13}
            stroke={t.faint}
            strokeWidth={1.6}
          />
        </span>
      </div>
      {open && (
        <div
          style={{
            padding: `0 ${String(t.space5)}px ${String(t.space5)}px 44px`,
            display: "flex",
            flexDirection: "column",
          }}
        >
          {children}
        </div>
      )}
    </div>
  );
}

export default PresetGroupRow;
