// src/views/catalog/DetailBar.tsx — the Models tab's 96px bottom ribbon: the picked
// model's tinted swatch, its type path, name, modeled unit and meta cells (type, CPU,
// routing, engine, since) — or the "Select a model" prompt.

import type { ReactNode } from "react";

import type { ThemeTokens } from "../../theme/tokens";
import { toneBodyHex } from "../../ui/blockart/shared";
import { CPU_BUDGET, cpuStr } from "../../models/cpu";
import { displayName, type ModelRecord } from "../../models/catalog";
import { artFor, hexLum } from "./catalogArt";
import { ModelTags } from "./ModelTags";
import { ModelTile } from "./ModelTile";

interface MetaCellProps {
  t: ThemeTokens;
  k: string;
  v: ReactNode;
}

function MetaCell({ t, k, v }: MetaCellProps) {
  return (
    <div style={{ textAlign: "right" }}>
      <div
        style={{
          fontFamily: t.mono,
          fontSize: t.fsTag,
          letterSpacing: t.lsTag,
          color: t.faint,
          textTransform: "uppercase",
        }}
      >
        {k}
      </div>
      <div
        style={{
          fontFamily: t.mono,
          fontSize: t.fsData,
          color: t.ink,
          marginTop: t.space2,
          whiteSpace: "nowrap",
        }}
      >
        {v}
      </div>
    </div>
  );
}

export interface DetailBarProps {
  r: ModelRecord | null;
  t: ThemeTokens;
}

export function DetailBar({ r, t }: DetailBarProps) {
  if (!r) {
    return (
      <div
        style={{
          height: 96,
          flexShrink: 0,
          borderTop: `0.5px solid ${t.hairline}`,
          background: t.bgAlt,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <span
          style={{
            fontFamily: t.sans,
            fontSize: t.fsControl,
            color: t.mutedInk,
          }}
        >
          Select a model for more information.
        </span>
      </div>
    );
  }
  const typePath =
    r.cat === "Effects" ? `${r.sub ?? ""}${r.et ? ` · ${r.et}` : ""}` : r.cat;
  const cells: [string, ReactNode][] = [
    ["type", typePath],
    [
      "cpu",
      <span key="cpu">
        {cpuStr(r.cpu)}
        {r.cpu != null && <span style={{ color: t.accentDeep }}>*</span>}
      </span>,
    ],
    ["routing", r.ch === "stereo" ? "Stereo" : "Mono"],
  ];
  if (r.cat === "Effects" && r.sub === "Reverb")
    cells.push(["engine", r.conv ? "Convolution" : "Algorithmic"]);
  cells.push(["since", "v" + r.since]);
  // Half-stacks are taller than wide, so cap the DRIVING dimension on height:
  // 46 renders a half-stack ≈ 54×64 px (fits the 74 box), square forms at 54.
  const swatch = r.form === "half_stack" ? 46 : 54;
  // The bottom-ribbon icon box is filled with the selected model's chassis color,
  // softened by a luminance-scaled white wash so dark amps don't read as a black
  // void and the icon stays legible (item 8). Half-stacks tint to the HEAD's
  // chassis tone (the same art ModelTile renders, via the shared artFor).
  const detailArt = artFor(r);
  const chassis = toneBodyHex(detailArt?.tone);
  const wash = Math.max(0, Math.min(0.55, 0.6 - hexLum(chassis) * 1.05));
  return (
    <div
      style={{
        height: 96,
        flexShrink: 0,
        borderTop: `0.5px solid ${t.hairlineStrong}`,
        background: t.bgAlt,
        display: "flex",
        alignItems: "center",
        gap: t.space8,
        padding: `0 ${String(t.space8)}px`,
      }}
    >
      <div
        style={{
          width: 74,
          height: 74,
          borderRadius: t.rCard,
          // amp chassis color, softened by a luminance-scaled white wash:
          background: `linear-gradient(rgba(255,255,255,${String(wash)}), rgba(255,255,255,${String(wash)})), ${chassis}`,
          border: `0.5px solid ${t.hairline}`,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          flexShrink: 0,
          filter: "drop-shadow(0 2px 4px rgba(15,17,21,0.22))",
        }}
      >
        <ModelTile r={r} size={swatch} />
      </div>
      <div style={{ minWidth: 0, flex: 1 }}>
        <div style={{ display: "flex", alignItems: "center", gap: t.space4 }}>
          <span
            style={{
              fontFamily: t.mono,
              fontSize: t.fsMicro,
              color: t.accentDeep,
              letterSpacing: "0.07em",
              textTransform: "uppercase",
            }}
          >
            {typePath}
          </span>
          <ModelTags r={r} t={t} />
        </div>
        <div
          style={{
            fontFamily: t.serif,
            fontSize: 18,
            color: t.ink,
            marginTop: t.space1,
            lineHeight: 1.15,
          }}
        >
          {displayName(r)}
        </div>
        <div
          style={{
            fontFamily: t.sans,
            fontSize: t.fsUi,
            color: t.mutedInk,
            marginTop: t.space1,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          Models <strong style={{ color: t.ink2 }}>{r.real}</strong>
        </div>
      </div>
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "flex-end",
          gap: t.space4,
          flexShrink: 0,
        }}
      >
        <div style={{ display: "flex", gap: t.space10 }}>
          {cells.map(([k, v]) => (
            <MetaCell key={k} t={t} k={k} v={v} />
          ))}
        </div>
        <div
          style={{
            fontFamily: t.mono,
            fontSize: t.fsTag,
            color: t.faint,
            letterSpacing: "0.02em",
            whiteSpace: "nowrap",
          }}
        >
          <span style={{ color: t.accentDeep }}>*</span> CPU capped at{" "}
          {CPU_BUDGET}% per preset
        </div>
      </div>
    </div>
  );
}
