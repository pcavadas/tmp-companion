// src/__tests__/footswitch-roster-separation.test.tsx — BUG→GATE: pins the
// Level/Doctor footswitch-roster SEPARATION through the REAL wiring (backup mock →
// `libraryScan` → `usePresetData` → the view), not via directly-injected props.
//
// History: the BUG 1 fix (a footswitch with no level-class parameter, e.g. "PHASER",
// must not silently vanish from the Level list) was first implemented by widening
// `footswitchesPerIndex` itself in `libraryScan.ts`. That map is threaded to BOTH
// tabs — Level's own list AND Doctor's SELECT list (`DoctorView.tsx`'s `handleCheck`
// deliberately reuses the levelable-only filter, per its own "ponytail" comment) — so
// widening it silently changed what Doctor offers for diagnosis too, with no test
// catching it. The fix instead moved to a per-CALLER option on `usePresetData`
// (`footswitchRoster: "all"` for Level, the default "levelable" for Doctor), and
// `footswitchesPerIndex` itself stayed levelable-only. This spec renders BOTH real
// views off the SAME backup fixture (one levelable "Solo" + one bare "Tuner" switch)
// and asserts the two lists disagree exactly as intended — so a future change that
// re-collapses them (by widening the shared map, or by dropping the option) fails
// here instead of needing another by-hand catch.

import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ThemeProvider } from "../theme/ThemeProvider";

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

// Imported AFTER the mocks so both views pick up the forced isTauri().
import { LevelView } from "../views/level";
import { DoctorView } from "../views/doctor";
import {
  ensureLibraryScan,
  resetLibraryScan,
} from "../views/level/libraryScan";
import { resetLiveDevice } from "../views/level/useLiveDevice";

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
      class: "level_linear",
    },
  ],
};

// No level_params at all — the real incident's "PHASER" shape, standing in as
// "Tuner" here (mirrors DoctorView.test.tsx's BARE_FOOTSWITCH fixture).
const BARE_FOOTSWITCH = {
  switch: 5, // → tag FS6
  label: "Tuner",
  link_group: null,
  functions: [],
  level_params: [],
};

function mockPresetWithBothFootswitches() {
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
              slot: 1, // device slot 1 = list index 0
              name: "Studio Clean",
              scene_count: 0,
              scenes: [],
              blocks: [],
              footswitches: [SOLO_FOOTSWITCH, BARE_FOOTSWITCH],
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

function freshConnection() {
  vi.mocked(invoke).mockClear();
  listeners.clear();
  resetLibraryScan();
  resetLiveDevice();
}

describe("footswitch roster separation — Level vs. Doctor (BUG 1 regression guard)", () => {
  it("Level's list shows the no-level-control switch too — disabled, with a reason", async () => {
    freshConnection();
    mockPresetWithBothFootswitches();
    const user = userEvent.setup();
    render(
      <ThemeProvider>
        <LevelView connected={true} />
      </ThemeProvider>,
    );
    void ensureLibraryScan();

    await screen.findByText("Studio Clean");
    await user.click(await screen.findByTitle("Show Base Preset + sounds"));

    // Both switches render as rows…
    expect(await screen.findByText("Solo")).toBeInTheDocument();
    expect(await screen.findByText("Tuner")).toBeInTheDocument();
    // …but "Tuner" is disabled with a short reason (BUG 1's required behaviour),
    // never silently omitted.
    expect(screen.getByText(/no level control/i)).toBeInTheDocument();

    // And it must not be COUNTED as selectable: ticking the whole preset (Base +
    // every child `childKeys` considers selectable) must read as fully checked, not
    // indeterminate — if "Tuner" were still counted, it could never be ticked
    // (disabled), so the preset checkbox would be stuck "N of M" forever.
    const presetCheckboxWrap = screen.getAllByTitle(
      "Select preset to level",
    )[0];
    await user.click(presetCheckboxWrap);
    expect(
      presetCheckboxWrap.querySelector('[role="checkbox"]'),
    ).toHaveAttribute("aria-checked", "true");
  });

  it("Doctor's list shows ONLY the levelable switch — the bare one never appears at all", async () => {
    freshConnection();
    mockPresetWithBothFootswitches();
    const user = userEvent.setup();
    render(
      <ThemeProvider>
        <DoctorView connected={true} />
      </ThemeProvider>,
    );
    void ensureLibraryScan();

    await screen.findByText("Studio Clean");
    await user.click(await screen.findByTitle("Show Base Preset + sounds"));

    // The levelable switch is offered as its own checkable "sound"…
    expect(await screen.findByText("Solo")).toBeInTheDocument();
    // …the bare one is not shown here at all (not even disabled) — Doctor's SELECT
    // list stays exactly what it was before the Level-side fix.
    expect(screen.queryByText("Tuner")).toBeNull();
    expect(screen.queryByText(/no level control/i)).toBeNull();
  });
});
