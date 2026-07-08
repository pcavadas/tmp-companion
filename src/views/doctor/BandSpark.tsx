// src/views/doctor/BandSpark.tsx — the inline six-band sparkline shown on every
// problem row: 17×46px of thin bars drawn from the sound's real per-band balance,
// with the row's hot band(s) filled in its worst-severity color. Presentational —
// reuses the full BandMeter's (db+30)/45 normalization at row scale. Flagged as a
// DS sign-off candidate (the one genuinely new visual).

const BANDS6 = [0, 1, 2, 3, 4, 5];
const FAINT = "rgba(15,17,21,0.14)";

export interface BandSparkProps {
  /** The sound's real six-band balance (dB), one entry per band. */
  balanceDb: number[];
  /** Union of the row's diagnoses' hot band indices — filled in `color`. */
  hotBands: number[];
  /** The row's worst-severity color (used for hot bars). */
  color: string;
  /** Clear rows draw every bar faint. */
  muted: boolean;
}

export function BandSpark({
  balanceDb,
  hotBands,
  color,
  muted,
}: BandSparkProps) {
  const hot = new Set(hotBands);
  return (
    <span
      aria-hidden
      title="Band balance"
      style={{
        display: "inline-flex",
        alignItems: "flex-end",
        gap: 1.5,
        height: 17,
        width: 46,
        flexShrink: 0,
      }}
    >
      {BANDS6.map((i) => {
        const db = i < balanceDb.length ? balanceDb[i] : -30;
        const frac = Math.max(0.08, Math.min(1, (db + 30) / 45));
        const on = !muted && hot.has(i);
        return (
          <span
            key={i}
            style={{
              flex: 1,
              height: `${String(frac * 100)}%`,
              borderRadius: "1.5px 1.5px 0 0",
              background: on ? color : FAINT,
            }}
          />
        );
      })}
    </span>
  );
}

export default BandSpark;
