// src/views/songs/SongRow.tsx — one song row for the Songs lists (All songs, a setlist,
// a preset's songs), on the look B `Row`: the position ("01", tabular), the name
// 13/500, the notes (if any) in secondary text after it on the same line, the tempo
// ("120 BPM", only when the song has an active tempo) at the right, and an optional
// trailing control (the ⋯). A setlist row adds a leading grip and is draggable
// (`rootProps` spread onto the row root). The notes give way first, then the name, so
// the tempo and the ⋯ always stay in view.

import type { HTMLAttributes, ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Row } from "../../ui/List";
import { pad2 } from "../../lib/format";
import type { SongRecord } from "../../lib/types";
import { tempoLabel } from "./songUtil";

export interface SongRowProps {
  song: SongRecord;
  /** 0-based position in the list; shown 1-based as "01". */
  idx: number;
  /** Before the position (a setlist row's grip). */
  leading?: ReactNode;
  /** After the tempo (the row's ⋯ and its menu). */
  trailing?: ReactNode;
  /** Its menu is open: the grey `rowSelected` fill. */
  selected?: boolean;
  /** The entry names no known song: the name is drawn in secondary text. */
  unknown?: boolean;
  /** Drag-and-drop (or other) props spread onto the row root. */
  rootProps?: Omit<HTMLAttributes<HTMLDivElement>, "style" | "onClick">;
}

export function SongRow({
  song,
  idx,
  leading,
  trailing,
  selected = false,
  unknown = false,
  rootProps,
}: SongRowProps) {
  const { t } = useTheme();
  const tempo = tempoLabel(song);
  const secondary = {
    fontSize: t.fontCaption,
    color: t.textSecondary,
    whiteSpace: "nowrap",
  } as const;
  return (
    <Row {...rootProps} selected={selected}>
      {leading}
      <span
        style={{
          ...secondary,
          width: 30,
          flexShrink: 0,
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {pad2(idx + 1)}
      </span>
      <span
        style={{
          flex: 1,
          minWidth: 0,
          display: "flex",
          alignItems: "baseline",
          gap: t.sp8,
          overflow: "hidden",
        }}
      >
        <span
          style={{
            // Keeps its full width while the notes give way; truncates only
            // once it alone is wider than the line.
            flexShrink: 0,
            maxWidth: "100%",
            minWidth: 0,
            fontWeight: t.weightMedium,
            color: unknown ? t.textSecondary : t.text,
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {song.name}
        </span>
        {song.notes && (
          <span
            style={{
              ...secondary,
              // Gives way first.
              flex: "0 1 auto",
              minWidth: 0,
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            {song.notes}
          </span>
        )}
      </span>
      {tempo != null && (
        <span
          style={{
            ...secondary,
            flexShrink: 0,
            fontVariantNumeric: "tabular-nums",
          }}
        >
          {tempo}
        </span>
      )}
      {trailing}
    </Row>
  );
}
