// src/theme/ThemeProvider.tsx — the theme provider component (LIGHT-ONLY).
//
// Split from ./ThemeContext so that module exports only hooks + the context
// (no component) and this one exports only the component — each satisfies React
// Fast Refresh's "only export components" boundary without disabling the rule.

import { useEffect, type ReactNode } from "react";
import { light } from "./tokens";
import { ThemeContext, themeValue } from "./ThemeContext";

/**
 * Wraps the app and exposes the token object via context. Also pins
 * `document.body` background/color to the window grey and text colour so the
 * chrome behind React matches (index.html sets the same before React loads).
 */
export function ThemeProvider({ children }: { children: ReactNode }) {
  useEffect(() => {
    document.body.style.background = light.window;
    document.body.style.color = light.text;
  }, []);

  return (
    <ThemeContext.Provider value={themeValue}>{children}</ThemeContext.Provider>
  );
}
