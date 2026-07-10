// src/__tests__/RunPieces.test.tsx — the run-wizard DS extractions shared by the
// Doctor "Check" run and the Leveling run: ConfirmBar (the inline stop prompt),
// RunRow (one progress row), and useAutoAdvance (the natural-finish → summary jump).

import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import type { ReactNode } from "react";

import { ThemeProvider } from "../theme/ThemeProvider";
import { ConfirmBar } from "../ui/ConfirmBar";
import { RunRow } from "../ui/RunRow";
import { useAutoAdvance } from "../lib/useAutoAdvance";

function under(node: ReactNode) {
  return render(<ThemeProvider>{node}</ThemeProvider>);
}

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

describe("ConfirmBar", () => {
  it("renders the message and both default labels (Continue / Stop)", () => {
    under(
      <ConfirmBar
        message="Stop the check? Checked sounds keep their results."
        onCancel={vi.fn()}
        onConfirm={vi.fn()}
      />,
    );
    expect(
      screen.getByText("Stop the check? Checked sounds keep their results."),
    ).toBeTruthy();
    expect(screen.getByText("Continue")).toBeTruthy();
    expect(screen.getByText("Stop")).toBeTruthy();
  });

  it("honors custom labels", () => {
    under(
      <ConfirmBar
        message="m"
        onCancel={vi.fn()}
        onConfirm={vi.fn()}
        cancelLabel="Keep going"
        confirmLabel="Halt"
      />,
    );
    expect(screen.getByText("Keep going")).toBeTruthy();
    expect(screen.getByText("Halt")).toBeTruthy();
  });

  it("wires the cancel button to onCancel and the confirm button to onConfirm", () => {
    const onCancel = vi.fn();
    const onConfirm = vi.fn();
    under(<ConfirmBar message="m" onCancel={onCancel} onConfirm={onConfirm} />);

    fireEvent.click(screen.getByText("Continue"));
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();

    fireEvent.click(screen.getByText("Stop"));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });
});

describe("RunRow", () => {
  it("renders name, tag, instrument chip and status content", () => {
    under(
      <RunRow
        name="Reverse Delay"
        tag="FS1"
        instrument="Telecaster"
        statusWidth={96}
        icon={<span>ic</span>}
        status={<span>checked</span>}
      />,
    );
    expect(screen.getByText("Reverse Delay")).toBeTruthy();
    expect(screen.getByText("FS1")).toBeTruthy();
    expect(screen.getByText("Telecaster")).toBeTruthy();
    expect(screen.getByText("checked")).toBeTruthy();
    expect(screen.getByText("ic")).toBeTruthy();
  });

  it("omits the instrument chip and tag when not provided", () => {
    under(
      <RunRow
        name="Plain"
        statusWidth={150}
        icon={<span>ic</span>}
        status={<span>queued</span>}
      />,
    );
    expect(screen.queryByText("Telecaster")).toBeNull();
    expect(screen.getByText("Plain")).toBeTruthy();
  });

  it("applies statusWidth to the status cell", () => {
    under(
      <RunRow
        name="n"
        statusWidth={150}
        icon={<span>ic</span>}
        status={<span>done · −18.0</span>}
      />,
    );
    // The status content sits in RunRow's fixed-width wrapper span.
    const cell = screen.getByText("done · −18.0").parentElement;
    expect(cell?.style.width).toBe("150px");
  });

  it("colors the tag with tagColor", () => {
    under(
      <RunRow
        name="n"
        tag="BASE"
        tagColor="#123456"
        statusWidth={96}
        icon={<span>ic</span>}
        status={<span>s</span>}
      />,
    );
    // #123456 → rgb(18, 52, 86).
    expect(screen.getByText("BASE").style.color).toBe("rgb(18, 52, 86)");
  });

  it("renders the expanded drawer (children) under the row", () => {
    under(
      <RunRow
        name="n"
        statusWidth={150}
        active
        icon={<span>ic</span>}
        status={<span>{""}</span>}
      >
        <div>live-vu-drawer</div>
      </RunRow>,
    );
    expect(screen.getByText("live-vu-drawer")).toBeTruthy();
  });
});

function Advancer(props: {
  done: boolean;
  stopped: boolean;
  onAdvance: () => void;
  delayMs?: number;
}) {
  useAutoAdvance(props.done, props.stopped, props.onAdvance, props.delayMs);
  return null;
}

describe("useAutoAdvance", () => {
  it("fires after a natural finish (done && !stopped)", async () => {
    const onAdvance = vi.fn();
    under(<Advancer done stopped={false} onAdvance={onAdvance} delayMs={20} />);
    expect(onAdvance).not.toHaveBeenCalled();
    await wait(60);
    expect(onAdvance).toHaveBeenCalledTimes(1);
  });

  it("does NOT fire after a manual stop (done && stopped)", async () => {
    const onAdvance = vi.fn();
    under(<Advancer done stopped onAdvance={onAdvance} delayMs={20} />);
    await wait(60);
    expect(onAdvance).not.toHaveBeenCalled();
  });

  it("does NOT fire while the run is still going (not done)", async () => {
    const onAdvance = vi.fn();
    under(
      <Advancer
        done={false}
        stopped={false}
        onAdvance={onAdvance}
        delayMs={20}
      />,
    );
    await wait(60);
    expect(onAdvance).not.toHaveBeenCalled();
  });
});
