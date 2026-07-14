// src/views/SignalChainView.tsx — signal-chain strip geometry for all TMP path
// template families. Blocks render through the shared BlockArt SVG engine so
// the active strip and Models tab stay identical by construction.

import React from "react";
import { useTheme } from "../theme/ThemeContext";

import { BlockArt, HalfStackArt } from "../ui/BlockArt";
import type { HalfStackSpec } from "../models/blockArt";
import { Icon, type IconName } from "../ui/Icon";
import { Skel } from "../ui/Skeleton";

const MONO = "'JetBrains Mono', ui-monospace, monospace";
const STRIP_LBL = 18;
const GHOST_INK = "rgba(15,17,21,0.12)";
const ACCENT = "#d97757";
/** Shared bracket/diamond/emptyLine border width — the SPLIT/MIX geometry comments
 *  below rely on these strokes lining up pixel-for-pixel, so they share one constant. */
const STROKE_W = 1.5;
const STROKE_W_PX = `${String(STROKE_W)}px`;

/** How a tile was changed by an edit (Copy feature). `added` → a `+` badge (inserted
 *  block); `replaced` → a `refresh` ⟳ badge (in-place model swap). Drives the badge
 *  icon + the terracotta "changed" styling. */
export type TileChange = "added" | "replaced";

export interface StripBlock {
  icon?: string;
  tone?: string;
  /** ref-derived per-block body color (pedals) — overlays the tone default. */
  body?: string;
  /** ref-derived control-panel color behind the knobs/sliders (EQ/filter pedals). */
  panel?: string;
  /** stompbox footswitch style (pedal-form blocks) — plate/metal/round. */
  footswitch?: "plate" | "metal" | "round";
  /** Fender reverb cream-chassis accent (footswitch-band colour); the 8 reverbs. */
  accent?: string;
  /** terse caption / 1.8 dispatch token (= art.short) the engine reads. */
  lab?: string;
  name: string;
  /** the fuller Pro-Control-style model name, shown on hover (`title`); the tile's
   *  visible caption stays the terse `name`. */
  fullName?: string;
  /** The block's model id (FenderId) — used by the e2e `data-block-tile` hook for
   *  model-exact matching against candidates; optional (the Level strip omits it). */
  model?: string;
  bypassed?: boolean;
  /** Dual-cab CabSim params (display-only). When `cabSim2Enabled` the strip's
   *  `expandDualCab` helper splits this ONE block into two parallel named cab tiles
   *  (cab1 from `cabSimId`, cab2 from `cabSimId2`); a single-cab CabSim just names
   *  the one tile from `cabSimId`. */
  cabSimId?: string;
  cabSimId2?: string;
  cabSim2Enabled?: boolean;
  /** Head-over-cab art for an amp that carries its own cab (a combo/half-stack,
   *  e.g. a HIWAY head on a British 4×12). Set ONLY when the device amp node has a
   *  `cabsimid`; bare heads omit it and render as a plain amp tile. */
  halfStack?: HalfStackSpec;
  /** Tap handler — set ONLY by the interactive Copy path. When present the tile
   *  renders a rounded hit-box + becomes a button; the Level page omits it, so its
   *  tiles render exactly as before (no box, no badge). */
  onClick?: () => void;
  /** Selected (its inline editor is open) — accent ring + tinted label. */
  selected?: boolean;
  /** Edit badge, when this tile was changed. */
  change?: TileChange;
}

export type StripStage =
  | { kind: "series"; blocks: StripBlock[] }
  | { kind: "split"; a: StripBlock[]; b: StripBlock[] };

export type SourceType = "guitar" | "mic";
export type SinkType = "out" | "out1" | "out2";

export interface StripInputLane {
  type: SourceType;
  blocks: StripBlock[];
}

export interface StripOutputLane {
  type: Extract<SinkType, "out1" | "out2">;
  blocks: StripBlock[];
}

export interface StripIndependentLane {
  input: SourceType;
  output: Extract<SinkType, "out1" | "out2">;
  blocks: StripBlock[];
}

export interface StripGraph {
  template?: string | null;
  inputType?: SourceType;
  outputType?: SinkType;
  inputs?: { a: StripInputLane; b: StripInputLane } | null;
  outputs?: { a: StripOutputLane; b: StripOutputLane } | null;
  lanes?: StripIndependentLane[] | null;
  stages: StripStage[];
}

