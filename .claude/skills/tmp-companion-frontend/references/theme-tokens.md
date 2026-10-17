# Theme tokens & composed styles

The companion is **light-only** — `src/theme/tokens.ts` exports ONE token object (`light`); its type `ThemeTokens` is DERIVED (`typeof light`), so a new token is one keyed entry with its doc comment — never a second hand-written interface line. `useTheme()` returns `{ t }` where `t` is that object. This file mirrors `tokens.ts` + `styles.ts` for a quick lookup — `tokens.ts` is the source of truth; if a value here ever disagrees, trust the file.

The object is two parts, spread together:

1. **The look B tokens** — six exported groups (`color`, `type`, `space`, `radius`, `size`, `elevation`). Everything outside the signal-path diagram reads ONLY these.
2. **The frozen signal-path tokens** — read ONLY by the keep-as-is diagram files (below). Do not use them anywhere else.

There are no old aliases any more (`inset`, `rowSel`, `fsName`, `rMd`, `space10`, `lsKicker`, `microLabel()`, … are gone). A name you can't find is a B token to look up here, not one to re-add.

Styling is **inline `style={{}}` objects** read off `t`. No CSS modules, no styled-components, no Tailwind, no className system — that's deliberate.

## Colour (`color`)

| Token           | Value                   | Use                                                                                             |
| --------------- | ----------------------- | ----------------------------------------------------------------------------------------------- |
| `window`        | `#f5f5f7`               | Window background, toolbar, footer, sidebars                                                    |
| `surface`       | `#ffffff`               | List groups, dialogs, menus, fields, the signal path                                            |
| `groupHeader`   | `#fafafa`               | Header row of a list group (a preset in Set up, column headers)                                 |
| `separator`     | `rgba(0,0,0,0.06)`      | Line between rows inside a group                                                                |
| `border`        | `rgba(0,0,0,0.10)`      | Edge of a group, card, dialog, toolbar and footer                                               |
| `controlBorder` | `#86868b`               | Edge of buttons, fields, dropdowns, unticked checkboxes, a switch that is off                   |
| `text`          | `#1d1d1f`               | Primary text                                                                                    |
| `textSecondary` | `#6e6e73`               | Secondary text: metadata, slot numbers, column headers, placeholders                            |
| `textDisabled`  | `#a1a1a6`               | Disabled text and icons                                                                         |
| `accent`        | `#d97757`               | Brand terracotta for marks that aren't controls: progress fill, the live meter, the target tick |
| `accentFill`    | `rgba(217,119,87,0.14)` | Tint behind an `accent` Badge ("Changed") and the halo around a working Dot                     |
| `action`        | `#bd5632`               | Fill of everything you click that's "on": primary button, ticked checkbox, switch on            |
| `actionPressed` | `#a7461f`               | Primary button pressed                                                                          |
| `onAction`      | `#ffffff`               | Text and glyphs on `action` and `danger` fills                                                  |
| `link`          | `#a7461f`               | Text links ("Record yours in Settings")                                                         |
| `focus`         | `#bd5632`               | Focus ring (index.html: `:focus-visible`; `SearchInput` rings its frame via `.tmp-search`)      |
| `rowSelected`   | `rgba(0,0,0,0.04)`      | Selected row, open preset, active sidebar item — **grey, never terracotta**                     |
| `rowHover`      | `rgba(0,0,0,0.03)`      | Row and menu-item hover                                                                         |
| `good`          | `#246b3a`               | Connected, at target, saved                                                                     |
| `goodFill`      | `rgba(36,107,58,0.10)`  | Background of a good Badge or Notice                                                            |
| `caution`       | `#8a5a00`               | Needs a look: loud peaks, a choice to make, untested firmware                                   |
| `cautionFill`   | `rgba(176,125,28,0.12)` | Background of a caution Badge or Notice; the ring round a dropdown that needs a choice          |
| `cautionLine`   | `#b07d1c`               | Edge of a dropdown that needs a choice; the caution Dot                                         |
| `danger`        | `#a93226`               | Something failed; a destructive confirm                                                         |
| `dangerFill`    | `rgba(169,50,38,0.08)`  | Background of a danger Badge or Notice                                                          |
| `track`         | `#e8e8ed`               | Tracks of meters, progress bars and sliders                                                     |
| `meter`         | `#a1a1a6`               | CPU bar fill; the neutral Dot                                                                   |
| `switchOff`     | `#86868b`               | Switch track when off                                                                           |
| `segmentTrack`  | `rgba(0,0,0,0.06)`      | Track behind a segmented control; the neutral Badge fill                                        |
| `scrim`         | `rgba(0,0,0,0.25)`      | Behind dialogs                                                                                  |

