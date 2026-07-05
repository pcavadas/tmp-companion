// src/ui/blockart/ampsHeadBritish.tsx — British/US "loud" amp-HEAD bodies (Orange,
// Gallien-Krueger, Mesa Dual Rectifier, Ampeg SVT), split from ./amps so each file
// stays ≤500 lines. Dispatched by AmpBody. Shared data/helpers in ./shared/./parts.
import type { PedalTone } from "./shared";
import { GrilleCloth, ChassisBody } from "./parts";

// Orange heads: cream picture-frame border, white control panel with the brand
// bar + red square up top, and a lower orange control band with black knobs.
// The cream frame is a FILLED rect with the white panel inset by an EQUAL
// margin on all four sides, so the visible cream border is uniform width all
// around (a stroked frame + mismatched inner panel reads as uneven borders).
export function AmpHeadOrange({
  c,
  t,
  edge,
  uid,
}: {
  c: PedalTone;
  t: string;
  edge: string;
  uid: string;
}) {
  const frame = "#efe9da";
  return (
    <g>
      <ChassisBody
        x={5}
        y={20}
        w={54}
        h={22}
        rx={3}
        c={c}
        t={t}
        edge={edge}
        uid={uid}
        k="oh"
      />
      {/* filled cream picture-frame */}
      <rect
        x="7.5"
        y="22"
        width="49"
        height="18"
        rx="1.8"
        fill={frame}
        stroke="rgba(0,0,0,0.18)"
        strokeWidth="0.4"
      />
      {/* white panel, inset 2.2 on every side → uniform cream border */}
      <rect
        x="9.7"
        y="24.2"
        width="44.6"
        height="13.6"
        rx="0.9"
        fill="#f9f7f1"
      />
      <rect x="12" y="25.4" width="18.5" height="3.3" rx="0.6" fill="#16120c" />
      <rect
        x="49.4"
        y="25.2"
        width="3.8"
        height="3.8"
        rx="0.6"
        fill="#c33b1e"
        opacity="0.85"
      />
      <rect
        x="12"
        y="30.6"
        width="40"
        height="6.1"
        rx="0.9"
        fill={c.body}
        stroke="rgba(0,0,0,0.25)"
        strokeWidth="0.4"
      />
      {[0, 1, 2, 3, 4, 5].map((i) => (
        <g key={i}>
          <circle
            cx={15.6 + i * 6.3}
            cy={33.6}
            r="1.9"
            fill="#15110f"
            stroke="rgba(0,0,0,0.4)"
            strokeWidth="0.4"
          />
          <line
            x1={15.6 + i * 6.3}
            y1={33.6}
            x2={15.6 + i * 6.3}
            y2={32}
            stroke="#e8e8e8"
            strokeWidth="0.55"
            strokeLinecap="round"
          />
        </g>
      ))}
    </g>
  );
}

// Gallien-Krueger solid-state bass head: black steel chassis with a finned
// heatsink top, a silver control strip packed with small knobs, and the white
// GK wordmark across the lower face.
export function AmpHeadGk({
  c,
  t,
  edge,
  uid,
}: {
  c: PedalTone;
  t: string;
  edge: string;
  uid: string;
}) {
  const sil = c.panel;
  return (
    <g>
      <ChassisBody
        x={5}
        y={20}
        w={54}
        h={22}
        rx={3}
        c={c}
        t={t}
        edge={edge}
        uid={uid}
        k="gk"
      />
      {/* finned heatsink top */}
      <path
        d="M8 20 h48 a3 3 0 0 1 3 3 v2.4 h-54 v-2.4 a3 3 0 0 1 3 -3 z"
        fill="#202327"
      />
      {Array.from({ length: 14 }).map((_, i) => (
        <line
          key={i}
          x1={10 + i * 3.05}
          y1={21}
          x2={10 + i * 3.05}
          y2={25}
          stroke="#0c0d0f"
          strokeWidth="0.55"
        />
      ))}
      {/* silver control strip */}
      <rect
        x="7"
        y="27"
        width="50"
        height="8"
        rx="1"
        fill={sil}
        stroke="rgba(0,0,0,0.32)"
        strokeWidth="0.4"
      />
      <circle
        cx="11"
        cy="31"
        r="1.5"
        fill="#16161a"
        stroke="rgba(0,0,0,0.45)"
        strokeWidth="0.4"
      />
      {Array.from({ length: 10 }).map((_, i) => (
        <circle
          key={i}
          cx={16 + i * 4.05}
          cy={31}
          r="1.45"
          fill="#16161a"
          stroke="rgba(0,0,0,0.45)"
          strokeWidth="0.3"
        />
      ))}
      {/* abstract logo plate + wordmark suggestion on the lower black face */}
      <rect
        x="8.5"
        y="37"
        width="6"
        height="3.6"
        rx="1"
        fill="none"
        stroke="#e8eaec"
        strokeWidth="0.6"
      />
      <rect
        x="10"
        y="38.45"
        width="3"
        height="0.9"
        rx="0.45"
        fill="#e8eaec"
        opacity="0.85"
      />
      <rect
        x="17"
        y="38.2"
        width="24"
        height="1.5"
        rx="0.6"
        fill="#e8eaec"
        opacity="0.85"
      />
    </g>
  );
}

