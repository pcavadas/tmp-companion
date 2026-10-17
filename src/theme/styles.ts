// src/theme/styles.ts — composed style registry, consumed via `useStyles()`.
//
// Tokens (`tokens.ts`) hold scalar design values (colours, sizes, radii). This
// module holds the *composed* style objects that more than one component reuses:
// the search-box frame, the popover card and the dropdown-menu card. Components
// read them with `const s = useStyles()` and either use them directly
// (`style={s.searchBox}`) or spread + override (`style={{ ...s.searchBox, flex: 1 }}`).
// Every value is expressed through the look B tokens, so the literal lives in
// exactly one place.

import type { CSSProperties } from "react";
import type { ThemeTokens } from "./tokens";

export function buildStyles(t: ThemeTokens) {
  return {
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

    /** Floating popover card (look B): white, `radiusGroup`, `shadowMenu` (which
     *  carries its own 0.5 px ring, so no border). The caller owns the inner layout. */
    popoverCard: {
      background: t.surface,
      borderRadius: t.radiusGroup,
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
  };
}

/** The composed-style registry returned by `useStyles()`. */
export type Styles = ReturnType<typeof buildStyles>;
