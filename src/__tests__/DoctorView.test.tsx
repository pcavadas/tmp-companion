// src/__tests__/DoctorView.test.tsx — the Doctor tab stage machine (select → instrument
// → check → results), device mocked.
//
//   • Disconnected shows the shared "Plug in your Tone Master Pro" page.
//   • Connected renders the preset list from the mocked list/store/backup rows.
//   • Ticking a preset offers "Check 1 sound" (singular). A preset with no saved
//     instrument goes through "Which instrument plays these?" (one per preset; the
//     choice is remembered with `set_preset_instrument`, never on the unit); one whose
//     instrument is saved (an explicit None counts) skips straight to the check. The run
//     fires ONE `doctor_check` with the right key/listIndex/nodes, then lands on Results.
//   • The list never recalls a preset and nothing in the flow writes to the unit.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { DoctorView } from "../views/doctor";
import { ensureLibraryScan, resetLibraryScan } from "../device/libraryScan";
import type { CommandArgs } from "../lib/commands";
import type { DoctorCheckResult, FootswitchInfo, Store } from "../lib/types";
import {
  forceTauri,
  makeBackupPresetRow,
  makeBackupRead,
  makeStore,
  mockCommands,
} from "./commandMocks";
import { renderWithTheme } from "./testKit";
import { choose } from "./setupPageTestUtils";

// In-app: the live bridges and the library scan subscribe.
forceTauri();

function renderView(connected: boolean) {
  const r = renderWithTheme(<DoctorView connected={connected} />);
  // The backup scan is App-owned in production; seed it here when connected so the
  // view's scene/graph data populates (the view no longer triggers the scan).
  if (connected) void ensureLibraryScan();
  return r;
}

// One non-empty, scene-less preset; the backup returns its (empty) scenes so the
// row settles to "base only" and selecting it yields a single sound, its main sound.
type DoctorCheckArgs = CommandArgs<"doctor_check">;
let lastDoctorArgs: DoctorCheckArgs | null = null;

// A block-acting footswitch with a levelable candidate (the leveling filter's
// gate) — mirrors LevelView.test.tsx's `SOLO_FOOTSWITCH` fixture.
const SOLO_FOOTSWITCH: FootswitchInfo = {
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
      class: "level_linear",
    },
  ],
};

// An Other-class-only footswitch (no level candidate) — a plain on/off toggle, e.g.
// a tuner mute. Zero level_params, so `footswitchesPerIndex` (Level's filtered map)
// drops it entirely; the damage detector must still see it via the unfiltered feed.
const BARE_FOOTSWITCH: FootswitchInfo = {
  switch: 5,
  label: "Tuner",
  link_group: null,
  functions: [],
  level_params: [],
};

function mockOnePreset(
  opts: {
    hangCheck?: boolean;
    footswitch?: boolean;
    bareFootswitch?: boolean;
    store?: Partial<Store>;
  } = {},
) {
  lastDoctorArgs = null;
  mockCommands({
    list_presets: () => [{ slot: 0, name: "Studio Clean" }],
    get_store: () =>
      makeStore({
        targets: [{ name: "Rhythm", lufs: -26 }],
        playback_level: "rehearsal",
        ...opts.store,
      }),
    read_library_via_backup: () =>
      makeBackupRead({
        total_rows: 1,
        presets: [
          makeBackupPresetRow({
            slot: 1, // device slot 1 = list index 0
            name: "Studio Clean",
            footswitches: [
              ...(opts.footswitch === true ? [SOLO_FOOTSWITCH] : []),
              ...(opts.bareFootswitch === true ? [BARE_FOOTSWITCH] : []),
            ],
          }),
        ],
      }),
    doctor_check: (a) => {
      lastDoctorArgs = a;
      // Hanging run — for the unmount-mid-run cancel test.
      if (opts.hangCheck === true) {
        return new Promise<DoctorCheckResult>(() => {
          /* never resolves */
        });
      }
      // Stream each sound active → done, then resolve the check result.
      a.items.forEach((it) => {
        a.onResult.onmessage({ key: it.key, status: "active", message: null });
        a.onResult.onmessage({ key: it.key, status: "done", message: null });
      });
      return {
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
              status: "measured" as const,
              integratedLufs: -20,
              tailRatioDb: 0,
              balanceDb: [],
              bandLabels: [
                "Lows",
                "Low mids",
                "Mids",
                "High mids",
                "Highs",
                "Air",
              ],
              cutThrough: null,
              error: null,
              errorKind: null,
              skippedBandCount: 0,
            })),
            sceneConsistency: null,
            levelingDamage: [],
          },
        ],
        stopped: false,
      };
    },
  });
}

