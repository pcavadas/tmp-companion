# Testing companion views

Frontend tests are **Vitest + React Testing Library** in a **jsdom** environment. `vitest.config.ts`:

```ts
export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    pool: "vmThreads",
    globals: true,
    setupFiles: ["./src/__tests__/setup.ts"],
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
  },
});
```

Run with `bun run test` (`vitest run`). Tests match `src/**/*.{test,spec}.{ts,tsx}` (Vitest's `include`); most live in `src/__tests__/`. `pool: "vmThreads"` builds one jsdom per worker but still gives each file a fresh VM context (module graph and globals), so file-level state (`forceTauri()`, a module store) never leaks between files; it halved the suite's run time.

**Name a test file after the unit it tests** (`SetupPage.labels.test.tsx`, `CopyView.test.tsx`), never after a bug or a past component name. A bug's regression lock goes in that unit's file as `describe("regression: BUG-n …")`.

## The global invoke mock (`src/__tests__/setup.ts`)

Every screen calls Tauri commands on mount, so `setup.ts` mocks `@tauri-apps/api/core` once, globally:

```ts
vi.mock("@tauri-apps/api/core", () => {
  class MockChannel {
    onmessage: ((message: unknown) => void) | null = null;
  }
  return {
    invoke: vi.fn((command: string, args?: unknown) =>
      resolveCommand({}, command, args),
    ),
    Channel: MockChannel,
  };
});
```

`resolveCommand` (`src/__tests__/commandMocks.ts`) answers each command from `DEFAULT_HANDLERS`, its **empty** shape (e.g. `list_presets → []`, `get_store → makeStore()`). `DEFAULT_HANDLERS` is typed `Handlers` — one handler per `Commands` key, each returning `Commands[K]["ret"]` — so a wrong default is a `tsc` error and a new command can't be left without one. A command outside the registry (a plugin's own IPC) resolves `null`. setup.ts also shims `localStorage` (jsdom lacks it), mocks `@tauri-apps/api/event`'s `listen` into a listener registry, and stubs `SVGElement.getBBox` (see Gotchas).

## The test kit

