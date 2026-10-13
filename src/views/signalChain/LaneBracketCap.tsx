// src/views/signalChain/LaneBracketCap.tsx — one side's "[" / "]" cap of a split or
// join bracket, sized from a `useLaneBracket` measurement.

import { STROKE_W_PX, type BracketSide } from "./stripTypes";
import type { LaneBracket } from "./useLaneBracket";

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
export interface LaneBracketCapProps {
  brk: LaneBracket | null;
  side: BracketSide;
  ink: string;
}

export function LaneBracketCap({ brk, side, ink }: LaneBracketCapProps) {
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
