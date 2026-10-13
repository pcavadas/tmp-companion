// src/__tests__/CopyHappyPath.test.tsx — the FULL Copy wizard happy path end-to-end
// against a mocked invoke bridge: Step 1 (pick reference + a target) → Step 2 (tap a
// target tile, Replace with an origin block, tick the backup box, Save) → the live
// `copy_apply` run streams a result → "Saved to the unit." → Done returns to Step 1.
//
// The single locked assertion is the device contract: `copy_apply` fires exactly ONCE
// with a `jobs[]` carrying the edited preset's listIndex + the exact `replace` op shape
// `diffToOps` produces (cross-checked against CopyView.test.tsx's copyModel expectations).

import { describe, it, expect, vi, beforeEach } from "vitest";
import { screen, act, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import type { ActiveGraph, CopyApplyItem } from "../lib/types";

import { CopyView } from "../views/copy";
// CopyView reuses the ONE module-scoped libraryScan that lives under views/level
// (useCopyLibrary imports it from there) — reset it between tests so each run scans fresh.
import { ensureLibraryScan, resetLibraryScan } from "../device/libraryScan";
// The 3-block series preset + backup row are shared with CopyView.test.tsx's BUG-2 lock.
import { backupRow, seriesGraph } from "./copyFixtures";
import {
  callsOf,
  forceTauri,
  makeBackupRead,
  mockCommands,
  rejectWith,
} from "./commandMocks";
import { renderWithTheme } from "./testKit";

// In-app: the live bridges and the library scan subscribe.
forceTauri();

// Two presets: list index 0 ("Stadium Lead") + 1 ("Clean Verse"), via the shared backupRow.

// `readBack` = the graph copy_apply returns on each item (the device's post-save read-back);
// omitted = no read-back (the offline shape).
function installInvoke(rejectSave = false, readBack?: ActiveGraph) {
  mockCommands({
    list_presets: () => [
      { slot: 0, name: "Stadium Lead" },
      { slot: 1, name: "Clean Verse" },
    ],
    read_library_via_backup: () =>
      makeBackupRead({
        total_rows: 2,
        scene_mode: "off",
        presets: [backupRow(0, "Stadium Lead"), backupRow(1, "Clean Verse")],
      }),
    copy_apply: ({ jobs, onResult }) => {
      if (rejectSave)
        return rejectWith(new Error("device disconnected mid-save"));
      const items: CopyApplyItem[] = jobs.map((j) => ({
        slot: j.listIndex,
        name: j.name,
        outcome: "updated",
        detail: "",
        ...(readBack ? { graph: readBack } : {}),
      }));
      items.forEach((i) => {
        onResult.onmessage(i);
      });
      return items;
    },
  });
}

/** The model ids of the block tiles on one target's edit card, in strip order. */
function tileModels(target: string): string[] {
  return Array.from(
    document.querySelectorAll(
      `[data-target-card="${target}"] [data-block-tile]`,
    ),
  ).map((el) => el.getAttribute("data-block-tile") ?? "");
}

function renderView() {
  const r = renderWithTheme(<CopyView connected={true} />);
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
    const copyApplyCalls = callsOf("copy_apply");
    expect(copyApplyCalls).toHaveLength(1);
    const { jobs } = copyApplyCalls[0];
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

  it("keeps the saved edit when the post-save read-back is the pre-edit graph (stale, no refetch)", async () => {
    // HW 2026-09-02 (fw 1.8.45): after "Saved to the unit." the graph copy_apply handed
    // back was the LOAD-TIME document (the held session's buffer, not a fresh read), and
    // preferring it patched the cache back to the original blocks — the re-opened editor
    // listed a deleted block again. The read-back here is exactly that: the pre-edit
    // roster (Twin still in place) while the acked edit replaced Twin with DynaComp.
    installInvoke(false, seriesGraph("Clean Verse", 1));
    const user = userEvent.setup();
    renderView();
    await driveToSave(user);
    await screen.findByText("Saved to the unit.", undefined, { timeout: 3000 });
    await user.click(
      await screen.findByRole("button", { name: /done/i }, { timeout: 3000 }),
    );

    // Re-open the same targets (the selection survives Done): the cached graph must show
    // the edit the device acked, not the stale read-back.
    await user.click(
      await screen.findByRole(
        "button",
        { name: /place the blocks/i },
        { timeout: 3000 },
      ),
    );
    await waitFor(() => {
      expect(tileModels("Clean Verse")).toEqual([
        "ACD_DynaComp",
        "ACD_DynaComp",
        "ACD_TMSmallHall",
      ]);
    });
    // …and the heavy backup read ran exactly once: the patch never triggered a refetch.
    expect(
      vi
        .mocked(invoke)
        .mock.calls.filter((c) => c[0] === "read_library_via_backup"),
    ).toHaveLength(1);
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
