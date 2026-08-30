// src/__tests__/useLevelBlocks.test.tsx — the Set-up step's Base handle candidate cache
// resolves INSTANT-FIRST off the startup backup scan (`base_handles`), never firing a
// `list_level_blocks` device read for a preset the scan already covers. The fallback fires
// only when the scan has NO ENTRY for the slot at all — an empty row is the correct,
// expected answer for a genuinely blockless/unparseable preset (mirrors
// `useSceneHandles.ts`'s own discriminator: MAP KEY PRESENCE, not list emptiness).

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
  FlatLevelPick,
  type BlockLevelHandle,
} from "../views/level/FlatLevelPick";
import { WithCard } from "./pickCardTestUtils";
import { useLevelBlocks } from "../views/level/useLevelBlocks";
import {
  ensureLibraryScan,
  resetLibraryScan,
} from "../views/level/libraryScan";
import type {
  ActiveGraph,
  BackupReadResult,
  SceneHandleCandidate,
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
    return Promise.resolve(null);
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
          value: 0.42,
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
          value: 0.7,
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

  it("never reaches FlatLevelPick's 'Loading controls…' state on the backup path", async () => {
    await seedScan([backupRow(1, "Stadium Lead", [AMP_CANDIDATE])]);

    function Harness() {
      const { prefetch, blocksFor } = useLevelBlocks();
      const [handle, setHandle] = useState<BlockLevelHandle | null>(null);
      const st = blocksFor(0);
      const candidates =
        st.status === "resolved"
          ? {
              status: "resolved" as const,
              list: st.blocks.map((b) => ({
                groupId: b.group_id,
                nodeId: b.node_id,
                fenderId: b.model_id,
                parameterId: b.parameter_id,
                paramClass: b.paramClass,
              })),
            }
          : { status: st.status };
      return (
        <FlatLevelPick
          pseudoLabel="Preset level"
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

    await user.click(screen.getByText("Preset level"));
    // The candidate is already resolved by open time (the backup scan ran at
    // `beforeEach`/`seedScan` time, well before this click) — the skeleton never
    // shows, and the flattened list's one candidate row (block + param name) is
    // there immediately, in a single click (no separate block/control stage).
    expect(screen.queryByText("Loading controls…")).toBeNull();
    const row = await screen.findByText(
      "FENDER '65 TWIN REVERB — Output level",
    );
    await user.click(row);
    expect(
      screen.getByText("FENDER '65 TWIN REVERB — Output level"),
    ).toBeInTheDocument();
  });
});
