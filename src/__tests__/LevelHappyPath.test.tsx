// LevelHappyPath — the FULL Level wizard happy path, end-to-end, against a mocked
// invoke bridge. Selects the WHOLE preset (Base + its FS scenes) so the run exercises
// all three device commands: level_preset (Base), list_level_blocks (FS amp discovery),
// and level_scenes_apply_batched (FS scenes). Walks list → setup → run → summary →
// Done, then asserts the footer returns to the idle hint.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { LevelView } from "../views/level";
import { ensureLibraryScan, resetLibraryScan } from "../device/libraryScan";
import { resetLevelFlow } from "../views/level/levelRunStore";
import type { LevelResult } from "../lib/types";
import {
  callsOf,
  fired,
  forceTauri,
  makeBackupPresetRow,
  makeBackupRead,
  makeBatchRun,
  makeLevelResult,
  makeStore,
  mockCommands,
  withoutAmpCandidates,
} from "./commandMocks";
import { renderWithTheme } from "./testKit";

// In-app: the live bridges and the library scan subscribe.
forceTauri();

// The leveling wizard lives in a module store that outlives a mount; start each case clean.
beforeEach(() => {
  resetLevelFlow();
});

const SLOT = 0; // list index 0 == device slot 1
const TARGET_LUFS = -26; // the single "Rhythm" store target

const levelResultStub = (over: Partial<LevelResult> = {}): LevelResult =>
  makeLevelResult({ slot: SLOT, target_lufs: TARGET_LUFS, ...over });

// "Stadium Lead" with a base + two FS scenes. The row deliberately has NO
// `amp_candidates` key, so the scene lane must fall back to the live
// `list_level_blocks` read — the fallback this suite exercises.
const STADIUM_LEAD = withoutAmpCandidates(
  makeBackupPresetRow({
    slot: 1, // device slot 1 == list index 0
    name: "Stadium Lead",
    scene_count: 2,
    scenes: [
      { name: "Rhythm", fs: 1 },
      { name: "Lead", fs: 2 },
    ],
  }),
);

// One preset; the backup read supplies its scenes (so the row expands), and the three
// level commands are stubbed. The batched scene command fires its Channel onmessage per
// scene before resolving.
function mockHappyPath(baseOver: Partial<LevelResult> = {}) {
  mockCommands({
    list_presets: () => [{ slot: SLOT, name: "Stadium Lead" }],
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
        targets: [{ name: "Rhythm", lufs: TARGET_LUFS }],
      }),
    read_library_via_backup: () =>
      makeBackupRead({ total_rows: 1, presets: [STADIUM_LEAD] }),
    level_preset: () => levelResultStub(baseOver),
    list_level_blocks: () => [
      {
        group_id: "amp",
        node_id: "amp0",
        model_id: "ACD_TweedDeluxe", // a real amp bid → passes ampCandidates
        parameter_id: "outputLevel",
        value: 0.5,
      },
    ],
    level_scenes_apply_batched: ({ jobs, onResult }) => {
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
    },
  });
}

function renderView() {
  const r = renderWithTheme(<LevelView connected={true} />);
  void ensureLibraryScan(); // App-owned scan in production; seed it for the view here
  return r;
}

// Tick the inline backup acknowledgment in the Set-up footer — it gates the
// "Level N sounds" button (there is no separate Back-up step).
async function ackBackup(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByText(/backed up with Pro Control/i));
}

describe("LevelView — full Level wizard happy path (e2e, device mocked)", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    resetLibraryScan();
    localStorage.clear();
  });

  it("whole-preset: list → setup → run → summary → Done returns to idle", async () => {
    mockHappyPath();
    const user = userEvent.setup();
    renderView();

    // ── 1. list lands; the wizard is NOT open at setup yet ──────────────────────
    await screen.findByText("Stadium Lead");
    await screen.findByText("2 scenes"); // 2 FS scenes (count excludes Base), backup ready
    expect(screen.queryByText("Set instrument & target")).toBeNull();

    // ── 2. select the whole preset → open the wizard directly at Set up ──────────
    await user.click(
      screen.getByRole("checkbox", { name: "Choose Stadium Lead" }),
    );
    await user.click(
      await screen.findByRole("button", { name: /^level 3 sounds$/i }),
    );
    // Opens straight at Set up — there is no separate Back-up step.
    expect(
      await screen.findByRole("heading", { name: /^Level \d+ sounds?$/ }),
    ).toBeInTheDocument();
    expect(screen.queryByText("Back up your unit first")).toBeNull();

    // ── 3. setup stage: instrument + target controls render ─────────────────────
    // Instrument + target controls render (the lone profile/target are surfaced
    // on the apply-to bar and per-row — at least once each).
    expect(screen.getAllByText("Telecaster").length).toBeGreaterThan(0);
    expect(screen.getAllByText(/Rhythm/i).length).toBeGreaterThan(0);
    // Commit: tick the footer backup ack, then "Start leveling" (Base + Rhythm + Lead).
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: "Start leveling" }),
    );

    // ── 4 + 5. run auto-advances (650 ms) to the summary headline ───────────────
    expect(
      await screen.findByText("All 3 sounds leveled.", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();

    // ── 6. Done → footer returns to the idle hint ───────────────────────────────
    await user.click(screen.getByRole("button", { name: "Done" }));
    expect(
      await screen.findByText("Choose presets or sounds to level"),
    ).toBeInTheDocument();

    // ── assertions: the three device commands fired ─────────────────────────────
    expect(fired("level_preset")).toBe(true); // Base via presetLevel
    expect(fired("list_level_blocks")).toBe(true); // FS amp discovery
    expect(fired("level_scenes_apply_batched")).toBe(true); // FS scene level

    // The level_preset call carries a job with the expected slot + target. The job
    // payload keys are snake_case on the wire (LevelJob.target_lufs).
    const [baseCall] = callsOf("level_preset");
    expect(baseCall.job.slot).toBe(SLOT);
    expect(baseCall.job.target_lufs).toBe(TARGET_LUFS);
  });

  // #187: the store maps the backend's `verify_failed` to a "not confirmed" row. A Base
  // whose check capture failed carries only a prediction, so it must not count as leveled:
  // it needs you, and Try again confirms it (with its preset's leveled scenes, which a
  // main sound's new level moves).
  it("a Base whose verify failed is reported not confirmed, never matched", async () => {
    mockHappyPath({ verification: "verify_failed", verify_lufs: null });
    const user = userEvent.setup();
    renderView();

    await screen.findByText("Stadium Lead");
    await screen.findByText("2 scenes");
    await user.click(
      screen.getByRole("checkbox", { name: "Choose Stadium Lead" }),
    );
    await user.click(
      await screen.findByRole("button", { name: /^level 3 sounds$/i }),
    );
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: "Start leveling" }),
    );

    expect(
      await screen.findByText(
        "Volume set, but the final check didn't finish. Try again to confirm it.",
        undefined,
        { timeout: 3000 },
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "2 sounds leveled. 1 needs you." }),
    ).toBeInTheDocument();
    expect(screen.queryByText("All 3 sounds leveled.")).toBeNull();
    expect(screen.getByRole("button", { name: "Try again (3)" })).toBeEnabled();
  });
});
