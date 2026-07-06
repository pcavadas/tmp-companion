---
name: tmp-companion-frontend
description: "How to build and change the TMP Companion app's React/TypeScript frontend (a Tauri 2 desktop app) the way this codebase expects. Use whenever working in src/ — implementing a design handoff, adding or editing a view in the Level/Copy/Songs/Catalog/Settings tabs, wiring a Tauri command into the UI, removing a feature as a full vertical slice (UI + wrapper + types + backend command), writing a Vitest or Playwright spec, or debugging why a frontend change fails lint or tsc. Covers the theme-token system (useTheme/useStyles), one-component-per-file layout, ui/primitives + the Icon/BlockArt catalog, the typed lib/invoke wrappers and their camelCase/snake_case rule, the module-scoped libraryScan/useLiveDevice stores, the mocked-invoke Vitest pattern (real-timers gotcha) plus the dual-mode Playwright harness, and the strict lint/tsc traps (react-hooks/refs, set-state-in-effect, no-unnecessary-condition). NOT for Rust wire code (→ tmp-companion-protocol) or catalog data rows (→ tmp-companion-catalog)."
---

# TMP Companion frontend

TMP Companion is a Tauri 2 desktop app: a Rust backend exposing ~80 `invoke` commands and a React 18 + TypeScript frontend. This skill is the playbook for changing that frontend without re-deriving the house conventions — which matters because the app receives recurring **design handoffs** and each one otherwise re-learns the same token mapping, file layout, test scaffold, and lint traps.

**Orient first.** `CLAUDE.md` is the authoritative index (house rules + gotchas); this skill is the _how-to_. When the two disagree, `CLAUDE.md` wins — but the source tree outranks both: grep before trusting any doc symbol, and flag drift when you find it.

## Layout at a glance

```text
src/
  theme/      tokens.ts (LIGHT-ONLY design tokens) · styles.ts (composed-style registry) · ThemeContext.tsx (provider + useTheme/useStyles)
  ui/         primitives.tsx (Button/Slider/Modal/Scrim/MenuItem/…) · Icon.tsx + iconNames.ts (line icons) · BlockArt.tsx + blockart/ (procedural SVG amp/cab/pedal art)
  lib/        invoke.ts (typed Tauri-command wrappers) · types.ts (hand-written mirrors of Rust structs) · format.ts · gates.ts
  views/      level/ · copy/ · songs/ · settings/ — feature folders, one component per file + a barrel index.ts; flat views (CatalogView, PresetList, SignalChainView…) sit directly under views/. (`models/` is the model-catalog DATA layer the CatalogView reads, NOT a view folder.)
  views/overlays/  the multi-step leveling wizard (WizardShell + one file per *Body.tsx stage)
  App.tsx     shell routing the 5-tab IA (Level / Copy / Songs / Catalog / Settings)
  __tests__/  setup.ts (global invoke mock + jsdom shims) + *.test.tsx
```

The app is **click-only by design** — no keyboard shortcuts, no command palette (the ⌘K palette was deleted on purpose). Don't add them back.

## Where does this go? (decision block)

- **A new bit of UI** → reuse an existing primitive first (`primitives.tsx`, `Dialog`, `Menu`, `ActionBar`, `SearchInput`); a new shared primitive is justified only at ≥2 call sites. Cross-cutting UI = ONE tokenized component owning its chrome, never per-instance wrappers with hand-set padding/px.
- **A new component** → its own file under the right feature folder, re-exported from the folder's `index.ts`.
- **A new line-icon** → the `Icon` catalog 3-file edit (below); never inline SVG in a view.
- **Data every device tab needs** → subscribe to the `libraryScan` store; NEVER add a second device read or a per-tab scan trigger (`references/lint-and-stores.md`).
- **A new backend value** → typed wrapper in `lib/invoke.ts` + mirror in `types.ts`; a value with no backing command renders an explicit empty/`—`/disabled state, never an invented number (slow regions use `.tmp-skel` skeletons driven by real fetch status).

## When you're handed a Claude-Design handoff

