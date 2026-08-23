# Secondary frontend gotchas

## No-fabricate rule: the literal-count corollary

**Size repeated UI off real data, never a literal count**: any component that renders one element per enum/family variant must take its count/labels from a data field (e.g. `DoctorSoundResult.bandLabels`), not a hardcoded number. BandMeter/BandSpark once hardcoded 6 bars and silently mis-rendered when the 7-band Bass VI family arrived — no test catches a wrong literal count structurally.

## Design-handoff step detail

Trailing detail for `SKILL.md`'s "When you're handed a Claude-Design handoff" numbered steps (the step lead + number stay in the body; this is the rest, moved out for space). Step 3 (the Severity-token trap) is kept whole in the body and isn't repeated here.

**Step 1** (Read the whole handoff first) continues: (the spec usually has a checklist or a file manifest). Handoffs bundle several changes; the easy failure is shipping the loud one and silently dropping a quiet one (a removed sub-feature, a renamed prop, the _correct_ exported icon variant). Diff each shipped asset against the handoff's final export.

**Step 2** (Reconcile the design against the codebase before coding) continues: — diff the current implementation against the spec first to find the delta, and prefer extending an existing shared component backward-compatibly over re-rolling a parallel one (e.g. the Copy refinement extended the Level page's `SignalChainView` with optional interactive props rather than keeping Copy's own renderer). Prototypes are also built in isolation and often omit features that are already wired (e.g. a handoff for the Settings page that forgets the existing Playback-level control). When the design and the live app conflict, that's a question for the user, not a call to make silently — they generally prefer fixing the source handoff over you guessing.

**Step 4** (One component per file) continues: Pedro consistently prefers small modules over barrels-of-many; propose the split proactively.

