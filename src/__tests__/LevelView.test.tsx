// Presets view (scene-tree selection) — connection gating, crash regression,
// and the unified leveling WIZARD (setup → run → summary in one frame).
//
//   • The view must not load until `connected`, and must load on the rising edge.
//   • The error→ready transition (Try again) must not crash (hook-count regression).
//   • Clicking a row selects the preset (NOT recall); the checkbox selects it too.
//   • Selecting + "Level N presets…" opens the wizard directly at Set up; the footer's
//     inline backup acknowledgment gates the commit, then run auto-advances to the
//     summary on a natural finish.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ThemeProvider } from "../theme/ThemeProvider";

// Capture event listeners (the backup-progress + live-sync subscriptions) so the
// view's useLiveDevice / usePresetData take the real listen() path under isTauri().
const listeners = new Map<string, (e: { payload: unknown }) => void>();
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, cb: (e: { payload: unknown }) => void) => {
    listeners.set(name, cb);
    return Promise.resolve(() => listeners.delete(name));
  },
}));
vi.mock("../lib/invoke", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/invoke")>();
  return { ...actual, isTauri: () => true };
});

// Imported AFTER the mocks so the view picks up the forced isTauri().
import { LevelView } from "../views/level";
import {
  ensureLibraryScan,
  resetLibraryScan,
} from "../views/level/libraryScan";
import { resetLiveDevice } from "../views/level/useLiveDevice";
import type { ActiveGraph } from "../lib/types";

function renderView(connected: boolean) {
  const r = render(
    <ThemeProvider>
      <LevelView connected={connected} />
    </ThemeProvider>,
  );
  // The backup scan is App-owned in production; seed it here when connected so the
  // view's scene/block data populates (the view itself no longer triggers the scan).
  if (connected) void ensureLibraryScan();
  return r;
}

// Tick the inline backup acknowledgment in the Set-up footer — it gates the
// "Level N sounds" button (there is no separate Back-up step).
async function ackBackup(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByText(/backed up with Pro Control/i));
}

const listPresetsCalls = () =>
  vi.mocked(invoke).mock.calls.filter((c) => c[0] === "list_presets").length;

// One non-empty, scene-less preset; the backup read returns its (empty) scenes so the
// row settles to "base only" and selecting it yields a single "Whole preset" sound.
function mockOnePreset() {
  vi.mocked(invoke).mockImplementation((command: string) => {
    switch (command) {
      case "list_presets":
        return Promise.resolve([{ slot: 0, name: "Studio Clean" }]);
      case "get_store":
        return Promise.resolve({
          profiles: [],
          profile_by_slot: {},
          targets: [{ name: "Rhythm", lufs: -26 }],
        });
      case "read_library_via_backup":
        return Promise.resolve({
          members: [],
          db_bytes: 0,
          total_rows: 1,
          scene_mode: "test",
          // device slot 1 = list index 0; scene-less → "Whole preset".
          presets: [
            {
              slot: 1,
              name: "Studio Clean",
              scene_count: 0,
              scenes: [],
              blocks: [],
              footswitches: [],
            },
          ],
          song_presets: [],
          songs: [],
          setlists: [],
          setlist_songs: [],
        });
      default:
        return Promise.resolve(null);
    }
  });
}

describe("LevelView — gating + crash regression", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    listeners.clear();
    resetLibraryScan();
    resetLiveDevice();
  });

  it("does not load while disconnected, then loads once on the rising edge", async () => {
    const { rerender } = renderView(false);
    expect(listPresetsCalls()).toBe(0);
    rerender(
      <ThemeProvider>
        <LevelView connected={true} />
      </ThemeProvider>,
    );
    await waitFor(() => {
      expect(listPresetsCalls()).toBe(1);
    });
  });

  it("survives the error→ready transition via Try again", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("device offline"));
    renderView(true);
    const user = userEvent.setup();
    expect(await screen.findByText("device offline")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /try again/i }));
    await waitFor(() =>
      expect(screen.queryByText("device offline")).not.toBeInTheDocument(),
    );
  });
});

