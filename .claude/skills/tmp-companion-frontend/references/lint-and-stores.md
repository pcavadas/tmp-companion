# Strict lint/tsc traps + the module-scoped device stores

## Lint & typecheck traps (the full catalog)

`bun run lint` (eslint `--max-warnings 0`) + `bunx tsc --noEmit` are the strict checks. The flat config is the STRICTEST typescript-eslint presets — `strictTypeChecked` + `stylisticTypeChecked` (type-aware via `parserOptions.projectService`) + `eslint-plugin-react` (recommended + jsx-runtime; `react/prop-types` + `react-in-jsx-scope` are off, superseded by TS / the new JSX transform).

**NO escape hatches anywhere in `src/`.** No `eslint-disable` / `@ts-nocheck` / `@ts-ignore` / `@ts-expect-error`, no `any` / `as any`, no non-null `!`. Fix the CODE/TYPES — and verify a strip/edit actually applied before trusting a 0-count (a botched in-place edit once silently no-op'd and faked "0 tsc errors"). Common strict findings + their code fix:

- **`react-hooks/refs`** ERRORS on reading/writing `ref.current` during render — **sync the ref in an EFFECT (after commit), not during render.** The old "this `useRef` read is a false positive — keep it in React state" guidance is SUPERSEDED.
- **`react-hooks/set-state-in-effect`** ERRORS on a synchronous `setState` in an effect — use the **"adjust state during render when an input changes"** prev-compare pattern (`const [prev,setPrev]=useState(x); if (x!==prev){setPrev(x); …}`), or derive the value during render (no state at all). Timers/ref-writes stay in the effect; only the `setState` moves.
- **Legit exception — DOM measurement:** a `useLayoutEffect` that measures the COMMITTED DOM (`getBoundingClientRect`/`getBBox`) then `setState`s the measurement is the one case the `setState` must stay in the effect (the node isn't laid out yet); guard it with a prev-value compare (`setX(p => close(p, next) ? p : next)`) so it converges. Instances: `SignalChainView`'s measured `SplitGroup` brackets, `ui/BlockArt.tsx` `HalfStackArt`, `overlays/Pick.tsx`, `settings/TargetRow.tsx`.
- **`no-unnecessary-condition`** reads the **INITIALIZER** type, not a widening annotation — to express "a `Record`/array index may be absent" use a genuine `T | undefined` (Partial-cast the map / length-guard or ternary the array) or model optionality in the TYPE itself (e.g. `profile_by_slot: Partial<Record<number,string>>`). Don't keep a redundant `??` — the rule is telling you the type is a lie.
- **`no-unnecessary-condition` via aliased narrowing** — TS narrows the OPERANDS of a derived boolean alias downstream, so re-testing one is "always falsy": `const blocked = !edit || …; if (blocked || !edit) return;` errors on the trailing `!edit`. **Fix: drop the redundant operand** (do NOT add a `?.` or recheck).
- **`optionalArr?.length > 0` does NOT compile** (`number | undefined > 0` is a type error) — write `arr && arr.length > 0`, not the `?.` short form.
- **`Array.prototype.at()` is absent** from the ES2020 lib target — use a bounds-guarded `[i]` index (`calls[calls.length - 1]`).
- **`restrict-template-expressions`** (allowNumber:false) → `String(x)` for numbers in template strings; **`no-confusing-void-expression`** → brace-wrap void-returning arrow handlers (`onClick={() => { f(); }}`); **`no-misused-promises`** → wrap async handlers `() => { void asyncFn(); }`; **`react-refresh/only-export-components`** → move the **MINORITY** export to a sibling file (a component-less file isn't a refresh boundary).
- **React hooks must precede any conditional early return** — a violation ("Rendered more hooks than during the previous render") once blanked the whole window on the first `error→ready` transition. Hoist the hooks; the top-level + per-tab `ErrorBoundary` is the backstop, not the fix.
- **`bunx tsc --noEmit`** also catches what the Vite build won't (the build transpiles without typechecking).
- **TypeScript 6** no longer resolves `node:` imports from `@types/node` alone — `tsconfig.json` needs `"types": ["node"]`.
- The IDE/LSP emits **stale phantom diagnostics during rapid file moves** — `bunx tsc --noEmit` from the CLI is authoritative; trust it over live editor squiggles mid-refactor.
- **Fresh checkout:** `node_modules` and `dist/` are gitignored. Run `bun install` (else hundreds of phantom "Cannot find module 'react'" errors) and `bun run build` (else the Rust `tauri-build` `generate_context!` panics on a missing `frontendDist`) before the checks.

## Shared device data: the `libraryScan` store (App-owned, ONE scan/connection)

Some device data is too expensive to read per-tab: the whole preset library — scenes, blocks, signal graphs, levelable footswitches (`footswitchesPerIndex`, consumed by the Level wizard's third dispatch), AND the song↔preset map — arrives in ONE ~22 s device backup (`read_library_via_backup` → `BackupReadResult`, decoded from `normalDb.db3`). It lives in a **module-scoped store**, `src/views/level/libraryScan.ts` (`subscribeLibraryScan` / `getLibraryScan` + `useSyncExternalStore`), consumed by **Level, Copy, and Songs**.

- The scan TRIGGER is **App-owned**: `App.tsx` fires `ensureLibraryScan()` once on the connect edge and `resetLibraryScan()` on detach — so every device tab shares ONE scan and a tab switch NEVER re-triggers it.
- A new tab that needs backup-sourced data **CONSUMES the store** (`const lib = useSyncExternalStore(subscribeLibraryScan, getLibraryScan)`); it does NOT add its own `ensureLibraryScan` trigger — that re-introduces the per-tab-rescan bug this layout exists to prevent.
- Extend the store by adding a field to `BackupReadResult` (+ the Rust parse + the `types.ts` mirror) and deriving the shape you need inside `ensureLibraryScan`, keyed by 0-based list index (device slot − 1) — never a second device read.

The SAME module-store pattern (not component `useState`) backs `src/views/level/useLiveDevice.ts` — the app-global LIVE device state (active preset/scene/graph from the 5 `tmp://` monitor events). It must be module-scoped because a LevelView tab-switch REMOUNTS the hook: a component-local snapshot would reset to INITIAL and, since the monitor only pushes on a CHANGE, the hero would revert to the stale connect-time preset (a real bug this fixed). Consequences:

- The hero SLOT badge reads the frontend `activeListIndex` (live-preset event), NOT `graph.slot` (the field-3 graph push carries no slot).
- The store exports a TEST-ONLY `resetLiveDevice()` (tears down + re-arms the event bridge) — call it in `beforeEach`; prod never remounts the bridge. See `references/testing.md` for the seeding rules an isolated view test needs.
