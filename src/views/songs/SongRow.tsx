// src/views/songs/SongRow.tsx — one song row for the Songs tables (Library /
// Setlist detail / Preset detail). Shared anatomy: № · name+notes · bpm. Divergent
// affordances flow through the leading (drag handle) / trailing (⋯ menu · remove ✗)
// slots; drag-and-drop props spread onto the row root via rootProps.
//
// bpm alignment and the "<n> bpm" suffix are independent props (Setlist / Preset
// pass both; the Library passes neither and renders the bare number). All render
// DASH when the song has no active BPM.
import type { HTMLAttributes, ReactNode } from "react";
import { useTheme } from "../../theme/ThemeContext";
import { DASH, pad2 } from "../../lib/format";
import type { SongRecord } from "../../lib/types";
import { songBpm, bpmStr } from "./songUtil";

export interface SongRowProps {
  song: SongRecord;
  idx: number;
  /** grid-template-columns — must match the caller's ListHeader cols. */
  gridCols: string;
  /** left cell before № (e.g. the drag handle); omitted → no cell. */
  leading?: ReactNode;
  /** right cell after bpm (e.g. the ⋯ menu / remove ✗); omitted → no cell. */
  trailing?: ReactNode;
  /** right-align the bpm cell. */
  bpmAlign?: "right";
  /** render "<n> bpm" instead of the bare number. */
  bpmSuffix?: boolean;
  /** drag-and-drop (or other) props spread onto the row root. */
  rootProps?: HTMLAttributes<HTMLDivElement>;
}

export function SongRow({
  song,
  idx,
  gridCols,
  leading,
  trailing,
  bpmAlign,
  bpmSuffix,
  rootProps,
}: SongRowProps) {
  const { t } = useTheme();
  const bpm = songBpm(song);
  return (
    <div
      {...rootProps}
      style={{
        display: "grid",
        gridTemplateColumns: gridCols,
        alignItems: "center",
        height: 48,
        padding: `0 ${String(t.space8)}px 0 ${String(t.space8)}px`,
        borderBottom: `0.5px solid ${t.hairline}`,
      }}
    >
      {leading}
      <span
        style={{ fontFamily: t.mono, fontSize: t.fsData, color: t.mutedInk }}
      >
        {pad2(idx + 1)}
      </span>
      <div style={{ minWidth: 0, paddingRight: t.space6 }}>
        <div
          style={{
            fontFamily: t.serif,
            fontSize: t.fsName,
            color: t.ink,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {song.name}
        </div>
        {song.notes && (
          <div
            style={{
              fontFamily: t.sans,
              fontSize: t.fsLabel,
              color: t.mutedInk,
              marginTop: t.space1,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {song.notes}
          </div>
        )}
      </div>
      <span
        style={{
          fontFamily: t.mono,
          fontSize: t.fsLabel,
          color: bpm != null ? t.ink2 : t.faint,
          ...(bpmAlign === "right" ? { textAlign: "right" } : {}),
        }}
      >
        {bpmSuffix ? (bpm != null ? `${String(bpm)} bpm` : DASH) : bpmStr(bpm)}
      </span>
      {trailing}
    </div>
  );
}
