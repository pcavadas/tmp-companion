// src/ui/blockart/partsPanel.tsx — panel/knob/accent COMPONENTS of the block-art
// engine (alu gradient + skirted knob + silverface panel + EVH accent + corner
// protectors). Split from ./parts so each file stays ≤500 lines. The cloth/chassis/
// speaker components live in ./partsCloth; shared data/helpers/types in ./shared.

// The 2-stop (3-step) brushed-aluminium gradient used for silverface panels +
// trim bezels. Emit once per uid into <defs>; reference via url(#<id>).
export function AluGradient({ id }: { id: string }) {
  return (
    <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stopColor="#c2c6ca" />
      <stop offset="0.5" stopColor="#a7abaf" />
      <stop offset="1" stopColor="#8d9195" />
    </linearGradient>
  );
}

// A skirted Fender knob (chrome skirt under a dark cap + a light ~12 o'clock
// pointer). Ref: the six combo knobs at r1.95/1.2.
export function SkirtedKnob({
  x,
  y,
  skirt = 1.95,
  cap = 1.2,
  ptr = "#eef1f4",
}: {
  x: number;
  y: number;
  skirt?: number;
  cap?: number;
  ptr?: string;
}) {
  return (
    <g transform={`translate(${String(x)},${String(y)})`}>
      <circle
        r={skirt}
        fill="#cfd3d7"
        stroke="rgba(0,0,0,0.4)"
        strokeWidth="0.3"
      />
      <circle r={cap} fill="#1c1d20" />
      <line
        x1="0"
        y1="0"
        x2="0"
        y2={-(skirt - 0.45)}
        stroke={ptr}
        strokeWidth="0.55"
        strokeLinecap="round"
      />
    </g>
  );
}

// The silverface brushed-alu control panel: alu-gradient plate + top highlight +
// blue silkscreen rule, two input jacks, `nKnobs` skirted knobs, and the red
// pilot jewel. Geometry matches the combo reference (panel x9 w46, cy 12.9).
export function SilverfacePanel({
  x = 9,
  y = 9,
  w = 46,
  gid,
  nKnobs = 6,
}: {
  x?: number;
  y?: number;
  w?: number;
  gid: string;
  nKnobs?: number;
}) {
  const cy = y + 3.9;
  const k0 = 18,
    kStep = 6.2;
  return (
    <g>
      <rect
        x={x}
        y={y}
        width={w}
        height="7.6"
        rx="1.2"
        fill={`url(#${gid})`}
        stroke="rgba(0,0,0,0.4)"
        strokeWidth="0.4"
      />
      <rect
        x={x + 0.4}
        y={y + 0.5}
        width={w - 0.8}
        height="0.7"
        rx="0.35"
        fill="#dfe3e6"
        opacity="0.7"
      />
      {/* blue silkscreen rule */}
      <rect
        x={x + 2}
        y={y + 1.4}
        width={w - 4}
        height="0.5"
        rx="0.25"
        fill="#2f6c98"
        opacity="0.55"
      />
      {/* two input jacks */}
      <circle
        cx={x + 2.6}
        cy={cy}
        r="0.95"
        fill="rgba(0,0,0,0.55)"
        stroke="#cfd3d7"
        strokeWidth="0.35"
      />
      <circle
        cx={x + 5.1}
        cy={cy}
        r="0.95"
        fill="rgba(0,0,0,0.55)"
        stroke="#cfd3d7"
        strokeWidth="0.35"
      />
      {/* skirted knob row */}
      {Array.from({ length: nKnobs }, (_, i) => (
        <SkirtedKnob key={i} x={k0 + i * kStep} y={cy} />
      ))}
      {/* red pilot jewel */}
      <circle
        cx={x + w - 1.8}
        cy={cy}
        r="1.25"
        fill="#d24a3a"
        stroke="rgba(60,8,4,0.6)"
        strokeWidth="0.3"
      />
    </g>
  );
}

// The abstract EVH geometric corner accent — an outlined square rotated 45° + a
// short internal slash. NEVER the striped trade dress. `cx,cy` = square centre,
// `s` = side. Ref: 6×6 square @ (47.6,12.6) + slash.
export function EvhAccent({
  cx = 47.6,
  cy = 12.6,
  s = 6,
  stroke = "#eef0f2",
}: {
  cx?: number;
  cy?: number;
  s?: number;
  stroke?: string;
}) {
  return (
    <g
      stroke={stroke}
      strokeWidth="0.85"
      fill="none"
      opacity="0.92"
      strokeLinejoin="round"
    >
      <rect
        x={cx - s / 2}
        y={cy - s / 2}
        width={s}
        height={s}
        rx="0.6"
        transform={`rotate(45 ${String(cx)} ${String(cy)})`}
      />
      <line x1={cx - 1.6} y1={cy} x2={cx + 1.6} y2={cy} />
    </g>
  );
}

// Four dark corner protectors at the grille corners (ref: #0d0d0e quarter-shapes).
// gx,gy = grille top-left; gw,gh = grille size; q = leg length.
export function CornerProtectors({
  gx,
  gy,
  gw,
  gh,
  q = 4.5,
  fill = "#0d0d0e",
}: {
  gx: number;
  gy: number;
  gw: number;
  gh: number;
  q?: number;
  fill?: string;
}) {
  const gx2 = gx + gw,
    gy2 = gy + gh;
  return (
    <g fill={fill}>
      <path
        d={`M${String(gx)} ${String(gy)} H${String(gx + q)} A1 1 0 0 1 ${String(gx + q - 1)} ${String(gy + q)} H${String(gx)} Z`}
      />
      <path
        d={`M${String(gx2)} ${String(gy)} H${String(gx2 - q)} A1 1 0 0 0 ${String(gx2 - q + 1)} ${String(gy + q)} H${String(gx2)} Z`}
      />
      <path
        d={`M${String(gx)} ${String(gy2)} H${String(gx + q)} A1 1 0 0 0 ${String(gx + q - 1)} ${String(gy2 - q)} H${String(gx)} Z`}
      />
      <path
        d={`M${String(gx2)} ${String(gy2)} H${String(gx2 - q)} A1 1 0 0 1 ${String(gx2 - q + 1)} ${String(gy2 - q)} H${String(gx2)} Z`}
      />
    </g>
  );
}
