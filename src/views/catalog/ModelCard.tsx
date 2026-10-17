// src/views/catalog/ModelCard.tsx — one tile of the Catalog wall (look B): the model's
// art (44 × 44), its name and the real unit it models (up to two lines), the CPU cost
// only while the wall is sorted by CPU, and "Stereo" / "Convolution" badges. A pressed
// button that chooses the model for the detail.

import { memo, type CSSProperties } from "react";

import type { ThemeTokens } from "../../theme/tokens";
import { Badge } from "../../ui/Badge";
import { cpuStr } from "../../models/cpu";
import { displayName, type ModelRecord } from "../../models/catalog";
import { realLine } from "./catalogLabels";
import { ModelTile } from "./ModelTile";

/** The tile's art box. */
const TILE_ART = 44;
/** The tile's padding and the gap between its art and its words (off the ramp, as
 *  drawn). */
const TILE_PAD = 10;

export interface ModelCardProps {
  r: ModelRecord;
  t: ThemeTokens;
  selected: boolean;
  /** The wall is sorted by CPU: the tile shows the model's CPU cost. */
  showCpu: boolean;
  onPick: (r: ModelRecord) => void;
}

// Memoized: the wall holds every model (~350 procedural-SVG tiles), and its props are
// stable — `t` is the module-constant theme, `onPick` is the `setPick` setter — so a
// pick re-renders only the tiles whose `selected` flipped, not the whole wall.
export const ModelCard = memo(function ModelCard({
  r,
  t,
  selected,
  showCpu,
  onPick,
}: ModelCardProps) {
  const clamp2: CSSProperties = {
    display: "-webkit-box",
    WebkitLineClamp: 2,
    WebkitBoxOrient: "vertical",
    overflow: "hidden",
  };
  // The selected edge is 2 px; the padding gives back the extra pixel so the
  // contents don't shift when a tile is chosen.
  const edge = selected ? 2 : 1;
  const pad = TILE_PAD - (edge - 1);
  return (
    <button
      type="button"
      aria-pressed={selected}
      data-model={r.cid}
      onClick={() => {
        onPick(r);
      }}
      style={{
        appearance: "none",
        margin: 0,
        minWidth: 0,
        boxSizing: "border-box",
        display: "flex",
        alignItems: "flex-start",
        gap: TILE_PAD,
        padding: pad,
        background: t.surface,
        border: `${String(edge)}px solid ${selected ? t.action : t.border}`,
        borderRadius: t.radiusGroup,
        textAlign: "left",
        fontFamily: t.fontSans,
        color: t.text,
        cursor: "pointer",
      }}
    >
      <ModelTile r={r} size={TILE_ART} />
      <span
        style={{
          minWidth: 0,
          display: "flex",
          flexDirection: "column",
          alignItems: "flex-start",
          gap: t.sp2,
        }}
      >
        <span
          style={{
            fontSize: t.fontBody,
            lineHeight: `${String(t.lineBody)}px`,
            fontWeight: t.weightMedium,
            ...clamp2,
          }}
        >
          {displayName(r)}
        </span>
        <span
          style={{
            fontSize: t.fontCaption,
            lineHeight: `${String(t.lineCaption)}px`,
            color: t.textSecondary,
            ...clamp2,
          }}
        >
          {realLine(r)}
        </span>
        {showCpu && r.cpu != null && (
          <span
            style={{
              fontSize: t.fontCaption,
              lineHeight: `${String(t.lineCaption)}px`,
              color: t.textSecondary,
              fontVariantNumeric: "tabular-nums",
            }}
          >
            {cpuStr(r.cpu)} CPU
          </span>
        )}
        {(r.ch === "stereo" || r.conv) && (
          <span
            style={{
              display: "flex",
              flexWrap: "wrap",
              gap: t.sp4,
              marginTop: t.sp2,
            }}
          >
            {r.ch === "stereo" && <Badge>Stereo</Badge>}
            {r.conv && <Badge>Convolution</Badge>}
          </span>
        )}
      </span>
    </button>
  );
});
