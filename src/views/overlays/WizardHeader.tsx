// src/views/overlays/WizardHeader.tsx — the bgAlt bar that frames the wizard's
// StepRail, shared by the modal `WizardShell` and the full-page `WizardPage`.

import { useTheme } from "../../theme/ThemeContext";
import { DIALOG_PAD_X } from "../../ui/Dialog";
import { Button } from "../../ui/primitives";
import { StepRail } from "./StepRail";
import type { RailStep } from "./wizardContext";

// ---- rail header (the bgAlt bar that frames the StepRail) -------------------
// Shared by the modal WizardShell and the full-page `WizardPage` (Level + Doctor) so
// the header styling lives in ONE place; only the outer container (centered modal
// vs opaque inset:0 page) differs between the two. `back`, when given, puts a Back
// link before the rail (the Doctor Set up page).
export interface WizardHeaderProps {
  current: number;
  steps?: readonly RailStep[];
  back?: () => void;
}

export function WizardHeader({ current, steps, back }: WizardHeaderProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        gap: t.space8,
        padding: `${String(t.space8)}px ${String(DIALOG_PAD_X)}px`,
        borderBottom: `0.5px solid ${t.hairline}`,
        background: t.bgAlt,
      }}
    >
      {back && (
        <Button
          variant="bare"
          onClick={back}
          style={{
            fontFamily: t.sans,
            fontSize: t.fsLabel,
            color: t.mutedInk,
            whiteSpace: "nowrap",
            flexShrink: 0,
          }}
        >
          Back
        </Button>
      )}
      <div style={{ flex: 1, minWidth: 0 }}>
        <StepRail current={current} steps={steps} />
      </div>
    </div>
  );
}
