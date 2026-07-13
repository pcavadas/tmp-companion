// src/ui/BlockArt.tsx — the public block-art engine: BlockArt (single icon)
// + HalfStackArt (head-over-cab) + the legacy short-code map. Dispatches a
// icon to its form-factor renderer (formFor → amps/cabs/mics/forms/pedals).
// The procedural-SVG bodies + shared helpers live under ./blockart/.
import React from "react";
import { useTheme } from "../theme/ThemeContext";
import { formFor, toneOf } from "./blockart/shared";
import { AmpBody } from "./blockart/amps";
import { CabBody, IRBody, BluesbreakerBody } from "./blockart/cabs";
import { MicBody } from "./blockart/mics";
import {
  TreadleBody,
  RoundBody,
  RackBody,
  DeskBody,
  ScreenBody,
  RockboxBody,
} from "./blockart/forms";
import { PedalBody } from "./blockart/pedals";

export interface BlockArtProps {
  code?: string;
  size?: number;
  label?: boolean;
  icon?: string;
  tone?: string;
  lab?: string;
  dim?: boolean;
  /** Stompbox footswitch style (pedal form only) — from the catalog BlockArtSpec. */
  footswitch?: "plate" | "metal" | "round";
  /** ref-derived per-block body color (pedals) — overlays the tone default. */
  bodyColor?: string;
  /** Fender reverb-chassis accent (footswitch colour) — triggers the cream chassis. */
  accentColor?: string;
  /** ref-derived colour of a recessed control panel behind the knobs/sliders (pedals). */
  panelColor?: string;
  /** Catalog form-factor — for amps, picks the combo/head/half-stack chassis the
   *  per-id icon can't distinguish (the same block_id is both a combo and a head). */
  form?: "combo" | "head" | "half_stack";
}
export interface HalfStackArtProps {
  topIcon?: string;
  topTone?: string;
  topLab?: string;
  cabIcon?: string;
  cabTone?: string;
  cabLab?: string;
  cabW?: number;
  size?: number;
}

interface BlockDef {
  t: string;
  g: string;
  lab: string;
  fam: string;
}

// Unique-id counter for per-instance gradient/clip ids (mutated here only — the
// bodies receive `uid` as a prop).
let __gcUid = 0;
const nextUid = (): string => `u${String(++__gcUid)}`;

// Side margin so a body's drop-shadow isn't clipped by the half-stack viewBox.
const MX = 4;

const BLOCK_MAP: Record<string, BlockDef> = {
  COMP: { t: "slate", g: "comp", lab: "COMP", fam: "Dynamics" },
  GATE: { t: "slate", g: "gate", lab: "GATE", fam: "Dynamics" },
  "TS-OD": { t: "green", g: "od", lab: "TS", fam: "Drive" },
  "OD-K": { t: "gold", g: "od", lab: "KLON", fam: "Drive" },
  "OD-T": { t: "amber", g: "od", lab: "OD", fam: "Drive" },
  FUZZ: { t: "muff", g: "fuzz", lab: "FUZZ", fam: "Drive" },
  OCT: { t: "teal", g: "octave", lab: "OCT", fam: "Pitch" },
  CHO: { t: "blue", g: "chorus", lab: "CHO", fam: "Modulation" },
  PHA: { t: "yellow", g: "phaser", lab: "PHA", fam: "Modulation" },
  FLT: { t: "plum", g: "envf", lab: "FLT", fam: "Modulation" },
  TREM: { t: "blackface", g: "tremolo", lab: "TREM", fam: "Modulation" },
  WAH: { t: "ink", g: "wah", lab: "WAH", fam: "Filter" },
  VOL: { t: "ink", g: "vol", lab: "VOL", fam: "Utility" },
  "EQ-7": { t: "slate", g: "eq", lab: "EQ", fam: "EQ" },
  "AMP-T": { t: "tweed", g: "amp", lab: "TWEED", fam: "Amp" },
  "AMP-F": { t: "blackface", g: "amp", lab: "BLACK", fam: "Amp" },
  "AMP-V": { t: "vox", g: "amp", lab: "VOX", fam: "Amp" },
  "AMP-P": { t: "marshall", g: "amp", lab: "PLEXI", fam: "Amp" },
  "AMP-B": { t: "vox", g: "amp", lab: "BOUT", fam: "Amp" },
  "AMP-M": { t: "boutique", g: "amp", lab: "MODRN", fam: "Amp" },
  "AMP-BS": { t: "bass", g: "amp", lab: "BASS", fam: "Amp" },
  "CAB-T": { t: "blackface", g: "cab1", lab: "1x12", fam: "Cab" },
  "CAB-G": { t: "marshall", g: "cab2", lab: "2x12", fam: "Cab" },
  "CAB-M": { t: "marshall", g: "cab4", lab: "4x12", fam: "Cab" },
  "CAB-BS": { t: "ampeg", g: "cab8", lab: "8x10", fam: "Cab" },
  "DLY-D": { t: "mint", g: "delay", lab: "DLY", fam: "Delay" },
  "DLY-T": { t: "gold", g: "delay", lab: "TAPE", fam: "Delay" },
  "REV-PLT": { t: "lake", g: "plate", lab: "PLATE", fam: "Reverb" },
  "REV-SPR": { t: "blackface", g: "spring", lab: "SPR", fam: "Reverb" },
  "REV-HL": { t: "lake", g: "hall", lab: "HALL", fam: "Reverb" },
  "REV-SHM": { t: "mint", g: "shimmer", lab: "SHIM", fam: "Reverb" },
};