### Severity mapping (a handoff's words → tokens)

| A handoff says                         | Text / icon                        | Fill          | Edge          | Components (their `tone` / `variant` / `status`)                              |
| -------------------------------------- | ---------------------------------- | ------------- | ------------- | ----------------------------------------------------------------------------- |
| ok, success, green, at target, saved   | `good`                             | `goodFill`    | —             | Badge `good`, Dot `good`, Toast `success`                                     |
| warning, amber, needs a look, a choice | `caution`                          | `cautionFill` | `cautionLine` | Badge `caution`, Notice `caution`, Select `attention`                         |
| error, failed, destructive             | `danger`                           | `dangerFill`  | —             | Badge `danger`, Notice `danger`, Button `danger`, PageState `danger`          |
| info, neutral                          | `textSecondary` icon, `text` words | `groupHeader` | `border`      | Notice `info`, Badge `neutral` (`segmentTrack` fill), Dot `neutral` (`meter`) |
| working, in progress                   | `accent`                           | `accentFill`  | —             | Dot `working`, Spinner, ProgressBar                                           |

**Severity-token trap:** a handoff's green is `good`, never terracotta; its error is `danger`, never `link`/`action`. There is no `t.ok` / `t.err` / `t.info` / `t.warn` alias — never add one. (`warn` survives only as a frozen signal-path token, the diagram header's over-budget CPU figure.)

## Type (`type`) — Inter only

`fontSans` `'Inter', system-ui, sans-serif` is the only face. Four sizes, three weights; hierarchy comes from weight, not more sizes.

| Size          | px  | Line             | Use                                                        |
| ------------- | --- | ---------------- | ---------------------------------------------------------- |
| `fontCaption` | 12  | `lineCaption` 16 | Metadata, column headers, badges, captions, section labels |
| `fontBody`    | 13  | `lineBody` 18    | Everything else: rows, buttons, body, menus                |
| `fontLarge`   | 15  | `lineLarge` 20   | Dialog titles, empty-state titles, a Doctor finding's name |
| `fontTitle`   | 20  | `lineTitle` 26   | Page and step titles                                       |

Weights `weightRegular` 400 · `weightMedium` 500 · `weightSemibold` 600. `trackingTitle` `-0.01em` is the ONE letter-spacing, on PageHead's title only. No serif, no mono, no uppercase, no other letter-spacing outside the diagram. Numbers that line up use `fontVariantNumeric: "tabular-nums"` (SlotLabel, meters, counts).

## Spacing (`space`)

Named by pixels: `sp2` · `sp4` · `sp6` · `sp8` · `sp12` · `sp16` · `sp24` · `sp32`. A composite built from those tokens is allowed where a frame calls for it: the Row's 10 px gap (`t.sp8 + 2`), the 20 px Dialog padding, the 52–64 px sub-row indents, the 1–2 px optical nudges. Keep those as drawn rather than snapping them to a step. Never add a new spacing token: a value that must AGREE across surfaces gets a role-named const beside its component (e.g. `DIALOG_PAD_X` = 20 in `ui/Dialog.tsx`).

## Radii (`radius`)

`radiusSmall` 4 (checkboxes, badges, menu items) · `radiusControl` 6 (buttons, fields, dropdowns, tabs, sidebar items) · `radiusPanel` 8 (notices, menus, toasts, the segmented track) · `radiusGroup` 10 (list groups, dialogs, the popover card) · `radiusPill` 999 (switches, dots).

