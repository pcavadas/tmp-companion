// src/__tests__/CopyPlace.test.tsx — Place blocks' cards (look B): the source card's line
// and CPU cost, a target card's "Changed" badge, its greyed gaps and their reason when the
// chosen block can't fit, its danger edge when the edit breaks a limit, and the block
// menu (Replace with the chosen block, Remove).

import { describe, expect, it, vi } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { SourceCard } from "../views/copy/SourceCard";
import { TargetEditCard } from "../views/copy/TargetEditCard";
import {
  applyEditOp,
  initEdit,
  type PresetEdit,
} from "../views/copy/copyModel";
import type { ActiveGraph, GraphNode } from "../lib/types";
import { light } from "../theme/tokens";
import { renderWithTheme, toRgb } from "./testKit";

const node = (id: string, model: string): GraphNode => ({
  group_id: "G1",
  node_id: id,
  model,
  bypassed: false,
  params: {},
});

function graph(models: string[], name = "T"): ActiveGraph {
  const nodes = models.map((m, i) => node(`n${String(i)}`, m));
  return {
    name,
    slot: 4,
    template: "gtrSeries",
    split_mix: null,
    nodes,
    stages: [{ kind: "series", blocks: nodes }],
  };
}

const editOf = (models: string[]): PresetEdit =>
  initEdit([4], () => graph(models))[4];

function firstUid(e: PresetEdit, i: number): string {
  const st = e.graph.stages[0];
  if (st.kind !== "series") throw new Error("series expected");
  return st.blocks[i].uid;
}

const noop = () => undefined;

function renderCard(
  edit: PresetEdit,
  over: Partial<Parameters<typeof TargetEditCard>[0]> = {},
) {
  const props = {
    slot: 4,
    name: "Scooped Verse",
    edit,
    openUid: null,
    pick: null,
    blocked: null,
    onTapBlock: vi.fn(),
    onRemove: vi.fn(),
    onApply: vi.fn(),
    onCloseMenu: vi.fn(),
    ...over,
  };
  renderWithTheme(<TargetEditCard {...props} />);
  return props;
}

describe("SourceCard", () => {
  const from = {
    slot: 7,
    name: "Plexi Crunch",
    onUnit: true,
    graph: graph(["ACD_TubeScreamer", "ACD_TwinReverb65NoFx"]),
    cpu: null,
  };

  it("asks for a block until one is chosen, and marks the loaded preset", () => {
    renderWithTheme(<SourceCard from={from} pick={null} onPick={noop} />);
    expect(screen.getByText("Copy from Plexi Crunch")).toBeTruthy();
    expect(screen.getByText("008")).toBeTruthy();
    expect(screen.getByText("Loaded")).toBeTruthy();
    expect(screen.getByText("Choose a block to copy.")).toBeTruthy();
    expect(screen.queryByText(/CPU/)).toBeNull();
  });

  it("names the chosen block and what it costs", () => {
    renderWithTheme(
      <SourceCard
        from={from}
        pick={{ uid: "x", model: "ACD_TubeScreamer" }}
        onPick={noop}
      />,
    );
    expect(screen.getByText("Greenbox 8 is ready to copy.")).toBeTruthy();
    expect(screen.getByText("Adds 3.3% CPU")).toBeTruthy();
  });

  it("hides the cost of a block the CPU table doesn't know", () => {
    renderWithTheme(
      <SourceCard
        from={from}
        pick={{ uid: "x", model: "ACD_NoSuchBlock" }}
        onPick={noop}
      />,
    );
    expect(screen.queryByText(/Adds/)).toBeNull();
  });

  it("chooses a block by its tile", async () => {
    const onPick = vi.fn();
    const user = userEvent.setup();
    renderWithTheme(<SourceCard from={from} pick={null} onPick={onPick} />);
    const tile = document.querySelector<HTMLElement>(
      '[data-block-tile="ACD_TwinReverb65NoFx"]',
    );
    if (!tile) throw new Error("no tile");
    await user.click(tile);
    expect(onPick).toHaveBeenCalledWith(
      expect.any(String),
      "ACD_TwinReverb65NoFx",
    );
  });
});

