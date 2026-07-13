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

    /** Search / filter input frame (the icon + input live inside). Add
     *  `flex: 1` at the call site where it should grow. */
    searchBox: {
      display: "flex",
      alignItems: "center",
      gap: t.space4,
      border: `0.5px solid ${t.hairlineStrong}`,
      borderRadius: t.rMd,
      padding: `${String(t.space3)}px ${String(t.space5)}px`,
      background: "transparent",
    } as CSSProperties,

    /** Floating popover / dropdown card frame. Spread + override `borderRadius`
     *  (→ `t.rPopover`) or `width` per call site. */
    popoverCard: {
      background: t.bg,
      border: `0.5px solid ${t.hairlineStrong}`,
      borderRadius: t.rLg,
      boxShadow: t.shadowModal,
      overflow: "hidden",
    } as CSSProperties,

    /** Scrollable dropdown-menu card (the leveling wizard's `Pick`). Lighter,
     *  closer shadow than `popoverCard`; position + `minWidth` per call site. */
    menuCard: {
      background: t.bg,
      border: `0.5px solid ${t.hairlineStrong}`,
      borderRadius: t.rCard,
      boxShadow: "0 16px 38px -14px rgba(15,17,21,0.28)",
      padding: t.space2,
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
