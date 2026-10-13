// src/views/catalog/ModelCard.tsx — one card of the Models wall: the art, the name, the
// modeled unit, the CPU chip and the routing/engine badges. A pressed button that picks
// the model into the detail bar.

import { memo, useState, type CSSProperties } from "react";

import { plainButton, type ThemeTokens } from "../../theme/tokens";
import { Tag } from "../../ui/Tag";
import { cpuStr } from "../../models/cpu";
import { displayName, type ModelRecord } from "../../models/catalog";
import { ICON_SHADOW } from "./catalogArt";
import { ModelTags } from "./ModelTags";
import { ModelTile } from "./ModelTile";

/** The always-on per-card CPU cost chip (MONO, ink2 on inset). Hidden when the
 *  model is not a costed DSP module (mics / FX-loop markers → `cpu` null). */
interface CpuChipProps {
  cpu: number | null;
}

function CpuChip({ cpu }: CpuChipProps) {
  if (cpu == null) return null;
  return <Tag tone="neutralFill">{cpuStr(cpu)} CPU</Tag>;
}

// Card geometry (px), relative to the icon size: the card is `icon + CARD_PAD`
// wide; the clamped name/real text spans `icon + TEXT_PAD` (icon + side padding).
const CARD_PAD = 78;
const TEXT_PAD = 66;

export interface ModelCardProps {
  r: ModelRecord;
  t: ThemeTokens;
  selected: boolean;
  icon: number;
  onPick: (r: ModelRecord) => void;
}

// Memoized: the wall holds every model (~350 procedural-SVG cards), and its props are
// stable — `t` is the module-constant theme, `onPick` is the `setPick` setter — so a
// pick re-renders only the cards whose `selected` flipped, not the whole wall.
export const ModelCard = memo(function ModelCard({
  r,
  t,
  selected,
  icon,
  onPick,
}: ModelCardProps) {
  const [hov, setHov] = useState(false);
  const clamp2: CSSProperties = {
    display: "-webkit-box",
    WebkitLineClamp: 2,
    WebkitBoxOrient: "vertical",
    overflow: "hidden",
    maxWidth: icon + TEXT_PAD,
  };
  return (
    <button
      type="button"
      aria-pressed={selected}
      onClick={() => {
        onPick(r);
      }}
      onMouseEnter={() => {
        setHov(true);
      }}
      onMouseLeave={() => {
        setHov(false);
      }}
      title={`${displayName(r)} — ${r.real}`}
      style={{
        ...plainButton(),
        width: icon + CARD_PAD,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        gap: t.space3,
        padding: `${String(t.space6)}px ${String(t.space4)}px ${String(t.space5)}px`,
        borderRadius: t.rLg,
        cursor: "pointer",
        border: `0.5px solid ${selected ? t.accent : "transparent"}`,
        background: selected ? t.accentSoft : hov ? t.bgAlt : "transparent",
      }}
    >
      <div style={{ filter: ICON_SHADOW }}>
        <ModelTile r={r} size={icon} />
      </div>
      <div
        style={{
          fontFamily: t.serif,
          fontSize: t.fsLabel,
          lineHeight: 1.16,
          color: t.ink,
          textAlign: "center",
          ...clamp2,
        }}
      >
        {displayName(r)}
      </div>
      <div
        style={{
          fontFamily: t.sans,
          fontSize: t.fsMicro,
          lineHeight: 1.25,
          color: t.mutedInk,
          textAlign: "center",
          ...clamp2,
        }}
      >
        {r.real}
      </div>
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          gap: t.space2,
        }}
      >
        <CpuChip cpu={r.cpu} />
        {(r.ch === "stereo" || r.conv) && (
          <div
            style={{
              display: "flex",
              gap: t.space2,
              flexWrap: "wrap",
              justifyContent: "center",
            }}
          >
            <ModelTags r={r} t={t} />
          </div>
        )}
      </div>
    </button>
  );
});
