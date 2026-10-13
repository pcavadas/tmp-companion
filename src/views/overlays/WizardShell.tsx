// src/views/overlays/WizardShell.tsx — the leveling wizard's stable chrome.
//
// A leveling run is one linear task, so it lives in ONE mounted frame whose BODY
// swaps per stage — the frame never resizes or re-centers. The header is a 3-step
// rail (Set up · Level · Summary), the footer keeps the same two slots (secondary
// left · primary right) on every step. Backdrop click closes on every stage EXCEPT
// the run (never abort an in-progress device operation).
//
// The frame is the DS `Dialog`; its rail header is `WizardHeader`.

import type { ReactNode } from "react";

import { Dialog, type DialogSize } from "../../ui/Dialog";
import { WizardHeader } from "./WizardHeader";
import { WIZ_STEPS, type Stage, type RailStep } from "./wizardContext";

// Re-export the wizard's shared TYPES so existing importers keep working; the
// VALUE exports (DialogCardCtx, WIZ_STEPS) live in ./wizardContext. The rail, header,
// footer and title pieces live in their own files (StepRail, WizardHeader,
// WizardFooter, WizTitle).
export type { Stage, RailStep };

// ---- the fixed frame -------------------------------------------------------
export interface WizardShellProps {
  current: number;
  /** Dismiss handler for a backdrop click — pass undefined to make the scrim inert
   *  (the run stage, so a stray click never aborts a device operation). */
  onBackdrop?: () => void;
  children: ReactNode;
  /** Fixed frame height (px). Default 512. */
  height?: number;
  /** Frame width step. Default "md", the only size any current caller passes (the
   *  leveling wizard moved off `WizardShell` entirely, design 1a) — "lg" stays wired
   *  for a future wider consumer. */
  size?: DialogSize;
  /** Rail nodes (defaults to the leveling rail). Bulk Block Edit passes its own
   *  4-node rail (Block · Choose · Check · Done). */
  steps?: readonly RailStep[];
  /** The dialog's accessible name (the DS `Dialog` requires one — an unnamed dialog is
   *  announced as just "dialog"). */
  label: string;
}

export function WizardShell({
  current,
  label,
  onBackdrop,
  children,
  height = 512,
  size = "md",
  steps = WIZ_STEPS,
}: WizardShellProps) {
  return (
    <Dialog size={size} height={height} onClose={onBackdrop} label={label}>
      <WizardHeader current={current} steps={steps} />
      {children}
    </Dialog>
  );
}