type Size = "md" | "sm";
type BracketSide = "left" | "right";

// Endpoint glyphs come from the DS icon catalog (`<Icon>`), never hand-rolled SVG:
// the instrument input is the canonical `cable` plug, the mic is `mic`, the output is the
// `wave` meter. Keeps these nodes in the design system (one source of truth) instead of a
// private inline drawing that drifts (the old guitar SVG rendered as a paintbrush).
const ENDPOINT_ICON: Record<SourceType | "out", IconName> = {
  guitar: "cable",
  mic: "mic",
  out: "wave",
};

interface BlockTileProps {
  b: StripBlock;
  size: Size;
  skeleton?: boolean;
}

function BlockTile({ b, size, skeleton }: BlockTileProps) {
  const { t } = useTheme();
  const dims =
    size === "sm" ? { w: 56, art: 46, font: 8 } : { w: 70, art: 58, font: 9 };
  const byp = b.bypassed === true;
  if (skeleton) {
    return (
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          width: dims.w,
          flexShrink: 0,
        }}
      >
        <div
          style={{
            flex: 1,
            minHeight: dims.art,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          <Skel w={dims.art} h={dims.art} r={9} />
        </div>
        <div
          style={{
            height: STRIP_LBL,
            display: "flex",
            alignItems: "flex-start",
            justifyContent: "center",
            paddingTop: t.space3,
          }}
        >
          <Skel w={Math.round(dims.w * 0.52)} h={6} r={3} />
        </div>
      </div>
    );
  }
  // Copy-only interactive extensions. When `onClick` is set the tile becomes a
  // button with a rounded hit-box (terracotta when changed/selected) + an edit
  // badge; otherwise it renders exactly as the Level page expects (no box/badge).
  const interactive = b.onClick != null;
  const changed = b.change != null;
  const selected = b.selected === true;
  // The rounded hit-box accent (Copy only) — empty when non-interactive so the Level
  // page's tiles render untouched.
  const boxAccent: React.CSSProperties = !interactive
    ? {}
    : {
        borderRadius: 9,
        transition: "border-color .12s, background .12s",
        border: selected
          ? `2px solid ${ACCENT}`
          : changed
            ? "1.5px solid rgba(217,119,87,0.55)"
            : "1px solid rgba(15,17,21,0.12)",
        background: selected
          ? "rgba(217,119,87,0.10)"
          : changed
            ? "rgba(217,119,87,0.06)"
            : "transparent",
      };

  return (
    <div
      role={interactive ? "button" : undefined}
      // e2e hook: a stable selector for an interactive (Copy-path) block tile, carrying the
      // block's MODEL id so a test can match it model-exactly against candidates (the
      // display label differs between the tile and the candidate chip). Absent on the
      // non-interactive Level strip (no behavior/appearance change).
      data-block-tile={interactive ? (b.model ?? b.name) : undefined}
      // Hover shows the fuller Pro-Control model name; the visible caption stays terse.
      title={b.fullName}
      onClick={b.onClick}
      style={{
        display: "flex",
        flexDirection: "column",
        width: dims.w,
        flexShrink: 0,
        opacity: byp ? 0.34 : 1,
        cursor: interactive ? "pointer" : undefined,
      }}
    >
      <div
        style={{
          flex: 1,
          minHeight: dims.art,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <div
          style={{
            position: "relative",
            width: dims.art + (interactive ? 10 : 0),
            height: dims.art + (interactive ? 10 : 0),
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            ...boxAccent,
          }}
        >
          <div
            style={{
              position: "relative",
              width: dims.art,
              height: dims.art,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              filter: "drop-shadow(0 2px 3px rgba(15,17,21,0.18))",
            }}
          >
            {b.halfStack ? (
              // Amp that carries its own cab (combo/half-stack): head over cab, so the
              // strip mirrors the unit (Pro Control draws preset 003's HIWAY this way).
              // cabW is scaled so the stacked height ≈ the art box (no row reflow).
              <HalfStackArt
                topIcon={b.halfStack.topIcon}
                topTone={b.halfStack.topTone}
                topLab={b.halfStack.topLab}
                cabIcon={b.halfStack.cabIcon}
                cabTone={b.halfStack.cabTone}
                cabLab=""
                cabW={Math.round(dims.art * 0.62)}
              />
            ) : (
              <BlockArt
                icon={b.icon}
                tone={b.tone}
                lab={b.lab}
                footswitch={b.footswitch}
                bodyColor={b.body}
                accentColor={b.accent}
                panelColor={b.panel}
                size={dims.art}
                label={false}
              />
            )}
            {byp && (
              <div
                style={{
                  position: "absolute",
                  inset: 0,
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                }}
              >
                <div
                  style={{
                    width: "92%",
                    height: 1.6,
                    background: "#33373f",
                    transform: "rotate(-28deg)",
                    borderRadius: 2,
                  }}
                />
              </div>
            )}
          </div>
          {changed && (
            <span
              title={b.change === "replaced" ? "replaced" : "added"}
              style={{
                position: "absolute",
                top: -7,
                right: -5,
                zIndex: 2,
                width: 15,
                height: 15,
                borderRadius: 999,
                background: ACCENT,
                border: "1px solid #fff",
                display: "inline-flex",
                alignItems: "center",
                justifyContent: "center",
                boxShadow: "0 1px 2px rgba(15,17,21,0.2)",
              }}
            >
              <Icon
                name={b.change === "replaced" ? "refresh" : "plus"}
                size={b.change === "replaced" ? 10 : 9}
                stroke="#fff"
                strokeWidth={2.4}
              />
            </span>
          )}
        </div>
      </div>
      <div
        style={{
          height: STRIP_LBL,
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "center",
          paddingTop: t.space2,
        }}
      >
        <span
          style={{
            fontFamily: MONO,
            fontSize: dims.font,
            letterSpacing: "0.04em",
            color: selected ? "#a5421b" : "var(--strip-ink, #5b554a)",
            lineHeight: 1,
            textTransform: "uppercase",
            whiteSpace: "nowrap",
          }}
        >
          {b.name}
        </span>
      </div>
    </div>
  );
}

interface WireProps {
  ink: string;
  w?: number;
  // A fork/join lane's own row is narrower than its longer sibling — grow
  // instead of a fixed width so the trailing endpoint node (SinkNode/
  // SourceNode) still lines up across lanes, with the drawn line stretching
  // to meet it rather than leaving a dangling gap.
  grow?: boolean;
}

function Wire({ ink, w = 18, grow }: WireProps) {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        ...(grow ? { flex: 1, minWidth: w } : { width: w, flexShrink: 0 }),
      }}
    >
      <div style={{ flex: 1, display: "flex", alignItems: "center" }}>
        <div style={{ width: "100%", height: 1.5, background: ink }} />
      </div>
      <div style={{ height: STRIP_LBL }} />
    </div>
  );
}

