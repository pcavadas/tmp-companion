// PresetList (scene-tree selection) — the row contract.
//
// Locks: the loading skeleton; the background scene-scan strip + parent-row meta
// (loading… / N scenes · M footswitches / nothing); select-on-row-click; whole-preset
// checkbox select; select-all; empty rows; caret expand → scene + footswitch sub-rows →
// per-child toggle; inert carets until `ready`.

import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { ThemeProvider } from "../theme/ThemeProvider";
import {
  PresetList,
  type PresetRow,
  type PresetListProps,
} from "../views/PresetList";
import type { ActiveGraph, FootswitchInfo, SceneInfo } from "../lib/types";

const noop = () => {
  /* no-op */
};

type ListOverrides = Partial<PresetListProps>;

const ROWS: PresetRow[] = [
  { slot: 0, name: "Clean DI", empty: false },
  { slot: 1, name: "Plexi Crunch", empty: false },
  { slot: 2, name: "", empty: true },
  { slot: 3, name: "Tweed Warm", empty: false },
];

// slot 0: 2 FS scenes + 1 footswitch; slot 1: nothing; slot 3: 1 footswitch only —
// all known after the backup read.
const SCENES = new Map<number, SceneInfo[]>([
  [
    0,
    [
      { name: "Bright", fs: 1 },
      { name: "Lead", fs: 2 },
    ],
  ],
  [1, []],
  [3, []],
]);

function fsw(sw: number, label: string): FootswitchInfo {
  return {
    switch: sw,
    label,
    link_group: null,
    functions: [],
    level_params: [
      {
        group_id: "G1",
        node_id: `N${String(sw)}`,
        fender_id: "ACD_BluesDriver",
        parameter_id: "gain",
        current: 0.5,
      },
    ],
  };
}
const FOOTSWITCHES = new Map<number, FootswitchInfo[]>([
  [0, [fsw(4, "Solo")]], // tag FS5
  [3, [fsw(0, "Drive")]], // tag FS1
]);

function renderList(over: ListOverrides = {}) {
  const props: PresetListProps = {
    rows: ROWS,
    sel: new Set<string>(),
    pendingWhole: new Set<number>(),
    expanded: new Set<number>(),
    ready: true,
    activeSlot: null,
    filter: "",
    loading: false,
    scan: { scanning: false, percent: 100 },
    sceneInfo: SCENES,
    footswitchInfo: FOOTSWITCHES,
    graphByIndex: new Map(),
    onFilterChange: noop,
    onTogglePreset: noop,
    onToggleExpand: noop,
    onToggleKey: noop,
    onToggleAll: noop,
    ...over,
  };
  return render(
    <ThemeProvider>
      <PresetList {...props} />
    </ThemeProvider>,
  );
}

