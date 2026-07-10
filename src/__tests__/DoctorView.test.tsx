// src/__tests__/DoctorView.test.tsx — the Doctor tab stage machine (select → setup
// → run → results), device mocked.
//
//   • Disconnected shows the connect-your-TMP empty state.
//   • Connected renders the preset list from the mocked list/store/backup rows.
//   • Ticking a preset offers "Check 1 sound…" (singular); the run fires ONE
//     `doctor_check` carrying the right key/listIndex/nodes, then auto-advances to
//     the results placeholder.
//   • The list never recalls a preset and the whole flow never saves.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ThemeProvider } from "../theme/ThemeProvider";

// Capture the backup-progress / live-sync listeners so usePresetData takes the real
// listen() path under the forced isTauri().
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
import { DoctorView } from "../views/doctor";
import {
  ensureLibraryScan,
  resetLibraryScan,
} from "../views/level/libraryScan";

function renderView(connected: boolean) {
  const r = render(
    <ThemeProvider>
      <DoctorView connected={connected} />
    </ThemeProvider>,
  );
  // The backup scan is App-owned in production; seed it here when connected so the
  // view's scene/graph data populates (the view no longer triggers the scan).
  if (connected) void ensureLibraryScan();
  return r;
}

// One non-empty, scene-less preset; the backup returns its (empty) scenes so the
// row settles to "base only" and selecting it yields a single "Whole preset" sound.
interface DoctorCheckArgs {
  items: {
    key: string;
    listIndex: number;
    scene: number | null;
    footswitch: number | null;
    tag: string | null;
    nodes: unknown[];
  }[];
  restoreListIndex: number | null;
  onResult?: { onmessage?: (item: unknown) => void };
}
let lastDoctorArgs: DoctorCheckArgs | null = null;

// A block-acting footswitch with a levelable candidate (the leveling filter's
// gate) — mirrors LevelView.test.tsx's `SOLO_FOOTSWITCH` fixture.
const SOLO_FOOTSWITCH = {
  switch: 3, // → tag FS4
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

function mockOnePreset(
  opts: { hangCheck?: boolean; footswitch?: boolean } = {},
) {
  lastDoctorArgs = null;
  vi.mocked(invoke).mockImplementation((command: string, args?: unknown) => {
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
              slot: 1, // device slot 1 = list index 0
              name: "Studio Clean",
              scene_count: 0,
              scenes: [],
              blocks: [],
              footswitches: opts.footswitch === true ? [SOLO_FOOTSWITCH] : [],
            },
          ],
          song_presets: [],
          songs: [],
          setlists: [],
          setlist_songs: [],
        });
      case "doctor_check": {
        const a = args as DoctorCheckArgs;
        lastDoctorArgs = a;
        // Hanging run — for the unmount-mid-run cancel test.
        if (opts.hangCheck === true) {
          return new Promise(() => {
            /* never resolves */
          });
        }
        // Stream each sound active → done, then resolve the cohort result.
        a.items.forEach((it) => {
          a.onResult?.onmessage?.({
            key: it.key,
            status: "active",
            message: null,
          });
          a.onResult?.onmessage?.({
            key: it.key,
            status: "done",
            message: null,
          });
        });
        return Promise.resolve({
          presets: [
            {
              listIndex: 0,
              sounds: a.items.map((it) => ({
                key: it.key,
                listIndex: it.listIndex,
                scene: it.scene,
                footswitch: it.footswitch,
                label: it.key,
                tag: it.tag,
                diags: [],
                integratedLufs: -20,
                tailRatioDb: 0,
                balanceDb: [],
                bandLabels: [
                  "Lows",
                  "Low-mids",
                  "Mids",
                  "High-mids",
                  "Highs",
                  "Air",
                ],
                error: null,
              })),
              sceneConsistency: null,
            },
          ],
          stopped: false,
          cohort: "absolute",
        });
      }
      default:
        return Promise.resolve(null);
    }
  });
}

const callsFor = (command: string) =>
  vi.mocked(invoke).mock.calls.filter((c) => c[0] === command).length;
const fired = (command: string) =>
  vi.mocked(invoke).mock.calls.some((c) => c[0] === command);

describe("DoctorView — connection gating", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    listeners.clear();
    resetLibraryScan();
    localStorage.clear();
  });

  it("shows the connect-your-TMP empty state when disconnected", () => {
    renderView(false);
    expect(
      screen.getByText("Doctor needs the Tone Master Pro"),
    ).toBeInTheDocument();
  });
});

