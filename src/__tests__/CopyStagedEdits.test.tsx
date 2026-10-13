// src/__tests__/CopyStagedEdits.test.tsx — the Copy wizard keeps staged work it has not
// written, against a mocked invoke bridge (the CopyHappyPath harness):
//   #291  the done screen reports a partial or total failure as one (no green "Saved to
//         the unit."), and Done keeps the unsaved presets' edits on Step 2;
//   #292  Back → "Place the blocks" with the same selection keeps the edits + undo;
//   #293  a bypassed block renders bypassed in Copy's strip, and a save's cache patch
//         keeps the untouched blocks' bypass state;
//   #294  opening another block resets the editor to Replace.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { screen, act, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import type { ActiveGraph, BackupPresetRow, CopyApplyItem } from "../lib/types";

import { CopyView } from "../views/copy";
import {
  ensureLibraryScan,
  getLibraryScan,
  resetLibraryScan,
} from "../device/libraryScan";
import { backupRow, seriesGraph } from "./copyFixtures";
import {
  forceTauri,
  makeBackupRead,
  mockCommands,
  rejectWith,
} from "./commandMocks";
import { renderWithTheme } from "./testKit";

// In-app: the live bridges and the library scan subscribe.
forceTauri();

const NAMES = ["Stadium Lead", "Clean Verse", "Lead Two"];
const TWIN = "ACD_TwinReverb65NoFx";

interface Opts {
  /** Per-list-index outcome of copy_apply (default "updated"). */
  outcome?: Partial<Record<number, "updated" | "error">>;
  /** copy_apply rejects outright after streaming the per-item results. */
  reject?: boolean;
  /** List indexes whose last block (the Small Hall, n3) is bypassed. */
  bypassHall?: number[];
}

function rowFor(
  listIndex: number,
  name: string,
  bypassHall: boolean,
): BackupPresetRow {
  const row = backupRow(listIndex, name);
  if (!bypassHall) return row;
  const g = seriesGraph(name);
  const nodes = g.nodes.map((n) =>
    n.node_id === "n3" ? { ...n, bypassed: true } : n,
  );
  const graph: ActiveGraph = {
    ...g,
    nodes,
    stages: [{ kind: "series", blocks: nodes }],
  };
  return { ...row, graph };
}

function installInvoke(opts: Opts = {}) {
  mockCommands({
    list_presets: () => NAMES.map((name, slot) => ({ slot, name })),
    read_library_via_backup: () =>
      makeBackupRead({
        total_rows: NAMES.length,
        scene_mode: "off",
        presets: NAMES.map((name, i) =>
          rowFor(i, name, opts.bypassHall?.includes(i) ?? false),
        ),
      }),
    copy_apply: ({ jobs, onResult }) => {
      const items: CopyApplyItem[] = jobs.map((j) => ({
        slot: j.listIndex,
        name: j.name,
        outcome: opts.outcome?.[j.listIndex] ?? "updated",
        detail: "",
      }));
      items.forEach((i) => {
        onResult.onmessage(i);
      });
      if (opts.reject)
        return rejectWith(new Error("device disconnected mid-save"));
      return items;
    },
  });
}

function renderView() {
  renderWithTheme(<CopyView connected={true} />);
  void ensureLibraryScan();
}

function card(name: string): HTMLElement {
  const el = document.querySelector<HTMLElement>(
    `[data-target-card="${name}"]`,
  );
  if (!el) throw new Error(`no target card for ${name}`);
  return el;
}

function tileModels(name: string): string[] {
  return Array.from(card(name).querySelectorAll("[data-block-tile]")).map(
    (el) => el.getAttribute("data-block-tile") ?? "",
  );
}

function tile(name: string, model: string): HTMLElement {
  const el = card(name).querySelector<HTMLElement>(
    `[data-block-tile="${model}"]`,
  );
  if (!el) throw new Error(`no ${model} tile on ${name}`);
  return el;
}

function undoButton(): HTMLElement {
  const el = document.querySelector<HTMLElement>(
    '[title^="Undo / redo"] button[aria-label="Undo"]',
  );
  if (!el) throw new Error("no undo button");
  return el;
}

