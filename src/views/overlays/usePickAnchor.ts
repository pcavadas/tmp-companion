// src/views/overlays/usePickAnchor.ts — the card-portaled dropdown's shared
// open/anchor/measure state machine.
//
// `Pick`'s state machine: an Anchor interface, open/anchor/pos/cardEl state, a
// triggerRef/menuRef pair, a two-pass `useLayoutEffect` (render hidden to measure, then
// place — clamped horizontally, flipped ABOVE the trigger when it would overflow the
// card's fixed bottom edge), and an openMenu/close pair. Its placement rule
// (`placeAnchored`) is shared with the DS `Menu`, whose own `contentKey` re-places a
// list whose rows land after the open (the look B `Select`).
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

/** The ONE placement rule for an anchored floating menu, shared by this hook (in the
 *  dialog card's coordinate space) and the DS `Menu` (in the viewport's): clamp `left`
 *  into the frame with an 8px margin, drop BELOW the trigger, and flip ABOVE it (`gap`
 *  px clear) when the `mw`×`mh` menu would overflow the frame's bottom edge. */
export function placeAnchored(
  anchor: PickAnchor,
  mw: number,
  mh: number,
  gap = 4,
): { left: number; top: number } {
  const left = Math.min(
    Math.max(8, anchor.left),
    Math.max(8, anchor.cardW - mw - 8),
  );
  let top = anchor.below;
  if (top + mh > anchor.cardH - 8) top = Math.max(8, anchor.above - mh - gap);
  return { left, top };
}

export interface UsePickAnchorOptions {
  /** Fired once the menu has opened (a lazy per-preset fetch — idempotent, safe to
   *  call on every open). */
  onOpen?: () => void;
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
 *  undefined/no-current falls back to `anchor: null`, which `Pick` renders as an
 *  inline (non-portaled) menu. A `Pick`'s options are fixed at open time, so the
 *  placement is measured once per open. */
export function usePickAnchor(
  cardRef: React.RefObject<HTMLDivElement | null> | null | undefined,
  { onOpen }: UsePickAnchorOptions = {},
): UsePickAnchorResult {
  const [open, setOpen] = useState(false);
  const [anchor, setAnchor] = useState<PickAnchor | null>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const [cardEl, setCardEl] = useState<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  // Two-pass: render hidden to measure, then place (clamped horizontally, flipped
  // above when it would overflow the card bottom). Converges: the effect only ever
  // writes `pos`, which changes no measured dimension.
  useLayoutEffect(() => {
    if (!open || !anchor || !menuRef.current) return;
    setPos(
      placeAnchored(
        anchor,
        menuRef.current.offsetWidth,
        menuRef.current.offsetHeight,
      ),
    );
  }, [open, anchor]);

  const openMenu = (e: React.MouseEvent) => {
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
