// Level view (look B list, scene-tree selection) — connection gating, crash regression,
// choosing on the list, and the unified leveling WIZARD (setup → run → summary).
//
//   • The view must not load until `connected`, and must load on the rising edge.
//   • The error→ready transition (Try again) must not crash (hook-count regression).
//   • The checkbox chooses a preset; its name opens a preset with scenes and chooses any
//     other; choosing never recalls a preset on the unit.
//   • "Select all N shown" chooses only the presets the filter shows (a bug fix).
//   • The Level button is always there: greyed with its reason when nothing is chosen
//     or the scan still reads, else "Level N sounds" (it counts sounds).
//   • "Level N sounds" opens the flow directly at Set up; the footer's inline backup
//     acknowledgment gates the commit, then the run auto-advances to the Summary on a
//     natural finish, and to the Summary straight away after a Stop.
//   • The Summary says each outcome in plain words; a sound that can't reach its target
//     gets no button, and the way to a quieter target is the list again (the clamp loop).

import { describe, it, expect, vi, beforeEach } from "vitest";
import { screen, waitFor, act, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke, type InvokeArgs } from "@tauri-apps/api/core";

import { LevelView } from "../views/level";
import { ensureLibraryScan, resetLibraryScan } from "../device/libraryScan";
import { resetLiveDevice } from "../device/useLiveDevice";
import type {
  ActiveGraph,
  BackupPresetRow,
  BackupReadResult,
  FootswitchInfo,
  FootswitchLevelResult,
  LevelBlock,
  LevelResult,
  Store,
} from "../lib/types";
import { resetLevelFlow } from "../views/level/levelRunStore";
import {
  callsOf,
  fired,
  forceTauri,
  makeActiveGraph,
  makeBackupPresetRow,
  makeBackupRead,
  makeBatchRun,
  makeFootswitchLevelResult,
  makeLevelResult,
  makeStore,
  mockCommands,
  mockLiveEvents,
  rejectWith,
  withoutAmpCandidates,
  type Handler,
} from "./commandMocks";
import { choose } from "./setupPageTestUtils";
import { renderWithTheme } from "./testKit";

// In-app: the live bridges and the library scan subscribe.
forceTauri();

const listeners = mockLiveEvents();

// The leveling wizard lives in a module store that outlives a mount; start each case clean.
beforeEach(() => {
  resetLevelFlow();
});

function renderView(connected: boolean) {
  const r = renderWithTheme(<LevelView connected={connected} />);
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

const listPresetsCalls = () => callsOf("list_presets").length;

const RHYTHM_STORE = makeStore({ targets: [{ name: "Rhythm", lufs: -26 }] });

/** A scene-less preset at device slot 1 (list index 0). */
const STUDIO_CLEAN = makeBackupPresetRow({ slot: 1, name: "Studio Clean" });

// One non-empty, scene-less preset; the backup read returns its (empty) scenes so the
// row settles to "base only" and selecting it yields a single "Main sound".
function mockOnePreset() {
  mockCommands({
    list_presets: () => [{ slot: 0, name: "Studio Clean" }],
    get_store: () => RHYTHM_STORE,
    // device slot 1 = list index 0; scene-less → just its main sound.
    read_library_via_backup: () =>
      makeBackupRead({ total_rows: 1, presets: [STUDIO_CLEAN] }),
  });
}

describe("LevelView — gating + crash regression", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    resetLibraryScan();
    resetLiveDevice();
  });

  it("does not load while disconnected, then loads once on the rising edge", async () => {
    const { rerender } = renderView(false);
    expect(listPresetsCalls()).toBe(0);
    rerender(<LevelView connected={true} />);
    await waitFor(() => {
      expect(listPresetsCalls()).toBe(1);
    });
  });

  it("survives the error→ready transition via Try again", async () => {
    let offline = true; // the first list read fails; Try again succeeds
    mockCommands({
      list_presets: () => {
        if (!offline) return [];
        offline = false;
        throw new Error("device offline");
      },
    });
    renderView(true);
    const user = userEvent.setup();
    // The error page says it in plain words; the backend text stays in the log.
    expect(
      await screen.findByText(
        "Couldn't read your presets from your Tone Master Pro",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText("device offline")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /try again/i }));
    await waitFor(() =>
      expect(
        screen.queryByText(
          "Couldn't read your presets from your Tone Master Pro",
        ),
      ).not.toBeInTheDocument(),
    );
  });
});

