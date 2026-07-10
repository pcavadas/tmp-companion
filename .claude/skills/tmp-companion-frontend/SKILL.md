---
name: tmp-companion-frontend
description: "How to build and change the TMP Companion app's React/TypeScript frontend (a Tauri 2 desktop app) the way this codebase expects. Use this skill whenever working in src/ — implementing a design handoff, adding or editing a view in the Level/Doctor/Copy/Songs/Catalog/Settings tabs, wiring a Tauri command into the UI, removing a command or feature as a full vertical slice (UI + invoke wrapper + types + the backend command), writing a Vitest test or a Playwright e2e spec for a screen, extracting or consolidating shared DS components, primitives, or hooks across views, or debugging why a frontend change fails lint or tsc. Covers the theme-token system (useTheme/useStyles), one-component-per-file layout, the ui/primitives + Icon/BlockArt catalog, the typed lib/invoke wrappers and their camelCase/snake_case rule, the Vitest + mocked-invoke test pattern (real-timers gotcha) plus the dual-mode Playwright e2e harness, and the lint/tsc traps that are easy to miss — most importantly that react-hooks/refs errors on reading ref.current during render, so 'derive state into a useRef' refactors are false positives here."
---

# TMP Companion frontend

TMP Companion is a Tauri 2 desktop app: a Rust backend exposing ~80 `invoke` commands and a React 18 + TypeScript frontend that talks to it. This skill is the playbook for changing that frontend without re-deriving the house conventions every time — which matters because the app receives recurring **design handoffs** (the leveling-page v2, the Catalog tab, the Settings redesign, the Copy feature, …) and each one otherwise re-learns the same token mapping, file layout, test scaffold, and lint traps.

**Orient first.** `CLAUDE.md` is the authoritative architecture index + the running log of gotchas — skim its `UI (src/)` bullet and its Testing/Gotchas paragraph before a non-trivial change; this skill is the _how-to_, that file is the _what-is-where_ and the latest dated traps. When the two disagree, `CLAUDE.md` wins; tell the user if you spot drift.

## Layout at a glance

```text
src/
  theme/      tokens.ts (LIGHT-ONLY design tokens) · styles.ts (composed-style registry) · ThemeContext.tsx (provider + useTheme/useStyles)
  ui/         primitives.tsx (Button/Slider/Modal/Scrim/MenuItem/…) · Icon.tsx + iconNames.ts (line icons) · BlockArt.tsx (dispatch + AMP_FORM_ICON) + blockart/ (procedural SVG amp/cab/pedal art, split per family — amps/ampsCombo/ampsHead*, mics/micBodies*, pedals/pedalsMotif*/pedalsSpecial/pedalKnobs, forms/formsPedal/formsRack, parts/partsCloth/partsPanel, shared.ts + sharedIds/sharedTones/sharedCloth, cabs.tsx, blockColors.generated.ts)
  lib/        invoke.ts (typed Tauri-command wrappers) · types.ts (hand-written mirrors of Rust structs) · format.ts · gates.ts · shared hooks useDeviceLoad.ts / useAutoAdvance.ts (run-wizard auto-advance) / usePickedRows.ts (setup-step bulk-pick selection)
  models/     catalog.ts (taxonomy) · blockArt.ts (resolvers) · blockArtCatalog/ (the catalog rows, one file per category) · tmp-model-guide.json — the model-catalog DATA layer the CatalogView reads, NOT a view folder
  views/      level/ · doctor/ · copy/ · songs/ · settings/ — feature folders, one component per file + a barrel index.ts; flat views (CatalogView, PresetList, SignalChainView…) sit directly under views/.
  views/overlays/  the multi-step WIZARD — LevelingWizard (one file per *Step.tsx body). (The Bulk Block Edit `overlays/bulk/` wizard was DELETED — superseded by the Copy tab.)
  App.tsx     shell routing the 6-tab IA (Level / Doctor / Copy / Songs / Catalog / Settings)
  __tests__/  setup.ts (global invoke mock + jsdom shims) + *.test.tsx
```

The app is **click-only by design** — no keyboard shortcuts, no command palette (the ⌘K palette was deleted on purpose). Don't add them back.

## When you're handed a Claude-Design handoff

A handoff is usually a folder (often `~/Downloads/design_handoff_*`) with a prototype + a written spec. The work is to land its design in real components wired to the real backend. Steps that consistently pay off:

