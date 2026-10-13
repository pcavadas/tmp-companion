// src/views/catalog/ModelTile.tsx — one catalog model's illustration through the shared
// BlockArt engine (identical to the signal-chain strip by construction).

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
  size: number;
}

export function ModelTile({ r, size }: ModelTileProps) {
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