describe("LevelView — list + leveling entry", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    resetLibraryScan();
    resetLiveDevice();
    mockOnePreset();
  });

  it("renders the preset list after connect", async () => {
    renderView(true);
    expect(await screen.findByText("Studio Clean")).toBeInTheDocument();
  });

  it("clicking a scene-less preset's name chooses it and does NOT recall it on the unit", async () => {
    renderView(true);
    const user = userEvent.setup();
    await user.click(await screen.findByText("Studio Clean"));
    // Selecting enables the Level button…
    expect(
      await screen.findByRole("button", { name: /^level 1 sound$/i }),
    ).toBeInTheDocument();
    // …and the list never touches the unit (no preset recall).
    expect(
      vi.mocked(invoke).mock.calls.some((c) => c[0] === "load_preset_on_amp"),
    ).toBe(false);
  });

  it("selecting + Level opens Set up as the page (no Back-up step)", async () => {
    renderView(true);
    const user = userEvent.setup();
    await screen.findByText("Studio Clean");
    // A scene-less, footswitch-less preset shows no child meta — the Level button only
    // appears once `ready` (findByRole below waits for it), so no extra ready-anchor.
    await user.click(
      screen.getByRole("checkbox", { name: "Choose Studio Clean" }),
    );
    await user.click(
      await screen.findByRole("button", { name: /^level 1 sound$/i }),
    );
    // Set up replaces the list in the tab (the list stays mounted, hidden) — there is no
    // separate Back-up step.
    expect(
      await screen.findByRole("heading", { name: /^Level \d+ sounds?$/ }),
    ).toBeInTheDocument();
    expect(screen.queryByText("Back up your unit first")).toBeNull();
    expect(screen.getByText("Main sound")).toBeInTheDocument();
    // The backup acknowledgment is an inline gate in the footer.
    expect(screen.getByText(/backed up with Pro Control/i)).toBeInTheDocument();
  });
});

// ── Choosing on the list (look B) ─────────────────────────────────────────────

/** Three scene-less presets at device slots 1–3 (list indices 0–2). */
const THREE_PRESETS = ["Clean Room", "Crunch One", "Crunch Two"];
function mockThreePresets(
  readBackup?: () => Promise<BackupReadResult> | BackupReadResult,
) {
  mockCommands({
    list_presets: () => THREE_PRESETS.map((name, slot) => ({ slot, name })),
    get_store: () => RHYTHM_STORE,
    read_library_via_backup:
      readBackup ??
      (() =>
        makeBackupRead({
          total_rows: 3,
          presets: THREE_PRESETS.map((name, i) =>
            makeBackupPresetRow({ slot: i + 1, name }),
          ),
        })),
  });
}

