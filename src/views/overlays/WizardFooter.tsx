// src/views/overlays/WizardFooter.tsx — the wizard's two-slot footer (secondary left ·
// primary right), the same slots on every step.

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { DialogFooter } from "../../ui/Dialog";

// ---- footer (two fixed slots) ----------------------------------------------
export interface WizardFooterProps {
  left: ReactNode;
  right: ReactNode;
}

export function WizardFooter({ left, right }: WizardFooterProps) {
  // The wizard's two-slot footer is the DS DialogFooter with a left group (pushed
  // left by the `start` slot) and the primary actions on the right.
  const { t } = useTheme();
  return (
    <DialogFooter
      start={
        <div style={{ display: "flex", alignItems: "center", gap: t.space4 }}>
          {left}
        </div>
      }
    >
      <div style={{ display: "flex", alignItems: "center", gap: t.space4 }}>
        {right}
      </div>
    </DialogFooter>
  );
}
