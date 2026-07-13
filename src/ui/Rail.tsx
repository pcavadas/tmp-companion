// src/ui/Rail.tsx — the shared left-rail idiom (the Songs setlist rail + the
// Settings category rail): a 210px bgAlt column behind a 0.5px hairline edge,
// mono micro-labels, and serif rows with a 2px left-accent bar when active.
// Rows are role="tab" (the rail always picks what the pane shows) with a
// leading icon slot (Settings categories) and/or a trailing mono meta slot
// (Songs counts).

import type { CSSProperties, ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";
import { Icon } from "./Icon";
import type { IconName } from "./Icon";

interface RailProps {
  children: ReactNode;
  /** Per-view layout extras (e.g. the Songs rail's `gap: 2`). */
  style?: CSSProperties;
}

/** The rail column: 210px, bgAlt fill, hairline right edge, "12px 10px" pad. */
export function Rail({ children, style }: RailProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        width: 210,
        boxSizing: "border-box",
        flexShrink: 0,
        minHeight: 0,
        borderRight: `0.5px solid ${t.hairline}`,
        background: t.bgAlt,
        display: "flex",
        flexDirection: "column",
        padding: `${String(t.space6)}px ${String(t.space5)}px`,
        ...style,
      }}
    >
      {children}
    </div>
  );
}

interface RailLabelProps {
  children: ReactNode;
  /** Per-site spacing (the label's padding differs by rail section). */
  style?: CSSProperties;
}

/** The rail's mono micro-label ("SETTINGS", "SETLISTS", …) — the faint lsWide
 * variant, deliberately lighter than `s.kicker`. */
export function RailLabel({ children, style }: RailLabelProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        fontFamily: t.mono,
        fontSize: t.fsMicro,
        letterSpacing: t.lsWide,
        color: t.faint,
        textTransform: "uppercase",
        ...style,
      }}
    >
      {children}
    </div>
  );
}

interface RailItemProps {
  label: string;
  active: boolean;
  onClick: () => void;
  /** Leading icon (the Settings category rows). */
  icon?: IconName;
  /** Trailing mono meta — a count or DASH (the Songs setlist rows). */
  meta?: ReactNode;
}

export function RailItem({
  label,
  active,
  onClick,
  icon,
  meta,
}: RailItemProps) {
  const { t } = useTheme();
  return (
    <div
      role="tab"
      aria-selected={active}
      onClick={onClick}
      style={{
        display: "flex",
        alignItems: "center",
        gap: t.space4,
        padding: `${String(t.space4)}px ${String(t.space5)}px`,
        borderRadius: t.rMd,
        cursor: "pointer",
        userSelect: "none",
        background: active ? t.accentSoft : "transparent",
        borderLeft: active ? `2px solid ${t.accent}` : "2px solid transparent",
      }}
    >
      {icon && (
        <span style={{ flexShrink: 0, display: "flex" }}>
          <Icon
            name={icon}
            size={15}
            stroke={active ? t.accentDeep : t.mutedInk}
            strokeWidth={1.6}
          />
        </span>
      )}
      <span
        style={{
          flex: 1,
          minWidth: 0,
          fontFamily: t.serif,
          fontSize: t.fsName2,
          color: active ? t.ink : t.ink2,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
      >
        {label}
      </span>
      {meta != null && (
        <span
          style={{
            fontFamily: t.mono,
            fontSize: t.fsData2,
            color: t.faint,
            flexShrink: 0,
          }}
        >
          {meta}
        </span>
      )}
    </div>
  );
}
