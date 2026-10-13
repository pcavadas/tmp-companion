// src/ui/dialogContext.ts — the dialog-card portal context, split from Dialog.tsx so that
// file exports only components (React Fast Refresh's component-boundary rule).

import { createContext, type RefObject } from "react";

/** The dialog card's ref, exposed so `Pick`/`FlatLevelPick` can portal their dropdown
 *  INTO the card and position it in the card's own coordinate space — a menu on a bottom
 *  row then flips ABOVE instead of clipping past the frame. Every <Dialog> provides it;
 *  the full-page `LevelPage` and Doctor's `DoctorSetup` provide their own ref against the
 *  same context. */
export const DialogCardCtx =
  createContext<RefObject<HTMLDivElement | null> | null>(null);
