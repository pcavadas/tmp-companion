---
paths:
  # Listed separately rather than as "*.{ts,tsx}" — brace expansion is not portable
  # across glob matchers, and a rule whose pattern never matches never loads.
  - "src/**/*.ts"
  - "src/**/*.tsx"
---

# Frontend rules

Applies while editing anything under `src/`. Component-level how-to lives in the `tmp-companion-frontend` skill; these are the traps that bite silently.

## Lint and typecheck — no escape hatches

`bun run lint` is eslint `--max-warnings 0` over the strictest typescript-eslint presets (`strictTypeChecked` + `stylisticTypeChecked`, type-aware via `projectService`) plus `eslint-plugin-react`. **Nowhere in `src/`:** no `eslint-disable` / `@ts-nocheck` / `@ts-ignore` / `@ts-expect-error`, no `any` / `as any`, no non-null `!`. Fix findings by changing CODE, never by silencing.

- `no-unnecessary-condition` on a Record/array index is the no-`noUncheckedIndexedAccess` lie: express the real `T | undefined` (Partial-cast a map, length-guard an array, ternary). Don't keep a redundant `??`.
- `optionalArr?.length > 0` does NOT compile (`number | undefined > 0` is a type error) — write `arr && arr.length > 0`.
- `Array.prototype.at()` is absent from the ES2020 lib target (`tsc`: `Property 'at' does not exist`) — use a bounds-guarded `[i]` index.
- `react-hooks/refs` errors on reading/writing `ref.current` during render — sync such refs in an EFFECT. `set-state-in-effect` errors on a synchronous `setState` in an effect — use the "adjust state during render when an input changes" pattern (a `prev*` compare) or a render-phase derivation.
- **Legit exception — DOM measurement.** A `useLayoutEffect` that measures the COMMITTED DOM (`getBoundingClientRect`/`getBBox`) then `setState`s the measurement is the one case the `setState` must stay in the effect (the node isn't laid out yet). Guard it with a prev-value compare so it converges. Instances: `SignalChainView`'s `SplitGroup` brackets, `ui/BlockArt.tsx` `HalfStackArt`, `overlays/Pick.tsx`, `settings/TargetRow.tsx`.
- **React hooks must precede any conditional early return.** `LevelView.tsx` once declared `useMemo`s after the `loading`/`error` returns, giving "Rendered more hooks than during the previous render" on the first `error→ready` transition — with no ErrorBoundary that unmounted the whole tree to a **blank window**.

## Contract mirrors — these fail SILENTLY

Serialized Rust structs have hand-written TS mirrors in `src/lib/types.ts`. Adding a Rust field without updating the mirror fails with no error, because test mocks are untyped.

- `invoke.test.ts` asserts the EXACT `cmd` wrapper count (`Object.keys(cmd).length`, hardcoded) — extend it when you add/remove a wrapper **in the `cmd` namespace**. Some wrappers are deliberately named-export-only and OUTSIDE `cmd` (the fire-and-forget `cancel{Preset,Scene,Footswitch}Leveling` lane), so adding those does NOT move the count — cover them with their own `expectCall`.
- `liveEvents.test.ts` asserts the WHOLE `LIVE_EVENT` registry via an exact `toEqual` — adding a `tmp://` event (registry entry + `onXxx` wrapper + `types.ts` payload mirror) must extend that `toEqual` too.

## Platform and framework traps

- **`window.confirm()` / `window.alert()` silently no-op in Tauri's WKWebView** (no JS-dialog delegate) — `confirm` returns `false`, so never gate logic on them. Use inline UI or a countdown.
- **Dialogs/overlays go through the ONE DS `Dialog` (`ui/Dialog.tsx`)** — never roll a per-view `position:absolute` scrim. An `absolute, inset:0` backdrop resolves against the nearest positioned ancestor (the view body BELOW the 46px tab bar), so it fails to cover the menu and the flex-centered card crops. This was a real bug across all four dialogs. [→ evidence](../../notes/gotchas.md#dialogsoverlays-go-through-the-one-ds-dialog-uidialogtsx--never-roll-a-per-view-positionabsolute-scrim)
- **Tauri 2 `core:default` does NOT grant window creation.** Any future second `WebviewWindow` silently fails unless `core:webview:allow-create-webview-window` + `core:window:allow-{create,show,set-focus,close}` are re-added to `capabilities/default.json`.
- **Device-tab loads are connection-gated, not mount-only.** Views refresh on the `connected` flag flipping true, so plugging the TMP in after launch auto-populates the body.

## Shared device data

- There is ONE App-owned `libraryScan` backup scan per connection. Consumers **subscribe** via `useSyncExternalStore`; a new device tab consumes the store and **does not add its own trigger**.
- Live-device state is a MODULE-SCOPED store, not component `useState` — a tab-switch remount would reset component state to initial, and since the monitor only pushes on a CHANGE, the hero would revert to the stale connect-time preset. The hero slot badge reads the frontend `activeListIndex`, **not** `graph.slot` (field-3 carries no slot).
- **Row click is SELECTION only — app-driven preset recall was REMOVED.** Recall is owned by Pro Control and the footswitches.
- A written block copy is **not undoable from the app**; the done summary points at a Pro Control backup restore.