type User = ReturnType<typeof userEvent.setup>;

/** Step 1: Stadium Lead is the reference; tick each of `targets`; Place the blocks. */
async function pickAndPlace(user: User, targets: string[]): Promise<void> {
  await screen.findByText("Copy blocks between presets", undefined, {
    timeout: 3000,
  });
  const ref = await screen.findAllByText("Stadium Lead", undefined, {
    timeout: 3000,
  });
  await user.click(ref[0]);
  for (const name of targets) {
    const m = await screen.findAllByText(name, undefined, { timeout: 3000 });
    await user.click(m[m.length - 1]);
  }
  await placeBlocks(user);
}

async function placeBlocks(user: User): Promise<void> {
  await user.click(
    await screen.findByRole(
      "button",
      { name: /place the blocks/i },
      { timeout: 3000 },
    ),
  );
  await waitFor(() => {
    expect(document.querySelector("[data-target-card]")).not.toBeNull();
  });
}

/** Replace the '65 Twin on `name` with the Dyna Comp. */
async function replaceTwin(user: User, name: string): Promise<void> {
  await user.click(tile(name, TWIN));
  await user.click(
    await screen.findByText("DYNAMIC COMPRESSOR", undefined, { timeout: 3000 }),
  );
}

async function backUpAndSave(user: User): Promise<void> {
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

const EDITED = ["ACD_DynaComp", "ACD_DynaComp", "ACD_TMSmallHall"];
const PRISTINE = ["ACD_DynaComp", TWIN, "ACD_TMSmallHall"];

beforeEach(() => {
  vi.mocked(invoke).mockReset();
  resetLibraryScan();
  localStorage.clear();
});

describe("Copy save outcome (#291)", () => {
  it("every preset errors → no success state, and Done keeps every staged edit", async () => {
    installInvoke({ outcome: { 1: "error", 2: "error" } });
    const user = userEvent.setup();
    renderView();
    await pickAndPlace(user, ["Clean Verse", "Lead Two"]);
    await replaceTwin(user, "Clean Verse");
    await replaceTwin(user, "Lead Two");
    await backUpAndSave(user);

    await screen.findByText("Save failed.", undefined, { timeout: 3000 });
    expect(screen.queryByText("Saved to the unit.")).toBeNull();
    expect(screen.queryByText(/changes are now on the unit/i)).toBeNull();
    expect(screen.getByText(/2 presets could not be saved/i)).toBeTruthy();

    await user.click(screen.getByRole("button", { name: /done/i }));
    // Still on Step 2 with both edits staged (and undoable).
    expect(tileModels("Clean Verse")).toEqual(EDITED);
    expect(tileModels("Lead Two")).toEqual(EDITED);
    expect(undoButton().style.opacity).toBe("1");
  });

  it("one of two errors → a partial outcome, and Done keeps only the failed preset's edit", async () => {
    installInvoke({ outcome: { 2: "error" } });
    const user = userEvent.setup();
    renderView();
    await pickAndPlace(user, ["Clean Verse", "Lead Two"]);
    await replaceTwin(user, "Clean Verse");
    await replaceTwin(user, "Lead Two");
    await backUpAndSave(user);

    await screen.findByText("Some presets weren’t saved.", undefined, {
      timeout: 3000,
    });
    expect(screen.queryByText("Saved to the unit.")).toBeNull();
    expect(
      screen.getByText(/1 preset updated · 1 could not be saved/i),
    ).toBeTruthy();

    await user.click(screen.getByRole("button", { name: /done/i }));
    // The failed preset keeps its staged edit (still "edited")…
    expect(tileModels("Lead Two")).toEqual(EDITED);
    expect(within(card("Lead Two")).getByText(/edited/i)).toBeTruthy();
    // …the saved one restarts from its just-saved graph, with nothing staged.
    expect(tileModels("Clean Verse")).toEqual(EDITED);
    expect(within(card("Clean Verse")).queryByText(/edited/i)).toBeNull();
  });

  it("a rejected run keeps the presets the channel reported as saved", async () => {
    installInvoke({ outcome: { 2: "error" }, reject: true });
    const user = userEvent.setup();
    renderView();
    await pickAndPlace(user, ["Clean Verse", "Lead Two"]);
    await replaceTwin(user, "Clean Verse");
    await replaceTwin(user, "Lead Two");
    await backUpAndSave(user);

    await screen.findByText("Some presets weren’t saved.", undefined, {
      timeout: 3000,
    });
    expect(screen.getByText(/device disconnected mid-save/i)).toBeTruthy();
    // The saved preset's cache was patched even though the run rejected.
    expect(
      getLibraryScan()
        .graphByIndex.get(1)
        ?.nodes.map((n) => n.model),
    ).toEqual(EDITED);
  });

  it("all saved → the green success state and Done returns to Step 1", async () => {
    installInvoke();
    const user = userEvent.setup();
    renderView();
    await pickAndPlace(user, ["Clean Verse"]);
    await replaceTwin(user, "Clean Verse");
    await backUpAndSave(user);
    await screen.findByText("Saved to the unit.", undefined, { timeout: 3000 });
    expect(screen.getByText(/1 preset updated\./i)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /done/i }));
    await screen.findByText("Copy blocks between presets");
  });
});

