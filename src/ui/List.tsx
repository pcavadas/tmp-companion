// src/ui/List.tsx — the one list style across tabs (look B): a `Group` of `Row`s, and
// the `Caret` that opens a row's sub-rows.
//
// Group: a white card on the grey window — 1 px `border`, `radiusGroup`,
// `overflow: hidden`. Rows inside are separated by a 1 px `separator`. Each Row draws its
// own top rule and the Group tucks the FIRST one under its edge (the inner box sits 1 px
// up inside the clipped card), so any mix of rows — mapped, nested in fragments, or a
// header followed by its sub-rows — gets a line between every pair and none at the top.
//
// Row: one line of cells, left to right, 10 px apart, padding 0 12 px (`padX` widens it). Heights from the
// tokens: `regular` 36 (lists), `controls` 34 (rows holding dropdowns), `compact` 30
// (sub-rows, the run list). `header` is a preset's group header (`groupHeader` fill,
// 34 high). `selected` (chosen or open) is the grey `rowSelected`, never terracotta.
// Sub-rows indent 52–64 px (`indent`). A row that needs a note shows it UNDER the line:
// 12/16 px with a 13 px caution (or danger) icon. Row logic stays with the views; this
// is only the chrome.
//
// Caret: an 18 px button with a right/down chevron, `aria-expanded`, named
// "Open <name>" / "Close <name>". Only on presets that have scenes or footswitches.

import type {
  ComponentPropsWithoutRef,
  CSSProperties,
  MouseEvent,
  ReactNode,
} from "react";

import { useTheme } from "../theme/ThemeContext";
import { plainButton } from "../theme/tokens";
import { Icon } from "./Icon";

// ---- Group -----------------------------------------------------------------

export interface GroupProps {
  children: ReactNode;
  /** Accessible name for the list, when it has one. */
  label?: string;
  /** Layout extras for the card (margins, flex). */
  style?: CSSProperties;
}

export function Group({ children, label, style }: GroupProps) {
  const { t } = useTheme();
  return (
    <div
      aria-label={label}
      role={label ? "group" : undefined}
      style={{
        background: t.surface,
        border: `1px solid ${t.border}`,
        borderRadius: t.radiusGroup,
        overflow: "hidden",
        flexShrink: 0,
        fontFamily: t.fontSans,
        ...style,
      }}
    >
      {/* 1 px up, so the first row's top rule sits under the card's own edge. */}
      <div style={{ marginTop: -1 }}>{children}</div>
    </div>
  );
}

// ---- Row -------------------------------------------------------------------

export type RowSize = "regular" | "controls" | "compact";

export interface RowProps extends Omit<
  ComponentPropsWithoutRef<"div">,
  "style" | "onClick"
> {
  children: ReactNode;
  /** `regular` 36 (default), `controls` 34 (rows with dropdowns), `compact` 30. A
   *  `header` row defaults to `controls`. */
  size?: RowSize;
  /** A preset's group header: `groupHeader` fill. */
  header?: boolean;
  /** Chosen or open: the grey `rowSelected` fill. */
  selected?: boolean;
  /** Left padding for a sub-row (52–64 px). Default `padX`. */
  indent?: number;
  /** Side padding of the line and its note (Set up's rows use 14). Default 12. */
  padX?: number;
  /** A note under the row (one line or two, 12/16 px, with an icon). */
  note?: ReactNode;
  /** The note's icon colour. Default "caution". */
  noteTone?: "caution" | "danger";
  /** Layout extras for the row's LINE (e.g. a CSS grid for column-aligned cells). */
  style?: CSSProperties;
}

export function Row({
  children,
  size,
  header = false,
  selected = false,
  indent,
  padX,
  note,
  noteTone = "caution",
  style,
  ...rest
}: RowProps) {
  const { t } = useTheme();
  const height = {
    regular: t.row,
    controls: t.rowControls,
    compact: t.rowCompact,
  }[size ?? (header ? "controls" : "regular")];
  const padRight = padX ?? t.sp12;
  const padLeft = indent ?? padRight;
  return (
    <div
      {...rest}
      style={{
        borderTop: `1px solid ${t.separator}`,
        background: header
          ? t.groupHeader
          : selected
            ? t.rowSelected
            : "transparent",
        fontSize: t.fontBody,
        color: t.text,
      }}
    >
      <div
        style={{
          // The rule above is inside the row's box: the line keeps its full height.
          height: height - 1,
          boxSizing: "border-box",
          display: "flex",
          alignItems: "center",
          gap: t.sp8 + 2,
          padding: `0 ${String(padRight)}px 0 ${String(padLeft)}px`,
          minWidth: 0,
          ...style,
        }}
      >
        {children}
      </div>
      {note != null && (
        <div
          style={{
            display: "flex",
            alignItems: "flex-start",
            gap: t.sp6,
            padding: `0 ${String(padRight)}px ${String(t.sp8 + 2)}px ${String(padLeft)}px`,
            fontSize: t.fontCaption,
            lineHeight: `${String(t.lineCaption)}px`,
            color: t.text,
          }}
        >
          <span style={{ display: "flex", paddingTop: 1.5, flexShrink: 0 }}>
            <Icon
              name={noteTone === "danger" ? "alert" : "warn-tri"}
              size={13}
              stroke={noteTone === "danger" ? t.danger : t.caution}
              strokeWidth={2}
            />
          </span>
          <span style={{ minWidth: 0 }}>{note}</span>
        </div>
      )}
    </div>
  );
}

// ---- Caret -----------------------------------------------------------------

export interface CaretProps {
  open: boolean;
  /** What it opens ("Plexi Crunch"): named "Open <name>" / "Close <name>". */
  name: string;
  /** Receives the click event, so a caret inside a clickable surface can
   *  stopPropagation. */
  onToggle: (e: MouseEvent<HTMLButtonElement>) => void;
}

export function Caret({ open, name, onToggle }: CaretProps) {
  const { t } = useTheme();
  return (
    <button
      type="button"
      aria-expanded={open}
      aria-label={`${open ? "Close" : "Open"} ${name}`}
      onClick={onToggle}
      style={plainButton({
        width: 18,
        height: 18,
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        borderRadius: t.radiusSmall,
      })}
    >
      <Icon
        name={open ? "chev-down" : "chev-right"}
        size={12}
        stroke={t.textSecondary}
        strokeWidth={2.2}
      />
    </button>
  );
}
