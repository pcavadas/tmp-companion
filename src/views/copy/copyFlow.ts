// src/views/copy/copyFlow.ts — the Copy tab's state machine, one reducer over the
// whole flow instead of a dozen independent flags.
//
// The page is ALWAYS in exactly one phase:
//   • choose — Step 1 (ChoosePresets): pick the reference + the targets.
//   • edit   — Step 2 (PlaceBlocks): stage edits on each target. `pick` is the source
//              block chosen to copy (it stays chosen after each placement, so the next
//              target is one click); `open` is the target block whose menu is open.
//   • saving — Step 2 under the SaveOverlay: the copy_apply run and its results.
// The staged work (`Draft`: the undo/redo history + the "I've backed up" tick) rides
// every phase, so it survives Back → Choose blocks and a failed save; only a fully
// saved run clears it. `edit`/`saving` always hold a history, so Step 2 can never
// render without a working copy.
//
// Pure: no device calls, no uid minting side effects (`applyEditOp` is pure). The
// component computes anything that needs the library (`retargetHistory`'s graphs) and
// hands it in on the action.

import type { CopyApplyItem } from "../../lib/types";
import {
  applyEditOp,
  removeEditBlock,
  type EditHistory,
  type EditMap,
  type EditMode,
} from "./copyModel";

/** The target block whose menu (Replace / Remove) is open: a target slot + the block's
 *  uid. */
export interface OpenBlock {
  slot: number;
  uid: string;
}

/** The source block chosen to copy: its uid on the source strip and its model id. */
export interface PickedBlock {
  uid: string;
  model: string;
}

/** The staged work. `hist` is the snapshot stack of the whole staged edit + a pointer
 *  (the working copy is `hist.stack[hist.idx]`); it is local to the editing session and
 *  never touches the unit (the only device write is Save). */
export interface Draft {
  hist: EditHistory | null;
  backedUp: boolean;
}

export type CopyPhase =
  | { kind: "choose"; draft: Draft }
  | {
      kind: "edit";
      draft: Draft & { hist: EditHistory };
      pick: PickedBlock | null;
      open: OpenBlock | null;
    }
  | {
      kind: "saving";
      draft: Draft & { hist: EditHistory };
      /** The source block chosen when Save was pressed (still drawn under the overlay,
       *  and still chosen if some presets come back unsaved). */
      pick: PickedBlock | null;
      /** The menu left open when Save was pressed (still drawn under the overlay). */
      open: OpenBlock | null;
      /** The slots this run writes, in order. */
      slots: number[];
      /** Every result seen so far, by slot. */
      results: Map<number, CopyApplyItem>;
      /** The run settled (resolved or rejected). A rejected run is logged; what it
       *  saved is read from `results` alone. */
      done: boolean;
    };

export interface CopyState {
  fromSlot: number | null;
  toSet: Set<number>;
  phase: CopyPhase;
}

export type CopyAction =
  | { type: "setFrom"; slot: number }
  | { type: "setTo"; next: Set<number> }
  /** Enter Step 2 with the history re-targeted onto the chosen slots. */
  | { type: "enterEdit"; hist: EditHistory }
  | { type: "back" }
  | { type: "setBackedUp"; value: boolean }
  /** Choose a source block to copy; choosing the chosen one again clears it. */
  | { type: "pickBlock"; uid: string; model: string }
  /** Open (or, on the open one, close) a target block's menu. */
  | { type: "tapBlock"; slot: number; uid: string }
  | { type: "closeMenu" }
  /** Place `model` on target `slot`: replace block `uid`, or insert it before/after
   *  `uid` (a "+" gap). The source block stays chosen. */
  | { type: "apply"; slot: number; uid: string; mode: EditMode; model: string }
  | { type: "remove"; slot: number; uid: string }
  | { type: "undo" }
  | { type: "redo" }
  | { type: "saveStart"; slots: number[] }
  | { type: "saveResults"; results: Map<number, CopyApplyItem> }
  | { type: "saveSettled" }
  /** Dismiss the finished SaveOverlay. `hist` is null when every preset saved
   *  (back to Step 1, staged work discarded), else the history re-targeted with the
   *  written presets restarted from their just-saved graph. */
  | { type: "saveDismiss"; hist: EditHistory | null };

export const INITIAL_COPY_STATE: CopyState = {
  fromSlot: null,
  toSet: new Set(),
  phase: { kind: "choose", draft: { hist: null, backedUp: false } },
};

