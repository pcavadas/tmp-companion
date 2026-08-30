// src/__tests__/SetupBodySceneHandlePreselect.test.tsx — issue 5: a scene whose
// overlay un-bypasses a block the base graph keeps bypassed (Solo un-bypassing
// ACD_Boost, the plan's own example) should default its leveling handle to THAT
// block's own control, not "Amp output level" — the amp barely moves the sound the
// scene actually turns on. `SceneHandleCandidate.enablesBlock` is the backend's
// signal; SetupPage must preselect the first such candidate once the row's
// candidates resolve (they arrive ASYNC — the warm effect's backup-sourced fetch
// still takes a microtask — so the mount `useState` initializer alone can't see it),
// never override an explicit user choice (including the pseudo-option, itself
// `null`), and never pick anything outside the row's own resolved candidate list.

import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ThemeProvider } from "../theme/ThemeProvider";
import { SetupPage } from "../views/level/SetupPage";
import { WithCard } from "./pickCardTestUtils";
import { blockArtTile } from "../models/blockArt";
import { paramLabel } from "../views/level/leveling";
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

const BOOST_LABEL = (() => {
  const art = blockArtTile("ACD_Boost");
  return art.fullName ?? art.name;
})();
// The flattened picker combines block + param into one row/trigger label.
const BOOST_ROW = `${BOOST_LABEL} — ${paramLabel("gain")}`;

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
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === "read_library_via_backup") {
      const result: BackupReadResult = {
        members: [],
        db_bytes: 0,
        total_rows: 1,
        scene_mode: "test",
        presets: [
          {
            slot: 1, // device slot 1 = list index 0
            name: "Friedman HBE",
            scene_count: 1,
            scenes: [],
            amp_candidates: [],
            base_active_amp_count: 1,
            blocks: [],
            graph: emptyGraph,
            footswitches: [],
            silence_hint: null,
            scene_handles: rows,
            base_handles: [],
          },
        ],
        song_presets: [],
        songs: [],
        setlists: [],
        setlist_songs: [],
      };
      return Promise.resolve(result);
    }
    if (cmd === "list_scene_level_handles") return Promise.resolve(rows);
    if (cmd === "list_level_blocks") return Promise.resolve([]);
    return Promise.resolve(null);
  });
  return ensureLibraryScan();
}

const sceneOpt: SetupOption = {
  key: "s0:0",
  slot: 0,
  presetName: "Friedman HBE",
  isBase: false,
  sceneSlot: 0,
  sceneName: "Solo",
  tag: "FS1",
  hasScenes: true,
};

const instrumentOptions: PickOption[] = [{ id: "none", label: "None" }];
const targetOptions: PickOption[] = [{ id: "Rhythm", label: "Rhythm −18" }];

function renderSetup(options: SetupOption[]) {
  return render(
    <ThemeProvider>
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
      </WithCard>
    </ThemeProvider>,
  );
}

describe("SetupPage — scene handle preselect (issue 5)", () => {
  beforeEach(() => {
    resetLibraryScan();
    vi.mocked(invoke).mockReset();
  });

  it("preselects the enabling handle once candidates resolve", async () => {
    await seedScan([
      {
        sceneSlot: 0,
        candidates: [boostCandidate(true)],
        allCandidates: [boostCandidate(true)],
      },
    ]);
    renderSetup([sceneOpt]);

    // Starts on the pseudo default before the async candidate fetch resolves…
    expect(screen.getByText("Amp output level")).toBeInTheDocument();
    // …then flips to the enabling block once it does.
    await waitFor(() => {
      expect(screen.getByText(BOOST_ROW)).toBeInTheDocument();
    });
    expect(screen.queryByText("Amp output level")).toBeNull();
  });

  it("never overrides an explicit user choice of the pseudo-option", async () => {
    await seedScan([
      {
        sceneSlot: 0,
        candidates: [boostCandidate(true)],
        allCandidates: [boostCandidate(true)],
      },
    ]);
    const user = userEvent.setup();
    renderSetup([sceneOpt]);

    // Explicitly re-affirm the pseudo-option BEFORE resolution lands.
    await user.click(screen.getByText("Amp output level"));
    await user.click(await screen.findByText("Amp output level (default)"));

    // Give the async fetch every chance to resolve — the explicit choice must stick.
    await waitFor(() => {
      expect(screen.getByText("Amp output level")).toBeInTheDocument();
    });
    expect(screen.queryByText(BOOST_ROW)).toBeNull();
  });

  it("falls through to the amp pseudo-option when no candidate enables a block", async () => {
    await seedScan([
      {
        sceneSlot: 0,
        candidates: [boostCandidate(false)],
        allCandidates: [boostCandidate(false)],
      },
    ]);
    const user = userEvent.setup();
    renderSetup([sceneOpt]);

    // Open the menu and wait for the (non-enabling) Boost candidate row to appear —
    // proof the async candidate fetch actually resolved, not just that nothing rendered
    // yet.
    await user.click(screen.getByText("Amp output level"));
    await screen.findByText(BOOST_ROW);

    // The trigger (outside the menu) never moved off the pseudo default.
    expect(screen.getAllByText("Amp output level").length).toBeGreaterThan(0);
  });
});