describe("LevelView — choosing on the list", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    resetLibraryScan();
    resetLiveDevice();
  });

  // BUG→GATE (owner review, 2026-10-16): Select all ignored the filter and chose the whole
  // library, so leveling a bank meant ticking each preset by hand.
  it("Select all chooses only the presets the filter shows", async () => {
    mockThreePresets();
    renderView(true);
    const user = userEvent.setup();
    await screen.findByText("3 presets"); // the list is in
    await waitFor(() => {
      expect(screen.queryAllByText("Reading…")).toHaveLength(0);
    });
    await user.type(screen.getByLabelText("Filter by name or slot"), "crunch");
    await user.click(
      screen.getByRole("checkbox", { name: "Select all 2 shown" }),
    );
    // Both shown presets' sounds are chosen…
    expect(
      await screen.findByRole("button", { name: /^level 2 sounds$/i }),
    ).toBeEnabled();
    await user.clear(screen.getByLabelText("Filter by name or slot"));
    expect(
      screen.getByRole("checkbox", { name: "Choose Crunch One" }),
    ).toHaveAttribute("aria-checked", "true");
    expect(
      screen.getByRole("checkbox", { name: "Choose Crunch Two" }),
    ).toHaveAttribute("aria-checked", "true");
    // …and the hidden one is not.
    expect(
      screen.getByRole("checkbox", { name: "Choose Clean Room" }),
    ).toHaveAttribute("aria-checked", "false");
    // Unfiltered, the box is mixed, and a click there chooses the rest.
    const all = screen.getByRole("checkbox", { name: "Select all 3 shown" });
    expect(all).toHaveAttribute("aria-checked", "mixed");
    await user.click(all);
    expect(
      screen.getByRole("button", { name: /^level 3 sounds$/i }),
    ).toBeEnabled();
    // A filtered Select all over fully chosen presets unchooses only those.
    await user.type(screen.getByLabelText("Filter by name or slot"), "clean");
    await user.click(
      screen.getByRole("checkbox", { name: "Select all 1 shown" }),
    );
    expect(
      screen.getByRole("button", { name: /^level 2 sounds$/i }),
    ).toBeEnabled();
  });

  it("the Level button is always there, greyed with its reason until something is chosen", async () => {
    mockThreePresets();
    renderView(true);
    const user = userEvent.setup();
    await screen.findByText("3 presets");
    await waitFor(() => {
      expect(screen.queryAllByText("Reading…")).toHaveLength(0);
    });
    expect(
      screen.getByText("Choose presets or sounds to level"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^level$/i })).toBeDisabled();
    await user.click(
      screen.getByRole("checkbox", { name: "Choose Crunch One" }),
    );
    // It counts sounds, and names the presets beside it.
    expect(
      screen.getByRole("button", { name: /^level 1 sound$/i }),
    ).toBeEnabled();
    expect(screen.getByText("1 preset")).toBeInTheDocument();
    // No loudness numbers on the list: the old targets readout is gone.
    expect(screen.queryByText(/Targets/)).toBeNull();
  });

  it("while the scan reads, ticks wait and Level stays busy with its reason", async () => {
    let finish!: () => void;
    const gate = new Promise<void>((res) => {
      finish = res;
    });
    mockThreePresets(() =>
      gate.then(() =>
        makeBackupRead({
          total_rows: 3,
          presets: THREE_PRESETS.map((name, i) =>
            makeBackupPresetRow({ slot: i + 1, name }),
          ),
        }),
      ),
    );
    renderView(true);
    const user = userEvent.setup();
    await screen.findByText("Crunch One");
    expect(screen.getAllByText("Reading…")).toHaveLength(3);
    // A tick from the name chooses the whole preset while its sounds are unknown.
    await user.click(screen.getByRole("button", { name: "Crunch One" }));
    expect(
      screen.getByRole("checkbox", { name: "Choose Crunch One" }),
    ).toHaveAttribute("aria-checked", "true");
    expect(
      screen.getByText("You can start when reading finishes"),
    ).toBeInTheDocument();
    const busy = screen.getByRole("button", { name: /^level$/i });
    expect(busy).toBeDisabled();
    expect(busy).toHaveAttribute("aria-busy", "true");
    finish();
    expect(
      await screen.findByRole("button", { name: /^level 1 sound$/i }),
    ).toBeEnabled();
  });

  it("How it works opens the explainer; Done closes it", async () => {
    mockThreePresets();
    renderView(true);
    const user = userEvent.setup();
    await screen.findByText("3 presets");
    await user.click(screen.getByRole("button", { name: "How it works" }));
    const dialog = await screen.findByRole("dialog", {
      name: "How leveling works",
    });
    expect(dialog).toHaveTextContent("Which knob it turns");
    expect(dialog).toHaveTextContent("The preset's level");
    // No close (×) button: Done closes it.
    expect(screen.queryByRole("button", { name: "Close" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});

// ── Full leveling-flow e2e (device mocked) ─────────────────────────────────────
// Walks the whole wizard — select in the list → setup → run → summary —
// deterministically, asserting each stage + that the right device commands fire.

const levelResultStub = (over: Partial<LevelResult> = {}): LevelResult =>
  makeLevelResult({
    target_lufs: -22,
    predicted_lufs: -22,
    verify_lufs: -22,
    dynamic_spread_lu: null,
    ...over,
  });

// FootswitchLevelResult shape (distinct from LevelResult — carries `method`).
const footswitchResultStub = (
  over: Partial<FootswitchLevelResult> = {},
): FootswitchLevelResult =>
  makeFootswitchLevelResult({
    switch: 4,
    measured_lufs: -13.0,
    final_value: 0.4,
    target_lufs: -22,
    predicted_lufs: -22,
    verify_lufs: -22,
    ...over,
  });

/** The amp output knob the live `list_level_blocks` fallback finds. */
const TWEED_OUTPUT: LevelBlock = {
  group_id: "amp",
  node_id: "amp0",
  model_id: "ACD_TweedDeluxe", // a real amp bid → passes ampCandidates
  parameter_id: "outputLevel",
  value: 0.5,
};

/** A scene batch that streams "done" for every job, then returns their results. */
const levelScenesDone: Handler<"level_scenes_apply_batched"> = ({
  jobs,
  onResult,
}) => {
  const results = jobs.map(() => levelResultStub());
  jobs.forEach((j, i) => {
    onResult.onmessage({
      sceneSlot: j.sceneSlot,
      status: "done",
      result: results[i],
      message: null,
      skipKind: null,
    });
  });
  return makeBatchRun(results);
};

/** "Plexi" (device slot 1 = list index 0) with two FS scenes. No `amp_candidates`, so
 * a scene run takes the live `list_level_blocks` fallback. */
const plexiRow = (over: Partial<BackupPresetRow> = {}) =>
  withoutAmpCandidates(
    makeBackupPresetRow({
      slot: 1,
      name: "Plexi",
      scene_count: 2,
      scenes: [
        { name: "Rhythm", fs: 1 },
        { name: "Lead", fs: 2 },
      ],
      ...over,
    }),
  );

const SOLO_FOOTSWITCH: FootswitchInfo = {
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
      class: "level_linear",
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
    /** The main sound reaches no target louder than this (LUFS): it clamps at full
     *  volume there, and levels at any quieter one. */
    ceiling?: number;
  } = {},
) {
  mockCommands({
    list_presets: () => [{ slot: 0, name: "Plexi" }],
    get_store: () =>
      makeStore({
        profiles: [
          {
            id: "tele",
            name: "Telecaster",
            topology_id: "sc",
            calibration_lufs: -22,
          },
        ],
        // A louder target only for the clamp loop, so the scene named "Lead" keeps
        // Rhythm (the only target) everywhere else.
        targets:
          opts.ceiling != null
            ? [
                { name: "Rhythm", lufs: -26 },
                { name: "Lead", lufs: -19 },
              ]
            : [{ name: "Rhythm", lufs: -26 }],
      }),
    read_library_via_backup: () =>
      makeBackupRead({
        total_rows: 1,
        presets: [
          plexiRow({
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
          }),
        ],
      }),
    level_footswitches_apply: ({ jobs, onResult }) => {
      // Every row levels now (D2 — the backend removed the verify-only mode
      // entirely), so every job resolves the same way.
      jobs.forEach((j) => {
        onResult.onmessage({
          switch: j.switch,
          status: "done",
          result: footswitchResultStub(),
          message: null,
          skipKind: null,
        });
      });
      return makeBatchRun(jobs.map(() => footswitchResultStub()));
    },
    level_preset: ({ job }) => {
      const ceiling = opts.ceiling;
      const clamped =
        !!opts.clamped || (ceiling != null && job.target_lufs > ceiling);
      const reached = clamped ? (ceiling ?? -28) : job.target_lufs;
      return levelResultStub({
        target_lufs: job.target_lufs,
        predicted_lufs: reached,
        verify_lufs: reached,
        clamped,
        clamp_kind: clamped ? "scene_ceiling" : null,
        dynamic_spread_lu: opts.spreadLu ?? null,
        verify_by_ear: !!opts.verifyByEar,
      });
    },
    list_level_blocks: () => [TWEED_OUTPUT],
    level_scenes_apply_batched: levelScenesDone,
    // No candidates in these fixtures — the point is exercising the target-mode
    // pick, not the handle picker's candidate list (see SceneLevelPick.test.tsx).
    list_scene_level_handles: () => [
      { sceneSlot: 0, candidates: [], allCandidates: [] },
      { sceneSlot: 1, candidates: [], allCandidates: [] },
    ],
  });
}

// Open Plexi with its caret and tick just its main sound → setup with 1 sound.
async function selectBaseOnly(user: ReturnType<typeof userEvent.setup>) {
  await screen.findByText("Plexi");
  await screen.findByText("2 scenes"); // 2 FS scenes (count excludes Base), backup ready
  await user.click(screen.getByRole("button", { name: "Open Plexi" }));
  // Toggles the main sound's key only.
  await user.click(
    await screen.findByRole("checkbox", { name: "Choose Main sound in Plexi" }),
  );
}

// Tick the WHOLE Plexi preset (checkbox) → Base + both FS scenes selected.
async function selectWholePreset(user: ReturnType<typeof userEvent.setup>) {
  await screen.findByText("Plexi");
  await screen.findByText("2 scenes");
  await user.click(screen.getByRole("checkbox", { name: "Choose Plexi" }));
}

describe("LevelView — hero recovers from a graphless connect", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    resetLibraryScan();
    resetLiveDevice();
  });

  // Repro: the connect-time handshake returned graph=none (App.initialGraph=null),
  // and the device is idle so no tmp://signal-chain push arrives after mount. The
  // hero must NOT stay stuck on "No active preset" — it self-heals by reading the
  // monitor's already-current cached graph via current_graph.
  it("seeds the hero from current_graph when initialGraph is null and no live push arrives", async () => {
    mockCommands({
      // The list name differs from the hero name so the assertion can only
      // match the hero (not a preset row).
      list_presets: () => [{ slot: 0, name: "Studio Clean" }],
      get_store: () => RHYTHM_STORE,
      current_graph: () =>
        makeActiveGraph({
          name: "Klon Live",
          slot: 0,
          template: "gtrSeries",
          stages: [
            {
              kind: "series",
              blocks: [
                {
                  group_id: "G1",
                  node_id: "n0",
                  model: "ACD_Klon",
                  bypassed: false,
                  params: {},
                },
              ],
            },
          ],
        }),
    });

    renderWithTheme(<LevelView connected={true} initialGraph={null} />);

    // The hero recovers: the active preset's name appears and the empty state is gone.
    expect(await screen.findByText("Klon Live")).toBeInTheDocument();
    expect(screen.queryByText("No active preset")).toBeNull();
  });
});

