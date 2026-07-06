// src/ui/blockart/pedalsMotifTime.tsx — time/reverb pedal MOTIFs (delay/spring/
// plate/hall/shimmer), split from ./pedals so each file stays ≤500 lines. Returns
// the control-zone motif for `g`, or null if not in this family.
import type { PedalTone } from "./shared";
import { knobs } from "./pedalKnobs";

export function timeMotif(
  g: string,
  c: PedalTone,
  edge: string,
  jewel: string,
) {
  switch (g) {
    case "delay":
      return (
        <g>
          {knobs(c, edge, [20, 32, 44], 14, 4.2)}
          {[20, 28, 36, 44].map((x, i) => (
            <line
              key={i}
              x1={x}
              y1={27 - (3 - i) * 1.6}
              x2={x}
              y2="27"
              stroke={c.knob}
              strokeWidth="1.6"
              opacity={0.85 - i * 0.18}
              strokeLinecap="round"
            />
          ))}
        </g>
      );
    case "spring":
      return (
        <g>
          <rect
            x="14"
            y="11"
            width="36"
            height="15"
            rx="2"
            fill="rgba(0,0,0,0.2)"
          />
          {[15, 19].map((y, k) => (
            <path
              key={k}
              d={`M18 ${String(y)} q2 -3 4 0 t4 0 t4 0 t4 0 t4 0 t4 0 t4 0`}
              fill="none"
              stroke={c.knob}
              strokeWidth="0.9"
              opacity="0.8"
            />
          ))}
        </g>
      );
    case "plate":
      return (
        <g>
          <rect
            x="15"
            y="10"
            width="34"
            height="17"
            rx="1.5"
            fill="rgba(0,0,0,0.22)"
            stroke={c.knob}
            strokeWidth="0.6"
            opacity="0.85"
          />
          {[0, 1, 2, 3, 4].map((i) => (
            <line
              key={i}
              x1={17 + i * 6}
              y1="26"
              x2={23 + i * 6}
              y2="11"
              stroke={c.knob}
              strokeWidth="0.7"
              opacity="0.5"
            />
          ))}
        </g>
      );
    case "hall":
      return (
        <g>
          {[5, 9, 13].map((rr, i) => (
            <path
              key={i}
              d={`M${String(32 - rr)} 25 a${String(rr)} ${String(rr)} 0 0 1 ${String(rr * 2)} 0`}
              fill="none"
              stroke={c.knob}
              strokeWidth="1.1"
              opacity={0.85 - i * 0.22}
            />
          ))}
        </g>
      );
    case "shimmer":
      return (
        <g>
          {[6, 11].map((rr, i) => (
            <path
              key={i}
              d={`M${String(32 - rr)} 26 a${String(rr)} ${String(rr)} 0 0 1 ${String(rr * 2)} 0`}
              fill="none"
              stroke={c.knob}
              strokeWidth="1.1"
              opacity={0.8 - i * 0.25}
            />
          ))}
          <path
            d="M32 8 l1.4 3 3 1.4 -3 1.4 -1.4 3 -1.4 -3 -3 -1.4 3 -1.4 z"
            fill={jewel}
            opacity="0.85"
          />
        </g>
      );
    default:
      return null;
  }
}
