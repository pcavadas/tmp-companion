// Regression locks for the pre-1.0 hardening fixes that aren't covered by the existing
// component suites: BUG-2 (Copy save to the active preset optimistically repaints the hero),
// BUG-5 (setlist row mutate-gated-while-busy), BUG-6 (calibration unmount can't apply a
// result), BUG-7 (Copy ON-UNIT chip follows the live preset).

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, act, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ThemeProvider } from "../theme/ThemeProvider";

// Capture the live-sync listeners (BUG-7) so the test can push a tmp://live-preset event,
// and force isTauri() true so useLiveDevice takes the real listen() path.
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

// Imported AFTER the mocks so they pick up the forced isTauri().
import { SetlistDetail } from "../views/songs/SetlistDetail";
import { InstrumentRow } from "../views/settings/InstrumentRow";
import { CopyView } from "../views/copy";
import {
  ensureLibraryScan,
  resetLibraryScan,
} from "../views/level/libraryScan";
import { resetLiveDevice, useLiveDevice } from "../views/level/useLiveDevice";
import { backupRow, seriesGraph } from "./copyFixtures";
import type {
  ActiveGraph,
  Profile,
  SetlistRecord,
  SongRecord,
} from "../lib/types";

/** Nearest ancestor matching `sel` — avoids the lint-forbidden non-null `!`. */
function ancestor(el: HTMLElement, sel: string): HTMLElement {
  const found = el.closest(sel);
  if (!found) throw new Error(`no ${sel} ancestor`);
  return found as HTMLElement;
}

// ── BUG-5: a setlist row's reorder + remove are gated while a device write is busy ──
describe("SetlistDetail — mutate gated while busy (BUG-5)", () => {
  const song: SongRecord = { slot: 1, name: "Song A", notes: "", bpm: 0 };
  const setlist: SetlistRecord = { slot: 1, name: "Set 1" };
  const baseProps = {
    setlist,
    members: [song],
    available: [],
    onRename: vi.fn(),
    onDelete: vi.fn(),
    onReorder: vi.fn(),
    onAdd: vi.fn(),
    onCreateAndAdd: vi.fn(),
  };

  it("does not fire remove while busy, does when idle; the row isn't draggable while busy", async () => {
    const onRemoveSong = vi.fn();
    const props = { ...baseProps, onRemoveSong };
    const { rerender } = render(
      <ThemeProvider>
        <SetlistDetail {...props} busy={true} />
      </ThemeProvider>,
    );
    const user = userEvent.setup();
    // The row is non-draggable while a write is in flight.
    expect(
      ancestor(screen.getByText("Song A"), "[draggable]").getAttribute(
        "draggable",
      ),
    ).toBe("false");
    await user.click(screen.getByTitle("Remove from setlist"));
    expect(onRemoveSong).not.toHaveBeenCalled();

    rerender(
      <ThemeProvider>
        <SetlistDetail {...props} busy={false} />
      </ThemeProvider>,
    );
    expect(
      ancestor(screen.getByText("Song A"), "[draggable]").getAttribute(
        "draggable",
      ),
    ).toBe("true");
    await user.click(screen.getByTitle("Remove from setlist"));
    expect(onRemoveSong).toHaveBeenCalledWith(1); // 1-based position
  });
});

