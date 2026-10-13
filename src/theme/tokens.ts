// src/theme/tokens.ts — design tokens (LIGHT-ONLY).
//
// Source of truth: the prototype's `APP` token set (handoff_drift_audit/
// reference_prototype/screens.jsx) === the README Part 1.1 table. The app is
// light-only: dark mode was removed, so there is ONE token object. `useTheme()`
// always returns this set; field names are kept stable so call-sites read
// `t.bg`, `t.mutedInk`, `t.serif`, etc. unchanged.
//
// The spacing scale and font-family constants are carried on the same object so
// primitives can read `t.space2`, `t.serif`, etc.

import type { CSSProperties } from "react";

// ---------------------------------------------------------------------------
// Font families. Three Google-Fonts families.
// ---------------------------------------------------------------------------
const FONT_SERIF = "'Source Serif 4', Georgia, serif";
export const FONT_SANS = "'Inter', system-ui, sans-serif";
const FONT_MONO = "'JetBrains Mono', ui-monospace, monospace";

// ---------------------------------------------------------------------------
// Spacing scale — the Tailwind ramp (2px steps to 16, 4px to 32, then 48). The
// DS is FULLY EVEN: all spacing is one of these tokens; odd/off-grid values snap
// to the nearest step. Ordinal names carry the px in a comment — the `spaceN =
// 2N` mnemonic holds ONLY through space8; do not guess it past there (space10 is
// 24, not 20). Cross-surface values that must AGREE get a role-named const
// instead (see DIALOG_PAD_X), not a primitive.
// ---------------------------------------------------------------------------
export const spacing = {
  space1: 2,
  space2: 4,
  space3: 6,
  space4: 8,
  space5: 10,
  space6: 12,
  space7: 14,
  space8: 16,
  space9: 20,
  space10: 24,
  space11: 28,
  space12: 32,
  space13: 48,
};

// ---------------------------------------------------------------------------
// Type scale (README Part 1.2). Theme-invariant px sizes.
// ---------------------------------------------------------------------------
export const typeScale = {
  fsTitle: 24, // page header (serif)
  fsSheet: 21, // overlay-sheet icon icon
  fsSubhead: 19, // hero name (ActiveSignalChainView)
  fsCard: 16, // models-card / medium heading
  fsName: 14.5, // table name cell / row title (serif)
  fsName2: 14, // compact serif name / strong label
  fsBody2: 13.5, // dialog / sheet body
  fsBody: 13, // default sans body / nav
  fsUi: 12, // buttons / controls
  fsControl: 12.5, // inputs, menu items (between Ui and Body)
  fsLabel: 11.5, // secondary labels
  fsData: 11, // mono numerics
  fsMeta: 10.5, // muted compact labels
  fsData2: 10, // small mono data / micro labels
  fsMicro: 9.5, // uppercase mono kickers
  fsMicro2: 9, // compact micro kicker
  fsTag: 8.5, // tiny badge text
};

// ---------------------------------------------------------------------------
// Corner radii (README Part 1.3). Theme-invariant px.
// ---------------------------------------------------------------------------
export const radii = {
  rSm: 3, // chips, inline tags, badges
  rMenuItem: 5, // popover menu-item row
  rBtn: 6, // small buttons / measure pill / icon buttons
  rMd: 7, // buttons, inputs, fields
  rCard: 8, // some cards / popovers (distinct from rLg — do not fold)
  rLg: 9, // cards, rows, popovers, modals (8–9)
  rPopover: 12, // large popovers / pickers
  rDialog: 14, // the DS Dialog card (every modal/overlay)
  rPill: 999, // pill toggles, switches
};

// ---------------------------------------------------------------------------
// Letter-spacing scale (uppercase mono kickers + tracked labels). Unitful
// strings, mirroring the canon literals exactly.
// ---------------------------------------------------------------------------
export const letterSpacing = {
  lsTight: "-0.01em", // page titles (negative tracking)
  lsMeta: "0.02em", // secondary metadata / counts
  lsTag: "0.08em", // badges / tags
  lsLabel: "0.1em", // standard uppercase mono label
  lsWide: "0.12em", // popover headers / setlist labels
  lsKicker: "0.14em", // section kicker (microLabel)
};