| Helper                                           | Where             | What it does                                                                                                                                                                                                                     |
| ------------------------------------------------ | ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `renderWithTheme(ui)`                            | `testKit.tsx`     | `render` inside `ThemeProvider` (as RTL's `wrapper`, so `rerender(ui)` keeps it)                                                                                                                                                 |
| `toRgb(light.x)`                                 | `testKit.tsx`     | a hex token as jsdom reports it in `element.style` — assert colours by token, not value                                                                                                                                          |
| `mockCommands`, `callsOf`, `fired`, `rejectWith` | `commandMocks.ts` | typed per-test command overrides and call reads; `rejectWith(reason)` rejects with `reason` unchanged — pass a bare string to mimic a backend `Err`                                                                              |
| `make*()`                                        | `commandMocks.ts` | complete typed shapes: command returns (`makeStore`, `makeLevelResult`, `makeBackupRead`, …) and the Level wizard's inputs (`makeSetupOption`, `makeSceneOption`, `makeFootswitchOption`, `makeChoices`, `makeLevelingFlowDeps`) |
| `withoutAmpCandidates(row)`                      | `commandMocks.ts` | a backup row with no `amp_candidates` key, so a scene run takes the live `list_level_blocks` fallback                                                                                                                            |
| `mockLiveEvents()`                               | `commandMocks.ts` | the captured `listen` callbacks, cleared before each test; push with `act(() => { live.get("tmp://live-preset")?.({ payload }) })`                                                                                               |
| `forceTauri()`                                   | `commandMocks.ts` | makes `isTauri()` true (sets `window.__TAURI_INTERNALS__`) so the live bridges and the library scan run; returns the undo, so `beforeEach(forceTauri)` scopes it to one `describe`                                               |

Never `vi.mock("../lib/invoke")` to force `isTauri`, nor re-mock `@tauri-apps/api/event` per file: both are in the kit. A hook test that drives commands (`useLevelingFlow-*.test.ts`) goes through `mockCommands` too, so the real wrappers and their `Channel`s run — stream progress with `args.onResult.onmessage(item)` inside the handler.

## Rendering a view

`useTheme()`/`useStyles()` throw outside the provider, so render through the kit:

```tsx
import { renderWithTheme } from "./testKit";

function renderView() {
  return renderWithTheme(<SettingsView connected={false} />);
}
```

## Feeding real data + asserting a write

Override the global mock per-test with `mockCommands`, returning real shapes for the commands under test; every other command falls through to its default. Handlers get the command's typed args and must return its typed result (or a promise of it; a `throw` rejects like a backend `Err`), and the `make*()` factories build complete shapes:

```ts
import { mockCommands, makeStore } from "./commandMocks";

function mockStore(targets = SEED_TARGETS, playback: PlaybackLevel = "stage") {
  mockCommands({
    get_store: () => makeStore({ targets, playback_level: playback }),
  });
}

beforeEach(() => {
  vi.mocked(invoke).mockReset();
  mockStore();
});
```

Assert that a command fired with the right payload by reading the recorded calls — `callsOf(command)` returns each call's args, typed:

```ts
const lastArgs = <K extends CommandName>(command: K) => {
  const calls = callsOf(command);
  return calls.length ? calls[calls.length - 1] : undefined;
};

it("persists the picked level via set_playback_level", async () => {
  const user = userEvent.setup();
  renderView();
  await screen.findByText("Rhythm");
  await user.click(within(group).getByRole("radio", { name: "Rehearsal" }));
  await waitFor(() =>
    expect(lastArgs("set_playback_level")).toEqual({ level: "rehearsal" }),
  );
});
```

This is how you verify the camelCase-top-level / snake_case-nested arg contract end to end: the payload object you assert is exactly what the wrapper passed to `invoke`.

## Gotchas

- **Fake timers only where nothing awaits `findBy*`/`waitFor`.** RTL's `waitFor`/`findBy` detect fake timers via the `jest` global and then run their own poll interval **and** their own timeout on the frozen clock — so they hang forever. A pure timeout (e.g. `useAutoAdvance` in `RunPieces.test.tsx`) takes `vi.useFakeTimers()` + `act(() => { vi.advanceTimersByTime(ms) })` and `vi.useRealTimers()` in `afterEach`; never sleep on the real clock. A flow that awaits the DOM stays on real timers, and short mock delays are fine to wait out with `findBy*`/`waitFor`.
- **Prefer async `findBy*`** for anything that appears after a mount fetch (`await screen.findByText("Rhythm")`), then synchronous `getBy*` for siblings already present.
- **`.at(-1)` may not typecheck** depending on the lib target — index with `calls[calls.length - 1]` instead.
- A passing test is NOT implied by a green `tsc`/build — `bun run test` is its own step. `invoke.test.ts` pins every wrapper's call shape, and its last test checks the wrappers reach exactly the `Commands` registry — see `SKILL.md`'s "Wiring a Tauri command into the UI" section.
- Tests render under jsdom — no real layout, so geometry-driven code (e.g. a slider reading `getBoundingClientRect()`) returns zeros; assert on values/roles/text, not pixel positions. **Sharper case: jsdom has no `SVGGraphicsElement.getBBox`, so a component that MEASURES SVG geometry in a layout effect — notably `HalfStackArt` (it sizes head-over-cab via `getBBox`) — would THROW on `render()` (not zeros).** `setup.ts` stubs it for every file (a fixed 72×100 box); don't re-stub it per file. It also stubs `Element.scrollIntoView` (a no-op: Set up's footer link scrolls a row into view). Assert on `resolveBlockArt`-resolved data (glyph/tone/pairing) when the geometry itself is the question.
- **Full-wizard (Level/Copy) test traps** (driving disclaimer→setup→run→summary, or choose→place→save, end to end): **(a)** a `level_preset` call's `job`/`LevelJob` payload has **snake_case** nested keys (`target_lufs`, `ref_level`) — assert `job.target_lufs`, not `targetLufs` (the camelCase-top / snake_case-nested contract; `buildLevelJob` in `leveling.ts` is the source). **(b)** `CopyView` and the **Songs** Presets axis both reuse **`device/libraryScan`** — there is NO per-view `libraryScan`; mocking/resetting the wrong module path leaves the consuming view rendering empty (no error). **(c)** Block-tile captions run through `normalizeShort` (`models/blockArt.ts`), so a tile renders e.g. `"65 TWN"`, not `"65TWN"` — query the normalized caption; and preset names appear in BOTH the Copy-from and Copy-to lists, so use `findAllByText` + positional select, not `findByText`.
- **Level's flow pages (Set up, the run, the Summary) sit over a list that stays mounted, hidden** (`display: none`, so its scroll, filter and choices survive). Role queries skip the hidden list; `getByText` does NOT — a sound name ("Main sound", "Rhythm") matches the hidden list too. The run page's rows carry `data-run-row` (`setupRowHookKey`) and `data-run-row-key`; the Summary's `data-summary-row`; scope text queries to those or to the Summary's sections (`getByRole("region", { name: "Need you" })`). Query Set up by role, by its `Select` hooks (`setupPageTestUtils.tsx`: `selectTrigger("knob:p0")`, `choose(user, tid, value)`), or scoped to `[data-setup-row]`.
- **The `libraryScan` scan is App-owned, so an isolated view test must SEED it.** Because `App.tsx` (not the view) fires `ensureLibraryScan()` on the connect edge, a view rendered ALONE in a test (`<LevelView/>` / `<CopyView/>` / `<SongsView/>`, no `<App>`) never starts the scan — any `findBy*`/`waitFor` on scan-derived data (scenes, blocks, the Songs Presets axis) HANGS with no error. The render helper must do App's job: `void ensureLibraryScan()` AFTER `render()` (when `connected`), with `read_library_via_backup` mocked to a COMPLETE `BackupReadResult` (build it with `makeBackupRead({ presets: [makeBackupPresetRow({...})] })` through `mockCommands`, so `tsc` catches a missing field — an untyped mock that omits one, e.g. a new `footswitches: []` per preset row (the case that broke 5+ mocks this last add) or `song_presets`, throws inside the store and is swallowed → empty render), plus `resetLibraryScan()` in `beforeEach` for module-state hygiene. `LevelView.test.tsx` / `SongsView.test.tsx` are the worked examples. **`useLiveDevice` is the SAME story** — it's a module store too, so `beforeEach` must ALSO call its test-only `resetLiveDevice()` (state reset + event-bridge teardown). `mockLiveEvents()` clears its listener registry per case, so the live bridge must be re-armable; without the reset the store leaks live state across cases and the re-mounted bridge has no listeners. Emit a live push in a test via the captured listener (`listeners.get("tmp://signal-chain")?.({ payload: graph })`) inside `act(...)`, with `forceTauri()` on so the bridge subscribed at all.
- **Asserting "the parent passed prop X to a child component" — mock the child and capture props.** When the bug is a dropped/wrong prop (not visible text or geometry), `vi.mock("../ui/BlockArt", () => ({ BlockArt: (p) => { captured.push(p); return null; }, HalfStackArt: () => null }))`, render the parent, then assert `captured.some(p => p.footswitch === "plate")`. This covers the whole adapter→child path with NO brittle SVG-geometry assertion and NO production test-seam, and the red is HONEST — the captured props genuinely lack the field before the fix (vs asserting a `data-*` attribute that doesn't exist yet, which fails on absence not on the real defect). `src/__tests__/SignalChainArtFields.test.tsx` is the worked example (it proved the signal-chain strip feeds `BlockArt` the full art prop set). Mocking `BlockArt` also means no real SVG renders, so the `getBBox` stub never matters.

## Worked reference

`src/__tests__/SettingsView.test.tsx` is a compact, current example: seed-data render, an "add row → rename mode → assert `save_targets` payload" flow, an empty-state assertion, and the two playback-level cases above. Read it before writing a new view test.
