// src/__tests__/pickCardTestUtils.tsx — the dialog-card harness for suites that render
// `FlatLevelPick`, directly or inside `SetupPage`. Its menu portals into the card that
// `DialogCardCtx` names (`usePickAnchor`), so a test must supply one. The context is
// imported from `../views/overlays/wizardContext`, the path the pickers use; that module
// re-exports the DS Dialog's own context (`../ui/dialogContext`), so both are one object.

import { useRef, type ReactNode } from "react";

import { DialogCardCtx } from "../views/overlays/wizardContext";

/** `FlatLevelPick`'s menu only opens against a DialogCardCtx card ref (mirrors the
 *  real wizard's `LevelPage`) — supply a plain positioned div, the same shape
 *  production uses. */
export function WithCard({ children }: { children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  return (
    <DialogCardCtx.Provider value={ref}>
      <div ref={ref} style={{ position: "relative", width: 400, height: 400 }}>
        {children}
      </div>
    </DialogCardCtx.Provider>
  );
}
