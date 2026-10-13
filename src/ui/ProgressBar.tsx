// src/ui/ProgressBar.tsx — a thin determinate progress bar (terracotta fill on a
// faint track). Used by the Presets-tab scan strip + the leveling setup/run bars.
// `percent` is clamped 0..100; the fill animates its width (respects the global
// reduced-motion preference via the same CSS the app already honours).

import { useTheme } from "../theme/ThemeContext";

export interface ProgressBarProps {
  /** 0..100 (clamped). */
  percent: number;
  /** Track height in px (default 4). */
  height?: number;
}

export function ProgressBar({ percent, height = 4 }: ProgressBarProps) {
  const { t } = useTheme();
  const pct = Math.max(0, Math.min(100, percent));
  return (
    <div
      style={{
        height,
        background: t.track,
        borderRadius: 999,
        overflow: "hidden",
      }}
    >
      <div
        style={{
          height: "100%",
          width: `${String(pct)}%`,
          background: t.accent,
          borderRadius: 999,
          transition: "width 0.4s ease",
        }}
      />
    </div>
  );
}

export default ProgressBar;