describe("DoctorView — select → setup → run → results", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    listeners.clear();
    resetLibraryScan();
    localStorage.clear();
    mockOnePreset();
  });

  it("walks the whole flow, fires ONE doctor_check, and auto-advances to results", async () => {
    renderView(true);
    const user = userEvent.setup();

    // The list renders from the mocked reads.
    await screen.findByText("Studio Clean");

    // Tick the preset → a single "Whole preset" sound → "Check 1 sound…" (singular).
    await user.click(screen.getAllByTitle("Select preset to check")[0]);
    await user.click(
      await screen.findByRole("button", { name: /check 1 sound…/i }),
    );

    // Set up shows the instrument Pick with its default (None, no profiles).
    expect(
      await screen.findByText("What are you playing?"),
    ).toBeInTheDocument();
    expect(screen.getAllByText("None").length).toBeGreaterThan(0);

    // Run the check → the run fires and auto-advances to the results page (the one
    // all-clear sound resolves to the summary + a "Check other sounds" exit).
    // (The mocked check resolves at once, so the transient run title isn't asserted.)
    await user.click(
      screen.getByRole("button", { name: /run check on 1 sound/i }),
    );
    expect(
      await screen.findByRole(
        "button",
        { name: /check other sounds/i },
        { timeout: 3000 },
      ),
    ).toBeInTheDocument();

    // ONE doctor_check, carrying the right key / listIndex / nodes.
    expect(callsFor("doctor_check")).toBe(1);
    expect(lastDoctorArgs?.items).toHaveLength(1);
    expect(lastDoctorArgs?.items[0].key).toBe("p0");
    expect(lastDoctorArgs?.items[0].listIndex).toBe(0);
    expect(lastDoctorArgs?.items[0].scene).toBeNull();
    expect(lastDoctorArgs?.items[0].nodes).toEqual([]);
    // No live-preset event fired in this test → no slot to restore.
    expect(lastDoctorArgs?.restoreListIndex).toBeNull();

    // The list never recalled a preset, and the whole flow never saved.
    expect(fired("load_preset_on_amp")).toBe(false);
    expect(fired("load_scene_on_amp")).toBe(false);
    expect(fired("doctor_save")).toBe(false);
    expect(fired("doctor_apply")).toBe(false);
    expect(fired("level_preset")).toBe(false);
  });

  it("includes a block-acting footswitch sound (footswitch set, scene null, FS tag)", async () => {
    mockOnePreset({ footswitch: true });
    renderView(true);
    const user = userEvent.setup();

    await screen.findByText("Studio Clean");

    // Whole-row select includes Base + the footswitch child key → 2 sounds.
    await user.click(screen.getAllByTitle("Select preset to check")[0]);
    await user.click(
      await screen.findByRole("button", { name: /check 2 sounds…/i }),
    );
    await screen.findByText("What are you playing?");
    await user.click(
      screen.getByRole("button", { name: /run check on 2 sounds/i }),
    );
    expect(
      await screen.findByRole(
        "button",
        { name: /check other sounds/i },
        { timeout: 3000 },
      ),
    ).toBeInTheDocument();

    expect(callsFor("doctor_check")).toBe(1);
    const items = lastDoctorArgs?.items ?? [];
    expect(items).toHaveLength(2);
    const fsItem = items.find((it) => it.footswitch !== null);
    expect(fsItem?.footswitch).toBe(3);
    expect(fsItem?.scene).toBeNull();
    expect(fsItem?.tag).toMatch(/^FS\d/);
  });

  it("fires cancel_doctor_check on unmount while a check is in flight", async () => {
    mockOnePreset({ hangCheck: true });
    const { unmount } = renderView(true);
    const user = userEvent.setup();

    await screen.findByText("Studio Clean");
    await user.click(screen.getAllByTitle("Select preset to check")[0]);
    await user.click(
      await screen.findByRole("button", { name: /check 1 sound…/i }),
    );
    await screen.findByText("What are you playing?");
    await user.click(
      screen.getByRole("button", { name: /run check on 1 sound/i }),
    );

    // The check hangs — a tab switch (unmount) must cancel the orphaned run.
    expect(fired("cancel_doctor_check")).toBe(false);
    unmount();
    expect(fired("cancel_doctor_check")).toBe(true);
  });
});
