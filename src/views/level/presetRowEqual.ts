// src/views/level/presetRowEqual.ts — the `memo` equality for `PresetRow` (#315).
//
// PresetList hands every row the list-wide selection Set, and each tick builds a fresh
// one, so a plain shallow `memo` would re-render all ~500 rows per click. This compares
// every prop by identity EXCEPT `sel`, which it compares only on the row's OWN keys.
// Kept out of PresetRow.tsx so that file exports components only (fast refresh).

import { baseKey, fswKey, sceneKeyOf } from "./leveling";
import type { FootswitchInfo, SceneInfo } from "../../lib/types";

/** Every selection key this row reads — its Base, each scene, and EVERY footswitch
 *  (a non-levelable switch still renders a row whose tick reads `sel`, even though
 *  `childKeys` leaves it out of the selectable set). */
function rowSelKeys(
  slot: number,
  scenes: SceneInfo[] | undefined,
  footswitches: FootswitchInfo[] | undefined,
): string[] {
  return [
    baseKey(slot),
    ...(scenes ?? []).map((_, i) => sceneKeyOf(slot, i)),
    ...(footswitches ?? []).map((_, i) => fswKey(slot, i)),
  ];
}

/** The props the equality reads by name. Kept structural (not `PresetRowProps`) so this
 *  pure module never imports the component that imports it. */
export interface RowSelProps {
  row: { slot: number };
  scenes: SceneInfo[] | undefined;
  footswitches: FootswitchInfo[] | undefined;
  sel: Set<string>;
}

/** `memo` equality for a row: every prop by identity EXCEPT `sel`, the list-wide
 *  selection Set (a fresh Set on every toggle), which compares only by membership of
 *  this row's own keys. So one tick re-renders the one row it touched, not all ~500. */
export function presetRowPropsEqual<P extends RowSelProps>(
  prev: P,
  next: P,
): boolean {
  const keys = Object.keys(next) as (keyof P)[];
  if (Object.keys(prev).length !== keys.length) return false;
  for (const k of keys) {
    if (k !== "sel" && !Object.is(prev[k], next[k])) return false;
  }
  if (prev.sel === next.sel) return true;
  return rowSelKeys(next.row.slot, next.scenes, next.footswitches).every(
    (k) => prev.sel.has(k) === next.sel.has(k),
  );
}
