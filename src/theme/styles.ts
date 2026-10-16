// src/theme/styles.ts — composed style registry, consumed via `useStyles()`.
//
// Tokens (`tokens.ts`) hold scalar design values (colors, font sizes, radii,
// letter-spacing). This module holds the *composed* style objects that more
// than one screen reuses — the search-box frame, the popover card, the
// measure-control pill, the square icon button, etc. Components read them with
// `const s = useStyles()` and either use them directly (`style={s.searchBox}`)
// or spread + override (`style={{ ...s.searchBox, flex: 1 }}`).
//
// Every value here is lifted byte-for-byte from the canonical inline source it
// replaces (the app is verified pixel-exact against canon), expressed through
// tokens so the literal lives in exactly one place. Factory functions cover the
// parameterized styles (e.g. `iconBtnBox({ box, radius })`); static objects cover the rest.

import type { CSSProperties } from "react";
import { microLabel, type ThemeTokens } from "./tokens";

export function buildStyles(t: ThemeTokens) {
  return {
    /** Uppercase mono kicker (section label) in the canon per-section accent
     *  (accentDeep / faint / warn / good). */
    kicker(color: string): CSSProperties {
      return { ...microLabel(t), color };
    },

    /** kicker variant with the tighter 0.12em tracking (rail/setup section labels). */
    kickerWide(color: string): CSSProperties {
      return { ...microLabel(t), letterSpacing: t.lsWide, color };
    },

    /** Search / filter field frame (look B): 28 high, `controlBorder` edge, the
     *  icon + input live inside. Add `flex: 1` at the call site where it should grow. */
    searchBox: {
      display: "flex",
      alignItems: "center",
      gap: t.sp6,
      height: t.control,
      boxSizing: "border-box",
      border: `1px solid ${t.controlBorder}`,
      borderRadius: t.radiusControl,
      padding: `0 ${String(t.sp8)}px`,
      background: t.surface,
    } as CSSProperties,

    /** Floating popover card (look B): white, `radiusPanel`, `shadowMenu` (which
     *  carries its own 0.5 px ring, so no border). The caller owns the inner layout. */
    popoverCard: {
      background: t.surface,
      borderRadius: t.radiusPanel,
      boxShadow: t.shadowMenu,
      overflow: "hidden",
    } as CSSProperties,

    /** Scrollable dropdown-menu card (look B): the `Menu` and `Select` list. White,
     *  `radiusPanel`, `shadowMenu`, 4 px inset; position + width per call site. */
    menuCard: {
      background: t.surface,
      borderRadius: t.radiusPanel,
      boxShadow: t.shadowMenu,
      padding: t.sp4,
      boxSizing: "border-box",
      maxHeight: 280,
      overflowY: "auto",
      overflowX: "hidden",
    } as CSSProperties,

    /** Square hairline icon button. `box` = fixed side length; `radius` differs
     *  by surface (ActiveSignalChainView = `t.rBtn`, SongsView = `t.rMd`);
     *  `danger` flips to the terracotta error border + colour. */
    iconBtnBox({
      box,
      radius,
      danger,
    }: {
      box: number;
      radius: number;
      danger?: boolean;
    }): CSSProperties {
      return {
        width: box,
        height: box,
        boxSizing: "border-box",
        borderRadius: radius,
        border: `0.5px solid ${danger ? t.warnBorder : t.hairlineStrong}`,
        color: danger ? t.warn : t.ink2,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        cursor: "pointer",
        flexShrink: 0,
        background: "transparent",
      };
    },
  };
}

/** The composed-style registry returned by `useStyles()`. */
export type Styles = ReturnType<typeof buildStyles>;
