// src/ui/blockart/pedalKnobs.tsx — the shared knob-row helper for every pedal MOTIF
// family. Kept out of ./pedals (a component file) so that file stays component-only
// for Fast Refresh; imported by ./pedals + the ./pedalsMotif* modules.
import { ptrColor, type PedalTone } from "./shared";

// knob row helper
export function knobs(
  c: PedalTone,
  edge: string,
  xs: number[],
  y: number,
  r: number,
) {
  return xs.map((x, i) => (
    <g key={i}>
      <circle
        cx={x}
        cy={y}
        r={r}
        fill={c.knob}
        stroke={edge}
        strokeWidth="0.5"
      />
      <line
        x1={x}
        y1={y}
        x2={x}
        y2={y - r + 0.6}
        stroke={ptrColor(c)}
        strokeWidth="0.7"
      />
    </g>
  ));
}
