// src/views/copy/validateBlockEdit.ts — firmware-faithful pre-flight validation
// for the Copy tab's block edits.
//
// The device's `NodeSelectionRestrictions` (5 rules: a CPU cap + 4 block-count /
// coexistence caps) are enforced ONLY in the `tone-master-stomp-client` GUI layer —
// `tm-stomp-server` enforces none of them and cannot reject an over-cap edit. So
// THIS module is UX only (up-front grey-out + a save-gate warning): the checkpoint
// of record is the Rust `copy_apply` guard, which covers every writer. Getting this
// wrong just means a stale hint, not an unsafe write — but it should still match the
// firmware's own predicates so the feedback is trustworthy.
//
// `block-classification.json` is the byte-exact extraction of the 3 hardcoded id
// sets the firmware uses (see that file's `_comment` + the plan doc). Membership is
// EXACT-STRING ONLY — no `resolveDeviceId` / suffix-stripping — because the suffix
// itself is the classification signal (e.g. a `…CabIRConvRvb` reverb combo is both a
// convolution reverb AND a cabinet; its `…NoFxCabIR` sibling is a cabinet only).

import blockClassification from "../../models/block-classification.json";
import { CPU_BUDGET } from "../../models/cpu";
import {
  blockArrays,
  cpuOfGraph,
  type EditGraph,
  type PresetEdit,
} from "./copyModel";
import type { EditMode } from "./BlockEditor";

const CONV_SET = new Set<string>(blockClassification.convolutionSet);
const CABINET_SET = new Set<string>(blockClassification.cabinetSet);
const GLOOPER_ID = blockClassification.glooper;
const FX_LOOP_STEREO = blockClassification.fxLoopStereo;
const FX_LOOP_MONO = new Set<string>(blockClassification.fxLoopMono);

export type BlockEditReason =
  | "ProcessorUtilization"
  | "FXLoopCoexistence"
  | "ConvolutionReverbLimit"
  | "ComboHalfStackCabinetsLimit"
  | "GlooperEffectsLimit";

/** Short user-facing copy per reason — the save-gate warning + the greyed-chip
 *  tooltip both read from here, so the two surfaces never drift apart. */
export const REASON_COPY: Record<BlockEditReason, string> = {
  ProcessorUtilization: `Over ${String(CPU_BUDGET)}% DSP budget — remove a block`,
  FXLoopCoexistence:
    "The stereo FX loop can't coexist with a mono FX loop block",
  ConvolutionReverbLimit: "Only 1 convolution reverb per preset",
  ComboHalfStackCabinetsLimit: "Only 2 cabinets per preset",
  GlooperEffectsLimit: "Only 2 Glooper effects per preset",
};

/** Exact-string set-membership against the firmware's own hardcoded id lists. A
 *  model id not present in a set is simply not a member of that cap — this is not a
 *  fallback, it's the correct classification for every other block (incl. FX loops
 *  1/2, mics, and anything uncosted). */
export function classify(model: string): {
  convLimit: boolean;
  cabinet: boolean;
  glooper: boolean;
} {
  return {
    convLimit: CONV_SET.has(model),
    cabinet: CABINET_SET.has(model),
    glooper: model === GLOOPER_ID,
  };
}

/** The graph's current standing against each cap, walked once from `blockArrays`
 *  (pure — no `applyEditOp`, so no `newUid()` side effect). A `cabSim2Enabled`
 *  block occupies two cabinet slots (a dual-cab node). `fxLoopPresent` is the set of
 *  FX-loop ids (stereo `ACD_FxLoop3_4` / mono `ACD_FxLoop3`/`ACD_FxLoop4`) currently
 *  present anywhere in the graph — loops 1/2 are non-placeable rear-panel fixtures
 *  and never appear here since they're not in `fxLoopStereo`/`fxLoopMono`. */
export interface BaseCounts {
  conv: number;
  cabinet: number;
  glooper: number;
  fxLoopPresent: Set<string>;
}

