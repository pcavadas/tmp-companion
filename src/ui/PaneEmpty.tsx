// src/ui/PaneEmpty.tsx — the DS medallion empty state for a detail pane.
//
// One tokenized "nothing here yet" panel replacing the two hand-rolled twins
// (SetlistDetail's "No songs in this setlist yet" + PresetDetail's "No songs
// use this preset"). Fills its parent (flex:1, minHeight:0) under a hairline,
// centering an outlined icon medallion over a serif title, a muted body, and an
// optional caller-composed CTA.

import type { ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";
import { Icon } from "./Icon";
import type { IconName } from "./iconNames";

export interface PaneEmptyProps {
  icon: IconName;
  title: string;
  body: ReactNode;
  /** optional CTA rendered under the body (caller composes the Button). */
  cta?: ReactNode;
}

export function PaneEmpty({ icon, title, body, cta }: PaneEmptyProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        borderTop: `0.5px solid ${t.hairline}`,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: 13,
        padding: "0 40px",
        textAlign: "center",
      }}
    >
      <span
        style={{
          width: 46,
          height: 46,
          borderRadius: 12,
          border: `0.5px solid ${t.hairlineStrong}`,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <Icon name={icon} size={20} stroke={t.faint} />
      </span>
      <div style={{ fontFamily: t.serif, fontSize: 18, color: t.ink2 }}>
        {title}
      </div>
      <div
        style={{
          fontFamily: t.sans,
          fontSize: t.fsBody,
          color: t.mutedInk,
          maxWidth: 320,
          lineHeight: 1.5,
        }}
      >
        {body}
      </div>
      {cta}
    </div>
  );
}
