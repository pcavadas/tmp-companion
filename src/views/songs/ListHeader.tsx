// src/views/songs/ListHeader.tsx — the shared mono uppercase column-header row for
// the Songs tables (Library / Setlist detail / Preset detail). Canonical look; each
// caller passes its own grid template + cell labels (right-align optional). An empty
// label ("") renders an empty spacer cell (drag-handle / trailing-affordance columns).
import { useTheme } from "../../theme/ThemeContext";

export interface ListHeaderProps {
  /** grid-template-columns — must match the caller's SongRow gridCols. */
  cols: string;
  cells: { label: string; align?: "right" }[];
}

export function ListHeader({ cols, cells }: ListHeaderProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: cols,
        alignItems: "center",
        height: 28,
        padding: `0 ${String(t.space8)}px 0 ${String(t.space8)}px`,
        borderBottom: `0.5px solid ${t.hairline}`,
        borderTop: `0.5px solid ${t.hairline}`,
        fontFamily: t.mono,
        fontSize: t.fsMicro,
        letterSpacing: t.lsLabel,
        color: t.faint,
        textTransform: "uppercase",
      }}
    >
      {cells.map((c, i) => (
        <span
          key={i}
          style={c.align === "right" ? { textAlign: "right" } : undefined}
        >
          {c.label}
        </span>
      ))}
    </div>
  );
}
