// src/ui/LoudnessMeter.tsx — the run screen's one live meter (look B).
//
// A 340×4 `track` with an `accent` fill for the loudness now, a 2 px `text` tick at the
// target with the target's name centred under it (12/600), "Quieter" at the left end and
// "Louder" at the right. No dB or LUFS on screen (voice rule 2): the meter shows where
// the sound is against its target, not a number. `role="meter"`; the value is the fill
// as a % of the bar, and `aria-valuetext` says where it is against the target in words.
//
// The bar maps `min`..`max` LUFS. The default range holds every target the Settings
// slider can set (−29 to −13) with 3 LU of room on both sides; a caller that knows the
// saved targets can pass a tighter range.

import { useTheme } from "../theme/ThemeContext";

export interface LoudnessMeterProps {
  /** The loudness now (LUFS); null before the first reading. */
  lufs: number | null;
  /** The target's loudness (LUFS). */
  target: number;
  /** The target's name, shown under the tick ("Crunch"). */
  targetName: string;
  /** The sound being measured, for the accessible name ("Crunch"). */
  soundName: string;
  /** The bar's range in LUFS. Default −32 … −10. */
  min?: number;
  max?: number;
  /** Bar width in px. Default 340. */
  width?: number;
}

/** Within this many LU of the target, the reading is "close to" it. */
const NEAR_LU = 1;

function pctOf(v: number, min: number, max: number): number {
  if (max <= min) return 0;
  return Math.max(0, Math.min(100, ((v - min) / (max - min)) * 100));
}

export function LoudnessMeter({
  lufs,
  target,
  targetName,
  soundName,
  min = -32,
  max = -10,
  width = 340,
}: LoudnessMeterProps) {
  const { t } = useTheme();
  const now = lufs == null ? 0 : Math.round(pctOf(lufs, min, max));
  const tick = pctOf(target, min, max);
  const where =
    lufs == null
      ? "Not heard yet"
      : Math.abs(lufs - target) <= NEAR_LU
        ? `Close to ${targetName}`
        : lufs < target
          ? `Quieter than ${targetName}`
          : `Louder than ${targetName}`;

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: t.sp4,
        width,
        maxWidth: "100%",
        fontFamily: t.fontSans,
      }}
    >
      <div
        role="meter"
        aria-label={`Loudness of ${soundName} now, against its target`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={now}
        aria-valuetext={where}
        style={{ position: "relative", height: 22 }}
      >
        <span
          style={{
            position: "absolute",
            left: 0,
            right: 0,
            top: 9,
            height: 4,
            borderRadius: 2,
            background: t.track,
          }}
        />
        <span
          style={{
            position: "absolute",
            left: 0,
            width: `${String(now)}%`,
            top: 9,
            height: 4,
            borderRadius: 2,
            background: t.accent,
          }}
        />
        <span
          style={{
            position: "absolute",
            left: `calc(${String(tick)}% - 1px)`,
            top: 2,
            width: 2,
            height: 18,
            background: t.text,
          }}
        />
      </div>
      <div
        style={{
          position: "relative",
          height: t.lineCaption,
          fontSize: t.fontCaption,
          lineHeight: `${String(t.lineCaption)}px`,
          color: t.textSecondary,
        }}
      >
        <span style={{ position: "absolute", left: 0 }}>Quieter</span>
        <span
          style={{
            position: "absolute",
            left: `calc(${String(tick)}% - 40px)`,
            width: 80,
            textAlign: "center",
            color: t.text,
            fontWeight: t.weightSemibold,
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {targetName}
        </span>
        <span style={{ position: "absolute", right: 0 }}>Louder</span>
      </div>
    </div>
  );
}
