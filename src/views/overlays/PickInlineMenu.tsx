// src/views/overlays/PickInlineMenu.tsx — the Pick dropdown's fallback menu.
//
// Used when `Pick` renders OUTSIDE a wizard card (no DialogCardCtx): a trigger-anchored
// menu (top:100% / right:0) over a fixed full-viewport click-catcher. No flip logic —
// the fallback assumes there's room below (the wizard path handles the clipped case).

import { useStyles } from "../../theme/ThemeContext";

export interface PickInlineMenuProps {
  onClose: () => void;
  children: React.ReactNode;
}

export function PickInlineMenu({ onClose, children }: PickInlineMenuProps) {
  const s = useStyles();
  return (
    <>
      <div
        onClick={(e) => {
          e.stopPropagation();
          onClose();
        }}
        style={{ position: "fixed", inset: 0, zIndex: 60 }}
      />
      <div
        style={{
          ...s.menuCard,
          position: "absolute",
          top: "calc(100% + 4px)",
          right: 0,
          zIndex: 61,
          minWidth: 172,
        }}
      >
        {children}
      </div>
    </>
  );
}

export default PickInlineMenu;