const callsFor = (command: string) =>
  vi.mocked(invoke).mock.calls.filter((c) => c[0] === command).length;
const fired = (command: string) =>
  vi.mocked(invoke).mock.calls.some((c) => c[0] === command);

describe("DoctorView — connection gating", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    resetLibraryScan();
    localStorage.clear();
  });

  it("shows the shared plug-it-in page when disconnected", () => {
    renderView(false);
    expect(
      screen.getByText("Plug in your Tone Master Pro"),
    ).toBeInTheDocument();
  });
});

/** Tick Studio Clean and click "Check N sounds". */
async function chooseAndCheck(
  user: ReturnType<typeof userEvent.setup>,
  sounds: number,
) {
  await screen.findByText("Studio Clean");
  await user.click(
    screen.getByRole("checkbox", { name: "Choose Studio Clean" }),
  );
  await user.click(
    await screen.findByRole("button", {
      name: `Check ${String(sounds)} ${sounds === 1 ? "sound" : "sounds"}`,
    }),
  );
}

/** On the Instrument page: choose None for Studio Clean and check. */
async function chooseNoneAndRun(
  user: ReturnType<typeof userEvent.setup>,
  sounds: number,
) {
  await screen.findByText("Which instrument plays these?");
  await choose(user, "inst:0", "none");
  await user.click(
    screen.getByRole("button", {
      name: `Check ${String(sounds)} ${sounds === 1 ? "sound" : "sounds"}`,
    }),
  );
}

