// src/ui/BlockArt.tsx — the public block-art engine: BlockArt (single icon)
// + HalfStackArt (head-over-cab). Dispatches an icon to its form-factor renderer
// (formFor → amps/cabs/mics/forms/pedals). The procedural-SVG bodies + shared
// helpers live under ./blockart/. Both are pure functions of their props (plus the
// theme context), so both are `React.memo`'d: a Catalog wall or a chain strip that
// re-renders for an unrelated reason skips redrawing every procedural SVG.
import React from "react";
import { useTheme } from "../theme/ThemeContext";
import { formFor, toneOf } from "./blockart/shared";
import type { ArtVariant, IconId, ToneId } from "./blockart/shared";
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
  size?: number;
  label?: boolean;
  icon?: IconId;
  tone?: ToneId;
  /** caption text (display only — art never keys on it; see `variant`). */
  lab?: string;
  /** explicit per-model art treatment — from the catalog BlockArtSpec. */
  variant?: ArtVariant;
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
  topIcon?: IconId;
  topTone?: ToneId;
  /** the head's explicit art treatment (the cab carries none). */
  topVariant?: ArtVariant;
  cabIcon?: IconId;
  cabTone?: ToneId;
  cabW?: number;
}

// Per-instance id prefix for the bodies' gradient/clip/pattern ids, from React's
// useId so ids are unique per render tree (and stable across SSR/hydration). useId
// can contain characters (`:`, `«»`) that break an unescaped `url(#…)` reference,
// so keep only the id-safe ones.
function useArtUid(): string {
  return "u" + React.useId().replace(/[^A-Za-z0-9_-]/g, "");
}

// Side margin so a body's drop-shadow isn't clipped by the half-stack viewBox.
const MX = 4;

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

const HalfStackArt = React.memo(function HalfStackArt({
  topIcon,
  topTone,
  topVariant,
  cabIcon,
  cabTone,
  cabW = 72,
}: HalfStackArtProps) {
  const topRef = React.useRef<SVGGElement>(null);
  const cabRef = React.useRef<SVGGElement>(null);
  const [dims, setDims] = React.useState<HalfStackDims | null>(null);
  const uid = useArtUid();
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
  }, [topIcon, topTone, topVariant, cabIcon, cabTone, cabW]);

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
          variant={topVariant}
          uid={`${uid}t`}
        />
      </g>
      <g ref={cabRef} transform={dims ? dims.cabTf : undefined}>
        <CabBody c={cCab} t={cabTone ?? ""} g={cabIcon ?? ""} uid={`${uid}c`} />
      </g>
    </svg>
  );
});

// Catalog form-factor → amp chassis icon. The SAME amp block_id is catalogued as
// both a combo and a head (different `form`, identical icon), so the icon alone
// can't pick the chassis — when a caller passes the catalog `form` we honor it.
const AMP_FORM_ICON: Record<"combo" | "head" | "half_stack", IconId> = {
  combo: "combo",
  head: "amp",
  half_stack: "stack",
};

const BlockArt = React.memo(function BlockArt({
  size = 56,
  label = true,
  icon,
  tone,
  lab,
  variant,
  dim,
  footswitch,
  bodyColor,
  accentColor,
  panelColor,
  form,
}: BlockArtProps) {
  const { t } = useTheme();
  const def = { t: tone ?? "slate", g: icon ?? "knobs2", lab: lab ?? "" };
  const base = toneOf(def.t);
  const c = bodyColor ? { ...base, body: bodyColor } : base;
  const ff = formFor(def.g);
  // For amps, the catalog form selects the chassis (combo vs head vs half-stack)
  // that the per-id icon can't distinguish; non-amp forms ignore it.
  const ampG = ff === "amp" && form != null ? AMP_FORM_ICON[form] : def.g;
  const uid = useArtUid();
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
            <AmpBody c={c} t={def.t} g={ampG} variant={variant} uid={uid} />
          ))}
        {ff === "cab" &&
          (def.t === "bluesbreaker" ? (
            <BluesbreakerBody c={c} uid={uid} />
          ) : (
            <CabBody c={c} t={def.t} g={def.g} variant={variant} uid={uid} />
          ))}
        {ff === "ir" && <IRBody uid={uid} />}
        {ff === "extcab" && <IRBody uid={uid} label="EXT" />}
        {ff === "mic" && <MicBody c={c} g={def.g} lab={def.lab} uid={uid} />}
        {ff === "treadle" && <TreadleBody c={c} g={def.g} lab={def.lab} />}
        {ff === "round" && <RoundBody c={c} lab={def.lab} uid={uid} />}
        {ff === "rack" && <RackBody c={c} g={def.g} variant={variant} />}
        {ff === "desk" && <DeskBody c={c} lab={def.lab} />}
        {ff === "screen" && <ScreenBody lab={def.lab} />}
        {ff === "rockbox" && <RockboxBody c={c} lab={def.lab} />}
        {ff === "pedal" && (
          <PedalBody
            c={c}
            g={def.g}
            lab={def.lab}
            variant={variant}
            footswitch={footswitch ?? "round"}
            uid={uid}
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
});

export { BlockArt, HalfStackArt };

export type { ArtVariant, IconId, ToneId } from "./blockart/shared";
