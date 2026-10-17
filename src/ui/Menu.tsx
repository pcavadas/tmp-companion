// src/ui/Menu.tsx — the single design-system anchored menu / popover (look B: white,
// `radiusPanel`, `shadowMenu`, 28-high items).
//
// Every "click a trigger → floating panel" surface routes through this (the songs/settings
// context menus, the add-songs popover, the `Select` list, Copy's block menu, anchored to a
// signal-path tile through `anchor`), so the anchored card +
// outside-dismiss + card
// chrome live in ONE place instead of being hand-rolled per view (which had drifted to
// different shadows, radii, and z-index pairs). The CALLER wraps the trigger + <Menu> in
// one box (conventionally `position: relative`): that box — the Menu's DOM parent — is the
// ANCHOR the card aligns under (right edges flush, or left edges with `align="start"`),
// and a pointerdown inside it is not an "outside" press, so the trigger's own toggle
// still closes the menu.
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
//      the shared `placeAnchored` rule (`./placeAnchored`) against the viewport — so a menu on
//      the last rows of an `overflowY: auto` list (SongList) flips ABOVE its trigger
//      instead of being clipped by the scroller. It re-places on scroll/resize, when
//      its own content resizes, and whenever `contentKey` changes (a list whose rows
//      land after the open — see the prop).
//   2. Dismiss is a document `pointerdown` OUTSIDE the card and the anchor box — no
//      transparent Scrim. The Scrim closed the menu on the first outside click AND ate
//      it; now that one click both closes the menu and lands on whatever was clicked.
//   3. `role` comes from the caller: an item menu is "menu", the `Select` list is
//      "listbox", the add-songs popover (a search field + checklist + buttons) is a
//      non-modal "dialog".

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
import { placeAnchored } from "./placeAnchored";

export interface MenuProps {
  /** Dismiss — fired by a pointerdown outside the card and its anchor box. */
  onClose: () => void;
  /** ARIA role of the floating card: "menu" for a list of MenuItems (default),
   *  "listbox" for a `Select`'s options, "dialog" for a richer popover holding form
   *  controls (add-songs). */
  role?: "menu" | "listbox" | "dialog";
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
  /** Which edges line up with the anchor box: "end" (right edges, default — a ⋯ menu)
   *  or "start" (left edges — a dropdown list under its Select). */
  align?: "start" | "end";
  /** A value that changes whenever the card's CONTENT changes size (a fetch status plus
   *  a row count is enough). The card is measured and re-placed on each change, so a
   *  list whose rows land AFTER the open (a lazy read) re-clamps and flips above its
   *  trigger instead of running off the bottom of the window. (A ResizeObserver does the
   *  same in the browser; this keeps it deterministic where there is none.) */
  contentKey?: string | number;
  /** Card surface: the compact dropdown ("menu", padded + auto-scroll) or the larger
   *  floating panel ("popover", no padding, caller owns inner layout). Default "menu". */
  surface?: "menu" | "popover";
  /** The anchor box, when it can't be the Menu's DOM parent: a trigger the caller
   *  doesn't render itself (a Copy signal-path tile). Read at each placement and outside
   *  press, so it must be stable (a `useCallback`). Default: the Menu's parent. */
  anchor?: () => Element | null;
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
  align = "end",
  contentKey,
  surface = "menu",
  anchor,
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
    const host = anchor?.() ?? markerRef.current?.parentElement;
    const card = cardRef.current;
    if (!host || !card) return;
    const r = host.getBoundingClientRect();
    const mw = card.offsetWidth;
    const next = placeAnchored(
      {
        left: align === "start" ? r.left : r.right - mw,
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
  }, [gap, align, anchor]);

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

  // Content that changes size after the open (`contentKey`): measure and place again.
  useLayoutEffect(() => {
    place();
  }, [place, contentKey]);

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
      if (anchor?.()?.contains(target)) return;
      onClose();
    };
    document.addEventListener("pointerdown", onDown, true);
    return () => {
      document.removeEventListener("pointerdown", onDown, true);
    };
  }, [onClose, anchor]);

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
