// src/ui/PresetOptionRow.tsx — one selectable option row in a setup list.
//
// Used by the Doctor DoctorSetup (the Level wizard's SetupPage renders its own rows).
// Layout is a grid: a 26px tick cell + a 1fr name/tag/sub cell + a caller-supplied
// TAIL of trailing cells (`columns` = the grid-template-columns tail; `children` = the
// cells that fill it — the Pick dropdowns). The tail column count and the children
// count must stay in lockstep: Doctor passes 1 (one instrument Pick).
//
// The whole tick cell and name cell toggle bulk-pick (onTogglePick). Tag chips
// route through the DS <Tag> (accent for FS/scene, neutral for BASE).

import type { ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";
import { Button, Checkbox } from "./primitives";
import { Tag } from "./Tag";

export interface PresetOptionRowProps {
  name: string;
  /** Tag chip text ("BASE" | `FS${n}` | scene tag); omit for no chip. */
  tag?: string;
  /** BASE row → neutral chip; otherwise accent. */
  isBase?: boolean;
  /** Optional sub-line under the name (Doctor omits it). */
  sub?: string;
  isPicked: boolean;
  onTogglePick: () => void;
  /** Tooltip on the tick cell (differs slightly per caller). */
  title?: string;
  /** The grid-template-columns TAIL for the trailing cells. */
  columns: string;
  /** The trailing cells — one grid item per `columns` track. */
  children: ReactNode;
}

export function PresetOptionRow({
  name,
  tag,
  isBase,
  sub,
  isPicked,
  onTogglePick,
  title,
  columns,
  children,
}: PresetOptionRowProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: `26px 1fr ${columns}`,
        alignItems: "center",
        gap: t.space5,
        padding: `${String(t.space4)}px 0 ${String(t.space4)}px ${String(t.space3)}px`,
        borderTop: `0.5px solid ${t.hairline}`,
        background: isPicked ? t.rowSel : "transparent",
      }}
    >
      <Button
        variant="bare"
        role="checkbox"
        aria-checked={isPicked}
        aria-label={tag ? `${name} ${tag}` : name}
        onClick={onTogglePick}
        title={title}
        style={{ display: "flex", alignItems: "center" }}
      >
        <Checkbox checked={isPicked} />
      </Button>
      <Button
        variant="bare"
        tabIndex={-1}
        aria-hidden
        onClick={onTogglePick}
        style={{
          display: "block",
          width: "100%",
          textAlign: "left",
          minWidth: 0,
          paddingRight: t.space4,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: t.space4 }}>
          <span
            style={{
              fontFamily: t.serif,
              fontSize: t.fsName2,
              color: t.ink,
              whiteSpace: "nowrap",
            }}
          >
            {name}
          </span>
          {tag && <Tag tone={isBase ? "neutral" : "accent"}>{tag}</Tag>}
        </div>
        {sub && (
          <div
            style={{
              fontFamily: t.sans,
              fontSize: t.fsMeta,
              color: t.faint,
              marginTop: t.space1,
            }}
          >
            {sub}
          </div>
        )}
      </Button>
      {children}
    </div>
  );
}
