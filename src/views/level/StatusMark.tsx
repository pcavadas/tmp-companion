// src/views/level/StatusMark.tsx — the 16 px status mark at the left of a run or Summary
// row (look B): done is a `good` circle with a white check; listening an 8 px `accent`
// dot that pulses (`tmp-listen`, still under reduced motion); waiting an 8 px hollow
// `meter` dot; a problem an 8 px solid dot (`cautionLine` or `danger`). Always beside
// words that say the same thing, so it is hidden from assistive tech.

import { useTheme } from "../../theme/ThemeContext";
import { Dot } from "../../ui/Dot";
import { Icon } from "../../ui/Icon";
import type { StatusMarkKind } from "./runModel";

export interface StatusMarkProps {
  kind: StatusMarkKind;
}

export function StatusMark({ kind }: StatusMarkProps) {
  const { t } = useTheme();
  if (kind === "done")
    return (
      <span
        aria-hidden
        style={{
          width: 16,
          height: 16,
          flexShrink: 0,
          borderRadius: t.radiusPill,
          background: t.good,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <Icon name="check" size={11} stroke={t.onAction} strokeWidth={3} />
      </span>
    );
  return (
    <span
      aria-hidden
      style={{
        width: 16,
        height: 16,
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      {kind === "listening" ? (
        <span
          className="tmp-listen"
          style={{
            width: 8,
            height: 8,
            borderRadius: t.radiusPill,
            background: t.accent,
          }}
        />
      ) : kind === "waiting" ? (
        <Dot tone="neutral" hollow />
      ) : (
        <Dot tone={kind} />
      )}
    </span>
  );
}
