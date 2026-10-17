// src/views/doctor/BandMeter.tsx — "Tone by band": a sound's six bands (seven, with "Sub"
// first, for Bass VI) as bars from its real per-band balance, the finding's bands in the
// finding's tone (danger or caution) and the others in the neutral `meter` grey, each
// labelled underneath in 12 px words. 300 px wide, shown only inside an opened sound. The
// bars are a picture; the accessible name says what they show ("Tone by band: Low mids
// too strong").

import { useTheme } from "../../theme/ThemeContext";
import type { DoctorBandDirection } from "../../lib/types";
import type { FindingTone } from "./severity";

const DIRECTION_WORDS: Record<DoctorBandDirection, string> = {
  strong: "too strong",
  weak: "too weak",
};

/** The meter's width (px). */
const METER_W = 300;
/** The bars' full height (px). */
const BAR_H = 40;

/** "Low mids", "Highs and Air", "Lows, Mids and Air". */
function listWords(words: string[]): string {
  const last = words.slice(-1).join("");
  const rest = words.slice(0, -1);
  return rest.length === 0 ? last : `${rest.join(", ")} and ${last}`;
}

/** The meter's accessible name: the finding's bands and which way they're
 *  off ("Tone by band: Low mids too strong"). */
function meterLabel(
  bandLabels: string[],
  bands: number[],
  direction: DoctorBandDirection,
): string {
  const hot = bands
    .filter((i) => i >= 0 && i < bandLabels.length)
    .map((i) => bandLabels[i]);
  if (hot.length === 0) return "Tone by band";
  return `Tone by band: ${listWords(hot)} ${DIRECTION_WORDS[direction]}`;
}

// Map a per-band balance (dB) to a bar-height %. The capture's balanceDb spans
// roughly −30…+15 dB; project that onto 8…100% so a silent band still shows a stub
// and a hot band never overflows the bar. (db + 30) / 45 normalizes −30→0,
// +15→1; ×92 + 8 lands it in the 8…100 band, then clamp.
function barHeightPct(db: number): number {
  const pct = 8 + ((db + 30) / 45) * 92;
  return Math.max(8, Math.min(100, pct));
}

export interface BandMeterProps {
  /** The sound's real per-band balance (dB), one entry per band. */
  balanceDb: number[];
  /** Display labels for the sound's band layout (`DoctorSoundResult.bandLabels`) —
   *  6 for guitar/bass, 7 ("Sub" first) for bass-vi. */
  bandLabels: string[];
  /** The finding's band indices — drawn in its tone. */
  bands: number[];
  /** Whether those bands are too strong or too weak (`DoctorDiag.direction`). */
  direction: DoctorBandDirection;
  tone: FindingTone;
}

export function BandMeter({
  balanceDb,
  bandLabels,
  bands,
  direction,
  tone,
}: BandMeterProps) {
  const { t } = useTheme();
  const hotColor = tone === "danger" ? t.danger : t.cautionLine;
  const hot = new Set(bands);

  return (
    <div
      role="img"
      aria-label={meterLabel(bandLabels, bands, direction)}
      style={{
        display: "flex",
        gap: t.sp6,
        width: METER_W,
        maxWidth: "100%",
        flexShrink: 0,
      }}
    >
      {bandLabels.map((label, i) => {
        const db = i < balanceDb.length ? balanceDb[i] : -30;
        return (
          <span
            key={label}
            style={{
              flex: 1,
              minWidth: 0,
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              gap: t.sp4,
            }}
          >
            <span
              style={{
                width: "100%",
                height: BAR_H,
                display: "flex",
                alignItems: "flex-end",
              }}
            >
              <span
                data-band-bar={label}
                style={{
                  display: "block",
                  width: "100%",
                  height: `${String(barHeightPct(db))}%`,
                  background: hot.has(i) ? hotColor : t.meter,
                  borderRadius: 2,
                }}
              />
            </span>
            <span
              style={{
                fontSize: t.fontCaption,
                lineHeight: `${String(t.lineCaption)}px`,
                color: t.textSecondary,
                whiteSpace: "nowrap",
              }}
            >
              {label}
            </span>
          </span>
        );
      })}
    </div>
  );
}
