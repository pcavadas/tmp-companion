// PresetRow — the memo contract behind the ~500-row preset list (#315).
//
// Every tick builds a fresh `sel` Set, and PresetList hands that whole Set to every row.
// `PresetRow` is `memo`'d with `presetRowPropsEqual`, which compares `sel` only on the
// row's OWN keys, so one toggle re-renders the one row it touched. Locked two ways:
// the equality function itself, and a render-count probe through the real list (each
// row draws exactly one SlotLabel, so SlotLabel renders == row renders).

import { useCallback, useState } from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { PresetList } from "../views/PresetList";
import type { PresetRow as PresetRowData } from "../views/level/leveling";
import type { PresetRowProps } from "../views/level/PresetRow";
import { presetRowPropsEqual } from "../views/level/presetRowEqual";
import type { FootswitchInfo, SceneInfo } from "../lib/types";
import { renderWithTheme } from "./testKit";

// Render-count probe: record the slot of every SlotLabel render.
const { slotRenders } = vi.hoisted(() => ({ slotRenders: [] as number[] }));
vi.mock("../ui/SlotLabel", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../ui/SlotLabel")>();
  return {
    ...actual,
    SlotLabel: (props: Parameters<typeof actual.SlotLabel>[0]) => {
      slotRenders.push(props.index);
      return actual.SlotLabel(props);
    },
  };
});

const noop = () => {
  /* no-op */
};

describe("presetRowPropsEqual — row-scoped selection equality", () => {
  const row: PresetRowData = { slot: 4, name: "Lead", empty: false };
  const scenes: SceneInfo[] = [{ name: "Solo", fs: 1 }];
  const footswitches: FootswitchInfo[] = [];
  const base: PresetRowProps = {
    row,
    active: false,
    ready: true,
    scenes,
    footswitches,
    expanded: false,
    sel: new Set<string>(),
    pendingWhole: false,
    cpu: null,
    onTogglePreset: noop,
    onToggleExpand: noop,
    onToggleKey: noop,
  };

  it("holds when only ANOTHER row's keys changed in a fresh Set", () => {
    expect(
      presetRowPropsEqual(base, { ...base, sel: new Set(["p7", "s9:0"]) }),
    ).toBe(true);
  });

  it("breaks when this row's base, scene or footswitch key flips", () => {
    expect(presetRowPropsEqual(base, { ...base, sel: new Set(["p4"]) })).toBe(
      false,
    );
    expect(presetRowPropsEqual(base, { ...base, sel: new Set(["s4:0"]) })).toBe(
      false,
    );
    const withFsw: PresetRowProps = {
      ...base,
      footswitches: [
        {
          switch: 0,
          label: "Boost",
          link_group: null,
          functions: [],
          level_params: [],
        },
      ],
    };
    // A footswitch row reads its key even when not levelable — still compared.
    expect(
      presetRowPropsEqual(withFsw, { ...withFsw, sel: new Set(["f4:0"]) }),
    ).toBe(false);
  });

  it("breaks on any other prop change (identity)", () => {
    expect(presetRowPropsEqual(base, { ...base, expanded: true })).toBe(false);
    expect(presetRowPropsEqual(base, { ...base, scenes: [...scenes] })).toBe(
      false,
    );
    expect(
      presetRowPropsEqual(base, {
        ...base,
        onToggleKey: () => {
          /* a different function */
        },
      }),
    ).toBe(false);
    expect(presetRowPropsEqual(base, { ...base, selectTitle: "x" })).toBe(
      false,
    );
  });
});

// A stateful harness with stable toggles, like usePresetData's.
const N = 120;
const ROWS: PresetRowData[] = Array.from({ length: N }, (_, i) => ({
  slot: i,
  name: `Preset ${String(i)}`,
  empty: false,
}));
const SCENES = new Map<number, SceneInfo[]>(ROWS.map((r) => [r.slot, []]));
const NO_FSW = new Map<number, FootswitchInfo[]>();
const NO_SLOTS = new Set<number>();
const NO_GRAPHS = new Map();
const SCAN = { scanning: false };

function Harness() {
  const [sel, setSel] = useState<Set<string>>(() => new Set());
  const toggle = useCallback((key: string) => {
    setSel((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);
  const togglePreset = useCallback(
    (slot: number) => {
      toggle(`p${String(slot)}`);
    },
    [toggle],
  );
  return (
    <PresetList
      rows={ROWS}
      sel={sel}
      pendingWhole={NO_SLOTS}
      expanded={NO_SLOTS}
      ready
      activeSlot={null}
      filter=""
      scan={SCAN}
      sceneInfo={SCENES}
      footswitchInfo={NO_FSW}
      graphByIndex={NO_GRAPHS}
      onFilterChange={noop}
      onTogglePreset={togglePreset}
      onToggleExpand={noop}
      onToggleKey={toggle}
      onToggleAll={noop}
    />
  );
}

describe("regression: one preset tick re-renders only that row (#315)", () => {
  beforeEach(() => {
    slotRenders.length = 0;
  });

  it("ticking one row re-renders it alone, and the tick lands", async () => {
    const user = userEvent.setup();
    renderWithTheme(<Harness />);
    expect(slotRenders).toHaveLength(N);

    slotRenders.length = 0;
    await user.click(screen.getByRole("checkbox", { name: "Preset 7" }));
    expect(slotRenders).toEqual([7]);
    expect(screen.getByRole("checkbox", { name: "Preset 7" })).toHaveAttribute(
      "aria-checked",
      "true",
    );

    slotRenders.length = 0;
    await user.click(screen.getByRole("checkbox", { name: "Preset 42" }));
    expect(slotRenders).toEqual([42]);
    // The earlier tick survived the row skipping its re-render.
    expect(screen.getByRole("checkbox", { name: "Preset 7" })).toHaveAttribute(
      "aria-checked",
      "true",
    );

    slotRenders.length = 0;
    await user.click(screen.getByRole("checkbox", { name: "Preset 7" }));
    expect(slotRenders).toEqual([7]);
    expect(screen.getByRole("checkbox", { name: "Preset 7" })).toHaveAttribute(
      "aria-checked",
      "false",
    );
  });
});
