// src/ui/blockart/parts.tsx — the procedural-SVG building-block COMPONENTS of the
// block-art engine (split from ./shared so that module exports only data + helper
// functions and this one only components — each satisfies React Fast Refresh's
// component-boundary rule). Shared data/helpers/types live in ./shared.
import { clothFor, TWEED_BODY, type Cloth, type PedalTone } from "./shared";

export function WeavePattern({ id, cl }: { id: string; cl: Cloth }) {
  const { weave, line, lineW, op } = cl;
  const p = { id, patternUnits: "userSpaceOnUse" };
  if (weave === "twill")
    return (
      <pattern {...p} width="3.4" height="3.4">
        <path
          d="M-0.6 4 L4 -0.6 M1.1 4.6 L4.6 1.1 M-1.1 1.1 L1.1 -1.1"
          stroke={line}
          strokeWidth={lineW}
          opacity={op}
        />
        <path
          d="M0 3.9 L3.9 0"
          stroke={line}
          strokeWidth={lineW * 0.6}
          opacity={op * 0.4}
          transform="translate(1.7 1.7)"
        />
      </pattern>
    );
  if (weave === "diamond")
    return (
      <pattern {...p} width="5" height="5">
        <path
          d="M2.5 0 L5 2.5 L2.5 5 L0 2.5 Z"
          fill="none"
          stroke={line}
          strokeWidth={lineW}
          opacity={op}
        />
      </pattern>
    );
  if (weave === "speckle")
    // fine salt-and-pepper flecks (Marshall / SVT grille) — small tile so it
    // reads as cloth, not polka dots
    return (
      <pattern {...p} width="2.6" height="2.6">
        <circle cx="0.7" cy="0.9" r="0.3" fill={line} opacity={op} />
        <circle cx="1.9" cy="1.8" r="0.3" fill={line} opacity={op} />
        <circle cx="1.5" cy="0.3" r="0.2" fill={line} opacity={op * 0.6} />
        <circle cx="0.3" cy="2.1" r="0.2" fill={line} opacity={op * 0.6} />
      </pattern>
    );
  if (weave === "basket")
    return (
      <pattern {...p} width="4" height="4">
        <path
          d="M0.3 0.9 H3.7 M0.3 1.5 H3.7"
          stroke={line}
          strokeWidth={lineW}
          opacity={op}
        />
        <path
          d="M1.0 2.4 V3.8 M1.8 2.4 V3.8 M2.6 2.4 V3.8 M3.4 2.4 V3.8"
          stroke={line}
          strokeWidth={lineW}
          opacity={op}
        />
      </pattern>
    );
  if (weave === "lattice")
    return (
      <pattern {...p} width="6" height="6">
        <g fill="none" stroke={line} strokeWidth={lineW} opacity={op}>
          <circle cx="3" cy="3" r="3" />
          <circle cx="0" cy="0" r="3" />
          <circle cx="6" cy="0" r="3" />
          <circle cx="0" cy="6" r="3" />
          <circle cx="6" cy="6" r="3" />
        </g>
      </pattern>
    );
  // 1.8 silverface sparkle (ref: sf-spark) — fine grid + light/dark speckle flecks
  // over the tinted-silver base GrilleCloth already painted. 4.4px tile.
  if (weave === "sparkle") {
    const lt = cl.sparkLt ?? "#d2dedb",
      dk = cl.sparkDk ?? "#5d736f";
    return (
      <pattern {...p} width="4.4" height="4.4">
        <path
          d="M0 0 H4.4 M0 0 V4.4"
          stroke={line}
          strokeWidth={lineW}
          opacity={op}
        />
        <circle cx="1.1" cy="1.4" r="0.45" fill={lt} opacity="0.75" />
        <circle cx="3.3" cy="3.0" r="0.45" fill={lt} opacity="0.75" />
        <circle cx="2.7" cy="0.6" r="0.3" fill={dk} opacity="0.6" />
        <circle cx="0.5" cy="3.6" r="0.3" fill={dk} opacity="0.6" />
      </pattern>
    );
  }
  // 1.8 EVH fine dark grid (ref: evh-grille) — 2.1px grid over the near-black base.
  if (weave === "evhgrid")
    return (
      <pattern {...p} width="2.1" height="2.1">
        <path
          d="M0 0 H2.1 M0 0 V2.1"
          stroke={line}
          strokeWidth={lineW}
          opacity={op}
        />
      </pattern>
    );
  // grid (fine cross-weave) — small tile so it reads as cloth, not graph paper
  return (
    <pattern {...p} width="1.3" height="1.3">
      <path
        d="M0 0 H1.3 M0 0 V1.3"
        stroke={line}
        strokeWidth={lineW}
        opacity={op}
      />
    </pattern>
  );
}

