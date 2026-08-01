---
paths:
  - "src/theme/**"
  - "src/ui/**"
---

# Theme and design-system rules

Applies while editing the token layer or the shared UI primitives. Token tables live in the `tmp-companion-frontend` skill's `references/theme-tokens.md`.

- **Never hardcode colors or sizes.** Read the active tokens via `useTheme()` / `useStyles()`. The theme is light-only — **dark mode was removed**.
- **Spacing is the `space1..13` ramp** (2px steps to 16, 4px to 32, then 48). The DS is **FULLY EVEN**: no odd or fractional spacing anywhere; new values snap to the nearest step. The `spaceN = 2N` mnemonic holds **only through `space8`** — `space10` is 24, not 20. There is no `density` token group; the old one was dead and removed.
- **A value that must AGREE across surfaces gets a role-named const, not a primitive** — the `DIALOG_PAD_X` pattern, declared beside its component.
- **`Meter.tsx` is deliberately NOT `ProgressBar`.** It is a static track+fill CPU meter; `ProgressBar`'s 0.4s transition would lag the paint. Do not "consolidate" them.
- **`ui/primitives.tsx` is deliberately NOT split.** `Modal` renders `Button`, so splitting it reintroduces a circular-import risk. Keep it one file.
- **Every line glyph routes through `<Icon name=…>`**; block art routes through the BlockArt engine. No inline glyph SVG in views.
- **Block art is original SVG — never an `<img>` photo tile.** The copyrighted vendor PNGs are not bundled and must not be reintroduced (Fender IP).
- Every prop-bearing component declares a named `XxxProps` interface.
