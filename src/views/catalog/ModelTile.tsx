// src/views/catalog/ModelTile.tsx — one catalog model's illustration through the shared
// BlockArt engine (identical to the signal-chain strip by construction), centred in a
// `size` × `size` box (44 on a tile, 72 in the detail).

import { BlockArt, HalfStackArt } from "../../ui/BlockArt";
import {
  resolveBlockArt,
  HALF_STACK_PAIR,
  HALF_STACK_CAB_TONE,
} from "../../models/blockArt";
import type { ModelRecord } from "../../models/catalog";
import { artFor, ICON_SHADOW } from "./catalogArt";

// Per-model illustration via the shared BlockArt engine (identical to the
// signal-chain strip by construction). Icon + chassis tone resolve BY ID from
// the block-art catalog; half-stacks stack the head on its paired cab. (The old
// `public/tmp_blocks/` PNG photos were Fender's copyright and are no longer used.)
export interface ModelTileProps {
  r: ModelRecord;
  /** The square box the art sits in (px). */
  size: number;
}

/** A half stack (head over cab) is about 1.45× taller than its width, so it is drawn
 *  at this share of the box to fit its height. */
const HALF_STACK_SHARE = 0.65;

export function ModelTile({ r, size }: ModelTileProps) {
  return (
    <span
      aria-hidden
      style={{
        width: size,
        height: size,
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      <TileArt r={r} size={size} />
    </span>
  );
}

function TileArt({ r, size }: ModelTileProps) {
  const art = artFor(r);
  // Per-model art treatments (EVH channel colour, the Seventy Sixer face, …) ride
  // the explicit `variant`; the caption is display text only.
  const lab = art?.short ?? "";
  if (r.form === "half_stack") {
    const cabBid = r.bid ? HALF_STACK_PAIR[r.bid] : undefined;
    const cabArt = cabBid ? resolveBlockArt(cabBid) : null;
    return (
      <HalfStackArt
        topIcon={art?.icon}
        topTone={art?.tone}
        topVariant={art?.variant}
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
        cabW={Math.round(size * HALF_STACK_SHARE)}
      />
    );
  }
  return (
    <div style={{ filter: ICON_SHADOW, display: "flex" }}>
      <BlockArt
        icon={art?.icon}
        tone={art?.tone}
        lab={lab}
        variant={art?.variant}
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
