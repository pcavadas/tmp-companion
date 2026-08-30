// src/views/level/LevelPage.tsx — the leveling wizard's shared full-page shell
// (design handoff 1a). ALL THREE stages (Set up · Level · Summary) now render as a
// full window replacing LevelView's body — the old centered 780×606 modal
// (`WizardShell`, `Dialog size="lg"`) is gone; `WizardShell` itself stays for Doctor.
//
// One mounted frame per stage: step-rail header (`WizardHeader`, reused verbatim from
// the modal era), a title/sub block with an optional right-aligned slot (the Level
// stage's live readout), the stage's own body, and the shared `WizardFooter`. Provides
// its OWN ref as `DialogCardCtx` so the body's Pick-family dropdowns portal into THIS
// page and position in its coordinate space — same contract `LevelSetupPage` (now
// folded into this file) already established for Set up alone.

import { useRef, type ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { WizardHeader, WizardFooter, WizTitle } from "../overlays/WizardShell";
import { DialogCardCtx } from "../overlays/wizardContext";

export interface LevelPageProps {
  step: number;
  title: ReactNode;
  sub?: ReactNode;
  /** Right-aligned header slot — the Level stage's live LUFS readout. */
  right?: ReactNode;
  children: ReactNode;
  footerLeft: ReactNode;
  footerRight: ReactNode;
  /** Replaces the whole footer with this instead of the normal left/right split —
   *  for a full-width bar (e.g. `ConfirmBar`) that isn't a left/right pair. */
  footerOverride?: ReactNode;
}

export function LevelPage({
  step,
  title,
  sub,
  right,
  children,
  footerLeft,
  footerRight,
  footerOverride,
}: LevelPageProps) {
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
      <WizardHeader current={step} />
      <DialogCardCtx.Provider value={pageRef}>
        <div
          style={{
            flex: 1,
            minHeight: 0,
            padding: `${String(t.space8)}px ${String(t.space10)}px 0`,
            display: "flex",
            flexDirection: "column",
            gap: t.space7,
            overflow: "hidden",
          }}
        >
          <div
            style={{
              flexShrink: 0,
              display: "flex",
              alignItems: "flex-start",
              justifyContent: "space-between",
              gap: t.space10,
            }}
          >
            <div
              style={{
                maxWidth: 620,
                display: "flex",
                flexDirection: "column",
                gap: t.space4,
              }}
            >
              <WizTitle
                size={27}
                style={{ letterSpacing: "-0.018em", textWrap: "balance" }}
              >
                {title}
              </WizTitle>
              {sub && (
                <div
                  style={{
                    fontFamily: t.sans,
                    fontSize: 13.5,
                    lineHeight: 1.6,
                    color: t.ink2,
                    textWrap: "pretty",
                  }}
                >
                  {sub}
                </div>
              )}
            </div>
            {right}
          </div>
          {children}
        </div>
      </DialogCardCtx.Provider>
      {footerOverride ?? <WizardFooter left={footerLeft} right={footerRight} />}
    </div>
  );
}

export default LevelPage;
