// src/__tests__/CopyChoose.test.tsx — Copy's Choose page and the Place page's save gate
// (look B):
//   - "Select all N shown" acts on the rows the search shows (as on Level's list), and
//     clears them when all are ticked;
//   - the search's no-match state and its "Clear search", "No other presets to copy into";
//   - the footer says what will happen or why "Choose blocks" waits;
//   - on Place blocks, Save says why it waits: nothing changed, one preset over a limit
//     (named), or several (with "Show only these").

import { describe, expect, it, vi, beforeEach } from "vitest";
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { CopyView } from "../views/copy";
import { ChoosePresets } from "../views/copy/ChoosePresets";
import type { CopyPreset } from "../views/copy/useCopyLibrary";
import { ensureLibraryScan, resetLibraryScan } from "../device/libraryScan";
import type { ActiveGraph, BackupPresetRow, GraphNode } from "../lib/types";
import { backupRow } from "./copyFixtures";
import {
  forceTauri,
  makeBackupPresetRow,
  makeBackupRead,
  mockCommands,
} from "./commandMocks";
import { renderWithTheme } from "./testKit";

forceTauri();

const EMPTY: ActiveGraph = {
  name: null,
  slot: null,
  template: null,
  split_mix: null,
  nodes: [],
  stages: [],
};

const preset = (slot: number, name: string): CopyPreset => ({
  slot,
  name,
  onUnit: false,
  graph: EMPTY,
  cpu: null,
});

const PRESETS = [
  preset(0, "Clean DI"),
  preset(1, "Scooped Verse"),
  preset(2, "Studio Clean"),
  preset(3, "Plexi Crunch"),
];

function renderChoose(over: Partial<Parameters<typeof ChoosePresets>[0]> = {}) {
  const props = {
    presets: PRESETS,
    listLoading: false,
    fromSlot: 3,
    setFrom: vi.fn(),
    toSet: new Set<number>(),
    setTo: vi.fn(),
    ready: true,
    scanning: false,
    onContinue: vi.fn(),
    ...over,
  };
  renderWithTheme(<ChoosePresets {...props} />);
  return props;
}

