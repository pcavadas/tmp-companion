// src/views/overlays/WizardPage.tsx — the full-page wizard frame (#314), shared by
// the Level wizard (`level/LevelPage`, its three stages) and the Doctor's Set up and
// Results pages.
//
// An opaque `absolute; inset:0` page that replaces its tab's body (not a dialog — it
// has no scrim), topped by the ONE step-rail header (`WizardHeader`) with the
// caller's `steps` and an optional Back link (`back`). It provides its OWN ref as
// `DialogCardCtx`, so every Pick-family dropdown inside portals into THIS page and
// positions in its coordinate space. The body below the header is the caller's.

import { useRef, type ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { WizardHeader } from "./WizardHeader";
import { DialogCardCtx, WIZ_STEPS, type RailStep } from "./wizardContext";

export interface WizardPageProps {
  /** The rail's nodes. Default: the leveling rail (Set up · Level · Summary). */
  steps?: readonly RailStep[];
  /** 0-based index of the current rail node. */
  current: number;
  /** Puts a Back link before the rail. Omit for no Back. */
  back?: () => void;
  children: ReactNode;
}

export function WizardPage({
  steps = WIZ_STEPS,
  current,
  back,
  children,
}: WizardPageProps) {
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
      <WizardHeader current={current} steps={steps} back={back} />
      <DialogCardCtx.Provider value={pageRef}>
        {children}
      </DialogCardCtx.Provider>
    </div>
  );
}
