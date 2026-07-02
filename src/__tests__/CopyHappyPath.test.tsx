// src/__tests__/CopyHappyPath.test.tsx — the FULL Copy wizard happy path end-to-end
// against a mocked invoke bridge: Step 1 (pick reference + a target) → Step 2 (tap a
// target tile, Replace with an origin block, tick the backup box, Save) → the live
// `copy_apply` run streams a result → "Saved to the unit." → Done returns to Step 1.
//
// The single locked assertion is the device contract: `copy_apply` fires exactly ONCE
// with a `jobs[]` carrying the edited preset's listIndex + the exact `replace` op shape
// `diffToOps` produces (cross-checked against CopyView.test.tsx's copyModel expectations).

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ThemeProvider } from "../theme/ThemeProvider";

// 1) event bridge + listeners Map (backup-progress listen resolves to a noop unlisten).
const listeners = new Map<string, (e: { payload: unknown }) => void>();
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, cb: (e: { payload: unknown }) => void) => {
    listeners.set(name, cb);
    return Promise.resolve(() => listeners.delete(name));
  },
}));

// 2) force isTauri() true so libraryScan's guardedListen + the backup read actually run.
vi.mock("../lib/invoke", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/invoke")>();
  return { ...actual, isTauri: () => true };
});

// NOTE: @tauri-apps/api/core invoke + Channel are globally mocked in src/__tests__/setup.ts
// (Channel = class MockChannel { onmessage = null }). We OVERRIDE invoke per test;
// unhandled commands fall through to setup.ts's empty shapes.

import { CopyView } from "../views/copy";
// CopyView reuses the ONE module-scoped libraryScan that lives under views/level
// (useCopyLibrary imports it from there) — reset it between tests so each run scans fresh.
import {
  ensureLibraryScan,
  resetLibraryScan,
} from "../views/level/libraryScan";
// The 3-block series preset + backup row are shared with hardeningFixes.test.tsx's BUG-2 lock.
import { backupRow } from "./copyFixtures";

// Two presets: list index 0 ("Stadium Lead") + 1 ("Clean Verse"), via the shared backupRow.

let copyApplyArgs: { jobs: unknown[]; save: boolean } | null = null;

function installInvoke(rejectSave = false) {
  copyApplyArgs = null;
  vi.mocked(invoke).mockImplementation((command: string, args?: unknown) => {
    switch (command) {
      case "list_presets":
        return Promise.resolve([
          { slot: 0, name: "Stadium Lead" },
          { slot: 1, name: "Clean Verse" },
        ]);
      case "get_store":
        return Promise.resolve({
          profiles: [],
          profile_by_slot: {},
          targets: [],
          playback_level: "stage",
        });
      case "read_library_via_backup":
        return Promise.resolve({
          members: [],
          db_bytes: 0,
          total_rows: 2,
          scene_mode: "off",
          presets: [backupRow(0, "Stadium Lead"), backupRow(1, "Clean Verse")],
          song_presets: [],
          songs: [],
          setlists: [],
          setlist_songs: [],
        });
      case "copy_apply": {
        const a = args as {
          jobs: { listIndex: number; name: string }[];
          save: boolean;
          onResult?: { onmessage?: (i: unknown) => void };
        };
        copyApplyArgs = { jobs: a.jobs, save: a.save };
        if (rejectSave)
          return Promise.reject(new Error("device disconnected mid-save"));
        const items = a.jobs.map((j) => ({
          slot: j.listIndex,
          name: j.name,
          outcome: "updated",
          detail: "",
        }));
        items.forEach((i) => a.onResult?.onmessage?.(i));
        return Promise.resolve(items);
      }
      default:
        return Promise.resolve(null);
    }
  });
}

function renderView() {
  const r = render(
    <ThemeProvider>
      <CopyView connected={true} />
    </ThemeProvider>,
  );
  void ensureLibraryScan(); // App-owned scan in production; seed it for the view here
  return r;
}