1. **Read the whole handoff first** and enumerate every deliverable (spec checklists/manifests). Handoffs bundle several changes; the easy failure is shipping the loud one and dropping a quiet one. Diff each shipped asset against the handoff's final export.
2. **Reconcile the design against the codebase before coding.** A handoff may refine an already-shipped feature — diff current implementation vs spec to find the delta, and prefer extending an existing shared component backward-compatibly over re-rolling a parallel one (the Copy refinement extended the Level page's `SignalChainView` with optional interactive props rather than keeping its own renderer). Prototypes omit already-wired features; when design and live app conflict, ask — don't silently pick.
3. **Map palette/typography to real tokens**, never raw hex. **Severity-token trap:** this DS's `t.ok`/`t.okSoft` are the terracotta ACCENT — map a handoff's green→`good`/`goodSoft`/`goodBorder`, amber→`sevWarn`. A literal `t.ok` renders terracotta where green was intended.
4. **A Catalog-tab handoff that changes catalog DATA must keep the test oracles in sync** — changing a row's `form`/`category`/glyph-source breaks `models-catalog.test.tsx`'s count oracles; update them in the same change (and see the `tmp-companion-catalog` skill: the catalog is generated).

## Conventions

### Theme tokens — never hardcode colors/sizes

- `const { t } = useTheme();` — the token object (colors, fonts, sizes, radii, letter-spacing). LIGHT-ONLY; no dark mode.
- `const s = useStyles();` — the composed-style registry; entries like `s.kicker(color)` are factories returning `CSSProperties`.

`src/theme/tokens.ts` is the source of truth for token names — **read it** rather than trusting any list. Styling is **inline `style={{}}` objects** read straight off `t` — no CSS modules, styled-components, Tailwind, or className systems. Full token catalog: `references/theme-tokens.md`.

### Primitives, Icon, and block art

- **Primitives** live in one file, `ui/primitives.tsx` (deliberately unsplit — Modal renders Button, circular-import risk). Stand-alone shared primitives: `Dialog` (the ONE DS dialog shell — every overlay routes through it; `position:fixed` backdrop, `DialogHeader/Body/Footer` slots, sm/md size scale), `Menu` (the ONE anchored dropdown/context menu), `ActionBar` (the ONE bottom-bar shell), `ProgressBar`, `ReadingPill`. Never hand-roll a scrim + absolutely-positioned panel. Every prop-bearing component declares a named `XxxProps` interface.
- **Icons** are `<Icon name="plus" size={14} stroke="currentColor" />` from `ui/Icon` — a fixed union of names. Adding one is a 3-file edit: the `ICONS` array in `ui/iconNames.ts` + a `case` in `ui/Icon.tsx` + bumping the `ICONS.length` count assertion in `__tests__/DSPrimitives.test.tsx`.
- **Device block art** renders through `ui/BlockArt.tsx` + `ui/blockart/*` — never Fender product photos (`<img>` tiles are banned; the copyrighted PNGs were removed). `BlockArt` takes an optional `form` (combo|head|half_stack, picks the chassis via `AMP_FORM_ICON`) because one `block_id` is catalogued under BOTH combo and head. Device FenderIds carry cab/IR suffixes — resolve via `models/blockArt.ts` `resolveBlockArt`/`resolveDeviceId` (check-first-then-strip, mirrors Rust `is_amp_model_id`); never hand-roll suffix stripping. **Every BlockArt-feeding path must pass the FULL art prop set** through the ONE shared adapter `models/blockArt.ts::blockArtTile(model)` — a caller that cherry-picks props silently renders defaults (real bug: wrong footswitch shape, lost loop numbers, lost accent chassis — on the strip but right in the Catalog). Guard a new caller with the mock-`BlockArt` prop-capture test (`references/testing.md`).
- **Strip amp/cab CREATE decision is DEVICE-DRIVEN; catalog `form` only VETOES the combo case.** `models/blockArt.ts::nodeTileArt(model, cabSimId, isCombo)` (**`isCombo` REQUIRED, no default**) is the single branch every strip caller routes through: a standalone `ACD_CabSimTMS` names its cab (dual-cab split via `stripExpand.ts::expandDualCab`); a head-form amp carrying a `cabSimId` becomes ONE head-over-cab tile (`HalfStackArt`); a combo's `cabSimId` is its built-in speaker — `isComboBid` SUPPRESSES the stack. `form` must NEVER CREATE a cab split (the deleted `models/halfStack.ts` drew phantom cabs onto bare heads — don't resurrect it). `cabSim2Enabled` on an AMP node means dual-MIC, not dual cab.
- **Device-returned strings are NOT unique — never use one alone as a React key** (duplicate-named user IRs collided; prefix with the index or a stable id).

