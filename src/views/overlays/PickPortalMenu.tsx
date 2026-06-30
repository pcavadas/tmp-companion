// src/views/overlays/PickPortalMenu.tsx — the Pick dropdown's card-portaled menu.
//
// Portals a backdrop + positioned menu INTO the wizard card so the menu lives in the
// card's own (clipped) coordinate space. `Pick` measures via `menuRef` and feeds back
// the resolved left/top (flipped above when it would overflow the card bottom); until
// then the menu renders hidden to avoid a wrong-position flash.

import { createPortal } from "react-dom";

import { useStyles } from "../../theme/ThemeContext";

export interface PickPortalMenuProps {
  /** The wizard card element to portal into (the menu positions relative to it). */
  cardEl: HTMLDivElement;
  /** Measured by `Pick` to compute the flip-above placement. */
  menuRef: React.RefObject<HTMLDivElement | null>;
  left: number;
  top: number;
  /** False until the two-pass measure resolves the final placement. */
  visible: boolean;
  minWidth: number;
  onClose: () => void;
  children: React.ReactNode;
}

export function PickPortalMenu({
  cardEl,
  menuRef,
  left,
  top,
  visible,
  minWidth,
  onClose,
  children,
}: PickPortalMenuProps) {
  const s = useStyles();
  return createPortal(
    <>
      <div
        onClick={onClose}
        style={{ position: "absolute", inset: 0, zIndex: 60 }}
      />
      <div
        ref={menuRef}
        style={{
          ...s.menuCard,
          position: "absolute",
          left,
          top,
          zIndex: 61,
          visibility: visible ? "visible" : "hidden",
          minWidth,
          maxWidth: 340,
        }}
      >
        {children}
      </div>
    </>,
    cardEl,
  );
}

export default PickPortalMenu;
