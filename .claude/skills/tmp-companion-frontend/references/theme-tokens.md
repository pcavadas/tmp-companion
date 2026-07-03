# Theme tokens & composed styles

The companion is **light-only** — `src/theme/tokens.ts` exports ONE token object (`light`, type `ThemeTokens`). `useTheme()` returns `{ t }` where `t` is that object; field names are stable so call-sites read `t.bg`, `t.mutedInk`, `t.serif`, `t.rMd`, etc. directly. This file mirrors `tokens.ts` + `styles.ts` so you don't have to open them for a quick lookup — but `tokens.ts` is the source of truth; if a value here ever disagrees, trust the file.

Styling is **inline `style={{}}` objects** read off `t`. No CSS modules, no styled-components, no Tailwind, no className system — that's deliberate and verified pixel-exact against the design canon.

## Colors (palette)

| Token                           | Value                                                | Use                                                                   |
| ------------------------------- | ---------------------------------------------------- | --------------------------------------------------------------------- |
| `bg`                            | `#ffffff`                                            | base surface                                                          |
| `bgAlt`                         | `#f6f7f9`                                            | alt surface (README `alt`)                                            |
| `inset`                         | `#eef0f3`                                            | recessed inset wells (rare)                                           |
| `ink`                           | `#0f1115`                                            | primary text                                                          |
| `ink2`                          | `#33373f`                                            | secondary text                                                        |
| `mutedInk`                      | `#6b7280`                                            | muted text (README `muted`)                                           |
| `faint`                         | `#9aa0a9`                                            | quaternary text / icon strokes                                        |
| `onInk`                         | `#ffffff`                                            | text/icon on an ink fill (== bg, so primary buttons invert)           |
| `hairline`                      | `rgba(15,17,21,0.09)`                                | hairline borders (README `hair`)                                      |
| `hairlineStrong`                | `rgba(15,17,21,0.18)`                                | stronger hairline (README `hairStrong`)                               |
| `accent`                        | `#d97757`                                            | terracotta — fills                                                    |
| `accentDeep`                    | `#a7461f`                                            | accent-colored text / counts / kickers                                |
| `accentSoft`                    | `rgba(217,119,87,0.10)`                              | accent pill background                                                |
| `accentBadgeSoft`               | `rgba(217,119,87,0.14)`                              | active-scene badge fill                                               |
| `accentBorder`                  | `rgba(217,119,87,0.45)`                              | accent pill border                                                    |
| `warn` / `err`                  | `#a7461f`                                            | warning/error text (terracotta)                                       |
| `warnSoft`                      | `rgba(167,70,31,0.08)`                               | error chip background                                                 |
| `warnBorder`                    | `rgba(167,70,31,0.45)`                               | error badge border                                                    |
| `sevWarn`                       | `#b07d1c`                                            | amber "measuring" severity (distinct from `err`)                      |
| `sevWarnSoft` / `sevWarnBorder` | amber soft / border                                  | measuring pill                                                        |
| `good`                          | `#3f7d4e`                                            | connected / healthy / ACTIVE / measured (green)                       |
| `goodSoft`                      | `rgba(63,125,78,0.10)`                               | green chip background (README `okSoft`)                               |
| `goodBorder`                    | `rgba(63,125,78,0.4)`                                | green badge border                                                    |
| `danger*`                       | `dangerBorder` / `dangerBorderStrong` / `dangerSoft` | destructive-confirm red (distinct from `warn`)                        |
| `record`                        | `#c0392b`                                            | recording indicator red (off-palette, audio semantics) + `recordSoft` |
| `rowSel`                        | `rgba(15,17,21,0.035)`                               | selected-row tint (non-active)                                        |
| `hover`                         | `rgba(15,17,21,0.05)`                                | row/menu-item hover tint                                              |
| `track` / `knob` / `knobRing`   | slider track / knob fill / knob ring                 | sliders                                                               |
| `shadow`                        | `rgba(15,17,21,0.28)`                                | popover drop-shadow base                                              |
| `badgeStereo` / `badgeConv`     | `#2f6c98` / `#6a4ba0`                                | model badge foregrounds (stereo / convolution)                        |
| `info`                          | `#6b7280`                                            | == `mutedInk` (severity)                                              |
| `ok`                            | `#d97757`                                            | == `accent` (severity "ok"; green status uses `good`)                 |

