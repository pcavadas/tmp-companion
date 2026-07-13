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

import { useEffect, useRef } from "react";
import type { ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";
import { DialogCardCtx } from "./dialogContext";

/** The dialog width scale (standard sm/md steps). sm = confirms / progress, md = the
 *  explainer sheet + leveling wizard. (Add lg/xl here when a wider dialog needs one.) */
export type DialogSize = "sm" | "md";

const DIALOG_WIDTH: Record<DialogSize, number> = { sm: 460, md: 560 };

/** Shared horizontal padding for every dialog section (header/body/footer). The wizard's
 *  own WizardHeader/WizardFooter import this too so nothing drifts. */
export const DIALOG_PAD_X = 22;

const VIEWPORT_CAP = "calc(100vh - 32px)";

export interface DialogProps {
  /** Backdrop click + Escape. Pass undefined to make the backdrop inert (e.g. a run in
   *  progress that must not be dismissed by a stray click). */
  onClose?: () => void;
  /** Width step. Default "sm". */
  size?: DialogSize;
  /** Fixed card height (px) — a constant frame (the wizard). Capped to the viewport. */
  height?: number;
  /** Stacking order. Default 50. */
  zIndex?: number;
  /** Accessible name — labels the dialog for screen readers. */
  label?: string;
  children: ReactNode;
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

  useEffect(() => {
    if (!onClose) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  // Move focus into the dialog on open and restore it to the prior element on close.
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    cardRef.current?.focus();
    return () => {
      prev?.focus();
    };
  }, []);

  return (
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
    </div>
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
