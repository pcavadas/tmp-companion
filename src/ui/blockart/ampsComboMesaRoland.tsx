// src/ui/blockart/ampsComboMesaRoland.tsx — the Mesa Mark IIC + Roland JC-120 amp
// COMBO bodies, split from ./ampsCombo so each file stays ≤500 lines. Dispatched by
// AmpComboBody (via the `t === "mesa"` / `t === "roland"` branches). Shared helpers
// in ./parts.
import { GrilleCloth } from "./parts";

// Marksman (Mesa Mark IIC+) combo: black tolex, a busy black control panel
// (knob row + 5-band graphic-EQ slider block + toggle switches & red LED),
// a black jute grille — minimal flat sketch (no bolts/badge).
export function AmpComboMesa({ uid, edge }: { uid: string; edge: string }) {
  const knobs = [14.5, 18, 21.5, 25, 28.5];
  const eqX = [33, 35, 37, 39, 41],
    eqNub = [13.8, 12.3, 14.6, 12.7, 13.4];
  const sw = [45.6, 48.4, 51.2];
  return (
    <g>
      <rect
        x="7"
        y="7"
        width="50"
        height="50"
        rx="3"
        fill="#111114"
        stroke={edge}
        strokeWidth="0.7"
      />
      {/* black jute grille */}
      <GrilleCloth
        x={11}
        y={19}
        w={42}
        h={34}
        rx={1.6}
        tone="mesa"
        uid={uid + "a"}
      />
      {/* control panel */}
      <rect
        x="9"
        y="9"
        width="46"
        height="8.4"
        rx="1.2"
        fill="rgba(13,13,16,0.97)"
        stroke={edge}
        strokeWidth="0.4"
      />
      {/* input jacks (stacked, far left) */}
      {[11.2, 14.2].map((cy, i) => (
        <circle
          key={`j${String(i)}`}
          cx="11"
          cy={cy}
          r="0.7"
          fill="rgba(0,0,0,0.6)"
          stroke="#9aa0a6"
          strokeWidth="0.3"
        />
      ))}
      {/* knob row */}
      {knobs.map((cx, i) => (
        <circle
          key={`k${String(i)}`}
          cx={cx}
          cy="13"
          r="1.15"
          fill="#cfd3d7"
          stroke="rgba(0,0,0,0.45)"
          strokeWidth="0.3"
        />
      ))}
      {/* 5-band graphic EQ block */}
      <rect
        x="31.4"
        y="10.3"
        width="11.4"
        height="6.6"
        rx="0.8"
        fill="#39434f"
        stroke="rgba(0,0,0,0.5)"
        strokeWidth="0.3"
      />
      {eqX.map((x, i) => (
        <g key={`eq${String(i)}`}>
          <line
            x1={x}
            y1="11.1"
            x2={x}
            y2="16.2"
            stroke="#1b232c"
            strokeWidth="0.4"
          />
          <rect
            x={x - 0.85}
            y={eqNub[i]}
            width="1.7"
            height="0.65"
            rx="0.25"
            fill="#b3bac2"
          />
        </g>
      ))}
      {/* toggle switches + red LED */}
      {sw.map((x, i) => (
        <g key={`sw${String(i)}`}>
          <rect
            x={x - 0.7}
            y="13.1"
            width="1.4"
            height="1.3"
            rx="0.35"
            fill="#3a3a3e"
            stroke="rgba(0,0,0,0.45)"
            strokeWidth="0.25"
          />
          <line
            x1={x}
            y1="13.2"
            x2={x}
            y2="11"
            stroke="#d2d6da"
            strokeWidth="0.7"
            strokeLinecap="round"
          />
          <circle cx={x} cy="10.9" r="0.5" fill="#e8ebee" />
        </g>
      ))}
      <circle
        cx="53.7"
        cy="12.7"
        r="0.85"
        fill="#d24a3a"
        stroke="rgba(60,8,4,0.55)"
        strokeWidth="0.25"
      />
    </g>
  );
}

