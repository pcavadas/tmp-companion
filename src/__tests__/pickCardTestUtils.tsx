// src/__tests__/pickCardTestUtils.tsx — shared DialogCardCtx test harness for
// FsParamPick.test.tsx and SceneLevelPick.test.tsx (was a byte-identical `WithCard`
// in each). Imports `DialogCardCtx` from `../views/overlays/wizardContext` — the
// SAME path the pickers themselves import it from (they re-export the DS Dialog's
// context, `../ui/dialogContext`, so both paths resolve to one object; this just
// keeps tests and components pointed at the one the components use).

import { useRef, type ReactNode } from "react";

import { DialogCardCtx } from "../views/overlays/wizardContext";

/** FsParamPick's and SceneLevelPick's menus only open against a DialogCardCtx card
 *  ref (mirrors the real wizard's `<Dialog>`) — supply a plain positioned div, the
 *  same shape production uses. */
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