## Fixed heights (`size`)

`controlSmall` 22 (sm buttons and selects inside rows) · `control` 28 (md buttons, fields, the search box) · `controlLarge` 32 (the footer's main action) · `row` 36 (a list row) · `rowControls` 34 (rows that hold dropdowns — Set up) · `rowCompact` 30 (sub-rows) · `toolbar` 52 · `footer` 56.

## Elevation (`elevation`)

`shadowSegment` (the selected segment / tab) · `shadowMenu` `0 8px 24px rgba(0,0,0,0.14), 0 0 0 0.5px rgba(0,0,0,0.12)` (menus, dropdown lists, toasts, the popover card — it carries its own 0.5 px ring, so no border) · `shadowDialog` (dialogs and sheets).

## Signal-path tokens (frozen, diagram only)

`bgAlt` (`#ffffff` — the diagram sits on white) · `bg` · `ink` · `ink2` · `mutedInk` · `faint` · `hairline` · `hairlineStrong` · `accentDeep` · `accentSoft` · `accentBorder` · `onAccent` · `warn` · `sevWarn` · `sevWarnBorder` · `shadow` · `shadowBadge` · `dropShadowStripArt` · `strip*` · `serif` · `sans` · `mono` · `fsSubhead` · `fsName2` · `fsUi` · `fsLabel` · `fsData` · `fsData2` · `fsTag` · `lsTag` · `rBtn` · `rCard` · `rLg` · `space1`…`space8`.

Read ONLY by `ui/BlockArt.tsx`, `ui/blockart/`, `models/blockArt.ts`, `views/SignalChainView.tsx`, `views/ActiveSignalChainView.tsx` and `views/signalChain/` — names and values frozen with that code. `FONT_SANS` (exported) and `plainButton` are imported by the diagram too, so they stay as they are. **Enforced:** `src/__tests__/DSPrimitives.test.tsx` scans every other source file and fails on a `t.<signal-path token>` read, on `textTransform`, and on a `letterSpacing` other than PageHead's `trackingTitle` or an `"inherit"` reset. A value the diagram gets from a caller (Level's live-scene tag colour) is passed as a B token.

## Composed styles — `useStyles()` → `s`

`src/theme/styles.ts` `buildStyles(t)` returns the composed style objects more than one component reuses, so a literal lives in exactly one place. Read directly (`style={s.searchBox}`) or spread + override (`style={{ ...s.searchBox, flex: 1 }}`).

| Entry           | What it is                                                                                                  |
| --------------- | ----------------------------------------------------------------------------------------------------------- |
| `s.searchBox`   | Search/filter field frame (28 high, `controlBorder` edge, icon + input inside); add `flex: 1` per call site |
| `s.popoverCard` | Floating popover card (white, `radiusGroup`, `shadowMenu`, no border) — the `Menu` `surface="popover"`      |
| `s.menuCard`    | Scrollable menu card (white, `radiusPanel`, `shadowMenu`, 4 px inset) — the `Menu` and `Select` list        |

Helpers in `tokens.ts`: `sectionLabel(t)` (a section label: sentence case, 12/600 `textSecondary` — there are no kickers), `plainInput(t, extra?)` (the border-less transparent inline-edit input) and `plainButton(extra?)` (the native-`<button>` reset behind clickable rows, chips and glyphs; the `Button` `bare` variant). For a search/filter box prefer the `SearchInput` primitive over re-inlining `s.searchBox`. When you need a shared style that doesn't exist yet, add it here (built from B tokens) rather than duplicating an inline literal.

## Component catalogue

Which component to reach for, and where it lives, is in `ui-components.md` (primitives, `Select`, `List`, `Badge`, `Notice`, `Footer`, `PageHead`/`Steps`, `PageState`/`EmptyState`, `SplitView`/`PageColumn`, meters, `Icon`, `BlockArt`).
