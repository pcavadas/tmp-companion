// src/views/level/useGroupOpen.ts — the per-preset-group auto-open/manual-override
// hook behind the redesigned Level wizard's grouped list (design handoff 1a).
//
// A preset row auto-opens only when it "earns it" — the first row in Set up, the
// active preset in Level, any preset that came back with a problem in Summary
// (`autoSlots`, recomputed by the caller each render). The user can expand/collapse
// any row on top of that: a manual toggle wins over the auto-open for that row.
// `isOpen(slot) = manual.has(slot) || (auto.has(slot) && !closed.has(slot))` — an
// auto-opened row the user manually CLOSES stays closed even while it's still in
// `autoSlots` (`closed`); a row the user manually OPENS stays open even once it
// leaves `autoSlots` (`manual`), until the user closes it again (which also clears
// the manual override, so a later auto-open can re-open it).

import { useState } from "react";

export function useGroupOpen(
  autoSlots: number[],
): [(slot: number) => boolean, (slot: number) => void] {
  const [manual, setManual] = useState<Set<number>>(() => new Set());
  const [closed, setClosed] = useState<Set<number>>(() => new Set());
  // A plain Set built fresh each render — `autoSlots` is a handful of preset slots at
  // most, so there's no memoization win worth a manual (and lint-unfriendly) dep-array
  // override.
  const auto = new Set(autoSlots);

  const isOpen = (slot: number): boolean =>
    manual.has(slot) || (auto.has(slot) && !closed.has(slot));

  const toggle = (slot: number) => {
    if (isOpen(slot)) {
      if (auto.has(slot)) {
        setClosed((prev) => new Set(prev).add(slot));
      }
      setManual((prev) => {
        const next = new Set(prev);
        next.delete(slot);
        return next;
      });
    } else {
      setClosed((prev) => {
        const next = new Set(prev);
        next.delete(slot);
        return next;
      });
      setManual((prev) => new Set(prev).add(slot));
    }
  };

  return [isOpen, toggle];
}
