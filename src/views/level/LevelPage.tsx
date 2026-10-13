// src/views/level/LevelPage.tsx — the leveling wizard's shared full-page shell
// (design handoff 1a). ALL THREE stages (Set up · Level · Summary) now render as a
// full window replacing LevelView's body — the old centered 780×606 modal
// (`WizardShell`, `Dialog size="lg"`) is gone; `WizardShell` itself stays for Doctor.
//
// One mounted frame per stage: the shared full-page frame (`WizardPage` — page,
// step-rail header on the leveling rail, and the `DialogCardCtx` the body's Pick-family
// dropdowns portal into), then a title/sub block with an optional right-aligned slot
// (the Level stage's live readout), the stage's own body, and the shared
// `WizardFooter`.

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { WizardFooter, WizTitle } from "../overlays/WizardShell";
import { WizardPage } from "../overlays/WizardPage";

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
  return (
    <WizardPage current={step}>
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
      {footerOverride ?? <WizardFooter left={footerLeft} right={footerRight} />}
    </WizardPage>
  );
}
