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
// (catalog-models.jsx); styling reads the app's theme tokens. Split: the filter
// state + grouping in ./catalog/useCatalogFilter; the card, tile, badges and
// detail bar in ./catalog/.

import { useState } from "react";

import { useTheme } from "../theme/ThemeContext";
import { plainButton, type ThemeTokens } from "../theme/tokens";
import { Icon } from "../ui/Icon";
import { SearchInput } from "../ui/primitives";
import {
  TOTAL,
  CAT_ORDER,
  SUB_ORDER,
  CAT_COUNT,
  SUB_COUNT,
} from "../models/catalog";
import { DetailBar } from "./catalog/DetailBar";
import { ModelCard } from "./catalog/ModelCard";
import { useCatalogFilter } from "./catalog/useCatalogFilter";

// Off-state chip fill: white (light-only), matching the prototype's chipBg.
const chipBgFor = (t: ThemeTokens) => t.bg;

/** Vertical padding of the "No models match" empty wall — off the spacing ramp
 *  (48 + 6), kept exact so the empty state sits where canon puts it. */
const EMPTY_WALL_PAD_Y = 54;

type Density = "comfortable" | "compact";

// ── the page ────────────────────────────────────────────────────────────────

export interface CatalogViewProps {
  density?: Density;
}

export function CatalogView({ density = "comfortable" }: CatalogViewProps) {
  const { t } = useTheme();
  const {
    q,
    setQ,
    deferredQ,
    sel,
    choose,
    et,
    setEt,
    routing,
    setRouting,
    convOnly,
    setConvOnly,
    sort,
    setSort,
    cpuDir,
    setCpuDir,
    pick,
    setPick,
    hasStereo,
    shown,
    groups,
    etChips,
    showConv,
  } = useCatalogFilter();
  const [fxOpen, setFxOpen] = useState(true);
  const icon = density === "compact" ? 54 : 66;

  // ── small renderers ──
  const railRow = (
    label: string,
    count: number,
    active: boolean,
    onClick: () => void,
    opts: { child?: boolean; caret?: boolean; k?: string } = {},
  ) => (
    <button
      type="button"
      key={opts.k ?? label}
      aria-current={active ? true : undefined}
      aria-expanded={opts.caret}
      onClick={onClick}
      style={{
        // Hand-rolled reset (not `plainButton`): the varying `borderLeft` bar must
        // not share the style object with a `border` shorthand.
        appearance: "none",
        margin: 0,
        fontFamily: "inherit",
        fontSize: "inherit",
        color: "inherit",
        textAlign: "left",
        width: "100%",
        boxSizing: "border-box",
        borderTop: "none",
        borderRight: "none",
        borderBottom: "none",
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: t.space4,
        padding: opts.child
          ? `${String(t.space3)}px ${String(t.space4)}px ${String(t.space3)}px 36px`
          : `${String(t.space4)}px ${String(t.space4)}px`,
        borderRadius: t.rMd,
        cursor: "pointer",
        background: active ? t.accentSoft : "transparent",
        borderLeft: active ? `2px solid ${t.accent}` : "2px solid transparent",
      }}
    >
      <span
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.space3,
          minWidth: 0,
        }}
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
    </button>
  );

  const facetBtn = (
    label: string,
    on: boolean,
    onClick: () => void,
    disabled = false,
  ) => (
    <button
      type="button"
      key={label}
      aria-pressed={on}
      disabled={disabled}
      onClick={onClick}
      style={{
        ...plainButton(),
        fontFamily: t.mono,
        fontSize: t.fsData2,
        letterSpacing: "0.03em",
        color: disabled ? t.faint : on ? t.onAccent : t.ink2,
        background: on ? t.accent : chipBgFor(t),
        border: `0.5px solid ${on ? t.accent : t.hairlineStrong}`,
        borderRadius: t.rPill,
        padding: `${String(t.space2)}px ${String(t.space5)}px`,
        cursor: disabled ? "default" : "pointer",
        opacity: disabled ? 0.5 : 1,
        userSelect: "none",
        whiteSpace: "nowrap",
      }}
    >
      {label}
    </button>
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
          gap: t.space6,
          padding: `${String(t.space5)}px ${String(t.space8)}px ${String(t.space5)}px`,
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
        <div style={{ display: "flex", gap: t.space3, alignItems: "center" }}>
          {facetBtn("Mono", routing === "mono", () => {
            setRouting(routing === "mono" ? "all" : "mono");
          })}
          {facetBtn(
            "Stereo",
            routing === "stereo",
            () => {
              setRouting(routing === "stereo" ? "all" : "stereo");
            },
            // Disabled only while OFF: a scope with no stereo models can't turn
            // it on, but an active Stereo filter must always be clearable (it
            // survives a category change).
            !hasStereo && routing !== "stereo",
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
        <div style={{ display: "flex", gap: t.space3, alignItems: "center" }}>
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
            gap: t.space3,
            padding: `${String(t.space4)}px ${String(t.space8)}px ${String(t.space4)}px`,
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
            padding: `${String(t.space4)}px ${String(t.space4)}px ${String(t.space7)}px`,
          }}
        >
          <div
            style={{
              fontFamily: t.mono,
              fontSize: t.fsMicro2,
              letterSpacing: t.lsWide,
              color: t.faint,
              textTransform: "uppercase",
              padding: `${String(t.space2)}px ${String(t.space4)}px ${String(t.space4)}px`,
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
                padding: `${String(EMPTY_WALL_PAD_Y)}px ${String(t.space10)}px`,
                textAlign: "center",
                fontFamily: t.sans,
                fontSize: t.fsBody2,
                color: t.mutedInk,
              }}
            >
              No models match {deferredQ ? `“${deferredQ}”` : "these filters"}.
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
                    gap: t.space5,
                    padding: `${String(t.space4)}px ${String(t.space8)}px ${String(t.space4)}px`,
                    background: t.bgFrost,
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
                    gap: t.space3,
                    padding: `${String(t.space6)}px ${String(t.space6)}px ${String(t.space8)}px`,
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
