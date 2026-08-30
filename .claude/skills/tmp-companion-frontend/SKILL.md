---
name: tmp-companion-frontend
description: "How to build and change the TMP Companion app's React/TypeScript frontend (a Tauri 2 desktop app) the way this codebase expects. Use whenever working in `src/` — implementing a design handoff, adding or editing a view in the Level/Doctor/Copy/Songs/Catalog/Settings tabs, wiring or removing a Tauri command as a full vertical slice (UI + invoke wrapper + types + backend command), writing a Vitest test or a Playwright e2e spec for a screen, extracting or consolidating shared DS components, primitives or hooks across views, or debugging why a frontend change fails lint or tsc. Covers the theme-token system, one-component-per-file layout, the `ui/primitives` + Icon/BlockArt catalog, the typed `lib/invoke` wrappers, the Vitest and Playwright test patterns, and the lint/tsc traps that are easy to miss."
---

# TMP Companion frontend

TMP Companion is a Tauri 2 desktop app: a Rust backend exposing ~90 `invoke` commands and a React 18 + TypeScript frontend that talks to it. This skill is the playbook for changing that frontend without re-deriving the house conventions — the app receives recurring **design handoffs** that would otherwise each re-learn the same token mapping, file layout, test scaffold and lint traps.

**Orient first.** [`notes/overview.md`](../../../notes/overview.md) is the architecture map (_what is where_) and [`.claude/rules/frontend.md`](../../rules/frontend.md) carries the edit-time lint/contract rules, loading automatically when you open a `src/` file. This skill is the _how-to_. `CLAUDE.md` is the index and wins on any rule it states; tell the user if you spot drift.

## Layout at a glance

`src/` splits into `theme/` (tokens + composed styles), `ui/` (primitives, `Icon`, `BlockArt` + the block-art SVG engine), `lib/` (typed `invoke` wrappers, `types.ts`, shared hooks), `models/` (the catalog data layer), `views/` (five feature folders — `level`, `doctor`, `copy`, `songs`, `settings` — plus a flat `CatalogView` and `views/overlays/` for the leveling wizard), and `App.tsx` (shell routing). Full file tree: `references/ui-components.md`.

The app is **click-only by design** — no keyboard shortcuts, no command palette (the ⌘K palette was deleted on purpose). Enter/Escape inside a focused text input, committing or cancelling that field's own edit, is exempt — see the carve-out in `CLAUDE.md`.

## When you're handed a Claude-Design handoff

A handoff is usually a folder (often `~/Downloads/design_handoff_*`) with a prototype + a written spec; the work is landing its design in real components wired to the real backend. Detail for steps 1, 2, 4, 5: `references/gotchas.md`.

1. **Read the whole handoff first**, and enumerate every deliverable it lists.
2. **Reconcile the design against the codebase before coding.** A handoff may **refine an already-shipped feature**, not add a new one.
3. **Map the design's palette/typography to real tokens** (next section) rather than pasting raw hex. A design color with no token means pick the closest one or ask — never hardcode `#c0392b`. **Severity-token trap:** a handoff's `ok`/green means GREEN, but this DS's `t.ok`/`t.okSoft` are the terracotta ACCENT — map green→`good`/`goodSoft`/`goodBorder`, amber→`sevWarn` (see Theme tokens below). A literal `t.ok` renders terracotta where green was intended — a silent visual bug.
4. **One component per file.** Split a multi-component prototype into focused files under the right feature folder, each re-exported from the folder's `index.ts`.
5. **A Catalog-tab handoff that changes catalog DATA must keep the test oracles in sync.**

## Conventions

### Theme tokens — never hardcode colors/sizes

Two hooks, both from `theme/ThemeContext`:

- `const { t } = useTheme();` — `t` is the token object (colors, fonts, sizes, radii, letter-spacing). LIGHT-ONLY; there is no dark mode.
- `const s = useStyles();` — `s` is the composed-style registry; entries like `s.kicker(color)` are factories returning a `CSSProperties` object.

`src/theme/tokens.ts` is the source of truth for token names — **read it** rather than trusting a summary here.

Styling is **inline `style={{}}` objects**, read straight off `t`. This is the deliberate house style — do not reach for CSS modules, styled-components, Tailwind, or a className system. The full token table + the composed-style list live in `references/theme-tokens.md`.

### Primitives, Icon, and block art

Primitives live in `ui/primitives.tsx` + a few stand-alone `ui/` files (`Dialog` — the ONE DS dialog shell, `Menu` — the ONE anchored dropdown, `Tag`, `Spinner`, `ActionBar`, …) — reuse before hand-rolling. Icons route through `ui/Icon` — never paste raw emoji/symbol chars. Device block art (amps/cabs/pedals/mics) renders through the procedural SVG engine `ui/BlockArt.tsx` + `ui/blockart/*` — **never render Fender product photos**. Full inventory, the FenderId→art resolution algorithm, and the strip prop-parity rules: `references/ui-components.md`.

### No-fabricate rule

Every value shown must trace to a real backend command. A field with no backing data renders an explicit empty / `—` / disabled state — never an invented number. Slow-to-arrive regions use the `.tmp-skel` shimmer skeletons, driven by real fetch status, not timers. Corollary — size repeated UI off real data, never a literal count: `references/gotchas.md`.

## Wiring a Tauri command into the UI

