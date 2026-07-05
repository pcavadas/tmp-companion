// src/ui/blockart/pedalsMotifDrive.tsx — drive/distortion/fuzz pedal MOTIFs
// (knob1/boost/knobs2-6/od/dist/fuzz/bigmuff), split from ./pedals so each file
// stays ≤500 lines. Returns the control-zone motif for a given `g`, or null if the
// icon isn't in this family. Byte-identical to the original PedalBody switch cases.
import { lum, type PedalTone } from "./shared";
import { knobs } from "./pedalKnobs";

export function driveMotif(
  g: string,
  c: PedalTone,
  edge: string,
  jewel: string,
  lab: string,
) {
  switch (g) {
    case "knob1":
    case "boost":
      return <g>{knobs(c, edge, [32], 18, 7)}</g>;
    case "knobs2":
      return <g>{knobs(c, edge, [22, 42], 18, 5.4)}</g>;
    case "knobs3":
    case "od":
      return <g>{knobs(c, edge, [19, 32, 45], 18, 4.6)}</g>;
    case "knobs4":
      return <g>{knobs(c, edge, [16.5, 26.8, 37.2, 47.5], 18, 4.2)}</g>;
    case "knobs6":
      return (
        <g>
          {knobs(c, edge, [19, 32, 45], 13, 3.6)}
          {knobs(c, edge, [19, 32, 45], 24, 3.6)}
        </g>
      );
    case "dist":
      return (
        <g>
          {knobs(c, edge, [18, 28, 38, 47], 17, 3.8)}
          <path
            d="M17 26 l4 -4 3 5 3 -6 3 6 3 -5 3 4"
            fill="none"
            stroke={c.knob}
            strokeWidth="1"
            opacity="0.7"
          />
        </g>
      );
    case "fuzz":
      return (
        <g>
          {knobs(c, edge, [23, 41], 17, 6)}
          <circle cx="32" cy="28" r="2.2" fill={jewel} opacity="0.8" />
        </g>
      );
    case "bigmuff": {
      // Stylized Big Muff Pi (recognizable, not a logo repro): 3 knobs, a
      // wordmark band, and the signature big π. The Ram's Head adds a pair of horn
      // curls. `ink` contrasts with the actual (ref-derived) body colour — derived
      // from body luminance, NOT c.text (the tone's text can mismatch an overridden
      // body, e.g. BigFuzz's chrome tone has dark text on a dark sampled body).
      const ink = lum(c.body) > 0.55 ? "#1a1a1c" : "#eef0f2";
      const isRams = /\bRAM|HORN/i.test(lab);
      const isRuss = /RUSS/i.test(lab);
      // π colour: RED for the NYC + Ram's Head muffs, BLACK for the Green Russian.
      const piColor = isRuss ? "#1a1a1c" : "#c0241c";
      // Ram's Head + Green Russian get BLACK knobs.
      const knobC = isRams || isRuss ? { ...c, knob: "#161618" } : c;
      return (
        <g>
          {knobs(knobC, edge, [18, 32, 46], 15, 4.4)}
          {/* wordmark band suggesting "BIG MUFF" */}
          <rect
            x="11"
            y="23.5"
            width="42"
            height="4.4"
            rx="1.1"
            fill={ink}
            opacity="0.14"
          />
          <rect
            x="14"
            y="25.3"
            width="36"
            height="0.9"
            rx="0.45"
            fill={ink}
            opacity="0.5"
          />
          {/* big π icon, centered */}
          <g
            stroke={piColor}
            strokeWidth="1.5"
            strokeLinecap="round"
            fill="none"
          >
            <line x1="26.5" y1="32" x2="37.5" y2="32" />
            <line x1="29.4" y1="32.4" x2="28.9" y2="37" />
            <line x1="34.6" y1="32.4" x2="35.1" y2="37" />
          </g>
          {isRams && (
            // two ram horn curls flanking the π
            <g stroke={piColor} strokeWidth="1" fill="none" opacity="0.85">
              <path d="M24 33 q-3.4 0.4 -3.4 3.4 q0 2.4 2.4 2.4 q2 0 2 -2.2" />
              <path d="M40 33 q3.4 0.4 3.4 3.4 q0 2.4 -2.4 2.4 q-2 0 -2 -2.2" />
            </g>
          )}
        </g>
      );
    }
    default:
      return null;
  }
}
