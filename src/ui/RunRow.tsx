// src/ui/RunRow.tsx — one row of the Doctor "Check" run's live progress list
// (DoctorRun): icon · name+tag · instrument chip · fixed-width status. `icon` and
// `status` are opaque ReactNode slots the caller fills (with its own colors).

import type { CSSProperties, ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";
import { Tag } from "./Tag";

const ELLIPSIS: CSSProperties = {
  whiteSpace: "nowrap",
  overflow: "hidden",
  textOverflow: "ellipsis",
};

/** Leading glyph cell width. */
const GLYPH_W = 18;

export interface RunRowProps {
  /** Leading status glyph (spinner / dot / check / warn / x). */
  icon: ReactNode;
  name: string;
  tag?: string;
  /** Tag text color; default accentDeep. */
  tagColor?: string;
  /** Instrument profile display name (omit ⇒ no chip). */
  instrument?: string;
  /** Right-cell content (already colored by the caller). */
  status: ReactNode;
  /** Width of the right status cell (px). */
  statusWidth?: number;
  /** Currently-processing row (accentSoft background). */
  active?: boolean;
  /** Dim the name to mutedInk (queued rows). */
  dim?: boolean;
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
}: RunRowProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        padding: `${String(t.space4)}px ${String(t.space5)}px`,
        borderRadius: t.rCard,
        background: active ? t.accentSoft : "transparent",
        display: "flex",
        alignItems: "center",
        gap: t.space6,
      }}
    >
      <span
        style={{
          width: GLYPH_W,
          flexShrink: 0,
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        {icon}
      </span>
      <span
        style={{
          flex: 1,
          minWidth: 0,
          display: "flex",
          alignItems: "baseline",
          gap: t.space4,
        }}
      >
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
      </span>
      {instrument && (
        <Tag size="md" tone="neutral">
          {instrument}
        </Tag>
      )}
      <span
        style={{
          fontFamily: t.mono,
          fontSize: t.fsData,
          flexShrink: 0,
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
