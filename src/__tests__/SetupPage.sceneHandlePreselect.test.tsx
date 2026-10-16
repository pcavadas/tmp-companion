// src/__tests__/SetupPage.sceneHandlePreselect.test.tsx — issue 5: a scene whose
// overlay un-bypasses a block the base graph keeps bypassed (Solo un-bypassing
// ACD_Boost, the plan's own example) should default its leveling handle to THAT
// block's own control, not the amp's output level — the amp barely moves the sound the
// scene actually turns on. `SceneHandleCandidate.enablesBlock` is the backend's
// signal; SetupPage must preselect the first such candidate once the row's
// candidates resolve (they arrive ASYNC — the warm effect's backup-sourced fetch
// still takes a microtask — so the mount `useState` initializer alone can't see it),
// never override an explicit user choice (including the pseudo-option, itself
// `null`), and never pick anything outside the row's own resolved candidate list.

import { describe, it, expect, beforeEach, vi } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { blockStripName } from "../views/level/leveling";
import { ensureLibraryScan, resetLibraryScan } from "../device/libraryScan";
import type { SceneHandleCandidate, SceneHandleRow } from "../lib/types";
import {
  makeBackupPresetRow,
  makeBackupRead,
  makeSceneOption,
  mockCommands,
} from "./commandMocks";
import type { SetupOption } from "../views/level/leveling";
import { choose, renderSetupPage, selectTrigger } from "./setupPageTestUtils";

const BOOST = blockStripName("ACD_Boost");

function boostCandidate(enablesBlock: boolean): SceneHandleCandidate {
  return {
    groupId: "g1",
    nodeId: "boost0",
    fenderId: "ACD_Boost",
    parameterId: "gain",
    class: "level_db",
    range: [0, 12],
    current: 6,
    scope: "isolated",
    headroom: "full",
    enablesBlock,
  };
}

function seedScan(rows: SceneHandleRow[]) {
  mockCommands({
    read_library_via_backup: () =>
      makeBackupRead({
        total_rows: 1,
        presets: [
          makeBackupPresetRow({
            slot: 1, // device slot 1 = list index 0
            name: "Friedman HBE",
            scene_count: 1,
            base_active_amp_count: 1,
            scene_handles: rows,
          }),
        ],
      }),
    list_scene_level_handles: () => rows,
  });
  return ensureLibraryScan();
}

const sceneOpt: SetupOption = makeSceneOption(0, {
  presetName: "Friedman HBE",
  sceneName: "Solo",
});

const blockOf = () => selectTrigger("block:s0:0");
const knobOf = () => selectTrigger("knob:s0:0");

describe("SetupPage — scene handle preselect (issue 5)", () => {
  beforeEach(() => {
    resetLibraryScan();
    vi.mocked(invoke).mockReset();
  });

  it("preselects the enabling block's knob once candidates resolve", async () => {
    await seedScan([
      {
        sceneSlot: 0,
        candidates: [boostCandidate(true)],
        allCandidates: [boostCandidate(true)],
      },
    ]);
    renderSetupPage([sceneOpt]);

    // Shows the enabling block, not the amp default, once the candidates land.
    await waitFor(() => {
      expect(blockOf()).toHaveAccessibleName(
        `Block for Friedman HBE, Solo: ${BOOST}`,
      );
    });
    expect(knobOf()).toHaveAccessibleName("Knob for Friedman HBE, Solo: Gain");
  });

  it("never overrides an explicit user choice of the amp default", async () => {
    await seedScan([
      {
        sceneSlot: 0,
        candidates: [boostCandidate(true)],
        allCandidates: [boostCandidate(true)],
      },
    ]);
    const user = userEvent.setup();
    renderSetupPage([sceneOpt]);
    await waitFor(() => {
      expect(blockOf()).toHaveAccessibleName(
        `Block for Friedman HBE, Solo: ${BOOST}`,
      );
    });

    // Explicitly pick the amp default — the preselect must not take it back.
    await choose(user, "block:s0:0", "default");
    expect(blockOf()).toHaveAccessibleName("Block for Friedman HBE, Solo: Amp");
    expect(knobOf()).toHaveAccessibleName(
      "Knob for Friedman HBE, Solo: Output level",
    );
  });

  it("stays on the amp default when no candidate enables a block", async () => {
    await seedScan([
      {
        sceneSlot: 0,
        candidates: [boostCandidate(false)],
        allCandidates: [boostCandidate(false)],
      },
    ]);
    const user = userEvent.setup();
    renderSetupPage([sceneOpt]);

    // Open the list and wait for the (non-enabling) Boost to appear — proof the
    // candidates actually resolved, not just that nothing rendered yet.
    await user.click(blockOf());
    await screen.findByRole("option", { name: BOOST });
    expect(blockOf()).toHaveAccessibleName("Block for Friedman HBE, Solo: Amp");
  });
});