Frontend never calls `invoke()` inline; it calls a typed wrapper in `src/lib/invoke.ts`:

```ts
// argument keys are camelCase; return type is a types.ts interface
export const listLevelBlocks = (slot: number): Promise<LevelBlock[]> =>
  invoke("list_level_blocks", { slot });
```

Two load-bearing rules:

- **Casing:** top-level arg keys passed to `invoke` are **camelCase** (Tauri converts them to the Rust handler's snake_case params), but keys _inside_ a JSON payload struct stay **snake_case** to match `serde` (e.g. `target_lufs`, `topology_id`). Get this wrong and the command silently receives `undefined`.
- **The type mirror:** `src/lib/types.ts` mirrors the Rust `serde` structs by hand — adding a Rust field without updating it **fails silently** (test mocks are untyped). `invoke.test.ts` asserts the exact wrapper count (`Object.keys(cmd).length`, hardcoded) — update it when you add **or remove** a wrapper IN the `cmd` namespace. Named-export-only wrappers outside `cmd` (the fire-and-forget leveling cancel lane `cancel{Preset,Scene,Footswitch}Leveling`) don't move the count — assert those with their own `expectCall`.

If a command doesn't exist yet, that's a backend change — coordinate it, don't fake data. Grep the existing seams before assuming one's missing. Detail: `references/gotchas.md`.

### Shared device data: the `libraryScan` store (App-owned, ONE scan/connection)

Some device data (scenes, blocks, graphs, footswitches, the song↔preset map) is too expensive to read per-tab, so it's read ONCE per connection into a **module-scoped store**, `src/views/level/libraryScan.ts`, consumed by **Level, Copy, and Songs**. The scan TRIGGER is **App-owned**: `App.tsx` fires `ensureLibraryScan()` once on the connect edge and `resetLibraryScan()` on detach — so every device tab shares ONE scan and a tab switch NEVER re-triggers it. A new tab that needs backup-sourced data **CONSUMES the store** (`useSyncExternalStore(subscribeLibraryScan, getLibraryScan)`); it does NOT add its own trigger — that re-introduces the per-tab-rescan bug this layout prevents.

The SAME module-store pattern (not component `useState`) backs `src/views/level/useLiveDevice.ts` — the app-global LIVE device state (active preset/scene/graph from the 5 `tmp://` monitor events), for the same tab-switch-remount reason. The hero SLOT badge reads the frontend `activeListIndex`, not `graph.slot`. Deep-dive on both stores: `references/gotchas.md`.

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
- **`invoke` is globally mocked** in `src/__tests__/setup.ts` — `emptyResultFor(command)` returns a sensible empty shape per command so any screen mounts. Override per-test with `vi.mocked(invoke).mockImplementation(...)`, then assert against `vi.mocked(invoke).mock.calls` to verify a write happened.
- **Use REAL timers, not fake ones.** RTL's `waitFor`/`findBy` detect fake timers via the `jest` global and hang forever polling a frozen clock.
- After adding a test, run the suite (`bun run test`) — a green `tsc` + build does **not** run your test.

**Full UI journeys** (connect → navigate → edit → save) are covered above Vitest by the dual-mode Playwright e2e harness in `e2e/` (`bun run e2e` offline / `bun run e2e online`) — reach for it when a change spans the click→invoke→device round trip.

## Lint & typecheck traps

`bun run lint` (`--max-warnings 0`) + `bunx tsc --noEmit` are the strict checks — see `.claude/rules/frontend.md` for the escape-hatch ban and the other common findings. Two more, restated here because this skill's own frontmatter names them:

- **`react-hooks/refs`** ERRORS on reading/writing `ref.current` during render — **sync the ref in an EFFECT (after commit), not during render.** The old "this `useRef` read is a false positive — keep it in React state" guidance is SUPERSEDED.
- **`react-hooks/set-state-in-effect`** ERRORS on a synchronous `setState` in an effect — use the **"adjust state during render when an input changes"** prev-compare pattern (`const [prev,setPrev]=useState(x); if (x!==prev){setPrev(x); …}`), or derive the value during render (no state at all). Timers/ref-writes stay in the effect; only the `setState` moves.

More traps (other eslint rules, fresh-worktree/TypeScript-6 setup, stale IDE diagnostics): `references/gotchas.md`.

## Before you call it done

Run all five, by hand, from the repo root:

```bash
bunx tsc --noEmit     # types
bun run lint          # strict eslint, --max-warnings 0
bun run test          # Vitest
bun run format        # prettier --write — run before calling a change "done"
bun run build         # Vite production build
```

Then sanity-check against the _ask_: for a handoff, re-walk its deliverable list vs the final export; for a cleanup/refactor, the criterion is the **loaded-content budget** — tokens in the always-loaded and skill-body tiers went down, and every deliverable and link still resolves. `git diff --stat` is supporting evidence only: a docs or component split legitimately adds files while cutting loaded tokens. State plainly what you verified.

## References

- `references/theme-tokens.md` — full token catalog + the composed-style registry.
- `references/testing.md` — the Vitest + mocked-invoke pattern end to end.
- `references/ui-components.md` — the full `src/` file tree + the primitives/Icon/block-art inventory and resolution rules.
- `references/gotchas.md` — handoff/wiring/libraryScan detail, the e2e harness, and secondary lint/typecheck traps not in `.claude/rules/frontend.md`.
