---
paths:
  - "src/theme/**"
  - "src/ui/**"
---

# Theme and design-system rules

Applies while editing the token layer or the shared UI primitives. Token tables live in the `tmp-companion-frontend` skill's `references/theme-tokens.md`.

- **Never hardcode colors or sizes.** Read the active tokens via `useTheme()` / `useStyles()`. The theme is light-only — **dark mode was removed**.
- **Look B tokens only.** Everything reads the B groups in `tokens.ts`: `color` (`window`, `surface`, `groupHeader`, `text`, `textSecondary`, `action`, `good`/`caution`/`danger` and their fills, …), `type`, `space`, `radius`, `size`, `elevation`. The old aliases are gone; a name you can't find is a B token to look up, not one to re-add. Selection is grey (`rowSelected`), never terracotta.
- **The signal-path tokens are the diagram's alone.** `bgAlt`, `ink`, `mutedInk`, `hairline`, `mono`, `serif`, `fsData`, `space5`, … (every name on the token object that isn't a B token) are read ONLY by the frozen diagram files: `ui/BlockArt.tsx`, `ui/blockart/`, `models/blockArt.ts`, `views/SignalChainView.tsx`, `views/ActiveSignalChainView.tsx`, `views/signalChain/`. Their names and values are frozen with that code. A Vitest source scan in `DSPrimitives.test.tsx` fails when any other file reads one.
- **Inter only, four sizes.** `fontCaption` 12 · `fontBody` 13 · `fontLarge` 15 · `fontTitle` 20, weights 400/500/600 — hierarchy comes from weight, not more sizes. No serif, no mono, no uppercase, no letter-spacing outside the diagram: the one tracking is `trackingTitle` on PageHead's title (a `letterSpacing: "inherit"` reset is fine). The same source scan enforces it. Section labels are `sectionLabel(t)` (sentence case, 12/600 secondary); there are no kickers. Numbers that line up use `fontVariantNumeric: "tabular-nums"`.
- **Spacing is the `sp2..sp32` scale**, named by pixels: 2 · 4 · 6 · 8 · 12 · 16 · 24 · 32. A composite built from those tokens is allowed where a frame calls for it — the Row's 10 px gap (`t.sp8 + 2`), the 20 px Dialog padding (`DIALOG_PAD_X`), the 52–64 px sub-row indents, the 1–2 px optical nudges — so don't "snap" one of those to the nearest step. Never add a new spacing token. Fixed heights come from `size` (`control` 28, `row` 36, `toolbar` 52, `footer` 56, …), corners from `radius` (`radiusSmall` 4 … `radiusGroup` 10, `radiusPill`). There is no `density` token group.
- **A value that must AGREE across surfaces gets a role-named const, not a primitive** — the `DIALOG_PAD_X` pattern, declared beside its component.
- **`Meter.tsx` is deliberately NOT `ProgressBar`.** It is a static track+fill CPU meter; `ProgressBar`'s 0.4s transition would lag the paint. Do not "consolidate" them.
- **`ui/primitives.tsx` is deliberately NOT split.** `Modal` (the confirm shell) renders `Button`, so splitting it reintroduces a circular-import risk. Keep it one file. `ui/Dialog.tsx` stays button-free for the same reason: any dialog that carries buttons lives in `primitives.tsx`, which imports `Dialog`, never the reverse.
- **Every line glyph routes through `<Icon name=…>`**; block art routes through the BlockArt engine. No inline glyph SVG in views.
- **Block art is original SVG — never an `<img>` photo tile.** The copyrighted vendor PNGs are not bundled and must not be reintroduced (Fender IP).
- Every prop-bearing component declares a named `XxxProps` interface.
