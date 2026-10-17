// src/views/signalChain/useLaneBracket.ts — the measured bracket geometry for any
// split/join point (one or two diamonds, two lane rows).
//
// The bracket is DOM-measured from the two lane rows' real rendered centres rather than
// derived from hardcoded tile-height constants (the old per-lane SVG curve math assumed
// both lanes were exactly the same fixed height, which left a visible gap once a
// gtrSplit/micSplit preset had a different block count per output lane). Used by
// SplitGroup (two-sided, in-line splits like gtrParallel1/2) and ForkTail/JoinHead
// (one-sided, split-output/dual-input lanes) alike.
//
// WHEN it re-measures (#313): after EVERY commit of the owning component, and whenever
// the column or either lane row resizes. A lane's height moves with more than its block
// COUNT — a label, the skeleton→real swap, a half-stack tile, a late web-font load — so
// keying the measurement on the count alone (the old `${aLen}|${bLen}` signature) left
// a stale bracket after any same-count change. Re-measuring every commit is safe: the
// measurement converges (an unchanged rect keeps the SAME state object, so React bails
// out of the re-render), and it is three `getBoundingClientRect` reads. The
// ResizeObserver catches the changes that re-render nothing (a web font landing, a
// parent reflow); it is absent under jsdom, where the per-commit measure still runs.

import { useLayoutEffect, useRef, useState, type RefObject } from "react";

import { STRIP_LBL } from "./stripTypes";

/** A bracket's vertical span inside its lane column: from lane A's art centre down
 *  to lane B's. */
export interface LaneBracket {
  top: number;
  height: number;
}

/** Measure the bracket from the committed DOM, or null while any node is missing or
 *  not laid out yet. The shared MATH only — not a hook itself. */
function measureLaneBracket(
  colRef: RefObject<HTMLDivElement | null>,
  aRef: RefObject<HTMLDivElement | null>,
  bRef: RefObject<HTMLDivElement | null>,
): LaneBracket | null {
  const col = colRef.current,
    ar = aRef.current,
    br = bRef.current;
  if (!col || !ar || !br) return null;
  const ct = col.getBoundingClientRect();
  const arr = ar.getBoundingClientRect();
  const brr = br.getBoundingClientRect();
  if (!arr.height || !brr.height) return null;
  // getBoundingClientRect reads the SCREEN box, which an ancestor's CSS scale shrinks
  // (the Level list fits a long chain to the window with a transform), while the
  // bracket is placed in the column's own layout px. Divide the scale back out, or a
  // scaled diagram draws its bracket short of the lower lane.
  const scale =
    col.offsetHeight > 0 && ct.height > 0 ? ct.height / col.offsetHeight : 1;
  const aC = (arr.top - ct.top) / scale + (arr.height / scale - STRIP_LBL) / 2;
  const bC = (brr.top - ct.top) / scale + (brr.height / scale - STRIP_LBL) / 2;
  return { top: aC, height: Math.max(0, bC - aC) };
}

/** Sub-pixel jitter is not a change: keep the previous object so the state converges. */
function settle(prev: LaneBracket | null, next: LaneBracket): LaneBracket {
  return prev &&
    Math.abs(prev.top - next.top) < 0.5 &&
    Math.abs(prev.height - next.height) < 0.5
    ? prev
    : next;
}

// Refs are created HERE (inside the hook), not passed in as parameters, so eslint's
// exhaustive-deps still sees them as useRef-stable. The ONE layout effect measures the
// committed DOM and then sets state — the documented DOM-measurement exception to
// `set-state-in-effect`, guarded by `settle` so it converges.
export function useLaneBracket() {
  const colRef = useRef<HTMLDivElement>(null);
  const aRef = useRef<HTMLDivElement>(null);
  const bRef = useRef<HTMLDivElement>(null);
  const [brk, setBrk] = useState<LaneBracket | null>(null);
  useLayoutEffect(() => {
    const remeasure = () => {
      const next = measureLaneBracket(colRef, aRef, bRef);
      if (next) setBrk((p) => settle(p, next));
    };
    remeasure();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(remeasure);
    for (const el of [colRef.current, aRef.current, bRef.current]) {
      if (el) ro.observe(el);
    }
    return () => {
      ro.disconnect();
    };
  });
  return { colRef, aRef, bRef, brk };
}