**Step 5** (Catalog-tab handoffs and the test oracle) continues: The catalog is the committed `src/models/tmp-model-guide.json`. Changing a row's `form`/`category`/glyph-source breaks `models-catalog.test.tsx`'s count oracles — update them in the same change. (The Catalog tab is `CatalogView`; its data lives in `src/models/`, whose folder + the `models-catalog.test.tsx` filename kept the `models` domain name.) (Prototype data shapes also don't always map 1:1 onto prod's normalized model — diff each shipped deliverable against the handoff's final export.)

## Wiring a Tauri command: when the command doesn't exist yet

If a command you need doesn't exist yet, that's a backend change (Rust `lib.rs` `generate_handler!` + an engine function) — coordinate it; don't fake the data on the frontend. Conversely, before _assuming_ a backend command is missing, grep the existing seams first — `src-tauri/src/session.rs` (e.g. `replace_node`/`insert_node`/`remove_node`/`extract_active_graph`), `proto.rs` (often already golden-tested), and the `probe` subcommands. The Copy feature's whole save path was a thin Tauri wrapper because the live structural-edit primitives were already RE'd + present (`probe --insert-active`); per-preset data was likewise already on the one `read_library_via_backup`/`BackupPresetRow` backup (one added field, no new device read).

## The `libraryScan` store: full contents + extending it

Some device data is too expensive to read per-tab: the whole preset library — scenes, blocks, signal graphs, levelable footswitches (`footswitchesPerIndex`, consumed by the Level wizard's third dispatch), AND the song↔preset map — arrives in ONE ~22 s device backup (`read_library_via_backup` → `BackupReadResult`, decoded from `normalDb.db3`). It lives in a **module-scoped store**, `src/views/level/libraryScan.ts` (`subscribeLibraryScan` / `getLibraryScan` + `useSyncExternalStore`), consumed by **Level, Copy, and Songs**. The scan TRIGGER is **App-owned**: `App.tsx` fires `ensureLibraryScan()` once on the connect edge and `resetLibraryScan()` on detach — so every device tab shares ONE scan and a tab switch NEVER re-triggers it. A new tab that needs backup-sourced data **CONSUMES the store** (`const lib = useSyncExternalStore(subscribeLibraryScan, getLibraryScan)`); it does NOT add its own `ensureLibraryScan` trigger — that re-introduces the per-tab-rescan bug this layout exists to prevent. Extend the store by adding a field to `BackupReadResult` (+ the `lib.rs` parse + the `types.ts` mirror) and deriving the shape you need inside `ensureLibraryScan`, keyed by 0-based list index (device slot − 1) — never a second device read. **That index is a scan-local join key, not preset identity**: it is valid only within the one scan that produced it, and a reorder or save shifts it. Persistent identity is `presetJson.info.preset_id` (`tmp-companion-data-model`). `useCopyLibrary` consumes `graphByIndex` positionally, so anything that outlives a single scan must resolve through `preset_id`.

## The Playwright e2e harness

**Full UI journeys** (connect → navigate → edit → save) are covered above Vitest by the dual-mode Playwright e2e harness in `e2e/`: the specs in `e2e/specs/` (the four core journeys `copy`/`doctor`/`level`/`songs` plus bug→gate regression specs like `pedal-fiasco` and `level.online` — enumerate with `ls e2e/specs/`, never from memory) drive the REAL React app in headless Chromium → an HTTP bridge → a windowless Rust backend (`tauri::test::mock_builder`) → `SimDevice` offline or the real device online. Run via the turn-key wrapper `bun run e2e` (offline, SimDevice, default, no hardware) / `bun run e2e online` (real device — handshake-preflight + a device-recovery trap), which wraps `e2e/playwright{,.online}.config.ts`; don't hand-invoke `playwright test` directly (you'd skip the stale-`:7600` guard + the online recovery). Reach for Vitest for component/logic coverage; reach for the harness when a change spans the click→invoke→device round trip.

## The `useLiveDevice` module store

The SAME module-store pattern (not component `useState`) backs `src/views/level/useLiveDevice.ts` — the app-global LIVE device state (active preset/scene/graph from the 5 `tmp://` monitor events). It must be module-scoped because a LevelView tab-switch REMOUNTS the hook: a component-local snapshot would reset to INITIAL and, since the monitor only pushes on a CHANGE, the hero would revert to the stale connect-time preset (a real bug this fixed). Consequences: the hero SLOT badge reads the frontend `activeListIndex` (live-preset event), NOT `graph.slot` (the field-3 graph push carries no slot); and the store exports a TEST-ONLY `resetLiveDevice()` (tears down + re-arms the event bridge) — call it in `beforeEach` since the event-mock clears its listener registry per case (prod never remounts the bridge, so it never tears down).

## Secondary lint & typecheck traps

`.claude/rules/frontend.md` already covers the no-escape-hatch ban, `no-unnecessary-condition` on a Record/array index, `optionalArr?.length > 0`, `Array.prototype.at()`, and the DOM-measurement `setState`-in-effect exception — this file only has what that one doesn't:

- Verify a strip/edit actually applied before trusting a 0-count (a botched in-place edit once silently no-op'd and faked "0 tsc errors").
- The flat config's `eslint-plugin-react` runs recommended + jsx-runtime; `react/prop-types` + `react-in-jsx-scope` are off, superseded by TS / the new JSX transform.
- **`no-unnecessary-condition` via aliased narrowing** — TS narrows the OPERANDS of a derived boolean alias downstream, so re-testing one is "always falsy": `const blocked = !edit || …; if (blocked || !edit) return;` errors on the trailing `!edit` (once `blocked` is false, TS knows `edit` is non-null). **Fix: drop the redundant operand** — the alias already covers it (do NOT add a `?.` or recheck). Same root cause as the Record-index variant: the rule is reading TS's real narrowed type.
- **`restrict-template-expressions`** (allowNumber:false) → `String(x)` for numbers in template strings; **`no-confusing-void-expression`** → brace-wrap void-returning arrow handlers (`onClick={() => { f(); }}`); **`no-misused-promises`** → wrap async handlers `() => { void asyncFn(); }`; **`react-refresh/only-export-components`** → move the **MINORITY** export to a sibling file (a component-less file isn't a refresh boundary).
- **`bunx tsc --noEmit`** also catches what the Vite build won't (the build transpiles without typechecking).
- **Fresh worktree needs deps before checks** (CLAUDE.md, "Traps that fire when you run something")
  — `bun install` before typecheck/lint/test/build, `bun run build` before any `cargo` gate.
- **TypeScript 6** no longer resolves `node:` imports from `@types/node` alone — `tsconfig.json` needs `"types": ["node"]`.
- The IDE/LSP emits **stale phantom diagnostics during rapid file moves or while another editor/agent rewrites a file concurrently** — `bunx tsc --noEmit` from the CLI is authoritative; trust it over live editor squiggles mid-refactor. A concurrent save can also silently REVERT a tool write with no error — after a multi-file edit, re-grep a distinctive symbol to confirm each write LANDED.
