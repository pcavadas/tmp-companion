// src/ui/Dialog.tsx — the single design-system dialog shell + its chrome slots.
//
// Every dialog/overlay renders through THIS component (the leveling wizard, the "How
// leveling works" sheet, the copy SaveOverlay, the destructive-confirm Modal), so the
// backdrop, card, and section padding live in exactly one place. Rules the old per-dialog
// copies got wrong, fixed here once:
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
import { DialogCardCtx } from "./dialogContext";

/** The dialog width scale. sm = confirms / progress, md = the explainer sheet + the Doctor
 *  run, lg = the leveling wizard (its run step is a 5-column table that needs the width).
 *  (Add xl here when a wider dialog needs one.) */
export type DialogSize = "sm" | "md" | "lg";

const DIALOG_WIDTH: Record<DialogSize, number> = { sm: 460, md: 560, lg: 780 };

/** Shared horizontal padding for every dialog section (header/body/footer). The wizard's
 *  own WizardHeader/WizardFooter import this too so nothing drifts. */
export const DIALOG_PAD_X = 22;

const VIEWPORT_CAP = "calc(100vh - 32px)";
/** Width twin of VIEWPORT_CAP. The window's minWidth (640) is narrower than the `lg`
 *  card, so without this the wizard clips off-screen on a narrowed window. */
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
        style={{
          position: "absolute",
          inset: 0,
          background: t.scrim,
          backdropFilter: "blur(1.5px)",
          WebkitBackdropFilter: "blur(1.5px)",
        }}
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
          background: t.bg,
          color: t.ink,
          border: `0.5px solid ${t.hairlineStrong}`,
          borderRadius: t.rDialog,
          boxShadow: t.shadowModal,
          fontFamily: t.sans,
          outline: "none",
          overflow: "hidden",
        }}
      >
        <DialogCardCtx.Provider value={cardRef}>
          {children}
        </DialogCardCtx.Provider>
      </div>
    </div>,
    document.body,
  );
}

// ---- chrome slots ----------------------------------------------------------
// Header (bottom hairline) · Body (scrolls) · Footer (top hairline, bgAlt tint).
// One DIALOG_PAD_X so dialogs never drift to ad-hoc padding again.

export function DialogHeader({ children }: { children: ReactNode }) {
  const { t } = useTheme();
  return (
    <div
      style={{
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: t.space6,
        padding: `${String(t.space8)}px ${String(DIALOG_PAD_X)}px`,
        borderBottom: `0.5px solid ${t.hairline}`,
      }}
    >
      {children}
    </div>
  );
}

export function DialogBody({ children }: { children: ReactNode }) {
  const { t } = useTheme();
  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        overflowY: "auto",
        padding: `${String(t.space8)}px ${String(DIALOG_PAD_X)}px`,
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
        gap: t.space4,
        padding: `${String(t.space6)}px ${String(DIALOG_PAD_X)}px`,
        borderTop: `0.5px solid ${t.hairline}`,
        background: t.bgAlt,
      }}
    >
      {start != null && <div style={{ marginRight: "auto" }}>{start}</div>}
      {children}
    </div>
  );
}