describe("TargetEditCard", () => {
  it("shows Changed only once the preset differs, and its CPU", () => {
    const e = editOf(["ACD_DynaComp", "ACD_TMSmallHall"]);
    renderCard(e);
    expect(screen.queryByText("Changed")).toBeNull();
    expect(screen.getByText("CPU")).toBeTruthy();
    expect(screen.getByText("12.7%")).toBeTruthy();
  });

  it("badges an edited preset Changed", () => {
    let e = editOf(["ACD_DynaComp", "ACD_TMSmallHall"]);
    e = applyEditOp(e, firstUid(e, 0), "after", "ACD_DynaComp");
    renderCard(e);
    expect(screen.getByText("Changed")).toBeTruthy();
  });

  it("greys the gaps and says why when the chosen block can't fit", () => {
    const e = editOf(["ACD_GuitarSynth", "ACD_POG"]); // 65% of 76.5
    renderCard(e, { pick: { uid: "s", model: "ACD_GuitarSynth" } });
    const gaps = document.querySelectorAll("[data-gap]");
    expect(gaps).toHaveLength(3);
    gaps.forEach((g) => {
      expect(g).toBeDisabled();
    });
    expect(screen.getByText(/^There isn't enough CPU left for /)).toBeTruthy();
  });

  it("leaves the gaps open when the chosen block fits", () => {
    const e = editOf(["ACD_GuitarSynth", "ACD_POG"]);
    renderCard(e, { pick: { uid: "s", model: "ACD_DynaComp" } });
    document.querySelectorAll("[data-gap]").forEach((g) => {
      expect(g).toBeEnabled();
    });
    expect(screen.queryByText(/enough CPU/)).toBeNull();
  });

  it("turns its edge danger and says why when the edit breaks a limit", () => {
    const e = editOf(["ACD_DynaComp"]);
    renderCard(e, { blocked: "ComboHalfStackCabinetsLimit" });
    expect(screen.getByText("Only two cabinets fit in a preset.")).toBeTruthy();
    const card = document.querySelector<HTMLElement>("[data-target-card]");
    expect(card?.style.border).toContain(toRgb(light.danger));
  });

  it("opens a menu with Replace (the chosen block) and Remove", async () => {
    const e = editOf(["ACD_DynaComp", "ACD_TMSmallHall"]);
    const user = userEvent.setup();
    const props = renderCard(e, {
      openUid: firstUid(e, 1),
      pick: { uid: "s", model: "ACD_TubeScreamer" },
    });
    await user.click(
      screen.getByRole("menuitem", { name: "Replace with Greenbox 8" }),
    );
    expect(props.onApply).toHaveBeenCalledWith(
      firstUid(e, 1),
      "replace",
      "ACD_TubeScreamer",
    );
    await user.click(screen.getByRole("menuitem", { name: "Remove" }));
    expect(props.onRemove).toHaveBeenCalledWith(firstUid(e, 1));
  });

  it("offers only Remove while no block is chosen", () => {
    const e = editOf(["ACD_DynaComp"]);
    renderCard(e, { openUid: firstUid(e, 0) });
    expect(screen.getAllByRole("menuitem").map((m) => m.textContent)).toEqual([
      "Remove",
    ]);
  });

  it("leaves Replace out when the swap would break a limit", () => {
    const e = editOf(["ACD_GuitarSynth", "ACD_POG", "ACD_DynaComp"]);
    renderCard(e, {
      openUid: firstUid(e, 2), // DynaComp 4.8 → GuitarSynth 36: over the budget
      pick: { uid: "s", model: "ACD_GuitarSynth" },
    });
    expect(
      screen.queryByRole("menuitem", { name: /^Replace with/ }),
    ).toBeNull();
    expect(screen.getByRole("menuitem", { name: "Remove" })).toBeTruthy();
  });
});