describe("LevelView — list + leveling entry", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    listeners.clear();
    resetLibraryScan();
    resetLiveDevice();
    mockOnePreset();
  });

  it("renders the preset list after connect", async () => {
    renderView(true);
    expect(await screen.findByText("Studio Clean")).toBeInTheDocument();
  });

  it("row click selects the preset and does NOT recall it on the unit", async () => {
    renderView(true);
    const user = userEvent.setup();
    await user.click(await screen.findByText("Studio Clean"));
    // Selecting enables the Level button…
    expect(
      await screen.findByRole("button", { name: /level 1 preset/i }),
    ).toBeInTheDocument();
    // …and the list never touches the unit (no preset recall).
    expect(
      vi.mocked(invoke).mock.calls.some((c) => c[0] === "load_preset_on_amp"),
    ).toBe(false);
  });

  it("selecting + Level opens the wizard at Set up (no Back-up step)", async () => {
    renderView(true);
    const user = userEvent.setup();
    await screen.findByText("Studio Clean");
    // A scene-less, footswitch-less preset shows no child meta — the Level button only
    // appears once `ready` (findByRole below waits for it), so no extra ready-anchor.
    await user.click(screen.getAllByTitle("Select preset to level")[0]);
    await user.click(
      await screen.findByRole("button", { name: /level 1 preset/i }),
    );
    // The wizard opens directly at the Set-up step — there is no separate Back-up step.
    expect(
      await screen.findByText("Set instrument & target"),
    ).toBeInTheDocument();
    expect(screen.queryByText("Back up your unit first")).toBeNull();
    expect(screen.getByText("Whole preset")).toBeInTheDocument();
    // The backup acknowledgment is an inline gate in the footer.
    expect(screen.getByText(/backed up with Pro Control/i)).toBeInTheDocument();
  });
});

// ── Full leveling-flow e2e (device mocked) ─────────────────────────────────────
// Walks the whole wizard — select in the list → setup → run → summary —
// deterministically, asserting each stage + that the right device commands fire.

function levelResultStub(over: Record<string, unknown> = {}) {
  return {
    slot: 0,
    ref_level: 0.5,
    measured_lufs: -13.0,
    constant_c: -13.0,
    final_level: 0.18,
    target_lufs: -22,
    predicted_lufs: -22,
    clamped: false,
    saved: true,
    verify_lufs: -22,
    iterations: 1,
    ...over,
  };
}

// FootswitchLevelResult shape (distinct from LevelResult — carries `method`).
function footswitchResultStub(over: Record<string, unknown> = {}) {
  return {
    switch: 4,
    measured_lufs: -13.0,
    final_value: 0.4,
    target_lufs: -22,
    predicted_lufs: -22,
    clamped: false,
    clamp_reason: null,
    saved: true,
    verify_lufs: -22,
    iterations: 1,
    dynamic_spread_lu: null,
    method: "baked",
    ...over,
  };
}

const SOLO_FOOTSWITCH = {
  switch: 4, // → tag FS5
  label: "Solo",
  link_group: null,
  functions: [],
  level_params: [
    {
      group_id: "amp",
      node_id: "fs0",
      fender_id: "ACD_BluesDriver",
      parameter_id: "gain",
      current: 0.5,
    },
  ],
};

