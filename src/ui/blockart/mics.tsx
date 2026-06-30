// Microphones — block-art renderer(s) split from the BlockArt engine.
// Shared data/helpers (tones, cloth, chassis, speakers) live in ./shared.
import type { PedalTone } from "./shared";

// ============================================================================
export function MicBody({
  c: _c,
  g,
  lab: _lab,
  uid,
}: {
  c: PedalTone;
  g: string;
  lab: string;
  uid: string;
}) {
  const edge = "rgba(0,0,0,0.45)";
  const cid = "mc" + uid;

  // AKG C414 — VERTICAL: squared charcoal head with a champagne-gold mesh grille
  // up top, the XLS selector window + green LED, polar-pattern dot row, the round
  // blue AKG badge, "C 414" label, and the cylindrical XLR barrel at the bottom.
  if (g === "mic_c414") {
    const shell = "#34363b",
      shellDk = "#27292d";
    const gold = "#cdbb8e",
      goldDk = "#9c8a5e";
    const wv = cid + "w";
    // near-rectangular, vertically-symmetric hexagon: flat top & bottom, sides
    // bulging slightly out to the widest point at the mid-seam
    const head = "M24 5 L40 5 L42 25.5 L40 46 L24 46 L22 25.5 Z";
    return (
      <g>
        {/* XLR barrel at the base */}
        <rect
          x="27.5"
          y="45.5"
          width="9"
          height="11.5"
          rx="1.8"
          fill={shellDk}
          stroke={edge}
          strokeWidth="0.7"
        />
        {/* faceted head shell */}
        <path
          d={head}
          fill={shell}
          stroke={edge}
          strokeWidth="0.9"
          strokeLinejoin="round"
        />
        {/* champagne-gold mesh grille fills the upper half (above the seam) */}
        <defs>
          <pattern
            id={wv}
            patternUnits="userSpaceOnUse"
            width="2.4"
            height="2.4"
            patternTransform="rotate(45)"
          >
            <rect width="2.4" height="2.4" fill={gold} />
            <line
              x1="0"
              y1="0"
              x2="0"
              y2="2.4"
              stroke={goldDk}
              strokeWidth="0.55"
            />
            <line
              x1="0"
              y1="0"
              x2="2.4"
              y2="0"
              stroke={goldDk}
              strokeWidth="0.55"
            />
          </pattern>
          <clipPath id={cid}>
            <path d="M24 5.4 L40 5.4 L41.6 25 L22.4 25 Z" />
          </clipPath>
        </defs>
        <g clipPath={`url(#${cid})`}>
          <rect x="20" y="4" width="24" height="22" fill={`url(#${wv})`} />
        </g>
        {/* horizontal seam line across the widest part of the head */}
        <line
          x1="22"
          y1="25.5"
          x2="42"
          y2="25.5"
          stroke={goldDk}
          strokeWidth="0.8"
        />
      </g>
    );
  }
  // Sennheiser MD421 — VERTICAL: short, wide black mesh grille basket up top,
  // a long body tapering down to a narrow base, and a small connector barrel.
  if (g === "mic_421") {
    const shell = "#34363b",
      shellHi = "#46484e",
      shellDk = "#202125";
    const meshc = "#3b3d42",
      meshDk = "#1b1c1f";
    const wv = cid + "w";
    return (
      <g>
        {/* small connector barrel at the base */}
        <rect
          x="28"
          y="50"
          width="8"
          height="8"
          rx="1.6"
          fill={shellDk}
          stroke={edge}
          strokeWidth="0.7"
        />
        {/* tapering body: wide at the grille shoulder, narrowing to the base */}
        <path
          d="M23 20 L41 20 L37 50.5 L27 50.5 Z"
          fill={shell}
          stroke={edge}
          strokeWidth="0.9"
          strokeLinejoin="round"
        />
        <path
          d="M25 20 L28 20 L29.5 50.5 L27.7 50.5 Z"
          fill={shellHi}
          opacity="0.32"
        />
        {/* taller, narrower grille basket — top a touch narrower than its base */}
        <path
          d="M26.7 5 Q23.6 5 23.4 8.5 L23 20 L41 20 L40.6 8.5 Q40.4 5 37.3 5 Z"
          fill={shell}
          stroke={edge}
          strokeWidth="0.9"
          strokeLinejoin="round"
        />
        {/* black diamond mesh */}
        <defs>
          <pattern
            id={wv}
            patternUnits="userSpaceOnUse"
            width="2.2"
            height="2.2"
            patternTransform="rotate(45)"
          >
            <rect width="2.2" height="2.2" fill={meshc} />
            <line
              x1="0"
              y1="0"
              x2="0"
              y2="2.2"
              stroke={meshDk}
              strokeWidth="0.55"
            />
            <line
              x1="0"
              y1="0"
              x2="2.2"
              y2="0"
              stroke={meshDk}
              strokeWidth="0.55"
            />
          </pattern>
          <clipPath id={cid}>
            <path d="M26.7 5.6 Q23.9 5.6 23.7 8.5 L23.4 19.4 L40.6 19.4 L40.3 8.5 Q40.1 5.6 37.3 5.6 Z" />
          </clipPath>
        </defs>
        <g clipPath={`url(#${cid})`}>
          <rect x="22" y="4" width="20" height="16" fill={`url(#${wv})`} />
        </g>
        {/* seam between grille and body */}
        <line
          x1="23"
          y1="20"
          x2="41"
          y2="20"
          stroke={meshDk}
          strokeWidth="0.8"
        />
      </g>
    );
  }
  // EV RE20 — VERTICAL broadcast dynamic: a rounded-corner body of uniform width.
  // Top zone = 4 square mesh windows (2×2, with gaps); a bevel groove; lower zone =
  // distinct horizontal mesh slats; a narrow inset label band; mount barrel.
  if (g === "mic_re20") {
    const shell = "#d7d4c9",
      shellDk = "#b0ada1",
      meshc = "#c2bfb2",
      meshDk = "#8b887c",
      band = "#26261f";
    const wv = cid + "w";
    const mesh = (x: number, y: number, w: number, h: number, rx: number) => (
      <rect
        x={x}
        y={y}
        width={w}
        height={h}
        rx={rx}
        fill={`url(#${wv})`}
        stroke={meshDk}
        strokeWidth="0.35"
      />
    );
    return (
      <g>
        <defs>
          <pattern
            id={wv}
            patternUnits="userSpaceOnUse"
            width="1.7"
            height="1.7"
          >
            <rect width="1.7" height="1.7" fill={meshc} />
            <line
              x1="0"
              y1="0"
              x2="1.7"
              y2="0"
              stroke={meshDk}
              strokeWidth="0.4"
            />
            <line
              x1="0"
              y1="0"
              x2="0"
              y2="1.7"
              stroke={meshDk}
              strokeWidth="0.4"
            />
          </pattern>
        </defs>
        {/* bottom mount barrel */}
        <rect
          x="28.5"
          y="50.5"
          width="7"
          height="7.5"
          rx="1.3"
          fill={shellDk}
          stroke={edge}
          strokeWidth="0.6"
        />
        {/* uniform-width body — equal, gentle rounding top & bottom */}
        <path
          d="M24 7 Q24 3.5 27.5 3.5 L36.5 3.5 Q40 3.5 40 7 L40 48.5 Q40 52 36.5 52 L27.5 52 Q24 52 24 48.5 Z"
          fill={shell}
          stroke={edge}
          strokeWidth="0.9"
          strokeLinejoin="round"
        />
        {/* TOP zone — 4 square mesh windows (2×2) with gaps */}
        {mesh(25.3, 6, 5.7, 3.6, 0.7)}
        {mesh(33, 6, 5.7, 3.6, 0.7)}
        {mesh(25.3, 11.2, 5.7, 3.6, 0.7)}
        {mesh(33, 11.2, 5.7, 3.6, 0.7)}
        {/* bevel groove between the two zones */}
        <rect
          x="24.5"
          y="15.7"
          width="15"
          height="1.2"
          fill="rgba(0,0,0,0.15)"
        />
        <line
          x1="24.5"
          y1="17.1"
          x2="39.5"
          y2="17.1"
          stroke="rgba(255,255,255,0.5)"
          strokeWidth="0.5"
        />
        {/* LOWER zone — distinct horizontal mesh slats with gaps */}
        {[0, 1, 2, 3, 4, 5, 6, 7].map((i) => (
          <g key={i}>{mesh(25.3, 18.4 + i * 2.7, 13.4, 1.3, 0.65)}</g>
        ))}
        {/* narrow inset label band (does not touch edges) — near the bottom */}
        <rect x="27.5" y="46" width="9" height="2.8" rx="0.4" fill={band} />
      </g>
    );
  }
  // Shure SM7B — symmetric foam/body either side of a wide middle seam; mount stub
  // on the middle-right at the seam. Foam top width == body bottom width.
  if (g === "mic_sm7b") {
    const shell = "#34363b",
      shellHi = "#46484e",
      shellDk = "#202125",
      foam = "#1d1e21";
    const wv = cid + "w";
    return (
      <g>
        <defs>
          <pattern id={wv} width="2" height="2" patternUnits="userSpaceOnUse">
            <rect width="2" height="2" fill={foam} />
            <circle cx="0.5" cy="0.5" r="0.4" fill="rgba(255,255,255,0.06)" />
            <circle cx="1.5" cy="1.5" r="0.4" fill="rgba(0,0,0,0.28)" />
          </pattern>
          <clipPath id={cid}>
            <path d="M26.7 4.6 Q23.9 4.6 23.7 8 L23.4 29.9 L40.6 29.9 L40.3 8 Q40.1 4.6 37.3 4.6 Z" />
          </clipPath>
        </defs>
        {/* mount stub on the middle-right at the seam */}
        <rect
          x="40"
          y="27.7"
          width="6.6"
          height="5.6"
          rx="1.8"
          fill={shellDk}
          stroke={edge}
          strokeWidth="0.7"
        />
        <rect
          x="44.6"
          y="27.9"
          width="1.8"
          height="5.2"
          rx="0.9"
          fill="rgba(0,0,0,0.3)"
        />
        {/* body — wide at the seam, narrowing to the base (mirror of the foam) */}
        <path
          d="M23 30.5 L41 30.5 L40.6 53 Q40.4 57 37.3 57 L26.7 57 Q23.6 57 23.4 53 Z"
          fill={shell}
          stroke={edge}
          strokeWidth="0.9"
          strokeLinejoin="round"
        />
        <path
          d="M25 30.5 L28 30.5 L27.7 56.6 L26 56.6 Z"
          fill={shellHi}
          opacity="0.3"
        />
        {/* foam windscreen — narrow top widening to the seam */}
        <path
          d="M26.7 4 Q23.6 4 23.4 8 L23 30.5 L41 30.5 L40.6 8 Q40.4 4 37.3 4 Z"
          fill={foam}
          stroke={edge}
          strokeWidth="0.9"
          strokeLinejoin="round"
        />
        <g clipPath={`url(#${cid})`}>
          <rect x="22" y="3" width="20" height="28" fill={`url(#${wv})`} />
        </g>
        {/* thick separation seam */}
        <line
          x1="23"
          y1="30.5"
          x2="41"
          y2="30.5"
          stroke="#141519"
          strokeWidth="1.8"
        />
      </g>
    );
  }
  // Royer R-121 — M23 chrome-cylinder base; the body is ONE rectangle with horizontal
  // grille slots on its upper section, flanked by two narrow vertical mesh "ears".
  if (g === "mic_ribbon") {
    const shell = "#c6c9ce",
      shellHi = "#eceef0",
      shellDk = "#9b9ea4",
      slot = "#3a3c40",
      green = "#3f7d63",
      greenEdge = "#cfeadd";
    return (
      <g>
        <defs>
          <clipPath id={cid}>
            <rect x="26.5" y="5" width="11" height="52" rx="1.4" />
          </clipPath>
        </defs>
        {/* two narrow plain-metal ears flanking the grille (outer corners rounded, inner square) */}
        <path
          d="M26.5 10 L24.9 10 Q23.6 10 23.6 11.3 L23.6 26.2 Q23.6 27.5 24.9 27.5 L26.5 27.5 Z"
          fill={shell}
          stroke={edge}
          strokeWidth="0.6"
          strokeLinejoin="round"
        />
        <path
          d="M37.5 10 L39.1 10 Q40.4 10 40.4 11.3 L40.4 26.2 Q40.4 27.5 39.1 27.5 L37.5 27.5 Z"
          fill={shell}
          stroke={edge}
          strokeWidth="0.6"
          strokeLinejoin="round"
        />
        {/* single chrome body — grille and body in one rectangle */}
        <rect
          x="26.5"
          y="5"
          width="11"
          height="52"
          rx="1.4"
          fill={shell}
          stroke={edge}
          strokeWidth="0.8"
        />
        <rect
          x="27.7"
          y="6.2"
          width="2.4"
          height="49.4"
          rx="1.2"
          fill={shellHi}
          opacity="0.6"
        />
        <rect
          x="35.3"
          y="6.2"
          width="1.5"
          height="49.4"
          rx="0.75"
          fill={shellDk}
          opacity="0.5"
        />
        {/* horizontal grille slots on the upper section */}
        <g clipPath={`url(#${cid})`}>
          {Array.from({ length: 8 }, (_, i) => (
            <rect
              key={i}
              x="27.3"
              y={10.5 + i * 2.2}
              width="9.4"
              height="1.1"
              rx="0.5"
              fill={slot}
            />
          ))}
        </g>
        {/* top screw */}
        <circle cx="32" cy="6.8" r="0.8" fill="rgba(0,0,0,0.28)" />
        {/* green ROYER oval badge */}
        <ellipse
          cx="32"
          cy="35"
          rx="2.4"
          ry="5"
          fill={green}
          stroke={greenEdge}
          strokeWidth="0.5"
        />
        {/* lower screw */}
        <circle cx="32" cy="52" r="0.8" fill="rgba(0,0,0,0.28)" />
      </g>
    );
  }
  // Shure SM57 — VERTICAL, slim: grey vertical-bar grille on top, then a hexagonal
  // body (short straight shoulders tapering to a narrower base) + connector barrel.
  if (g === "mic_sm57") {
    const shell = "#34363b",
      shellDk = "#202125",
      grey = "#9a9da2",
      bar = "#15161a";
    return (
      <g>
        <defs>
          <clipPath id={cid}>
            <path d="M26 7 Q26 5 28 5 L36 5 Q38 5 38 7 L38 13.5 L26 13.5 Z" />
          </clipPath>
        </defs>
        {/* connector barrel at the base */}
        <rect
          x="29"
          y="51"
          width="6"
          height="7"
          rx="1.4"
          fill={shellDk}
          stroke={edge}
          strokeWidth="0.7"
        />
        {/* hexagonal body — straight shoulders, tapering to a narrower base */}
        <path
          d="M26 13.5 L38 13.5 L38 22 L36 52 Q36 52.6 35.4 52.6 L28.6 52.6 Q28 52.6 28 52 L26 22 Z"
          fill={shell}
          stroke={edge}
          strokeWidth="0.9"
          strokeLinejoin="round"
        />
        {/* grey grille with black vertical lines */}
        <path
          d="M26 7 Q26 5 28 5 L36 5 Q38 5 38 7 L38 13.5 L26 13.5 Z"
          fill={grey}
          stroke={edge}
          strokeWidth="0.9"
          strokeLinejoin="round"
        />
        <g clipPath={`url(#${cid})`}>
          {Array.from({ length: 7 }, (_, i) => (
            <line
              key={i}
              x1={27.5 + i * 1.5}
              y1="5"
              x2={27.5 + i * 1.5}
              y2="13.5"
              stroke={bar}
              strokeWidth="0.95"
            />
          ))}
        </g>
      </g>
    );
  }
  // Earthworks M23 — VERTICAL: slim chrome pencil condenser. Thin rounded capsule
  // tip up top, a smooth flared taper widening down to a black M23 label ring,
  // then a long straight brushed-steel body cylinder.
  const m23Shell = "#c6c9ce",
    m23Hi = "#eceef0",
    m23Dk = "#9b9ea4",
    m23Ring = "#1d1e21";
  return (
    <g>
      {/* long straight body cylinder */}
      <rect
        x="27"
        y="28.5"
        width="10"
        height="27.5"
        rx="1.4"
        fill={m23Shell}
        stroke={edge}
        strokeWidth="0.7"
      />
      <rect
        x="28.2"
        y="29.6"
        width="2.4"
        height="25.4"
        rx="1.2"
        fill={m23Hi}
        opacity="0.6"
      />
      <rect
        x="34.4"
        y="29.6"
        width="1.6"
        height="25.4"
        rx="0.8"
        fill={m23Dk}
        opacity="0.5"
      />
      {/* smooth capsule tip + flared taper */}
      <path
        d="M30.7 3.4 Q32 2.5 33.3 3.4 L33.5 11.5 C35.4 15 37 21 37 28 L27 28 C27 21 28.6 15 30.5 11.5 Z"
        fill={m23Shell}
        stroke={edge}
        strokeWidth="0.7"
        strokeLinejoin="round"
      />
      <path
        d="M31.1 3.6 Q31.8 3.1 32.2 3.4 L32.3 11.8 C30.8 15.4 29.6 21 29.4 27.8 L28.4 27.8 C28.6 21 29.8 15 31 11.8 Z"
        fill={m23Hi}
        opacity="0.5"
      />
      {/* black label ring at the flare/body junction */}
      <rect
        x="26.7"
        y="27.1"
        width="10.6"
        height="3.5"
        rx="0.6"
        fill={m23Ring}
        stroke="rgba(0,0,0,0.4)"
        strokeWidth="0.4"
      />
    </g>
  );
}
