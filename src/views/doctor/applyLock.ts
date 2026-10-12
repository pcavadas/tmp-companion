// src/views/doctor/applyLock.ts — every prescription card's apply/save lifecycle,
// owned by the results PAGE, plus the "one applied-but-unsaved fix at a time" lock.
//
// The DEVICE has a single edit buffer, so EVERY PrescriptionCard on the results
// page — even ones in different presets — targets the same live, unsaved state:
// a second card's apply/discard would clobber the first card's edit while it
// still shows "Applied · not saved" with stale A/B clips. So at most one card
// may be past `idle` and short of `saved` at a time (the lock holder).
//
// The lifecycle lives here, keyed by a STABLE card id, and not in the card: a
// card remounts on every row collapse/reopen, and a card-local "saved" would be
// forgotten — offering "Apply" again over a preset that already carries the fix
// (a chain fix would insert its block a second time). Backed by a ref so the
// transitions are race-safe between a card's async continuation and a
// concurrent unmount cleanup; a version counter re-renders the page.
//
// A dedicated module (not DoctorResults) so PrescriptionCard can import it
// without a cycle.

import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
} from "react";

import { doctorDiscard } from "../../lib/invoke";
import type { DoctorApplyResult, DoctorRx } from "../../lib/types";

/** What an applied (or saving) card holds: the A/B clips plus the exact ops and
 *  scene that were applied, so Save persists what was auditioned even if the
 *  card's live `rx` prop has since been recomputed (a Match-reference swap). */
export interface AppliedFix {
  clips: DoctorApplyResult;
  ops: DoctorRx["ops"];
  scene: number | null;
}

/** One card's lifecycle. `applying`/`saving`/`discarding` are BUSY (a device
 *  command is in flight); every state but `saved` holds the lock. */
export type CardState =
  | { phase: "applying"; listIndex: number }
  | ({ phase: "applied"; listIndex: number } & AppliedFix)
  | ({ phase: "saving"; listIndex: number } & AppliedFix)
  | ({ phase: "discarding"; listIndex: number } & AppliedFix)
  | { phase: "saved" };

export interface ApplyLock {
  /** A card's lifecycle state; undefined = idle (draft). */
  stateOf: (id: string) => CardState | undefined;
  /** The card holding the device's edit buffer, or null. */
  holder: string | null;
  /** A device command is in flight for some card — the page disables the
   *  controls that would unmount it (row collapse, "Check other sounds", a
   *  Match-reference change, the results filter). */
  busy: boolean;
  /** idle → applying, IFF no card holds the lock. Returns false when refused. */
  beginApply: (id: string, listIndex: number) => boolean;
  /** applying → applied. Returns false (and changes nothing) when the card is
   *  no longer applying — an unmount already discarded it. */
  finishApply: (id: string, fix: AppliedFix) => boolean;
  /** applying → idle (a failed apply auto-restores on the device). */
  failApply: (id: string) => void;
  beginSave: (id: string) => void;
  finishSave: (id: string) => void;
  /** saving → applied (the edit is still live and unsaved). */
  failSave: (id: string) => void;
  beginDiscard: (id: string) => void;
  finishDiscard: (id: string) => void;
  failDiscard: (id: string) => void;
  /** Discard the device edit + release the lock IFF `id` holds an applying or
   *  applied edit, else a no-op — the single arbiter a PrescriptionCard calls on
   *  ITS OWN unmount and the page calls on leave, so an abandoned edit never
   *  strands the device + the lock with no mounted UI left to save/discard it.
   *  A card that is SAVING is skipped: the save is persisting the fix, and a
   *  discard queued behind it is an unintended device op. Whichever of two
   *  simultaneous cleanups calls it first wins; the other no-ops. */
  discardIfMine: (id: string) => void;
}

const NOOP: ApplyLock = {
  stateOf: () => undefined,
  holder: null,
  busy: false,
  beginApply: () => true,
  finishApply: () => true,
  failApply: () => undefined,
  beginSave: () => undefined,
  finishSave: () => undefined,
  failSave: () => undefined,
  beginDiscard: () => undefined,
  finishDiscard: () => undefined,
  failDiscard: () => undefined,
  discardIfMine: () => undefined,
};

