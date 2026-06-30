// src/views/overlays/ByEarChip.tsx — the "by ear" chip, shared by the leveling wizard.
//
// One consistent chip (the cause distinction lives in the engine + the Summary
// footnote, never in the chip wording). Shown in the Set-up Run-option caveat, on
// flagged Summary rows, and in the Summary's reason-aware footnote.

import { useTheme } from "../../theme/ThemeContext";

export function ByEarChip() {
  const { t } = useTheme();
  return (
    <span
      style={{
        fontFamily: t.mono,
        fontSize: 8.5,
        letterSpacing: "0.04em",
        color: t.accentDeep,
        background: t.accentSoft,
        border: `0.5px solid ${t.accentBorder}`,
        borderRadius: 3,
        padding: "1px 5px",
        flexShrink: 0,
      }}
    >
      by ear
    </span>
  );
}

export default ByEarChip;
