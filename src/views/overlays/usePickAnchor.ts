// src/views/overlays/usePickAnchor.ts — the card-portaled dropdown's shared
// open/anchor/measure state machine.
//
// `Pick`, `FsParamPick` and `SceneLevelPick` each hand-rolled the SAME ~70 lines:
// an Anchor interface, open/anchor/pos/cardEl state, a triggerRef/menuRef pair, a
// two-pass `useLayoutEffect` (render hidden to measure, then place — clamped
// horizontally, flipped ABOVE the trigger when it would overflow the card's fixed
// bottom edge), and an openMenu/close pair. This hook is the ONE copy; the three
// pickers now just plug in their own trigger chrome + menu content.
//
// `.claude/rules/frontend.md`'s legit `useLayoutEffect` DOM-measurement exception
// (a committed-DOM measure that `setState`s, guarded by a prev-value compare so it
// converges) names this hook's effect, not the old per-component ones.

import { useLayoutEffect, useRef, useState } from "react";

export interface PickAnchor {
  left: number;
  below: number;
  above: number;
  width: number;
  cardW: number;
  cardH: number;
}

export interface UsePickAnchorOptions {
  /** Checked at the top of `openMenu`; returning false suppresses the open
   *  entirely (FsParamPick's "nothing to choose" guard: `!interactive`). */
  guard?: () => boolean;
  /** Fired once the menu has opened (SceneLevelPick's lazy per-preset candidate
   *  fetch — idempotent, safe to call on every open). */
  onOpen?: () => void;
  /** A value that changes whenever the menu's CONTENT changes size — included in the
   *  measure/place effect's deps.
   *
   *  Why it exists: the effect measures `menuRef.current.offsetHeight` and depends only
   *  on `[open, anchor]`, both of which are fixed at `openMenu` time. A menu whose body
   *  arrives AFTER the open — `SceneLevelPick` opens on "Loading controls…" and fires a
   *  device read (`onOpen`) that lands a moment later with N candidate rows — therefore
   *  keeps the placement computed for the one-line skeleton: the grown menu never
   *  re-clamps and never flips above the trigger, so it renders off the bottom of the
   *  card with its rows unreachable.
   *
   *  Pass something derived from what is rendered (a fetch status plus a row count is
   *  enough; the height need not be known). Omit it for a menu whose content is fixed at
   *  open time (`Pick`, `FsParamPick`). */
  contentKey?: string | number;
}

export interface UsePickAnchorResult {
  open: boolean;
  anchor: PickAnchor | null;
  pos: { left: number; top: number } | null;
  /** The portal target, captured from `cardRef` in `openMenu` (an event handler)
   *  so render never reads `ref.current`. Null when opened with no card context
   *  (Pick's trigger-anchored inline-menu fallback). */
  cardEl: HTMLDivElement | null;
  menuRef: React.RefObject<HTMLDivElement | null>;
  triggerRef: React.RefObject<HTMLDivElement | null>;
  openMenu: (e: React.MouseEvent) => void;
  close: () => void;
}

/** `cardRef` is the wizard card to portal/anchor into (`DialogCardCtx`'s value) —
 *  undefined/no-current falls back to `anchor: null`, which `Pick` alone renders
 *  as an inline (non-portaled) menu; `FsParamPick`/`SceneLevelPick` are only ever
 *  mounted inside a card, so that branch is unreached for them in practice. */
export function usePickAnchor(
  cardRef: React.RefObject<HTMLDivElement | null> | null | undefined,
  { guard, onOpen, contentKey }: UsePickAnchorOptions = {},
): UsePickAnchorResult {
  const [open, setOpen] = useState(false);
  const [anchor, setAnchor] = useState<PickAnchor | null>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const [cardEl, setCardEl] = useState<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  // Two-pass: render hidden to measure, then place (clamped horizontally, flipped
  // above when it would overflow the card bottom). `contentKey` is in the deps so a menu
  // that GROWS after opening (a lazy fetch landing) is re-measured and re-placed — see
  // its doc above. Converges: the effect only ever writes `pos`, which changes no
  // measured dimension, so a re-run with an unchanged key re-derives the same numbers.
  useLayoutEffect(() => {
    if (!open || !anchor || !menuRef.current) return;
    const mw = menuRef.current.offsetWidth;
    const mh = menuRef.current.offsetHeight;
    const left = Math.min(
      Math.max(8, anchor.left),
      Math.max(8, anchor.cardW - mw - 8),
    );
    let top = anchor.below;
    if (top + mh > anchor.cardH - 8) top = Math.max(8, anchor.above - mh - 4);
    setPos({ left, top });
  }, [open, anchor, contentKey]);

  const openMenu = (e: React.MouseEvent) => {
    if (guard && !guard()) return;
    e.stopPropagation();
    const card = cardRef?.current;
    if (!card || !triggerRef.current) {
      // No card context (Pick used outside a wizard) → the caller's trigger-
      // anchored inline-menu fallback.
      setAnchor(null);
      setOpen((o) => !o);
      return;
    }
    const tr = triggerRef.current.getBoundingClientRect();
    const cr = card.getBoundingClientRect();
    setAnchor({
      left: tr.left - cr.left,
      below: tr.bottom - cr.top + 4,
      above: tr.top - cr.top,
      width: tr.width,
      cardW: cr.width,
      cardH: cr.height,
    });
    setCardEl(card);
    setPos(null);
    setOpen(true);
    onOpen?.();
  };

  const close = () => {
    setOpen(false);
    setPos(null);
  };

  return { open, anchor, pos, cardEl, menuRef, triggerRef, openMenu, close };
}