// ---- resolve a block descriptor from props/code ----------------------------
function resolveBlock({
  code,
  icon,
  tone,
  lab,
}: Pick<BlockArtProps, "code" | "icon" | "tone" | "lab">): {
  t: string;
  g: string;
  lab: string;
} {
  const base: BlockDef | undefined =
    code != null
      ? (BLOCK_MAP as Partial<Record<string, BlockDef>>)[code]
      : undefined;
  return {
    t: tone ?? base?.t ?? "slate",
    g: icon ?? base?.g ?? "knobs2",
    lab: lab ?? base?.lab ?? code ?? "",
  };
}

// ============================================================================
// <BlockArt /> — one block icon (+ optional caption)
// ============================================================================

// ---------------------------------------------------------------------------
// Half-stack composite — a short amp head (or, for combo-modeled amps, the
// combo's livery adapted to head height) resting on its default speaker cab
// with a small gap between them.
//   • The top is ALWAYS scaled UNIFORMLY (every element stays undistorted —
//     round knobs, round badge, square grille weave).
//   • The top is fitted to the cab's EXACT drawn width; its wide-short
//     proportions then land it at ~2/5 the cab's height.
// Footprints are measured with getBBox so the fit holds for any body.
// ---------------------------------------------------------------------------
interface HalfStackDims {
  cabTf: string;
  topTf: string;
  W: number;
  H: number;
}

function HalfStackArt({
  topIcon,
  topTone,
  topLab,
  cabIcon,
  cabTone,
  cabLab,
  cabW = 72,
}: HalfStackArtProps) {
  const topRef = React.useRef<SVGGElement>(null);
  const cabRef = React.useRef<SVGGElement>(null);
  const [dims, setDims] = React.useState<HalfStackDims | null>(null);
  const uid = React.useMemo(() => nextUid(), []);
  const cTop = toneOf(topTone);
  const cCab = toneOf(cabTone);

  React.useLayoutEffect(() => {
    if (!topRef.current || !cabRef.current) return;
    const tb = topRef.current.getBBox();
    const cb = cabRef.current.getBBox();
    if (!cb.width || !cb.height || !tb.width || !tb.height) return;
    // ONE uniform scale for BOTH bodies — the cab fills the target width and the
    // head is drawn to the same width, so this just stacks the real head over
    // the real cab. No independent stretch/scale, so every shape stays true.
    const s = cabW / cb.width;
    const cabWpx = cb.width * s,
      cabHpx = cb.height * s;
    const topWpx = tb.width * s,
      topHpx = tb.height * s;
    const GAP = Math.max(3, cabHpx * 0.05); // small gap between head and cab
    const topX = MX + (cabWpx - topWpx) / 2; // centre the head over the cab
    const topTf = `translate(${String(topX)}, 0) scale(${String(s)}) translate(${String(-tb.x)}, ${String(-tb.y)})`;
    const cabTf = `translate(${String(MX)}, ${String(topHpx + GAP)}) scale(${String(s)}) translate(${String(-cb.x)}, ${String(-cb.y)})`;
    setDims({ cabTf, topTf, W: cabWpx + MX * 2, H: topHpx + GAP + cabHpx });
  }, [topIcon, topTone, topLab, cabIcon, cabTone, cabW]);

  const W = dims ? dims.W : cabW + MX * 2;
  const H = dims ? dims.H : cabW * 1.4;
  return (
    <svg
      viewBox={`0 0 ${String(W)} ${String(H)}`}
      width={W}
      height={H}
      style={{
        display: "block",
        filter: "drop-shadow(0 1px 2px rgba(15,17,21,0.18))",
        opacity: dims ? 1 : 0,
      }}
    >
      <g ref={topRef} transform={dims ? dims.topTf : undefined}>
        <AmpBody
          c={cTop}
          t={topTone ?? ""}
          g="head"
          lab={topLab ?? ""}
          uid={`${uid}t`}
        />
      </g>
      <g ref={cabRef} transform={dims ? dims.cabTf : undefined}>
        <CabBody
          c={cCab}
          t={cabTone ?? ""}
          g={cabIcon ?? ""}
          lab={cabLab ?? ""}
          uid={`${uid}c`}
        />
      </g>
    </svg>
  );
}

