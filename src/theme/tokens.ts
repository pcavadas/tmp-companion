// src/theme/tokens.ts — design tokens, look B ("Mac utility"). LIGHT-ONLY.
//
// Two parts, one object:
//
//   1. The B tokens. Everything outside the signal-path diagram uses these.
//   2. SIGNAL-PATH TOKENS. Read ONLY by the keep-as-is diagram files
//      (src/ui/BlockArt.tsx, src/ui/blockart/, src/models/blockArt.ts,
//      src/views/SignalChainView.tsx, src/views/ActiveSignalChainView.tsx,
//      src/views/signalChain/). Names and values are frozen, except `bgAlt`, which
//      is white so the diagram sits on white. Nothing else reads them: a Vitest
//      source scan (DSPrimitives.test.tsx) fails the build if another file does.
//
// `bgAlt` is the diagram's own background, nothing else's. Outside the diagram a
// grey fill reads `window` (window, toolbar, footer, sidebars) or `groupHeader`
// (header rows).
//
// Fonts: B uses Inter only. Source Serif 4 and JetBrains Mono stay bundled
// because the signal-path diagram uses them; nothing else should.

import type { CSSProperties } from "react";

const FONT_SERIF = "'Source Serif 4', Georgia, serif";
/** Unchanged: the signal-path code imports it. */
export const FONT_SANS = "'Inter', system-ui, sans-serif";
const FONT_MONO = "'JetBrains Mono', ui-monospace, monospace";

// ---------------------------------------------------------------------------
// 1. B tokens
// ---------------------------------------------------------------------------

/** Colour. Text meets 4.5:1 on window, surface and groupHeader, and on its own
 *  tone's fill; control edges and fills meet 3:1. */
export const color = {
  /** Window background, toolbar, footer, sidebars. */
  window: "#f5f5f7",
  /** List groups, dialogs, menus, fields, the signal path. */
  surface: "#ffffff",
  /** Header row of a list group (a preset in Set up, column headers). */
  groupHeader: "#fafafa",
  /** Line between rows inside a group. */
  separator: "rgba(0,0,0,0.06)",
  /** Edge of a group, card, dialog, toolbar and footer. */
  border: "rgba(0,0,0,0.10)",
  /** Edge of buttons, fields, dropdowns and unticked checkboxes, and a switch
   *  that is off (3.6:1 on white, 3.3:1 on window). */
  controlBorder: "#86868b",

  /** Primary text. */
  text: "#1d1d1f",
  /** Secondary text: metadata, slot numbers, column headers, placeholders. */
  textSecondary: "#6e6e73",
  /** Disabled text and icons (exempt from contrast rules). */
  textDisabled: "#a1a1a6",

  /** Brand terracotta for marks that aren't controls: progress fill, live meter,
   *  the target tick, the app icon. */
  accent: "#d97757",
  /** Terracotta tint behind an accent Badge ("Changed") and the halo around a
   *  working Dot, and nothing else; it isn't a selection colour. */
  accentFill: "rgba(217,119,87,0.14)",
  /** Fill of everything you click that's "on": primary button, ticked checkbox,
   *  switch on, focus ring (white on it is 4.6:1). */
  action: "#bd5632",
  /** Primary button pressed. */
  actionPressed: "#a7461f",
  /** Text and glyphs on `action`, `danger` fills. */
  onAction: "#ffffff",
  /** Text links ("Record yours in Settings"). */
  link: "#a7461f",
  /** Focus ring. */
  focus: "#bd5632",

  /** Selected row, open preset group, active sidebar item. Grey, never terracotta. */
  rowSelected: "rgba(0,0,0,0.04)",
  /** Row and menu-item hover. */
  rowHover: "rgba(0,0,0,0.03)",

  /** Connected, at target, saved. 4.5:1 or more on its own fill too. */
  good: "#246b3a",
  goodFill: "rgba(36,107,58,0.10)",
  /** Needs a look: loud peaks, a choice to make, firmware not tested. */
  caution: "#8a5a00",
  cautionFill: "rgba(176,125,28,0.12)",
  /** Edge of a dropdown that needs a choice. */
  cautionLine: "#b07d1c",
  /** Something failed; destructive confirmation. */
  danger: "#a93226",
  dangerFill: "rgba(169,50,38,0.08)",

  /** Tracks of meters, progress bars and sliders. */
  track: "#e8e8ed",
  /** CPU bar fill. */
  meter: "#a1a1a6",
  /** Switch track when off (3.3:1 on window). */
  switchOff: "#86868b",
  /** Track behind a segmented control (the tab switch). */
  segmentTrack: "rgba(0,0,0,0.06)",
  /** Behind dialogs. */
  scrim: "rgba(0,0,0,0.25)",
};

/** Type: Inter only, four sizes. Hierarchy comes from weight, not more sizes. */
export const type = {
  fontSans: FONT_SANS,
  /** Metadata, column headers, badges, captions. */
  fontCaption: 12,
  /** Everything else: rows, buttons, body, menus. */
  fontBody: 13,
  /** Dialog titles, empty-state titles, a Doctor finding's name. */
  fontLarge: 15,
  /** Page and step titles ("Level 6 sounds"). */
  fontTitle: 20,
  lineCaption: 16,
  lineBody: 18,
  lineLarge: 20,
  lineTitle: 26,
  weightRegular: 400,
  weightMedium: 500,
  weightSemibold: 600,
  /** Page titles only. */
  trackingTitle: "-0.01em",
};