// A grille-cloth rectangle: base colour + tiled weave (+ optional piping).
export function GrilleCloth({
  x,
  y,
  w,
  h,
  rx = 1.5,
  tone,
  uid,
  cloth,
}: {
  x: number;
  y: number;
  w: number;
  h: number;
  rx?: number;
  tone: string;
  uid: string;
  /** explicit cloth recipe, overriding the tone→cloth lookup (e.g. a Mesa cab's
   *  diamond-mesh grille that its combo, sharing the tone, must NOT get). */
  cloth?: Cloth;
}) {
  const cl = cloth ?? clothFor(tone);
  const pid = "gc" + uid;
  return (
    <g>
      <defs>
        <WeavePattern id={pid} cl={cl} />
      </defs>
      <rect
        x={x}
        y={y}
        width={w}
        height={h}
        rx={rx}
        fill={cl.base}
        stroke="rgba(0,0,0,0.5)"
        strokeWidth="0.5"
      />
      <rect
        x={x}
        y={y}
        width={w}
        height={h}
        rx={rx}
        fill={"url(#" + pid + ")"}
      />
      {cl.border && (
        <rect
          x={x + 0.7}
          y={y + 0.7}
          width={w - 1.4}
          height={h - 1.4}
          rx={Math.max(rx - 0.5, 0.5)}
          fill="none"
          stroke={cl.border}
          strokeWidth="1"
          opacity="0.9"
        />
      )}
    </g>
  );
}

// Chassis body — flat tolex for most amps; tweed amps get a woven mustard-twill
// overlay so they read as lacquered tweed rather than a flat yellow box.
export function ChassisBody({
  x,
  y,
  w,
  h,
  rx = 3,
  c,
  t,
  edge = "rgba(0,0,0,0.4)",
  uid,
  k = "",
}: {
  x: number;
  y: number;
  w: number;
  h: number;
  rx?: number;
  c: PedalTone;
  t: string;
  edge?: string;
  uid: string;
  k?: string;
}) {
  if (t === "tweed") {
    const pid = "tw" + uid + k;
    return (
      <g>
        <defs>
          <pattern
            id={pid}
            patternUnits="userSpaceOnUse"
            width="2.6"
            height="2.6"
          >
            <rect width="2.6" height="2.6" fill={TWEED_BODY.base} />
            {/* smooth diagonal twill: one fine dark thread + one light sheen line */}
            <path
              d="M-0.3 2.9 L2.9 -0.3"
              stroke={TWEED_BODY.line}
              strokeWidth={TWEED_BODY.lineW}
              opacity={TWEED_BODY.op}
            />
            <path
              d="M-0.3 1.6 L1.6 -0.3"
              stroke="#e6cf8f"
              strokeWidth="0.3"
              opacity="0.45"
            />
          </pattern>
        </defs>
        <rect
          x={x}
          y={y}
          width={w}
          height={h}
          rx={rx}
          fill={"url(#" + pid + ")"}
          stroke={edge}
          strokeWidth="0.7"
        />
      </g>
    );
  }
  return (
    <rect
      x={x}
      y={y}
      width={w}
      height={h}
      rx={rx}
      fill={c.body}
      stroke={edge}
      strokeWidth="0.7"
    />
  );
}

// One speaker drawn on a grille (ring colour comes from the cloth).
export function Speaker({
  x,
  y,
  r,
  cl,
}: {
  x: number;
  y: number;
  r: number;
  cl: Cloth;
}) {
  return (
    <g>
      <circle
        cx={x}
        cy={y}
        r={r}
        fill="none"
        stroke={cl.ring}
        strokeWidth="0.75"
      />
      <circle
        cx={x}
        cy={y}
        r={r * 0.62}
        fill="none"
        stroke={cl.ring}
        strokeWidth="0.5"
        opacity="0.7"
      />
      <circle cx={x} cy={y} r={r * 0.26} fill={cl.ring} opacity="0.55" />
    </g>
  );
}

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
