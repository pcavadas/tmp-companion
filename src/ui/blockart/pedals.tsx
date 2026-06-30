// Stompbox pedals (knob-row motifs) — block-art renderer(s) split from the BlockArt engine.
// Shared data/helpers (tones, cloth, chassis, speakers) live in ./shared.
import { lum, ptrColor, type PedalTone } from "./shared";

const SANS = "'Inter', system-ui, sans-serif";

// knob row helper
function knobs(c: PedalTone, edge: string, xs: number[], y: number, r: number) {
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

// ===========================================================================
// 1.8 gear-pedal layout (the Pinions OD enclosure), shared by Pinions / Runes /
// Lightyear. 3 knobs in a row, a 3-way mini toggle, an amber LED, and a footswitch
// well + chrome nut. The control inset colour flips per unit (see `inset`). No text.
// ===========================================================================
function GearPedalBody({ c, lab }: { c: PedalTone; lab: string }) {
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
function LabBoostBody({ c }: { c: PedalTone }) {
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
function GruntBoostBody({ c }: { c: PedalTone }) {
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
        fontFamily={SANS}
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

export function PedalBody({
  c: cIn,
  g,
  lab,
  footswitch = "round",
  accent,
  panel,
}: {
  c: PedalTone;
  g: string;
  lab: string;
  footswitch?: "plate" | "metal" | "round";
  /** Fender reverb-chassis accent (footswitch colour). When present the pedal
   *  renders the cream chassis and the screen icon is drawn in this colour. */
  accent?: string;
  /** recessed coloured control panel behind the knobs/sliders (MEGA EQ-5, FILTRON). */
  panel?: string;
}) {
  // On the Fender reverb chassis the body is cream and the reverb-type icon reads in
  // the accent colour, so draw the motif with knob=accent.
  const c = accent ? { ...cIn, knob: accent } : cIn;
  // FX-loop number (1/2/3/4/3+4) — the ONLY text an fxloop block carries.
  const loopNum = lab.replace(/^.*?FX\s*-?\s*/i, "").trim();
  const edge = "rgba(0,0,0,0.4)";
  const jewel = "#f5a986";
  // Glooper (EHX-style glitch looper) wears a holographic/iridescent finish — a
  // wash of shifting greens/blues/purples over its dark body.
  const isGloop = /GLOOP/i.test(lab);
  // Footswitch well — a recessed darker rounded rect behind the round footswitch.
  // EVERY round-footswitch pedal gets one (for consistency); it is SOLID BLACK on
  // the few whose ref has a black footswitch panel (Big Muff, Electric Mistress,
  // Small Stone, Memory Man / Memory Man Stereo), else a translucent recess. The
  // treadle (plate) and TS808 (metal) footswitches and the round-form (roundfuzz)
  // fuzz faces never reach the round branch, so they get no well.
  // `lab` is the normalized short (normalizeShort turns "BMP-NYC" into "BMP NYC"),
  // so match the de-hyphenated forms. "BMP NYC" is Big Fuzz only — the other Big
  // Muff variants (BMP RUSS / BMP RH) are not in the black-well list.
  const wellFill = /BMP NYC|MISTRESS|SMSTONE|DMM/i.test(lab)
    ? "#1a1a1c"
    : "rgba(0,0,0,0.22)";
  // 1.8 gear pedals + boosts → full-custom enclosures (own LED + footswitch).
  if (g === "od3") return <GearPedalBody c={c} lab={lab} />;
  if (g === "labboost") return <LabBoostBody c={c} />;
  if (g === "gruntboost") return <GruntBoostBody c={c} />;
  // motif drawn in the top control zone (~y 8..30)
  let motif = null;
  switch (g) {
    case "knob1":
    case "boost":
      motif = <g>{knobs(c, edge, [32], 18, 7)}</g>;
      break;
    case "knobs2":
      motif = <g>{knobs(c, edge, [22, 42], 18, 5.4)}</g>;
      break;
    case "knobs3":
    case "od":
      motif = <g>{knobs(c, edge, [19, 32, 45], 18, 4.6)}</g>;
      break;
    case "knobs4":
      motif = <g>{knobs(c, edge, [16.5, 26.8, 37.2, 47.5], 18, 4.2)}</g>;
      break;
    case "knobs6":
      motif = (
        <g>
          {knobs(c, edge, [19, 32, 45], 13, 3.6)}
          {knobs(c, edge, [19, 32, 45], 24, 3.6)}
        </g>
      );
      break;
    case "dist":
      motif = (
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
      break;
    case "fuzz":
      motif = (
        <g>
          {knobs(c, edge, [23, 41], 17, 6)}
          <circle cx="32" cy="28" r="2.2" fill={jewel} opacity="0.8" />
        </g>
      );
      break;
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
      motif = (
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
          <g stroke={piColor} strokeWidth="1.5" strokeLinecap="round" fill="none">
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
      break;
    }
    case "comp":
      motif = (
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
      break;
    case "gate":
      motif = (
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
      break;
    case "vol":
      motif = (
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
      break;
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
      motif = (
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
      break;
    }
    case "peq":
      motif = (
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
      break;
    case "wah":
      motif = (
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
      break;
    case "envf":
      motif = (
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
      break;
    case "chorus":
      motif = (
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
      break;
    case "phaser":
      motif = (
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
      break;
    case "flanger":
      motif = (
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
      break;
    case "tremolo":
      motif = (
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
      break;
    case "rotary":
      motif = (
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
      break;
    case "univibe":
      motif = (
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
      break;
    case "delay":
      motif = (
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
      break;
    case "spring":
      motif = (
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
      break;
    case "plate":
      motif = (
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
      break;
    case "hall":
      motif = (
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
      break;
    case "shimmer":
      motif = (
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
      break;
    case "octave":
      motif = (
        <g>
          {knobs(c, edge, [22, 42], 16, 5)}
          <path
            d="M27 28 l0 -7 -2 0 3 -4 3 4 -2 0 0 7 z"
            fill={c.knob}
            opacity="0.8"
          />
          <path
            d="M37 11 l0 7 2 0 -3 4 -3 -4 2 0 0 -7 z"
            fill={c.knob}
            opacity="0.6"
          />
        </g>
      );
      break;
    case "whammy":
      motif = (
        <g>
          <polygon points="16,26 48,26 44,11 20,11" fill="rgba(0,0,0,0.22)" />
          <path
            d="M22 23 l18 -9 -3 -1 4 -2 1 4 -2 -0.5"
            fill="none"
            stroke={c.knob}
            strokeWidth="1.4"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </g>
      );
      break;
    case "synth":
      motif = (
        <g>
          <rect x="15" y="11" width="34" height="13" rx="1.5" fill="#0c0c10" />
          <path
            d="M17 17 h4 v-3 h4 v6 h4 v-4 h4 v3 h4 v-5 h4 v5 h2"
            fill="none"
            stroke="#7fd9c4"
            strokeWidth="1"
          />
        </g>
      );
      break;
    case "fxloop":
      motif = (
        <g>
          <path
            d="M18 14 h22 a4 4 0 0 1 0 8 h-9"
            fill="none"
            stroke={c.knob}
            strokeWidth="1.4"
          />
          <path
            d="M34 18 l-4 4 4 4"
            fill="none"
            stroke={c.knob}
            strokeWidth="1.4"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
          <path
            d="M46 26 h-22 a4 4 0 0 1 0 -8 h9"
            fill="none"
            stroke={c.knob}
            strokeWidth="1.4"
            opacity="0.55"
          />
          {/* loop number — the allowed FX-loop wordmark */}
          <text
            x="32"
            y="34"
            textAnchor="middle"
            fontFamily={SANS}
            fontSize="10"
            fontWeight="800"
            letterSpacing="0.04em"
            fill={c.knob}
          >
            {loopNum}
          </text>
        </g>
      );
      break;
    // ---- 1.8 Fender-designed concept motifs (abstract marks on the standard
    // enclosure; one knob + the marque in the control zone) -------------------
    case "steptrem":
      // staircase-stepped square-wave LFO
      motif = (
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
      break;
    case "stepfilter":
      // staircase carving through a frequency-sweep curve
      motif = (
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
      break;
    case "stepfilterdelay":
      // staircase + receding echo dots
      motif = (
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
      break;
    case "pitchseq":
      // 4 vertical step sliders (ref: Pitch Sequencer — four coloured faders)
      motif = (
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
      break;
    case "octslider":
      // 8 vertical octave faders (ref: POLYGON OCTAVE SHIFTER — Orig/sub/up/HC)
      motif = (
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
      break;
    case "prismdelay":
      // a prism splitting one beam into a fan of fading echoes
      motif = (
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
      break;
    case "spectralverb":
      // ghostly shimmer / aurora rising from a note tail
      motif = (
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
      break;
    case "cirrusverb":
      // high wispy cirrus-cloud sheet
      motif = (
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
      break;
    case "cirrussynthverb":
      // cirrus cloud sheet + a glowing synth waveform woven through
      motif = (
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
      break;
    default:
      motif = <g>{knobs(c, edge, [22, 42], 18, 5.4)}</g>;
  }
  if (accent) {
    // Fender reverb chassis (8 models): cream body, a recessed screen holding the
    // reverb-type icon (the motif, in the accent colour), and a coloured footswitch
    // section at the bottom whose colour identifies the model. Ref-discovered accent.
    return (
      <g>
        <rect
          x="11"
          y="4"
          width="42"
          height="56"
          rx="6"
          fill="#e7e4d9"
          stroke={edge}
          strokeWidth="0.7"
        />
        {/* screen recess framing the reverb-type icon */}
        <rect
          x="14"
          y="8"
          width="36"
          height="26"
          rx="2"
          fill="#d6d3c6"
          stroke="rgba(0,0,0,0.18)"
          strokeWidth="0.4"
        />
        {motif}
        {/* status LED, above the footswitch well */}
        <circle cx="32" cy="37.5" r="1.7" fill={jewel} opacity="0.9" />
        {/* coloured footswitch well — the model-identifying accent, now just the
            recess behind the switch (consistent with every other round footswitch) */}
        <rect x="19" y="43" width="26" height="14" rx="3" fill={accent} />
        {/* round footswitch */}
        <circle
          cx="32"
          cy="50"
          r="5.4"
          fill="#c9ccce"
          stroke="rgba(0,0,0,0.32)"
          strokeWidth="0.6"
        />
        <circle
          cx="32"
          cy="50"
          r="2.7"
          fill="#aeb1b4"
          stroke="rgba(0,0,0,0.25)"
          strokeWidth="0.5"
        />
      </g>
    );
  }
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
      {isGloop && (
        <>
          <defs>
            <clipPath id="gloopclip">
              <rect x="11" y="4" width="42" height="56" rx="6" />
            </clipPath>
          </defs>
          <g clipPath="url(#gloopclip)">
            <ellipse cx="20" cy="16" rx="18" ry="16" fill="#3fae84" opacity="0.34" />
            <ellipse cx="46" cy="30" rx="16" ry="20" fill="#4f8fd0" opacity="0.32" />
            <ellipse cx="22" cy="46" rx="17" ry="16" fill="#9a7bc8" opacity="0.3" />
            <ellipse cx="42" cy="54" rx="15" ry="12" fill="#3fb0a0" opacity="0.28" />
          </g>
        </>
      )}
      {panel && (
        // recessed coloured control panel behind the knobs/sliders
        <rect
          x="14"
          y="8"
          width="36"
          height="23"
          rx="2.5"
          fill={panel}
          stroke="rgba(0,0,0,0.25)"
          strokeWidth="0.4"
        />
      )}
      {motif}
      {/* status LED, just above the footswitch */}
      <circle cx="32" cy="37.5" r="1.7" fill={jewel} opacity="0.9" />
      {footswitch === "plate" ? (
        // big black-rubber treadle plate (Boss / metal gate / TS-10)
        <g>
          <rect x="13" y="42" width="38" height="15" rx="3" fill="#1a1a1c" />
          <rect
            x="14.2"
            y="43"
            width="35.6"
            height="1.4"
            rx="0.7"
            fill="rgba(255,255,255,0.10)"
          />
        </g>
      ) : footswitch === "metal" ? (
        // small metallic rectangle switch (Ibanez TS808)
        <g>
          <rect
            x="25"
            y="47"
            width="14"
            height="8"
            rx="1.5"
            fill="#cfd3d7"
            stroke="rgba(0,0,0,0.5)"
            strokeWidth="0.5"
          />
          <rect
            x="25.7"
            y="47.7"
            width="12.6"
            height="1"
            rx="0.5"
            fill="#eef0f2"
          />
        </g>
      ) : (
        // round chrome footswitch button (default) in its recessed well
        <g>
          <rect x="19" y="43" width="26" height="14" rx="3" fill={wellFill} />
          <circle
            cx="32"
            cy="50"
            r="5.4"
            fill="#c9cdd1"
            stroke="#8a8f94"
            strokeWidth="0.8"
          />
          <circle
            cx="32"
            cy="50"
            r="2.6"
            fill="#aeb3b8"
            stroke="#7a7f84"
            strokeWidth="0.5"
          />
        </g>
      )}
    </g>
  );
}

// ---- legacy code → {tone, icon, label, family} ----------------------------
