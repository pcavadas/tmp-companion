// src/__tests__/DoctorRun.test.tsx — the Check run screen's stop/done footer
// contract. Regression-guards the two bugs the run-parity fix addresses: (1) a
// stopped run must show "Check stopped" (not get stuck on "Stopping…" — the
// bug a naive verbatim copy of Leveling's stopping flag would have shipped,
// since Doctor is one backend command with no per-item publish loop to clear
// it automatically); (2) a natural finish must NOT render a clickable CTA
// that duplicates the imminent auto-advance.

import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
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
};

function doctorRun(overrides: Partial<Parameters<typeof DoctorRun>[0]> = {}) {
  const el: ReactElement = (
    <ThemeProvider>
      <DoctorRun
        items={[item]}
        statusByKey={{ k0: "active" }}
        currentIndex={0}
        total={1}
        done={false}
        stopped={false}
        stopping={false}
        instName={() => null}
        onStop={vi.fn()}
        onComplete={vi.fn()}
        {...overrides}
      />
    </ThemeProvider>
  );
  return el;
}

describe("DoctorRun footer/header — stop + done states", () => {
  it("stopping: shows immediate acknowledgment, no Stop button", () => {
    render(doctorRun({ stopping: true }));
    expect(screen.getByText("Stopping…")).toBeInTheDocument();
    expect(screen.getByText("finishing current item…")).toBeInTheDocument();
    expect(screen.queryByText("Stop")).not.toBeInTheDocument();
  });

  it("stopped (terminal): header reads 'Check stopped', not stuck on 'Stopping…'", () => {
    render(
      doctorRun({
        done: true,
        stopped: true,
        stopping: false,
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
        done: true,
        stopped: false,
        stopping: false,
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
});
