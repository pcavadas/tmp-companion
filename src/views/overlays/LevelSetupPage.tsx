// src/views/overlays/LevelSetupPage.tsx — the full-page chrome for the leveling
// CONFIGURE phase (Set up).
//
// The write phase (Run · Summary) stays a centered modal (WizardShell) — writing to
// the unit is correctly a blocking step. The configure phase instead REPLACES the
// Level page body (an opaque inset:0 overlay inside LevelView's container, so the app
// tab nav above stays put), mirroring the Copy feature's full-page select → configure.
//
// Same step rail as the modal; provides its OWN ref as DialogCardCtx so the body's
// Pick dropdowns portal into THIS page and position in its coordinate space.

import { useRef, type ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { WizardHeader } from "./WizardShell";
import { DialogCardCtx, stageToStep, type Stage } from "./wizardContext";

export interface LevelSetupPageProps {
  stage: Extract<Stage, "setup">;
  children: ReactNode;
}

export function LevelSetupPage({ stage, children }: LevelSetupPageProps) {
  const { t } = useTheme();
  const pageRef = useRef<HTMLDivElement>(null);
  return (
    <div
      ref={pageRef}
      style={{
        position: "absolute",
        inset: 0,
        zIndex: 40,
        display: "flex",
        flexDirection: "column",
        background: t.bg,
        color: t.ink,
        fontFamily: t.sans,
      }}
    >
      <WizardHeader current={stageToStep(stage)} />
      <DialogCardCtx.Provider value={pageRef}>
        {children}
      </DialogCardCtx.Provider>
    </div>
  );
}

export default LevelSetupPage;
