// src/__tests__/testKit.tsx — the render half of the test kit.
//
// `renderWithTheme(ui)` renders inside the app's `ThemeProvider` (which `useTheme()` /
// `useStyles()` need) as RTL's `wrapper`, so a `rerender(ui)` keeps the provider too.
// `toRgb(light.x)` is how jsdom reports a hex token set as an inline style, so a colour
// assertion names the token, not its current value.
// The IPC half — `mockCommands`, `callsOf`, the `make*` factories, `mockLiveEvents()`,
// `forceTauri()` — lives in ./commandMocks.

import type { ReactNode } from "react";
import { render, type RenderOptions } from "@testing-library/react";

import { ThemeProvider } from "../theme/ThemeProvider";

/** `render(ui)` inside the theme provider. */
export function renderWithTheme(
  ui: ReactNode,
  options: Omit<RenderOptions, "wrapper"> = {},
) {
  return render(ui, { ...options, wrapper: ThemeProvider });
}

/** A `#rrggbb` token as jsdom serializes it in `element.style` (`rgb(r, g, b)`). */
export function toRgb(hex: string): string {
  const n = parseInt(hex.slice(1), 16);
  const r = (n >> 16) & 255;
  const g = (n >> 8) & 255;
  const b = n & 255;
  return `rgb(${String(r)}, ${String(g)}, ${String(b)})`;
}
