// src/__tests__/SetupBodyBackupWarm.test.tsx — Set-up step's eager warm effect (Base +
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
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ThemeProvider } from "../theme/ThemeProvider";
import { SetupBody } from "../views/overlays/SetupBody";
import { WithCard } from "./pickCardTestUtils";
import {
  ensureLibraryScan,
  resetLibraryScan,
} from "../views/level/libraryScan";
import type {
  ActiveGraph,
  BackupReadResult,
  SceneHandleCandidate,
} from "../lib/types";
import type { SetupOption } from "../views/level/leveling";
import type { PickOption } from "../views/overlays/Pick";

vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(() => undefined),
}));

const emptyGraph: ActiveGraph = {
  name: null,
  slot: null,
  template: null,
  split_mix: null,
  nodes: [],
  stages: [],
};

function backupRow(
  slot: number,
  name: string,
  baseHandles: SceneHandleCandidate[],
) {
  return {
    slot,
    name,
    scene_count: 0,
    scenes: [],
    amp_candidates: [],
    base_active_amp_count: 0,
    blocks: [],
    graph: emptyGraph,
    footswitches: [],
    silence_hint: null,
    scene_handles: [],
    base_handles: baseHandles,
  };
}

async function seedScan(
  presets: ReturnType<typeof backupRow>[],
  deviceBlocks: unknown[] = [],
) {
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === "read_library_via_backup") {
      const result: BackupReadResult = {
        members: [],
        db_bytes: 0,
        total_rows: presets.length,
        scene_mode: "test",
        presets,
        song_presets: [],
        songs: [],
        setlists: [],
        setlist_songs: [],
      };
      return Promise.resolve(result);
    }
    if (cmd === "list_level_blocks") return Promise.resolve(deviceBlocks);
    if (cmd === "list_scene_level_handles") return Promise.resolve([]);
    return Promise.resolve(null);
  });
  await ensureLibraryScan();
}

const baseOpt: SetupOption = {
  key: "p0",
  slot: 0,
  presetName: "Clean",
  isBase: true,
  sceneSlot: null,
  sceneName: "Whole preset",
  tag: null,
  hasScenes: false,
};

const instrumentOptions: PickOption[] = [{ id: "none", label: "None" }];
const targetOptions: PickOption[] = [{ id: "Rhythm", label: "Rhythm −18" }];

function renderSetup(options: SetupOption[]) {
  return render(
    <ThemeProvider>
      <WithCard>
        <SetupBody
          options={options}
          presetCount={options.length}
          isRelevel={false}
          instrumentOptions={instrumentOptions}
          targetOptions={targetOptions}
          defaultInst="none"
          defaultTarget="Rhythm"
          onCancel={vi.fn()}
          onStart={vi.fn()}
        />
      </WithCard>
    </ThemeProvider>,
  );
}

const deviceCallsAgainst = (cmd: string) =>
  vi.mocked(invoke).mock.calls.filter(([c]) => c === cmd);

describe("SetupBody's eager warm effect stays provably device-free", () => {
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
    // The block dropdown lands with the one fallback-fetched block (Tweed Deluxe,
    // rendered by its catalog full name) — pick it (the row's only, hence best-
    // ranked, candidate) to reach the CONTROL dropdown's trigger, which now shows
    // the picked param.
    const blockRow = await screen.findByText("FENDER '57 DELUXE");
    await user.click(blockRow);
    expect(screen.getByText("Output level")).toBeInTheDocument();
  });
});
