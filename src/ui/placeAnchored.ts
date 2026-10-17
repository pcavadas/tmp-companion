// src/ui/placeAnchored.ts — the ONE placement rule for an anchored floating list (the DS
// `Menu`, and so every `Select`'s list): clamp it into the frame, drop it below its anchor,
// and flip it above when it would overflow the frame's bottom edge.

/** Where the anchor sits, and the frame the list must stay inside (px). */
export interface AnchorBox {
  left: number;
  /** The y just under the anchor (the list's top when it opens below). */
  below: number;
  /** The anchor's own top (the list's bottom when it flips above). */
  above: number;
  width: number;
  cardW: number;
  cardH: number;
}

/** Clamp `left` into the frame with an 8px margin, drop BELOW the anchor, and flip ABOVE
 *  it (`gap` px clear) when the `mw`×`mh` list would overflow the frame's bottom edge. */
export function placeAnchored(
  anchor: AnchorBox,
  mw: number,
  mh: number,
  gap = 4,
): { left: number; top: number } {
  const left = Math.min(
    Math.max(8, anchor.left),
    Math.max(8, anchor.cardW - mw - 8),
  );
  let top = anchor.below;
  if (top + mh > anchor.cardH - 8) top = Math.max(8, anchor.above - mh - gap);
  return { left, top };
}
