// src/views/doctor/BandMeter.tsx — the opt-in "frequency picture": player-named
// bands (6 for guitar/bass, 7 with "Sub" for bass-vi) drawn as div bars from a
// sound's real per-band balance, with the problem band(s) filled in the severity
// color. Genuinely new visual — flagged as a DS sign-off candidate (kept local for
// now).

import { useTheme } from "../../theme/ThemeContext";
import { sevTone, type Sev } from "./severity";

// Map a per-band balance (dB) to a bar-height %. The capture's balanceDb spans
// roughly −30…+15 dB; project that onto 8…100% so a silent band still shows a stub
// and a hot band never overflows the 40px row. (db + 30) / 45 normalizes −30→0,
// +15→1; ×92 + 8 lands it in the 8…100 band, then clamp.
function barHeightPct(db: number): number {
  const pct = 8 + ((db + 30) / 45) * 92;
  return Math.max(8, Math.min(100, pct));
}

export interface BandMeterProps {
  /** The sound's real per-band balance (dB), one entry per band. */
  balanceDb: number[];
  /** Display labels for the sound's band layout (`DoctorSoundResult.bandLabels`) —
   *  6 for guitar/bass, 7 ("Sub" first) for bass-vi. Drives both the bar count and
   *  the axis labels below them. */
  bandLabels: string[];
  /** Problem band indices from the diagnosis — filled + ringed in the sev color. */
  bands: number[];
  sev: Sev;
}

export function BandMeter({
  balanceDb,
  bandLabels,
  bands,
  sev,
}: BandMeterProps) {
  const { t } = useTheme();
  const tone = sevTone(t, sev);
  const problem = new Set(bands);
  const faint = "rgba(15,17,21,0.10)";

  return (
    <div style={{ marginTop: 10 }}>
      <div
        style={{ display: "flex", alignItems: "flex-end", gap: 3, height: 40 }}
      >
        {bandLabels.map((label, i) => {
          const hot = problem.has(i);
          const db = i < balanceDb.length ? balanceDb[i] : -30;
          return (
            <div
              key={label}
              style={{
                flex: 1,
                height: `${String(barHeightPct(db))}%`,
                borderRadius: "3px 3px 1px 1px",
                background: hot ? tone.fg : faint,
                border: hot
                  ? `0.5px solid ${tone.fg}`
                  : "0.5px solid transparent",
                boxSizing: "border-box",
              }}
            />
          );
        })}
      </div>
      <div style={{ display: "flex", gap: 3, marginTop: 4 }}>
        {bandLabels.map((label, i) => (
          <span
            key={label}
            style={{
              flex: 1,
              textAlign: "center",
              fontFamily: t.mono,
              fontSize: 7.5,
              letterSpacing: "0.06em",
              textTransform: "uppercase",
              color: problem.has(i) ? tone.fg : t.faint,
              whiteSpace: "nowrap",
            }}
          >
            {label}
          </span>
        ))}
      </div>
    </div>
  );
}

export default BandMeter;