/** Default is an unguarded no-op lock (a card in isolation keeps no state here). */
export const ApplyLockContext = createContext<ApplyLock>(NOOP);

export const useApplyLock = (): ApplyLock => useContext(ApplyLockContext);

const BUSY: ReadonlySet<CardState["phase"]> = new Set([
  "applying",
  "saving",
  "discarding",
]);

/** The card holding the lock: the one past idle and short of saved. */
function holderIn(cards: ReadonlyMap<string, CardState>): string | null {
  for (const [id, s] of cards) {
    if (s.phase !== "saved") return id;
  }
  return null;
}

/** An applied-family card re-tagged with `phase` (its fix carried over). */
function asPhase(
  phase: "applied" | "saving" | "discarding",
): (cur: CardState) => CardState {
  return (cur) =>
    cur.phase === "saved" || cur.phase === "applying" ? cur : { ...cur, phase };
}

/** The page-side owner of every card's state (DoctorResults provides it). The
 *  ref is the source of truth for transitions (read only in callbacks); each
 *  transition publishes a snapshot to React state for rendering. */
export function useApplyLockOwner(): ApplyLock {
  const cardsRef = useRef(new Map<string, CardState>());
  const [cards, setCards] = useState<ReadonlyMap<string, CardState>>(
    () => new Map(),
  );
  const publish = useCallback(() => {
    setCards(new Map(cardsRef.current));
  }, []);

  // Move `id` from one of `from` to `next` (undefined → idle). A no-op returning
  // false when the card isn't in `from` — every async continuation goes through
  // this, so a stale one can't resurrect a card an unmount already discarded.
  const move = useCallback(
    (
      id: string,
      from: CardState["phase"][],
      next: (cur: CardState) => CardState | undefined,
    ): boolean => {
      const cur = cardsRef.current.get(id);
      if (cur === undefined || !from.includes(cur.phase)) return false;
      const n = next(cur);
      if (n === undefined) cardsRef.current.delete(id);
      else cardsRef.current.set(id, n);
      publish();
      return true;
    },
    [publish],
  );

  const transitions = useMemo(
    () => ({
      beginApply: (id: string, listIndex: number): boolean => {
        if (holderIn(cardsRef.current) !== null) return false;
        cardsRef.current.set(id, { phase: "applying", listIndex });
        publish();
        return true;
      },
      finishApply: (id: string, fix: AppliedFix): boolean =>
        move(id, ["applying"], (cur) =>
          cur.phase === "applying"
            ? { phase: "applied", listIndex: cur.listIndex, ...fix }
            : cur,
        ),
      failApply: (id: string) => {
        move(id, ["applying"], () => undefined);
      },
      beginSave: (id: string) => {
        move(id, ["applied"], asPhase("saving"));
      },
      finishSave: (id: string) => {
        move(id, ["saving"], () => ({ phase: "saved" }));
      },
      failSave: (id: string) => {
        move(id, ["saving"], asPhase("applied"));
      },
      beginDiscard: (id: string) => {
        move(id, ["applied"], asPhase("discarding"));
      },
      finishDiscard: (id: string) => {
        move(id, ["discarding"], () => undefined);
      },
      failDiscard: (id: string) => {
        move(id, ["discarding"], asPhase("applied"));
      },
      discardIfMine: (id: string) => {
        const cur = cardsRef.current.get(id);
        if (cur?.phase !== "applying" && cur?.phase !== "applied") return;
        cardsRef.current.delete(id);
        void doctorDiscard(cur.listIndex).catch(() => undefined);
        publish();
      },
    }),
    [move, publish],
  );

  return useMemo<ApplyLock>(() => {
    const holder = holderIn(cards);
    const holderState = holder === null ? undefined : cards.get(holder);
    return {
      ...transitions,
      stateOf: (id) => cards.get(id),
      holder,
      busy: holderState !== undefined && BUSY.has(holderState.phase),
    };
  }, [cards, transitions]);
}
