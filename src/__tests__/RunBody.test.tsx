// src/__tests__/RunBody.test.tsx — the live "leveling…" strip contract. While an item is
// active AND a live LUFS value is streaming, the readout shows the LATEST value and owns the
// right status cell; with no live value the row shows its normal "leveling…" status; a
// resolved row never shows the strip (the result row is the confirm). Mirrors the advisory
// semantics — no assertion that the live value ≈ the result value.

import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import type { ReactElement } from "react";

import { ThemeProvider } from "../theme/ThemeProvider";
import { RunBody } from "../views/overlays/RunBody";
import type { RunItem } from "../views/level/leveling";

const activeItem: RunItem = {
  key: "k0",
  slot: 0,
  presetName: "Reverse Delay",
  isBase: true,
  sceneSlot: null,
  sceneName: "",
  tag: null,
  instId: "",
  targetName: "Live",
  label: "Reverse Delay",
  status: "active",
};

function runBody(
  liveLufs: number | null,
  item: RunItem = activeItem,
): ReactElement {
  return (
    <ThemeProvider>
      <RunBody
        items={[item]}
        currentIndex={0}
        total={1}
        done={false}
        stopped={false}
        stopping={false}
        liveLufs={liveLufs}
        instrumentName={() => ""}
        onCancel={vi.fn()}
        onComplete={vi.fn()}
      />
    </ThemeProvider>
  );
}

describe("RunBody live measuring strip", () => {
  it("shows the readout (latest value + leveling…) during an active capture", () => {
    render(runBody(-23.1));
    expect(screen.getByText("−23.1")).toBeInTheDocument();
    expect(screen.getByText("LUFS")).toBeInTheDocument();
    expect(screen.getByText("leveling…")).toBeInTheDocument();
    // The readout owns the right cell — the pre-bars "connecting…" status is suppressed.
    expect(screen.queryByText("connecting…")).not.toBeInTheDocument();
  });

  it("renders the latest value when it updates (no smoothing, just the newest)", () => {
    const { rerender } = render(runBody(-30.0));
    expect(screen.getByText("−30.0")).toBeInTheDocument();
    rerender(runBody(-21.6));
    expect(screen.getByText("−21.6")).toBeInTheDocument();
    expect(screen.queryByText("−30.0")).not.toBeInTheDocument();
  });

  it("hides the readout and shows connecting… when nothing is streaming", () => {
    render(runBody(null));
    expect(screen.queryByText("leveling…")).not.toBeInTheDocument();
    expect(screen.getByText("connecting…")).toBeInTheDocument();
  });

  it("never shows the strip on a resolved row (the result row is the confirm)", () => {
    const resolved: RunItem = {
      ...activeItem,
      status: "result",
      outcome: "done",
      value: -18.0,
    };
    // Even if a late event left liveLufs non-null, a non-active row shows no strip.
    render(runBody(-21.6, resolved));
    expect(screen.queryByText("leveling…")).not.toBeInTheDocument();
    expect(screen.getByText("done · −18.0")).toBeInTheDocument();
  });
});
