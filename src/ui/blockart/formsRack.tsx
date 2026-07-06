// src/ui/blockart/formsRack.tsx — rack / desk / screen / rockbox form-factor
// renderers, split from ./forms so each file stays ≤500 lines. The treadle +
// round-fuzz forms live in ./formsPedal. Shared data/helpers live in ./shared.
import type { PedalTone } from "./shared";

// 19″ rack unit — preamps, with a VU meter (blue or amber) + knob row.
// 1.8: the Seventy Sixer 2U rack compressor (lab matches "Seventy Sixer" /
// "76") gets its own face — two big knurled knobs L/R, a lit cream VU meter on
// the right, and a column of square push-buttons (input/output/ratio bank).
export function RackBody({
  c,
  g,
  lab,
}: {
  c: PedalTone;
  g: string;
  lab: string;
}) {
  const edge = "rgba(0,0,0,0.42)";
  const vu = g === "racktube" ? "#d39a3a" : (c.jewel ?? "#5b9bd0");
  if (/SEVENTY ?SIXER|^76\b|1176/i.test(lab)) {
    // a knurled-rim big knob: outer knurl ring + dark cap + light pointer.
    const bigKnob = (cx: number, cy: number) => (
      <g transform={`translate(${String(cx)},${String(cy)})`}>
        <circle
          r="5.4"
          fill={c.knob}
          stroke="rgba(0,0,0,0.45)"
          strokeWidth="0.5"
        />
        {Array.from({ length: 24 }).map((_, i) => {
          const a = (i / 24) * Math.PI * 2;
          return (
            <line
              key={i}
              x1={Math.cos(a) * 4.6}
              y1={Math.sin(a) * 4.6}
              x2={Math.cos(a) * 5.3}
              y2={Math.sin(a) * 5.3}
              stroke="rgba(0,0,0,0.4)"
              strokeWidth="0.4"
            />
          );
        })}
        <circle r="3.1" fill="#1c1d20" />
        <line
          x1="0"
          y1="0"
          x2="0"
          y2="-2.6"
          stroke="#e8eaec"
          strokeWidth="0.8"
          strokeLinecap="round"
        />
      </g>
    );
    return (
      <g>
        {/* 2U black rack face */}
        <rect
          x="3"
          y="18"
          width="58"
          height="30"
          rx="2.5"
          fill={c.body}
          stroke={edge}
          strokeWidth="0.7"
        />
        {/* top highlight */}
        <rect
          x="4"
          y="18.6"
          width="56"
          height="0.7"
          rx="0.35"
          fill="#dfe3e6"
          opacity="0.2"
        />
        {/* two big knurled knobs L / R */}
        {bigKnob(15, 33)}
        {bigKnob(28, 33)}
        {/* column of square push-buttons (ratio/meter bank) */}
        {[0, 1, 2, 3].map((i) => (
          <rect
            key={i}
            x={37 + (i % 2) * 4.4}
            y={24 + Math.floor(i / 2) * 5.2}
            width="3.6"
            height="3.8"
            rx="0.6"
            fill="#15171b"
            stroke="#5a5f66"
            strokeWidth="0.4"
          />
        ))}
        {/* lit cream VU meter, right */}
        <rect
          x="46.5"
          y="24"
          width="12"
          height="11.5"
          rx="1.2"
          fill="#efe7cf"
          stroke="rgba(0,0,0,0.4)"
          strokeWidth="0.5"
        />
        <path
          d="M48.5 33.5 A 7 7 0 0 1 56.5 33.5"
          fill="none"
          stroke="#7a6f4a"
          strokeWidth="0.5"
          opacity="0.55"
        />
        <line
          x1="52.5"
          y1="33.8"
          x2="55.4"
          y2="26.4"
          stroke="#b23a3a"
          strokeWidth="0.9"
          strokeLinecap="round"
        />
        <circle cx="52.5" cy="33.8" r="0.8" fill="#3a3324" />
      </g>
    );
  }
  return (
    <g>
      <rect
        x="3"
        y="20"
        width="58"
        height="26"
        rx="2.5"
        fill={c.body}
        stroke={edge}
        strokeWidth="0.7"
      />
      {/* VU meter */}
      <rect
        x="11"
        y="25"
        width="18"
        height="15"
        rx="1.5"
        fill="#0e1116"
        stroke={edge}
        strokeWidth="0.5"
      />
      <path
        d="M13 38 A 9 9 0 0 1 27 38"
        fill="none"
        stroke={vu}
        strokeWidth="0.5"
        opacity="0.5"
      />
      <line
        x1="20"
        y1="38.5"
        x2="24"
        y2="28"
        stroke={vu}
        strokeWidth="1"
        strokeLinecap="round"
      />
      <circle cx="20" cy="38.5" r="0.9" fill={vu} />
      {/* knob row */}
      {[36, 42.5, 49, 55].map((x, i) => (
        <circle
          key={i}
          cx={x}
          cy={33}
          r="2.6"
          fill={c.knob}
          stroke={edge}
          strokeWidth="0.4"
        />
      ))}
    </g>
  );
}

