// src/views/CatalogView.tsx — the MODELS tab.
//
// A read-only, filterable catalog of every model the Tone Master Pro ships.
// Layout (below the App shell's tab bar):
//   toolbar (search + Mono/Stereo/Convolution facets + Type/CPU sort + count)
//   → effect-type chip row (only inside an Effects subcategory)
//   → category rail (194px) + grouped sticky model wall
//   → detail bar (96px).
// Filters compose: e.g. Effects → Stompboxes → Fuzz, or Effects + Stereo. Each
// card + the inspector show the model's REAL per-block DSP cost (% of the 76.5%
// per-preset budget, from models/cpu.ts); sorting by CPU flattens the wall into
// one ranked "By CPU usage" section.
// Renders per-model artwork through the shared BlockArt SVG engine (identical
// to the signal-chain strip). Logic + grouping ported from the design prototype
// (catalog-models.jsx); styling reads the app's theme tokens.

import { useMemo, useState, type CSSProperties, type ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";
import type { ThemeTokens } from "../theme/tokens";
import { Icon } from "../ui/Icon";
import { SearchInput } from "../ui/primitives";
import { Tag } from "../ui/Tag";
import { BlockArt, HalfStackArt } from "../ui/BlockArt";
import { toneBodyHex } from "../ui/blockart/shared";
import {
  resolveBlockArt,
  resolveBlockArtByName,
  HALF_STACK_PAIR,
  HALF_STACK_CAB_TONE,
} from "../models/blockArt";
import { CPU_BUDGET, cpuStr } from "../models/cpu";
import {
  MODELS,
  TOTAL,
  CAT_ORDER,
  SUB_ORDER,
  CAT_COUNT,
  SUB_COUNT,
  CAB_SIZE_ORDER,
  cabSize,
  etSort,
  etypesFor,
  displayName,
  type ModelRecord,
} from "../models/catalog";

// Off-state chip fill: white (light-only), matching the prototype's chipBg.
const chipBgFor = (t: ThemeTokens) => t.bg;

const ICON_SHADOW = "drop-shadow(0 2px 4px rgba(15,17,21,0.2))";

// Relative luminance (Rec. 709) of a #hex color, 0 (black) … 1 (white). Drives
// the bottom-ribbon icon box's white-overlay alpha so dark chassis are lightened
// to a soft grey while light/vivid chassis keep their true color.
function hexLum(hex: string): number {
  const h = (hex || "").replace("#", "");
  const n =
    h.length === 3
      ? h
          .split("")
          .map((c) => c + c)
          .join("")
      : h;
  if (n.length < 6) return 0.5;
  const r = parseInt(n.slice(0, 2), 16) / 255,
    g = parseInt(n.slice(2, 4), 16) / 255,
    b = parseInt(n.slice(4, 6), 16) / 255;
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

type Density = "comfortable" | "compact";
type Routing = "all" | "mono" | "stereo";
type Sort = "type" | "cpu";
type CpuDir = "desc" | "asc";
interface Sel {
  cat: string;
  sub: string | null;
}

// ── tile rendering ──────────────────────────────────────────────────────────

// Per-model illustration via the shared BlockArt engine (identical to the
// signal-chain strip by construction). Icon + chassis tone resolve BY ID from
// the block-art catalog; half-stacks stack the head on its paired cab. (The old
// `public/tmp_blocks/` PNG photos were Fender's copyright and are no longer used.)
interface ModelTileProps {
  r: ModelRecord;
  size: number;
}

// Resolve a record's block art the way every icon does: most rows resolve by
// FenderId; the 7 Microphones carry no block_id (they're cab parameters, not DSP
// blocks), so they resolve by NAME — without which they'd fall through to the
// generic "knobs2" pedal icon. Shared by ModelTile (the icon) and DetailBar
// (the ribbon chassis tint) so the two can't drift.
function artFor(r: ModelRecord) {
  return r.bid ? resolveBlockArt(r.bid) : resolveBlockArtByName(r.name);
}

function ModelTile({ r, size }: ModelTileProps) {
  const art = artFor(r);
  // The art's terse caption doubles as the engine's label-keyed dispatch token:
  // some 1.8 treatments select by `lab` (EVH 5150 III channel accent green/blue/
  // red, the '65 Twin 15 single 15" speaker, the gear-pedal on-device name print,
  // the Seventy Sixer compressor face), so the caption — which carries that token
  // (GREEN/BLUE/RED · 15 · PINIONS · SEVENTY SIXER) — must flow into `lab`.
  const lab = art?.short ?? "";
  if (r.form === "half_stack") {
    const cabBid = r.bid ? HALF_STACK_PAIR[r.bid] : undefined;
    const cabArt = cabBid ? resolveBlockArt(cabBid) : null;
    return (
      <HalfStackArt
        topIcon={art?.icon}
        topTone={art?.tone}
        topLab={lab}
        cabIcon={cabArt?.icon ?? "cab4"}
        // an unpaired head's cab inherits the HEAD's tone so its grille matches the
        // head/combo (Fender bass/blackface cabs must share the head grille),
        // not the old "marshall" fallback that gave them a salt-pepper grille.
        cabTone={
          (r.bid ? HALF_STACK_CAB_TONE[r.bid] : undefined) ??
          cabArt?.tone ??
          art?.tone ??
          "marshall"
        }
        cabLab=""
        cabW={size}
      />
    );
  }
  return (
    <div style={{ filter: ICON_SHADOW, display: "flex" }}>
      <BlockArt
        icon={art?.icon}
        tone={art?.tone}
        lab={lab}
        footswitch={art?.footswitch}
        bodyColor={art?.body}
        accentColor={art?.accent}
        panelColor={art?.panel}
        size={size}
        label={false}
        // The combo and head rows of one amp share a icon; the form picks the chassis.
        form={r.form === "combo" || r.form === "head" ? r.form : undefined}
      />
    </div>
  );
}

// ── badges ────────────────────────────────────────────────────────────────

/** The STEREO / CONV badges for a record — shown on cards and in the detail bar. */
interface ModelTagsProps {
  r: ModelRecord;
  t: ThemeTokens;
}

function ModelTags({ r, t }: ModelTagsProps) {
  return (
    <>
      {r.ch === "stereo" && <Tag fg={t.badgeStereo}>STEREO</Tag>}
      {r.conv && <Tag fg={t.badgeConv}>CONV</Tag>}
    </>
  );
}

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

// ── model card (the wall) ────────────────────────────────────────────────────

interface ModelCardProps {
  r: ModelRecord;
  t: ThemeTokens;
  selected: boolean;
  icon: number;
  onPick: (r: ModelRecord) => void;
}

function ModelCard({ r, t, selected, icon, onPick }: ModelCardProps) {
  const [hov, setHov] = useState(false);
  const clamp2: CSSProperties = {
    display: "-webkit-box",
    WebkitLineClamp: 2,
    WebkitBoxOrient: "vertical",
    overflow: "hidden",
    maxWidth: icon + TEXT_PAD,
  };
  return (
    <div
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
        width: icon + CARD_PAD,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        gap: 5,
        padding: "12px 7px 11px",
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
          gap: 4,
        }}
      >
        <CpuChip cpu={r.cpu} />
        {(r.ch === "stereo" || r.conv) && (
          <div
            style={{
              display: "flex",
              gap: 4,
              flexWrap: "wrap",
              justifyContent: "center",
            }}
          >
            <ModelTags r={r} t={t} />
          </div>
        )}
      </div>
    </div>
  );
}

