// src/ui/MoreMenu.tsx — a row's or a page's "⋯" button and the menu it opens (look B;
// Songs and Settings): a plain 22 px icon button in a row ("More for {name}"), or a
// secondary 28 px one beside a page title ("Setlist options"). The menu is the DS
// `Menu`, 180 wide, anchored to the button with right edges flush. The caller owns
// `open`, so a row can show `rowSelected` while its menu is open, and passes the
// MenuItems. While a device write is busy the button is disabled and the menu stays
// shut.

import type { ReactNode } from "react";

import { Button } from "./primitives";
import { Menu } from "./Menu";

export interface MoreMenuProps {
  /** The button's accessible name ("More for Higher Ground", "Setlist options"). */
  label: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  disabled?: boolean;
  /** "row": plain 22 px, inside a row (default). "page": secondary 28 px, in a page
   *  title's actions. */
  size?: "row" | "page";
  /** The MenuItems (and MenuDividers). */
  children: ReactNode;
}

export function MoreMenu({
  label,
  open,
  onOpenChange,
  disabled = false,
  size = "row",
  children,
}: MoreMenuProps) {
  const shown = open && !disabled;
  return (
    <span style={{ position: "relative", display: "flex", flexShrink: 0 }}>
      <Button
        variant={size === "page" ? "secondary" : "plain"}
        size={size === "page" ? "md" : "sm"}
        icon="more"
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={shown}
        disabled={disabled}
        onClick={() => {
          onOpenChange(!open);
        }}
      />
      {shown && (
        <Menu
          width={180}
          onClose={() => {
            onOpenChange(false);
          }}
        >
          {children}
        </Menu>
      )}
    </span>
  );
}