interface SourceNodeProps {
  type: SourceType;
  ink: string;
  skeleton?: boolean;
  size: Size;
}

function SourceNode({ type, ink, skeleton, size }: SourceNodeProps) {
  const label = type === "guitar" ? "GUITAR" : "MIC/LINE";
  return (
    <EndpointNode
      icon={type}
      label={label}
      ink={ink}
      skeleton={skeleton}
      size={size}
      skeletonLabelWidth={type === "guitar" ? 28 : 34}
    />
  );
}

interface SinkNodeProps {
  type: SinkType;
  ink: string;
  skeleton?: boolean;
  size: Size;
}

function SinkNode({ type, ink, skeleton, size }: SinkNodeProps) {
  const label = type === "out1" ? "OUT 1" : type === "out2" ? "OUT 2" : "OUT";
  return (
    <EndpointNode
      icon="out"
      label={label}
      ink={ink}
      skeleton={skeleton}
      size={size}
      skeletonLabelWidth={type === "out" ? 22 : 28}
    />
  );
}

interface EndpointNodeProps {
  icon: SourceType | "out";
  label: string;
  ink: string;
  skeleton?: boolean;
  size: Size;
  skeletonLabelWidth: number;
}

function EndpointNode({
  icon,
  label,
  ink,
  skeleton,
  size,
  skeletonLabelWidth,
}: EndpointNodeProps) {
  const { t } = useTheme();
  const artH = size === "sm" ? 46 : 58;
  const fs = size === "sm" ? 7 : 8;
  if (skeleton) {
    return (
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          width: 34,
          flexShrink: 0,
        }}
      >
        <div
          style={{
            flex: 1,
            minHeight: artH,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          <Skel w={34} h={34} r={999} />
        </div>
        <div
          style={{
            height: STRIP_LBL,
            display: "flex",
            alignItems: "flex-start",
            justifyContent: "center",
            paddingTop: t.space3,
          }}
        >
          <Skel w={skeletonLabelWidth} h={6} r={3} />
        </div>
      </div>
    );
  }
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        width: 34,
        flexShrink: 0,
      }}
    >
      <div
        style={{
          flex: 1,
          minHeight: artH,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <div
          style={{
            // border-box so the 1.5px border is INSIDE the 34px box: otherwise the
            // ring's outer width (34 + 2×1.5 = 37) exceeds its 34px flex column and
            // flex-shrink squashes the width back to 34 while height stays 34 — an
            // oval that reads as "circle cropped left & right". border-box → no shrink.
            boxSizing: "border-box",
            width: 34,
            height: 34,
            borderRadius: 999,
            border: `1.5px solid ${ink}`,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: ink,
          }}
        >
          <Icon name={ENDPOINT_ICON[icon]} size={17} stroke={ink} />
        </div>
      </div>
      <div
        style={{
          height: STRIP_LBL,
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "center",
          paddingTop: t.space2,
        }}
      >
        <span
          style={{
            fontFamily: MONO,
            fontSize: fs,
            letterSpacing: "0.1em",
            color: "var(--strip-ink,#5b554a)",
            lineHeight: 1,
            whiteSpace: "nowrap",
          }}
        >
          {label}
        </span>
      </div>
    </div>
  );
}