// ── BUG-6: unmounting mid-recording can't setState / fire onCalibrated after unmount ──
describe("InstrumentRow — calibration abort on unmount (BUG-6)", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
  });

  const profile: Profile = {
    id: "tele",
    name: "Telecaster",
    topology_id: "sc",
    calibration_lufs: null,
  };

  it(
    "a calibrate that resolves AFTER unmount never calls onCalibrated",
    { timeout: 10000 },
    async () => {
      let resolveCal!: () => void;
      const gate = new Promise<void>((res) => {
        resolveCal = res;
      });
      vi.mocked(invoke).mockImplementation((command: string) =>
        command === "calibrate_profile"
          ? gate.then(() => ({
              lufs: -20,
              clipped: false,
              stimulus_shortfall_lu: null,
              spread_lu: 0,
              band_coverage: [],
              band_labels: [],
            }))
          : Promise.resolve(null),
      );
      const onCalibrated = vi.fn();
      const { unmount } = render(
        <ThemeProvider>
          <InstrumentRow
            profile={profile}
            topology={null}
            connected={true}
            onCalibrated={onCalibrated}
            onEdit={vi.fn()}
            onDelete={vi.fn()}
            onMove={vi.fn()}
          />
        </ThemeProvider>,
      );
      const user = userEvent.setup();
      await user.click(screen.getByRole("button", { name: /calibrate/i }));
      // Wait out the 3×850ms countdown into the recording phase (calibrate_profile in flight).
      // "play steadily" is unique to the recording sub-line (vs. two "Recording" strings).
      await screen.findByText(/play steadily/i, undefined, { timeout: 6000 });
      expect(
        vi.mocked(invoke).mock.calls.some((c) => c[0] === "calibrate_profile"),
      ).toBe(true);
      // Unmount mid-recording, THEN let the backend resolve — the abort gate must hold.
      unmount();
      await act(async () => {
        resolveCal();
        await gate;
      });
      expect(onCalibrated).not.toHaveBeenCalled();
    },
  );

  it(
    "clip + stimulus-ceiling caveats surface as a non-fatal warning",
    { timeout: 10000 },
    async () => {
      vi.mocked(invoke).mockImplementation((command: string) =>
        command === "calibrate_profile"
          ? Promise.resolve({
              lufs: -12.4,
              clipped: true,
              stimulus_shortfall_lu: 2.3,
              spread_lu: 3.5,
              band_coverage: [true, true, true, true, true, true],
              band_labels: ["Lo", "LoM", "Mid", "HiM", "Hi", "Air"],
            })
          : Promise.resolve(null),
      );
      const onCalibrated = vi.fn();
      render(
        <ThemeProvider>
          <InstrumentRow
            profile={{ ...profile, calibration_lufs: -12.4 }}
            topology={null}
            connected={true}
            onCalibrated={onCalibrated}
            onEdit={vi.fn()}
            onDelete={vi.fn()}
            onMove={vi.fn()}
          />
        </ThemeProvider>,
      );
      const user = userEvent.setup();
      await user.click(screen.getByRole("button", { name: /re-calibrate/i }));
      // Both caveats land on the idle sub-line, joined "; " — success still fires.
      await screen.findByText(/signal clipped/i, undefined, { timeout: 8000 });
      expect(
        screen.getByText(/leveling drives ~2\.3 LU softer/i),
      ).toBeInTheDocument();
      expect(onCalibrated).toHaveBeenCalled();
    },
  );
});

// ── BUG-7: the Copy ON-UNIT chip follows the unit's LIVE active preset ──
describe("CopyView — ON UNIT chip follows the live preset (BUG-7)", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    listeners.clear();
    resetLibraryScan();
    resetLiveDevice();
  });

  const graphAtSlot = (slot: number): ActiveGraph => ({
    name: "On Unit",
    slot,
    template: "gtrSeries",
    split_mix: null,
    nodes: [],
    stages: [],
  });

  it("moves the chip from the connect-time slot to the live one on a tmp://live-preset push", async () => {
    vi.mocked(invoke).mockImplementation((command: string) =>
      command === "list_presets"
        ? Promise.resolve([
            { slot: 0, name: "Alpha" },
            { slot: 1, name: "Beta" },
          ])
        : Promise.resolve(null),
    );
    render(
      <ThemeProvider>
        <CopyView connected={true} initialGraph={graphAtSlot(1)} />
      </ThemeProvider>,
    );
    // Exactly one ON UNIT chip; it starts in Beta's row (the connect-time graph slot).
    // Anchor on the unique chip, not the name (names can repeat across Copy's columns).
    const chip0 = await screen.findByText("ON UNIT");
    expect(
      within(ancestor(chip0, "div")).getByText("Beta"),
    ).toBeInTheDocument();
    expect(within(ancestor(chip0, "div")).queryByText("Alpha")).toBeNull();

    // A hardware preset change to slot 0 arrives — the chip follows to Alpha.
    await waitFor(() => {
      expect(listeners.has("tmp://live-preset")).toBe(true);
    });
    act(() => {
      listeners.get("tmp://live-preset")?.({
        payload: {
          listIndex: 0,
          name: "Alpha",
          isDirty: false,
          isFavorite: false,
        },
      });
    });
    await waitFor(() => {
      const chip = screen.getByText("ON UNIT"); // still exactly one
      expect(
        within(ancestor(chip, "div")).queryByText("Alpha"),
      ).toBeInTheDocument();
    });
    expect(
      within(ancestor(screen.getByText("ON UNIT"), "div")).queryByText("Beta"),
    ).toBeNull();
  });
});