// JC Clean (Roland JC-120) combo: black cabinet, a wide control panel (input +
// bright switch, 6 knobs, a khaki chorus sub-panel of 3 knobs, a right section
// with red LED + rocker), a dark-charcoal grid grille, and a neutral badge.
export function AmpComboRoland({ uid, edge }: { uid: string; edge: string }) {
  const k6 = [16, 19, 22, 25, 28, 31];
  const k3 = [35.5, 38.5, 41.5];
  return (
    <g>
      <rect
        x="7"
        y="7"
        width="50"
        height="50"
        rx="3"
        fill="#121316"
        stroke={edge}
        strokeWidth="0.7"
      />
      {/* dark-charcoal grid grille */}
      <GrilleCloth
        x={11}
        y={19.5}
        w={42}
        h={33}
        rx={1.6}
        tone="jc"
        uid={uid + "a"}
      />
      {/* JC badge, top-left of the grille */}
      <g>
        <rect
          x="14"
          y="23"
          width="11"
          height="6.4"
          rx="0.8"
          fill="rgba(13,14,17,0.5)"
          stroke="#9aa0a6"
          strokeWidth="0.45"
        />
        <rect
          x="16.4"
          y="24.4"
          width="2"
          height="3.4"
          rx="0.4"
          fill="#c7cbcf"
          opacity="0.8"
        />
        <rect
          x="19.4"
          y="24.4"
          width="2"
          height="3.4"
          rx="0.4"
          fill="#c7cbcf"
          opacity="0.8"
        />
      </g>
      {/* control panel */}
      <rect
        x="9"
        y="9"
        width="46"
        height="8.4"
        rx="1.2"
        fill="#2b3338"
        stroke={edge}
        strokeWidth="0.4"
      />
      {/* input jack + bright toggle */}
      <circle
        cx="11"
        cy="13"
        r="0.8"
        fill="rgba(0,0,0,0.6)"
        stroke="#9aa0a6"
        strokeWidth="0.3"
      />
      <g>
        <rect
          x="12.9"
          y="13.1"
          width="1.1"
          height="1.2"
          rx="0.3"
          fill="#1d2326"
          stroke="rgba(0,0,0,0.4)"
          strokeWidth="0.25"
        />
        <line
          x1="13.45"
          y1="13.2"
          x2="13.45"
          y2="11.4"
          stroke="#d2d6da"
          strokeWidth="0.6"
          strokeLinecap="round"
        />
      </g>
      {/* 6 main knobs */}
      {k6.map((cx, i) => (
        <circle
          key={`k${String(i)}`}
          cx={cx}
          cy="13"
          r="1.05"
          fill="#cfd3d7"
          stroke="rgba(0,0,0,0.45)"
          strokeWidth="0.3"
        />
      ))}
      {/* khaki chorus sub-panel + 3 knobs */}
      <rect
        x="33.5"
        y="10"
        width="10"
        height="6.8"
        rx="0.7"
        fill="#8a7f4e"
        stroke="rgba(0,0,0,0.45)"
        strokeWidth="0.3"
      />
      {k3.map((cx, i) => (
        <circle
          key={`c${String(i)}`}
          cx={cx}
          cy="13"
          r="0.95"
          fill="#23231f"
          stroke="rgba(0,0,0,0.45)"
          strokeWidth="0.3"
        />
      ))}
      {/* right section: red LED + rocker switch */}
      <rect
        x="44.5"
        y="10"
        width="9.5"
        height="6.8"
        rx="0.7"
        fill="#15191c"
        stroke="rgba(0,0,0,0.45)"
        strokeWidth="0.3"
      />
      <circle
        cx="46.9"
        cy="13"
        r="0.75"
        fill="#d24a3a"
        stroke="rgba(60,8,4,0.55)"
        strokeWidth="0.25"
      />
      <rect
        x="49.6"
        y="11.6"
        width="2.2"
        height="2.8"
        rx="0.4"
        fill="#3a3e42"
        stroke="rgba(0,0,0,0.5)"
        strokeWidth="0.25"
      />
    </g>
  );
}