interface DiamondNodeProps {
  kind: "split" | "mix" | "join";
  ink: string;
  skeleton?: boolean;
}

function DiamondNode({ kind, ink, skeleton }: DiamondNodeProps) {
  const label = kind === "split" ? "SPLIT" : kind === "mix" ? "MIX" : "JOIN";
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        width: 14,
        flexShrink: 0,
      }}
    >
      <div
        style={{
          flex: 1,
          minHeight: 14,
          position: "relative",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <div
          style={{
            // border-box + no shrink so the 1.5px border stays INSIDE the 12×12
            // box and the flex cell can't squash the width — otherwise the
            // square renders a touch wider than tall (a lopsided diamond).
            boxSizing: "border-box",
            flexShrink: 0,
            width: 12,
            height: 12,
            transform: "rotate(45deg)",
            border: `${STROKE_W_PX} solid ${ink}`,
            borderRadius: 2,
          }}
        />
        {!skeleton && (
          <div
            style={{
              position: "absolute",
              top: "calc(50% + 12px)",
              left: -16,
              right: -16,
              textAlign: "center",
              fontFamily: MONO,
              fontSize: 7.5,
              letterSpacing: "0.1em",
              color: "var(--strip-ink,#5b554a)",
              lineHeight: 1,
              whiteSpace: "nowrap",
            }}
          >
            {label}
          </div>
        )}
      </div>
      <div style={{ height: STRIP_LBL }} />
    </div>
  );
}

// An empty split branch reserves the lane's height/width but draws NO wire of
// its own: `SplitGroup` paints the straight-through as one full-width line at
// the exact measured bracket-border y (see `emptyLine`), so the branch wire and
// the bracket corners are one continuous stroke instead of two elements that
// drift apart vertically (and double up where they overlap the vertical bar).
function EmptyLane({ size }: { size: Size }) {
  const artH = size === "sm" ? 46 : 58;
  const minW = size === "sm" ? 56 : 70;
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        // Grow to fill the lane row (its sibling populated lane sets the column
        // width) so the painted line spans the whole SPLIT→MIX distance.
        flex: 1,
        minWidth: minW,
      }}
    >
      <div style={{ flex: 1, minHeight: artH }} />
      <div style={{ height: STRIP_LBL }} />
    </div>
  );
}

