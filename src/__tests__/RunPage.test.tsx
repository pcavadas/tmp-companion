// src/__tests__/RunPage.test.tsx — the run step's live-capture contract and its
// per-preset grouped rows (design handoff 1a). The live readout lives in the step
// HEADER (once per run, never per row) and shows the LATEST streamed value; a row
// states its own scene name and target rather than one concatenated label — the
// preset name and slot now live once on the GROUP header, not repeated per row.
// Mirrors the advisory semantics — no assertion that the live value ≈ the result
// value. Replaces `RunBody.test.tsx`.

import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement } from "react";

import { ThemeProvider } from "../theme/ThemeProvider";
import { RunPage } from "../views/level/RunPage";
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

function runPage(
  liveLufs: number | null,
  items: RunItem[] = [activeItem],
  tailMessage: string | null = null,
): ReactElement {
  return (
    <ThemeProvider>
      <RunPage
        items={items}
        currentIndex={0}
        total={items.length}
        done={false}
        stopped={false}
        stopping={false}
        liveLufs={liveLufs}
        liveTrace={[]}
        tailMessage={tailMessage}
        instrumentName={() => "Telecaster"}
        targetLufsByName={() => -26}
        onCancel={vi.fn()}
        onComplete={vi.fn()}
      />
    </ThemeProvider>
  );
}

describe("RunPage live measuring strip", () => {
  it("shows the readout (latest value + measuring caption) during an active capture", () => {
    render(runPage(-23.1));
    expect(screen.getByText("−23.1")).toBeInTheDocument();
    expect(screen.getByText("LUFS")).toBeInTheDocument();
    expect(screen.getByText("measuring FS7")).toBeInTheDocument();
    // The row's own status cell keeps its own live number; "connecting…" is only
    // the pre-capture state.
    expect(screen.getByText("leveling · −23.1")).toBeInTheDocument();
    expect(screen.queryByText("connecting…")).not.toBeInTheDocument();
  });

  it("renders the latest value when it updates (no smoothing, just the newest)", () => {
    const { rerender } = render(runPage(-30.0));
    expect(screen.getByText("−30.0")).toBeInTheDocument();
    rerender(runPage(-21.6));
    expect(screen.getByText("−21.6")).toBeInTheDocument();
    expect(screen.queryByText("−30.0")).not.toBeInTheDocument();
  });

  it("shows a dash placeholder (never a fabricated 0.0) and the measuring caption while nothing streams yet, with the row itself connecting", () => {
    render(runPage(null));
    expect(screen.getByText("—")).toBeInTheDocument();
    expect(screen.queryByText("0.0")).not.toBeInTheDocument();
    expect(screen.getByText("measuring FS7")).toBeInTheDocument();
    // The header readout has no live value to show yet, but the ROW's own status
    // cell still states its pre-capture state plainly.
    expect(screen.getByText("connecting…")).toBeInTheDocument();
  });

  // GATE (user report, preset 30 "Plumes+BD2+OCD"): the ceiling prepass measures every
  // footswitch row before anything is solved, ~10 s a row, and used to be invisible — one
  // row held the highlight for the whole minute while the unit stepped through four, so the
  // run read as "leveling the wrong footswitch". The backend now captions those rows
  // (`leveller::PREPASS_ACTIVE_MSG`); the caption must reach the row as its VERB, or the
  // phase is still indistinguishable from the solve.
  it("uses the backend caption as the verb while a capture streams", () => {
    render(runPage(-18.9, [{ ...activeItem, activeMessage: "measuring" }]));
    expect(screen.getByText("measuring · −18.9")).toBeInTheDocument();
    expect(screen.queryByText("leveling · −18.9")).not.toBeInTheDocument();
  });

  it("keeps the default verb for a solve row, which carries no caption", () => {
    render(runPage(-23.1));
    expect(screen.getByText("leveling · −23.1")).toBeInTheDocument();
  });

  // The other half of the caption contract: a message sent while NOTHING streams is a note,
  // not a verb, and must never be composed with a number.
  it("renders a caption verbatim when nothing is streaming", () => {
    const note = "waiting for the device to commit the previous save…";
    render(runPage(null, [{ ...activeItem, activeMessage: note }]));
    expect(screen.getByText(note)).toBeInTheDocument();
    expect(screen.queryByText("connecting…")).not.toBeInTheDocument();
  });

  it("renders the live readout ONCE, in the header — never per row", () => {
    const second: RunItem = { ...activeItem, key: "k1", status: "queued" };
    render(runPage(-23.1, [activeItem, second]));
    expect(screen.getAllByText("LUFS")).toHaveLength(1);
  });

  it("never shows a live number on a resolved row (the result row is the confirm)", async () => {
    const resolved: RunItem = {
      ...activeItem,
      status: "result",
      outcome: "done",
      value: -18.0,
    };
    // Even if a late event left liveLufs non-null, a non-active row shows its result.
    render(runPage(-21.6, [resolved]));
    // No item is "active", so the group doesn't auto-open — expand it to reach the row.
    await userEvent.click(screen.getByText("E2E Hiwatt 3S"));
    expect(screen.queryByText("leveling · −21.6")).not.toBeInTheDocument();
    // A plain match's checkmark glyph already says "done" — the text is just the
    // reading (unlike clamped/off-target/skipped, which each carry their own prefix).
    expect(screen.getByText("−18.0 LUFS")).toBeInTheDocument();
  });
});

describe("RunPage grouped rows", () => {
  it("states the scene name on its own row, and the preset name + slot once on the group", () => {
    render(runPage(null));
    // The sound's own name owns its row.
    expect(screen.getByText("Rhythm Crunch")).toBeInTheDocument();
    // The preset name and slot live once on the GROUP header, never repeated as a
    // per-row concatenated label.
    expect(screen.getByText("E2E Hiwatt 3S")).toBeInTheDocument();
    expect(screen.getByText("028")).toBeInTheDocument();
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
    render(runPage(null, [overridden]));
    expect(screen.getByText("Stage · −29.4")).toBeInTheDocument();
  });

  // The Result cell is an if-chain whose fallthrough is "done", so a miss state with no
  // branch of its own reports as LEVELED — the reason unconverged needs one here too.
  it("states an unconverged row as off target, not done and not clamped", async () => {
    const missed: RunItem = {
      ...activeItem,
      status: "result",
      outcome: "unconverged",
      value: -24.3,
    };
    render(runPage(null, [missed]));
    // No item is "active", so the group doesn't auto-open — expand it to reach the row.
    await userEvent.click(screen.getByText("E2E Hiwatt 3S"));
    expect(screen.getByText("off target · −24.3")).toBeInTheDocument();
    expect(screen.queryByText("done · −24.3")).not.toBeInTheDocument();
    expect(screen.queryByText(/clamped/)).not.toBeInTheDocument();
  });
});

// Issue 6b: the batch-wide tail caption ("Saving…" / "Verifying…") has no row of its
// own — it surfaces once, under the progress bar.
describe("RunPage tail caption (issue 6b)", () => {
  const defaultCaption = /preset.*sound.*saves automatically/;

  it("renders tailMessage in place of the default caption while set", () => {
    render(runPage(null, [activeItem], "Saving…"));
    expect(screen.getByText("Saving…")).toBeInTheDocument();
    expect(screen.queryByText(defaultCaption)).not.toBeInTheDocument();
  });

  it("falls back to the default caption when no tail is set", () => {
    render(runPage(null, [activeItem], null));
    expect(screen.getByText(defaultCaption)).toBeInTheDocument();
  });
});
