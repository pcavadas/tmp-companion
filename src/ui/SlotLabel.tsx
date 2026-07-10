// src/ui/SlotLabel.tsx — the DS mono slot-number cell.
//
// One tokenized span replacing the hand-rolled `{ fontFamily: t.mono, fontSize:
// t.fsData, color: t.mutedInk }` wrapper around `slotLabel()` re-rolled at several
// call sites. `faint` covers the empty-row variant; `style` covers per-site layout
// (width/flexShrink) — never re-declare the font/color triplet at a call site.

import type { CSSProperties } from "react";

import { useTheme } from "../theme/ThemeContext";
import { slotLabel } from "../lib/format";

export interface SlotLabelProps {
  /** 0-based list index — renders via slotLabel() (the shared formatter). */
  index: number;
  /** faint color (empty rows). */
  faint?: boolean;
  /** per-site layout escape (width/flexShrink); merged last. */
  style?: CSSProperties;
}

export function SlotLabel({ index, faint, style }: SlotLabelProps) {
  const { t } = useTheme();
  return (
    <span
      style={{
        fontFamily: t.mono,
        fontSize: t.fsData,
        color: faint ? t.faint : t.mutedInk,
        ...style,
      }}
    >
      {slotLabel(index)}
    </span>
  );
}