## Wiring a Tauri command into the UI

Frontend never calls `invoke()` inline; it calls a typed wrapper in `src/lib/invoke.ts`:

```ts
export const listLevelBlocks = (slot: number): Promise<LevelBlock[]> =>
  invoke("list_level_blocks", { slot });
```

- **Casing:** top-level arg keys are **camelCase** (Tauri converts); keys _inside_ a JSON payload struct stay **snake_case** to match serde (`target_lufs`). Get it wrong and the command silently receives `undefined`.
- **The type mirror:** `src/lib/types.ts` mirrors the Rust serde structs by hand. Adding a Rust field without the mirror **fails silently** (test mocks are untyped). `invoke.test.ts` asserts the exact `cmd` wrapper count — bump on add/remove; named-export-only wrappers (the leveling cancel lane) live OUTSIDE `cmd` and get their own `expectCall`. `liveEvents.test.ts` pins the whole `LIVE_EVENT` registry — a new `tmp://` event extends the `toEqual`.
- A missing command is a backend change (a `#[tauri::command]` in `src-tauri/src/commands/` + the `bootstrap.rs` `generate_handler!` list) — but grep the existing seams first (`session.rs`, `proto.rs`, the `probe` subcommands, `BackupPresetRow`); the Copy save path and its per-preset data were both already-present seams, not new reads.

## Shared device stores (summary — full rules in `references/lint-and-stores.md`)

- The ~22 s startup backup (`read_library_via_backup`) feeds ONE module-scoped store (`views/level/libraryScan.ts`) consumed by Level, Copy, AND Songs. The trigger is App-owned; new tabs subscribe, never re-trigger, never add a second device read.
- Live device state (`useLiveDevice`) is the same module-store pattern — component `useState` would revert the hero on a tab-switch remount.
- Isolated view tests must SEED both stores (`ensureLibraryScan()` after render + `resetLibraryScan()`/`resetLiveDevice()` in `beforeEach`) — see `references/testing.md`.

## Testing

Vitest + React Testing Library, jsdom. Full pattern: `references/testing.md`. Essentials: render through `<ThemeProvider>` (hooks throw outside it); `invoke` is globally mocked in `src/__tests__/setup.ts` (`emptyResultFor` per command — extend it for new mount-time commands); **REAL timers only** (RTL's `waitFor`/`findBy` hang forever under fake timers); after adding a test, run `bun run test` — a green `tsc`/build does not run it.

**Full UI journeys** are the dual-mode Playwright harness in `e2e/` (same specs offline vs real device). Run via `bun run e2e` / `bun run e2e online` — never raw `playwright test` (you'd skip the stale-`:7600` guard + device recovery). Vitest owns component/logic coverage; the harness owns click→invoke→device round trips.

## Lint & typecheck (summary — full trap catalog in `references/lint-and-stores.md`)

Strictest typescript-eslint presets, type-aware, `--max-warnings 0`. **No escape hatches in `src/`** (no `eslint-disable`/`@ts-ignore`/`any`/`!`) — fix code, never silence. The most common traps: `react-hooks/refs` (sync refs in an effect, not render), `set-state-in-effect` (prev-compare render adjustment; the one legit exception is committed-DOM measurement), `no-unnecessary-condition` (express real `T | undefined`; drop redundant operands), `?.length > 0` doesn't compile, no `Array.at()` on this lib target.

## Before you call it done

```bash
bunx tsc --noEmit && bun run lint && bun run test && bun run format && bun run build
```

Then sanity-check against the _ask_: for a handoff, re-walk its deliverable list; for a cleanup, confirm the diff is net-negative (`git diff --stat`). A device-facing change additionally needs an eyeball on real hardware — a green suite does not prove device-data shapes render right. State plainly what you verified.

## References

- `references/theme-tokens.md` — full token catalog + the composed-style registry.
- `references/testing.md` — the Vitest + mocked-invoke pattern end to end, store seeding, the prop-capture pattern, wizard test traps.
- `references/lint-and-stores.md` — the full strict-lint trap catalog + the libraryScan/useLiveDevice store rules.