// One preset "Plexi" with two FS scenes (and, when opts.footswitch, one block-acting
// footswitch); the backup read supplies its children and the level commands are stubbed.
function mockLevelingFixture(
  opts: {
    clamped?: boolean;
    spreadLu?: number;
    verifyByEar?: boolean;
    footswitch?: boolean;
    /** Put an envelope filter in the preset's block roster (the "envelope" cause). */
    envelopeBlock?: boolean;
  } = {},
) {
  vi.mocked(invoke).mockImplementation((command: string, args?: unknown) => {
    switch (command) {
      case "list_presets":
        return Promise.resolve([{ slot: 0, name: "Plexi" }]);
      case "get_store":
        return Promise.resolve({
          profiles: [
            {
              id: "tele",
              name: "Telecaster",
              topology_id: "sc",
              calibration_lufs: -22,
            },
          ],
          profile_by_slot: {},
          targets: [{ name: "Rhythm", lufs: -26 }],
        });
      case "read_library_via_backup":
        return Promise.resolve({
          members: [],
          db_bytes: 0,
          total_rows: 1,
          scene_mode: "test",
          presets: [
            {
              slot: 1, // device slot 1 = list index 0
              name: "Plexi",
              scene_count: 2,
              scenes: [
                { name: "Rhythm", fs: 1 },
                { name: "Lead", fs: 2 },
              ],
              blocks: opts.envelopeBlock
                ? [
                    {
                      group_id: "pedals",
                      node_id: "env0",
                      fender_id: "ACD_MicroTronIV",
                    },
                  ]
                : [],
              footswitches: opts.footswitch ? [SOLO_FOOTSWITCH] : [],
            },
          ],
          song_presets: [],
          songs: [],
          setlists: [],
          setlist_songs: [],
        });
      case "level_footswitches_apply": {
        const job = args as {
          jobs: { switch: number }[];
          onResult?: { onmessage?: (item: unknown) => void };
        };
        const result = footswitchResultStub();
        job.jobs.forEach((j) =>
          job.onResult?.onmessage?.({
            switch: j.switch,
            status: "done",
            result,
            message: null,
          }),
        );
        return Promise.resolve(job.jobs.map(() => result));
      }
      case "level_preset":
        return Promise.resolve(
          levelResultStub({
            clamped: !!opts.clamped,
            dynamic_spread_lu: opts.spreadLu ?? null,
            verify_by_ear: !!opts.verifyByEar,
          }),
        );
      case "list_level_blocks":
        return Promise.resolve([
          {
            group_id: "amp",
            node_id: "amp0",
            model_id: "ACD_TweedDeluxe", // a real amp bid → passes ampCandidates
            parameter_id: "outputLevel",
            value: 0.5,
          },
        ]);
      case "level_scenes_apply_batched": {
        const job = args as {
          sceneSlots: number[];
          onResult?: { onmessage?: (item: unknown) => void };
        };
        const results = job.sceneSlots.map(() => levelResultStub());
        job.sceneSlots.forEach((sceneSlot, i) =>
          job.onResult?.onmessage?.({
            sceneSlot,
            status: "done",
            result: results[i],
            message: null,
          }),
        );
        return Promise.resolve(results);
      }
      default:
        return Promise.resolve(null);
    }
  });
}

// Expand Plexi's caret and tick just its Base scene → setup with 1 sound.
async function selectBaseOnly(user: ReturnType<typeof userEvent.setup>) {
  await screen.findByText("Plexi");
  await screen.findByText("2 scenes"); // 2 FS scenes (count excludes Base), backup ready
  await user.click(screen.getByTitle("Show Base + scenes"));
  await user.click(await screen.findByText("Base")); // toggles the Base key only
}

// Tick the WHOLE Plexi preset (checkbox) → Base + both FS scenes selected.
async function selectWholePreset(user: ReturnType<typeof userEvent.setup>) {
  await screen.findByText("Plexi");
  await screen.findByText("2 scenes");
  await user.click(screen.getAllByTitle("Select preset to level")[0]);
}

const fired = (command: string) =>
  vi.mocked(invoke).mock.calls.some((c) => c[0] === command);

describe("LevelView — hero recovers from a graphless connect", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    listeners.clear();
    resetLibraryScan();
    resetLiveDevice();
  });

  // Repro: the connect-time handshake returned graph=none (App.initialGraph=null),
  // and the device is idle so no tmp://signal-chain push arrives after mount. The
  // hero must NOT stay stuck on "No active preset" — it self-heals by reading the
  // monitor's already-current cached graph via current_graph.
  it("seeds the hero from current_graph when initialGraph is null and no live push arrives", async () => {
    vi.mocked(invoke).mockImplementation((command: string) => {
      switch (command) {
        // The list name differs from the hero name so the assertion can only
        // match the hero (not a preset row).
        case "list_presets":
          return Promise.resolve([{ slot: 0, name: "Studio Clean" }]);
        case "get_store":
          return Promise.resolve({
            profiles: [],
            profile_by_slot: {},
            targets: [{ name: "Rhythm", lufs: -26 }],
          });
        case "current_graph":
          return Promise.resolve({
            name: "Klon Live",
            slot: 0,
            template: "gtrSeries",
            split_mix: null,
            nodes: [],
            stages: [
              {
                kind: "series",
                blocks: [
                  {
                    group_id: "G1",
                    node_id: "n0",
                    model: "ACD_Klon",
                    bypassed: false,
                  },
                ],
              },
            ],
          });
        default:
          return Promise.resolve(null);
      }
    });

    render(
      <ThemeProvider>
        <LevelView connected={true} initialGraph={null} />
      </ThemeProvider>,
    );

    // The hero recovers: the active preset's name appears and the empty state is gone.
    expect(await screen.findByText("Klon Live")).toBeInTheDocument();
    expect(screen.queryByText("No active preset")).toBeNull();
  });
});

