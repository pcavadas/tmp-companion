// src/views/level/GuidanceBanner.tsx — one Summary guidance banner: a tone icon beside
// a wide kicker heading and a sentence (re-amp still on, amp at zero, expression pedal
// muting, needs routing, clamped).

import type { ReactNode } from "react";

import { useStyles, useTheme } from "../../theme/ThemeContext";
import { Icon, type IconName } from "../../ui/Icon";

export interface GuidanceBannerProps {
  icon: IconName;
  /** `warn` (red: nothing to re-level) or `sevWarn` (amber: re-levelable). */
  tone: "warn" | "sevWarn";
  heading: string;
  children: ReactNode;
}

export function GuidanceBanner({
  icon,
  tone,
  heading,
  children,
}: GuidanceBannerProps) {
  const { t } = useTheme();
  const s = useStyles();
  const [color, bg, border] =
    tone === "warn"
      ? [t.warn, t.warnSoft, t.warnBorder]
      : [t.sevWarn, t.sevWarnSoft, t.sevWarnBorder];
  return (
    <div
      style={{
        display: "flex",
        gap: t.space5,
        padding: `${String(t.space5)}px ${String(t.space6)}px`,
        borderRadius: 9,
        background: bg,
        border: `0.5px solid ${border}`,
      }}
    >
      <span style={{ flexShrink: 0, paddingTop: t.space1 }}>
        <Icon name={icon} size={15} stroke={color} strokeWidth={1.8} />
      </span>
      <div style={{ minWidth: 0 }}>
        <div style={{ ...s.kickerWide(color), marginBottom: t.space2 }}>
          {heading}
        </div>
        <div
          style={{
            fontFamily: t.sans,
            fontSize: 12.5,
            lineHeight: 1.5,
            color: t.ink2,
            textWrap: "pretty",
          }}
        >
          {children}
        </div>
      </div>
    </div>
  );
}
