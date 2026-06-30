// src/theme/tokens.ts — design tokens (LIGHT-ONLY).
//
// Source of truth: the prototype's `APP` token set (handoff_drift_audit/
// reference_prototype/screens.jsx) === the README Part 1.1 table. The app is
// light-only: dark mode was removed, so there is ONE token object. `useTheme()`
// always returns this set; field names are kept stable so call-sites read
// `t.bg`, `t.mutedInk`, `t.serif`, etc. unchanged.
//
// Density tokens (locked "regular") and font-family constants are carried on the
// same object so primitives can read `t.row`, `t.serif`, etc.

import type { CSSProperties } from "react";

// ---------------------------------------------------------------------------
// Font families. Three Google-Fonts families.
// ---------------------------------------------------------------------------
export const FONT_SERIF = "'Source Serif 4', Georgia, serif";
export const FONT_SANS = "'Inter', system-ui, sans-serif";
export const FONT_MONO = "'JetBrains Mono', ui-monospace, monospace";

// ---------------------------------------------------------------------------
// Density tokens (locked: "regular"). Mirrored onto the token object.
// ---------------------------------------------------------------------------
export const density = {
  row: 30, // table-row height (px, content-box)
  pad: 16, // base pane padding (px)
  rowPadY: 6, // table-row vertical padding (px)
  paneY: 20, // section paddingY (px)
  sectionGap: 16, // gap between sections in a pane (px)
} as const;

// ---------------------------------------------------------------------------
// Type scale (README Part 1.2). Theme-invariant px sizes.
// ---------------------------------------------------------------------------
export const typeScale = {
  fsDisplay: 28, // hero numbers
  fsTitle: 24, // page header (serif)
  fsSheetLg: 22, // overlay-sheet icon icon (large)
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
} as const;

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
  rWin: 11, // window chrome
  rPopover: 12, // large popovers / pickers
  rDialog: 14, // the DS Dialog card (every modal/overlay)
  rPill: 999, // pill toggles, switches
} as const;

// ---------------------------------------------------------------------------
// Letter-spacing scale (uppercase mono kickers + tracked labels). Unitful
// strings, mirroring the canon literals exactly.
// ---------------------------------------------------------------------------
export const letterSpacing = {
  lsTight: "-0.01em", // page titles (negative tracking)
  lsMeta: "0.02em", // secondary metadata / counts
  lsCaption: "0.05em", // scene-drawer capacity caption
  lsTag: "0.08em", // badges / tags
  lsLabel: "0.1em", // standard uppercase mono label
  lsWide: "0.12em", // popover headers / setlist labels
  lsKicker: "0.14em", // section kicker (microLabel)
} as const;

/** The full set of keys carried by the (single, light) token object. */
export interface ThemeTokens {
  // core palette
  bg: string;
  bgAlt: string; // README `alt`
  ink: string;
  /** secondary text — between `ink` and `mutedInk`. */
  ink2: string;
  mutedInk: string; // README `muted`
  /** quaternary text / icon strokes — fainter than `mutedInk`. */
  faint: string;
  /** recessed surface (rare inset wells). */
  inset: string;
  /** text/icon color on an ink fill (== bg, so primary buttons invert). */
  onInk: string;
  hairline: string; // README `hair`
  hairlineStrong: string; // README `hairStrong`
  accent: string;
  /** deeper accent for accent-colored text / counts (vs `accent` for fills). */
  accentDeep: string;
  accentSoft: string;
  /** terracotta SCENE-badge fill — a touch stronger than `accentSoft` so the live
   *  scene's ACTIVE badge reads distinctly (its border is `accentBorder`). */
  accentBadgeSoft: string;
  warn: string;
  /** destructive/error chip background. */
  warnSoft: string;
  /** connected / healthy / ACTIVE / measured / calibrated (green). */
  good: string;
  /** green chip background (ACTIVE badge / measured pill). README `okSoft`. */
  goodSoft: string;
  titlebar: string;
  /** selected-row tint (non-active). */
  rowSel: string;
  /** menu-item / row hover tint. */
  hover: string;
  /** slider track. */
  track: string;
  /** slider knob fill. */
  knob: string;
  /** slider knob ring. */
  knobRing: string;
  /** menu/popover drop-shadow base color. */
  shadow: string;
  // chip/pill borders (soft tints of the status colors, mirroring canon rgba)
  /** green badge border (ACTIVE / measured). */
  goodBorder: string;
  /** terracotta error/danger badge border. */
  warnBorder: string;
  /** accent (terracotta) border for accent-tinted pills. */
  accentBorder: string;
  /** amber "measuring" pill border + soft background. */
  sevWarnBorder: string;
  sevWarnSoft: string;
  /** destructive-confirm red (distinct from `warn` terracotta) — border + soft bg. */
  dangerBorder: string;
  /** stronger danger border (modal panel edge). */
  dangerBorderStrong: string;
  dangerSoft: string;
  /** recording indicator red (off-palette, audio-feedback semantics) + soft bg. */
  record: string;
  recordSoft: string;
  /** model badge foregrounds (stereo / convolution). */
  badgeStereo: string;
  badgeConv: string;
  // font families
  serif: string;
  sans: string;
  mono: string;
  // type scale (px)
  fsDisplay: number;
  fsTitle: number;
  fsSheetLg: number;
  fsSheet: number;
  fsSubhead: number;
  fsCard: number;
  fsName: number;
  fsName2: number;
  fsBody2: number;
  fsBody: number;
  fsUi: number;
  fsControl: number;
  fsLabel: number;
  fsData: number;
  fsMeta: number;
  fsData2: number;
  fsMicro: number;
  fsMicro2: number;
  fsTag: number;
  // density
  row: number;
  pad: number;
  rowPadY: number;
  paneY: number;
  sectionGap: number;
  // corner radii (px)
  rSm: number;
  rMenuItem: number;
  rBtn: number;
  rMd: number;
  rCard: number;
  rLg: number;
  rWin: number;
  rPopover: number;
  rDialog: number;
  rPill: number;
  // letter-spacing scale
  lsTight: string;
  lsMeta: string;
  lsCaption: string;
  lsTag: string;
  lsLabel: string;
  lsWide: string;
  lsKicker: string;
  // elevation
  shadowWin: string;
  shadowModal: string;
  scrim: string;
  // severity colors
  err: string; // == warn terracotta
  sevWarn: string; // amber, distinct from err
  info: string; // == mutedInk
  ok: string; // == accent (severity "ok"); green status uses `good`/`goodSoft`
}

