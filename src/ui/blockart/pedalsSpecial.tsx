// src/ui/blockart/pedalsSpecial.tsx — the 1.8 full-custom pedal enclosures
// (GearPedalBody / LabBoostBody / GruntBoostBody), split from ./pedals so each file
// stays ≤500 lines. These carry their own LED + footswitch (they do NOT use the
// shared enclosure returns). Shared data/helpers live in ./shared.
import { lum, type PedalTone } from "./shared";
import { FONT_SANS } from "../../theme/tokens";

// ===========================================================================
// 1.8 gear-pedal layout (the Pinions OD enclosure), shared by Pinions / Runes /
// Lightyear. 3 knobs in a row, a 3-way mini toggle, an amber LED, and a footswitch
// well + chrome nut. The control inset colour flips per unit (see `inset`). No text.
// ===========================================================================
export function GearPedalBody({ c, lab }: { c: PedalTone; lab: string }) {
  const edge = "rgba(0,0,0,0.4)";
  // pointer contrasts with the knob; body stays the gear-pedal colour. The control
  // inset colour flips per unit so the trio reads distinct: Pinions/Blumes = green
  // inset on a yellow body, Lightyear = blue, Plumes (green body) + Runes = yellow.
  const inset = /BLUMES|PINIONS/i.test(lab)
    ? "#3f6f37"
    : /LSPEED|LIGHTYEAR|LIGHTSPEED/i.test(lab)
      ? "#3f78c0"
      : "#e3c23e";
  const ptr = lum(c.knob) > 0.6 ? "#3a3f28" : "#e8e8e8";
  const fanned = [
    { x2: 0, y2: -3.6 },
    { x2: 1.6, y2: -3.2 },
    { x2: -1.6, y2: -3.2 },
  ];
  return (
    <g>
      <rect
        x="11"
        y="4"
        width="42"
        height="56"
        rx="6"
        fill={c.body}
        stroke={edge}
        strokeWidth="0.7"
      />
      {/* control inset (per-unit colour) */}
      <rect
        x="14"
        y="7"
        width="36"
        height="22"
        rx="2.5"
        fill={inset}
        stroke="rgba(0,0,0,0.2)"
        strokeWidth="0.3"
      />
      {/* three knobs, pointers fanned, no labels */}
      {[19, 32, 45].map((x, i) => (
        <g key={i} transform={`translate(${String(x)},18)`}>
          <circle
            r="4.2"
            fill={c.knob}
            stroke="rgba(0,0,0,0.4)"
            strokeWidth="0.5"
          />
          <line
            x1="0"
            y1="0"
            x2={fanned[i].x2}
            y2={fanned[i].y2}
            stroke={ptr}
            strokeWidth="0.8"
            strokeLinecap="round"
          />
        </g>
      ))}
      {/* 3-way mini toggle on the lower body */}
      <g transform="translate(32,34)">
        <rect
          x="-3"
          y="-1.4"
          width="6"
          height="2.8"
          rx="1.1"
          fill="#2a2c20"
          stroke="rgba(0,0,0,0.45)"
          strokeWidth="0.3"
        />
        <line
          x1="0"
          y1="0"
          x2="1.7"
          y2="-1.9"
          stroke="#cfd1c4"
          strokeWidth="0.9"
          strokeLinecap="round"
        />
        <circle cx="1.9" cy="-2.1" r="0.7" fill="#e9ebdf" />
      </g>
      {/* amber status LED */}
      <circle
        cx="32"
        cy="41.5"
        r="1.9"
        fill="#f3b14a"
        stroke="rgba(80,48,0,0.4)"
        strokeWidth="0.3"
      />
      {/* footswitch well + chrome nut */}
      <rect
        x="20"
        y="46"
        width="24"
        height="11"
        rx="2.5"
        fill="rgba(0,0,0,0.22)"
      />
      <circle
        cx="32"
        cy="51.5"
        r="3.1"
        fill="#cfd1c4"
        stroke="rgba(0,0,0,0.5)"
        strokeWidth="0.5"
      />
      <circle cx="32" cy="51.5" r="1.4" fill="#9aa089" />
    </g>
  );
}