/** Spacing, named by pixels. */
export const space = {
  sp2: 2,
  sp4: 4,
  sp6: 6,
  sp8: 8,
  sp12: 12,
  sp16: 16,
  sp24: 24,
  sp32: 32,
};

/** Corner radii. */
export const radius = {
  /** Checkboxes, badges, menu items. */
  radiusSmall: 4,
  /** Buttons, fields, dropdowns, tab buttons, sidebar items. */
  radiusControl: 6,
  /** Notices, menus, toasts, the segmented track (control + its 2 px padding). */
  radiusPanel: 8,
  /** List groups and dialogs. */
  radiusGroup: 10,
  /** Switches. */
  radiusPill: 999,
};

/** Fixed heights, so rows and controls line up across tabs. */
export const size = {
  controlSmall: 22,
  control: 28,
  controlLarge: 32,
  row: 36,
  /** Rows that hold dropdowns (Set up). */
  rowControls: 34,
  rowCompact: 30,
  toolbar: 52,
  footer: 56,
};

export const elevation = {
  /** The selected tab in a segmented control. */
  shadowSegment: "0 1px 2px rgba(0,0,0,0.15)",
  /** Menus, dropdown lists, toasts. */
  shadowMenu: "0 8px 24px rgba(0,0,0,0.14), 0 0 0 0.5px rgba(0,0,0,0.12)",
  /** Dialogs and sheets. */
  shadowDialog: "0 24px 60px rgba(0,0,0,0.22), 0 0 0 0.5px rgba(0,0,0,0.12)",
};

// ---------------------------------------------------------------------------
// 2. SIGNAL-PATH TOKENS (frozen; read by the keep-as-is files)
// ---------------------------------------------------------------------------
const signalPath = {
  /** Was #f6f7f9. White in look B: the diagram sits on white. */
  bgAlt: "#ffffff",
  bg: "#ffffff",
  ink: "#0f1115",
  ink2: "#33373f",
  mutedInk: "#6b7280",
  faint: "#9aa0a9",
  hairline: "rgba(15,17,21,0.09)",
  hairlineStrong: "rgba(15,17,21,0.18)",
  accent: "#d97757",
  accentDeep: "#a7461f",
  accentSoft: "rgba(217,119,87,0.10)",
  accentBorder: "rgba(217,119,87,0.45)",
  onAccent: "#ffffff",
  /** The over-budget CPU figure in the signal-path header. */
  warn: "#a7461f",
  sevWarn: "#b07d1c",
  sevWarnBorder: "rgba(176,125,28,0.5)",
  shadow: "rgba(15,17,21,0.28)",
  shadowBadge: "0 1px 2px rgba(15,17,21,0.2)",
  dropShadowStripArt: "drop-shadow(0 2px 3px rgba(15,17,21,0.18))",
  stripWire: "rgba(15,17,21,0.34)",
  stripGhost: "rgba(15,17,21,0.12)",
  stripInk: "#5b554a",
  stripInkSel: "#a5421b",
  stripChangedBorder: "rgba(217,119,87,0.55)",
  stripChangedSoft: "rgba(217,119,87,0.06)",
  serif: FONT_SERIF,
  sans: FONT_SANS,
  mono: FONT_MONO,
  fsSubhead: 19,
  fsName2: 14,
  fsUi: 12,
  fsLabel: 11.5,
  fsData: 11,
  fsData2: 10,
  fsTag: 8.5,
  lsTag: "0.08em",
  rBtn: 6,
  rCard: 8,
  rLg: 9,
  space1: 2,
  space2: 4,
  space3: 6,
  space4: 8,
  space5: 10,
  space6: 12,
  space7: 14,
  space8: 16,
};

// ---------------------------------------------------------------------------
// The single token object. `useTheme()` returns it as `t`.
// ---------------------------------------------------------------------------
export const light = {
  ...signalPath,
  ...color,
  ...type,
  ...space,
  ...radius,
  ...size,
  ...elevation,
} satisfies Record<string, string | number>;

export type ThemeTokens = typeof light;

/** Section label: sentence case, 12 px semibold, secondary. */
export function sectionLabel(t: ThemeTokens): CSSProperties {
  return {
    fontFamily: t.fontSans,
    fontSize: t.fontCaption,
    lineHeight: `${String(t.lineCaption)}px`,
    fontWeight: t.weightSemibold,
    color: t.textSecondary,
  };
}

/** Border-only / transparent text input — the app's inline-edit convention. */
export function plainInput(
  t: ThemeTokens,
  extra?: CSSProperties,
): CSSProperties {
  return {
    border: "none",
    outline: "none",
    background: "transparent",
    color: t.text,
    ...extra,
  };
}

/** Native-<button> reset behind every clickable row, chip, glyph and inline
 *  link. Unchanged: the signal-path code imports it. */
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