// ---------------------------------------------------------------------------
// The single light token set — README Part 1.1 / prototype `APP`. `ThemeTokens`
// is DERIVED from this object (below), so a key is declared exactly once: add it
// here with its doc comment and every `t.<key>` reader types itself.
// ---------------------------------------------------------------------------
export const light = {
  // core palette
  bg: "#ffffff",
  /** README `alt`. */
  bgAlt: "#f6f7f9",
  ink: "#0f1115",
  /** secondary text — between `ink` and `mutedInk`. */
  ink2: "#33373f",
  /** README `muted`. */
  mutedInk: "#6b7280",
  /** quaternary text / icon strokes — fainter than `mutedInk`. */
  faint: "#9aa0a9",
  /** recessed surface (rare inset wells). */
  inset: "#eef0f3",
  /** text/icon color on an ink fill (== bg, so primary buttons invert). */
  onInk: "#ffffff",
  /** white glyph/text/ring on an `accent` (terracotta) fill — checkbox tick,
   *  active facet chip, the Copy edit badge. */
  onAccent: "#ffffff",
  /** sticky section header over a scrolling wall — a near-opaque white wash
   *  (paired with a backdrop blur at the call site). */
  bgFrost: "rgba(255,255,255,0.92)",
  /** README `hair`. */
  hairline: "rgba(15,17,21,0.09)",
  /** README `hairStrong`. */
  hairlineStrong: "rgba(15,17,21,0.18)",
  /** terracotta. */
  accent: "#d97757",
  /** deeper accent for accent-colored text / counts (vs `accent` for fills). */
  accentDeep: "#a7461f",
  accentSoft: "rgba(217,119,87,0.10)",
  /** terracotta SCENE-badge fill — a touch stronger than `accentSoft` so the live
   *  scene's ACTIVE badge reads distinctly (its border is `accentBorder`). */
  accentBadgeSoft: "rgba(217,119,87,0.14)",
  warn: "#a7461f",
  /** destructive/error chip background. */
  warnSoft: "rgba(167,70,31,0.08)",
  /** connected / healthy / ACTIVE / measured / calibrated (green). */
  good: "#3f7d4e",
  /** green chip background (ACTIVE badge / measured pill). README `okSoft`. */
  goodSoft: "rgba(63,125,78,0.10)",
  titlebar: "#ffffff",
  /** selected-row tint (non-active). */
  rowSel: "rgba(15,17,21,0.035)",
  /** menu-item / row hover tint. */
  hover: "rgba(15,17,21,0.05)",
  /** slider track. */
  track: "rgba(15,17,21,0.07)",
  /** slider knob fill (also the `Toggle` switch knob). */
  knob: "#ffffff",
  /** slider knob ring. */
  knobRing: "rgba(15,17,21,0.25)",
  /** `Toggle` switch track in the OFF state (ON fills with `accent`). */
  toggleTrack: "rgba(15,17,21,0.14)",
  /** menu/popover drop-shadow base color. */
  shadow: "rgba(15,17,21,0.28)",

  // chip/pill borders (soft tints of the status colors, mirroring canon rgba)
  /** green badge border (ACTIVE / measured). */
  goodBorder: "rgba(63,125,78,0.4)",
  /** terracotta error/danger badge border. */
  warnBorder: "rgba(167,70,31,0.45)",
  /** lighter terracotta error border (an instrument row in its error phase). */
  warnBorderSoft: "rgba(167,70,31,0.4)",
  /** accent (terracotta) border for accent-tinted pills. */
  accentBorder: "rgba(217,119,87,0.45)",
  /** lighter accent border (the Doctor shared-block caption). */
  accentBorderSoft: "rgba(217,119,87,0.3)",
  /** amber "measuring" pill border + soft background. */
  sevWarnBorder: "rgba(176,125,28,0.5)",
  sevWarnSoft: "rgba(176,125,28,0.08)",
  /** amber filter-chip border + fill (Copy's "only over budget" toggle). */
  sevWarnChipBorder: "rgba(176,125,28,0.4)",
  sevWarnChipSoft: "rgba(176,125,28,0.10)",
  /** recording indicator red (off-palette, audio-feedback semantics) + soft bg —
   *  also the Toast "error" status color (same true-failure red, no need for a
   *  second token). */
  record: "#c0392b",
  recordSoft: "rgba(192,57,43,0.16)",
  /** model badge foregrounds (stereo / convolution). */
  badgeStereo: "#2f6c98",
  badgeConv: "#6a4ba0",

  // signal-chain strip (SignalChainView)
  /** strip wire / bracket / endpoint ink. */
  stripWire: "rgba(15,17,21,0.34)",
  /** skeleton strip ink + an interactive (Copy) tile's idle border. */
  stripGhost: "rgba(15,17,21,0.12)",
  /** strip caption ink (block / endpoint / diamond labels). */
  stripInk: "#5b554a",
  /** a SELECTED Copy tile's caption ink. */
  stripInkSel: "#a5421b",
  /** a CHANGED (added/replaced) Copy tile's border + fill. */
  stripChangedBorder: "rgba(217,119,87,0.55)",
  stripChangedSoft: "rgba(217,119,87,0.06)",

  // font families
  serif: FONT_SERIF,
  sans: FONT_SANS,
  mono: FONT_MONO,

  ...typeScale,
  ...spacing,
  ...radii,
  ...letterSpacing,

  // elevation
  shadowModal: "0 40px 80px -16px rgba(15,17,21,0.28)",
  /** the scrollable dropdown-menu card (`styles.menuCard`) — lighter and closer
   *  than `shadowModal`. */
  shadowMenu: "0 16px 38px -14px rgba(15,17,21,0.28)",
  /** the `Toggle` switch knob. */
  shadowKnob: "0 1px 3px rgba(0,0,0,0.25)",
  /** a small floating badge (the Copy tile's edit badge). */
  shadowBadge: "0 1px 2px rgba(15,17,21,0.2)",
  /** the accent-bordered edit card's 1px terracotta under-edge (SongForm). */
  shadowAccentEdge: "0 1px 0 rgba(217,119,87,0.08)",
  /** block-art drop shadow on a signal-chain strip tile (a CSS `filter`). */
  dropShadowStripArt: "drop-shadow(0 2px 3px rgba(15,17,21,0.18))",
  scrim: "rgba(15,17,21,0.32)",

  // severity colors
  /** amber, distinct from `warn` terracotta. */
  sevWarn: "#b07d1c",
} satisfies Record<string, string | number>;

