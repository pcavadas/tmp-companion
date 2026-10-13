// src/views/overlays/PickWarnNote.tsx — a shared sub-line shape: warn-tri icon +
// message. Lives in its own neutral module because its callers span the picker and
// the setup rows: `FlatLevelPick`'s disabled/tone-risk candidate notes and its
// stale-pick menu footer, and `SetupPage`'s footswitch scene-context notes ("choose
// one" / "doesn't turn on in that scene").

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
