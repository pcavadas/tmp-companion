// src/ui/blockart/pedalsMotifConcept.tsx — the 1.8 Fender-designed concept pedal
// MOTIFs (steptrem/stepfilter/stepfilterdelay/pitchseq/octslider/prismdelay/
// spectralverb/cirrusverb/cirrussynthverb) + the DEFAULT fallback, split from
// ./pedals so each file stays ≤500 lines. This family is the terminal dispatch:
// its `default` returns the generic two-knob motif for any unknown icon id.
import type { PedalTone } from "./shared";
import { knobs } from "./pedalKnobs";

export function conceptMotif(
  g: string,
  c: PedalTone,
  edge: string,
  jewel: string,
) {
  switch (g) {
    // ---- 1.8 Fender-designed concept motifs (abstract marks on the standard
    // enclosure; one knob + the marque in the control zone) -------------------
    case "steptrem":
      // staircase-stepped square-wave LFO
      return (
        <g>
          {knobs(c, edge, [32], 13, 4.4)}
          <path
            d="M14 26 V22 H19 V18 H24 V26 H29 V22 H34 V18 H39 V26 H44 V22 H49"
            fill="none"
            stroke={c.knob}
            strokeWidth="1.1"
            opacity="0.85"
            strokeLinejoin="round"
          />
        </g>
      );
    case "stepfilter":
      // staircase carving through a frequency-sweep curve
      return (
        <g>
          {knobs(c, edge, [32], 13, 4.4)}
          <path
            d="M14 26 Q26 26 30 19 Q34 12 50 12"
            fill="none"
            stroke={c.knob}
            strokeWidth="0.9"
            opacity="0.5"
          />
          <path
            d="M15 25 V22 H20 V19 H25 V16 H30 V13"
            fill="none"
            stroke={c.knob}
            strokeWidth="1.1"
            opacity="0.85"
            strokeLinejoin="round"
          />
        </g>
      );
    case "stepfilterdelay":
      // staircase + receding echo dots
      return (
        <g>
          {knobs(c, edge, [32], 13, 4.4)}
          <path
            d="M14 26 V22 H18 V18 H22 V26 H26"
            fill="none"
            stroke={c.knob}
            strokeWidth="1.1"
            opacity="0.85"
            strokeLinejoin="round"
          />
          {[30, 36, 42, 48].map((x, i) => (
            <circle
              key={i}
              cx={x}
              cy="22"
              r={1.8 - i * 0.35}
              fill={c.knob}
              opacity={0.8 - i * 0.16}
            />
          ))}
        </g>
      );
    case "pitchseq":
      // 4 vertical step sliders (ref: Pitch Sequencer — four coloured faders)
      return (
        <g>
          {[20, 28, 36, 44].map((x, i) => (
            <g key={i}>
              <line
                x1={x}
                y1="10"
                x2={x}
                y2="28"
                stroke="rgba(0,0,0,0.3)"
                strokeWidth="0.7"
              />
              <rect
                x={x - 1.9}
                y={11 + i * 3.6}
                width="3.8"
                height="2.6"
                rx="0.6"
                fill={c.knob}
                stroke={edge}
                strokeWidth="0.35"
              />
            </g>
          ))}
        </g>
      );
    case "octslider":
      // 8 vertical octave faders (ref: POLYGON OCTAVE SHIFTER — Orig/sub/up/HC)
      return (
        <g>
          {Array.from({ length: 8 }).map((_, i) => {
            const x = 14 + i * 5.1;
            return (
              <g key={i}>
                <line
                  x1={x}
                  y1="10"
                  x2={x}
                  y2="28"
                  stroke="rgba(0,0,0,0.3)"
                  strokeWidth="0.55"
                />
                <rect
                  x={x - 1.4}
                  y={12 + ((i * 3) % 5) * 2.4}
                  width="2.8"
                  height="2.2"
                  rx="0.5"
                  fill={c.knob}
                  stroke={edge}
                  strokeWidth="0.3"
                />
              </g>
            );
          })}
        </g>
      );
    case "prismdelay":
      // a prism splitting one beam into a fan of fading echoes
      return (
        <g>
          <path
            d="M22 24 L29 12 L36 24 Z"
            fill="none"
            stroke={c.knob}
            strokeWidth="1"
            opacity="0.8"
          />
          <line
            x1="14"
            y1="20"
            x2="25"
            y2="20"
            stroke={c.knob}
            strokeWidth="1.1"
            opacity="0.8"
          />
          {[16, 19, 22].map((y, i) => (
            <line
              key={i}
              x1="34"
              y1="19"
              x2="50"
              y2={y}
              stroke={c.knob}
              strokeWidth="0.9"
              opacity={0.75 - i * 0.2}
            />
          ))}
        </g>
      );
    case "spectralverb":
      // ghostly shimmer / aurora rising from a note tail
      return (
        <g>
          <line
            x1="22"
            y1="27"
            x2="22"
            y2="16"
            stroke={c.knob}
            strokeWidth="1.4"
            strokeLinecap="round"
            opacity="0.85"
          />
          <circle cx="22" cy="14.5" r="1.5" fill={c.knob} opacity="0.85" />
          {[0, 1, 2].map((i) => (
            <path
              key={i}
              d={`M26 ${String(24 - i * 4)} q8 -6 16 0`}
              fill="none"
              stroke={jewel}
              strokeWidth="1"
              opacity={0.8 - i * 0.22}
            />
          ))}
        </g>
      );
    case "cirrusverb":
      // high wispy cirrus-cloud sheet
      return (
        <g>
          {[0, 1, 2, 3].map((i) => (
            <path
              key={i}
              d={`M${String(14 + (i % 2) * 3)} ${String(14 + i * 3)} q6 -3 12 0 t12 0`}
              fill="none"
              stroke={c.knob}
              strokeWidth="0.9"
              opacity={0.8 - i * 0.13}
              strokeLinecap="round"
            />
          ))}
        </g>
      );
    case "cirrussynthverb":
      // cirrus cloud sheet + a glowing synth waveform woven through
      return (
        <g>
          {[0, 1].map((i) => (
            <path
              key={i}
              d={`M14 ${String(15 + i * 4)} q6 -3 12 0 t12 0`}
              fill="none"
              stroke={c.knob}
              strokeWidth="0.9"
              opacity={0.7 - i * 0.18}
              strokeLinecap="round"
            />
          ))}
          <path
            d="M14 24 q3 -6 6 0 t6 0 t6 0 t6 0 t6 0"
            fill="none"
            stroke={jewel}
            strokeWidth="1.1"
            opacity="0.85"
          />
        </g>
      );
    default:
      return <g>{knobs(c, edge, [22, 42], 18, 5.4)}</g>;
  }
}
