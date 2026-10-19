// src/views/copy/validateBlockEdit.ts — firmware-faithful pre-flight validation
// for the Copy tab's block edits.
//
// The firmware's `NodeSelectionRestrictions` (5 rules: a CPU cap + 4 block-count /
// coexistence caps): `tm-stomp-server` enforces none of the 4 count caps, and its CPU
// check is fatal (an over-budget insert aborts the server). So THIS module is UX only
// (up-front grey-out + a save-gate warning): the checkpoint of record is the Rust
// `blockcaps` guard on every apply path. Getting this
// wrong just means a stale hint, not an unsafe write — but it should still match the
// firmware's own predicates so the feedback is trustworthy. The words for each reason are
// in ./copyText (`REASON_TEXT`).
//
// `block-classification.json` holds the fw 1.8.58 client's id sets (see that file's
// `_comment`). Membership is
// EXACT-STRING ONLY — no `resolveDeviceId` / suffix-stripping — because the suffix
// itself is the classification signal (e.g. a `…CabIRConvRvb` reverb combo is both a
// convolution reverb AND a cabinet; its `…NoFxCabIR` sibling is a cabinet only).

import blockClassification from "../../models/block-classification.json";
import { chargedCpu, overCpuBudget } from "../../models/cpu";
import {
  blockArrays,
  cpuOfGraph,
  type EditGraph,
  type EditMode,
  type PresetEdit,
} from "./copyModel";

const CONV_SET = new Set<string>(blockClassification.convolutionSet);
const CABINET_SET = new Set<string>(blockClassification.cabinetSet);
const CABINET_EXEMPT = new Set<string>(blockClassification.cabinetExempt);
const GLOOPER_ID = blockClassification.glooper;
const FX_LOOPS = new Set<string>(blockClassification.fxLoops);
const FX_LOOP_STEREO = blockClassification.fxLoopStereo;
const FX_LOOP_MONO = new Set<string>(blockClassification.fxLoopMono);

export type BlockEditReason =
  | "ProcessorUtilization"
  | "FXLoopCoexistence"
  | "ConvolutionReverbLimit"
  | "ComboHalfStackCabinetsLimit"
  | "GlooperEffectsLimit";

