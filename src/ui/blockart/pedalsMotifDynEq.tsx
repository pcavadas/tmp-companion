// src/ui/blockart/pedalsMotifDynEq.tsx — dynamics/EQ/filter pedal MOTIFs
// (comp/gate/vol/eq*/peq/wah/envf), split from ./pedals so each file stays ≤500
// lines. Returns the control-zone motif for `g`, or null if not in this family.
import type { PedalTone } from "./shared";
import { knobs } from "./pedalKnobs";

export function dynEqMotif(
  g: string,
  c: PedalTone,
  edge: string,
  jewel: string,
) {
  switch (g) {
    case "comp":
      return (
        <g>
          {knobs(c, edge, [22, 42], 17, 5)}
          <rect
            x="20"
            y="25"
            width="24"
            height="3"
            rx="1.5"
            fill="rgba(0,0,0,0.25)"
          />
          <rect
            x="20"
            y="25"
            width="14"
            height="3"
            rx="1.5"
            fill={jewel}
            opacity="0.8"
          />
        </g>
      );
    case "gate":
      return (
        <g>
          {knobs(c, edge, [22, 42], 16, 5)}
          {[24, 28, 32, 36, 40].map((x, i) => (
            <rect
              key={i}
              x={x}
              y={24 - [2, 5, 3, 1, 0][i]}
              width="2"
              height={[4, 9, 6, 3, 1.5][i]}
              rx="0.6"
              fill={c.knob}
              opacity="0.7"
            />
          ))}
        </g>
      );
    case "vol":
      return (
        <g>
          <rect
            x="17"
            y="10"
            width="30"
            height="16"
            rx="2"
            fill="rgba(0,0,0,0.2)"
          />
          <path
            d="M19 24 L45 12"
            stroke={c.knob}
            strokeWidth="2"
            strokeLinecap="round"
          />
          <circle cx="45" cy="12" r="2" fill={c.knob} />
        </g>
      );
    case "eq":
    case "eq5":
    case "eq7":
    case "eq10": {
      const n = g === "eq5" ? 5 : g === "eq10" ? 10 : 7;
      // span fits inside the 42-wide body (x 11..53) with margin — the 10-band
      // case is the tightest, so the thumbs never bleed past the enclosure edge.
      const span = 32,
        x0 = 32 - span / 2,
        step = span / (n - 1),
        fw = Math.min(4.4, step - 0.6);
      return (
        <g>
          {Array.from({ length: n }).map((_, i) => {
            const x = x0 + step * i;
            return (
              <g key={i}>
                <line
                  x1={x}
                  y1="11"
                  x2={x}
                  y2="27"
                  stroke="rgba(0,0,0,0.3)"
                  strokeWidth="0.6"
                />
                <rect
                  x={x - fw / 2}
                  y={13 + ((i * 2) % 4) * 3}
                  width={fw}
                  height="2.8"
                  rx="0.5"
                  fill={c.knob}
                  stroke={edge}
                  strokeWidth="0.35"
                />
              </g>
            );
          })}
        </g>
      );
    }
    case "peq":
      return (
        <g>
          {knobs(c, edge, [19, 32, 45], 15, 4.4)}
          <path
            d="M14 27 Q26 27 28 21 Q30 15 32 21 Q34 27 50 27"
            fill="none"
            stroke={c.knob}
            strokeWidth="1"
            opacity="0.75"
          />
        </g>
      );
    case "wah":
      return (
        <g>
          <polygon points="16,26 48,26 44,11 20,11" fill="rgba(0,0,0,0.22)" />
          <path
            d="M20 24 L44 13"
            stroke={c.knob}
            strokeWidth="2"
            strokeLinecap="round"
          />
        </g>
      );
    case "envf":
      return (
        <g>
          {knobs(c, edge, [22, 42], 15, 5)}
          <path
            d="M14 27 C20 27 20 14 26 14 C32 14 32 27 50 27"
            fill="none"
            stroke={c.knob}
            strokeWidth="1.1"
            opacity="0.8"
          />
        </g>
      );
    default:
      return null;
  }
}