/** The working copy, when the flow has one. */
export function currentEdit(phase: CopyPhase): EditMap | null {
  const h = phase.draft.hist;
  return h ? h.stack[h.idx] : null;
}

/** Push a new edit snapshot (truncating any redo branch). */
function commit(
  hist: EditHistory,
  update: (m: EditMap) => EditMap,
): EditHistory {
  const next = update(hist.stack[hist.idx]);
  const stack = [...hist.stack.slice(0, hist.idx + 1), next];
  return { ...hist, stack, idx: stack.length - 1 };
}

/** The Step-2 transitions — they apply only while editing (not under the overlay). */
function editReducer(
  phase: Extract<CopyPhase, { kind: "edit" }>,
  action: CopyAction,
): CopyPhase {
  const { draft } = phase;
  const hist = draft.hist;
  switch (action.type) {
    case "pickBlock":
      return {
        ...phase,
        pick:
          phase.pick?.uid === action.uid
            ? null
            : { uid: action.uid, model: action.model },
      };
    case "tapBlock": {
      const o = phase.open;
      const same = o?.slot === action.slot && o.uid === action.uid;
      return {
        ...phase,
        open: same ? null : { slot: action.slot, uid: action.uid },
      };
    }
    case "closeMenu":
      return { ...phase, open: null };
    case "apply": {
      const { slot, uid, mode, model } = action;
      const next = commit(hist, (prev) => {
        const e = prev[slot];
        return e ? { ...prev, [slot]: applyEditOp(e, uid, mode, model) } : prev;
      });
      return { ...phase, draft: { ...draft, hist: next }, open: null };
    }
    case "remove": {
      const { slot, uid } = action;
      const next = commit(hist, (prev) => {
        const e = prev[slot];
        return e ? { ...prev, [slot]: removeEditBlock(e, uid) } : prev;
      });
      return { ...phase, draft: { ...draft, hist: next }, open: null };
    }
    case "undo":
      return hist.idx > 0
        ? {
            ...phase,
            draft: { ...draft, hist: { ...hist, idx: hist.idx - 1 } },
          }
        : phase;
    case "redo":
      return hist.idx < hist.stack.length - 1
        ? {
            ...phase,
            draft: { ...draft, hist: { ...hist, idx: hist.idx + 1 } },
          }
        : phase;
    case "back":
      return { kind: "choose", draft };
    case "setBackedUp":
      return { ...phase, draft: { ...draft, backedUp: action.value } };
    case "saveStart":
      return {
        kind: "saving",
        draft,
        pick: phase.pick,
        open: phase.open,
        slots: action.slots,
        results: new Map(),
        done: false,
      };
    default:
      return phase;
  }
}

/** The save-run transitions — they apply only under the SaveOverlay. */
function savingReducer(
  phase: Extract<CopyPhase, { kind: "saving" }>,
  action: CopyAction,
): CopyPhase {
  switch (action.type) {
    case "saveResults":
      return { ...phase, results: action.results };
    case "saveSettled":
      return { ...phase, done: true };
    case "saveDismiss":
      // Everything saved: the unit now matches the edit, so discard the staged work
      // and return to Step 1. Otherwise stay on Step 2 with the unsaved presets' staged
      // edits (and undo steps) intact — the backup acknowledgment stands, since those
      // presets are still as backed up.
      return action.hist == null
        ? { kind: "choose", draft: { hist: null, backedUp: false } }
        : {
            kind: "edit",
            draft: { ...phase.draft, hist: action.hist },
            pick: phase.pick,
            open: null,
          };
    default:
      return phase;
  }
}

export function copyReducer(state: CopyState, action: CopyAction): CopyState {
  switch (action.type) {
    case "setFrom":
      return { ...state, fromSlot: action.slot };
    case "setTo":
      return { ...state, toSet: action.next };
    case "enterEdit":
      // Re-entering Step 2 keeps the staged edits (and their undo steps) of every
      // target still selected; only a newly picked target starts from its library
      // graph (`retargetHistory`, run by the caller). Also from `edit`: Step 2 falls
      // back to the chooser while its reference preset is missing from the library.
      return state.phase.kind !== "saving"
        ? {
            ...state,
            phase: {
              kind: "edit",
              draft: { ...state.phase.draft, hist: action.hist },
              pick: null,
              open: null,
            },
          }
        : state;
    default:
      break;
  }
  const { phase } = state;
  const next =
    phase.kind === "edit"
      ? editReducer(phase, action)
      : phase.kind === "saving"
        ? savingReducer(phase, action)
        : phase;
  return next === phase ? state : { ...state, phase: next };
}
