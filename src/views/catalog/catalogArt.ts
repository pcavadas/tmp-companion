// src/views/catalog/catalogArt.ts — the Models tab's shared art helpers: how a catalog
// record resolves its block art, the icon drop shadow, and the chassis luminance the
// detail bar's swatch wash keys on.

import { resolveBlockArt, resolveBlockArtByName } from "../../models/blockArt";
import type { ModelRecord } from "../../models/catalog";

export const ICON_SHADOW = "drop-shadow(0 2px 4px rgba(15,17,21,0.2))";

// Relative luminance (Rec. 709) of a #hex color, 0 (black) … 1 (white). Drives
// the bottom-ribbon icon box's white-overlay alpha so dark chassis are lightened
// to a soft grey while light/vivid chassis keep their true color.
export function hexLum(hex: string): number {
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

// Resolve a record's block art the way every icon does: most rows resolve by
// FenderId; the 7 Microphones carry no block_id (they're cab parameters, not DSP
// blocks), so they resolve by NAME — without which they'd fall through to the
// generic "knobs2" pedal icon. Shared by ModelTile (the icon) and DetailBar
// (the ribbon chassis tint) so the two can't drift.
export function artFor(r: ModelRecord) {
  return r.bid ? resolveBlockArt(r.bid) : resolveBlockArtByName(r.name);
}
