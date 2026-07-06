# Testing companion views

Frontend tests are **Vitest + React Testing Library** in a **jsdom** environment. `vitest.config.ts`:

```ts
export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/__tests__/setup.ts"],
  },
});
```

Run with `bun run test` (`vitest run`). Tests match `src/**/*.{test,spec}.{ts,tsx}` (Vitest's `include`); most live in `src/__tests__/`.

## The global invoke mock (`src/__tests__/setup.ts`)

Every screen calls Tauri commands on mount, so `setup.ts` mocks `@tauri-apps/api/core` once, globally:

```ts
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn((command: string) => Promise.resolve(emptyResultFor(command))),
  Channel: MockChannel,
}));
```

`emptyResultFor(command)` is a big `switch` returning a sensible **empty** shape per command (e.g. `list_presets → []`, `get_store → { profiles: [], profile_by_slot: {}, targets: [] }`). This lets any view mount and render its empty state without a real backend. setup.ts also shims `localStorage` (jsdom lacks it) and a `MockChannel` (for streamed commands).

**When you add a new command** that a view calls on mount, add its empty shape to `emptyResultFor` — otherwise it resolves `null` and the view may throw.

## Rendering a view

`useTheme()`/`useStyles()` throw outside the provider, so always wrap:

```tsx
import { ThemeProvider } from "../theme/ThemeContext";

function renderView() {
  return render(
    <ThemeProvider>
      <SettingsView connected={false} />
    </ThemeProvider>,
  );
}
```

## Feeding real data + asserting a write

Override the global mock per-test with `mockImplementation`, returning real shapes for the commands under test and falling through for the rest:

```ts
import { invoke } from "@tauri-apps/api/core";

function mockStore(targets = SEED_TARGETS, playback = "stage") {
  vi.mocked(invoke).mockImplementation((command: string) => {
    if (command === "get_store")
      return Promise.resolve({
        profiles: [],
        profile_by_slot: {},
        targets,
        playback_level: playback,
      });
    if (command === "list_pickup_topologies") return Promise.resolve([]);
    return Promise.resolve(null);
  });
}

beforeEach(() => {
  vi.mocked(invoke).mockReset();
  mockStore();
});
```

Assert that a command fired with the right payload by reading the recorded calls:

```ts
const lastArgs = (command: string) => {
  const calls = vi.mocked(invoke).mock.calls.filter((c) => c[0] === command);
  return calls.length ? calls[calls.length - 1][1] : undefined;
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

- **Use REAL timers, never `vi.useFakeTimers()`.** RTL's `waitFor`/`findBy` detect fake timers via the `jest` global and then run their own poll interval **and** their own timeout on the frozen clock — so they hang forever. Let async resolve naturally; `T_LOAD`-scale delays are fine to wait out with `findBy*`/`waitFor` on real timers.
- **Prefer async `findBy*`** for anything that appears after a mount fetch (`await screen.findByText("Rhythm")`), then synchronous `getBy*` for siblings already present.
- **`.at(-1)` may not typecheck** depending on the lib target — index with `calls[calls.length - 1]` instead.
- A passing test is NOT implied by a green `tsc`/build — `bun run test` is its own step. And `invoke.test.ts` asserts the **exact** wrapper count (`Object.keys(cmd).length`) with a history comment; bump it when you add a wrapper to the `cmd` namespace — but named-export-only wrappers (e.g. the leveling cancel lane `cancel{Preset,Scene,Footswitch}Leveling`) live OUTSIDE `cmd`, don't move the count, and are tested with their own `expectCall`.
- Tests render under jsdom — no real layout, so geometry-driven code (e.g. a slider reading `getBoundingClientRect()`) returns zeros; assert on values/roles/text, not pixel positions. **Sharper case: jsdom has no `SVGGraphicsElement.getBBox`, so a component that MEASURES SVG geometry in a layout effect — notably `HalfStackArt` (it sizes head-over-cab via `getBBox`) — THROWS on `render()` (not zeros).** Stub it in a `beforeAll` (`(SVGElement.prototype as unknown as { getBBox: () => DOMRect }).getBBox = () => ({ x: 0, y: 0, width: 72, height: 100 }) as DOMRect`), as `BlockArt.test.tsx` / `models-catalog.test.tsx` already do — or assert on `resolveBlockArt`-resolved data (glyph/tone/pairing) instead of rendering the half-stack.
- **Full-wizard (Level/Copy) test traps** (driving disclaimer→setup→run→summary, or choose→place→save, end to end): **(a)** a `level_preset` call's `job`/`LevelJob` payload has **snake_case** nested keys (`target_lufs`, `ref_level`) — assert `job.target_lufs`, not `targetLufs` (the camelCase-top / snake_case-nested contract; `buildLevelJob` in `leveling.ts` is the source). **(b)** `CopyView` and the **Songs** Presets axis both reuse **`views/level/libraryScan`** — there is NO `views/copy/libraryScan`; mocking/resetting the wrong module path leaves the consuming view rendering empty (no error). **(c)** Block-tile captions run through `normalizeShort` (`models/blockArt.ts`), so a tile renders e.g. `"65 TWN"`, not `"65TWN"` — query the normalized caption; and preset names appear in BOTH the Copy-from and Copy-to lists, so use `findAllByText` + positional select, not `findByText`.
- **The `libraryScan` scan is App-owned, so an isolated view test must SEED it.** Because `App.tsx` (not the view) fires `ensureLibraryScan()` on the connect edge, a view rendered ALONE in a test (`<LevelView/>` / `<CopyView/>` / `<SongsView/>`, no `<App>`) never starts the scan — any `findBy*`/`waitFor` on scan-derived data (scenes, blocks, the Songs Presets axis) HANGS with no error. The render helper must do App's job: `void ensureLibraryScan()` AFTER `render()` (when `connected`), with `read_library_via_backup` mocked to a COMPLETE `BackupReadResult` (every field — an untyped mock that omits a field, e.g. a new `footswitches: []` per preset row (the case that broke 5+ mocks this last add) or `song_presets`, throws inside the store and is swallowed → empty render), plus `resetLibraryScan()` in `beforeEach` for module-state hygiene. `LevelView.test.tsx` / `SongsView.test.tsx` are the worked examples. **`useLiveDevice` is the SAME story** — it's a module store too, so `beforeEach` must ALSO call its test-only `resetLiveDevice()` (state reset + event-bridge teardown). The `@tauri-apps/api/event` mock clears its listener registry per case, so the live bridge must be re-armable; without the reset the store leaks live state across cases and the re-mounted bridge has no listeners. Emit a live push in a test via the captured listener (`listeners.get("tmp://signal-chain")?.({ payload: graph })`) inside `act(...)`.
- **Asserting "the parent passed prop X to a child component" — mock the child and capture props.** When the bug is a dropped/wrong prop (not visible text or geometry), `vi.mock("../ui/BlockArt", () => ({ BlockArt: (p) => { captured.push(p); return null; }, HalfStackArt: () => null }))`, render the parent, then assert `captured.some(p => p.footswitch === "plate")`. This covers the whole adapter→child path with NO brittle SVG-geometry assertion and NO production test-seam, and the red is HONEST — the captured props genuinely lack the field before the fix (vs asserting a `data-*` attribute that doesn't exist yet, which fails on absence not on the real defect). `src/__tests__/SignalChainArtFields.test.tsx` is the worked example (it proved the signal-chain strip feeds `BlockArt` the full art prop set). Mocking `BlockArt` also sidesteps the `getBBox` stub — no real SVG renders.

## Worked reference

`src/__tests__/SettingsView.test.tsx` is a compact, current example: seed-data render, an "add row → rename mode → assert `save_targets` payload" flow, an empty-state assertion, and the two playback-level cases above. Read it before writing a new view test.