Severity aliases exist so status code can read `t.err` / `t.sevWarn` / `t.info` / `t.ok` semantically — but for green status badges use `good`/`goodSoft`/`goodBorder`, not `ok`.

## Fonts

| Token   | Stack                                                           |
| ------- | --------------------------------------------------------------- |
| `serif` | `'Source Serif 4', Georgia, serif` — names, titles              |
| `sans`  | `'Inter', system-ui, sans-serif` — body, controls               |
| `mono`  | `'JetBrains Mono', ui-monospace, monospace` — numerics, kickers |

## Type scale (px)

`fsDisplay` 28 · `fsTitle` 24 · `fsSheetLg` 22 · `fsSheet` 21 · `fsSubhead` 19 · `fsCard` 16 · `fsName` 14.5 · `fsName2` 14 · `fsBody2` 13.5 · `fsBody` 13 · `fsControl` 12.5 · `fsUi` 12 · `fsLabel` 11.5 · `fsData` 11 · `fsMeta` 10.5 · `fsData2` 10 · `fsMicro` 9.5 · `fsMicro2` 9 · `fsTag` 8.5

(serif for `fsName*`/`fsTitle`/`fsSubhead`; mono for `fsData*`/`fsMicro*`; sans for the rest.)

## Radii (px)

`rSm` 3 (chips/tags) · `rMenuItem` 5 (popover row) · `rBtn` 6 (small/icon buttons) · `rMd` 7 (buttons/inputs/fields) · `rCard` 8 (cards/popovers — distinct from `rLg`, do not fold) · `rLg` 9 (cards/rows/modals) · `rWin` 11 (window chrome) · `rPopover` 12 (large pickers) · `rDialog` 14 (the DS `Dialog` card) · `rPill` 999 (pills/switches)

## Density (px)

`row` 30 (table-row height) · `pad` 16 (pane padding) · `rowPadY` 6 · `paneY` 20 (section paddingY) · `sectionGap` 16

## Letter-spacing

`lsTight` `-0.01em` (page titles) · `lsMeta` `0.02em` (counts) · `lsCaption` `0.05em` · `lsTag` `0.08em` · `lsLabel` `0.1em` · `lsWide` `0.12em` (popover headers) · `lsKicker` `0.14em` (section kicker)

## Elevation

`shadowWin` (window), `shadowModal` (modals/popovers), `scrim` `rgba(15,17,21,0.32)` (modal backdrop).

## Composed styles — `useStyles()` → `s`

`src/theme/styles.ts` `buildStyles(t)` returns the registry of _composed_ style objects/factories that more than one screen reuses, so a literal lives in exactly one place. Read directly (`style={s.searchBox}`) or spread + override (`style={{ ...s.searchBox, flex: 1 }}`).

| Entry                                  | Kind    | What it is                                                                                                                       |
| -------------------------------------- | ------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `s.kicker(color)`                      | factory | uppercase mono section micro-label in a per-section accent (`accentDeep` / `faint` / `warn` / `good`). Built on `microLabel(t)`. |
| `s.searchBox`                          | object  | search/filter input frame (icon + input inside); add `flex: 1` per call site                                                     |
| `s.popoverCard`                        | object  | floating popover/dropdown card frame (modal shadow)                                                                              |
| `s.menuCard`                           | object  | scrollable dropdown-menu card (lighter, closer shadow than `popoverCard`)                                                        |
| `s.iconBtnBox({box, radius, danger?})` | factory | square hairline icon button; `danger` flips to the terracotta error border + color                                               |
| `s.measurePill(phase)`                 | factory | Presets measure-control pill across `idle`/`measuring`/`done`/`error` phases                                                     |

When you need a shared style that doesn't exist yet, prefer adding a factory/object here (built from tokens) over duplicating an inline literal across files — that's the whole point of this registry. Also available from `tokens.ts`: `microLabel(t)` (the bare kicker style) and `plainInput(t, extra?)` (the border-less transparent inline-edit input style — lifted here from `songs/songUtil.ts`). For a search/filter box prefer the `SearchInput` primitive (icon + transparent input + optional clear, built on `s.searchBox`) over re-inlining the `s.searchBox` frame.
