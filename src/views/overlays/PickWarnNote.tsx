// src/views/overlays/PickWarnNote.tsx — a shared sub-line shape: warn-tri icon +
// message. Moved out of `BlockParamRow.tsx` (D2/Part C) into its own neutral module
// once it grew a THIRD caller outside the param-row context — `BlockLevelPick`'s
// own hand-rolled warn-tri + `sevWarn` footers (the stale-block and stale-param
// notes inside its two dropdown menus) duplicated this exact shape rather than
// reusing it, and `SetupBody`'s "doesn't turn on in that scene" footswitch note
// already did. Current callers: `BlockPickRow`'s shared disabled-block reason,
// `BlockParamRow`'s disabled/tone-risk notes, `BlockLevelPick`'s two stale-pick
// menu footers, and `SetupBody`'s non-enabling-footswitch note.

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";

export function PickWarnNote({ children }: { children: ReactNode }) {
  const { t } = useTheme();
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: t.space3,
        fontFamily: t.sans,
        fontSize: 10.5,
        color: t.sevWarn,
      }}
    >
      <Icon name="warn-tri" size={10} stroke={t.sevWarn} strokeWidth={1.7} />
      {children}
    </span>
  );
}

export default PickWarnNote;