// Catalog form-factor → amp chassis icon. The SAME amp block_id is catalogued as
// both a combo and a head (different `form`, identical icon), so the icon alone
// can't pick the chassis — when a caller passes the catalog `form` we honor it.
const AMP_FORM_ICON: Record<"combo" | "head" | "half_stack", string> = {
  combo: "combo",
  head: "amp",
  half_stack: "stack",
};

function BlockArt({
  code,
  size = 56,
  label = true,
  icon,
  tone,
  lab,
  dim,
  footswitch,
  bodyColor,
  accentColor,
  panelColor,
  form,
}: BlockArtProps) {
  const { t } = useTheme();
  const def = resolveBlock({ code, icon, tone, lab });
  const base = toneOf(def.t);
  const c = bodyColor ? { ...base, body: bodyColor } : base;
  const ff = formFor(def.g);
  // For amps, the catalog form selects the chassis (combo vs head vs half-stack)
  // that the per-id icon can't distinguish; non-amp forms ignore it.
  const ampG = ff === "amp" && form != null ? AMP_FORM_ICON[form] : def.g;
  const uid = React.useMemo(() => nextUid(), []);
  return (
    <div
      style={{
        display: "inline-flex",
        flexDirection: "column",
        alignItems: "center",
        gap: t.space2,
        width: size,
        opacity: dim ? 0.4 : 1,
      }}
    >
      <svg
        viewBox="0 0 64 64"
        width={size}
        height={size}
        style={{
          display: "block",
          filter: "drop-shadow(0 1px 2px rgba(15,17,21,0.18))",
        }}
      >
        {/* The JTM45 Bluesbreaker family (combo, head, AND its 2x12 cabs) renders
            through ONE clean-front body so they're byte-identical — the tone short-
            circuits each form's normal renderer. */}
        {ff === "amp" &&
          (def.t === "bluesbreaker" ? (
            <BluesbreakerBody c={c} uid={uid} />
          ) : (
            <AmpBody c={c} t={def.t} g={ampG} lab={def.lab} uid={uid} />
          ))}
        {ff === "cab" &&
          (def.t === "bluesbreaker" ? (
            <BluesbreakerBody c={c} uid={uid} />
          ) : (
            <CabBody c={c} t={def.t} g={def.g} lab={def.lab} uid={uid} />
          ))}
        {ff === "ir" && <IRBody uid={uid} />}
        {ff === "extcab" && <IRBody uid={uid} label="EXT" />}
        {ff === "mic" && <MicBody c={c} g={def.g} lab={def.lab} uid={uid} />}
        {ff === "treadle" && <TreadleBody c={c} g={def.g} lab={def.lab} />}
        {ff === "round" && <RoundBody c={c} lab={def.lab} uid={uid} />}
        {ff === "rack" && <RackBody c={c} g={def.g} lab={def.lab} />}
        {ff === "desk" && <DeskBody c={c} lab={def.lab} />}
        {ff === "screen" && <ScreenBody lab={def.lab} />}
        {ff === "rockbox" && <RockboxBody c={c} lab={def.lab} />}
        {ff === "pedal" && (
          <PedalBody
            c={c}
            g={def.g}
            lab={def.lab}
            footswitch={footswitch ?? "round"}
            accent={accentColor}
            panel={panelColor}
          />
        )}
      </svg>
      {label && (
        <span
          style={{
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 9,
            color: "var(--tmp-muted)",
            letterSpacing: "0.06em",
            textTransform: "uppercase",
          }}
        >
          {def.lab}
        </span>
      )}
    </div>
  );
}

export { BlockArt, HalfStackArt };

export type { IconId, ToneId } from "./blockart/shared";
