// src/ui/blockart/micBodiesDynamic.tsx — dynamic mic bodies (EV RE20, Shure SM7B,
// Shure SM57), split from ./mics so each file stays ≤500 lines. The condenser/
// ribbon/pencil mics live in ./micBodiesCondenser.

const edge = "rgba(0,0,0,0.45)";

// EV RE20 — VERTICAL broadcast dynamic: a rounded-corner body of uniform width.
// Top zone = 4 square mesh windows (2×2, with gaps); a bevel groove; lower zone =
// distinct horizontal mesh slats; a narrow inset label band; mount barrel.
export function MicRe20Body({ uid }: { uid: string }) {
  const cid = "mc" + uid;
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
        <pattern id={wv} patternUnits="userSpaceOnUse" width="1.7" height="1.7">
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
      <rect x="24.5" y="15.7" width="15" height="1.2" fill="rgba(0,0,0,0.15)" />
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
export function MicSm7bBody({ uid }: { uid: string }) {
  const cid = "mc" + uid;
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

// Shure SM57 — VERTICAL, slim: grey vertical-bar grille on top, then a hexagonal
// body (short straight shoulders tapering to a narrower base) + connector barrel.
export function MicSm57Body({ uid }: { uid: string }) {
  const cid = "mc" + uid;
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