/** One target that can't be saved as it stands, and why. */
export interface BlockedSlot {
  slot: number;
  reason: BlockEditReason;
}

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
 *  (no `applyEditOp` clone per check). A cabinet member with `secondCabSlot` (an amp's
 *  second mic or a CabSim's second cab) occupies two cabinet slots. `fxLoops` counts
 *  each FX-loop id present anywhere in the graph. */
export interface BaseCounts {
  cpu: number;
  conv: number;
  cabinet: number;
  glooper: number;
  fxLoops: Map<string, number>;
}

export function baseCounts(graph: EditGraph): BaseCounts {
  let conv = 0;
  let cabinet = 0;
  let glooper = 0;
  const fxLoops = new Map<string, number>();
  const cpu = cpuOfGraph(graph);
  for (const arr of blockArrays(graph)) {
    for (const b of arr) {
      const c = classify(b.model);
      if (c.convLimit) conv += 1;
      if (c.cabinet) cabinet += b.secondCabSlot ? 2 : 1;
      if (c.glooper) glooper += 1;
      if (FX_LOOPS.has(b.model)) {
        fxLoops.set(b.model, (fxLoops.get(b.model) ?? 0) + 1);
      }
    }
  }
  return { cpu, conv, cabinet, glooper, fxLoops };
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

/** Whether FX loop `id` clashes with the loops in `present`: the same loop again, or
 *  the stereo `ACD_FxLoop3_4` against either mono loop (both directions). */
function fxLoopClash(id: string, present: Map<string, number>): boolean {
  const has = (x: string) => (present.get(x) ?? 0) > 0;
  if (has(id)) return true;
  if (id === FX_LOOP_STEREO) return [...FX_LOOP_MONO].some(has);
  return FX_LOOP_MONO.has(id) && has(FX_LOOP_STEREO);
}

/** The save-gate's FX-loop rule over a whole graph: any loop that clashes with the
 *  others. */
function fxLoopReason(present: Map<string, number>): BlockEditReason | null {
  for (const id of present.keys()) {
    const others = new Map(present);
    others.set(id, (others.get(id) ?? 0) - 1);
    if (fxLoopClash(id, others)) return "FXLoopCoexistence";
  }
  return null;
}

/** The anchor being replaced — only meaningful (and only read) when `mode ===
 *  "replace"`: its model frees its own count/coexistence slot, and `dualCab` (its
 *  second cab slot) frees two cabinet slots when it is a cabinet member. */
export interface CheckOpOpts {
  anchor?: { model: string; dualCab?: boolean };
}

/** The per-op guard behind the grey-out: would placing `candidateModel` via `mode`
 *  (against a graph currently at `counts`) violate a firmware cap? Mirrors the Rust
 *  `blockcaps::check_op` (fw 1.8.58 client predicates): `replace` frees the anchor's
 *  own slot (CPU, counts, FX loop) and puts a fresh default-param instance in its place,
 *  one cabinet slot whatever the anchor had; `before`/`after` never free one. A
 *  candidate a rule doesn't cover passes it, except that a cabinet count already over
 *  the cap refuses every candidate but an exempt one (`ACD_ExternalCab`). Returns the
 *  first violated reason, or `null` when the placement is fine. */
export function checkOp(
  counts: BaseCounts,
  candidateModel: string,
  mode: EditMode,
  opts: CheckOpOpts = {},
): BlockEditReason | null {
  const cand = classify(candidateModel);
  const anchor = mode === "replace" ? opts.anchor : undefined;
  const anchorClass = anchor ? classify(anchor.model) : null;

  const cpu =
    counts.cpu +
    chargedCpu(candidateModel) -
    (anchor ? chargedCpu(anchor.model) : 0);
  if (overCpuBudget(cpu)) return "ProcessorUtilization";

  if (FX_LOOPS.has(candidateModel)) {
    const others = new Map(counts.fxLoops);
    if (anchor && others.has(anchor.model)) {
      others.set(anchor.model, (others.get(anchor.model) ?? 0) - 1);
    }
    if (fxLoopClash(candidateModel, others)) return "FXLoopCoexistence";
  }

  // The convolution cap counts presence, not members.
  const conv =
    (cand.convLimit ? 1 : 0) -
    (anchorClass?.convLimit ? 1 : 0) +
    (counts.conv > 0 ? 1 : 0);
  if (conv > 1) return "ConvolutionReverbLimit";

  if (!CABINET_EXEMPT.has(candidateModel)) {
    // A stock replacement is a fresh default-param instance: one slot, whatever the anchor
    // had. A saved dual-cab block would weigh 2, but its metadata does not say so; the Rust
    // guard shares that gap (`bulk_replace::blockcaps_check`).
    const candCabinet = cand.cabinet ? 1 : 0;
    const anchorCabinet = anchorClass?.cabinet ? (anchor?.dualCab ? 2 : 1) : 0;
    if (counts.cabinet + candCabinet - anchorCabinet > 2) {
      return "ComboHalfStackCabinetsLimit";
    }
  }

  const glooper =
    counts.glooper + (cand.glooper ? 1 : 0) - (anchorClass?.glooper ? 1 : 0);
  if (cand.glooper && glooper > 2) return "GlooperEffectsLimit";
  return null;
}

/** The save-gate: does the FINAL edited graph violate a firmware cap? Applies the
 *  SAME predicates `checkOp` uses (so the two never disagree), plus the CPU cap
 *  (`cpuOfGraph`, NOT `presetCpu` — this is the staged edit, not a live read).
 *  Returns the first violated reason in firmware rule order, or `null`. */
export function checkEdit(edit: PresetEdit): BlockEditReason | null {
  const counts = baseCounts(edit.graph);
  if (overCpuBudget(counts.cpu)) return "ProcessorUtilization";
  return fxLoopReason(counts.fxLoops) ?? countReason(counts);
}