export function baseCounts(graph: EditGraph): BaseCounts {
  let conv = 0;
  let cabinet = 0;
  let glooper = 0;
  const fxLoopPresent = new Set<string>();
  for (const arr of blockArrays(graph)) {
    for (const b of arr) {
      const c = classify(b.model);
      if (c.convLimit) conv += 1;
      if (c.cabinet) cabinet += b.cabSim2Enabled ? 2 : 1;
      if (c.glooper) glooper += 1;
      if (b.model === FX_LOOP_STEREO || FX_LOOP_MONO.has(b.model)) {
        fxLoopPresent.add(b.model);
      }
    }
  }
  return { conv, cabinet, glooper, fxLoopPresent };
}

function countReason(counts: {
  conv: number;
  cabinet: number;
  glooper: number;
}): BlockEditReason | null {
  if (counts.conv >= 2) return "ConvolutionReverbLimit";
  if (counts.cabinet >= 3) return "ComboHalfStackCabinetsLimit";
  if (counts.glooper >= 3) return "GlooperEffectsLimit";
  return null;
}

function fxLoopReason(present: Set<string>): BlockEditReason | null {
  const stereo = present.has(FX_LOOP_STEREO);
  const mono = blockClassification.fxLoopMono.some((id) => present.has(id));
  return stereo && mono ? "FXLoopCoexistence" : null;
}

/** The anchor being replaced — only meaningful (and only read) when `mode ===
 *  "replace"`: its model frees its own count/coexistence slot, and its dual-cab
 *  flag is what `applyEditOp` carries onto the replacement (`copyModel.ts:454`). */
export interface CheckOpOpts {
  anchor?: { model: string; dualCab?: boolean };
}

/** The per-op guard behind the grey-out: would placing `candidateModel` via `mode`
 *  (against a graph currently at `counts`) violate a firmware cap? Mode-aware:
 *  `replace` frees the anchor's own slot (count + coexistence); `before`/`after`
 *  never do (a fresh insert is never a dual-cab node — `applyEditOp`'s insert
 *  branch never sets `cabSim2Enabled`). Returns the first violated reason, or
 *  `null` when the placement is fine. */
export function checkOp(
  counts: BaseCounts,
  candidateModel: string,
  mode: EditMode,
  opts: CheckOpOpts = {},
): BlockEditReason | null {
  const cand = classify(candidateModel);
  const anchor = mode === "replace" ? opts.anchor : undefined;
  const anchorClass = anchor ? classify(anchor.model) : null;

  // FX-loop coexistence: fold the anchor's freed slot + the candidate into the
  // present set, then test both directions at once.
  const present = new Set(counts.fxLoopPresent);
  if (anchor) present.delete(anchor.model);
  present.add(candidateModel);
  const fx = fxLoopReason(present);
  if (fx) return fx;

  // Count caps: candidate adds, a replaced anchor frees its own contribution.
  const conv =
    counts.conv + (cand.convLimit ? 1 : 0) - (anchorClass?.convLimit ? 1 : 0);
  const glooper =
    counts.glooper + (cand.glooper ? 1 : 0) - (anchorClass?.glooper ? 1 : 0);
  const candCabinet = cand.cabinet ? (anchor?.dualCab ? 2 : 1) : 0;
  const anchorCabinet = anchorClass?.cabinet ? (anchor?.dualCab ? 2 : 1) : 0;
  const cabinet = counts.cabinet + candCabinet - anchorCabinet;

  return countReason({ conv, cabinet, glooper });
}

/** The save-gate: does the FINAL edited graph violate a firmware cap? Applies the
 *  SAME predicates `checkOp` uses (so the two never disagree), plus the CPU cap
 *  (`cpuOfGraph`, NOT `presetCpu` — this is the staged edit, not a live read).
 *  Returns the first violated reason in firmware rule order, or `null`. */
export function checkEdit(edit: PresetEdit): BlockEditReason | null {
  if (cpuOfGraph(edit.graph) > CPU_BUDGET) return "ProcessorUtilization";
  const counts = baseCounts(edit.graph);
  return fxLoopReason(counts.fxLoopPresent) ?? countReason(counts);
}