// Shared bracket geometry for any split/join point (one or two diamonds, two
// lane rows) — DOM-measured from the two lane rows' real rendered centres
// rather than derived from hardcoded tile-height constants, so it stays exact
// regardless of label/font metrics or lane content length (the old per-lane
// SVG curve math assumed both lanes were exactly the same fixed height, which
// left a visible gap once a gtrSplit/micSplit preset had a different block
// count per output lane). Used by SplitGroup (two-sided, in-line splits like
// gtrParallel1/2) and ForkTail/JoinHead (one-sided, split-output/dual-input
// lanes) alike. Each caller runs its OWN `useLayoutEffect` calling this
// function (refs created via useRef in the SAME component) rather than a
// shared hook, so eslint's exhaustive-deps can see the refs are useRef-stable
// instead of flagging them as effect dependencies. This helper is the shared
// MATH only (not a hook itself — no eslint hook rules apply to it).
function measureLaneBracket(
  colRef: React.RefObject<HTMLDivElement | null>,
  aRef: React.RefObject<HTMLDivElement | null>,
  bRef: React.RefObject<HTMLDivElement | null>,
  prev: { top: number; height: number } | null,
): { top: number; height: number } | null {
  const col = colRef.current,
    ar = aRef.current,
    br = bRef.current;
  if (!col || !ar || !br) return prev;
  const ct = col.getBoundingClientRect();
  const arr = ar.getBoundingClientRect();
  const brr = br.getBoundingClientRect();
  if (!arr.height || !brr.height) return prev;
  const aC = arr.top - ct.top + (arr.height - STRIP_LBL) / 2;
  const bC = brr.top - ct.top + (brr.height - STRIP_LBL) / 2;
  const next = { top: aC, height: Math.max(0, bC - aC) };
  return prev &&
    Math.abs(prev.top - next.top) < 0.5 &&
    Math.abs(prev.height - next.height) < 0.5
    ? prev
    : next;
}

// The "[" (or mirrored "]") cap for one side of a bracket, sized from a
// `measureLaneBracket` result. `boxSizing:border-box` is load-bearing: with
// the default content-box the two 1.5px horizontal borders render OUTSIDE the
// `height`, so the border-box grows to `brk.height + 3` and the bottom border
// lands ~3px below `brk.top + brk.height` — a gap `SplitGroup`'s `emptyLine`
// has to match. With border-box the border box is exactly `brk.height`:
// borderTop occupies [brk.top, brk.top+1.5] and borderBottom
// [brk.top+brk.height-1.5, brk.top+brk.height] — flush with `emptyLine`. It
// also fixes X: border-box folds the 1.5px vertical border INTO the 9px, so
// the left cap's border box spans [-9, 0] (was [-9, 1.5]) and its top/bottom
// strokes end exactly at x=0; symmetrically the right cap's strokes start at
// x=W. Used single-sided by ForkTail/JoinHead and rendered twice (once per
// side) by SplitGroup's two-sided bracket.
function LaneBracketCap({
  brk,
  side,
  ink,
}: {
  brk: { top: number; height: number } | null;
  side: BracketSide;
  ink: string;
}) {
  if (!brk) return null;
  return (
    <div
      style={{
        position: "absolute",
        boxSizing: "border-box",
        [side]: -9,
        top: brk.top,
        height: brk.height,
        width: 9,
        borderTop: `${STROKE_W_PX} solid ${ink}`,
        borderBottom: `${STROKE_W_PX} solid ${ink}`,
        [side === "left" ? "borderLeft" : "borderRight"]:
          `${STROKE_W_PX} solid ${ink}`,
        borderRadius: side === "left" ? "5px 0 0 5px" : "0 5px 5px 0",
      }}
    />
  );
}

interface ForkTailProps {
  outputs: NonNullable<StripGraph["outputs"]>;
  ink: string;
  size: Size;
  skeleton?: boolean;
  laneGap: number;
  renderRow: (arr: StripBlock[], pfx: string) => React.ReactNode[];
}

