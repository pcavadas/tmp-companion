// src/ui/LiveVU.tsx — a live "VU" bar field for leveling-capture surfaces.
// Data-driven: each bar's height is the corresponding hop's momentary level (dB) from the
// live-LUFS event trace, newest slot right-aligned. Still `aria-hidden` — the readout beside
// it carries the number for a screen reader; these bars are a visual level indicator only.

import { useTheme } from "../theme/ThemeContext";

/** Fixed slot count / field height — the trace is padded/trimmed to this width. */
const BAR_COUNT = 24;
const FIELD_HEIGHT = 22;

export interface LiveVUProps {
  /** Momentary levels in dB, newest last. Fewer than BAR_COUNT pads silent slots on the left. */
  values: number[];
}

/** Map a dB value to a 0..1 height fraction of the field. */
function heightFrac(db: number): number {
  const f = (db + 60) / 50;
  return Math.min(Math.max(f, 0.06), 1);
}

export function LiveVU({ values }: LiveVUProps) {
  const { t } = useTheme();
  // Right-align: trim to the newest BAR_COUNT hops, pad the left with silent slots.
  const slots = [
    ...Array<number>(Math.max(0, BAR_COUNT - values.length)).fill(-Infinity),
    ...values.slice(-BAR_COUNT),
  ];
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
      {slots.map((db, i) => (
        <span
          key={i}
          style={{
            flex: 1,
            minWidth: 2,
            height: `${String(heightFrac(db) * 100)}%`,
            borderRadius: 1,
            background: `linear-gradient(to top, ${t.accentDeep}, ${t.accent})`,
            opacity: 0.55 + (i % 5) * 0.09,
          }}
        />
      ))}
    </div>
  );
}

export default LiveVU;
