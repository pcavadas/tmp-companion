// src/views/doctor/LevelIndicator.tsx — the playback-level indicator: three venue
// pictographs (headphones → combo amp → stage stack) at ascending sizes, lit in the
// finding's severity colour where it fires, dim outline where it doesn't. It shows
// which playback levels a finding bites at — quiet, at rehearsal, or turned up to a
// gig — because equal-loudness makes a tone read boomier/harsher the louder you
// monitor it. Firing is monotonic ("this level and up"), so a finding's set is
// always one of {stage}, {rehearsal, stage}, or all three.
//
// Genuinely new visual — flagged as a DS sign-off candidate (kept local, like
// BandMeter/BandSpark). Not an Icon-catalog glyph: Icon is a monochrome
// stroke-only primitive and can't express these fill/stroke states.
//
// This is a READ-ONLY render of a finding's `fromLevel`. It never reads or writes
// the Settings `playback_level` store — the Doctor diagnoses all three levels at
// once, independent of the room level chosen for leveling.

import { useTheme } from "../../theme/ThemeContext";
import { sevTone, type Sev } from "./severity";
import type { PlaybackLevel } from "../../lib/types";

const DIM_LINE = "rgba(15,17,21,0.32)"; // ponytail: one new value, inline like BandSpark's FAINT; promote to a token only if reused elsewhere

type LevelState = "stage" | "rehearsal" | "all" | "clean";

/** A finding's `fromLevel` (the quietest level it fires at) → indicator state. The
 *  ordinal already encodes the monotonic "this level and up" suffix, so only
 *  `quiet` (fires everywhere) remaps. `"clean"` is the all-clear row (no finding). */
function stateFor(level: PlaybackLevel | "clean"): LevelState {
  return level === "clean" ? "clean" : level === "quiet" ? "all" : level;
}

const LEVEL_NAMES = ["Quiet", "Rehearsal", "Stage"] as const;
const VENUE_KINDS = ["quiet", "rehearsal", "stage"] as const;
const VENUE_SIZE: Record<"tiny" | "rich", [number, number, number]> = {
  tiny: [14, 16, 18],
  rich: [21, 23, 25],
};

// Which of [quiet, rehearsal, stage] the finding fires at, plus the accessible
// phrase. `clean` lights every venue green and reads "clean at every volume".
const STATES: Record<
  LevelState,
  { fire: [boolean, boolean, boolean]; aria: (f: string) => string }
> = {
  stage: {
    fire: [false, false, true],
    aria: (f) => `${f} only at stage volume`,
  },
  rehearsal: {
    fire: [false, true, true],
    aria: (f) => `${f} at rehearsal volume and up`,
  },
  all: { fire: [true, true, true], aria: (f) => `${f} at any volume` },
  // clean lights every venue (it's healthy at every level); the caller passes
  // sev "ok" so the lit colour is green.
  clean: { fire: [true, true, true], aria: () => "clean at every volume" },
};

interface VenueGlyphProps {
  kind: (typeof VENUE_KINDS)[number];
  size: number;
  on: boolean;
  col: string;
}

function VenueGlyph({ kind, size, on, col }: VenueGlyphProps) {
  const stroke = on ? col : DIM_LINE;
  const fill = on ? col : "none";
  const detail = on ? "#fff" : DIM_LINE;
  const c = {
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke,
    strokeWidth: 1.5,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    style: { display: "block" },
  };
  if (kind === "quiet") {
    // headphones — private / bedroom volume
    return (
      <svg {...c}>
        <path d="M5 13.5v-1.5a7 7 0 0 1 14 0v1.5" />
        <rect
          x="3.4"
          y="13"
          width="4"
          height="6.5"
          rx="1.5"
          fill={fill}
          stroke={stroke}
        />
        <rect
          x="16.6"
          y="13"
          width="4"
          height="6.5"
          rx="1.5"
          fill={fill}
          stroke={stroke}
        />
      </svg>
    );
  }
  if (kind === "rehearsal") {
    // combo amp — practice-room volume
    return (
      <svg {...c}>
        <rect
          x="4.5"
          y="4"
          width="15"
          height="16"
          rx="1.8"
          fill={fill}
          stroke={stroke}
        />
        <circle cx="12" cy="13.5" r="3.6" fill="none" stroke={detail} />
        <circle cx="7.5" cy="7" r="0.7" fill={detail} stroke="none" />
        <circle cx="16.5" cy="7" r="0.7" fill={detail} stroke="none" />
      </svg>
    );
  }
  // stage stack — gig / PA volume
  return (
    <svg {...c}>
      <rect
        x="6"
        y="2.6"
        width="12"
        height="8.4"
        rx="1.5"
        fill={fill}
        stroke={stroke}
      />
      <rect
        x="6"
        y="11.6"
        width="12"
        height="9.8"
        rx="1.5"
        fill={fill}
        stroke={stroke}
      />
      <circle cx="12" cy="6.8" r="2" fill="none" stroke={detail} />
      <circle cx="12" cy="16.4" r="2.6" fill="none" stroke={detail} />
    </svg>
  );
}

export interface LevelIndicatorProps {
  /** The quietest playback level the finding fires at, or `"clean"` for the
   *  all-clear row. */
  level: PlaybackLevel | "clean";
  /** Picks the lit colour via `sevTone`; the all-clear row passes `"ok"` (green). */
  sev: Sev;
  /** The finding label, used only to compose the aria-label/title. */
  finding: string;
  /** `tiny` = the collapsed triage row (label-less); `rich` = the expanded header
   *  (Quiet/Rehearsal/Stage captions). */
  size?: "tiny" | "rich";
}

export function LevelIndicator({
  level,
  sev,
  finding,
  size = "tiny",
}: LevelIndicatorProps) {
  const { t } = useTheme();
  const st = STATES[stateFor(level)];
  const litCol = sevTone(t, sev).fg;
  const sizes = VENUE_SIZE[size];
  const aria = st.aria(finding);
  const rich = size === "rich";

  // One render: three glyphs lit where the finding fires. `rich` (expanded
  // header) stacks a Quiet/Rehearsal/Stage caption under each; `tiny` (collapsed
  // triage row) is label-less.
  return (
    <span
      role="img"
      aria-label={aria}
      title={aria}
      style={{
        display: "inline-flex",
        alignItems: "flex-end",
        gap: rich ? 11 : 5,
        flexShrink: 0,
      }}
    >
      {VENUE_KINDS.map((k, i) => {
        const on = st.fire[i];
        const glyph = (
          <VenueGlyph key={k} kind={k} size={sizes[i]} on={on} col={litCol} />
        );
        if (!rich) return glyph;
        return (
          <span
            key={k}
            style={{
              display: "inline-flex",
              flexDirection: "column",
              alignItems: "center",
              gap: 5,
            }}
          >
            <span
              style={{ display: "flex", alignItems: "flex-end", height: 26 }}
            >
              {glyph}
            </span>
            <span
              style={{
                fontFamily: t.mono,
                fontSize: 7.5,
                letterSpacing: "0.04em",
                textTransform: "uppercase",
                color: on ? litCol : t.faint,
                lineHeight: 1,
              }}
            >
              {LEVEL_NAMES[i]}
            </span>
          </span>
        );
      })}
    </span>
  );
}

export default LevelIndicator;
