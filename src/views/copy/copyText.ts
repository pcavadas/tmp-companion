// src/views/copy/copyText.ts — every word the Copy tab draws from its state (look B), and
// the block names it uses, in one place so the views and the tests read the same strings.
//
// Block names: the name your Tone Master Pro shows for the block (`blockStripName`, the
// one Level's Set up uses), and for a cab block the name of its cabinet, as its tile does.

import { cabArtModel } from "../../models/blockArt";
import { CPU_BUDGET } from "../../models/cpu";
import { blockStripName } from "../level/leveling";
import type { BlockEditReason } from "./validateBlockEdit";

/** The name a block goes by on screen. */
export function blockName(model: string, cabSimId?: string): string {
  return blockStripName(cabArtModel(cabSimId, model));
}

const presets = (n: number): string =>
  `${String(n)} ${n === 1 ? "preset" : "presets"}`;

/** Why a target can't be saved yet (also the save gate's line for one blocked target). */
export const REASON_TEXT: Record<BlockEditReason, string> = {
  ProcessorUtilization: `Uses more than ${String(CPU_BUDGET)}% CPU. Remove a block.`,
  FXLoopCoexistence:
    "The stereo FX loop can't share a preset with a mono FX loop.",
  ConvolutionReverbLimit: "Only one convolution reverb fits in a preset.",
  ComboHalfStackCabinetsLimit: "Only two cabinets fit in a preset.",
  GlooperEffectsLimit: "Only two Glooper effects fit in a preset.",
};

export const COPY_TEXT = {
  title: "Copy blocks between presets",
  from: "Copy from",
  into: "Copy into",
  search: "Search",
  searchFrom: "Search presets to copy from",
  searchInto: "Search presets to copy into",
  readingPresets: "Reading presets",
  loaded: "Loaded",
  clearSearch: "Clear search",
  noOther: "No other presets to copy into",
  noPresets: "No presets to copy from",
  chooseInto: "Choose at least one preset to copy into",
  chooseBlocks: "Choose blocks",
  chooseBlock: "Choose a block to copy.",
  changed: "Changed",
  cpu: "CPU",
  showOnly: "Show only these",
  showAll: "Show all",
  back: "Back",
  undo: "Undo",
  redo: "Redo",
  addFirst: "Add a block to a preset first",
  save: "Save to your Tone Master Pro",
  remove: "Remove",
  saving: "Saving to your Tone Master Pro",
  keepConnected: "Keep your Tone Master Pro connected.",
  done: "Done",
  allSaved: "Saved to your Tone Master Pro",
  allSavedBody: "Restore your Pro Control backup to undo this.",
  someSaved: "Some presets weren't saved",
  noneSaved: "Nothing was saved",
  noneSavedBody: "Your changes are kept, so you can try again.",
  rowWaiting: "Waiting",
  rowSaving: "Saving",
  rowSaved: "Saved",
  rowNoChange: "No change",
  rowFailed: "Couldn't save",
} as const;

export const noMatch = (q: string): string => `No presets match "${q}"`;
export const selectAllShown = (n: number): string =>
  `Select all ${String(n)} shown`;
export const copyFromName = (name: string): string => `Copy from ${name}`;
export const copyIntoName = (name: string): string => `Copy into ${name}`;
export const fromInto = (from: string, n: number): string =>
  `From ${from} into ${presets(n)}`;
export const readingPercent = (pct: number): string =>
  `Reading presets ${String(Math.round(pct))}%`;

export const readyToCopy = (name: string): string =>
  `${name} is ready to copy.`;
/** The chosen block's CPU cost, one decimal. */
export const addsCpu = (cpu: number): string => `Adds ${cpu.toFixed(1)}% CPU`;
export const copyInto = (n: number): string => `Copy into ${presets(n)}`;
export const cantSaveYet = (n: number): string =>
  `${String(n)} can't be saved yet`;
export const changedOf = (changed: number, total: number): string =>
  `${String(changed)} of ${presets(total)} changed`;
export const replaceWith = (name: string): string => `Replace with ${name}`;

/** Where a "+" gap places the block, as the gap's accessible name. */
export function gapLabel(
  preset: string,
  at: { before: string } | { after: string } | "end",
): string {
  const where =
    at === "end"
      ? "at the end"
      : "before" in at
        ? `before ${at.before}`
        : `after ${at.after}`;
  return `Add here in ${preset}, ${where}`;
}

/** Why the chosen block can't go anywhere in a target (its "+" gaps are greyed). */
export function gapReason(reason: BlockEditReason, block: string): string {
  return reason === "ProcessorUtilization"
    ? `There isn't enough CPU left for ${block}.`
    : REASON_TEXT[reason];
}

/** Why Save is greyed, or null when only the backup tick (or nothing) holds it. */
export function saveReason(
  blocked: readonly { name: string; reason: BlockEditReason }[],
  changed: number,
): string | null {
  if (blocked.length > 1)
    return `${presets(blocked.length)} can't be saved yet`;
  if (blocked.length === 1)
    return `${blocked[0].name}: ${REASON_TEXT[blocked[0].reason]}`;
  return changed === 0 ? COPY_TEXT.addFirst : null;
}

// ── saving ───────────────────────────────────────────────────────────────────

export const savingPresets = (n: number): string => `Saving ${presets(n)}`;

/** The ticked targets with nothing to save, named once ("Tweed Warm is unchanged…"). */
export function unchangedLine(names: readonly string[]): string | null {
  if (names.length === 0) return null;
  return names.length === 1
    ? `${names[0]} is unchanged, so it isn't saved.`
    : `${presets(names.length)} are unchanged, so they aren't saved.`;
}

/** The body of a save that held for some presets but not all. */
export function someSavedBody(saved: number, unsaved: number): string {
  const keeps =
    unsaved === 1 ? "and it keeps your changes" : "and they keep your changes";
  return `${String(saved)} saved. ${String(unsaved)} couldn't be saved, ${keeps} so you can try again. Restore your Pro Control backup to undo the ones that saved.`;
}
