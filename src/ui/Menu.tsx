// src/ui/Menu.tsx — the single design-system anchored menu / popover.
//
// Every "click a trigger → floating panel" surface routes through this (the songs/settings
// context menus, the add-songs popover), so the anchored card + outside-dismiss + card
// chrome live in ONE place instead of being hand-rolled per view (which had drifted to
// different shadows, radii, and z-index pairs). The CALLER wraps the trigger + <Menu> in
// one box (conventionally `position: relative`): that box — the Menu's DOM parent — is the
// ANCHOR the card aligns under (right edges flush), and a pointerdown inside it is not an
// "outside" press, so the trigger's own toggle still closes the menu.
//
//   <span style={{ position: "relative" }}>
//     <button onClick={() => setOpen(o => !o)} />
//     {open && (
//       <Menu onClose={() => setOpen(false)}>
//         <MenuItem label="Edit…" onClick={…} />
//         <MenuDivider />
//         <MenuItem label="Delete" danger onClick={…} />
//       </Menu>
//     )}
//   </span>
//
// Fixed in #308, once here:
//   1. The card PORTALS to `document.body` at `position: fixed`, placed by
//      `usePickAnchor`'s shared `placeAnchored` rule against the viewport — so a menu on
//      the last rows of an `overflowY: auto` list (SongList) flips ABOVE its trigger
//      instead of being clipped by the scroller. It re-places on scroll/resize and when
//      its own content resizes.
//   2. Dismiss is a document `pointerdown` OUTSIDE the card and the anchor box — no
//      transparent Scrim. The Scrim closed the menu on the first outside click AND ate
//      it; now that one click both closes the menu and lands on whatever was clicked.
//   3. `role` comes from the caller: an item menu is "menu", the add-songs popover (a
//      search field + checklist + buttons) is a non-modal "dialog".

import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import type { ReactNode } from "react";
import { createPortal } from "react-dom";

import { useStyles } from "../theme/ThemeContext";
import { placeAnchored } from "../views/overlays/usePickAnchor";

export interface MenuProps {
  /** Dismiss — fired by a pointerdown outside the card and its anchor box. */
  onClose: () => void;
  /** ARIA role of the floating card: "menu" for a list of MenuItems (default), "dialog"
   *  for a richer popover holding form controls (add-songs). */
  role?: "menu" | "dialog";
  /** Accessible name for the card (pass one with `role="dialog"`). */
  label?: string;
  /** Gap (px) between the trigger and the card. Default 4. */
  gap?: number;
  /** Fixed width (px) — the richer popover case (add-songs). Omit for an item menu. */
  width?: number;
  /** Min width (px) when not fixed-width. Default 140. */
  minWidth?: number;
  /** Card z-index (the card is portaled to `document.body`, a sibling of the app root).
   *  Default 31. */
  zIndex?: number;
  /** Card surface: the compact dropdown ("menu", padded + auto-scroll) or the larger
   *  floating panel ("popover", no padding, caller owns inner layout). Default "menu". */
  surface?: "menu" | "popover";
  children: ReactNode;
}

export function Menu({
  onClose,
  role = "menu",
  label,
  gap = 4,
  width,
  minWidth = 140,
  zIndex = 31,
  surface = "menu",
  children,
}: MenuProps) {
  const s = useStyles();
  /** A hidden marker rendered IN PLACE: its parent element is the anchor box. */
  const markerRef = useRef<HTMLSpanElement>(null);
  const cardRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);

  // Measure the committed card + anchor and place the card in viewport space (right
  // edges flush, flipped above when it would overflow the window bottom). Converges:
  // `pos` changes no measured dimension, and an unchanged result keeps the old state.
  const place = useCallback(() => {
    const host = markerRef.current?.parentElement;
    const card = cardRef.current;
    if (!host || !card) return;
    const r = host.getBoundingClientRect();
    const mw = card.offsetWidth;
    const next = placeAnchored(
      {
        left: r.right - mw,
        below: r.bottom + gap,
        above: r.top,
        width: r.width,
        cardW: window.innerWidth,
        cardH: window.innerHeight,
      },
      mw,
      card.offsetHeight,
      gap,
    );
    setPos((p) => (p?.left === next.left && p.top === next.top ? p : next));
  }, [gap]);

  // First placement before paint (the card renders hidden until then), then follow the
  // anchor: any scroll (capture — the list that scrolls is not the window), a window
  // resize, or the card's own content resizing (add-songs growing its new-song form).
  useLayoutEffect(() => {
    place();
    window.addEventListener("scroll", place, true);
    window.addEventListener("resize", place);
    const card = cardRef.current;
    const ro =
      typeof ResizeObserver === "undefined" ? null : new ResizeObserver(place);
    if (ro && card) ro.observe(card);
    return () => {
      window.removeEventListener("scroll", place, true);
      window.removeEventListener("resize", place);
      ro?.disconnect();
    };
  }, [place]);

  // Outside dismiss WITHOUT swallowing the press: listen on the document (capture, so a
  // stopPropagation elsewhere can't hide it) and only close — the click that follows
  // still reaches its own target. Presses inside the card or the anchor box (the trigger
  // toggles the menu itself) are not outside.
  useEffect(() => {
    const onDown = (e: PointerEvent) => {
      const target = e.target;
      if (!(target instanceof Node)) return;
      if (cardRef.current?.contains(target)) return;
      if (markerRef.current?.parentElement?.contains(target)) return;
      onClose();
    };
    document.addEventListener("pointerdown", onDown, true);
    return () => {
      document.removeEventListener("pointerdown", onDown, true);
    };
  }, [onClose]);

  return (
    <>
      <span ref={markerRef} style={{ display: "none" }} />
      {createPortal(
        <div
          ref={cardRef}
          role={role}
          aria-label={label}
          style={{
            ...(surface === "popover" ? s.popoverCard : s.menuCard),
            position: "fixed",
            left: pos?.left ?? 0,
            top: pos?.top ?? 0,
            visibility: pos ? "visible" : "hidden",
            zIndex,
            ...(width != null ? { width } : { minWidth }),
          }}
        >
          {children}
        </div>,
        document.body,
      )}
    </>
  );
}