describe("Choose presets", () => {
  it("lists the source as a radio and leaves it out of Copy into", () => {
    renderChoose();
    expect(
      screen.getByRole("radio", { name: "Copy from Plexi Crunch" }),
    ).toHaveAttribute("aria-checked", "true");
    expect(
      screen.queryByRole("checkbox", { name: "Copy into Plexi Crunch" }),
    ).toBeNull();
    expect(
      screen.getByRole("checkbox", { name: "Copy into Clean DI" }),
    ).toBeTruthy();
  });

  it("a click anywhere on a row chooses it", async () => {
    const user = userEvent.setup();
    const props = renderChoose();
    await user.click(screen.getAllByText("Studio Clean")[0]);
    expect(props.setFrom).toHaveBeenCalledWith(2);
    await user.click(screen.getAllByText("Scooped Verse")[1]);
    expect(props.setTo).toHaveBeenCalledWith(new Set([1]));
  });

  it("Select all N shown ticks only the rows the search shows", async () => {
    const user = userEvent.setup();
    const props = renderChoose({ toSet: new Set([0]) });
    await user.type(
      screen.getByRole("textbox", { name: "Search presets to copy into" }),
      "s",
    );
    // "Scooped Verse" and "Studio Clean" match; "Clean DI" (ticked) is hidden.
    const all = screen.getByRole("checkbox", { name: "Select all 2 shown" });
    await user.click(all);
    expect(props.setTo).toHaveBeenLastCalledWith(new Set([0, 1, 2]));
  });

  it("Select all N shown clears the shown rows when all are ticked", async () => {
    const user = userEvent.setup();
    const props = renderChoose({ toSet: new Set([0, 1, 2]) });
    const all = screen.getByRole("checkbox", { name: "Select all 3 shown" });
    expect(all).toHaveAttribute("aria-checked", "true");
    await user.click(all);
    expect(props.setTo).toHaveBeenLastCalledWith(new Set());
  });

  it("is mixed when some shown rows are ticked", () => {
    renderChoose({ toSet: new Set([1]) });
    expect(
      screen.getByRole("checkbox", { name: "Select all 3 shown" }),
    ).toHaveAttribute("aria-checked", "mixed");
  });

  it("says when a search matches nothing, with Clear search", async () => {
    const user = userEvent.setup();
    renderChoose();
    const box = screen.getByRole("textbox", {
      name: "Search presets to copy from",
    });
    await user.type(box, "fuzz");
    expect(screen.getByText('No presets match "fuzz"')).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Clear search" }));
    expect(box).toHaveValue("");
    expect(screen.queryByText(/No presets match/)).toBeNull();
  });

  it("says when there is no other preset to copy into", () => {
    renderChoose({ presets: [preset(3, "Plexi Crunch")] });
    expect(screen.getByText("No other presets to copy into")).toBeTruthy();
  });

  it("the footer says what will happen, or why Choose blocks waits", async () => {
    const user = userEvent.setup();
    const props = renderChoose({ toSet: new Set([0, 1]) });
    expect(screen.getByText("From Plexi Crunch into 2 presets")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Choose blocks" }));
    expect(props.onContinue).toHaveBeenCalled();
  });

  it("asks for a target first", () => {
    renderChoose();
    expect(
      screen.getByText("Choose at least one preset to copy into"),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Choose blocks" }),
    ).toBeDisabled();
  });

  it("waits, busy, while the preset details are read", () => {
    renderChoose({ ready: false, toSet: new Set([0]) });
    expect(screen.getByText("Reading presets")).toBeTruthy();
    const btn = screen.getByRole("button", { name: "Choose blocks" });
    expect(btn).toBeDisabled();
    expect(btn).toHaveAttribute("aria-busy", "true");
  });
});

// ── the save gate on Place blocks ────────────────────────────────────────────

const node = (id: string, model: string): GraphNode => ({
  group_id: "G1",
  node_id: id,
  model,
  bypassed: false,
  params: {},
});

/** A preset already over the CPU budget (as an imported one can be). */
function overRow(listIndex: number, name: string): BackupPresetRow {
  const nodes = [
    node("a", "ACD_GuitarSynth"),
    node("b", "ACD_GuitarSynth"),
    node("c", "ACD_POG"),
  ];
  const graph: ActiveGraph = {
    name,
    slot: null,
    template: "gtrSeries",
    split_mix: null,
    nodes,
    stages: [{ kind: "series", blocks: nodes }],
  };
  return makeBackupPresetRow({
    slot: listIndex + 1,
    name,
    scene_count: 1,
    scenes: [{ name: "Base", fs: null }],
    amp_candidates: [],
    blocks: nodes.map((n) => ({
      group_id: n.group_id,
      node_id: n.node_id,
      fender_id: n.model,
    })),
    graph,
  });
}

const NAMES = ["Stadium Lead", "Clean Verse", "Heavy One", "Heavy Two"];

async function toPlace(
  targets: string[],
): Promise<ReturnType<typeof userEvent.setup>> {
  mockCommands({
    list_presets: () => NAMES.map((name, slot) => ({ slot, name })),
    read_library_via_backup: () =>
      makeBackupRead({
        total_rows: NAMES.length,
        scene_mode: "off",
        presets: [
          backupRow(0, NAMES[0]),
          backupRow(1, NAMES[1]),
          overRow(2, NAMES[2]),
          overRow(3, NAMES[3]),
        ],
      }),
  });
  const user = userEvent.setup();
  renderWithTheme(<CopyView connected={true} />);
  void ensureLibraryScan();
  const ref = await screen.findAllByText("Stadium Lead", undefined, {
    timeout: 3000,
  });
  await user.click(ref[0]);
  for (const name of targets) {
    const m = await screen.findAllByText(name);
    await user.click(m[m.length - 1]);
  }
  await user.click(
    await screen.findByRole(
      "button",
      { name: "Choose blocks" },
      { timeout: 3000 },
    ),
  );
  await screen.findByText("Choose a block to copy.");
  return user;
}

describe("Place blocks — why Save waits", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    resetLibraryScan();
  });

  it("asks for a change first, and counts the changed presets after", async () => {
    const user = await toPlace(["Clean Verse"]);
    expect(screen.getByText("Add a block to a preset first")).toBeTruthy();
    const save = screen.getByRole("button", {
      name: "Save to your Tone Master Pro",
    });
    expect(save).toBeDisabled();
    const dyna = document.querySelector<HTMLElement>(
      '[data-source-card] [data-block-tile="ACD_DynaComp"]',
    );
    if (!dyna) throw new Error("no DynaComp");
    await user.click(dyna);
    await user.click(
      screen.getByRole("button", {
        name: "Add here in Clean Verse, at the end",
      }),
    );
    expect(screen.queryByText("Add a block to a preset first")).toBeNull();
    expect(screen.getByText("1 of 1 preset changed")).toBeTruthy();
    // The backup tick is the last thing it waits for.
    expect(save).toBeDisabled();
    await user.click(screen.getByText("I've backed up with Pro Control"));
    expect(save).toBeEnabled();
  });

  it("names the one preset over a limit and why", async () => {
    await toPlace(["Clean Verse", "Heavy One"]);
    expect(
      screen.getByText("Heavy One: Uses more than 76.5% CPU. Remove a block."),
    ).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "Show only these" }),
    ).toBeNull();
  });

  it("counts several, and can show only those", async () => {
    const user = await toPlace(["Clean Verse", "Heavy One", "Heavy Two"]);
    expect(screen.getByText("2 presets can't be saved yet")).toBeTruthy();
    expect(screen.getByText("2 can't be saved yet")).toBeTruthy();
    const cards = () =>
      Array.from(document.querySelectorAll("[data-target-card]")).map((c) =>
        c.getAttribute("data-target-card"),
      );
    expect(cards()).toEqual(["Clean Verse", "Heavy One", "Heavy Two"]);
    await user.click(screen.getByRole("button", { name: "Show only these" }));
    expect(cards()).toEqual(["Heavy One", "Heavy Two"]);
    await user.click(screen.getByRole("button", { name: "Show all" }));
    expect(cards()).toHaveLength(3);
    // Each blocked card says why under its strip.
    const heavy = document.querySelector<HTMLElement>(
      '[data-target-card="Heavy One"]',
    );
    if (!heavy) throw new Error("no card");
    expect(
      within(heavy).getByText("Uses more than 76.5% CPU. Remove a block."),
    ).toBeTruthy();
    expect(within(heavy).getByText("Over")).toBeTruthy();
  });
});
