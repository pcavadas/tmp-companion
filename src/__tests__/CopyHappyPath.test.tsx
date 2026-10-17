// src/__tests__/CopyHappyPath.test.tsx — the FULL Copy happy path end-to-end against a
// mocked invoke bridge: Step 1 (pick the source + a target) → Step 2 (choose a block on
// the source strip, open a target block's menu, Replace, tick the backup box, Save) → the
// live `copy_apply` run streams a result → "Saved to your Tone Master Pro" → Done returns
// to Step 1. Also: a "+" gap's insert reaches the wire as the insert op `diffToOps` makes.
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

type User = ReturnType<typeof userEvent.setup>;

// Step 1: Stadium Lead is the source, Clean Verse the target → Choose blocks.
async function driveToPlace(user: User): Promise<void> {
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
    { name: /choose blocks/i },
    { timeout: 3000 },
  );
  await user.click(place);
  await screen.findByText("Choose a block to copy.", undefined, {
    timeout: 3000,
  });
}

// The source strip's DynaComp is the block to copy.
async function chooseDyna(user: User): Promise<void> {
  const dyna = document.querySelector<HTMLElement>(
    '[data-source-card] [data-block-tile="ACD_DynaComp"]',
  );
  if (!dyna) throw new Error("no DynaComp on the source strip");
  await user.click(dyna);
  await screen.findByText("Dynamic Comp is ready to copy.");
}

async function backUpAndSave(user: User): Promise<void> {
  await user.click(
    await screen.findByText(/backed up with pro control/i, undefined, {
      timeout: 3000,
    }),
  );
  const save = await screen.findByRole(
    "button",
    { name: /save to your tone master pro/i },
    { timeout: 3000 },
  );
  await act(async () => {
    await user.click(save);
  });
}

// Drive Step 1 → Step 2 (choose DynaComp, replace the TwinReverb with it through the
// Twin's menu, tick the backup box) → click Save. Stops the moment the copy_apply run is
// fired; the caller asserts the resulting dialog state.
async function driveToSave(user: User): Promise<void> {
  await driveToPlace(user);
  await chooseDyna(user);
  const twin = document.querySelector<HTMLElement>(
    '[data-target-card="Clean Verse"] [data-block-tile="ACD_TwinReverb65NoFx"]',
  );
  if (!twin) throw new Error("no Twin on Clean Verse");
  await user.click(twin);
  await user.click(
    await screen.findByRole("menuitem", { name: "Replace with Dynamic Comp" }),
  );
  await backUpAndSave(user);
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
    await screen.findByText("Saved to your Tone Master Pro", undefined, {
      timeout: 3000,
    });

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
    await screen.findByText("Saved to your Tone Master Pro", undefined, {
      timeout: 3000,
    });
    await user.click(
      await screen.findByRole("button", { name: /done/i }, { timeout: 3000 }),
    );

    // Re-open the same targets (the selection survives Done): the cached graph must show
    // the edit the device acked, not the stale read-back.
    await user.click(
      await screen.findByRole(
        "button",
        { name: /choose blocks/i },
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

    // The dialog shows the FAILURE — never "Saved"; the run's text goes to the log.
    await screen.findByText("Nothing was saved", undefined, { timeout: 3000 });
    expect(screen.queryByText(/device disconnected mid-save/i)).toBeNull();
    expect(screen.queryByText("Saved to your Tone Master Pro")).toBeNull();
    expect(screen.queryByText("Saved")).toBeNull();
  });

  it("a + gap's insert reaches copy_apply as the insert op diffToOps makes", async () => {
    installInvoke();
    const user = userEvent.setup();
    renderView();
    await driveToPlace(user);
    await chooseDyna(user);
    // Before the Twin (anchored on it) and at the end (appended: no anchor).
    await user.click(
      screen.getByRole("button", {
        name: "Add here in Clean Verse, before '65 Twin",
      }),
    );
    await user.click(
      screen.getByRole("button", {
        name: "Add here in Clean Verse, at the end",
      }),
    );
    await backUpAndSave(user);
    await screen.findByText("Saved to your Tone Master Pro", undefined, {
      timeout: 3000,
    });
    const [{ jobs }] = callsOf("copy_apply");
    expect(jobs).toHaveLength(1);
    expect(jobs[0].ops).toEqual([
      {
        kind: "insert",
        group: "G1",
        key: expect.any(String) as string,
        before: null,
        repl: { kind: "model", fenderId: "ACD_DynaComp" },
      },
      {
        kind: "insert",
        group: "G1",
        key: expect.any(String) as string,
        before: { kind: "node", nodeId: "n2" },
        repl: { kind: "model", fenderId: "ACD_DynaComp" },
      },
    ]);
  });
});