1. **Read the whole handoff first**, and enumerate every deliverable it lists (the spec usually has a checklist or a file manifest). Handoffs bundle several changes; the easy failure is shipping the loud one and silently dropping a quiet one (a removed sub-feature, a renamed prop, the _correct_ exported icon variant). Diff each shipped asset against the handoff's final export.
2. **Reconcile the design against the codebase before coding.** A handoff may **refine an already-shipped feature**, not add a new one — diff the current implementation against the spec first to find the delta, and prefer extending an existing shared component backward-compatibly over re-rolling a parallel one (e.g. the Copy refinement extended the Level page's `SignalChainView` with optional interactive props rather than keeping Copy's own renderer). Prototypes are also built in isolation and often omit features that are already wired (e.g. a handoff for the Settings page that forgets the existing Playback-level control). When the design and the live app conflict, that's a question for the user, not a call to make silently — they generally prefer fixing the source handoff over you guessing.
3. **Map the design's palette/typography to real tokens** (next section) rather than pasting raw hex. If a design color has no token, that's a signal to either pick the closest token or ask — not to hardcode `#c0392b`. **Severity-token trap:** a handoff's `ok`/green usually means GREEN, but this DS's `t.ok`/`t.okSoft` are the terracotta ACCENT — map a handoff's green→`good`/`goodSoft`/`goodBorder`, amber→`sevWarn` (see Theme tokens below). A literal `t.ok` renders terracotta where green was intended — a silent visual bug.
4. **One component per file.** Split a multi-component prototype into focused files under the right feature folder, each re-exported from the folder's `index.ts`. Pedro consistently prefers small modules over barrels-of-many; propose the split proactively.
5. **A Catalog-tab handoff that changes catalog DATA must keep the test oracles in sync.** (The Catalog tab is `CatalogView`; its data lives in `src/models/`, whose folder + the `models-catalog.test.tsx` filename kept the `models` domain name.) The catalog is the committed `src/models/tmp-model-guide.json`. Changing a row's `form`/`category`/glyph-source breaks `models-catalog.test.tsx`'s count oracles — update them in the same change. (Prototype data shapes also don't always map 1:1 onto prod's normalized model — diff each shipped deliverable against the handoff's final export.)

## Conventions

### Theme tokens — never hardcode colors/sizes

Two hooks, both from `theme/ThemeContext`:

- `const { t } = useTheme();` — `t` is the token object (colors, font families, font sizes, radii, letter-spacing, density). LIGHT-ONLY; there is no dark mode.
- `const s = useStyles();` — `s` is the composed-style registry; entries like `s.kicker(color)` are factories returning a `CSSProperties` object (e.g. `s.kicker(t.accentDeep)` for a section micro-label).

`src/theme/tokens.ts` is the source of truth for token names — **read it** rather than trusting this list. A representative sample so you recognize the shape: colors `bg` / `bgAlt` / `ink` / `ink2` / `mutedInk` / `faint` / `accent` / `accentDeep` / `hairline` / `track` / `knob`; fonts `serif` / `sans` / `mono`; sizes `fsTitle` / `fsBody` / `fsUi` / `fsLabel` / `fsMicro`; radii `rBtn` / `rMd` / `rLg` / `rPill`. There are many more (e.g. `accentSoft`, `hairlineStrong`, `onInk`, `hover`, `shadow`, `rMenuItem`, `rCard`, `lsMeta`) — grep `tokens.ts`.

Styling is **inline `style={{}}` objects**, read straight off `t`. This is the deliberate house style — do not reach for CSS modules, styled-components, Tailwind, or a className system. The full token table + the composed-style list live in `references/theme-tokens.md`.

### Primitives, Icon, and block art

- **Primitives** live in one file, `ui/primitives.tsx`, imported directly (no index barrel): `Button`, `Slider`, `Select`, `SearchInput`, `Modal`, `Scrim`, `Toast`, `Panel`, `Checkbox`, `Toggle`, `MenuItem`, `MenuDivider`, `AlertBanner`, `SegmentedControl` (generic radio pill). A few stand-alone shared primitives sit in their own `ui/` files: `Dialog` (the ONE DS dialog shell — `ui/Dialog.tsx`; `DialogHeader`/`DialogBody`/`DialogFooter` slots; `Modal`/`SaveOverlay`/`WizardShell`/`HowLevelingSheet` all route through it), `Menu` (the ONE anchored dropdown/context menu — `ui/Menu.tsx`; Scrim + anchored card), `ProgressBar` (determinate bar), `ActionBar` (the ONE bottom-bar shell), `ReadingPill` (the disabled "Reading presets…" gate pill), plus the DS atoms/scaffolds from the DS series (#49–#51): `Tag` (the chip — moved OUT of primitives.tsx to `ui/Tag.tsx`; tones accent/good/warn/neutral/neutralFill × sm/md), `Spinner`, `Dot`, `SlotLabel` (mono slot-number cell), `Meter` (STATIC track+fill CPU bar — deliberately NOT `ProgressBar`, whose 0.4s transition would lag the paint), `PaneEmpty` (medallion detail-pane empty state), `ConfirmBar` (run stop-confirm), `RunRow` (run progress row — opaque icon/status ReactNode slots), `SetupGroupHeader`/`PresetOptionRow`/`ApplyToBar` (the wizard setup scaffolds), `Rail` (the 210px left rail), and the songs-table pieces `views/songs/{ListHeader,SongRow}.tsx`. Use them before hand-rolling — a popover/context menu is `ui/Menu` (fill with `MenuItem`/`MenuDivider`), a dialog/overlay is `Dialog`, a search/filter box is `SearchInput`; never hand-roll a `Scrim` + absolutely-positioned panel (that was extracted into `Menu`). **DS bar (Pedro's standing brief):** for cross-cutting UI prefer ONE tokenized component over per-instance wrappers; the DS owns padding/chrome (don't hand-set padding); use the size scale + theme tokens (e.g. `rDialog`), never raw px or magic numbers.
- **One bottom-bar style across the app — reuse `ui/ActionBar`.** It's a `{ left, right }` two-slot shell (min-height 60, `0 20px`, hairline-top on `bgAlt`, space-between); the Copy steps + the Presets selection footer (`ContextFooter`) all render through it. Don't re-roll a per-view footer with its own height/padding (that drift — a 52px footer vs a 60px one — is exactly what `ActionBar` consolidated).
- **Icons** are `<Icon name="plus" size={14} stroke="currentColor" />` from `ui/Icon` — a fixed union of line-icon names. Never paste raw emoji/symbol chars as icons. Adding a NEW icon name is a 3-file edit: the `ICONS` array in `ui/iconNames.ts` + a `case` in `ui/Icon.tsx` + bumping the hardcoded `ICONS.length` count assertion in `__tests__/DSPrimitives.test.tsx` (same count-oracle pattern as the catalog test in step 5) — miss the count bump and Vitest goes red on the gate.
- **Device block art** (amps/cabs/pedals/mics) renders through the procedural SVG engine: `ui/BlockArt.tsx` dispatch + `ui/blockart/*`. **Never render Fender product photos** — the 315 copyrighted PNG tiles were removed on purpose; render SVG art, never an `<img>` photo tile. `BlockArt` takes an optional `form` (combo|head|half_stack); for amps it picks the chassis (`AMP_FORM_ICON`) the per-id glyph can't, because one `block_id` is catalogued under BOTH combo and head — thread `form` through when a row's form is known. Device FenderIds carry cab/IR/convolution suffixes the catalog omits — resolve via `models/blockArt.ts` `resolveBlockArt`/`resolveDeviceId` (shared `resolveCatalogId`: check-first-then-strip + a `+NoFx` bridge), which MIRRORS Rust `is_amp_model_id`; never hand-roll suffix stripping (greedy-strip breaks the catalogued-with-suffix reverb amps). See the companion CLAUDE.md "Amp-id matching" + "Block-art resolution" gotchas. **Every BlockArt-feeding path must pass the FULL art prop set** (`glyph`/`tone`/`lab`/`footswitch`/`bodyColor`/`accentColor`/`panelColor`): there is ONE shared adapter, `models/blockArt.ts` `blockArtTile(model)`, that every signal-chain strip caller consumes (`ActiveSignalChainView.toStripBlock`, `copy/CopyPath.mkTile`) — the Catalog (`CatalogView`) is the reference full-prop caller. A caller that cherry-picks props (or a new art field wired into only some callers) silently renders the DEFAULT: a Boss pedal got the round footswitch instead of its plate, FX-loop tiles lost their loop number, and the 8 Fender reverbs lost the accent chassis — all on the strip but right in the Catalog, because no test asserted strip↔Catalog prop parity. Guard a new caller with the mock-`BlockArt` prop-capture test (`references/testing.md`).
- **Strip amp/cab CREATE decision is DEVICE-DRIVEN; catalog `form` only VETOES the combo case.** `models/blockArt.ts::nodeTileArt(model, cabSimId, isCombo)` (3-arg, **`isCombo` REQUIRED — no default**) is the single branch every strip caller routes through: a standalone `ACD_CabSimTMS` block is named from its `cabSimId` (and `views/stripExpand.ts::expandDualCab` splits it into TWO parallel cab tiles when `cabSim2Enabled` + `cabSimId2`); a **head-form** AMP node carrying a `cabSimId` (e.g. preset 003's HIWAY `…CabIR`) becomes ONE head-over-cab tile via `ampCabHalfStack` → `HalfStackArt`; a **combo-form** amp's `cabSimId` is its built-in speaker, NOT a stack — `catalog.ts::isComboBid(model)` (catalog `form`) sets `isCombo` and SUPPRESSES the stack so it renders as a single combo glyph (e.g. blonde '65 Twin `ACD_TwinReverb65BlondeNoFx`); a bare head (no `cabSimId`) is a plain glyph. `cabSim2Enabled` on an AMP node means dual-MIC on ONE cab, NOT a dual cab. **Catalog `form` is load-bearing here BUT one-directional:** it may SUPPRESS a stack for combos (`isComboBid`, allowed), but must NEVER CREATE a cab split — the deleted `models/halfStack.ts` keyed _splits_ on `form` and drew phantom cabs onto bare heads; never resurrect that. Split creation stays keyed on device `cabSimId` presence. The 3rd `isCombo` arg is required precisely so a new strip caller can't silently omit it and re-stack every combo.
- **Device-returned strings are NOT unique — never use one as a React key.** IR names, preset displayNames, and saved-block names can collide (the device returned duplicate-named user IRs); a key like `` `ir:${name}` `` then collides → React reuses / can't swap the subtree, which masquerades as a navigation/state bug (it cost real debugging time before the duplicate-key console warning gave it away). Prefix with the array index (`` `ir:${i}:${name}` ``) or a stable id.

### No-fabricate rule

Every value shown must trace to a real backend command. A field with no backing data renders an explicit empty / `—` / disabled state — never an invented number. Slow-to-arrive regions use the `.tmp-skel` shimmer skeletons (`ui/Skeleton.tsx`) driven by real fetch status, not timers.

## Wiring a Tauri command into the UI

Frontend never calls `invoke()` inline; it calls a typed wrapper in `src/lib/invoke.ts`:

```ts
// one wrapper per command — argument keys are camelCase, return type is a types.ts interface
export const listLevelBlocks = (slot: number): Promise<LevelBlock[]> =>
  invoke("list_level_blocks", { slot });
```

Two load-bearing rules:

- **Casing:** the _top-level_ arg keys you pass to `invoke` are **camelCase** — Tauri auto-converts them to the Rust handler's snake*case params. But keys \_inside* a JSON payload struct stay **snake_case** to match `serde` (e.g. a `LevelJob` carries `target_lufs`, `topology_id`). Get this wrong and the command silently receives `undefined`.
- **The type mirror:** `src/lib/types.ts` holds hand-written TS interfaces mirroring the Rust `serde` structs. Adding a Rust field without updating the mirror **fails silently** (test mocks are untyped, so nothing complains until runtime). When you touch a command's shape, update both sides. `invoke.test.ts` asserts the exact wrapper count (`Object.keys(cmd).length`) with a history comment — update it when you add **or remove** a wrapper IN the `cmd` namespace (a feature deletion decrements it). Caveat: some wrappers are deliberately named-export-only and NOT in `cmd` (the fire-and-forget leveling cancel lane `cancel{Preset,Scene,Footswitch}Leveling`), so adding one does **not** move the count — assert it with its own `expectCall` instead. Check whether your new wrapper belongs in `cmd` before assuming the count changes.

If a command you need doesn't exist yet, that's a backend change (Rust `lib.rs` `generate_handler!` + an engine function) — coordinate it; don't fake the data on the frontend. Conversely, before _assuming_ a backend command is missing, grep the existing seams first — `src-tauri/src/session.rs` (e.g. `replace_node`/`insert_node`/`remove_node`/`extract_active_graph`), `proto.rs` (often already golden-tested), and the `probe` subcommands. The Copy feature's whole save path was a thin Tauri wrapper because the live structural-edit primitives were already RE'd + present (`probe --insert-active`); per-preset data was likewise already on the one `read_library_via_backup`/`BackupPresetRow` backup (one added field, no new device read).

### Shared device data: the `libraryScan` store (App-owned, ONE scan/connection)

Some device data is too expensive to read per-tab: the whole preset library — scenes, blocks, signal graphs, levelable footswitches (`footswitchesPerIndex`, consumed by the Level wizard's third dispatch), AND the song↔preset map — arrives in ONE ~22 s device backup (`read_library_via_backup` → `BackupReadResult`, decoded from `normalDb.db3`). It lives in a **module-scoped store**, `src/views/level/libraryScan.ts` (`subscribeLibraryScan` / `getLibraryScan` + `useSyncExternalStore`), consumed by **Level, Copy, and Songs**. The scan TRIGGER is **App-owned**: `App.tsx` fires `ensureLibraryScan()` once on the connect edge and `resetLibraryScan()` on detach — so every device tab shares ONE scan and a tab switch NEVER re-triggers it. A new tab that needs backup-sourced data **CONSUMES the store** (`const lib = useSyncExternalStore(subscribeLibraryScan, getLibraryScan)`); it does NOT add its own `ensureLibraryScan` trigger — that re-introduces the per-tab-rescan bug this layout exists to prevent. Extend the store by adding a field to `BackupReadResult` (+ the `lib.rs` parse + the `types.ts` mirror) and deriving the shape you need inside `ensureLibraryScan`, keyed by 0-based list index (device slot − 1) — never a second device read.

The SAME module-store pattern (not component `useState`) backs `src/views/level/useLiveDevice.ts` — the app-global LIVE device state (active preset/scene/graph from the 5 `tmp://` monitor events). It must be module-scoped because a LevelView tab-switch REMOUNTS the hook: a component-local snapshot would reset to INITIAL and, since the monitor only pushes on a CHANGE, the hero would revert to the stale connect-time preset (a real bug this fixed). Consequences: the hero SLOT badge reads the frontend `activeListIndex` (live-preset event), NOT `graph.slot` (the field-3 graph push carries no slot); and the store exports a TEST-ONLY `resetLiveDevice()` (tears down + re-arms the event bridge) — call it in `beforeEach` since the event-mock clears its listener registry per case (prod never remounts the bridge, so it never tears down).

## Testing

Tests are **Vitest + React Testing Library**, jsdom environment, rendered through the theme provider. The full pattern (mock overrides, async `findBy`, asserting an `invoke` was called with the right args) is in `references/testing.md`. The essentials:

- **Render through `<ThemeProvider>`** — `useTheme`/`useStyles` throw outside it:
  ```tsx
  render(
    <ThemeProvider>
      <SettingsView connected={false} />
    </ThemeProvider>,
  );
  ```
- **`invoke` is globally mocked** in `src/__tests__/setup.ts` — `emptyResultFor(command)` returns a sensible empty shape per command so any screen mounts. Override per-test with `vi.mocked(invoke).mockImplementation(...)` to feed real data, then assert against `vi.mocked(invoke).mock.calls` to verify a write happened with the right payload.
- **Use REAL timers, not fake ones.** RTL's `waitFor`/`findBy` detect fake timers via the `jest` global and then run their _own_ poll + timeout on the frozen clock, so they hang forever. Real timers + the natural async resolution work fine here.
- After adding a test, run the suite (`bun run test`) — a green `tsc` + build does **not** run your test.

**Full UI journeys** (connect → navigate → edit → save) are covered above Vitest by the dual-mode Playwright e2e harness in `e2e/`: the same `specs/{copy,level,songs}.spec.ts` drive the REAL React app in headless Chromium → an HTTP bridge → a windowless Rust backend (`tauri::test::mock_builder`) → `SimDevice` offline or the real device online. Run via the turn-key wrapper `bun run e2e` (offline, SimDevice, default, no hardware) / `bun run e2e online` (real device — handshake-preflight + a device-recovery trap), which wraps `e2e/playwright{,.online}.config.ts`; don't hand-invoke `playwright test` directly (you'd skip the stale-`:7600` guard + the online recovery). Reach for Vitest for component/logic coverage; reach for the harness when a change spans the click→invoke→device round trip.

## Lint & typecheck traps

`bun run lint` (eslint `--max-warnings 0`) + `bunx tsc --noEmit` are the strict checks (alongside Vitest + the Vite build). The flat config is the STRICTEST typescript-eslint presets — `strictTypeChecked` + `stylisticTypeChecked` (type-aware via `parserOptions.projectService`) + `eslint-plugin-react` (recommended + jsx-runtime; `react/prop-types` + `react-in-jsx-scope` are off, superseded by TS / the new JSX transform). Run lint + tsc by hand before committing — a fresh checkout's missing deps are the usual "passed locally, broken on a fresh checkout" trap.

- **NO escape hatches anywhere in `src/`.** No `eslint-disable` / `@ts-nocheck` / `@ts-ignore` / `@ts-expect-error`, no `any` / `as any`, no non-null `!`. Fix the CODE/TYPES — and verify a strip/edit actually applied before trusting a 0-count (a botched in-place edit once silently no-op'd and faked "0 tsc errors"). Common strict findings + their code fix:
  - **`react-hooks/refs`** ERRORS on reading/writing `ref.current` during render — **sync the ref in an EFFECT (after commit), not during render.** The old "this `useRef` read is a false positive — keep it in React state" guidance is SUPERSEDED.
  - **`react-hooks/set-state-in-effect`** ERRORS on a synchronous `setState` in an effect — use the **"adjust state during render when an input changes"** prev-compare pattern (`const [prev,setPrev]=useState(x); if (x!==prev){setPrev(x); …}`), or derive the value during render (no state at all). Timers/ref-writes stay in the effect; only the `setState` moves.
  - **`no-unnecessary-condition`** reads the **INITIALIZER** type, not a widening annotation — to express "a `Record`/array index may be absent" use a genuine `T | undefined` (Partial-cast the map / length-guard or ternary the array) or model optionality in the TYPE itself (e.g. `profile_by_slot: Partial<Record<number,string>>`).
  - **`no-unnecessary-condition` via aliased narrowing** — TS narrows the OPERANDS of a derived boolean alias downstream, so re-testing one is "always falsy": `const blocked = !edit || …; if (blocked || !edit) return;` errors on the trailing `!edit` (once `blocked` is false, TS knows `edit` is non-null). **Fix: drop the redundant operand** — the alias already covers it (do NOT add a `?.` or recheck). Same root cause as the Record-index variant: the rule is reading TS's real narrowed type.
  - **`restrict-template-expressions`** (allowNumber:false) → `String(x)` for numbers in template strings; **`no-confusing-void-expression`** → brace-wrap void-returning arrow handlers (`onClick={() => { f(); }}`); **`no-misused-promises`** → wrap async handlers `() => { void asyncFn(); }`; **`react-refresh/only-export-components`** → move the **MINORITY** export to a sibling file (a component-less file isn't a refresh boundary).
- **`bunx tsc --noEmit`** also catches what the Vite build won't (the build transpiles without typechecking).
- **Fresh checkout:** `node_modules` and `dist/` are gitignored. Run `bun install` (else hundreds of phantom "Cannot find module 'react'" errors) and `bun run build` (else the Rust `tauri-build` `generate_context!` panics on a missing `frontendDist`) before the checks.
- **TypeScript 6** no longer resolves `node:` imports from `@types/node` alone — `tsconfig.json` needs `"types": ["node"]`.
- The IDE/LSP emits **stale phantom diagnostics during rapid file moves or while another editor/agent rewrites a file concurrently** — `bunx tsc --noEmit` from the CLI is authoritative; trust it over live editor squiggles mid-refactor. A concurrent save can also silently REVERT a tool write with no error — after a multi-file edit, re-grep a distinctive symbol to confirm each write LANDED.

## Before you call it done

Run all five, by hand, from the repo root:

```bash
bunx tsc --noEmit     # types
bun run lint          # strict eslint, --max-warnings 0
bun run test          # Vitest
bun run format        # prettier --write — run before calling a change "done"
bun run build         # Vite production build
```

Then sanity-check against the _ask_, not just the mechanism: for a handoff, re-walk its deliverable list and visually diff shipped assets vs the final export; for a cleanup/refactor, confirm the diff is net-negative (`git diff --stat`) — a de-bloat that adds more than it removes isn't done. State plainly what you verified.

## References

- `references/theme-tokens.md` — full token catalog (colors, fonts, sizes, radii, density, letter-spacing) + the composed-style registry and how `s.kicker(color)`-style factories work.
- `references/testing.md` — the Vitest + mocked-invoke pattern end to end: the global setup shim, per-test overrides, asserting command calls, the real-timers rule, and a worked example.
