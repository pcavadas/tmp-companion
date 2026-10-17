// src/views/copy/BlockMenu.tsx — the menu on a target block in Place blocks (look B).
//
// A click on a target preset's block opens it under that block: "Replace with <the chosen
// block>" (only while a source block is chosen and the swap breaks no firmware limit) and
// "Remove" (danger). It closes on a click anywhere else, or on the same block again. The
// block is drawn by the signal-path strip, which this can't wrap, so the menu is anchored
// to it through `anchor`.

import { Menu } from "../../ui/Menu";
import { MenuItem } from "../../ui/primitives";
import { COPY_TEXT, replaceWith } from "./copyText";

export interface BlockMenuProps {
  /** The open block's tile (stable). */
  anchor: () => Element | null;
  /** The chosen source block's name, when Replace is offered. */
  replaceName: string | null;
  onReplace: () => void;
  onRemove: () => void;
  onClose: () => void;
}

export function BlockMenu({
  anchor,
  replaceName,
  onReplace,
  onRemove,
  onClose,
}: BlockMenuProps) {
  return (
    // The menu's own DOM parent is an empty box, so a press anywhere but the menu and
    // its block counts as outside.
    <span style={{ display: "none" }}>
      <Menu anchor={anchor} align="start" onClose={onClose}>
        {replaceName != null && (
          <MenuItem label={replaceWith(replaceName)} onClick={onReplace} />
        )}
        <MenuItem label={COPY_TEXT.remove} danger onClick={onRemove} />
      </Menu>
    </span>
  );
}