// Desktop synth module — LCD + sliders + knob bank.
export function DeskBody({ c, lab: _lab }: { c: PedalTone; lab: string }) {
  const edge = "rgba(0,0,0,0.42)";
  return (
    <g>
      <rect
        x="3"
        y="15"
        width="58"
        height="37"
        rx="3"
        fill={c.body}
        stroke={edge}
        strokeWidth="0.7"
      />
      {/* LCD display */}
      <rect
        x="7"
        y="19"
        width="22"
        height="11"
        rx="1.2"
        fill="#0b0e0c"
        stroke={edge}
        strokeWidth="0.5"
      />
      <path
        d="M9 25 h3 v-3 h3 v5 h3 v-4 h3 v3 h3 v-2 h2"
        fill="none"
        stroke="#7fd9c4"
        strokeWidth="0.9"
      />
      {/* sliders */}
      {[33, 39, 45, 51].map((x, i) => (
        <g key={i}>
          <line
            x1={x}
            y1="19"
            x2={x}
            y2="30"
            stroke="rgba(0,0,0,0.3)"
            strokeWidth="0.7"
          />
          <rect
            x={x - 1.6}
            y={20 + (i % 2) * 4}
            width="3.2"
            height="2.6"
            rx="0.5"
            fill={c.knob}
            stroke={edge}
            strokeWidth="0.3"
          />
        </g>
      ))}
      {/* knob bank */}
      {[10, 17, 24, 31, 38, 45, 52].map((x, i) => (
        <circle
          key={i}
          cx={x}
          cy={41}
          r="2.3"
          fill={c.knob}
          stroke={edge}
          strokeWidth="0.35"
        />
      ))}
    </g>
  );
}

// On-screen plug-in panel — no hardware: a software GUI with a freq-response
// curve. Used for the parametric / cut / notch filters.
export function ScreenBody({ lab: _lab }: { lab: string }) {
  const edge = "rgba(0,0,0,0.4)";
  const line = "#5b9bd0";
  return (
    <g>
      <rect
        x="6"
        y="9"
        width="52"
        height="46"
        rx="2.5"
        fill="#11161d"
        stroke={edge}
        strokeWidth="0.7"
      />
      <rect x="6" y="9" width="52" height="7" rx="2.5" fill="#1b232d" />
      <circle cx="11" cy="12.5" r="1.1" fill="#3a4654" />
      <circle cx="15" cy="12.5" r="1.1" fill="#3a4654" />
      {[22, 30, 38, 46].map((y, i) => (
        <line
          key={i}
          x1="10"
          y1={y}
          x2="54"
          y2={y}
          stroke="#27313d"
          strokeWidth="0.4"
        />
      ))}
      <path
        d="M10 38 Q22 38 28 26 Q32 18 36 30 Q42 44 54 40"
        fill="none"
        stroke={line}
        strokeWidth="1.4"
      />
      {[18, 32, 46].map((x, i) => (
        <circle
          key={i}
          cx={x}
          cy={50}
          r="2.1"
          fill="#cfd6df"
          stroke={edge}
          strokeWidth="0.4"
        />
      ))}
    </g>
  );
}

// ============================================================================
// 1.8 Rockbox 100 (RBX) — a stompbox: black enclosure, a blue control panel up
// top carrying two vertical sliders (VOL / GAIN) flanked by mode/effect LED
// columns, a neutral wordmark badge, and a round footswitch with a status LED.
// Minimal flat sketch — no text.
// ============================================================================
export function RockboxBody({ c, lab: _lab }: { c: PedalTone; lab: string }) {
  const edge = "rgba(0,0,0,0.45)";
  const black = c.body; // black enclosure
  const blue = "#2f9bd0"; // RBX blue control panel
  return (
    <g>
      {/* black pedal enclosure */}
      <rect
        x="11"
        y="6"
        width="42"
        height="52"
        rx="4"
        fill={black}
        stroke={edge}
        strokeWidth="0.8"
      />
      {/* blue control panel (upper section) */}
      <rect
        x="14.5"
        y="9.5"
        width="35"
        height="20"
        rx="2"
        fill={blue}
        stroke="rgba(0,0,0,0.3)"
        strokeWidth="0.5"
      />
      {/* two vertical slider slots + thumbs (VOL / GAIN) */}
      {[27, 37].map((cx, i) => (
        <g key={i}>
          <rect
            x={cx - 1}
            y="12.5"
            width="2"
            height="14"
            rx="1"
            fill="#16242e"
          />
          <rect
            x={cx - 3}
            y={i === 0 ? 16 : 20}
            width="6"
            height="2.8"
            rx="1"
            fill="#1a1a1c"
            stroke="#0c0c0d"
            strokeWidth="0.3"
          />
        </g>
      ))}
      {/* mode/effect LED columns flanking the sliders */}
      {[0, 1, 2, 3].map((r) => (
        <g key={r}>
          <circle
            cx="19"
            cy={13.5 + r * 3.5}
            r="1"
            fill={r === 3 ? "#39c8e8" : "#8a1f1f"}
          />
          <circle
            cx="45"
            cy={13.5 + r * 3.5}
            r="1"
            fill={r === 0 ? "#d23a3a" : "#6f5f1c"}
          />
        </g>
      ))}
      {/* round footswitch + status LED */}
      <circle
        cx="32"
        cy="50"
        r="5"
        fill="#c9cdd1"
        stroke="#8a8f94"
        strokeWidth="0.8"
      />
      <circle
        cx="32"
        cy="50"
        r="2.4"
        fill="#aeb3b8"
        stroke="#7a7f84"
        strokeWidth="0.5"
      />
      <circle cx="42" cy="50" r="1.4" fill="#d23a3a" />
    </g>
  );
}
