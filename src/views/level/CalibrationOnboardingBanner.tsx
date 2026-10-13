// src/views/level/CalibrationOnboardingBanner.tsx — the dismissible Set up banner that
// points an uncalibrated instrument at Settings → Instruments → Calibrate.

import { useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";

export interface CalibrationOnboardingBannerProps {
  show: boolean;
}

/** Onboarding nudge toward Tier-2 calibration — shown once per wizard open, only
 *  while the bulk instrument is a real, uncalibrated profile. */
export function CalibrationOnboardingBanner({
  show,
}: CalibrationOnboardingBannerProps) {
  const { t } = useTheme();
  const [dismissed, setDismissed] = useState(false);
  if (!show || dismissed) return null;
  return (
    <div
      role="status"
      style={{
        flexShrink: 0,
        display: "flex",
        alignItems: "flex-start",
        gap: t.space4,
        padding: `${String(t.space4)}px ${String(t.space5)}px`,
        borderRadius: t.rCard,
        border: `0.5px solid ${t.hairlineStrong}`,
        background: t.bgAlt,
      }}
    >
      <span style={{ display: "flex", flexShrink: 0, marginTop: t.space1 }}>
        <Icon name="info" size={14} stroke={t.accentDeep} strokeWidth={1.5} />
      </span>
      <span
        style={{
          flex: 1,
          fontFamily: t.sans,
          fontSize: 12,
          lineHeight: 1.45,
          color: t.ink2,
        }}
      >
        Level with your own guitar. A 2-minute calibration makes leveling match
        your instrument. Find it in Settings → Instruments → Calibrate.
      </span>
      <button
        type="button"
        aria-label="Dismiss"
        title="Dismiss"
        onClick={() => {
          setDismissed(true);
        }}
        style={{
          cursor: "pointer",
          display: "flex",
          flexShrink: 0,
          background: "transparent",
          border: 0,
          padding: 0,
        }}
      >
        <Icon name="x" size={12} stroke={t.mutedInk} />
      </button>
    </div>
  );
}