describe("DoctorView — select → instrument → check → results", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    resetLibraryScan();
    localStorage.clear();
    mockOnePreset();
  });

  it("walks the whole flow, fires ONE doctor_check, and lands on Results", async () => {
    renderView(true);
    const user = userEvent.setup();

    await chooseAndCheck(user, 1);

    // No instrument saved for the preset: the Instrument page asks, with "Choose" in
    // the attention look, and Check waits for it.
    expect(
      await screen.findByText("Which instrument plays these?"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Studio Clean doesn't have one yet. Doctor needs it to judge the tone.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Check 1 sound" }),
    ).toBeDisabled();
    await chooseNoneAndRun(user, 1);

    expect(
      await screen.findByRole(
        "button",
        { name: "Check other sounds" },
        { timeout: 3000 },
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Nothing to change in this sound" }),
    ).toBeInTheDocument();

    // ONE doctor_check, carrying the right key / listIndex / nodes.
    expect(callsFor("doctor_check")).toBe(1);
    expect(lastDoctorArgs?.items).toHaveLength(1);
    expect(lastDoctorArgs?.items[0].key).toBe("p0");
    expect(lastDoctorArgs?.items[0].listIndex).toBe(0);
    expect(lastDoctorArgs?.items[0].scene).toBeNull();
    expect(lastDoctorArgs?.items[0].nodes).toEqual([]);
    expect(lastDoctorArgs?.items[0].profileId).toBeNull();
    // No live-preset event fired in this test → no slot to restore.
    expect(lastDoctorArgs?.restoreListIndex).toBeNull();

    // The list never recalled a preset, and nothing wrote to the unit.
    expect(fired("load_preset_on_amp")).toBe(false);
    expect(fired("load_scene_on_amp")).toBe(false);
    expect(fired("level_preset")).toBe(false);
    expect(fired("copy_apply")).toBe(false);
  });

  it("remembers the chosen instrument for the preset in the app's own store", async () => {
    renderView(true);
    const user = userEvent.setup();
    await chooseAndCheck(user, 1);
    await chooseNoneAndRun(user, 1);
    await screen.findByRole(
      "button",
      { name: "Check other sounds" },
      { timeout: 3000 },
    );
    const calls = vi
      .mocked(invoke)
      .mock.calls.filter((c) => c[0] === "set_preset_instrument");
    expect(calls).toHaveLength(1);
    expect(calls[0][1]).toEqual({ slot: 0, profileId: "none" });
  });

  it("skips the Instrument page when the preset's instrument is saved — an explicit None counts", async () => {
    mockOnePreset({ store: { profile_by_slot: { 0: "none" } } });
    renderView(true);
    const user = userEvent.setup();
    await chooseAndCheck(user, 1);

    expect(
      await screen.findByRole(
        "button",
        { name: "Check other sounds" },
        { timeout: 3000 },
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText("Which instrument plays these?")).toBeNull();
    // The steps say the Instrument page was skipped.
    const steps = screen.getByRole("list", { name: "Steps" });
    expect(steps).toHaveTextContent("Check›Results");
    expect(steps).not.toHaveTextContent("Instrument");
    expect(callsFor("doctor_check")).toBe(1);
    expect(fired("set_preset_instrument")).toBe(false);
  });

  it("asks again when the saved instrument is no longer offered", async () => {
    mockOnePreset({ store: { profile_by_slot: { 0: "gone" } } });
    renderView(true);
    const user = userEvent.setup();
    await chooseAndCheck(user, 1);
    expect(
      await screen.findByText("Which instrument plays these?"),
    ).toBeInTheDocument();
    expect(fired("doctor_check")).toBe(false);
  });

  it("Back from the Instrument page keeps the ticks", async () => {
    renderView(true);
    const user = userEvent.setup();
    await chooseAndCheck(user, 1);
    await screen.findByText("Which instrument plays these?");
    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(
      await screen.findByRole("button", { name: "Check 1 sound" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("checkbox", { name: "Choose Studio Clean" }),
    ).toBeChecked();
  });

  it("includes a block-acting footswitch sound (footswitch set, scene null, FS tag)", async () => {
    mockOnePreset({ footswitch: true });
    renderView(true);
    const user = userEvent.setup();

    // Whole-row select includes the main sound + the footswitch child key → 2 sounds.
    await chooseAndCheck(user, 2);
    await chooseNoneAndRun(user, 2);
    expect(
      await screen.findByRole(
        "button",
        { name: "Check other sounds" },
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
    // The wire label names the preset; the name the backend writes into a sentence
    // is the one the screen uses.
    expect(fsItem?.label).toBe("Studio Clean · Solo");
    expect(fsItem?.soundName).toBe("footswitch 4");
    expect(items.find((it) => it.footswitch === null)?.soundName).toBe(
      "Main sound",
    );
  });

  // BUG→GATE: DoctorInput.footswitches must carry the FULL per-preset roster (incl. a
  // switch with zero level candidates), not `footswitchesPerIndex`'s levelable-only
  // subset — else the damage detector's offline force-bypass derivation can't see a
  // toggle-only switch at all. The SELECT list still shows only the levelable switch
  // as its own row (nothing to diagnose for a switch with no candidate as "a sound"),
  // but the sound(s) that DO run must still report the bare switch in their roster.
  it("DoctorInput.footswitches carries the FULL roster, not just levelable switches", async () => {
    mockOnePreset({ footswitch: true, bareFootswitch: true });
    renderView(true);
    const user = userEvent.setup();

    // Whole-row select: the main sound + the one SELECTABLE footswitch child (Solo) → 2
    // sounds. The bare Tuner switch has no level candidate, so it's not offered as its
    // own row — it should still ride along on every sound's `footswitches` roster.
    await chooseAndCheck(user, 2);
    await chooseNoneAndRun(user, 2);
    await screen.findByRole(
      "button",
      { name: "Check other sounds" },
      { timeout: 3000 },
    );

    const items = lastDoctorArgs?.items ?? [];
    expect(items.length).toBeGreaterThan(0);
    for (const it of items) {
      const labels = it.footswitches.map((f) => f.label);
      expect(labels).toContain("Solo");
      expect(labels).toContain("Tuner");
    }
  });

  it("fires cancel_doctor_check on unmount while a check is in flight", async () => {
    mockOnePreset({ hangCheck: true });
    const { unmount } = renderView(true);
    const user = userEvent.setup();

    await chooseAndCheck(user, 1);
    await chooseNoneAndRun(user, 1);
    expect(
      await screen.findByRole("heading", { name: "Checking 1 sound" }),
    ).toBeInTheDocument();

    // The check hangs — a tab switch (unmount) must cancel the orphaned run.
    expect(fired("cancel_doctor_check")).toBe(false);
    unmount();
    expect(fired("cancel_doctor_check")).toBe(true);
  });

  it("an all-clear check offers only Check other sounds, back to the list", async () => {
    renderView(true);
    const user = userEvent.setup();
    await chooseAndCheck(user, 1);
    await chooseNoneAndRun(user, 1);
    await screen.findByRole(
      "button",
      { name: "Check other sounds" },
      { timeout: 3000 },
    );
    // All clear: the footer's one action is "Check other sounds" — back to the list.
    expect(screen.queryByRole("button", { name: "Check again" })).toBeNull();
    await user.click(
      screen.getByRole("button", { name: "Check other sounds" }),
    );
    expect(
      await screen.findByRole("button", { name: "Check 1 sound" }),
    ).toBeInTheDocument();
  });
});
