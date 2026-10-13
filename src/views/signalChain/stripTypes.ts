// src/views/signalChain/stripTypes.ts — the signal-chain strip's data model (the
// `StripGraph` every strip renders) plus the geometry constants its pieces share.

import type { ArtVariant, IconId, ToneId } from "../../ui/BlockArt";
import type { HalfStackSpec } from "../../models/blockArt";

/** The label zone under every strip column (tile, endpoint, diamond, wire). */
export const STRIP_LBL = 18;
/** Shared bracket/diamond/emptyLine border width — the SPLIT/MIX geometry comments
 *  below rely on these strokes lining up pixel-for-pixel, so they share one constant. */
export const STROKE_W = 1.5;
export const STROKE_W_PX = `${String(STROKE_W)}px`;

/** How a tile was changed by an edit (Copy feature). `added` → a `+` badge (inserted
 *  block); `replaced` → a `refresh` ⟳ badge (in-place model swap). Drives the badge
 *  icon + the terracotta "changed" styling. */
export type TileChange = "added" | "replaced";

export interface StripBlock {
  icon?: IconId;
  tone?: ToneId;
  /** ref-derived per-block body color (pedals) — overlays the tone default. */
  body?: string;
  /** ref-derived control-panel color behind the knobs/sliders (EQ/filter pedals). */
  panel?: string;
  /** stompbox footswitch style (pedal-form blocks) — plate/metal/round. */
  footswitch?: "plate" | "metal" | "round";
  /** Fender reverb cream-chassis accent (footswitch-band colour); the 8 reverbs. */
  accent?: string;
  /** explicit per-model art treatment (= art.variant). */
  variant?: ArtVariant;
  /** terse caption (= art.short). */
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
  inputType?: SourceType;
  outputType?: SinkType;
  inputs?: { a: StripInputLane; b: StripInputLane } | null;
  outputs?: { a: StripOutputLane; b: StripOutputLane } | null;
  lanes?: StripIndependentLane[] | null;
  stages: StripStage[];
}

export type Size = "md" | "sm";
export type BracketSide = "left" | "right";
