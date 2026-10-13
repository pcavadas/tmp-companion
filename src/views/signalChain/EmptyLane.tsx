// src/views/signalChain/EmptyLane.tsx — the placeholder for an empty split branch.

import { STRIP_LBL, type Size } from "./stripTypes";

// An empty split branch reserves the lane's height/width but draws NO wire of
// its own: `SplitGroup` paints the straight-through as one full-width line at
// the exact measured bracket-border y (see `emptyLine`), so the branch wire and
// the bracket corners are one continuous stroke instead of two elements that
// drift apart vertically (and double up where they overlap the vertical bar).
export interface EmptyLaneProps {
  size: Size;
}

export function EmptyLane({ size }: EmptyLaneProps) {
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