// Mesa/Boogie Dual Rectifier: black tolex, diamond-plate steel upper panel with
// a central chrome MESA badge flanked by vent louvers, and a black lower control
// strip with a long row of knobs + the red power LED.
export function AmpHeadRecto({
  c,
  t,
  edge,
  uid,
}: {
  c: PedalTone;
  t: string;
  edge: string;
  uid: string;
}) {
  const dp = "dp" + uid;
  return (
    <g>
      <ChassisBody
        x={5}
        y={20}
        w={54}
        h={22}
        rx={3}
        c={c}
        t={t}
        edge={edge}
        uid={uid}
        k="rc"
      />
      <defs>
        <pattern id={dp} patternUnits="userSpaceOnUse" width="6" height="6">
          <rect width="6" height="6" fill="#cdd0d2" />
          <g strokeLinecap="round" stroke="#8d9094" strokeWidth="1">
            <line x1="1" y1="2.3" x2="2.7" y2="0.6" />
            <line x1="3.3" y1="5.4" x2="5" y2="3.7" />
            <line x1="3.3" y1="0.6" x2="5" y2="2.3" />
            <line x1="1" y1="3.7" x2="2.7" y2="5.4" />
          </g>
        </pattern>
      </defs>
      {/* diamond-plate upper panel */}
      <rect
        x="7"
        y="22.5"
        width="50"
        height="9"
        rx="1"
        fill={`url(#${dp})`}
        stroke="rgba(0,0,0,0.4)"
        strokeWidth="0.5"
      />
      {/* vent louvers */}
      {[12, 38].map((vx, k) => (
        <g key={k}>
          <rect
            x={vx}
            y="24.6"
            width="10"
            height="4.8"
            rx="0.5"
            fill="#16161a"
          />
          {[0, 1, 2, 3].map((i) => (
            <line
              key={i}
              x1={vx + 1.4 + i * 2.2}
              y1="25.1"
              x2={vx + 1.4 + i * 2.2}
              y2="28.9"
              stroke="#aeb1b4"
              strokeWidth="0.6"
            />
          ))}
        </g>
      ))}
      {/* central chrome logo badge (abstract — no brand wordmark) */}
      <rect
        x="25.5"
        y="24"
        width="13"
        height="6"
        rx="1"
        fill="#16161a"
        stroke="#cdd0d2"
        strokeWidth="0.4"
      />
      <rect
        x="28.5"
        y="26.55"
        width="7"
        height="0.9"
        rx="0.45"
        fill="#e8eaec"
        opacity="0.9"
      />
      {/* lower black control strip */}
      <rect x="7" y="33" width="50" height="6" rx="1" fill="#0e0e10" />
      <circle cx="11" cy="36" r="1.2" fill="#d23b2b" />
      {Array.from({ length: 11 }).map((_, i) => (
        <circle
          key={i}
          cx={16 + i * 3.7}
          cy="36"
          r="1.35"
          fill="#1c1c1f"
          stroke="#5a5a5e"
          strokeWidth="0.3"
        />
      ))}
    </g>
  );
}

// Ampeg SVT (Blue Line): black tolex, silver control panel with chrome knobs +
// black rocker switches and a blue jewel, and the dark silver-fleck Ampeg grille
// below with the script badge.
export function AmpHeadSvt({
  c,
  t,
  edge,
  uid,
}: {
  c: PedalTone;
  t: string;
  edge: string;
  uid: string;
}) {
  return (
    <g>
      <ChassisBody
        x={5}
        y={20}
        w={54}
        h={22}
        rx={3}
        c={c}
        t={t}
        edge={edge}
        uid={uid}
        k="svt"
      />
      {/* silver blue-line control panel across the TOP (ref) */}
      <rect
        x="7"
        y="21.5"
        width="50"
        height="7"
        rx="1"
        fill={c.panel}
        stroke="rgba(0,0,0,0.3)"
        strokeWidth="0.4"
      />
      {/* blue silkscreen line */}
      <rect
        x="9"
        y="22.4"
        width="46"
        height="0.5"
        rx="0.25"
        fill="#3f78c0"
        opacity="0.6"
      />
      <circle cx="10.5" cy="25.2" r="1.2" fill={c.jewel} />
      {[0, 1, 2, 3, 4].map((i) => (
        <g key={i}>
          <circle
            cx={18 + i * 5.2}
            cy="25.2"
            r="1.7"
            fill="#cfd3d7"
            stroke="rgba(0,0,0,0.45)"
            strokeWidth="0.4"
          />
          <line
            x1={18 + i * 5.2}
            y1="25.2"
            x2={18 + i * 5.2}
            y2="23.8"
            stroke="rgba(0,0,0,0.5)"
            strokeWidth="0.5"
          />
        </g>
      ))}
      {/* two rocker switches on the right of the panel */}
      {[46, 50].map((rx, i) => (
        <rect
          key={i}
          x={rx}
          y="23.8"
          width="3"
          height="2.8"
          rx="0.4"
          fill="#0e0e10"
        />
      ))}
      {/* tall silver-fleck Ampeg grille filling the lower face + script badge */}
      <GrilleCloth
        x={7}
        y={30}
        w={50}
        h={10}
        rx={1.3}
        tone="svt"
        uid={uid + "g"}
      />
      <ellipse cx="20" cy="35" rx="11" ry="2.6" fill="#15161a" opacity="0.82" />
      <rect
        x="15"
        y="34.55"
        width="10"
        height="0.9"
        rx="0.45"
        fill="#cfd3d7"
        opacity="0.85"
      />
    </g>
  );
}
