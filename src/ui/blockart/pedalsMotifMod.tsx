// src/ui/blockart/pedalsMotifMod.tsx — modulation pedal MOTIFs (chorus/phaser/
// flanger/tremolo/rotary/univibe), split from ./pedals so each file stays ≤500
// lines. Returns the control-zone motif for `g`, or null if not in this family.
import type { PedalTone } from "./shared";
import { knobs } from "./pedalKnobs";

export function modMotif(g: string, c: PedalTone, edge: string, jewel: string) {
  switch (g) {
    case "chorus":
      return (
        <g>
          {knobs(c, edge, [22, 42], 15, 5)}
          <path
            d="M13 26 q4 -5 8 0 t8 0 t8 0 t8 0"
            fill="none"
            stroke={c.knob}
            strokeWidth="1.1"
            opacity="0.8"
          />
        </g>
      );
    case "phaser":
      return (
        <g>
          {knobs(c, edge, [32], 16, 6.5)}
          {[15, 20, 44, 49].map((x, i) => (
            <line
              key={i}
              x1={x}
              y1="11"
              x2={x}
              y2="25"
              stroke={c.knob}
              strokeWidth="1.2"
              opacity={0.4 + i * 0.12}
            />
          ))}
        </g>
      );
    case "flanger":
      return (
        <g>
          {knobs(c, edge, [22, 42], 15, 5)}
          {[14, 18, 23, 29, 36, 44].map((x, i) => (
            <line
              key={i}
              x1={x}
              y1="24"
              x2={x}
              y2={27}
              stroke={c.knob}
              strokeWidth="1"
              opacity="0.7"
            />
          ))}
          <path
            d="M14 24 Q30 18 50 24"
            fill="none"
            stroke={c.knob}
            strokeWidth="0.8"
            opacity="0.5"
          />
        </g>
      );
    case "tremolo":
      return (
        <g>
          {knobs(c, edge, [22, 42], 15, 5)}
          <path
            d="M17 25 q3 -7 6 0 t6 0 t6 0 t6 0 t6 0"
            fill="none"
            stroke={c.knob}
            strokeWidth="1.2"
            opacity="0.8"
          />
        </g>
      );
    case "rotary":
      return (
        <g>
          <ellipse
            cx="32"
            cy="17"
            rx="13"
            ry="9"
            fill="none"
            stroke={c.knob}
            strokeWidth="1.1"
            opacity="0.8"
          />
          <path
            d="M21 13 Q32 8 43 13"
            fill="none"
            stroke={c.knob}
            strokeWidth="1"
            opacity="0.6"
          />
          <path
            d="M21 21 Q32 26 43 21"
            fill="none"
            stroke={c.knob}
            strokeWidth="1"
            opacity="0.6"
          />
          <line
            x1="32"
            y1="8"
            x2="32"
            y2="26"
            stroke={c.knob}
            strokeWidth="0.8"
            opacity="0.45"
          />
        </g>
      );
    case "univibe":
      return (
        <g>
          <circle
            cx="32"
            cy="17"
            r="9"
            fill="none"
            stroke={c.knob}
            strokeWidth="1.1"
            opacity="0.8"
          />
          <circle cx="32" cy="17" r="3.4" fill={c.knob} opacity="0.55" />
          <circle cx="32" cy="9" r="1.4" fill={jewel} opacity="0.8" />
        </g>
      );
    default:
      return null;
  }
}