// Split-OUTPUT lanes (gtrSplit/micSplit): one SPLIT diamond feeds two
// independent lanes that each terminate in their own physical OUT — unlike
// SplitGroup's in-line split, there's no MIX diamond on the far side, so only
// one bracket (on the split side) is needed. Lanes may hold a different
// number of blocks (HW-confirmed on a real "Instrument Split" preset): the
// column wrapper stretches both rows to a common width (the flex column
// default) so the two OUT jacks land on the same column regardless of lane
// length, but the STRETCH is absorbed by a trailing growing `Wire` right
// before SinkNode — not by the leading gap — so the tiles stay anchored
// immediately after the split instead of being shoved rightward.
function ForkTail({
  outputs,
  ink,
  size,
  skeleton,
  laneGap,
  renderRow,
}: ForkTailProps) {
  const { t } = useTheme();
  const colRef = React.useRef<HTMLDivElement>(null);
  const aRef = React.useRef<HTMLDivElement>(null);
  const bRef = React.useRef<HTMLDivElement>(null);
  const lanes = [outputs.a, outputs.b];
  const laneRefs = [aRef, bRef];
  const sig = `${String(outputs.a.blocks.length)}|${String(outputs.b.blocks.length)}`;
  const [brk, setBrk] = React.useState<{ top: number; height: number } | null>(
    null,
  );
  React.useLayoutEffect(() => {
    setBrk((p) => measureLaneBracket(colRef, aRef, bRef, p));
  }, [sig]);
  return (
    <>
      <DiamondNode kind="split" ink={ink} skeleton={skeleton} />
      <div
        ref={colRef}
        style={{
          display: "flex",
          flexDirection: "column",
          gap: laneGap,
          position: "relative",
          marginLeft: t.space6,
        }}
      >
        <LaneBracketCap brk={brk} side="left" ink={ink} />
        {lanes.map((lane, li) => (
          <div
            key={li}
            ref={laneRefs[li]}
            style={{ display: "flex", alignItems: "stretch", gap: t.space5 }}
          >
            {lane.blocks.length > 0 && renderRow(lane.blocks, `o${String(li)}`)}
            <Wire ink={ink} grow w={lane.blocks.length > 0 ? 10 : 18} />
            <SinkNode
              type={lane.type}
              ink={ink}
              skeleton={skeleton}
              size={size}
            />
          </div>
        ))}
      </div>
    </>
  );
}

interface JoinHeadProps {
  inputs: NonNullable<StripGraph["inputs"]>;
  ink: string;
  size: Size;
  skeleton?: boolean;
  laneGap: number;
  renderRow: (arr: StripBlock[], pfx: string) => React.ReactNode[];
}

// Dual-INPUT lanes (gtrMicSeries/gtrMicMix/etc.): mirror of ForkTail — two
// independent lanes converge into one JOIN diamond, so the single bracket
// sits on the join side. Same stretch-absorbed-by-a-growing-Wire rationale as
// ForkTail applies here, mirrored: the growing Wire sits right after
// SourceNode so the two source jacks stay aligned regardless of lane length.
function JoinHead({
  inputs,
  ink,
  size,
  skeleton,
  laneGap,
  renderRow,
}: JoinHeadProps) {
  const { t } = useTheme();
  const colRef = React.useRef<HTMLDivElement>(null);
  const aRef = React.useRef<HTMLDivElement>(null);
  const bRef = React.useRef<HTMLDivElement>(null);
  const lanes = [inputs.a, inputs.b];
  const laneRefs = [aRef, bRef];
  const sig = `${String(inputs.a.blocks.length)}|${String(inputs.b.blocks.length)}`;
  const [brk, setBrk] = React.useState<{ top: number; height: number } | null>(
    null,
  );
  React.useLayoutEffect(() => {
    setBrk((p) => measureLaneBracket(colRef, aRef, bRef, p));
  }, [sig]);
  return (
    <>
      <div
        ref={colRef}
        style={{
          display: "flex",
          flexDirection: "column",
          gap: laneGap,
          position: "relative",
          marginRight: t.space6,
        }}
      >
        <LaneBracketCap brk={brk} side="right" ink={ink} />
        {lanes.map((lane, li) => (
          <div
            key={li}
            ref={laneRefs[li]}
            style={{ display: "flex", alignItems: "stretch", gap: t.space5 }}
          >
            <SourceNode
              type={lane.type}
              ink={ink}
              skeleton={skeleton}
              size={size}
            />
            <Wire ink={ink} grow w={lane.blocks.length > 0 ? 10 : 18} />
            {lane.blocks.length > 0 && renderRow(lane.blocks, `i${String(li)}`)}
          </div>
        ))}
      </div>
      <DiamondNode kind="join" ink={ink} skeleton={skeleton} />
    </>
  );
}

interface SplitGroupProps {
  a: StripBlock[];
  b: StripBlock[];
  ink: string;
  laneGap: number;
  skeleton?: boolean;
  pfx: string;
  renderRow: (arr: StripBlock[], pfx: string) => React.ReactNode[];
}

