// src/ui/RunRow.tsx — one row of a run wizard's live progress list.
//
// Shared by the Doctor "Check" run (DoctorRun) and the Leveling run (RunBody).
// The two wizards have different status vocabularies, so `icon` and `status` are
// opaque ReactNode slots the caller fills (with its own colors); `status` sits in
// a fixed-width right-aligned mono cell (`statusWidth`). `children` is the
// expanded drawer under the row (RunBody's live VU bars). The instrument chip is
// the DS `Tag`.

import type { ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";
import { Tag } from "./Tag";

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
  statusWidth: number;
  /** Currently-processing row (accentSoft background). */
  active?: boolean;
  /** Dim the name to mutedInk (queued rows). */
  dim?: boolean;
  /** Expanded drawer under the row (e.g. live VU bars). */
  children?: ReactNode;
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
  children,
}: RunRowProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        padding: "9px 10px",
        borderRadius: 8,
        background: active ? t.accentSoft : "transparent",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <span
          style={{
            width: 18,
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
            gap: 8,
          }}
        >
          <span
            style={{
              fontFamily: t.serif,
              fontSize: 14.5,
              color: dim ? t.mutedInk : t.ink,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            {name}
          </span>
          {tag && (
            <span
              style={{
                fontFamily: t.mono,
                fontSize: 8.5,
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
            fontSize: 11,
            flexShrink: 0,
            width: statusWidth,
            whiteSpace: "nowrap",
            textAlign: "right",
          }}
        >
          {status}
        </span>
      </div>
      {children}
    </div>
  );
}