describe("LevelView — full leveling wizard e2e", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
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
      await screen.findByRole("button", { name: /^level 1 sound$/i }),
    );
    // The wizard opens directly at Set up; the footer ack gates the commit.
    expect(
      await screen.findByRole("heading", { name: /^Level \d+ sounds?$/ }),
    ).toBeInTheDocument();
    await ackBackup(user);
    await user.click(screen.getByRole("button", { name: "Start leveling" }));
    // The run finishes on its own and auto-advances to the summary (no Continue click).
    expect(
      await screen.findByText("1 sound leveled.", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();
    expect(fired("level_preset")).toBe(true);
  });

  it("Set up → Cancel comes back to the same list: filter, open preset and choice kept", async () => {
    mockLevelingFixture();
    renderView(true);
    const user = userEvent.setup();
    await selectBaseOnly(user);
    await user.type(
      screen.getByPlaceholderText(/Filter by name or slot/i),
      "Ple",
    );
    await user.click(
      await screen.findByRole("button", { name: /^level 1 sound$/i }),
    );
    await screen.findByRole("heading", { name: "Level 1 sound" });
    // Set up is the page now: the list stays mounted underneath, hidden.
    expect(
      screen.queryByRole("button", { name: /^level 1 sound$/i }),
    ).toBeNull();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.getByPlaceholderText(/Filter by name or slot/i)).toHaveValue(
      "Ple",
    );
    expect(
      screen.getByRole("button", { name: "Close Plexi" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("checkbox", { name: "Choose Main sound in Plexi" }),
    ).toBeChecked();
  });

  it("an instrument chosen on Set up is saved for the preset, and Set up starts from it next time", async () => {
    mockLevelingFixture();
    renderView(true);
    const user = userEvent.setup();
    await selectBaseOnly(user);
    await user.click(
      await screen.findByRole("button", { name: /^level 1 sound$/i }),
    );
    const inst = () =>
      screen.getByRole("button", { name: /^Instrument for Plexi:/ });
    await user.click(inst());
    await user.click(screen.getByRole("option", { name: "None" }));
    // Saved in the app's own store (never on the unit), keyed by list index.
    await waitFor(() => {
      expect(callsOf("set_preset_instrument")).toEqual([
        { slot: 0, profileId: "none" },
      ]);
    });
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await user.click(
      await screen.findByRole("button", { name: /^level 1 sound$/i }),
    );
    expect(inst()).toHaveAccessibleName("Instrument for Plexi: None");
  });

  // A tab switch unmounts the Level view while the flow stays on Set up, so coming back
  // mounts Set up before the app's store has loaded again.
  it("Set up met again after a tab round trip waits for the store, then starts from the saved instrument", async () => {
    mockLevelingFixture();
    const first = renderView(true);
    const user = userEvent.setup();
    await selectBaseOnly(user);
    await user.click(
      await screen.findByRole("button", { name: /^level 1 sound$/i }),
    );
    await screen.findByRole("heading", { name: "Level 1 sound" });
    first.unmount();

    // Back on the Level tab: the store answers only when the test releases it.
    let release: (st: Store) => void = () => undefined;
    const pending = new Promise<Store>((resolve) => {
      release = resolve;
    });
    const fixture = vi.mocked(invoke).getMockImplementation();
    vi.mocked(invoke).mockImplementation(
      (command: string, args?: InvokeArgs) =>
        command === "get_store"
          ? pending
          : (fixture?.(command, args) ?? Promise.resolve(undefined)),
    );
    renderView(true);
    await screen.findByRole("heading", { name: "Level 1 sound" });
    await ackBackup(user);
    expect(
      screen.getByRole("button", { name: "Start leveling" }),
    ).toBeDisabled();
    expect(screen.queryByText("Add a target in Settings")).toBeNull();

    await act(async () => {
      release(
        makeStore({
          profiles: [
            {
              id: "strat",
              name: "Stratocaster",
              topology_id: "sc",
              calibration_lufs: -22,
            },
            {
              id: "tele",
              name: "Telecaster",
              topology_id: "sc",
              calibration_lufs: -22,
            },
          ],
          profile_by_slot: { 0: "tele" },
          targets: [{ name: "Rhythm", lufs: -26 }],
        }),
      );
      await pending;
    });
    expect(
      await screen.findByRole("button", {
        name: "Instrument for Plexi: Telecaster",
      }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Start leveling" }));
    await waitFor(() => {
      expect(callsOf("level_preset")).toHaveLength(1);
    });
    expect(callsOf("level_preset")[0].job.profile_id).toBe("tele");
  });

  it("Base-only run → summary → Done clears selection", async () => {
    mockLevelingFixture();
    renderView(true);
    const user = userEvent.setup();
    await selectBaseOnly(user);
    await user.click(
      await screen.findByRole("button", { name: /^level 1 sound$/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: "Start leveling" }),
    );
    expect(
      await screen.findByText("1 sound leveled.", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();
    expect(fired("level_preset")).toBe(true);
    await user.click(screen.getByRole("button", { name: "Done" }));
    expect(
      await screen.findByText("Choose presets or sounds to level"),
    ).toBeInTheDocument();
  });

  it("whole-preset run uses level_preset + list_level_blocks + level_scenes_apply_batched", async () => {
    mockLevelingFixture();
    renderView(true);
    const user = userEvent.setup();
    await selectWholePreset(user);
    await user.click(
      await screen.findByRole("button", { name: /^level 3 sounds$/i }),
    );
    await ackBackup(user);
    // Base + Rhythm + Lead → "Level 3 sounds".
    await user.click(
      await screen.findByRole("button", { name: "Start leveling" }),
    );
    expect(
      await screen.findByText("All 3 sounds leveled.", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();
    expect(fired("level_preset")).toBe(true); // Base
    expect(fired("list_level_blocks")).toBe(true); // FS amp discovery
    expect(fired("level_scenes_apply_batched")).toBe(true); // FS scene level
    // Adjacent same-preset scene rows BATCH into ONE backend call (one prepass, one
    // runner) REGARDLESS of per-scene target — each job carries its own targetLufs, so
    // a mixed-target preset no longer splits into several prepasses. Per-scene calls
    // re-loaded the preset each time, flashing the unit back to base twice per scene.
    const sceneCalls = callsOf("level_scenes_apply_batched");
    expect(sceneCalls).toHaveLength(1);
    const sceneJobs = sceneCalls[0].jobs;
    expect(sceneJobs.map((j) => j.sceneSlot)).toEqual([0, 1]);
    // Every job carries a resolved per-scene target (the store's single target here).
    expect(sceneJobs.map((j) => j.targetLufs)).toEqual([-26, -26]);
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
      await screen.findByRole("button", { name: /^level 3 sounds$/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: "Start leveling" }),
    );
    expect(
      await screen.findByText("All 3 sounds leveled.", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Done" }));

    // Forget the first run's calls; the second run must re-issue them.
    vi.mocked(invoke).mockClear();

    await selectWholePreset(user);
    await user.click(
      await screen.findByRole("button", { name: /^level 3 sounds$/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: "Start leveling" }),
    );
    expect(
      await screen.findByText("All 3 sounds leveled.", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();
    expect(fired("level_preset")).toBe(true); // Base re-measured live
    expect(fired("level_scenes_apply_batched")).toBe(true); // FS scenes re-measured live
  });

  // BUG→GATE (#241): a wide dynamics spread is not a measurement problem, so it no longer
  // flags the row "Check by ear".
  it("large dynamics spread → no 'Check by ear' on the Summary", async () => {
    mockLevelingFixture({ spreadLu: 8.2 });
    renderView(true);
    const user = userEvent.setup();
    await selectBaseOnly(user);
    await user.click(
      await screen.findByRole("button", { name: /^level 1 sound$/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: "Start leveling" }),
    );
    expect(
      await screen.findByText("1 sound leveled.", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();
    expect(screen.queryByText("Check by ear")).toBeNull();
    expect(screen.queryByText(/loud\/quiet swings/i)).toBeNull();
  });

  it("rebalanced result → 'Check by ear' with the two-amp reason", async () => {
    // verify_by_ear true → the byEarCause resolves to "rebalance".
    mockLevelingFixture({ spreadLu: 1.1, verifyByEar: true });
    renderView(true);
    const user = userEvent.setup();
    await selectBaseOnly(user);
    await user.click(
      await screen.findByRole("button", { name: /^level 1 sound$/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: "Start leveling" }),
    );
    expect(
      await screen.findByText("1 sound leveled.", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("Check by ear")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Its two amps were balanced roughly. Listen to check it.",
      ),
    ).toBeInTheDocument();
  });

  it("envelope-filter preset → 'Check by ear' with the envelope reason", async () => {
    // An envelope block questions the measurement itself, so the row says so.
    mockLevelingFixture({ spreadLu: 8.2, envelopeBlock: true });
    renderView(true);
    const user = userEvent.setup();
    await selectBaseOnly(user);
    await user.click(
      await screen.findByRole("button", { name: /^level 1 sound$/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: "Start leveling" }),
    );
    expect(
      await screen.findByText("1 sound leveled.", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("Check by ear")).toBeInTheDocument();
    expect(
      screen.getByText(
        "It has an envelope filter, which reacts to the test tone differently from your playing. Listen to check it.",
      ),
    ).toBeInTheDocument();
  });

  // BUG→GATE (the 2026-07 "clamp loop unresolvable" report), retargeted to look B: a sound
  // clamped at full volume says so and what to change, with NO button (running it again
  // can't help) and no Try again; the way to a quieter target is Done, the list and Set
  // up — and there it resolves. The old "Re-level clamped…" reopened Set up in a re-level mode that is gone.
  it("clamped at a loud target: says so with no button, then levels at a quieter one", async () => {
    mockLevelingFixture({ ceiling: -22 });
    renderView(true);
    const user = userEvent.setup();
    await selectBaseOnly(user);
    await user.click(
      await screen.findByRole("button", { name: /^level 1 sound$/i }),
    );
    await ackBackup(user);
    await choose(user, "target:p0", "Lead");
    await user.click(screen.getByRole("button", { name: "Start leveling" }));
    expect(
      await screen.findByText(
        "Already at full volume, so it can't reach Lead. Choose a quieter target for it.",
        undefined,
        { timeout: 3000 },
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", {
        name: "Nothing was leveled. 1 sound needs you.",
      }),
    ).toBeInTheDocument();
    for (const old of [/re-level/i, /level lower/i, /try again/i])
      expect(screen.queryByRole("button", { name: old })).toBeNull();

    // Done → the list (the clamped sound was resolved, so it is unchosen; Plexi is still
    // open) → choose it again → a quieter target → it levels.
    await user.click(screen.getByRole("button", { name: "Done" }));
    await user.click(
      await screen.findByRole("checkbox", {
        name: "Choose Main sound in Plexi",
      }),
    );
    await user.click(
      await screen.findByRole("button", { name: /^level 1 sound$/i }),
    );
    await ackBackup(user);
    await choose(user, "target:p0", "Rhythm");
    await user.click(screen.getByRole("button", { name: "Start leveling" }));
    expect(
      await screen.findByText("1 sound leveled.", undefined, { timeout: 3000 }),
    ).toBeInTheDocument();
    const jobs = callsOf("level_preset").map((c) => c.job.target_lufs);
    expect(jobs).toEqual([-19, -26]);
  });

  it("footswitch run dispatches level_footswitches_apply (not level_preset) and summarizes it leveled", async () => {
    mockLevelingFixture({ footswitch: true });
    renderView(true);
    const user = userEvent.setup();
    await screen.findByText("Plexi");
    // Collapsed meta shows the scene + footswitch breakdown.
    await screen.findByText("2 scenes, 1 footswitch");
    await user.click(screen.getByRole("button", { name: "Open Plexi" }));
    // Tick ONLY the footswitch row (its own `f` key), not Base/scenes.
    await user.click(await screen.findByText("Solo"));
    await user.click(
      await screen.findByRole("button", { name: /^level 1 sound$/i }),
    );
    await ackBackup(user);
    expect(
      await screen.findByRole("heading", { name: /^Level \d+ sounds?$/ }),
    ).toBeInTheDocument();
    // D2: every row levels now (the verify-only default + "Make level-neutral" opt-in
    // are gone) — the row already carries a real handle (SOLO_FOOTSWITCH's one
    // candidate, tone-safe-classified `level_linear`) with no picker interaction
    // required to start (design 1a dropped the per-row explanatory caption).
    expect(screen.getAllByText("Solo").length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: "Start leveling" }));
    expect(
      await screen.findByText("1 sound leveled.", undefined, {
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
  // report "stopped", not mislabel a finished run. Look B: a stopped run lands on the
  // Summary straight away, and the Summary says it stopped.
  it("Stop during the last (only) item marks the run stopped, not finished (BUG-3)", async () => {
    let resolveLevel!: () => void;
    const gate = new Promise<void>((res) => {
      resolveLevel = res;
    });
    mockCommands({
      list_presets: () => [{ slot: 0, name: "Studio Clean" }],
      get_store: () => RHYTHM_STORE,
      read_library_via_backup: () =>
        makeBackupRead({ total_rows: 1, presets: [STUDIO_CLEAN] }),
      // The only item's leveling stays pending until the test resolves the gate, then
      // answers the way the backend answers a Stop it saw: cancelled.
      level_preset: () =>
        gate.then(() =>
          rejectWith({ kind: "cancelled", message: "cancelled" }),
        ),
    });
    renderView(true);
    const user = userEvent.setup();
    await screen.findByText("Studio Clean");
    await user.click(
      screen.getByRole("checkbox", { name: "Choose Studio Clean" }),
    );
    await user.click(
      await screen.findByRole("button", { name: /^level 1 sound$/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: "Start leveling" }),
    );
    // The run is active on the only item; level_preset is still pending — press Stop
    // and confirm it in the dialog.
    await user.click(await screen.findByRole("button", { name: "Stop" }));
    const dialog = await screen.findByRole("dialog", {
      name: "Stop leveling?",
    });
    await user.click(within(dialog).getByRole("button", { name: "Stop" }));
    expect(
      await screen.findByText("Stopping after Main sound in Studio Clean"),
    ).toBeInTheDocument();
    // Now let the in-flight item finish. The loop exits and the final publish must
    // report STOPPED, because the cancel flag was set mid-item.
    resolveLevel();
    expect(
      await screen.findByRole("heading", {
        name: "Stopped. Nothing was leveled, 1 not reached.",
      }),
    ).toBeInTheDocument();
    expect(screen.queryByText("Leveling stopped")).toBeNull();
    expect(screen.queryByRole("button", { name: /continue/i })).toBeNull();
    expect(screen.getByRole("button", { name: "Try again (1)" })).toBeEnabled();
  });

  // ── BUG-4: closing the flow after a run deselects the leveled sounds ──────────
  // Closing after a run used to leave the whole selection ticked, so the next Level
  // re-ran everything. Done now prunes exactly the keys the runs leveled, accumulating
  // across Try again rounds; a sound left over stays chosen.
  it("Done after a run deselects the already-leveled sounds (BUG-4)", async () => {
    mockLevelingFixture({ clamped: true });
    renderView(true);
    const user = userEvent.setup();
    await selectWholePreset(user); // Base + 2 FS scenes selected
    await user.click(
      await screen.findByRole("button", { name: /^level 3 sounds$/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: "Start leveling" }),
    );
    await user.click(
      await screen.findByRole("button", { name: "Done" }, { timeout: 3000 }),
    );
    // The leveled sounds are deselected, so the list returns to the empty prompt (NOT
    // still offering "Level 3 sounds").
    expect(
      await screen.findByText("Choose presets or sounds to level"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /^level \d+ sounds?$/i }),
    ).toBeNull();
  });

  // ── Partial-failure: one item errors mid-run → skipped, the run CONTINUES ──────
  // A device failure on one sound must not abort the whole run; it's marked skipped
  // and the remaining sounds still level (data-loss-family regression).
  it("a mid-run item failure is skipped and the run continues to the rest", async () => {
    mockCommands({
      list_presets: () => [{ slot: 0, name: "Plexi" }],
      get_store: () => RHYTHM_STORE,
      read_library_via_backup: () =>
        makeBackupRead({ total_rows: 1, presets: [plexiRow()] }),
      // The Base sound fails; the two FS scenes still level.
      level_preset: () => rejectWith(new Error("device dropped the level")),
      list_level_blocks: () => [TWEED_OUTPUT],
      level_scenes_apply_batched: levelScenesDone,
    });
    renderView(true);
    const user = userEvent.setup();
    await selectWholePreset(user);
    await user.click(
      await screen.findByRole("button", { name: /^level 3 sounds$/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: "Start leveling" }),
    );
    // The run finished (auto-advanced to the Summary) despite the Base failure: exactly
    // the failed row needs you, and the two FS scenes still leveled (data-loss
    // regression guard — a partial failure must not sweep the whole run into "skipped").
    // The backend's own reason goes to the log, never the screen.
    expect(
      await screen.findByRole(
        "heading",
        { name: "2 sounds leveled. 1 needs you." },
        { timeout: 3000 },
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Couldn't level this sound, so its volume wasn't changed.",
      ),
    ).toBeInTheDocument();
    expect(screen.getAllByText("On target")).toHaveLength(2);
    expect(screen.queryByText("device dropped the level")).toBeNull();
    // The main sound, and the two scenes its new level will move.
    expect(screen.getByRole("button", { name: "Try again (3)" })).toBeEnabled();
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
    mockCommands({
      list_presets: () => [
        { slot: 0, name: "Row One" },
        { slot: 2, name: "Row Three" },
      ],
      get_store: () => RHYTHM_STORE,
    });

    // Connect-time handshake graph = the (soon-stale) preset 003 "Cello".
    const seed = heroGraph("Cello", 2, "ACD_HiwattDR103CanModCabIR");
    const ui = <LevelView connected={true} initialGraph={seed} />;
    const { unmount } = renderWithTheme(ui);
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
    renderWithTheme(ui);

    // The hero stays on the unit's CURRENT preset (001 Guitar), never reverts to 003.
    expect(await screen.findByText("Guitar")).toBeInTheDocument();
    expect(screen.queryByText("Cello")).toBeNull();
  });
});