describe("PresetList (scene tree)", () => {
  it("shows the loading skeleton (no rows) while loading", () => {
    renderList({ loading: true });
    expect(screen.queryByText("Clean DI")).toBeNull();
    expect(screen.getByText("Reading presets…")).toBeTruthy();
  });

  it("renders preset rows + the empty marker", () => {
    renderList();
    expect(screen.getByText("Clean DI")).toBeTruthy();
    expect(screen.getByText("Plexi Crunch")).toBeTruthy();
    expect(screen.getByText("—— empty ——")).toBeTruthy();
  });

  it("shows the scan strip while the background read runs", () => {
    renderList({ scan: { scanning: true, percent: 42 } });
    expect(screen.getByText("Reading preset details…")).toBeTruthy();
    expect(screen.getByText("42%")).toBeTruthy();
  });

  it("parent meta: scene + footswitch breakdown once known", () => {
    renderList();
    // slot 0: 2 FS scenes + 1 footswitch; slot 3: 1 footswitch only; slot 1: nothing.
    expect(screen.getByText("2 scenes · 1 footswitch")).toBeTruthy();
    expect(screen.getByText("1 footswitch")).toBeTruthy();
    // A preset with no scenes AND no footswitches shows no meta (no "base only").
    expect(screen.queryByText("base only")).toBeNull();
  });

  it("parent meta: loading… while not ready", () => {
    renderList({ ready: false });
    expect(screen.getAllByText("loading…").length).toBeGreaterThan(0);
  });

  it("row-body click selects the whole preset (no recall)", async () => {
    const onTogglePreset = vi.fn();
    renderList({ onTogglePreset });
    await userEvent.click(screen.getByText("Plexi Crunch"));
    expect(onTogglePreset).toHaveBeenCalledWith(1);
  });

  it("checkbox selects the whole preset", async () => {
    const onTogglePreset = vi.fn();
    renderList({ onTogglePreset });
    await userEvent.click(screen.getAllByTitle("Select preset to level")[0]);
    expect(onTogglePreset).toHaveBeenCalledWith(0);
  });

  it("select-all header toggles every preset", async () => {
    const onToggleAll = vi.fn();
    renderList({ onToggleAll });
    await userEvent.click(screen.getByTitle("Select all"));
    expect(onToggleAll).toHaveBeenCalled();
  });

  it("caret expands → reveals Base + scene + footswitch sub-rows", async () => {
    const onToggleExpand = vi.fn();
    // collapsed: clicking the caret asks to expand.
    const { rerender } = renderList({ onToggleExpand });
    await userEvent.click(screen.getAllByTitle("Show Base + scenes")[0]);
    expect(onToggleExpand).toHaveBeenCalledWith(0);
    // expanded: the Base + FS scene + footswitch sub-rows render.
    rerender(
      <ThemeProvider>
        <PresetList
          rows={ROWS}
          sel={new Set<string>()}
          pendingWhole={new Set<number>()}
          expanded={new Set<number>([0])}
          ready
          activeSlot={null}
          filter=""
          scan={{ scanning: false, percent: 100 }}
          sceneInfo={SCENES}
          footswitchInfo={FOOTSWITCHES}
          graphByIndex={new Map()}
          onFilterChange={noop}
          onTogglePreset={noop}
          onToggleExpand={noop}
          onToggleKey={noop}
          onToggleAll={noop}
        />
      </ThemeProvider>,
    );
    expect(screen.getByText("Base")).toBeTruthy();
    expect(screen.getByText("Bright")).toBeTruthy();
    expect(screen.getByText("Lead")).toBeTruthy();
    // the footswitch rides as a sibling row: its label + accent FS tag.
    expect(screen.getByText("Solo")).toBeTruthy();
    expect(screen.getByText("FS5")).toBeTruthy();
  });

  it("shows the real per-preset CPU on base rows only — never scene sub-rows", () => {
    // ACD_TweedDeluxe costs 20.7% (model-cpu.json). A graph only for slot 0 means
    // exactly one base row carries a CPU readout; expanding it adds Base + scene +
    // footswitch sub-rows, and none of those gain a readout.
    const graph: ActiveGraph = {
      name: null,
      slot: null,
      template: null,
      split_mix: null,
      nodes: [
        {
          group_id: "G1",
          node_id: "N1",
          model: "ACD_TweedDeluxe",
          bypassed: false,
        },
      ],
      stages: [],
    };
    renderList({
      expanded: new Set<number>([0]),
      graphByIndex: new Map<number, ActiveGraph>([[0, graph]]),
    });
    expect(screen.getByText("20.7%")).toBeTruthy();
    // Exactly one readout total — the base row, not any of its expanded sub-rows.
    expect(screen.getAllByTitle(/^Preset CPU —/)).toHaveLength(1);
  });

  it("the caret is inert until ready", () => {
    renderList({ ready: false });
    // Not-ready carets carry the loading title, not the expand affordance.
    expect(screen.queryByTitle("Show Base + scenes")).toBeNull();
    expect(screen.getAllByTitle("Loading sounds…").length).toBeGreaterThan(0);
  });

  it("clicking a scene sub-row toggles that one key", async () => {
    const onToggleKey = vi.fn();
    renderList({ expanded: new Set<number>([0]), onToggleKey });
    await userEvent.click(screen.getByText("Bright"));
    expect(onToggleKey).toHaveBeenCalledWith("s0:0");
  });

  it("clicking a footswitch sub-row toggles its `f` key", async () => {
    const onToggleKey = vi.fn();
    renderList({ expanded: new Set<number>([0]), onToggleKey });
    await userEvent.click(screen.getByText("Solo"));
    expect(onToggleKey).toHaveBeenCalledWith("f0:0");
  });

  it("marks the active row", () => {
    const { container } = renderList({ activeSlot: 1 });
    expect(container.querySelector('[data-active="1"]')).toBeTruthy();
  });
});
