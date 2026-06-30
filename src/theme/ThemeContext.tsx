// src/theme/ThemeContext.tsx — theme provider + hook (LIGHT-ONLY).
//
// The app is light-only (dark mode was removed). There is one token object;
// `useTheme()` returns `{ t }`. The provider is kept (rather than importing the
// tokens directly everywhere) so the token source stays a single seam.

import { createContext, useContext } from "react";
import { light, type ThemeTokens } from "./tokens";
import { buildStyles, type Styles } from "./styles";

export interface ThemeContextValue {
  /** The active token object. */
  t: ThemeTokens;
}

// The context + its single static value live here (with the hooks); the
// <ThemeProvider> component is in ./ThemeProvider so this module exports no
// component — keeping it off React Fast Refresh's component-boundary rule while
// the widely-imported hooks stay at this path.
export const ThemeContext = createContext<ThemeContextValue | null>(null);
export const themeValue: ThemeContextValue = { t: light };

// The theme is a single static token object, so the composed-style registry is
// constant too — build it once at module load rather than per render.
const styles: Styles = buildStyles(light);

/**
 * Returns `{ t }` — the active token object. Throws if used outside a
 * `<ThemeProvider>`.
 */
export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useTheme must be used within <ThemeProvider>");
  return ctx;
}

/**
 * Returns the composed-style registry (search-box / popover / measure-pill /
 * icon-button styles). Pairs with `useTheme()`:
 *   `const { t } = useTheme(); const s = useStyles();`
 * It's a stable module-level constant (the theme is static), so it never causes
 * a re-render and is safe to read outside a provider too.
 */
export function useStyles(): Styles {
  return styles;
}