// ── BUG-2: saving a Copy edit to the ACTIVE preset optimistically repaints the hero ──
// A live structural edit pushes NO device field-3, so the hero (driven by useLiveDevice's
// graph) would stay stale until the monitor reconnects. CopyView must patch the live graph
// optimistically when the edited target IS the active preset — no device re-read.
describe("CopyView — hero optimistic patch after active-slot save (BUG-2)", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    listeners.clear();
    resetLibraryScan();
    resetLiveDevice();
    localStorage.clear();
  });

  // The authoritative post-save read-back the device returns (distinct name so the hero
  // assertion is unambiguous). copy_apply returns it on the `graph` field of the item.
  const SAVED = seriesGraph("HERO-AFTER-SAVE", 1);

  function HeroProbe() {
    const live = useLiveDevice(true);
    return <div data-testid="hero-graph">{live.graph?.name ?? "none"}</div>;
  }

  it("repaints the hero with the saved graph when the edited target is the active preset", async () => {
    vi.mocked(invoke).mockImplementation((command: string, args?: unknown) => {
      switch (command) {
        case "list_presets":
          return Promise.resolve([
            { slot: 0, name: "Stadium Lead" },
            { slot: 1, name: "Clean Verse" },
          ]);
        case "get_store":
          return Promise.resolve({
            profiles: [],
            profile_by_slot: {},
            targets: [],
            playback_level: "stage",
          });
        case "read_library_via_backup":
          return Promise.resolve({
            members: [],
            db_bytes: 0,
            total_rows: 2,
            scene_mode: "off",
            presets: [
              backupRow(0, "Stadium Lead"),
              backupRow(1, "Clean Verse"),
            ],
            song_presets: [],
            songs: [],
            setlists: [],
            setlist_songs: [],
          });
        case "copy_apply": {
          const a = args as {
            jobs: { listIndex: number; name: string }[];
            onResult?: { onmessage?: (i: unknown) => void };
          };
          const items = a.jobs.map((j) => ({
            slot: j.listIndex,
            name: j.name,
            outcome: "updated",
            detail: "",
            graph: SAVED, // authoritative device read-back
          }));
          items.forEach((i) => a.onResult?.onmessage?.(i));
          return Promise.resolve(items);
        }
        default:
          return Promise.resolve(null);
      }
    });
    const user = userEvent.setup();
    render(
      <ThemeProvider>
        {/* active preset = slot 1 ("Clean Verse") via initialGraph; that's also the target we edit */}
        <CopyView
          connected={true}
          initialGraph={seriesGraph("Clean Verse", 1)}
        />
        <HeroProbe />
      </ThemeProvider>,
    );
    void ensureLibraryScan();

    // Hero starts WITHOUT the saved graph.
    expect(screen.getByTestId("hero-graph").textContent).not.toBe(
      "HERO-AFTER-SAVE",
    );

    // Step 1: reference = Stadium Lead (slot 0), target = Clean Verse (slot 1 = active).
    await screen.findByText("Copy blocks between presets", undefined, {
      timeout: 3000,
    });
    const stadium = await screen.findAllByText("Stadium Lead", undefined, {
      timeout: 3000,
    });
    await user.click(stadium[0]);
    const clean = await screen.findAllByText("Clean Verse", undefined, {
      timeout: 3000,
    });
    await user.click(clean[clean.length - 1]);
    await user.click(
      await screen.findByRole(
        "button",
        { name: /place the blocks/i },
        { timeout: 3000 },
      ),
    );

    // Step 2: replace the TwinReverb tile with DynaComp, back up, Save.
    const twin = await screen.findAllByText("65 TWN", undefined, {
      timeout: 3000,
    });
    await user.click(twin[twin.length - 1]);
    await user.click(
      await screen.findByText("DYNAMIC COMPRESSOR", undefined, {
        timeout: 3000,
      }),
    );
    await user.click(
      await screen.findByText(/backed up with pro control/i, undefined, {
        timeout: 3000,
      }),
    );
    await act(async () => {
      await user.click(
        await screen.findByRole(
          "button",
          { name: /save to the unit/i },
          { timeout: 3000 },
        ),
      );
    });
    await screen.findByText("Saved to the unit.", undefined, { timeout: 3000 });

    // BUG-2 lock: the hero now shows the saved graph (optimistic, no device re-read).
    await waitFor(() => {
      expect(screen.getByTestId("hero-graph").textContent).toBe(
        "HERO-AFTER-SAVE",
      );
    });
  });
});
