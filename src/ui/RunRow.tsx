// src/ui/RunRow.tsx — one row of a run wizard's live progress list.
//
// Shared by the Doctor "Check" run (DoctorRun) and the Leveling run (RunBody).
// The two wizards have different status vocabularies, so `icon` and `status` are
// opaque ReactNode slots the caller fills (with its own colors).
//
// TWO layouts, one component:
//   · FLEX (default, Doctor) — icon · name+tag · instrument chip · fixed-width status.
//   · COLUMNED (`columns` set, Leveling) — the caller's shared grid template lines every
//     row up under a header row, and the extra `subLabel` / `target` cells become
//     available. The instrument is plain text here: as a column it needs no chip, and the
//     Tag border made the table noisy.

import type { CSSProperties, ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";
import { Tag } from "./Tag";

const ELLIPSIS: CSSProperties = {
  whiteSpace: "nowrap",
  overflow: "hidden",
  textOverflow: "ellipsis",
};

/** Leading glyph cell width. A columned caller's first grid track must match it. */
export const RUNROW_GLYPH_W = 18;

export interface RunRowProps {
  /** Leading status glyph (spinner / dot / check / warn / x). */
  icon: ReactNode;
  name: string;
  tag?: string;
  /** Tag text color; default accentDeep. */
  tagColor?: string;
  /** Instrument profile display name (omit ⇒ no cell). */
  instrument?: string;
  /** Right-cell content (already colored by the caller). */
  status: ReactNode;
  /** Width of the right status cell (px). FLEX mode only — in columned mode the grid
   *  template owns every column width. */
  statusWidth?: number;
  /** Currently-processing row (accentSoft background). */
  active?: boolean;
  /** Dim the name to mutedInk (queued rows). */
  dim?: boolean;
  /** `grid-template-columns` ⇒ COLUMNED mode (glyph · sound · instrument · target ·
   *  result). Omit for the flex row. */
  columns?: string;
  /** Mono sub-line under `name` (e.g. `028 · E2E Hiwatt 3S`). */
  subLabel?: string;
  /** Target cell text (e.g. `Stage · −26.0`). Columned mode only. */
  target?: string;
}

export function RunRow({
  icon,
  name,
  tag,
  tagColor,
  instrument,
  status,
  statusWidth,
  active,
  dim,
  columns,
  subLabel,
  target,
}: RunRowProps) {
  const { t } = useTheme();
  const nameLine = (
    <>
      <span
        style={{
          fontFamily: t.serif,
          fontSize: t.fsName,
          color: dim ? t.mutedInk : t.ink,
          ...ELLIPSIS,
        }}
      >
        {name}
      </span>
      {tag && (
        <span
          style={{
            fontFamily: t.mono,
            fontSize: t.fsTag,
            letterSpacing: "0.04em",
            color: tagColor ?? t.accentDeep,
            flexShrink: 0,
          }}
        >
          {tag}
        </span>
      )}
    </>
  );
  return (
    <div
      style={{
        padding: `${String(t.space4)}px ${String(t.space5)}px`,
        borderRadius: t.rCard,
        background: active ? t.accentSoft : "transparent",
        ...(columns == null
          ? { display: "flex", alignItems: "center", gap: t.space6 }
          : {
              display: "grid",
              gridTemplateColumns: columns,
              gap: t.space7,
              alignItems: "center",
            }),
      }}
    >
      <span
        style={{
          // The grid track already sizes this cell in columned mode.
          width: columns == null ? RUNROW_GLYPH_W : undefined,
          flexShrink: 0,
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        {icon}
      </span>
      {/* One name cell for both modes: a column stack whose sub-line is simply absent
          when the caller passes none (the Doctor run). */}
      <span
        style={{
          minWidth: 0,
          display: "flex",
          flexDirection: "column",
          gap: t.space1,
          ...(columns == null && { flex: 1 }),
        }}
      >
        <span
          style={{
            minWidth: 0,
            display: "flex",
            alignItems: "baseline",
            gap: t.space4,
          }}
        >
          {nameLine}
        </span>
        {subLabel != null && (
          <span
            style={{
              fontFamily: t.mono,
              fontSize: t.fsData2,
              color: t.faint,
              ...ELLIPSIS,
            }}
          >
            {subLabel}
          </span>
        )}
      </span>
      {columns == null ? (
        instrument && (
          <Tag size="md" tone="neutral">
            {instrument}
          </Tag>
        )
      ) : (
        <span
          style={{
            fontFamily: t.sans,
            fontSize: t.fsLabel,
            color: t.mutedInk,
            ...ELLIPSIS,
          }}
        >
          {instrument}
        </span>
      )}
      {columns != null && (
        <span
          style={{
            fontFamily: t.mono,
            fontSize: t.fsData,
            color: dim ? t.mutedInk : t.ink2,
            ...ELLIPSIS,
          }}
        >
          {target}
        </span>
      )}
      <span
        style={{
          fontFamily: t.mono,
          fontSize: t.fsData,
          flexShrink: 0,
          // Columned callers omit it — the grid track sizes the cell instead.
          width: statusWidth,
          whiteSpace: "nowrap",
          textAlign: "right",
        }}
      >
        {status}
      </span>
    </div>
  );
}
