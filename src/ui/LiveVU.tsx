// src/ui/LiveVU.tsx — a decorative live "VU" bar field for live-measuring surfaces.
// PURE DECORATION: the bars animate on a fixed CSS cadence and carry NO data — they only
// signal "a measurement is happening". Reduced motion freezes them flat (see the `.tmp-vu`
// rule in index.html). `aria-hidden` — there is nothing here for a screen reader to read.

import { useTheme } from "../theme/ThemeContext";

/** Fixed bar count / field height — bump here if a caller ever needs a denser field. */
const BAR_COUNT = 24;
const FIELD_HEIGHT = 22;

export function LiveVU() {
  const { t } = useTheme();
  return (
    <div
      aria-hidden
      style={{
        flex: 1,
        height: FIELD_HEIGHT,
        display: "flex",
        alignItems: "flex-end",
        gap: 2,
      }}
    >
      {Array.from({ length: BAR_COUNT }, (_, i) => (
        <span
          key={i}
          className="tmp-vu"
          style={{
            flex: 1,
            minWidth: 2,
            height: "100%",
            borderRadius: 1,
            transformOrigin: "bottom",
            background: `linear-gradient(to top, ${t.accentDeep}, ${t.accent})`,
            opacity: 0.55 + (i % 5) * 0.09,
            animationDuration: `${String(0.4 + (i % 6) * 0.045)}s`,
            animationDelay: `${String(-(i % 6) * 0.08)}s`,
          }}
        />
      ))}
    </div>
  );
}

export default LiveVU;
