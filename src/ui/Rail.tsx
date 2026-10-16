// src/ui/Rail.tsx — the shared sidebar pieces (look B) for the Songs, Catalog and
// Settings sidebars. `SplitView` draws the sidebar column; these are what goes in it.
//
// `RailLabel` is a section label (12 px semibold, secondary, sentence case). `RailItem`
// is a 28-high row: 13 px, `radiusControl`, an optional count at the right in secondary
// text, optionally indented 22 px under the item above; the current item is
// `rowSelected` with semibold text (no accent bar, no icon). Rows are real <button>s in
// a plain navigation list, and the shown item carries aria-current.

import type { CSSProperties, ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";
import { sectionLabel } from "../theme/tokens";

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
  /** Trailing count (or DASH), in secondary text. */
  meta?: ReactNode;
  /** A child of the item above (Catalog's Effects subcategories): 22 px in. */
  indent?: boolean;
}

/** Left padding of an indented (child) item. */
const RAIL_INDENT = 22;

export function RailItem({
  label,
  active,
  onClick,
  meta,
  indent = false,
}: RailItemProps) {
  const { t } = useTheme();
  return (
    <button
      type="button"
      aria-current={active ? true : undefined}
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
        padding: `0 ${String(t.sp8)}px 0 ${String(indent ? RAIL_INDENT : t.sp8)}px`,
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