// The two parallel branches between a SPLIT and a MIX diamond, joined by the
// rounded brackets. Branch heights vary (a half-stack tile is much taller than a
// pedal branch), so the brackets are MEASURED from the rendered branch art centres
// at runtime rather than from a fixed tile height — for equal-height branches this
// resolves to the same geometry the old fixed brackets used.
function SplitGroup({
  a,
  b,
  ink,
  laneGap,
  skeleton,
  pfx,
  renderRow,
}: SplitGroupProps) {
  const { t } = useTheme();
  const colRef = React.useRef<HTMLDivElement>(null);
  const aRef = React.useRef<HTMLDivElement>(null);
  const bRef = React.useRef<HTMLDivElement>(null);
  const [brk, setBrk] = React.useState<{ top: number; height: number } | null>(
    null,
  );
  // Re-measure when a height-affecting input changes (branch lengths or strip size).
  const sig = `${String(a.length)}|${String(b.length)}`;
  React.useLayoutEffect(() => {
    setBrk((p) => measureLaneBracket(colRef, aRef, bRef, p));
  }, [sig]);

  // The straight-through line for an EMPTY branch, painted here (not by
  // `EmptyLane`) so it lands on the SAME measured y as the bracket's top/bottom
  // border and reads as one continuous stroke. `atTop` picks the branch: the top
  // matches the border-box bracket's borderTop at [brk.top, brk.top+1.5]; the
  // bottom matches its borderBottom at [brk.top+brk.height-1.5, brk.top+brk.height].
  const emptyLine = (atTop: boolean) =>
    brk ? (
      <div
        style={{
          position: "absolute",
          left: 0,
          right: 0,
          top: atTop ? brk.top : brk.top + brk.height - STROKE_W,
          height: STROKE_W,
          background: ink,
        }}
      />
    ) : null;

  return (
    <>
      <DiamondNode kind="split" ink={ink} skeleton={skeleton} />
      <div
        ref={colRef}
        style={{
          display: "flex",
          flexDirection: "column",
          gap: laneGap,
          position: "relative",
          marginLeft: t.space6,
          marginRight: t.space6,
        }}
      >
        <LaneBracketCap brk={brk} side="left" ink={ink} />
        <LaneBracketCap brk={brk} side="right" ink={ink} />
        {a.length === 0 && emptyLine(true)}
        {b.length === 0 && emptyLine(false)}
        <div
          ref={aRef}
          style={{ display: "flex", alignItems: "stretch", gap: t.space5 }}
        >
          {renderRow(a, `${pfx}a`)}
        </div>
        <div
          ref={bRef}
          style={{ display: "flex", alignItems: "stretch", gap: t.space5 }}
        >
          {renderRow(b, `${pfx}b`)}
        </div>
      </div>
      <DiamondNode kind="mix" ink={ink} skeleton={skeleton} />
    </>
  );
}

export interface SignalChainViewProps {
  graph: StripGraph;
  size?: Size;
  skeleton?: boolean;
}

