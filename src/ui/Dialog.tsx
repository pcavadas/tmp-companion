// src/ui/Dialog.tsx — the single design-system dialog shell + its chrome slots (look B).
//
// Every dialog/overlay renders through THIS component (the "How leveling works" sheet,
// the copy SaveOverlay, the confirm Modal), so the backdrop, card,
// and section padding live in exactly one place. Look B: a `scrim` backdrop, a white card
// (`radiusGroup`, `shadowDialog`, no edge), 20 px padding, a 15/600 title, 13 px body and
// buttons right-aligned. This file stays BUTTON-FREE: any confirm shell that carries
// buttons (Modal) lives in primitives.tsx, which imports this one — never the reverse.
// Rules the old per-dialog copies got wrong, fixed here once:
//   1. The backdrop is `position: fixed` (the viewport), NOT `absolute` (which resolved
//      against the view container below the tab bar) — so the scrim/blur covers the WHOLE
//      window including the top tab bar, and the card centers in the viewport.
//   2. The card height is capped to the viewport; DialogBody scrolls, so an overflowing
//      dialog never clips past the window edge.
//   3. Padding + dividers are owned by DialogHeader/DialogBody/DialogFooter (one
//      DIALOG_PAD_X), so dialogs stop drifting to per-screen ad-hoc padding.
//   4. The dialog is MODAL for assistive tech and the Tab key too, not just the mouse
//      (#308): it portals to `document.body` — OUTSIDE the app root — and the app root
//      (`#root`, main.tsx) is `inert` while any dialog is open, so nothing behind the
//      scrim is Tab-reachable or announced. A module-level open-dialog COUNT owns the
//      attribute, so stacked dialogs (a confirm over a sheet) un-inert the root only when
//      the LAST one closes. No `#root` (a unit test's container) → the inert step no-ops.
//   5. `label` is REQUIRED — a dialog with no accessible name is announced as just
//      "dialog". The card takes focus on open only when nothing inside it already has it,
//      so a child's `autoFocus` (which fires before this component's effects) wins.

import { useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { createPortal } from "react-dom";

import { useTheme } from "../theme/ThemeContext";

/** The dialog width scale. xs = a confirm (Stop, Delete), progress = a save's progress
 *  (Copy's), sm = forms, explainer = the How leveling works sheet (its two-column
 *  table), md = a wider form, lg = a wide table. (Add xl here when a wider dialog needs
 *  one.) */
export type DialogSize = "xs" | "progress" | "sm" | "explainer" | "md" | "lg";

const DIALOG_WIDTH: Record<DialogSize, number> = {
  xs: 380,
  progress: 420,
  sm: 460,
  explainer: 500,
  md: 560,
  lg: 780,
};

/** Shared padding for every dialog section (header/body/footer). */
const DIALOG_PAD_X = 20;

const VIEWPORT_CAP = "calc(100vh - 32px)";
/** Width twin of VIEWPORT_CAP. The window's minWidth (800) less this margin is narrower
 *  than the `lg` card, so without this the wizard clips off-screen on a narrowed window. */
const VIEWPORT_CAP_X = "calc(100vw - 32px)";

export interface DialogProps {
  /** Backdrop click (click-only app — no Escape shortcut). Pass undefined to make the
   *  backdrop inert (e.g. a run in progress that must not be dismissed by a stray click). */
  onClose?: () => void;
  /** Width step. Default "sm". */
  size?: DialogSize;
  /** Fixed card height (px) — a constant frame (the wizard). Capped to the viewport. */
  height?: number;
  /** Stacking order. Default 50. */
  zIndex?: number;
  /** Accessible name — labels the dialog for screen readers. Required: an unnamed
   *  dialog is announced as just "dialog". */
  label: string;
  children: ReactNode;
}

// ---- app-root inert (shared by every open dialog) ---------------------------
// One count for the whole app: the root goes inert with the FIRST open dialog and comes
// back only when the LAST one closes, so closing a confirm stacked over a sheet never
// re-exposes the app behind the sheet that is still open.
const APP_ROOT_ID = "root";
let openDialogs = 0;

/** Marks the app root inert while ≥1 dialog is open; returns the matching release. */
function holdAppInert(): () => void {
  openDialogs += 1;
  if (openDialogs === 1)
    document.getElementById(APP_ROOT_ID)?.setAttribute("inert", "");
  let released = false;
  return () => {
    if (released) return;
    released = true;
    openDialogs -= 1;
    if (openDialogs === 0)
      document.getElementById(APP_ROOT_ID)?.removeAttribute("inert");
  };
}

export function Dialog({
  onClose,
  size = "sm",
  height,
  zIndex = 50,
  label,
  children,
}: DialogProps) {
  const { t } = useTheme();
  const cardRef = useRef<HTMLDivElement>(null);

  // The element focused BEFORE this dialog mounted, captured at first render — an
  // effect would run after a child's `autoFocus` and capture that child instead.
  const [opener] = useState(() =>
    document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null,
  );

  // Inert the app root, move focus into the dialog (unless a child already took it),
  // and on close release the root FIRST — focus() on an element inside an inert root is
  // a no-op — then restore focus to the opener.
  useLayoutEffect(() => {
    const release = holdAppInert();
    const card = cardRef.current;
    if (card && !card.contains(document.activeElement)) card.focus();
    return () => {
      release();
      opener?.focus();
    };
  }, [opener]);

  return createPortal(
    <div
      style={{
        position: "fixed",
        inset: 0,
        zIndex,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      <div
        data-backdrop=""
        onClick={onClose}
        style={{ position: "absolute", inset: 0, background: t.scrim }}
      />
      <div
        ref={cardRef}
        role="dialog"
        aria-modal="true"
        aria-label={label}
        tabIndex={-1}
        style={{
          position: "relative",
          width: DIALOG_WIDTH[size],
          height,
          maxWidth: VIEWPORT_CAP_X,
          maxHeight: VIEWPORT_CAP,
          display: "flex",
          flexDirection: "column",
          background: t.surface,
          color: t.text,
          borderRadius: t.radiusGroup,
          boxShadow: t.shadowDialog,
          fontFamily: t.fontSans,
          fontSize: t.fontBody,
          lineHeight: `${String(t.lineBody)}px`,
          outline: "none",
          overflow: "hidden",
        }}
      >
        {children}
      </div>
    </div>,
    document.body,
  );
}

// ---- chrome slots ----------------------------------------------------------
// Header (the title row) · Body (scrolls) · Footer (buttons, right-aligned). No rules or
// tinted bands: look B separates them with space alone. One DIALOG_PAD_X so dialogs never
// drift to ad-hoc padding. Spacing: 20 px around the card, 10 px between the title and
// the body (the header's negative bottom margin eats half the body's top padding), 18 px
// above the buttons — held by the FOOTER (16 + the body's 2), outside the body's scroll
// box, so a long body that scrolls still stops clear of the buttons. A dialog ends in a
// DialogFooter.

export function DialogHeader({ children }: { children: ReactNode }) {
  const { t } = useTheme();
  return (
    <div
      style={{
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: t.sp12,
        padding: `${String(DIALOG_PAD_X)}px ${String(DIALOG_PAD_X)}px 0`,
        marginBottom: -t.sp8 - t.sp2,
      }}
    >
      {children}
    </div>
  );
}

export function DialogBody({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        overflowY: "auto",
        padding: `${String(DIALOG_PAD_X)}px ${String(DIALOG_PAD_X)}px 2px`,
      }}
    >
      {children}
    </div>
  );
}

export interface DialogFooterProps {
  /** Optional left-aligned content (e.g. a caption); actions in `children` stay right. */
  start?: ReactNode;
  children: ReactNode;
}

export function DialogFooter({ start, children }: DialogFooterProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "flex-end",
        gap: t.sp8,
        padding: `${String(t.sp16)}px ${String(DIALOG_PAD_X)}px ${String(DIALOG_PAD_X)}px`,
      }}
    >
      {start != null && <div style={{ marginRight: "auto" }}>{start}</div>}
      {children}
    </div>
  );
}