// ---------------------------------------------------------------------------
// The single light token set — README Part 1.1 / prototype `APP`.
// ---------------------------------------------------------------------------
export const light: ThemeTokens = {
  bg: "#ffffff",
  bgAlt: "#f6f7f9", // alt
  ink: "#0f1115",
  ink2: "#33373f",
  mutedInk: "#6b7280", // muted
  faint: "#9aa0a9",
  inset: "#eef0f3",
  onInk: "#ffffff", // == bg
  hairline: "rgba(15,17,21,0.09)", // hair
  hairlineStrong: "rgba(15,17,21,0.18)", // hairStrong
  accent: "#d97757", // terracotta
  accentDeep: "#a7461f",
  accentSoft: "rgba(217,119,87,0.10)",
  accentBadgeSoft: "rgba(217,119,87,0.14)",
  warn: "#a7461f",
  warnSoft: "rgba(167,70,31,0.08)",
  good: "#3f7d4e", // green
  goodSoft: "rgba(63,125,78,0.10)", // okSoft
  titlebar: "#ffffff",
  rowSel: "rgba(15,17,21,0.035)",
  hover: "rgba(15,17,21,0.05)",
  track: "rgba(15,17,21,0.07)",
  knob: "#ffffff",
  knobRing: "rgba(15,17,21,0.25)",
  shadow: "rgba(15,17,21,0.28)",

  goodBorder: "rgba(63,125,78,0.4)",
  warnBorder: "rgba(167,70,31,0.45)",
  accentBorder: "rgba(217,119,87,0.45)",
  sevWarnBorder: "rgba(176,125,28,0.5)",
  sevWarnSoft: "rgba(176,125,28,0.08)",
  dangerBorder: "rgba(180,40,40,0.4)",
  dangerBorderStrong: "rgba(180,40,40,0.55)",
  dangerSoft: "rgba(180,40,40,0.07)",
  record: "#c0392b",
  recordSoft: "rgba(192,57,43,0.16)",
  badgeStereo: "#2f6c98",
  badgeConv: "#6a4ba0",

  serif: FONT_SERIF,
  sans: FONT_SANS,
  mono: FONT_MONO,

  ...typeScale,

  row: density.row,
  pad: density.pad,
  rowPadY: density.rowPadY,
  paneY: density.paneY,
  sectionGap: density.sectionGap,

  ...radii,
  ...letterSpacing,

  shadowWin:
    "0 0 0 0.5px rgba(15,17,21,0.16), 0 28px 70px -24px rgba(15,17,21,0.32)",
  shadowModal: "0 40px 80px -16px rgba(15,17,21,0.28)",
  scrim: "rgba(15,17,21,0.32)",

  err: "#a7461f", // == warn
  sevWarn: "#b07d1c", // amber
  info: "#6b7280", // == mutedInk
  ok: "#d97757", // == accent (severity)
};

/** Back-compat alias — some imports use `lightTokens`. */
export const lightTokens = light;

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
