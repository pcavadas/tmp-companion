// PresetList (look B, scene-tree selection) — the row contract.
//
// Locks: the loading skeleton; the filter bar's caption (count, "N of M shown", reading
// progress); the parent-row meta (Reading… / "2 scenes, 1 footswitch" / "1 of 4 chosen"
// / nothing); the Checkbox choosing a whole preset; the NAME opening a preset with scenes
// and choosing any other; "Select all N shown" acting on the presets the filter shows
// only; the no-match state; empty slots; Caret → main sound + scene + footswitch
// sub-rows → per-sound toggle; no carets until `ready`; the "Loaded" badge; CPU on preset
// rows only; and BUG 1 (a footswitch with no volume knob is shown, disabled, reasoned).

import { describe, it, expect, vi } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { PresetList, type PresetListProps } from "../views/PresetList";
import type { PresetRow } from "../views/level/leveling";
import type { ActiveGraph, FootswitchInfo, SceneInfo } from "../lib/types";
import { renderWithTheme } from "./testKit";

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

// slot 0: 2 scenes + 1 footswitch; slot 1: nothing; slot 3: 1 footswitch only —
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
        class: "level_linear",
      },
    ],
  };
}
// A footswitch with NO level-class parameter (real incident: "Friedman HBE" FS2
// "PHASER" → ACD_PhaserP90) — BUG 1: must still render as a row, visibly disabled
// with a reason, never silently dropped.
function fswNoLevel(sw: number, label: string): FootswitchInfo {
  return {
    switch: sw,
    label,
    link_group: null,
    functions: [
      {
        func: "on-off",
        group_id: "G2",
        node_id: `N${String(sw)}`,
        fender_id: "ACD_PhaserP90",
        parameter_id: null,
        value_a: null,
        value_b: null,
        is_active: false,
      },
    ],
    level_params: [],
  };
}
const FOOTSWITCHES = new Map<number, FootswitchInfo[]>([
  [0, [fsw(4, "Solo")]], // footswitch 5
  [3, [fsw(0, "Drive")]], // footswitch 1
]);

