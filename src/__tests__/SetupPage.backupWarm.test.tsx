// src/__tests__/SetupPage.backupWarm.test.tsx — Set-up step's eager warm effect (Base +
// Scene) must be provably device-free: it may only populate a row's picker cache from the
// startup backup scan's maps, gated on MAP KEY PRESENCE (`hasBackupData`), never by
// unconditionally calling the device-fallback-capable `prefetch` for every group on render.
//
// Three shapes, one bug class each:
//  - a "ready but failed" scan (every map empty — key ABSENT for every slot) must not make
//    Set-up's render fire a single `list_level_blocks`/`list_scene_level_handles` call —
//    the old effect called `prefetch` unconditionally per group, which WOULD have reached
//    the device for every row of a failed scan.
//  - a slot whose map entry is a present but EMPTY array (a genuinely blockless/scene-less
//    preset) still renders an empty picker with no device call — the warm effect's own
//    `hasBackupData` check is satisfied, `prefetch` runs, and the fetchFn resolves purely
//    off the backup with nothing to show.
//  - the LAZY fallback (the row's Block/Knob `onOpen` path, untouched by this fix) still
//    fires when the user actually opens a key-absent row's own dropdown.

import { describe, it, expect, beforeEach, vi } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ensureLibraryScan, resetLibraryScan } from "../device/libraryScan";
import type {
  BackupPresetRow,
  LevelBlock,
  SceneHandleCandidate,
} from "../lib/types";
import {
  makeBackupPresetRow,
  makeBackupRead,
  makeSetupOption,
  mockCommands,
} from "./commandMocks";
import { blockStripName, type SetupOption } from "../views/level/leveling";
import { renderSetupPage, selectTrigger } from "./setupPageTestUtils";

function backupRow(
  slot: number,
  name: string,
  baseHandles: SceneHandleCandidate[],
): BackupPresetRow {
  return makeBackupPresetRow({ slot, name, base_handles: baseHandles });
}

async function seedScan(
  presets: BackupPresetRow[],
  deviceBlocks: LevelBlock[] = [],
) {
  mockCommands({
    read_library_via_backup: () =>
      makeBackupRead({ total_rows: presets.length, presets }),
    list_level_blocks: () => deviceBlocks,
  });
  await ensureLibraryScan();
}

const baseOpt: SetupOption = makeSetupOption({
  presetName: "Clean",
  tag: null,
  hasScenes: false,
});

const deviceCallsAgainst = (cmd: string) =>
  vi.mocked(invoke).mock.calls.filter(([c]) => c === cmd);

describe("SetupPage's eager warm effect stays provably device-free", () => {
  beforeEach(() => {
    resetLibraryScan();
    vi.mocked(invoke).mockReset();
  });

  it("a ready-but-failed scan (every map key absent) fires no per-slot device calls on render", async () => {
    // No presets at all — every map (`baseHandlesByIndex` included) stays empty, so
    // EVERY slot's key is absent (the failed-scan shape: `ready: true`, maps empty).
    await seedScan([]);
    renderSetupPage([baseOpt]);

    // Let the warm effect's microtasks settle — nothing should have fired.
    await waitFor(() => {
      expect(selectTrigger("block:p0")).toHaveAccessibleName(
        "Block for Clean, Main sound: Preset",
      );
    });
    expect(deviceCallsAgainst("list_level_blocks")).toHaveLength(0);
    expect(deviceCallsAgainst("list_scene_level_handles")).toHaveLength(0);
  });

  it("a present but empty backup entry lists only the default, with no device call", async () => {
    await seedScan([backupRow(1, "No Handles", [])]);
    const user = userEvent.setup();
    renderSetupPage([baseOpt]);

    await user.click(selectTrigger("block:p0"));
    // Only the default shows — no block of its own, no "Reading knobs…".
    const options = await screen.findAllByRole("option");
    expect(options.map((o) => o.textContent)).toEqual(["Preset"]);
    expect(screen.queryByText("Reading knobs…")).toBeNull();
    expect(deviceCallsAgainst("list_level_blocks")).toHaveLength(0);
  });

  it("the lazy onOpen fallback still fires for a key-absent slot the warm effect skipped", async () => {
    await seedScan(
      [], // key absent for slot 0
      [
        {
          group_id: "amp",
          node_id: "amp0",
          model_id: "ACD_TweedDeluxe",
          parameter_id: "outputLevel",
          value: 0.7,
        },
      ],
    );
    const user = userEvent.setup();
    renderSetupPage([baseOpt]);

    // Nothing yet — the warm effect never called prefetch for this key-absent slot.
    expect(deviceCallsAgainst("list_level_blocks")).toHaveLength(0);

    await user.click(selectTrigger("block:p0"));
    await waitFor(() => {
      expect(deviceCallsAgainst("list_level_blocks")).toHaveLength(1);
    });
    expect(invoke).toHaveBeenCalledWith("list_level_blocks", { slot: 0 });
    // The device-read block lands in the open list; picking it lands on its knob.
    const tweed = blockStripName("ACD_TweedDeluxe");
    await user.click(await screen.findByRole("option", { name: tweed }));
    expect(selectTrigger("block:p0")).toHaveAccessibleName(
      `Block for Clean, Main sound: ${tweed}`,
    );
    expect(selectTrigger("knob:p0")).toHaveAccessibleName(
      "Knob for Clean, Main sound: Output level",
    );
  });
});
