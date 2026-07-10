// src/ui/Skeleton.tsx — the shared loading-state primitives (design handoff:
// "TMP Companion — Loading States").
//
// Two pieces ride every slow-to-arrive region (preset list · signal diagram ·
// song list). Both are honest by construction: a loading region looks like the
// thing that is coming (skeletons REUSE the real row/grid geometry, so the
// placeholders fill in place with zero layout shift), and a small mono status
// caption marks the wait as work-in-progress so a slow load never reads as the
// separate "no device" empty state.
//
//   • <Skel/>        — one shimmer placeholder bar (the `.tmp-skel` fill +
//                      keyframes live in index.html so a single sweep animates
//                      app-wide; reduced-motion swaps it for an opacity pulse).
//   • <SkelStatus/>  — the spinner + mono "still working" label.
//
// The per-region row skeletons (preset rows, library rows, rail rows) are
// co-located with the real rows they mirror (PresetList / SongsView) so the
// grid template lives in exactly one place.

import type { CSSProperties } from "react";

import { useTheme } from "../theme/ThemeContext";
import { Spinner } from "./Spinner";

export interface SkelProps {
  /** width (px or any CSS length). */
  w: number | string;
  /** height in px (default 10). */
  h?: number;
  /** corner radius in px (default 4). */
  r?: number;
  style?: CSSProperties;
}

/** One shimmer placeholder bar. Non-shrinking so it holds its width inside a
 *  flex row. */
export function Skel({ w, h = 10, r = 4, style }: SkelProps) {
  return (
    <div
      className="tmp-skel"
      style={{ width: w, height: h, borderRadius: r, flexShrink: 0, ...style }}
    />
  );
}

export interface SkelStatusProps {
  /** the mono "still working" label, e.g. "Reading presets…". */
  label: string;
  style?: CSSProperties;
}

/** Spinner icon + a small mono caption. Disambiguates a slow load from the
 *  disconnected empty state (README: "still working"). */
export function SkelStatus({ label, style }: SkelStatusProps) {
  const { t } = useTheme();
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 8,
        fontFamily: t.mono,
        fontSize: t.fsMeta,
        letterSpacing: "0.04em",
        color: t.faint,
        ...style,
      }}
    >
      <Spinner size={12} stroke={t.faint} />
      {label}
    </span>
  );
}
