// src/ui/PresetOptionRow.tsx — one selectable option row in a setup list.
//
// Shared by the Leveling SetupBody and the Doctor DoctorSetup. Layout is a grid:
// a 26px tick cell + a 1fr name/tag/sub cell + a caller-supplied TAIL of trailing
// cells (`columns` = the grid-template-columns tail; `children` = the cells that
// fill it — the Pick dropdowns). The tail column count and the children count must
// stay in lockstep: Doctor passes 1 (one instrument Pick); SetupBody passes 3
// (a footswitch-param cell + instrument + target Picks).
//
// The whole tick cell and name cell toggle bulk-pick (onTogglePick). Tag chips
// route through the DS <Tag> (accent for FS/scene, neutral for BASE).

import type { ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";
import { Checkbox } from "./primitives";
import { Tag } from "./Tag";

export interface PresetOptionRowProps {
  name: string;
  /** Tag chip text ("BASE" | `FS${n}` | scene tag); omit for no chip. */
  tag?: string;
  /** BASE row → neutral chip; otherwise accent. */
  isBase?: boolean;
  /** Optional sub-line under the name (leveling only; Doctor omits it). */
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
        gap: 10,
        padding: "7px 0 7px 6px",
        borderTop: `0.5px solid ${t.hairline}`,
        background: isPicked ? t.rowSel : "transparent",
      }}
    >
      <div
        onClick={onTogglePick}
        title={title}
        style={{ display: "flex", alignItems: "center", cursor: "pointer" }}
      >
        <Checkbox checked={isPicked} />
      </div>
      <div
        onClick={onTogglePick}
        style={{ minWidth: 0, paddingRight: 8, cursor: "pointer" }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span
            style={{
              fontFamily: t.serif,
              fontSize: 14,
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
              fontSize: 10.5,
              color: t.faint,
              marginTop: 2,
            }}
          >
            {sub}
          </div>
        )}
      </div>
      {children}
    </div>
  );
}
