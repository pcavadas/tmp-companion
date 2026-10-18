// src/__tests__/footswitch-roster-separation.test.tsx — BUG→GATE: pins the
// Level/Doctor footswitch-roster SEPARATION through the REAL wiring (backup mock →
// `libraryScan` → `usePresetData` → the view), not via directly-injected props.
//
// Level shows EVERY footswitch (one with no level-class parameter, e.g. "PHASER", is
// shown disabled with a reason, never dropped); Doctor's SELECT list stays
// levelable-only (`DoctorView.tsx`'s `handleCheck`). `footswitchesPerIndex`
// (`libraryScan.ts`) is threaded to BOTH tabs, so it stays levelable-only, and Level opts
// into the full roster per CALLER (`usePresetData`'s `footswitchRoster: "all"`). This spec
// renders BOTH real views off the SAME backup fixture (one levelable "Solo" + one bare
// "Tuner" switch) and asserts the two lists disagree exactly as intended — so a change
// that re-collapses them (by widening the shared map, or by dropping the option) fails
// here.

import { describe, it, expect, vi } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { LevelView } from "../views/level";
import { DoctorView } from "../views/doctor";
import { ensureLibraryScan, resetLibraryScan } from "../device/libraryScan";
import { resetLiveDevice } from "../device/useLiveDevice";
import type { FootswitchInfo } from "../lib/types";
import {
  forceTauri,
  makeBackupPresetRow,
  makeBackupRead,
  makeStore,
  mockCommands,
} from "./commandMocks";
import { renderWithTheme } from "./testKit";

// In-app: the live bridges and the library scan subscribe.
forceTauri();

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

// No level_params at all — the real incident's "PHASER" shape, standing in as
// "Tuner" here (mirrors DoctorView.test.tsx's BARE_FOOTSWITCH fixture).
const BARE_FOOTSWITCH: FootswitchInfo = {
  switch: 5, // → tag FS6
  label: "Tuner",
  link_group: null,
  functions: [],
  level_params: [],
};

function mockPresetWithBothFootswitches() {
  mockCommands({
    list_presets: () => [{ slot: 0, name: "Studio Clean" }],
    get_store: () => makeStore({ targets: [{ name: "Rhythm", lufs: -26 }] }),
    read_library_via_backup: () =>
      makeBackupRead({
        total_rows: 1,
        presets: [
          makeBackupPresetRow({
            slot: 1, // device slot 1 = list index 0
            name: "Studio Clean",
            footswitches: [SOLO_FOOTSWITCH, BARE_FOOTSWITCH],
          }),
        ],
      }),
  });
}

function freshConnection() {
  vi.mocked(invoke).mockClear();
  resetLibraryScan();
  resetLiveDevice();
}

describe("footswitch roster separation — Level vs. Doctor (BUG 1 regression guard)", () => {
  it("Level's list shows the no-level-control switch too — disabled, with a reason", async () => {
    freshConnection();
    mockPresetWithBothFootswitches();
    const user = userEvent.setup();
    renderWithTheme(<LevelView connected={true} />);
    void ensureLibraryScan();

    await screen.findByText("Studio Clean");
    await user.click(
      await screen.findByRole("button", { name: "Open Studio Clean" }),
    );

    // Both switches render as rows…
    expect(await screen.findByText("Solo")).toBeInTheDocument();
    expect(await screen.findByText("Tuner")).toBeInTheDocument();
    // …but "Tuner" is disabled with a short reason (the required behaviour),
    // never silently omitted.
    expect(screen.getByText("No volume knob to level")).toBeInTheDocument();

    // And it must not be COUNTED as selectable: ticking the whole preset (Base +
    // every child `childKeys` considers selectable) must read as fully checked, not
    // indeterminate — if "Tuner" were still counted, it could never be ticked
    // (disabled), so the preset checkbox would be stuck "N of M" forever.
    const presetCheckbox = screen.getByRole("checkbox", {
      name: "Choose Studio Clean",
    });
    await user.click(presetCheckbox);
    expect(presetCheckbox).toHaveAttribute("aria-checked", "true");
  });

  it("Doctor's list shows ONLY the levelable switch — the bare one never appears at all", async () => {
    freshConnection();
    mockPresetWithBothFootswitches();
    const user = userEvent.setup();
    renderWithTheme(<DoctorView connected={true} />);
    void ensureLibraryScan();

    await screen.findByText("Studio Clean");
    await user.click(
      await screen.findByRole("button", { name: "Open Studio Clean" }),
    );

    // The levelable switch is offered as its own checkable "sound"…
    expect(await screen.findByText("Solo")).toBeInTheDocument();
    // …the bare one is not shown here at all (not even disabled) — Doctor's SELECT
    // list stays levelable-only.
    expect(screen.queryByText("Tuner")).toBeNull();
    expect(screen.queryByText("No volume knob to level")).toBeNull();
  });
});