// Integrator Boost (ref spec): chrome lab-box, flat brushed-metal face, 3 small
// black knobs with lowercase labels.
export function LabBoostBody({ c }: { c: PedalTone }) {
  const edge = "rgba(0,0,0,0.4)";
  return (
    <g>
      <rect
        x="11"
        y="4"
        width="42"
        height="56"
        rx="4"
        fill={c.body}
        stroke={edge}
        strokeWidth="0.7"
      />
      {/* top brushed-metal highlight band */}
      <rect
        x="13"
        y="6"
        width="38"
        height="0.7"
        rx="0.35"
        fill="#dfe3e6"
        opacity="0.7"
      />
      {/* lab-box label plate */}
      <rect
        x="15"
        y="9"
        width="34"
        height="6"
        rx="1"
        fill="rgba(0,0,0,0.08)"
        stroke="rgba(0,0,0,0.2)"
        strokeWidth="0.3"
      />
      {/* three small dark-red knobs (TC Integrated Pre) */}
      {[19, 32, 45].map((x, i) => (
        <g key={i}>
          <circle
            cx={x}
            cy="22"
            r="3.2"
            fill="#6e1a1a"
            stroke="rgba(0,0,0,0.45)"
            strokeWidth="0.4"
          />
          <line
            x1={x}
            y1="22"
            x2={x}
            y2="19.4"
            stroke="#cfd3d7"
            strokeWidth="0.6"
            strokeLinecap="round"
          />
        </g>
      ))}
      {/* status LED + footswitch well */}
      <circle
        cx="32"
        cy="35.5"
        r="1.9"
        fill="#f3b14a"
        stroke="rgba(80,48,0,0.4)"
        strokeWidth="0.3"
      />
      <rect
        x="20"
        y="44"
        width="24"
        height="11"
        rx="2.5"
        fill="rgba(0,0,0,0.18)"
      />
      <circle
        cx="32"
        cy="49.5"
        r="3.1"
        fill="#cfd3d7"
        stroke="rgba(0,0,0,0.5)"
        strokeWidth="0.5"
      />
      <circle cx="32" cy="49.5" r="1.4" fill="#8d9195" />
    </g>
  );
}

// Grunt Boost (ref spec): matte black enclosure, ONE large central knob, a bold
// white numeral on the knob.
export function GruntBoostBody({ c }: { c: PedalTone }) {
  const edge = "rgba(0,0,0,0.4)";
  return (
    <g>
      <rect
        x="11"
        y="4"
        width="42"
        height="56"
        rx="6"
        fill={c.body}
        stroke={edge}
        strokeWidth="0.7"
      />
      {/* one large central knob */}
      <circle
        cx="32"
        cy="24"
        r="9.5"
        fill={c.knob}
        stroke="rgba(0,0,0,0.4)"
        strokeWidth="0.6"
      />
      <line
        x1="32"
        y1="24"
        x2="32"
        y2="16"
        stroke="#1b1b1b"
        strokeWidth="0.8"
        strokeLinecap="round"
      />
      <text
        x="32"
        y="40"
        textAnchor="middle"
        fontFamily={FONT_SANS}
        fontSize="6"
        fontWeight="800"
        fill={c.text}
      >
        +
      </text>
      {/* status LED + footswitch well */}
      <circle
        cx="32"
        cy="44.5"
        r="1.9"
        fill="#f3b14a"
        stroke="rgba(80,48,0,0.4)"
        strokeWidth="0.3"
      />
      <rect
        x="20"
        y="49"
        width="24"
        height="9"
        rx="2.5"
        fill="rgba(0,0,0,0.22)"
      />
      <circle
        cx="32"
        cy="53.5"
        r="2.6"
        fill="#cfd3d7"
        stroke="rgba(0,0,0,0.5)"
        strokeWidth="0.5"
      />
    </g>
  );
}
