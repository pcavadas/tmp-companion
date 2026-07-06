// src/ui/blockart/pedalsMotifPitch.tsx — pitch/utility pedal MOTIFs (octave/whammy/
// synth/fxloop), split from ./pedals so each file stays ≤500 lines. Returns the
// control-zone motif for `g`, or null if not in this family.
import type { PedalTone } from "./shared";
import { knobs } from "./pedalKnobs";
import { FONT_SANS } from "../../theme/tokens";

export function pitchMotif(
  g: string,
  c: PedalTone,
  edge: string,
  loopNum: string,
) {
  switch (g) {
    case "octave":
      return (
        <g>
          {knobs(c, edge, [22, 42], 16, 5)}
          <path
            d="M27 28 l0 -7 -2 0 3 -4 3 4 -2 0 0 7 z"
            fill={c.knob}
            opacity="0.8"
          />
          <path
            d="M37 11 l0 7 2 0 -3 4 -3 -4 2 0 0 -7 z"
            fill={c.knob}
            opacity="0.6"
          />
        </g>
      );
    case "whammy":
      return (
        <g>
          <polygon points="16,26 48,26 44,11 20,11" fill="rgba(0,0,0,0.22)" />
          <path
            d="M22 23 l18 -9 -3 -1 4 -2 1 4 -2 -0.5"
            fill="none"
            stroke={c.knob}
            strokeWidth="1.4"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </g>
      );
    case "synth":
      return (
        <g>
          <rect x="15" y="11" width="34" height="13" rx="1.5" fill="#0c0c10" />
          <path
            d="M17 17 h4 v-3 h4 v6 h4 v-4 h4 v3 h4 v-5 h4 v5 h2"
            fill="none"
            stroke="#7fd9c4"
            strokeWidth="1"
          />
        </g>
      );
    case "fxloop":
      return (
        <g>
          <path
            d="M18 14 h22 a4 4 0 0 1 0 8 h-9"
            fill="none"
            stroke={c.knob}
            strokeWidth="1.4"
          />
          <path
            d="M34 18 l-4 4 4 4"
            fill="none"
            stroke={c.knob}
            strokeWidth="1.4"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
          <path
            d="M46 26 h-22 a4 4 0 0 1 0 -8 h9"
            fill="none"
            stroke={c.knob}
            strokeWidth="1.4"
            opacity="0.55"
          />
          {/* loop number — the allowed FX-loop wordmark */}
          <text
            x="32"
            y="34"
            textAnchor="middle"
            fontFamily={FONT_SANS}
            fontSize="10"
            fontWeight="800"
            letterSpacing="0.04em"
            fill={c.knob}
          >
            {loopNum}
          </text>
        </g>
      );
    default:
      return null;
  }
}
