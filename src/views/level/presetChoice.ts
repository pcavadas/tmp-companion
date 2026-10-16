// src/views/level/presetChoice.ts — what a preset's tick on the list stands for, and
// whether it reads as chosen. Shared by the preset row, the "Select all N shown" box and
// the selection hook, so the three can never disagree.
//
// A preset's tick stands for every one of its sounds once the backup scan has read its
// scenes and footswitches (`childKeys`). When the scan finished WITHOUT them (it failed,
// or it missed this preset) the tick stands for its main sound only. While the scan is
// still reading, nothing is known yet: a tick then waits in `pendingWhole` and is folded
// into real sound keys when the scan settles (`usePresetData`).

import { baseKey, childKeys } from "./leveling";
import type { FootswitchInfo, SceneInfo } from "../../lib/types";

/** How much of a preset is chosen. */
export type PresetChoice = "on" | "some" | "off";

/** The sound keys a preset's tick stands for, or `null` while the scan is still reading
 *  its scenes (the tick then waits in `pendingWhole`). */
export function presetKeys(
  slot: number,
  scenes: SceneInfo[] | undefined,
  footswitches: FootswitchInfo[] | undefined,
  ready: boolean,
): string[] | null {
  if (scenes !== undefined) return childKeys(slot, scenes, footswitches ?? []);
  return ready ? [baseKey(slot)] : null;
}

/** Whether a preset reads as chosen, from its keys (`presetKeys`) and the selection. A
 *  preset still being read is chosen when it waits in `pendingWhole`, or when its main
 *  sound is already in the selection (a tick from before a re-read). */
export function presetChoice(
  slot: number,
  keys: string[] | null,
  sel: Set<string>,
  pendingWhole: boolean,
): PresetChoice {
  if (keys === null)
    return pendingWhole || sel.has(baseKey(slot)) ? "on" : "off";
  const n = keys.reduce((a, k) => a + (sel.has(k) ? 1 : 0), 0);
  if (n === 0) return "off";
  return n === keys.length ? "on" : "some";
}

/** The select-all box's state over a set of presets: on when every one is on, off when
 *  every one is off, mixed otherwise (and off when there are none). */
export function combineChoices(choices: PresetChoice[]): PresetChoice {
  if (choices.length === 0) return "off";
  if (choices.every((c) => c === "on")) return "on";
  if (choices.every((c) => c === "off")) return "off";
  return "some";
}
