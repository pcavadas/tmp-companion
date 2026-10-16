// src/ui/Rail.tsx — the shared sidebar pieces (look B): the Songs and Settings rails.
// `SplitView` lays a sidebar beside the content; these are what goes in it.
//
// `Rail` is the column itself (200 px, `window` fill, a 1 px `border` on the right,
// padding 12/10, items 2 px apart) for the screens that still lay it out by hand.
// `RailLabel` is a section label (12 px semibold, secondary, sentence case). `RailItem`
// is a 28-high row: 13 px, `radiusControl`, an optional leading icon, an optional count
// at the right in secondary text; the current item is `rowSelected` with semibold text
// (no accent bar). Rows are real <button>s. `nav="tab"` rows sit in the caller's
// role="tablist" (Settings) and carry aria-selected; `nav="current"` rows are a plain
// navigation list (Songs) and mark the shown item with aria-current.

import type { CSSProperties, ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";
import { sectionLabel } from "../theme/tokens";
import { Icon } from "./Icon";
import type { IconName } from "./Icon";

interface RailProps {
  children: ReactNode;
  /** Per-view layout extras. */
  style?: CSSProperties;
}

/** The sidebar column: 200 px, window fill, a border on the right, 12/10 padding. */
export function Rail({ children, style }: RailProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        width: 200,
        boxSizing: "border-box",
        flexShrink: 0,
        minHeight: 0,
        borderRight: `1px solid ${t.border}`,
        background: t.window,
        display: "flex",
        flexDirection: "column",
        gap: t.sp2,
        padding: `${String(t.sp12)}px ${String(t.sp8 + 2)}px`,
        fontFamily: t.fontSans,
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

/** A sidebar section label ("Setlists") — sentence case, 12 px semibold, secondary. */
export function RailLabel({ children, style }: RailLabelProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        ...sectionLabel(t),
        padding: `${String(t.sp4)}px ${String(t.sp8)}px`,
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
  /** "tab" inside a role="tablist"; "current" in a plain navigation list. */
  nav: "tab" | "current";
  /** Leading icon. */
  icon?: IconName;
  /** Trailing count (or DASH), in secondary text. */
  meta?: ReactNode;
}

export function RailItem({
  label,
  active,
  onClick,
  nav,
  icon,
  meta,
}: RailItemProps) {
  const { t } = useTheme();
  return (
    <button
      type="button"
      role={nav === "tab" ? "tab" : undefined}
      aria-selected={nav === "tab" ? active : undefined}
      aria-current={nav === "current" && active ? true : undefined}
      onClick={onClick}
      style={{
        appearance: "none",
        margin: 0,
        border: "none",
        flexShrink: 0,
        width: "100%",
        height: t.control,
        boxSizing: "border-box",
        display: "flex",
        alignItems: "center",
        gap: t.sp8,
        padding: `0 ${String(t.sp8)}px`,
        borderRadius: t.radiusControl,
        background: active ? t.rowSelected : "transparent",
        fontFamily: t.fontSans,
        fontSize: t.fontBody,
        fontWeight: active ? t.weightSemibold : t.weightRegular,
        color: t.text,
        textAlign: "left",
        cursor: "pointer",
        userSelect: "none",
      }}
    >
      {icon && (
        <span style={{ flexShrink: 0, display: "flex" }}>
          <Icon
            name={icon}
            size={14}
            stroke={active ? t.text : t.textSecondary}
            strokeWidth={1.6}
          />
        </span>
      )}
      <span
        style={{
          flex: 1,
          minWidth: 0,
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
            fontSize: t.fontCaption,
            fontWeight: t.weightRegular,
            color: active ? t.text : t.textSecondary,
            fontVariantNumeric: "tabular-nums",
            flexShrink: 0,
          }}
        >
          {meta}
        </span>
      )}
    </button>
  );
}
