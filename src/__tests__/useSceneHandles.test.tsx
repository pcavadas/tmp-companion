// src/__tests__/useSceneHandles.test.tsx — the Set-up step's scene handle candidate
// cache resolves INSTANT-FIRST off the startup backup scan (`scene_handles`), never
// firing a `list_scene_level_handles` device read for a preset the scan already covers.
// Unlike Base, the fallback fires only when the scan has NO ENTRY for the slot at all —
// an empty row is the correct, expected answer for a scene-less preset (see
// `useSceneHandles.ts`'s doc), so it must NOT trigger a device read.

import { describe, it, expect, beforeEach, vi } from "vitest";
import { renderHook, act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { useSceneHandles } from "../views/level/useSceneHandles";
import { blockStripName } from "../views/level/leveling";
import { ensureLibraryScan, resetLibraryScan } from "../device/libraryScan";
import type {
  BackupPresetRow,
  SceneHandleCandidate,
  SceneHandleRow,
} from "../lib/types";
import {
  makeBackupPresetRow,
  makeBackupRead,
  makeSceneOption,
  mockCommands,
} from "./commandMocks";
import { renderSetupPage, selectTrigger } from "./setupPageTestUtils";

function backupRow(
  slot: number,
  name: string,
  sceneHandles: SceneHandleRow[],
): BackupPresetRow {
  return makeBackupPresetRow({
    slot,
    name,
    scene_count: sceneHandles.length,
    scenes: sceneHandles.map(() => ({ name: "Scene", fs: null })),
    scene_handles: sceneHandles,
  });
}

const BOOST_CANDIDATE: SceneHandleCandidate = {
  groupId: "G1",
  nodeId: "boost",
  fenderId: "ACD_Boost",
  parameterId: "gain",
  class: "level_db",
  range: [0, 12],
  current: 5.0,
  scope: "isolated",
  headroom: "full",
};

async function seedScan(
  presets: BackupPresetRow[],
  deviceRows: SceneHandleRow[] = [],
) {
  mockCommands({
    read_library_via_backup: () =>
      makeBackupRead({ total_rows: presets.length, presets }),
    list_scene_level_handles: () => deviceRows,
  });
  await ensureLibraryScan();
}

describe("useSceneHandles — instant-first with device fallback", () => {
  beforeEach(() => {
    resetLibraryScan();
    vi.mocked(invoke).mockReset();
  });

  it("resolves backup-derived candidates with no list_scene_level_handles invoke", async () => {
    await seedScan([
      backupRow(1, "Friedman HBE", [
        {
          sceneSlot: 0,
          candidates: [BOOST_CANDIDATE],
          allCandidates: [BOOST_CANDIDATE],
        },
      ]),
    ]);
    const { result } = renderHook(() => useSceneHandles());

    act(() => {
      result.current.prefetch(0);
    });
    await waitFor(() => {
      expect(result.current.candidatesFor(0, 0).status).toBe("resolved");
    });

    const st = result.current.candidatesFor(0, 0);
    expect(st.status).toBe("resolved");
    if (st.status === "resolved") {
      expect(st.candidates).toEqual([BOOST_CANDIDATE]);
      expect(st.allCandidates).toEqual([BOOST_CANDIDATE]);
    }
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith(
      "list_scene_level_handles",
      expect.anything(),
    );
  });

  it("does NOT fall back for a scene-less preset — an empty row is a legitimate answer", async () => {
    await seedScan([backupRow(1, "Studio Clean", [])]);
    const { result } = renderHook(() => useSceneHandles());

    act(() => {
      result.current.prefetch(0);
    });
    await waitFor(() => {
      expect(result.current.candidatesFor(0, 0).status).toBe("resolved");
    });

    // Nothing to find at any scene slot, but the fetch resolved off the backup —
    // never fired the device command for a preset the scan legitimately covers.
    expect(result.current.candidatesFor(0, 0).status).toBe("resolved");
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith(
      "list_scene_level_handles",
      expect.anything(),
    );
  });

  it("falls back to list_scene_level_handles for a slot the scan never reached", async () => {
    // The scan only covers slot 0 (device slot 1) — slot 5 has no map entry at all.
    await seedScan(
      [backupRow(1, "Friedman HBE", [])],
      [
        {
          sceneSlot: 0,
          candidates: [BOOST_CANDIDATE],
          allCandidates: [BOOST_CANDIDATE],
        },
      ],
    );
    const { result } = renderHook(() => useSceneHandles());

    act(() => {
      result.current.prefetch(5);
    });
    await waitFor(() => {
      expect(result.current.candidatesFor(5, 0).status).toBe("resolved");
    });

    const st = result.current.candidatesFor(5, 0);
    expect(st.status).toBe("resolved");
    if (st.status === "resolved") {
      expect(st.candidates).toEqual([BOOST_CANDIDATE]);
    }
    expect(invoke).toHaveBeenCalledWith("list_scene_level_handles", {
      slot: 5,
    });
  });

  it("Set up's Block list never shows 'Reading knobs…' on the backup path", async () => {
    await seedScan([
      backupRow(1, "Friedman HBE", [
        {
          sceneSlot: 0,
          candidates: [BOOST_CANDIDATE],
          allCandidates: [BOOST_CANDIDATE],
        },
      ]),
    ]);
    const user = userEvent.setup();
    renderSetupPage([
      makeSceneOption(0, { presetName: "Friedman HBE", sceneName: "Solo" }),
    ]);

    await user.click(selectTrigger("block:s0:0"));
    // Already resolved by open time: no "Reading knobs…", the scene's block is listed
    // at once, and one click picks it, landing on its best knob.
    expect(screen.queryByText("Reading knobs…")).toBeNull();
    const boost = blockStripName("ACD_Boost");
    await user.click(await screen.findByRole("option", { name: boost }));
    expect(selectTrigger("block:s0:0")).toHaveAccessibleName(
      `Block for Friedman HBE, Solo: ${boost}`,
    );
    expect(selectTrigger("knob:s0:0")).toHaveAccessibleName(
      "Knob for Friedman HBE, Solo: Gain",
    );
    expect(invoke).not.toHaveBeenCalledWith("list_scene_level_handles", {
      slot: 0,
    });
  });
});