// Drive Step 1 (pick reference + target) → Step 2 (replace TwinReverb with DynaComp, tick
// the backup box) → click Save. Stops the moment the copy_apply run is fired; the caller
// asserts the resulting overlay state.
async function driveToSave(
  user: ReturnType<typeof userEvent.setup>,
): Promise<void> {
  await screen.findByText("Copy blocks between presets", undefined, {
    timeout: 3000,
  });
  const stadiumMatches = await screen.findAllByText("Stadium Lead", undefined, {
    timeout: 3000,
  });
  await user.click(stadiumMatches[0]);
  const cleanMatches = await screen.findAllByText("Clean Verse", undefined, {
    timeout: 3000,
  });
  await user.click(cleanMatches[cleanMatches.length - 1]);
  const place = await screen.findByRole(
    "button",
    { name: /place the blocks/i },
    { timeout: 3000 },
  );
  await user.click(place);
  const twinTiles = await screen.findAllByText("65 TWN", undefined, {
    timeout: 3000,
  });
  await user.click(twinTiles[twinTiles.length - 1]);
  const dynaChip = await screen.findByText("DYNAMIC COMPRESSOR", undefined, {
    timeout: 3000,
  });
  await user.click(dynaChip);
  await user.click(
    await screen.findByText(/backed up with pro control/i, undefined, {
      timeout: 3000,
    }),
  );
  const save = await screen.findByRole(
    "button",
    { name: /save to the unit/i },
    { timeout: 3000 },
  );
  await act(async () => {
    await user.click(save);
  });
}

beforeEach(() => {
  vi.mocked(invoke).mockReset();
  listeners.clear();
  resetLibraryScan();
  localStorage.clear();
});

describe("CopyView — full happy path (Step 1 → Step 2 → save → done)", () => {
  it("drives select → place → replace → save, fires copy_apply once with the replace op", async () => {
    installInvoke();
    const user = userEvent.setup();
    renderView();
    await driveToSave(user);

    // ── SaveOverlay reaches the done state ───────────────────────────────────
    await screen.findByText("Saved to the unit.", undefined, { timeout: 3000 });

    // ── LOCKED ASSERTIONS: the device contract ───────────────────────────────
    // copy_apply fired exactly once.
    const copyApplyCalls = vi
      .mocked(invoke)
      .mock.calls.filter((c) => c[0] === "copy_apply");
    expect(copyApplyCalls).toHaveLength(1);

    expect(copyApplyArgs).not.toBeNull();
    const jobs = copyApplyArgs?.jobs as {
      listIndex: number;
      name: string;
      ops: unknown[];
    }[];
    // One job — the edited target "Clean Verse" at list index 1.
    expect(jobs).toHaveLength(1);
    expect(jobs[0].listIndex).toBe(1);
    expect(jobs[0].name).toBe("Clean Verse");

    // The op shape matches diffToOps's `replace` (cross-checked vs CopyView.test.tsx):
    // the TwinReverb node (G1/n2) replaced by the DynaComp model.
    expect(jobs[0].ops).toEqual([
      {
        kind: "replace",
        group: "G1",
        nodeId: "n2",
        repl: { kind: "model", fenderId: "ACD_DynaComp" },
      },
    ]);

    // ── Done → returns to Step 1 ─────────────────────────────────────────────
    await user.click(
      await screen.findByRole("button", { name: /done/i }, { timeout: 3000 }),
    );
    await screen.findByText("Copy blocks between presets", undefined, {
      timeout: 3000,
    });
  });

  it("surfaces a copy_apply rejection as a failure, not a false 'saved'", async () => {
    installInvoke(true); // copy_apply REJECTS (device error / lost connection)
    const user = userEvent.setup();
    renderView();
    await driveToSave(user);

    // The overlay shows the FAILURE — not the green success + "N presets updated".
    await screen.findByText("Save failed.", undefined, { timeout: 3000 });
    expect(screen.getByText(/device disconnected mid-save/i)).toBeTruthy();
    expect(screen.queryByText("Saved to the unit.")).toBeNull();
    expect(screen.queryByText(/preset.*updated/i)).toBeNull();
  });
});
