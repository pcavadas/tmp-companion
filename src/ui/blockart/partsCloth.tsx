// src/ui/blockart/partsCloth.tsx — cloth/grille/chassis/speaker COMPONENTS of the
// block-art engine (weave patterns + grille cloth + chassis body + speaker). Split
// from ./parts so each file stays ≤500 lines. Shared data/helpers/types live in
// ./shared; the panel/knob/accent components live in ./partsPanel.
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
