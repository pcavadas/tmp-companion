// src/views/level/SignalPathFit.tsx — keeps the Level list's signal path whole: a long
// chain scales DOWN to fit the window instead of being cut or scrolled. The diagram code
// itself (ActiveSignalChainView and the frozen signalChain/ pieces) is unchanged — this
// wrapper only lays it out at its natural width and shrinks the picture.
//
// How: the inner box is `max-content` wide (never narrower than the strip, so the diagram
// lays out whole with nothing to scroll) and at least the full width (so a short chain
// still spans the white strip, centred by the diagram's own layout). A ResizeObserver reads
// the inner box's laid-out width — which a transform never changes — against the space
// available; the scale is min(1, available / natural), applied from the top-left corner,
// which maps the box exactly onto [0, available]. The outer box takes the scaled height,
// so the list below sits right under the picture and never jumps. At scale 1 nothing is
// transformed. The diagram draws its own 0.5 px hairline at its bottom edge; the outer box
// clips the last HAIRLINE_CLIP px of the diagram's bottom padding, hairline included, and
// draws the 1 px `border` line instead, so there is one line under the picture, not two.
// The measurement is the documented DOM-measurement exception to
// `set-state-in-effect`: one layout effect, guarded so an unchanged size keeps the same
// state and the render converges.

import { useLayoutEffect, useRef, useState, type ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";

/** How much of the diagram's bottom edge (its own hairline, inside its 16 px bottom
 *  padding) the wrapper clips, so only the wrapper's border shows. */
const HAIRLINE_CLIP = 2;

/** A measured fit: the scale and the inner box's natural (unscaled) height. */
interface Fit {
  scale: number;
  height: number;
}

/** Sub-pixel jitter is not a change: keep the previous object so the state converges. */
function settle(prev: Fit | null, next: Fit): Fit {
  return prev &&
    Math.abs(prev.scale - next.scale) < 0.001 &&
    Math.abs(prev.height - next.height) < 0.5
    ? prev
    : next;
}

export interface SignalPathFitProps {
  /** The accessible name of the picture ("Signal path of Verse — Split, the preset
   *  loaded on your Tone Master Pro"). */
  label: string;
  children: ReactNode;
}

export function SignalPathFit({ label, children }: SignalPathFitProps) {
  const { t } = useTheme();
  const outerRef = useRef<HTMLDivElement>(null);
  const innerRef = useRef<HTMLDivElement>(null);
  const [fit, setFit] = useState<Fit | null>(null);

  useLayoutEffect(() => {
    const measure = () => {
      const outer = outerRef.current;
      const inner = innerRef.current;
      if (!outer || !inner) return;
      const available = outer.clientWidth;
      const natural = inner.offsetWidth;
      if (available <= 0 || natural <= 0) return;
      const next = {
        scale: Math.min(1, available / natural),
        height: inner.offsetHeight,
      };
      setFit((prev) => settle(prev, next));
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    if (outerRef.current) ro.observe(outerRef.current);
    if (innerRef.current) ro.observe(innerRef.current);
    return () => {
      ro.disconnect();
    };
  }, []);

  const scaled = fit != null && fit.scale < 1;
  const height =
    fit != null
      ? Math.max(0, fit.height * fit.scale - HAIRLINE_CLIP)
      : undefined;
  return (
    <div
      ref={outerRef}
      role="figure"
      aria-label={label}
      style={{
        flexShrink: 0,
        position: "relative",
        overflow: "hidden",
        background: t.surface,
        borderBottom: `1px solid ${t.border}`,
        height,
      }}
    >
      <div
        ref={innerRef}
        style={{
          width: "max-content",
          minWidth: "100%",
          transform: scaled ? `scale(${String(fit.scale)})` : undefined,
          transformOrigin: "top left",
        }}
      >
        {children}
      </div>
    </div>
  );
}
