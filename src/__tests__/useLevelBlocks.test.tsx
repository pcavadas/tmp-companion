// src/__tests__/useLevelBlocks.test.tsx — the Set-up step's Base handle candidate cache
// resolves INSTANT-FIRST off the startup backup scan (`base_handles`), never firing a
// `list_level_blocks` device read for a preset the scan already covers. The fallback fires
// only when the scan has NO ENTRY for the slot at all — an empty row is the correct,
// expected answer for a genuinely blockless/unparseable preset (mirrors
// `useSceneHandles.ts`'s own discriminator: MAP KEY PRESENCE, not list emptiness).

import { describe, it, expect, beforeEach, vi } from "vitest";
import { renderHook, act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { useLevelBlocks } from "../views/level/useLevelBlocks";
import { blockStripName } from "../views/level/leveling";
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
import { renderSetupPage, selectTrigger } from "./setupPageTestUtils";

function backupRow(
  slot: number,
  name: string,
  baseHandles: SceneHandleCandidate[],
): BackupPresetRow {
  return makeBackupPresetRow({ slot, name, base_handles: baseHandles });
}

const AMP_CANDIDATE: SceneHandleCandidate = {
  groupId: "G1",
  nodeId: "amp",
  fenderId: "ACD_TwinReverb65NoFx",
  parameterId: "outputLevel",
  class: "level_linear",
  range: [0, 1],
  current: 0.42,
  scope: "isolated",
  headroom: "full",
};

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

describe("useLevelBlocks — instant-first with device fallback", () => {
  beforeEach(() => {
    resetLibraryScan();
    vi.mocked(invoke).mockReset();
  });

  it("resolves backup-derived candidates with no list_level_blocks invoke", async () => {
    await seedScan([backupRow(1, "Stadium Lead", [AMP_CANDIDATE])]);
    const { result } = renderHook(() => useLevelBlocks());

    act(() => {
      result.current.prefetch(0);
    });
    await waitFor(() => {
      expect(result.current.blocksFor(0).status).toBe("resolved");
    });

    const st = result.current.blocksFor(0);
    expect(st.status).toBe("resolved");
    if (st.status === "resolved") {
      expect(st.blocks).toEqual([
        {
          group_id: "G1",
          node_id: "amp",
          model_id: "ACD_TwinReverb65NoFx",
          parameter_id: "outputLevel",
          paramClass: "level_linear",
          headroom: "full",
        },
      ]);
    }
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith(
      "list_level_blocks",
      expect.anything(),
    );
  });

  it("does NOT fall back for a genuinely blockless preset — an empty row is a legitimate answer", async () => {
    await seedScan([backupRow(1, "No Handles", [])]);
    const { result } = renderHook(() => useLevelBlocks());

    act(() => {
      result.current.prefetch(0);
    });
    await waitFor(() => {
      expect(result.current.blocksFor(0).status).toBe("resolved");
    });

    const st = result.current.blocksFor(0);
    expect(st.status).toBe("resolved");
    if (st.status === "resolved") expect(st.blocks).toEqual([]);
    // Nothing to find, but the fetch resolved off the backup — never fired the
    // device command for a preset the scan legitimately covers.
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith(
      "list_level_blocks",
      expect.anything(),
    );
  });

  it("falls back to list_level_blocks for a slot the scan never reached", async () => {
    // The scan only covers slot 0 (device slot 1) — slot 5 has no map entry at all.
    await seedScan(
      [backupRow(1, "Friedman HBE", [])],
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
    const { result } = renderHook(() => useLevelBlocks());

    act(() => {
      result.current.prefetch(5);
    });
    await waitFor(() => {
      expect(result.current.blocksFor(5).status).toBe("resolved");
    });

    const st = result.current.blocksFor(5);
    expect(st.status).toBe("resolved");
    if (st.status === "resolved") {
      expect(st.blocks).toEqual([
        {
          group_id: "amp",
          node_id: "amp0",
          model_id: "ACD_TweedDeluxe",
          parameter_id: "outputLevel",
        },
      ]);
    }
    expect(invoke).toHaveBeenCalledWith("list_level_blocks", { slot: 5 });
  });

  it("hasBackupData: true for a present (even empty) key, false for an absent one", async () => {
    await seedScan([backupRow(1, "No Handles", [])]);
    const { result } = renderHook(() => useLevelBlocks());

    expect(result.current.hasBackupData(0)).toBe(true);
    expect(result.current.hasBackupData(5)).toBe(false);
  });

  it("Set up's Block list never shows 'Reading knobs…' on the backup path", async () => {
    await seedScan([backupRow(1, "Stadium Lead", [AMP_CANDIDATE])]);
    const user = userEvent.setup();
    renderSetupPage([makeSetupOption({ presetName: "Stadium Lead" })]);

    await user.click(selectTrigger("block:p0"));
    // The candidate is already resolved by open time (the backup scan ran in `seedScan`,
    // well before this click): no "Reading knobs…", the block is listed at once, and
    // one click picks it, landing on its best knob.
    expect(screen.queryByText("Reading knobs…")).toBeNull();
    const twin = blockStripName("ACD_TwinReverb65NoFx");
    await user.click(await screen.findByRole("option", { name: twin }));
    expect(selectTrigger("block:p0")).toHaveAccessibleName(
      `Block for Stadium Lead, Main sound: ${twin}`,
    );
    expect(selectTrigger("knob:p0")).toHaveAccessibleName(
      "Knob for Stadium Lead, Main sound: Output level",
    );
    expect(invoke).not.toHaveBeenCalledWith("list_level_blocks", {
      slot: 0,
    });
  });
});
