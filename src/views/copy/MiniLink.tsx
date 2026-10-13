// src/views/copy/MiniLink.tsx — the small text action ("Select all" / "Clear") of the
// Copy steps. A private feature-shared module (NOT in the barrel).

import type { ReactNode } from "react";
import { useTheme } from "../../theme/ThemeContext";
import { Button } from "../../ui/primitives";

export interface MiniLinkProps {
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
}

/** A small text action ("Select all" / "Clear"). */
export function MiniLink({ children, onClick, disabled }: MiniLinkProps) {
  const { t } = useTheme();
  return (
    <Button
      variant="bare"
      disabled={disabled}
      onClick={() => {
        onClick();
      }}
      style={{
        fontFamily: t.sans,
        fontSize: t.fsUi,
        color: disabled ? t.faint : t.accentDeep,
        whiteSpace: "nowrap",
      }}
    >
      {children}
    </Button>
  );
}
