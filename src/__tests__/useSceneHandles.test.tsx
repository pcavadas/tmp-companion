// src/__tests__/useSceneHandles.test.tsx — the Set-up step's scene handle candidate
// cache resolves INSTANT-FIRST off the startup backup scan (`scene_handles`), never
// firing a `list_scene_level_handles` device read for a preset the scan already covers.
// Unlike Base, the fallback fires only when the scan has NO ENTRY for the slot at all —
// an empty row is the correct, expected answer for a scene-less preset (see
// `useSceneHandles.ts`'s doc), so it must NOT trigger a device read.

import { useState } from "react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  renderHook,
  act,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ThemeProvider } from "../theme/ThemeProvider";
import {
  BlockLevelPick,
  type BlockLevelHandle,
} from "../views/overlays/BlockLevelPick";
import { WithCard } from "./pickCardTestUtils";
import { useSceneHandles } from "../views/level/useSceneHandles";
import {
  ensureLibraryScan,
  resetLibraryScan,
} from "../views/level/libraryScan";
import type {
  ActiveGraph,
  BackupReadResult,
  SceneHandleCandidate,
  SceneHandleRow,
} from "../lib/types";

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

function backupRow(slot: number, name: string, sceneHandles: SceneHandleRow[]) {
  return {
    slot,
    name,
    scene_count: sceneHandles.length,
    scenes: sceneHandles.map(() => ({ name: "Scene", fs: null })),
    amp_candidates: [],
    base_active_amp_count: 0,
    blocks: [],
    graph: emptyGraph,
    footswitches: [],
    silence_hint: null,
    scene_handles: sceneHandles,
    base_handles: [],
  };
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
  presets: ReturnType<typeof backupRow>[],
  deviceRows: unknown[] = [],
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
    if (cmd === "list_scene_level_handles") return Promise.resolve(deviceRows);
    return Promise.resolve(null);
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

  it("never reaches BlockLevelPick's 'Loading controls…' state on the backup path", async () => {
    await seedScan([
      backupRow(1, "Friedman HBE", [
        {
          sceneSlot: 0,
          candidates: [BOOST_CANDIDATE],
          allCandidates: [BOOST_CANDIDATE],
        },
      ]),
    ]);

    function Harness() {
      const { prefetch, candidatesFor } = useSceneHandles();
      const [handle, setHandle] = useState<BlockLevelHandle | null>(null);
      const st = candidatesFor(0, 0);
      const candidates =
        st.status === "resolved"
          ? {
              status: "resolved" as const,
              list: st.allCandidates.map((c) => ({
                groupId: c.groupId,
                nodeId: c.nodeId,
                fenderId: c.fenderId,
                parameterId: c.parameterId,
                paramClass: c.class,
              })),
            }
          : { status: st.status };
      return (
        <BlockLevelPick
          pseudoLabel="Amp output level"
          handle={handle}
          onHandleChange={setHandle}
          candidates={candidates}
          onOpen={() => {
            prefetch(0);
          }}
        />
      );
    }

    const user = userEvent.setup();
    render(
      <ThemeProvider>
        <WithCard>
          <Harness />
        </WithCard>
      </ThemeProvider>,
    );

    await user.click(screen.getByText("Amp output level"));
    // The candidate is already resolved by open time — the skeleton never shows,
    // and the BLOCK dropdown's one row (the Boost's catalog full name) is there
    // immediately.
    expect(screen.queryByText("Loading controls…")).toBeNull();
    const blockRow = await screen.findByText("BOOST");
    await user.click(blockRow);
    // Picking the (only, hence best-ranked) block auto-picks its candidate, landing
    // on the CONTROL dropdown's trigger.
    expect(screen.getByText("Gain")).toBeInTheDocument();
  });
});
