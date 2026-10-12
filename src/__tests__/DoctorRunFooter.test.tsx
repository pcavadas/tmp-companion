// src/__tests__/DoctorRunFooter.test.tsx — the Check run screen's stop/done footer
// contract. Regression-guards the two bugs the run-parity fix addresses: (1) a
// stopped run must show "Check stopped" (not get stuck on "Stopping…" — the
// bug a naive verbatim copy of Leveling's stopping flag would have shipped,
// since Doctor is one backend command with no per-item publish loop to clear
// it automatically); (2) a natural finish must NOT render a clickable CTA
// that duplicates the imminent auto-advance; (3) a check whose command REJECTS
// must read as a failure with a retry — never as "Check stopped" — and no row
// may keep its "listening…" spinner (#290).

import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement } from "react";

import { ThemeProvider } from "../theme/ThemeProvider";
import { DoctorRun } from "../views/doctor/DoctorRun";
import type { DoctorInputArg } from "../lib/types";

const item: DoctorInputArg = {
  key: "k0",
  listIndex: 0,
  scene: null,
  footswitch: null,
  label: "Reverse Delay",
  tag: null,
  topologyId: null,
  calibrationLufs: null,
  profileId: null,
  nodes: [],
  footswitches: [],
  sceneOverrides: [],
};

function doctorRun(overrides: Partial<Parameters<typeof DoctorRun>[0]> = {}) {
  const el: ReactElement = (
    <ThemeProvider>
      <DoctorRun
        status="running"
        items={[item]}
        statusByKey={{ k0: "active" }}
        error={null}
        instName={() => null}
        onStop={vi.fn()}
        onComplete={vi.fn()}
        onRetry={vi.fn()}
        onBack={vi.fn()}
        {...overrides}
      />
    </ThemeProvider>
  );
  return el;
}

describe("DoctorRun footer/header — stop + done states", () => {
  it("stopping: shows immediate acknowledgment, no Stop button", () => {
    render(doctorRun({ status: "stopping" }));
    expect(screen.getByText("Stopping…")).toBeInTheDocument();
    expect(screen.getByText("finishing current item…")).toBeInTheDocument();
    expect(screen.queryByText("Stop")).not.toBeInTheDocument();
  });

  it("stopped (terminal): header reads 'Check stopped', not stuck on 'Stopping…'", () => {
    render(
      doctorRun({
        status: "stopped",
        statusByKey: { k0: "done" },
      }),
    );
    expect(screen.getByText("Check stopped")).toBeInTheDocument();
    expect(screen.queryByText("Stopping…")).not.toBeInTheDocument();
    expect(screen.getByText("See results")).toBeInTheDocument();
  });

  it("natural finish: no clickable CTA competing with the auto-advance", () => {
    render(
      doctorRun({
        status: "done",
        statusByKey: { k0: "done" },
      }),
    );
    expect(screen.getByText("Check complete")).toBeInTheDocument();
    // "done" appears twice on a natural finish: the header's progress label
    // AND the static footer marker (the thing under test) — assert both,
    // rather than a single ambiguous getByText.
    expect(screen.getAllByText("done")).toHaveLength(2);
    expect(screen.queryByText("See results")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /see results/i }),
    ).not.toBeInTheDocument();
  });

  it("failed: says the check failed (not 'Check stopped'), shows why, and offers Try again", async () => {
    const onRetry = vi.fn();
    const onBack = vi.fn();
    render(
      doctorRun({
        status: "failed",
        // The stream left the row active when the command rejected.
        statusByKey: { k0: "active" },
        error: "the unit stopped answering",
        onRetry,
        onBack,
      }),
    );
    expect(screen.getByText("The check couldn't finish")).toBeInTheDocument();
    expect(screen.queryByText("Check stopped")).not.toBeInTheDocument();
    expect(screen.queryByText("See results")).not.toBeInTheDocument();
    expect(screen.getByText(/the unit stopped answering/)).toBeInTheDocument();
    // No row keeps spinning once the run has ended.
    expect(screen.queryByText("listening…")).not.toBeInTheDocument();
    expect(screen.getByText("didn't finish")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /try again/i }));
    expect(onRetry).toHaveBeenCalledTimes(1);
    await userEvent.click(
      screen.getByRole("button", { name: /back to presets/i }),
    );
    expect(onBack).toHaveBeenCalledTimes(1);
  });
});
