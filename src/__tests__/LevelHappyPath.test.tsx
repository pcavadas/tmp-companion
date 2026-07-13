// LevelHappyPath — the FULL Level wizard happy path, end-to-end, against a mocked
// invoke bridge. Selects the WHOLE preset (Base + its FS scenes) so the run exercises
// all three device commands: level_preset (Base), list_level_blocks (FS amp discovery),
// and level_scenes_apply_batched (FS scenes). Walks list → setup → run → summary →
// Done, then asserts the footer returns to the idle hint.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ThemeProvider } from "../theme/ThemeProvider";

// 1) event bridge + listeners Map — the view's live subscriptions take the real
//    listen() path under the forced isTauri().
const listeners = new Map<string, (e: { payload: unknown }) => void>();
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, cb: (e: { payload: unknown }) => void) => {
    listeners.set(name, cb);
    return Promise.resolve(() => listeners.delete(name));
  },
}));

// 2) force isTauri() true so libraryScan + live subscriptions run.
vi.mock("../lib/invoke", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/invoke")>();
  return { ...actual, isTauri: () => true };
});

// NOTE: @tauri-apps/api/core invoke + Channel are globally mocked in src/__tests__/setup.ts
//   (Channel = class MockChannel { onmessage = null }). Per test we OVERRIDE invoke via
//   vi.mocked(invoke).mockImplementation((command, args) => switch(command){...}); unhandled
//   commands fall through to setup.ts's empty shapes.

// Imported AFTER the mocks so the view picks up the forced isTauri().
import { LevelView } from "../views/level";
import {
  ensureLibraryScan,
  resetLibraryScan,
} from "../views/level/libraryScan";

const SLOT = 0; // list index 0 == device slot 1
const TARGET_LUFS = -26; // the single "Rhythm" store target

function levelResultStub(over: Record<string, unknown> = {}) {
  return {
    slot: SLOT,
    ref_level: 0.5,
    measured_lufs: -13.0,
    constant_c: -13.0,
    final_level: 0.18,
    target_lufs: TARGET_LUFS,
    predicted_lufs: TARGET_LUFS,
    clamped: false,
    clamp_reason: null,
    dynamic_spread_lu: 1.1, // below DYNAMIC_SPREAD_LU → no "by ear"
    verify_by_ear: null,
    saved: true,
    verify_lufs: TARGET_LUFS,
    iterations: 1,
    ...over,
  };
}

// One preset "Stadium Lead" with a base + two FS scenes; the backup read supplies its
// scenes (so the row expands), and the three level commands are stubbed. The batched
// scene command harmlessly fires its Channel onmessage per scene before resolving.
function mockHappyPath() {
  vi.mocked(invoke).mockImplementation((command: string, args?: unknown) => {
    switch (command) {
      case "list_presets":
        return Promise.resolve([{ slot: SLOT, name: "Stadium Lead" }]);
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
          targets: [{ name: "Rhythm", lufs: TARGET_LUFS }],
        });
      case "read_library_via_backup":
        return Promise.resolve({
          members: [],
          db_bytes: 0,
          total_rows: 1,
          scene_mode: "test",
          presets: [
            {
              slot: 1, // device slot 1 == list index 0
              name: "Stadium Lead",
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
      case "level_preset":
        return Promise.resolve(levelResultStub());
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
        const a = args as {
          jobs: { sceneSlot: number; targetLufs: number }[];
          onResult?: { onmessage?: (item: unknown) => void };
        };
        const results = a.jobs.map(() => levelResultStub());
        a.jobs.forEach((j, i) =>
          a.onResult?.onmessage?.({
            sceneSlot: j.sceneSlot,
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

function renderView() {
  const r = render(
    <ThemeProvider>
      <LevelView connected={true} />
    </ThemeProvider>,
  );
  void ensureLibraryScan(); // App-owned scan in production; seed it for the view here
  return r;
}

const fired = (command: string) =>
  vi.mocked(invoke).mock.calls.some((c) => c[0] === command);

// Tick the inline backup acknowledgment in the Set-up footer — it gates the
// "Level N sounds" button (there is no separate Back-up step).
async function ackBackup(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByText(/backed up with Pro Control/i));
}

describe("LevelView — full Level wizard happy path (e2e, device mocked)", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    listeners.clear();
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
    await user.click(screen.getAllByTitle("Select preset to level")[0]);
    await user.click(
      await screen.findByRole("button", { name: /level 1 preset/i }),
    );
    // Opens straight at Set up — there is no separate Back-up step.
    expect(
      await screen.findByText("Set instrument & target"),
    ).toBeInTheDocument();
    expect(screen.queryByText("Back up your unit first")).toBeNull();

    // ── 3. setup stage: instrument + target controls render ─────────────────────
    // Instrument + target controls render (the lone profile/target are surfaced
    // on the apply-to bar and per-row — at least once each).
    expect(screen.getAllByText("Telecaster").length).toBeGreaterThan(0);
    expect(screen.getAllByText(/Rhythm/i).length).toBeGreaterThan(0);
    // Commit: tick the footer backup ack, then Base + Rhythm + Lead → "Level 3 sounds".
    await ackBackup(user);
    await user.click(
      await screen.findByRole("button", { name: /level 3 sounds/i }),
    );

    // ── 4 + 5. run auto-advances (650 ms) to the summary headline ───────────────
    expect(
      await screen.findByText("All 3 sounds leveled", undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();

    // ── 6. Done → footer returns to the idle hint ───────────────────────────────
    await user.click(screen.getByRole("button", { name: /done/i }));
    expect(
      await screen.findByText(/select presets to level/i),
    ).toBeInTheDocument();

    // ── assertions: the three device commands fired ─────────────────────────────
    expect(fired("level_preset")).toBe(true); // Base via presetLevel
    expect(fired("list_level_blocks")).toBe(true); // FS amp discovery
    expect(fired("level_scenes_apply_batched")).toBe(true); // FS scene level

    // The level_preset call carries a job with the expected slot + target. The job
    // payload keys are snake_case on the wire (invoke.ts: invoke("level_preset", { job })
    // with LevelJob.target_lufs snake_case).
    const baseCall = vi
      .mocked(invoke)
      .mock.calls.find((c) => c[0] === "level_preset");
    expect(baseCall).toBeDefined();
    const job = (
      baseCall?.[1] as { job: { slot: number; target_lufs: number } }
    ).job;
    expect(job.slot).toBe(SLOT);
    expect(job.target_lufs).toBe(TARGET_LUFS);
  });
});
