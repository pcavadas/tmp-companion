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
//  - the LAZY fallback (today's per-row `onOpen` path, untouched by this fix) still fires
//    when the user actually opens a key-absent row's own picker.

import { describe, it, expect, beforeEach, vi } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { SetupPage } from "../views/level/SetupPage";
import { WithCard } from "./pickCardTestUtils";
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
import type { SetupOption } from "../views/level/leveling";
import type { PickOption } from "../views/overlays/Pick";
import { renderWithTheme } from "./testKit";

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
  sceneName: "Whole preset",
  tag: null,
  hasScenes: false,
});

const instrumentOptions: PickOption[] = [{ id: "none", label: "None" }];
const targetOptions: PickOption[] = [{ id: "Rhythm", label: "Rhythm −18" }];

function renderSetup(options: SetupOption[]) {
  return renderWithTheme(
    <WithCard>
      <SetupPage
        options={options}
        isRelevel={false}
        instrumentOptions={instrumentOptions}
        targetOptions={targetOptions}
        defaultInst="none"
        defaultTarget="Rhythm"
        onCancel={vi.fn()}
        onStart={vi.fn()}
      />
    </WithCard>,
  );
}

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
    renderSetup([baseOpt]);

    // Let the warm effect's microtasks settle — nothing should have fired.
    await waitFor(() => {
      expect(screen.getByText("Preset level")).toBeInTheDocument();
    });
    expect(deviceCallsAgainst("list_level_blocks")).toHaveLength(0);
    expect(deviceCallsAgainst("list_scene_level_handles")).toHaveLength(0);
  });

  it("a present but empty backup entry renders an empty picker with no device call", async () => {
    await seedScan([backupRow(1, "No Handles", [])]);
    const user = userEvent.setup();
    renderSetup([baseOpt]);

    await user.click(screen.getByText("Preset level"));
    // Only the pseudo default shows — no real candidate row, no loading skeleton.
    expect(
      await screen.findByText("Preset level (default)"),
    ).toBeInTheDocument();
    expect(screen.queryByText("Loading controls…")).toBeNull();
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
    renderSetup([baseOpt]);

    // Nothing yet — the warm effect never called prefetch for this key-absent slot.
    expect(deviceCallsAgainst("list_level_blocks")).toHaveLength(0);

    await user.click(screen.getByText("Preset level"));
    await waitFor(() => {
      expect(deviceCallsAgainst("list_level_blocks")).toHaveLength(1);
    });
    expect(invoke).toHaveBeenCalledWith("list_level_blocks", { slot: 0 });
    // The flattened list lands with the one fallback-fetched block's own candidate
    // row (Tweed Deluxe, rendered by its catalog full name + param) — one click
    // picks it directly (no separate block/control stage).
    const row = await screen.findByText("FENDER '57 DELUXE — Output level");
    await user.click(row);
    expect(
      screen.getByText("FENDER '57 DELUXE — Output level"),
    ).toBeInTheDocument();
  });
});