/** The full set of keys carried by the (single, light) token object — derived
 *  from `light`, so the type never re-lists a key by hand. */
export type ThemeTokens = typeof light;

/** Reusable uppercase mono micro-label (kicker) style. */
export function microLabel(t: ThemeTokens): CSSProperties {
  return {
    fontFamily: t.mono,
    fontSize: t.fsMicro,
    letterSpacing: t.lsKicker,
    textTransform: "uppercase",
    color: t.mutedInk,
  };
}

/** Border-only / transparent text input — the app's inline-edit convention. Spread
 *  `extra` for the per-field font/flex (e.g. `plainInput(t, { flex: 1, fontSize: … })`). */
export function plainInput(
  t: ThemeTokens,
  extra?: CSSProperties,
): CSSProperties {
  return {
    border: "none",
    outline: "none",
    background: "transparent",
    color: t.ink,
    ...extra,
  };
}

/** Native-<button> reset — the ONE shared reset behind every clickable row, chip,
 *  glyph and inline link (the `bare` Button variant and any hand-rolled <button>).
 *  It strips the UA chrome so a real, keyboard- and VoiceOver-reachable button keeps
 *  the exact look of the div/span it replaced. Spread the site's own layout in
 *  `extra` (a block-level row adds `display` + `width: "100%"`, since a button
 *  shrink-wraps where a div would stretch). */
export function plainButton(extra?: CSSProperties): CSSProperties {
  return {
    appearance: "none",
    background: "none",
    border: "none",
    padding: 0,
    margin: 0,
    fontFamily: "inherit",
    fontSize: "inherit",
    fontWeight: "inherit",
    lineHeight: "inherit",
    letterSpacing: "inherit",
    color: "inherit",
    textAlign: "inherit",
    cursor: "pointer",
    boxSizing: "border-box",
    ...extra,
  };
}
