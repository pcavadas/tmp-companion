// src/ui/blockart/micBodiesCondenser.tsx — condenser/ribbon mic bodies (AKG C414,
// Sennheiser MD421, Royer R-121 ribbon, Earthworks M23 pencil), split from ./mics
// so each file stays ≤500 lines. The dynamic mics live in ./micBodiesDynamic.

const edge = "rgba(0,0,0,0.45)";

// AKG C414 — VERTICAL: squared charcoal head with a champagne-gold mesh grille
// up top, the XLS selector window + green LED, polar-pattern dot row, the round
// blue AKG badge, "C 414" label, and the cylindrical XLR barrel at the bottom.
export function MicC414Body({ uid }: { uid: string }) {
  const cid = "mc" + uid;
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
export function Mic421Body({ uid }: { uid: string }) {
  const cid = "mc" + uid;
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
      <line x1="23" y1="20" x2="41" y2="20" stroke={meshDk} strokeWidth="0.8" />
    </g>
  );
}

// Royer R-121 — M23 chrome-cylinder base; the body is ONE rectangle with horizontal
// grille slots on its upper section, flanked by two narrow vertical mesh "ears".
export function MicRibbonBody({ uid }: { uid: string }) {
  const cid = "mc" + uid;
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

// Earthworks M23 — VERTICAL: slim chrome pencil condenser. Thin rounded capsule
// tip up top, a smooth flared taper widening down to a black M23 label ring,
// then a long straight brushed-steel body cylinder. (MicBody's default form.)
export function MicPencilBody() {
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