function listProps(over: ListOverrides = {}): PresetListProps {
  return {
    rows: ROWS,
    sel: new Set<string>(),
    pendingWhole: new Set<number>(),
    expanded: new Set<number>(),
    ready: true,
    activeSlot: null,
    filter: "",
    loading: false,
    scan: { scanning: false },
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
}

function renderList(over: ListOverrides = {}) {
  return renderWithTheme(<PresetList {...listProps(over)} />);
}

const selectAll = () => screen.getByRole("checkbox", { name: /^Select all/ });

describe("PresetList (look B scene tree)", () => {
  it("shows the loading skeleton (no rows) while loading, with select-all and the filter disabled", () => {
    renderList({ loading: true });
    expect(screen.queryByText("Clean DI")).toBeNull();
    expect(screen.getByText("Reading presets…")).toBeInTheDocument();
    expect(selectAll()).toBeDisabled();
    expect(screen.getByLabelText("Filter by name or slot")).toBeDisabled();
  });

  it('renders preset rows and an empty slot as "Empty" with no checkbox', () => {
    renderList();
    expect(screen.getByText("Clean DI")).toBeInTheDocument();
    expect(screen.getByText("Plexi Crunch")).toBeInTheDocument();
    expect(screen.getByText("Empty")).toBeInTheDocument();
    // Three presets, three preset checkboxes (the empty slot has none).
    expect(screen.getAllByRole("checkbox", { name: /^Choose / })).toHaveLength(
      3,
    );
  });

  it('captions the count of presets (empty slots excluded), and "N of M shown" while filtering', () => {
    const { rerender } = renderList();
    expect(screen.getByText("3 presets")).toBeInTheDocument();
    rerender(<PresetList {...listProps({ filter: "tweed" })} />);
    expect(screen.getByText("1 of 3 shown")).toBeInTheDocument();
  });

  it("shows the reading progress in the filter bar while the scan runs", () => {
    // The % comes from the scan's own progress store (0 until a tick arrives;
    // libraryScan.test.ts drives the ticks), not from a prop.
    renderList({ scan: { scanning: true }, ready: false });
    expect(screen.getByText("Reading preset details")).toBeInTheDocument();
    expect(screen.getByText("0%")).toBeInTheDocument();
  });

  it("parent meta: what the preset holds once known, joined with a comma", () => {
    renderList();
    expect(screen.getByText("2 scenes, 1 footswitch")).toBeInTheDocument();
    expect(screen.getByText("1 footswitch")).toBeInTheDocument();
  });

  it('parent meta: "Reading…" and no carets while the scan reads', () => {
    renderList({ ready: false });
    expect(screen.getAllByText("Reading…")).toHaveLength(3);
    expect(screen.queryByRole("button", { name: /^Open / })).toBeNull();
  });

  it('parent meta: "1 of 4 chosen" on a partly chosen preset', () => {
    renderList({ sel: new Set(["s0:1"]) });
    expect(screen.getByText("1 of 4 chosen")).toBeInTheDocument();
    expect(
      screen.getByRole("checkbox", { name: "Choose Clean DI" }),
    ).toHaveAttribute("aria-checked", "mixed");
  });

  it("the checkbox chooses the whole preset", async () => {
    const onTogglePreset = vi.fn();
    renderList({ onTogglePreset });
    await userEvent.click(
      screen.getByRole("checkbox", { name: "Choose Clean DI" }),
    );
    expect(onTogglePreset).toHaveBeenCalledWith(0);
  });

  describe("the preset's name", () => {
    it("opens a preset that has scenes", async () => {
      const onToggleExpand = vi.fn();
      const onTogglePreset = vi.fn();
      renderList({ onToggleExpand, onTogglePreset });
      await userEvent.click(screen.getByRole("button", { name: "Clean DI" }));
      expect(onToggleExpand).toHaveBeenCalledWith(0);
      expect(onTogglePreset).not.toHaveBeenCalled();
    });

    it("chooses a preset with footswitches but no scenes", async () => {
      const onToggleExpand = vi.fn();
      const onTogglePreset = vi.fn();
      renderList({ onToggleExpand, onTogglePreset });
      await userEvent.click(screen.getByRole("button", { name: "Tweed Warm" }));
      expect(onTogglePreset).toHaveBeenCalledWith(3);
      expect(onToggleExpand).not.toHaveBeenCalled();
    });

    it("chooses a preset with no scenes or footswitches", async () => {
      const onTogglePreset = vi.fn();
      renderList({ onTogglePreset });
      await userEvent.click(
        screen.getByRole("button", { name: "Plexi Crunch" }),
      );
      expect(onTogglePreset).toHaveBeenCalledWith(1);
    });

    it("chooses the whole preset while the scan still reads its scenes", async () => {
      const onToggleExpand = vi.fn();
      const onTogglePreset = vi.fn();
      renderList({ ready: false, onToggleExpand, onTogglePreset });
      await userEvent.click(screen.getByRole("button", { name: "Clean DI" }));
      expect(onTogglePreset).toHaveBeenCalledWith(0);
      expect(onToggleExpand).not.toHaveBeenCalled();
    });
  });

  describe("Select all N shown", () => {
    it("is named for the presets shown and hands their slots to the toggle", async () => {
      const onToggleAll = vi.fn();
      const { rerender } = renderList({ onToggleAll });
      await userEvent.click(
        screen.getByRole("checkbox", { name: "Select all 3 shown" }),
      );
      expect(onToggleAll).toHaveBeenLastCalledWith([0, 1, 3]);
      rerender(<PresetList {...listProps({ onToggleAll, filter: "tweed" })} />);
      await userEvent.click(
        screen.getByRole("checkbox", { name: "Select all 1 shown" }),
      );
      expect(onToggleAll).toHaveBeenLastCalledWith([3]);
    });

    it("reads its state over the shown presets only", () => {
      // Clean DI is fully chosen but hidden by the filter: the one shown preset (Tweed
      // Warm) is not chosen, so the box is off — not mixed.
      const all = ["p0", "s0:0", "s0:1", "f0:0"];
      const { rerender } = renderList({
        filter: "tweed",
        sel: new Set(all),
      });
      expect(selectAll()).toHaveAttribute("aria-checked", "false");
      // Tweed Warm chosen too → every SHOWN preset is on.
      rerender(
        <PresetList
          {...listProps({
            filter: "tweed",
            sel: new Set([...all, "p3", "f3:0"]),
          })}
        />,
      );
      expect(selectAll()).toHaveAttribute("aria-checked", "true");
      // No filter: Plexi Crunch is not chosen → mixed.
      rerender(
        <PresetList {...listProps({ sel: new Set([...all, "p3", "f3:0"]) })} />,
      );
      expect(selectAll()).toHaveAttribute("aria-checked", "mixed");
    });

    it("counts a preset ticked whole during the scan as chosen", () => {
      renderList({
        ready: false,
        sceneInfo: new Map(),
        footswitchInfo: new Map(),
        pendingWhole: new Set([0, 1, 3]),
      });
      expect(selectAll()).toHaveAttribute("aria-checked", "true");
    });
  });

  it("filter matching nothing: a message, Clear filter, and select-all disabled", async () => {
    const onFilterChange = vi.fn();
    renderList({ filter: "fuzzz ", onFilterChange });
    expect(screen.getByText('No presets match "fuzzz"')).toBeInTheDocument();
    expect(
      screen.getByText("Search by preset name or slot number."),
    ).toBeInTheDocument();
    expect(screen.getByText("0 of 3 shown")).toBeInTheDocument();
    expect(
      screen.getByRole("checkbox", { name: "Select all 0 shown" }),
    ).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "Clear filter" }));
    expect(onFilterChange).toHaveBeenCalledWith("");
  });

  it("caret opens → main sound + scene + footswitch sub-rows", async () => {
    const onToggleExpand = vi.fn();
    const { rerender } = renderList({ onToggleExpand });
    await userEvent.click(
      screen.getByRole("button", { name: "Open Clean DI" }),
    );
    expect(onToggleExpand).toHaveBeenCalledWith(0);
    rerender(<PresetList {...listProps({ expanded: new Set<number>([0]) })} />);
    expect(
      screen.getByRole("button", { name: "Close Clean DI" }),
    ).toHaveAttribute("aria-expanded", "true");
    for (const sound of ["Main sound", "Bright", "Lead", "Solo"]) {
      expect(
        screen.getByRole("checkbox", { name: `Choose ${sound} in Clean DI` }),
      ).toBeInTheDocument();
    }
    // A footswitch says which one it is, in words.
    expect(screen.getByText("footswitch 5")).toBeInTheDocument();
  });

  it("only a preset with scenes or footswitches has a caret", () => {
    renderList();
    expect(screen.getByRole("button", { name: "Open Clean DI" })).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Open Tweed Warm" }),
    ).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "Open Plexi Crunch" }),
    ).toBeNull();
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

  it("shows the real per-preset CPU on preset rows only — never on sub-rows", () => {
    // ACD_TweedDeluxe costs 20.7% (model-cpu.json). A graph only for slot 0 means
    // exactly one preset row carries a CPU figure; opening it adds the main sound +
    // scene + footswitch sub-rows, and none of those gain one.
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
          params: {},
        },
      ],
      stages: [],
    };
    renderList({
      expanded: new Set<number>([0]),
      graphByIndex: new Map<number, ActiveGraph>([[0, graph]]),
    });
    expect(screen.getAllByText("20.7%")).toHaveLength(1);
  });

  it("badges the preset loaded on the unit", () => {
    renderList({ activeSlot: 1 });
    expect(screen.getByText("Loaded")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /^Plexi Crunch/ }),
    ).toHaveTextContent("Loaded");
  });

  // ── BUG 1: a footswitch with no level-class parameter — shown, disabled, reasoned ──
  describe("a footswitch with no volume knob (BUG 1)", () => {
    const FSW_WITH_UNLEVELABLE = new Map<number, FootswitchInfo[]>([
      [0, [fsw(4, "Solo"), fswNoLevel(1, "PHASER")]],
    ]);

    it("is still shown as a row, never silently omitted", async () => {
      renderList({
        footswitchInfo: FSW_WITH_UNLEVELABLE,
        expanded: new Set<number>([0]),
      });
      expect(await screen.findByText("PHASER")).toBeInTheDocument();
    });

    it("is visibly disabled and NOT toggleable", async () => {
      const onToggleKey = vi.fn();
      renderList({
        footswitchInfo: FSW_WITH_UNLEVELABLE,
        expanded: new Set<number>([0]),
        onToggleKey,
      });
      const row = screen.getByRole("checkbox", {
        name: "Choose PHASER in Clean DI",
      });
      expect(row).toBeDisabled();
      await userEvent.click(row);
      expect(onToggleKey).not.toHaveBeenCalled();
    });

    it("carries a short reason the user can understand", async () => {
      renderList({
        footswitchInfo: FSW_WITH_UNLEVELABLE,
        expanded: new Set<number>([0]),
      });
      expect(
        await screen.findByText("No volume knob to level"),
      ).toBeInTheDocument();
    });

    it("is excluded from the preset's count (childKeys/row-builder agree)", () => {
      renderList({
        footswitchInfo: FSW_WITH_UNLEVELABLE,
        // No scenes here so the only children are the two footswitches — isolates
        // the count to exactly what's under test.
        sceneInfo: new Map([[0, []]]),
        expanded: new Set<number>([0]),
        // Everything selectABLE already ticked (main sound + the levelable Solo switch)
        // — if PHASER were still counted as selectable, this would read partly chosen
        // ("2 of 3 chosen") instead of fully checked.
        sel: new Set(["p0", "f0:0"]),
      });
      expect(screen.queryByText(/chosen$/)).toBeNull();
      expect(
        screen.getByRole("checkbox", { name: "Choose Clean DI" }),
      ).toHaveAttribute("aria-checked", "true");
    });
  });
});
