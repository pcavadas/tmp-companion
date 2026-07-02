// src/ui/Menu.tsx — the single design-system anchored menu / popover.
//
// Every "click a trigger → floating panel" surface routes through this (the songs/settings
// context menus, the add-songs popover), so the Scrim + anchored card + z-index pairing +
// card chrome live in ONE place instead of being hand-rolled per view (which had drifted to
// different shadows, radii, and z-index pairs). The CALLER still wraps the trigger + <Menu>
// in a `position: relative` box — the card anchors to it; the Scrim is the full-viewport
// click-catcher.
//
//   <span style={{ position: "relative" }}>
//     <button onClick={() => setOpen(o => !o)} />
//     {open && (
//       <Menu onClose={() => setOpen(false)}>
//         <MenuItem label="Edit…" onClick={…} />
//         <MenuDivider />
//         <MenuItem label="Delete" danger onClick={…} />
//       </Menu>
//     )}
//   </span>

import type { ReactNode } from "react";

import { useStyles } from "../theme/ThemeContext";
import { Scrim } from "./primitives";

export interface MenuProps {
  /** Outside-click + Scrim dismiss. */
  onClose: () => void;
  /** Gap (px) between the trigger and the card. Default 4. */
  gap?: number;
  /** Fixed width (px) — the richer popover case (add-songs). Omit for an item menu. */
  width?: number;
  /** Min width (px) when not fixed-width. Default 140. */
  minWidth?: number;
  /** Card z-index; the Scrim sits one below. Default 31 (Scrim 30). */
  zIndex?: number;
  /** Card surface: the compact dropdown ("menu", padded + auto-scroll) or the larger
   *  floating panel ("popover", no padding, caller owns inner layout). Default "menu". */
  surface?: "menu" | "popover";
  children: ReactNode;
}

export function Menu({
  onClose,
  gap = 4,
  width,
  minWidth = 140,
  zIndex = 31,
  surface = "menu",
  children,
}: MenuProps) {
  const s = useStyles();
  return (
    <>
      <Scrim onClose={onClose} zIndex={zIndex - 1} />
      <div
        role="menu"
        style={{
          ...(surface === "popover" ? s.popoverCard : s.menuCard),
          position: "absolute",
          top: `calc(100% + ${String(gap)}px)`,
          right: 0,
          zIndex,
          ...(width != null ? { width } : { minWidth }),
        }}
      >
        {children}
      </div>
    </>
  );
}
