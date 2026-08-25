// src/__tests__/RunBody.test.tsx — the run step's live-capture contract and its columned
// table. The live readout lives in the step HEADER (once per run, never per row) and shows
// the LATEST streamed value; a row states its own scene name and target rather than one
// concatenated label. Mirrors the advisory semantics — no assertion that the live value ≈
// the result value.

import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import type { ReactElement } from "react";

import { ThemeProvider } from "../theme/ThemeProvider";
import { RunBody } from "../views/overlays/RunBody";
import type { RunItem } from "../views/level/leveling";

const activeItem: RunItem = {
  key: "k0",
  slot: 27,
  presetName: "E2E Hiwatt 3S",
  isBase: false,
  sceneSlot: 1,
  sceneName: "Rhythm Crunch",
  tag: "FS7",
  instId: "",
  targetName: "Stage",
  status: "active",
};

function runBody(
  liveLufs: number | null,
  items: RunItem[] = [activeItem],
): ReactElement {
  return (
    <ThemeProvider>
      <RunBody
        items={items}
        currentIndex={0}
        total={items.length}
        done={false}
        stopped={false}
        stopping={false}
        liveLufs={liveLufs}
        liveTrace={[]}
        instrumentName={() => "Telecaster"}
        targetLufsByName={() => -26}
        onCancel={vi.fn()}
        onComplete={vi.fn()}
      />
    </ThemeProvider>
  );
}

describe("RunBody live measuring strip", () => {
  it("shows the readout (latest value + measuring caption) during an active capture", () => {
    render(runBody(-23.1));
    expect(screen.getByText("−23.1")).toBeInTheDocument();
    expect(screen.getByText("LUFS")).toBeInTheDocument();
    expect(screen.getByText("measuring FS7")).toBeInTheDocument();
    // The row's Result cell keeps its own live number; "connecting…" is only the
    // pre-capture state.
    expect(screen.getByText("leveling · −23.1")).toBeInTheDocument();
    expect(screen.queryByText("connecting…")).not.toBeInTheDocument();
  });

  it("renders the latest value when it updates (no smoothing, just the newest)", () => {
    const { rerender } = render(runBody(-30.0));
    expect(screen.getByText("−30.0")).toBeInTheDocument();
    rerender(runBody(-21.6));
    expect(screen.getByText("−21.6")).toBeInTheDocument();
    expect(screen.queryByText("−30.0")).not.toBeInTheDocument();
  });

  it("shows connecting… and hides the readout when nothing is streaming", () => {
    render(runBody(null));
    expect(screen.getByText("connecting…")).toBeInTheDocument();
    // Opacity-gated, not unmounted — the header must not change height between items.
    const meter = screen
      .getByText("LUFS")
      .closest<HTMLElement>("div[aria-hidden]");
    expect(meter?.getAttribute("aria-hidden")).toBe("true");
    expect(meter?.style.opacity).toBe("0");
  });

  // GATE (user report, preset 30 "Plumes+BD2+OCD"): the ceiling prepass measures every
  // footswitch row before anything is solved, ~10 s a row, and used to be invisible — one
  // row held the highlight for the whole minute while the unit stepped through four, so the
  // run read as "leveling the wrong footswitch". The backend now captions those rows
  // (`leveller::PREPASS_ACTIVE_MSG`); the caption must reach the row as its VERB, or the
  // phase is still indistinguishable from the solve.
  it("uses the backend caption as the verb while a capture streams", () => {
    render(runBody(-18.9, [{ ...activeItem, activeMessage: "measuring" }]));
    expect(screen.getByText("measuring · −18.9")).toBeInTheDocument();
    expect(screen.queryByText("leveling · −18.9")).not.toBeInTheDocument();
  });

  it("keeps the default verb for a solve row, which carries no caption", () => {
    render(runBody(-23.1));
    expect(screen.getByText("leveling · −23.1")).toBeInTheDocument();
  });

  // The other half of the caption contract: a message sent while NOTHING streams is a note,
  // not a verb, and must never be composed with a number.
  it("renders a caption verbatim when nothing is streaming", () => {
    const note = "waiting for the device to commit the previous save…";
    render(runBody(null, [{ ...activeItem, activeMessage: note }]));
    expect(screen.getByText(note)).toBeInTheDocument();
    expect(screen.queryByText("connecting…")).not.toBeInTheDocument();
  });

  it("renders the live readout ONCE, in the header — never per row", () => {
    const second: RunItem = { ...activeItem, key: "k1", status: "queued" };
    render(runBody(-23.1, [activeItem, second]));
    expect(screen.getAllByText("LUFS")).toHaveLength(1);
  });

  it("never shows a live number on a resolved row (the result row is the confirm)", () => {
    const resolved: RunItem = {
      ...activeItem,
      status: "result",
      outcome: "done",
      value: -18.0,
    };
    // Even if a late event left liveLufs non-null, a non-active row shows its result.
    render(runBody(-21.6, [resolved]));
    expect(screen.queryByText("leveling · −21.6")).not.toBeInTheDocument();
    expect(screen.getByText("done · −18.0")).toBeInTheDocument();
  });
});

describe("RunBody columned rows", () => {
  it("states the scene name and its own target, not a concatenated label", () => {
    render(runBody(null));
    // The sound's own name owns the column; its preset is the mono sub-line.
    expect(screen.getByText("Rhythm Crunch")).toBeInTheDocument();
    expect(screen.getByText("028 · E2E Hiwatt 3S")).toBeInTheDocument();
    expect(
      screen.queryByText("E2E Hiwatt 3S · Rhythm Crunch"),
    ).not.toBeInTheDocument();
    // Every row states what it is aiming at.
    expect(screen.getByText("Stage · −26.0")).toBeInTheDocument();
  });

  it("prefers a row's targetOverrideLufs over the named target's value", () => {
    // The reachable-common-target fallback overrides a row; the cell must state what the
    // row is ACTUALLY aiming at, not the store's value for the target's name.
    const overridden: RunItem = { ...activeItem, targetOverrideLufs: -29.4 };
    render(runBody(null, [overridden]));
    expect(screen.getByText("Stage · −29.4")).toBeInTheDocument();
  });

  it("labels the columns", () => {
    render(runBody(null));
    for (const head of ["Sound", "Instrument", "Target", "Result"]) {
      expect(screen.getByText(head)).toBeInTheDocument();
    }
  });

  // The Result cell is an if-chain whose fallthrough is "done", so a miss state with no
  // branch of its own reports as LEVELED — the reason unconverged needs one here too.
  it("states an unconverged row as off target, not done and not clamped", () => {
    const missed: RunItem = {
      ...activeItem,
      status: "result",
      outcome: "unconverged",
      value: -24.3,
    };
    render(runBody(null, [missed]));
    expect(screen.getByText("off target · −24.3")).toBeInTheDocument();
    expect(screen.queryByText("done · −24.3")).not.toBeInTheDocument();
    expect(screen.queryByText(/clamped/)).not.toBeInTheDocument();
  });
});