// ── detail bar ────────────────────────────────────────────────────────────

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
          marginTop: 3,
          whiteSpace: "nowrap",
        }}
      >
        {v}
      </div>
    </div>
  );
}

interface DetailBarProps {
  r: ModelRecord | null;
  t: ThemeTokens;
}

function DetailBar({ r, t }: DetailBarProps) {
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
        gap: 16,
        padding: "0 18px",
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
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
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
            marginTop: 2,
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
            marginTop: 2,
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
          gap: 7,
          flexShrink: 0,
        }}
      >
        <div style={{ display: "flex", gap: 22 }}>
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

// ── the page ────────────────────────────────────────────────────────────────

interface Group {
  key: string;
  label: string;
  blocks: ModelRecord[];
}

export interface CatalogViewProps {
  density?: Density;
}

export function CatalogView({ density = "comfortable" }: CatalogViewProps) {
  const { t } = useTheme();
  const [q, setQ] = useState("");
  const [sel, setSel] = useState<Sel>({ cat: "all", sub: null });
  const [et, setEt] = useState<string | null>(null);
  const [routing, setRouting] = useState<Routing>("all");
  const [convOnly, setConvOnly] = useState(false);
  const [sort, setSort] = useState<Sort>("type");
  const [cpuDir, setCpuDir] = useState<CpuDir>("desc");
  const [pick, setPick] = useState<ModelRecord | null>(null);
  const [fxOpen, setFxOpen] = useState(true);
  const icon = density === "compact" ? 54 : 66;

  // Selecting a rail category resets the chip, clears the pick, and drops the
  // convolution facet unless still in a convolution-relevant scope.
  function choose(cat: string, sub?: string | null) {
    setSel({ cat, sub: sub ?? null });
    setEt(null);
    if (!(cat === "Effects" && (sub === "Reverb" || !sub))) setConvOnly(false);
    setPick(null);
  }

  // One pass over the category/subcategory scope, yielding both the scoped list
  // and whether it contains any stereo model (drives the Stereo facet's enabled
  // state). Both only change when `sel` changes — search/facets don't touch them.
  const { inScope, hasStereo } = useMemo(() => {
    const scope = MODELS.filter((r) => {
      if (sel.cat !== "all" && r.cat !== sel.cat) return false;
      if (sel.cat === "Effects" && sel.sub && r.sub !== sel.sub) return false;
      return true;
    });
    return { inScope: scope, hasStereo: scope.some((r) => r.ch === "stereo") };
  }, [sel]);

  const shown = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return inScope.filter((r) => {
      if (et && r.et !== et) return false;
      if (routing === "mono" && r.ch !== "mono") return false;
      if (routing === "stereo" && r.ch !== "stereo") return false;
      if (convOnly && !r.conv) return false;
      if (needle && !r.search.includes(needle)) return false;
      return true;
    });
  }, [inScope, et, routing, convOnly, q]);

  const groups: Group[] = useMemo(() => {
    // CPU sort overrides grouping — one flat, ranked "By CPU usage" wall.
    // Uncosted models (mics / FX-loop markers, cpu null) sort to the bottom
    // regardless of direction, alphabetical among themselves.
    if (sort === "cpu") {
      const sorted = [...shown].sort((a, b) => {
        if (a.cpu == null && b.cpu == null) return a.name.localeCompare(b.name);
        if (a.cpu == null) return 1;
        if (b.cpu == null) return -1;
        return cpuDir === "desc" ? b.cpu - a.cpu : a.cpu - b.cpu;
      });
      return [{ key: "_cpu", label: "By CPU usage", blocks: sorted }];
    }
    if (sel.cat === "Effects" && sel.sub) {
      const types = [
        ...new Set(shown.map((r) => r.et).filter((x): x is string => !!x)),
      ].sort(etSort);
      const g: Group[] = types.map((ty) => ({
        key: ty,
        label: ty,
        blocks: shown.filter((r) => r.et === ty),
      }));
      const none = shown.filter((r) => !r.et);
      if (none.length) g.push({ key: "_", label: sel.sub, blocks: none });
      return g;
    }
    if (sel.cat === "Effects" && !sel.sub) {
      return SUB_ORDER.map((s) => ({
        key: s,
        label: s,
        blocks: shown.filter((r) => r.sub === s),
      })).filter((x) => x.blocks.length);
    }
    if (sel.cat === "Cabinets") {
      const map = new Map<string, ModelRecord[]>();
      shown.forEach((r) => {
        const s = cabSize(r.name);
        let bucket = map.get(s);
        if (!bucket) {
          bucket = [];
          map.set(s, bucket);
        }
        bucket.push(r);
      });
      const rank = (k: string) => {
        const i = CAB_SIZE_ORDER.indexOf(k);
        return i === -1 ? 999 : i;
      };
      const keys = [...map.keys()].sort(
        (a, b) => rank(a) - rank(b) || a.localeCompare(b),
      );
      return keys.map((k) => ({
        key: k,
        label: k.replace("X", "×"),
        blocks: map.get(k) ?? [],
      }));
    }
    if (sel.cat === "all") {
      return CAT_ORDER.map((c) => ({
        key: c,
        label: c,
        blocks: shown.filter((r) => r.cat === c),
      })).filter((x) => x.blocks.length);
    }
    return [
      {
        key: sel.cat,
        label: sel.cat,
        blocks: [...shown].sort((a, b) => a.name.localeCompare(b.name)),
      },
    ];
  }, [shown, sel, sort, cpuDir]);

  const etChips = sel.cat === "Effects" && sel.sub ? etypesFor(sel.sub) : null;
  const showConv = sel.cat === "Effects" && (sel.sub === "Reverb" || !sel.sub);

  // ── small renderers ──
  const railRow = (
    label: string,
    count: number,
    active: boolean,
    onClick: () => void,
    opts: { child?: boolean; caret?: boolean; k?: string } = {},
  ) => (
    <div
      key={opts.k ?? label}
      onClick={onClick}
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: 8,
        padding: opts.child ? "5px 9px 5px 36px" : "7px 9px",
        borderRadius: t.rMd,
        cursor: "pointer",
        background: active ? t.accentSoft : "transparent",
        borderLeft: active ? `2px solid ${t.accent}` : "2px solid transparent",
      }}
    >
      <span
        style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}
      >
        {/* Fixed 12px chevron gutter on EVERY top-level row (chevron inside it
            when present) so all top-level labels align in one column whether or
            not they carry a disclosure caret; child rows skip it and indent via
            padding-left instead. */}
        {!opts.child && (
          <span
            style={{
              width: 12,
              flexShrink: 0,
              display: "inline-flex",
              justifyContent: "center",
            }}
          >
            {opts.caret !== undefined && (
              <Icon
                name={opts.caret ? "chev-down" : "chev-right"}
                size={12}
                stroke={active ? t.accentDeep : t.faint}
              />
            )}
          </span>
        )}
        <span
          style={{
            fontFamily: opts.child ? t.sans : t.serif,
            fontSize: opts.child ? t.fsUi : t.fsBody2,
            color: active ? t.ink : t.ink2,
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {label}
        </span>
      </span>
      <span
        style={{
          fontFamily: t.mono,
          fontSize: t.fsMicro,
          color: active ? t.accentDeep : t.faint,
          flexShrink: 0,
        }}
      >
        {count}
      </span>
    </div>
  );

  const facetBtn = (
    label: string,
    on: boolean,
    onClick: () => void,
    disabled = false,
  ) => (
    <span
      key={label}
      onClick={disabled ? undefined : onClick}
      style={{
        fontFamily: t.mono,
        fontSize: t.fsData2,
        letterSpacing: "0.03em",
        color: disabled ? t.faint : on ? "#fff" : t.ink2,
        background: on ? t.accent : chipBgFor(t),
        border: `0.5px solid ${on ? t.accent : t.hairlineStrong}`,
        borderRadius: t.rPill,
        padding: "3px 10px",
        cursor: disabled ? "default" : "pointer",
        opacity: disabled ? 0.5 : 1,
        userSelect: "none",
        whiteSpace: "nowrap",
      }}
    >
      {label}
    </span>
  );

  return (
    <div
      style={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
        background: t.bg,
      }}
    >
      {/* toolbar: search + facets + count */}
      <div
        style={{
          flexShrink: 0,
          display: "flex",
          alignItems: "center",
          gap: 12,
          padding: "11px 16px 10px",
          borderBottom: `0.5px solid ${t.hairline}`,
          background: t.bg,
        }}
      >
        <SearchInput
          value={q}
          onChange={setQ}
          placeholder="Search a model, the real unit it models, or a brand (Marshall, Boss…)"
          clearable
          style={{ flex: 1, minWidth: 0 }}
        />
        <div style={{ display: "flex", gap: 5, alignItems: "center" }}>
          {facetBtn("Mono", routing === "mono", () => {
            setRouting(routing === "mono" ? "all" : "mono");
          })}
          {facetBtn(
            "Stereo",
            routing === "stereo",
            () => {
              setRouting(routing === "stereo" ? "all" : "stereo");
            },
            !hasStereo,
          )}
          {showConv &&
            facetBtn("Convolution", convOnly, () => {
              setConvOnly(!convOnly);
            })}
        </div>
        <div
          style={{
            width: 1,
            height: 18,
            background: t.hairline,
            flexShrink: 0,
          }}
        />
        <div style={{ display: "flex", gap: 5, alignItems: "center" }}>
          <span
            style={{
              fontFamily: t.mono,
              fontSize: t.fsTag,
              letterSpacing: t.lsWide,
              color: t.faint,
              textTransform: "uppercase",
            }}
          >
            Sort
          </span>
          {facetBtn("Type", sort === "type", () => {
            setSort("type");
          })}
          {facetBtn(
            sort === "cpu" ? (cpuDir === "desc" ? "CPU ↓" : "CPU ↑") : "CPU",
            sort === "cpu",
            () => {
              if (sort !== "cpu") setSort("cpu");
              else setCpuDir(cpuDir === "desc" ? "asc" : "desc");
            },
          )}
        </div>
        <span
          style={{
            fontFamily: t.mono,
            fontSize: t.fsMeta,
            color: t.mutedInk,
            whiteSpace: "nowrap",
          }}
        >
          <strong style={{ color: t.ink }}>{shown.length}</strong> / {TOTAL}
        </span>
      </div>

      {/* effect-type chips (inside an Effects subcategory) */}
      {etChips && etChips.length > 1 && (
        <div
          style={{
            flexShrink: 0,
            display: "flex",
            flexWrap: "wrap",
            gap: 5,
            padding: "8px 16px 9px",
            borderBottom: `0.5px solid ${t.hairline}`,
            background: t.bgAlt,
          }}
        >
          {facetBtn(`All ${String(sel.sub)}`, et === null, () => {
            setEt(null);
          })}
          {etChips.map((ty) =>
            facetBtn(ty, et === ty, () => {
              setEt(et === ty ? null : ty);
            }),
          )}
        </div>
      )}

      {/* rail + wall */}
      <div
        style={{
          flex: 1,
          minHeight: 0,
          display: "grid",
          gridTemplateColumns: "194px 1fr",
        }}
      >
        {/* rail */}
        <div
          style={{
            borderRight: `0.5px solid ${t.hairline}`,
            background: t.bgAlt,
            overflowY: "auto",
            padding: "9px 9px 14px",
          }}
        >
          <div
            style={{
              fontFamily: t.mono,
              fontSize: t.fsMicro2,
              letterSpacing: t.lsWide,
              color: t.faint,
              textTransform: "uppercase",
              padding: "4px 9px 7px",
            }}
          >
            Category
          </div>
          {railRow(
            "All models",
            TOTAL,
            sel.cat === "all",
            () => {
              choose("all");
            },
            { k: "all" },
          )}
          {CAT_ORDER.map((c) => {
            if (c === "Effects") {
              const active = sel.cat === "Effects";
              return (
                <div key="fx">
                  {railRow(
                    "Effects",
                    CAT_COUNT.Effects,
                    active && !sel.sub,
                    () => {
                      choose("Effects");
                      setFxOpen(true);
                    },
                    { caret: fxOpen },
                  )}
                  {fxOpen &&
                    SUB_ORDER.map((s) =>
                      railRow(
                        s,
                        SUB_COUNT[s],
                        sel.cat === "Effects" && sel.sub === s,
                        () => {
                          choose("Effects", s);
                        },
                        { child: true, k: "fx-" + s },
                      ),
                    )}
                </div>
              );
            }
            return railRow(
              c,
              CAT_COUNT[c] || 0,
              sel.cat === c,
              () => {
                choose(c);
              },
              { k: c },
            );
          })}
        </div>

        {/* wall */}
        <div style={{ minHeight: 0, overflowY: "auto" }}>
          {shown.length === 0 ? (
            <div
              style={{
                padding: "54px 24px",
                textAlign: "center",
                fontFamily: t.sans,
                fontSize: t.fsBody2,
                color: t.mutedInk,
              }}
            >
              No models match {q ? `“${q}”` : "these filters"}.
            </div>
          ) : (
            groups.map((g) => (
              <section key={g.key}>
                <div
                  style={{
                    position: "sticky",
                    top: 0,
                    zIndex: 1,
                    display: "flex",
                    alignItems: "baseline",
                    gap: 10,
                    padding: "9px 16px 7px",
                    background: "rgba(255,255,255,0.92)",
                    backdropFilter: "blur(6px)",
                    borderBottom: `0.5px solid ${t.hairline}`,
                  }}
                >
                  <span
                    style={{
                      fontFamily: t.serif,
                      fontSize: t.fsName2,
                      color: t.ink,
                    }}
                  >
                    {g.label}
                  </span>
                  <span
                    style={{
                      fontFamily: t.mono,
                      fontSize: t.fsData2,
                      color: t.accentDeep,
                    }}
                  >
                    {g.blocks.length}
                  </span>
                </div>
                <div
                  style={{
                    display: "flex",
                    flexWrap: "wrap",
                    gap: 6,
                    padding: "12px 13px 16px",
                    alignContent: "flex-start",
                  }}
                >
                  {g.blocks.map((r) => (
                    <ModelCard
                      key={r.cid}
                      r={r}
                      t={t}
                      selected={pick?.cid === r.cid}
                      icon={icon}
                      onPick={setPick}
                    />
                  ))}
                </div>
              </section>
            ))
          )}
        </div>
      </div>

      <DetailBar r={pick} t={t} />
    </div>
  );
}
