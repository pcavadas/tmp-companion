// src/ui/SlotLabel.tsx — the DS slot-number cell (look B): 12 px Inter, `textSecondary`,
// tabular numbers (no monospace font).
//
// One tokenized span around `slotLabel()` re-rolled at several call sites. `faint`
// covers the empty-row variant (`textDisabled`); `style` covers per-site layout (a
// 30 px column in a look B Row, flexShrink) — never re-declare the font/colour at a
// call site.

import type { CSSProperties } from "react";

import { useTheme } from "../theme/ThemeContext";
import { slotLabel } from "../lib/format";

export interface SlotLabelProps {
  /** 0-based list index — renders via slotLabel() (the shared formatter). */
  index: number;
  /** disabled colour (empty rows). */
  faint?: boolean;
  /** per-site layout escape (width/flexShrink); merged last. */
  style?: CSSProperties;
}

export function SlotLabel({ index, faint, style }: SlotLabelProps) {
  const { t } = useTheme();
  return (
    <span
      style={{
        fontFamily: t.fontSans,
        fontSize: t.fontCaption,
        color: faint ? t.textDisabled : t.textSecondary,
        fontVariantNumeric: "tabular-nums",
        ...style,
      }}
    >
      {slotLabel(index)}
    </span>
  );
}
