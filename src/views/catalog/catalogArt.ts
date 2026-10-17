// src/views/catalog/catalogArt.ts — the Catalog's shared art helpers: how a catalog
// record resolves its block art, and the icon drop shadow.

import { resolveBlockArt, resolveBlockArtByName } from "../../models/blockArt";
import type { ModelRecord } from "../../models/catalog";

export const ICON_SHADOW = "drop-shadow(0 2px 4px rgba(15,17,21,0.2))";

// Resolve a record's block art the way every icon does: most rows resolve by
// FenderId; the 7 Microphones carry no block_id (they're cab parameters, not DSP
// blocks), so they resolve by NAME — without which they'd fall through to the
// generic "knobs2" pedal icon. Shared by ModelTile's single and half-stack art.
export function artFor(r: ModelRecord) {
  return r.bid ? resolveBlockArt(r.bid) : resolveBlockArtByName(r.name);
}