describe("LevelView — full leveling wizard e2e", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    listeners.clear();
    resetLibraryScan();
    resetLiveDevice();
    localStorage.clear();
  });

  it("setup → run auto-advances to summary", async () => {
    mockLevelingFixture();
    renderView(true);
    const user = userEvent.setup();
    await selectBaseOnly(user);
    await user.click(
      await screen.findByRole("button", { name: /level 1 preset/i }),
    );
    // The wizard opens directly at Set up; the footer ack gates the commit.
    expect(
      await screen.findByText("Set instrument & target"),
    ).toBeInTheDocument();
    await ackBackup(user);
    await user.click(screen.getByRole("button", { name: /level 1 sound/i }));
    // The run finishes on its own and auto-advances to the summary (no Continue click).
    expect(
      await screen.findByText("All 1 sound leveled", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();
    expect(fired("level_preset")).toBe(true);
  });

  it("Base-only run → summary → Done clears selection", async () => {
    mockLevelingFixture();
    renderView(true);
    const user = userEvent.setup();
    await selectBaseOnly(user);
    await user.click(
      await screen.findByRole("button", { name: /level 1 preset/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: /level 1 sound/i }),
    );
    expect(
      await screen.findByText("All 1 sound leveled", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();
    expect(fired("level_preset")).toBe(true);
    await user.click(screen.getByRole("button", { name: /done/i }));
    expect(
      await screen.findByText(/select presets to level/i),
    ).toBeInTheDocument();
  });

  it("whole-preset run uses level_preset + list_level_blocks + level_scenes_apply_batched", async () => {
    mockLevelingFixture();
    renderView(true);
    const user = userEvent.setup();
    await selectWholePreset(user);
    await user.click(
      await screen.findByRole("button", { name: /level 1 preset/i }),
    );
    await ackBackup(user);
    // Base + Rhythm + Lead → "Level 3 sounds".
    await user.click(
      await screen.findByRole("button", { name: /level 3 sounds/i }),
    );
    expect(
      await screen.findByText("All 3 sounds leveled", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();
    expect(fired("level_preset")).toBe(true); // Base
    expect(fired("list_level_blocks")).toBe(true); // FS amp discovery
    expect(fired("level_scenes_apply_batched")).toBe(true); // FS scene level
  });

  it("re-leveling the same preset re-issues device commands (no stale cross-run cache)", async () => {
    // Staleness invariant: after a run writes new levels the connect-time backup is
    // stale for those slots, so a second run on the SAME preset must re-issue the
    // device commands (the backend re-measures live) — results are never cached
    // across runs. Guards against a future "cache the LevelResult" optimization.
    mockLevelingFixture();
    renderView(true);
    const user = userEvent.setup();

    // First run.
    await selectWholePreset(user);
    await user.click(
      await screen.findByRole("button", { name: /level 1 preset/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: /level 3 sounds/i }),
    );
    expect(
      await screen.findByText("All 3 sounds leveled", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /done/i }));

    // Forget the first run's calls; the second run must re-issue them.
    vi.mocked(invoke).mockClear();

    await selectWholePreset(user);
    await user.click(
      await screen.findByRole("button", { name: /level 1 preset/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: /level 3 sounds/i }),
    );
    expect(
      await screen.findByText("All 3 sounds leveled", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();
    expect(fired("level_preset")).toBe(true); // Base re-measured live
    expect(fired("level_scenes_apply_batched")).toBe(true); // FS scenes re-measured live
  });

  it("large dynamics spread → summary flags the sound 'by ear'", async () => {
    mockLevelingFixture({ spreadLu: 8.2 }); // ≥ DYNAMIC_SPREAD_LU
    renderView(true);
    const user = userEvent.setup();
    await selectBaseOnly(user);
    await user.click(
      await screen.findByRole("button", { name: /level 1 preset/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: /level 1 sound/i }),
    );
    // The headline stays green (dynamic ≠ failure) but the row + reason-aware footnote flag it.
    expect(
      await screen.findByText("All 1 sound leveled", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();
    expect(screen.getAllByText("by ear").length).toBeGreaterThan(0);
    expect(
      screen.getByText(
        /worth a quick listen — loud\/quiet swings make the number an average\./i,
      ),
    ).toBeInTheDocument();
  });

  it("rebalanced result → summary footnote names the rebalance reason", async () => {
    // spread below threshold + verify_by_ear true → the byEarCause resolves to "rebalance",
    // so the reason-aware footnote spells out the parallel-amp cause (not the dynamic one).
    mockLevelingFixture({ spreadLu: 1.1, verifyByEar: true });
    renderView(true);
    const user = userEvent.setup();
    await selectBaseOnly(user);
    await user.click(
      await screen.findByRole("button", { name: /level 1 preset/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: /level 1 sound/i }),
    );
    expect(
      await screen.findByText("All 1 sound leveled", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        /worth a quick listen — parallel amps balanced by approximate isolation\./i,
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText(/loud\/quiet swings/i)).toBeNull();
  });

  it("envelope-filter preset → 'by ear' with the envelope reason, beating dynamic", async () => {
    // High spread AND an envelope block: the envelope cause must WIN (it questions the
    // measurement itself), so the footnote names it and the dynamic reason is absent.
    mockLevelingFixture({ spreadLu: 8.2, envelopeBlock: true });
    renderView(true);
    const user = userEvent.setup();
    await selectBaseOnly(user);
    await user.click(
      await screen.findByRole("button", { name: /level 1 preset/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: /level 1 sound/i }),
    );
    expect(
      await screen.findByText("All 1 sound leveled", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();
    expect(screen.getAllByText("by ear").length).toBeGreaterThan(0);
    expect(
      screen.getByText(
        /worth a quick listen — an envelope filter responds to the test signal differently than to real playing\./i,
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText(/loud\/quiet swings/i)).toBeNull();
  });

  it("small dynamics spread → no 'by ear' marker in the summary", async () => {
    mockLevelingFixture({ spreadLu: 1.1 });
    renderView(true);
    const user = userEvent.setup();
    await selectBaseOnly(user);
    await user.click(
      await screen.findByRole("button", { name: /level 1 preset/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: /level 1 sound/i }),
    );
    expect(
      await screen.findByText("All 1 sound leveled", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();
    expect(screen.queryByText("by ear")).toBeNull();
  });

  it("clamped result → 'Re-level clamped…' reopens setup in re-level mode", async () => {
    mockLevelingFixture({ clamped: true });
    renderView(true);
    const user = userEvent.setup();
    await selectBaseOnly(user);
    await user.click(
      await screen.findByRole("button", { name: /level 1 preset/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: /level 1 sound/i }),
    );
    const relevel = await screen.findByRole(
      "button",
      { name: /re-level clamped/i },
      { timeout: 3000 },
    );
    await user.click(relevel);
    expect(
      await screen.findByText("Re-level — set instrument & target"),
    ).toBeInTheDocument();
  });

  it("footswitch run dispatches level_footswitches_apply (not level_preset) and summarizes it leveled", async () => {
    mockLevelingFixture({ footswitch: true });
    renderView(true);
    const user = userEvent.setup();
    await screen.findByText("Plexi");
    // Collapsed meta shows the scene + footswitch breakdown.
    await screen.findByText("2 scenes · 1 footswitch");
    await user.click(screen.getByTitle("Show Base + scenes"));
    // Tick ONLY the footswitch row (its own `f` key), not Base/scenes.
    await user.click(await screen.findByText("Solo"));
    await user.click(
      await screen.findByRole("button", { name: /level 1 preset/i }),
    );
    await ackBackup(user);
    expect(
      await screen.findByText("Set instrument & target"),
    ).toBeInTheDocument();
    // Setup carries the footswitch-specific sub-text (never "scene" / "preset" copy).
    expect(
      screen.getByText("evens this footswitch out to your target"),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /level 1 sound/i }));
    expect(
      await screen.findByText("All 1 sound leveled", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();
    // The footswitch lane fired; a footswitch-only run never levels the preset base.
    expect(fired("level_footswitches_apply")).toBe(true);
    expect(fired("level_preset")).toBe(false);
    // The bake/assign `method` is never surfaced to the user.
    expect(screen.queryByText(/baked/i)).toBeNull();
    expect(screen.queryByText(/assigned/i)).toBeNull();
  });

  // ── BUG-3: a Stop pressed during the LAST item must mark the run stopped ──────
  // The last item was already in flight when Stop was pressed, so the top-of-loop
  // cancel check never sees it; the final publish must still read the cancel flag and
  // report "stopped" (Continue, no auto-advance), not mislabel a finished run.
  it("Stop during the last (only) item marks the run stopped, not finished (BUG-3)", async () => {
    let resolveLevel!: () => void;
    const gate = new Promise<void>((res) => {
      resolveLevel = res;
    });
    vi.mocked(invoke).mockImplementation((command: string) => {
      switch (command) {
        case "list_presets":
          return Promise.resolve([{ slot: 0, name: "Studio Clean" }]);
        case "get_store":
          return Promise.resolve({
            profiles: [],
            profile_by_slot: {},
            targets: [{ name: "Rhythm", lufs: -26 }],
          });
        case "read_library_via_backup":
          return Promise.resolve({
            members: [],
            db_bytes: 0,
            total_rows: 1,
            scene_mode: "test",
            presets: [
              {
                slot: 1,
                name: "Studio Clean",
                scene_count: 0,
                scenes: [],
                blocks: [],
                footswitches: [],
              },
            ],
            song_presets: [],
            songs: [],
            setlists: [],
            setlist_songs: [],
          });
        // The only item's leveling stays pending until the test resolves the gate.
        case "level_preset":
          return gate.then(() => levelResultStub());
        default:
          return Promise.resolve(null);
      }
    });
    renderView(true);
    const user = userEvent.setup();
    await screen.findByText("Studio Clean");
    await user.click(screen.getAllByTitle("Select preset to level")[0]);
    await user.click(
      await screen.findByRole("button", { name: /level 1 preset/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: /level 1 sound/i }),
    );
    // The run is active on the only item; level_preset is still pending — press Stop.
    await user.click(await screen.findByRole("button", { name: /^cancel$/i }));
    await user.click(await screen.findByRole("button", { name: /^stop$/i }));
    // Now let the in-flight item finish. The loop exits and the final publish must
    // report STOPPED (Continue), because the cancel flag was set mid-item.
    resolveLevel();
    expect(await screen.findByText("Leveling stopped")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /continue/i }),
    ).toBeInTheDocument();
    // It did NOT auto-advance to the summary.
    expect(screen.queryByText(/sound leveled/i)).toBeNull();
  });

  // ── BUG-4: closing the wizard after a run deselects the leveled sounds ────────
  // Re-level → Cancel used to leave the whole selection ticked, so the next Level
  // re-ran everything. Closing after a run (Done OR Cancel) now prunes exactly the
  // keys it leveled, accumulating across re-level rounds.
  it("Re-level → Cancel deselects the already-leveled sounds (BUG-4)", async () => {
    mockLevelingFixture({ clamped: true });
    renderView(true);
    const user = userEvent.setup();
    await selectWholePreset(user); // Base + 2 FS scenes selected
    await user.click(
      await screen.findByRole("button", { name: /level 1 preset/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: /level 3 sounds/i }),
    );
    // Base clamped → "Re-level clamped…" reopens setup in re-level mode.
    const relevel = await screen.findByRole(
      "button",
      { name: /re-level clamped/i },
      { timeout: 3000 },
    );
    await user.click(relevel);
    await screen.findByText("Re-level — set instrument & target");
    // Cancel out of the re-level setup — the leveled sounds must be deselected, so the
    // list returns to the empty prompt (NOT still offering "Level 1 preset…").
    await user.click(screen.getByRole("button", { name: /^cancel$/i }));
    expect(
      await screen.findByText(/select presets to level/i),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /level 1 preset/i }),
    ).toBeNull();
  });

  // ── Partial-failure: one item errors mid-run → skipped, the run CONTINUES ──────
  // A device failure on one sound must not abort the whole run; it's marked skipped
  // and the remaining sounds still level (data-loss-family regression).
  it("a mid-run item failure is skipped and the run continues to the rest", async () => {
    vi.mocked(invoke).mockImplementation((command: string, args?: unknown) => {
      switch (command) {
        case "list_presets":
          return Promise.resolve([{ slot: 0, name: "Plexi" }]);
        case "get_store":
          return Promise.resolve({
            profiles: [],
            profile_by_slot: {},
            targets: [{ name: "Rhythm", lufs: -26 }],
          });
        case "read_library_via_backup":
          return Promise.resolve({
            members: [],
            db_bytes: 0,
            total_rows: 1,
            scene_mode: "test",
            presets: [
              {
                slot: 1,
                name: "Plexi",
                scene_count: 2,
                scenes: [
                  { name: "Rhythm", fs: 1 },
                  { name: "Lead", fs: 2 },
                ],
                blocks: [],
                footswitches: [],
              },
            ],
            song_presets: [],
            songs: [],
            setlists: [],
            setlist_songs: [],
          });
        // The Base sound fails; the two FS scenes still level.
        case "level_preset":
          return Promise.reject(new Error("device dropped the level"));
        case "list_level_blocks":
          return Promise.resolve([
            {
              group_id: "amp",
              node_id: "amp0",
              model_id: "ACD_TweedDeluxe",
              parameter_id: "outputLevel",
              value: 0.5,
            },
          ]);
        case "level_scenes_apply_batched": {
          const job = args as { sceneSlots: number[] };
          return Promise.resolve(job.sceneSlots.map(() => levelResultStub()));
        }
        default:
          return Promise.resolve(null);
      }
    });
    renderView(true);
    const user = userEvent.setup();
    await selectWholePreset(user);
    await user.click(
      await screen.findByRole("button", { name: /level 1 preset/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: /level 3 sounds/i }),
    );
    // The run finished (auto-advanced to the summary) despite the Base failure: the
    // summary has a "Skipped" group, and the FS scenes still ran afterwards.
    expect(
      await screen.findByText("Skipped", undefined, { timeout: 3000 }),
    ).toBeInTheDocument();
    expect(fired("level_scenes_apply_batched")).toBe(true);
  });
});

// ── Tab-switch hero persistence ────────────────────────────────────────────────
// Repro of the reported bug: a live preset change updates the hero, but switching
// tabs (LevelView unmount) and back (remount) reverted it to the STALE connect-time
// graph — because the live state was component-local and reset on remount, leaving
// the hero to re-seed from `initialGraph`. The live store now persists across mounts.
describe("LevelView — hero survives a tab-switch remount after a live change", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    listeners.clear();
    resetLibraryScan();
    resetLiveDevice();
  });

  const heroGraph = (
    name: string,
    slot: number,
    model: string,
  ): ActiveGraph => ({
    name,
    slot,
    template: "gtrSeries",
    split_mix: null,
    nodes: [],
    stages: [
      {
        kind: "series",
        blocks: [
          { group_id: "G1", node_id: "n0", model, bypassed: false, params: {} },
        ],
      },
    ],
  });

  it("keeps the live preset on remount instead of reverting to the connect-time seed", async () => {
    // List names are distinct from the hero names, so a name assertion can only match
    // the hero (never a preset row).
    vi.mocked(invoke).mockImplementation((command: string) => {
      switch (command) {
        case "list_presets":
          return Promise.resolve([
            { slot: 0, name: "Row One" },
            { slot: 2, name: "Row Three" },
          ]);
        case "get_store":
          return Promise.resolve({
            profiles: [],
            profile_by_slot: {},
            targets: [{ name: "Rhythm", lufs: -26 }],
          });
        default:
          return Promise.resolve(null);
      }
    });

    // Connect-time handshake graph = the (soon-stale) preset 003 "Cello".
    const seed = heroGraph("Cello", 2, "ACD_HiwattDR103CanModCabIR");
    const ui = (
      <ThemeProvider>
        <LevelView connected={true} initialGraph={seed} />
      </ThemeProvider>
    );
    const { unmount } = render(ui);
    expect(await screen.findByText("Cello")).toBeInTheDocument();

    // The unit is switched to preset 001 "Guitar" — a live signal-chain push arrives.
    await waitFor(() => {
      expect(listeners.has("tmp://signal-chain")).toBe(true);
    });
    act(() => {
      listeners.get("tmp://signal-chain")?.({
        payload: heroGraph("Guitar", 0, "ACD_KlonCentaur"),
      });
    });
    expect(await screen.findByText("Guitar")).toBeInTheDocument();

    // Tab away + back = unmount + remount with the SAME stale connect-time seed.
    unmount();
    render(ui);

    // The hero stays on the unit's CURRENT preset (001 Guitar), never reverts to 003.
    expect(await screen.findByText("Guitar")).toBeInTheDocument();
    expect(screen.queryByText("Cello")).toBeNull();
  });
});