function SignalChainViewImpl({
  graph,
  size = "md",
  skeleton,
}: SignalChainViewProps) {
  const { t } = useTheme();
  const ink = skeleton ? GHOST_INK : "rgba(15,17,21,0.34)";
  const stripInk = "#5b554a";
  const laneGap = 10;

  const tile = (b: StripBlock, key: React.Key) => (
    <BlockTile key={key} b={b} size={size} skeleton={skeleton} />
  );
  const row = (arr: StripBlock[], pfx: string): React.ReactNode[] =>
    arr.length > 0
      ? arr.flatMap((b, i) =>
          i === 0
            ? [tile(b, `${pfx}t${String(i)}`)]
            : [
                <Wire key={`${pfx}w${String(i)}`} ink={ink} />,
                tile(b, `${pfx}t${String(i)}`),
              ],
        )
      : [<EmptyLane key={`${pfx}empty`} size={size} />];

  const splitSection = (
    st: { a: StripBlock[]; b: StripBlock[] },
    key: string,
  ) => (
    <SplitGroup
      key={key}
      pfx={key}
      a={st.a}
      b={st.b}
      ink={ink}
      laneGap={laneGap}
      skeleton={skeleton}
      renderRow={row}
    />
  );

  // Inner strip — the handoff geometry verbatim (padding "2px 8px 4px"), with NO overflow
  // of its own so its overflow-y stays `visible` and the Copy edit badge (top:-7) paints
  // freely above series tiles.
  const stripStyle: React.CSSProperties = {
    ["--strip-ink" as string]: stripInk,
    display: "flex",
    // Each block is its own column (art + an 18px label zone). `center` keeps every
    // column at its natural height so each label hugs its own block; `stretch` grew
    // short columns to the tallest (a split group), dropping their labels onto a
    // shared bottom baseline far below the art (design drift #1). Lane rows inside a
    // split stay `stretch` — they want equal-height branches for the bracket math.
    alignItems: "center",
    // `safe center` centers the strip when it fits, but falls back to flex-start
    // when the chain overflows — so a long chain scrolls from its FIRST block
    // (plain `center` clips the left end out of the scroll range: the first tile,
    // e.g. SWELL, became unreachable and the next was cropped).
    justifyContent: "safe center",
    gap: t.space5,
    // Right clears the added-block badge's right:-5 overhang; bottom gives the slim
    // scrollbar room. Top stays small — the stripScroll wrapper adds the badge room.
    padding: `${String(t.space1)}px ${String(t.space5)}px ${String(t.space4)}px`,
  };

  // The horizontal scroll lives on this wrapper, not the strip: an overflowX:auto box
  // coerces overflow-y to `auto` (CSS spec), which would clip the badge poking above a
  // slack-less series tile. paddingTop gives the badge room; the matching negative
  // marginTop cancels the shift so neither the Copy nor Level strip moves a pixel.
  // The two MUST stay equal (both space4) — they cancel to net zero.
  const stripScroll: React.CSSProperties = {
    overflowX: "auto",
    width: "fit-content",
    maxWidth: "100%",
    margin: `-${String(t.space4)}px auto 0`,
    paddingTop: t.space4,
  };
  if (graph.lanes && graph.lanes.length > 0) {
    return (
      <div style={stripScroll} className="tmp-block-strip">
        <div
          style={{
            ...stripStyle,
            flexDirection: "column",
            alignItems: "center",
            gap: laneGap,
          }}
        >
          {graph.lanes.map((lane, li) => (
            <div
              key={li}
              style={{ display: "flex", alignItems: "stretch", gap: t.space5 }}
            >
              <SourceNode
                type={lane.input}
                ink={ink}
                skeleton={skeleton}
                size={size}
              />
              <Wire ink={ink} />
              {lane.blocks.length > 0 ? (
                <>
                  {row(lane.blocks, `ln${String(li)}`)}
                  <Wire ink={ink} />
                </>
              ) : (
                <Wire w={28} ink={ink} />
              )}
              <SinkNode
                type={lane.output}
                ink={ink}
                skeleton={skeleton}
                size={size}
              />
            </div>
          ))}
        </div>
      </div>
    );
  }

  const kids: React.ReactNode[] = [];
  if (graph.inputs) {
    kids.push(
      <JoinHead
        key="inputs"
        inputs={graph.inputs}
        ink={ink}
        size={size}
        skeleton={skeleton}
        laneGap={laneGap}
        renderRow={row}
      />,
    );
  } else {
    kids.push(
      <SourceNode
        key="src"
        type={graph.inputType ?? "guitar"}
        ink={ink}
        skeleton={skeleton}
        size={size}
      />,
    );
  }

  graph.stages.forEach((st, i) => {
    kids.push(<Wire key={`sw${String(i)}`} ink={ink} />);
    if (st.kind === "split") {
      kids.push(splitSection(st, `st${String(i)}`));
    } else {
      st.blocks.forEach((b, j) => {
        if (j > 0)
          kids.push(<Wire key={`bw${String(i)}-${String(j)}`} ink={ink} />);
        kids.push(tile(b, `b${String(i)}-${String(j)}`));
      });
    }
  });

  if (graph.outputs) {
    kids.push(<Wire key="wfork" ink={ink} />);
    kids.push(
      <ForkTail
        key="outputs"
        outputs={graph.outputs}
        ink={ink}
        size={size}
        skeleton={skeleton}
        laneGap={laneGap}
        renderRow={row}
      />,
    );
  } else {
    kids.push(<Wire key="wout" ink={ink} />);
    kids.push(
      <SinkNode
        key="sink"
        type={graph.outputType ?? "out"}
        ink={ink}
        skeleton={skeleton}
        size={size}
      />,
    );
  }

  return (
    <div style={stripScroll} className="tmp-block-strip">
      <div style={stripStyle}>{kids}</div>
    </div>
  );
}

export const SignalChainView = React.memo(SignalChainViewImpl);

export default SignalChainView;