describe("Copy Back keeps staged edits (#292)", () => {
  it("Back → Place the blocks with the same selection keeps the edit and its undo", async () => {
    installInvoke();
    const user = userEvent.setup();
    renderView();
    await pickAndPlace(user, ["Clean Verse"]);
    await replaceTwin(user, "Clean Verse");
    expect(tileModels("Clean Verse")).toEqual(EDITED);

    await user.click(screen.getByText("Back"));
    await placeBlocks(user);
    expect(tileModels("Clean Verse")).toEqual(EDITED);
    // The undo step survived: undo restores the original block.
    expect(undoButton().style.opacity).toBe("1");
    await user.click(undoButton());
    expect(tileModels("Clean Verse")).toEqual(PRISTINE);
  });

  it("adding a target keeps the existing target's edit and starts the new one fresh", async () => {
    installInvoke();
    const user = userEvent.setup();
    renderView();
    await pickAndPlace(user, ["Clean Verse"]);
    await replaceTwin(user, "Clean Verse");

    await user.click(screen.getByText("Back"));
    const m = await screen.findAllByText("Lead Two");
    await user.click(m[m.length - 1]);
    await placeBlocks(user);
    expect(tileModels("Clean Verse")).toEqual(EDITED);
    expect(tileModels("Lead Two")).toEqual(PRISTINE);
  });
});

describe("Copy bypass state (#293)", () => {
  it("draws a bypassed block bypassed, and a save keeps it in the cache patch", async () => {
    installInvoke({ bypassHall: [1] });
    const user = userEvent.setup();
    renderView();
    await pickAndPlace(user, ["Clean Verse"]);
    expect(tile("Clean Verse", "ACD_TMSmallHall").style.opacity).toBe("0.34");
    expect(tile("Clean Verse", TWIN).style.opacity).toBe("1");

    await replaceTwin(user, "Clean Verse");
    await backUpAndSave(user);
    await screen.findByText("Saved to the unit.", undefined, { timeout: 3000 });
    const patched = getLibraryScan().graphByIndex.get(1);
    expect(patched?.nodes.map((n) => [n.model, n.bypassed])).toEqual([
      ["ACD_DynaComp", false],
      ["ACD_DynaComp", false],
      ["ACD_TMSmallHall", true],
    ]);
  });
});

describe("Copy block editor mode (#294)", () => {
  it("opening another block while on Insert after resets the editor to Replace", async () => {
    installInvoke();
    const user = userEvent.setup();
    renderView();
    await pickAndPlace(user, ["Clean Verse"]);
    await user.click(tile("Clean Verse", TWIN));
    await user.click(screen.getByRole("radio", { name: "Insert after" }));
    expect(screen.getByRole("radio", { name: "Insert after" })).toHaveAttribute(
      "aria-checked",
      "true",
    );

    await user.click(tile("Clean Verse", "ACD_TMSmallHall"));
    expect(screen.getByRole("radio", { name: "Replace" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });
});
