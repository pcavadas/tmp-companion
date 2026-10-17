// src/__tests__/DoctorRunFooter.test.tsx — Doctor's Check page: its title, now line, rows
// and footer in every run state. Regression-guards the run-parity bugs: (1) a stopped run
// says it is stopping and then goes on to Results by itself (no extra "See results"
// click); (2) a natural finish shows no clickable action competing with the
// auto-advance; (3) a check whose command REJECTS reads as a failure with Back and Try
// again — never as a Stop — shows no backend text, and no row keeps its "Listening" mark
// (#290). Stop asks first, in a dialog only its own buttons close.

import { describe, it, expect, vi, afterEach } from "vitest";
import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement } from "react";

import { DoctorRun, type DoctorRunProps } from "../views/doctor/DoctorRun";
import { DOCTOR_STEPS } from "../views/doctor/useDoctorFlow";
import type { DoctorInputArg } from "../lib/types";
import { renderWithTheme } from "./testKit";

const item = (key: string): DoctorInputArg => ({
  key,
  listIndex: 0,
  scene: null,
  footswitch: null,
  label: key,
  soundName: key,
  tag: null,
  topologyId: null,
  calibrationLufs: null,
  profileId: null,
  nodes: [],
  footswitches: [],
  sceneOverrides: [],
});

const NAMES: Record<string, { preset: string; sound: string }> = {
  k0: { preset: "Reverse Delay", sound: "Main sound" },
  k1: { preset: "Reverse Delay", sound: "Chorus" },
};

function doctorRun(overrides: Partial<DoctorRunProps> = {}) {
  const el: ReactElement = (
    <DoctorRun
      status="running"
      items={[item("k0"), item("k1")]}
      statusByKey={{ k0: "active", k1: "queued" }}
      nameOf={(k) => NAMES[k] ?? { preset: "", sound: k }}
      steps={DOCTOR_STEPS}
      onStop={vi.fn()}
      onComplete={vi.fn()}
      onRetry={vi.fn()}
      onBack={vi.fn()}
      {...overrides}
    />
  );
  return el;
}

const rowOf = (key: string) => {
  const row = document.querySelector<HTMLElement>(`[data-check-row="${key}"]`);
  if (!row) throw new Error(`no row ${key}`);
  return within(row);
};

afterEach(() => {
  vi.useRealTimers();
});

describe("DoctorRun — running", () => {
  it("titles the check by its sounds and names the sound it listens to", () => {
    renderWithTheme(doctorRun());
    expect(
      screen.getByRole("heading", { name: "Checking 2 sounds" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Listening to Reverse Delay. Nothing changes on your Tone Master Pro.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/^About \d+ (seconds|minutes?) left$/),
    ).toBeInTheDocument();
    expect(rowOf("k0").getByText("Listening")).toBeInTheDocument();
    expect(rowOf("k1").getByText("Waiting")).toBeInTheDocument();
    // The main sound's row says "Main sound" — never "Base" or "Whole preset".
    expect(rowOf("k0").getByText("Main sound")).toBeInTheDocument();
    const steps = screen.getByRole("list", { name: "Steps" });
    expect(within(steps).getByText("Check")).toHaveAttribute(
      "aria-current",
      "step",
    );
  });

  it("names a scene in its preset", () => {
    renderWithTheme(doctorRun({ statusByKey: { k0: "done", k1: "active" } }));
    expect(
      screen.getByText(
        "Listening to Chorus in Reverse Delay. Nothing changes on your Tone Master Pro.",
      ),
    ).toBeInTheDocument();
    expect(rowOf("k0").getByText("Checked")).toBeInTheDocument();
  });

  it("Stop asks first, and only its own buttons close the question", async () => {
    const onStop = vi.fn();
    renderWithTheme(doctorRun({ onStop }));
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Stop" }));
    const dialog = screen.getByRole("dialog", { name: "Stop checking?" });
    expect(
      within(dialog).getByText("Sounds checked so far keep their results."),
    ).toBeInTheDocument();
    await user.click(
      within(dialog).getByRole("button", { name: "Keep checking" }),
    );
    expect(onStop).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Stop" }));
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: "Stop" }),
    );
    expect(onStop).toHaveBeenCalledOnce();
  });
});

describe("DoctorRun — stop and finish", () => {
  it("stopping: says so at once, and Stop turns busy", () => {
    renderWithTheme(doctorRun({ status: "stopping" }));
    expect(
      screen.getByRole("heading", { name: "Stopping…" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Finishing Reverse Delay first."),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Stop/ })).toBeDisabled();
  });

  it("stopped: goes on to Results by itself, no extra click", () => {
    vi.useFakeTimers();
    const onComplete = vi.fn();
    renderWithTheme(
      doctorRun({
        status: "stopped",
        statusByKey: { k0: "done", k1: "queued" },
        onComplete,
      }),
    );
    expect(screen.queryByRole("button", { name: /results/i })).toBeNull();
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(onComplete).toHaveBeenCalledOnce();
  });

  it("natural finish: says every sound is checked, offers no competing action, then advances", () => {
    vi.useFakeTimers();
    const onComplete = vi.fn();
    renderWithTheme(
      doctorRun({
        status: "done",
        statusByKey: { k0: "done", k1: "done" },
        onComplete,
      }),
    );
    expect(screen.getByText("All 2 sounds checked.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /results/i })).toBeNull();
    expect(screen.getByRole("button", { name: /Stop/ })).toBeDisabled();
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(onComplete).toHaveBeenCalledOnce();
  });

  it("failed: says the check couldn't finish (not stopped), shows no backend text, and offers Back and Try again (#290)", async () => {
    const onRetry = vi.fn();
    const onBack = vi.fn();
    const onComplete = vi.fn();
    renderWithTheme(
      doctorRun({
        status: "failed",
        statusByKey: { k0: "active", k1: "queued" },
        onRetry,
        onBack,
        onComplete,
      }),
    );
    expect(
      screen.getByRole("heading", { name: "The check couldn't finish" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Nothing changed on your Tone Master Pro. Try again, or go back to your presets.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText("Stopping…")).toBeNull();
    // The row the command left in flight is not still listening.
    expect(rowOf("k0").queryByText("Listening")).toBeNull();
    expect(rowOf("k0").getByText("Not checked")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(onRetry).toHaveBeenCalledOnce();
    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(onBack).toHaveBeenCalledOnce();
    expect(onComplete).not.toHaveBeenCalled();
  });

  it("a sound that couldn't be checked says so on its row", () => {
    renderWithTheme(doctorRun({ statusByKey: { k0: "error", k1: "active" } }));
    expect(rowOf("k0").getByText("Couldn't check")).toBeInTheDocument();
  });

  it("skipped Instrument page: Check is the first of two steps", () => {
    renderWithTheme(doctorRun({ steps: ["Check", "Results"] }));
    const steps = screen.getByRole("list", { name: "Steps" });
    expect(steps).not.toHaveTextContent("Instrument");
    expect(within(steps).getByText("Check")).toHaveAttribute(
      "aria-current",
      "step",
    );
  });
});

describe("DoctorRun — wide window", () => {
  const original = window.innerWidth;
  afterEach(() => {
    act(() => {
      Object.defineProperty(window, "innerWidth", {
        configurable: true,
        value: original,
      });
      window.dispatchEvent(new Event("resize"));
    });
  });

  // The narrow column holds the title and steps as well as the list, so the whole
  // page stays centred together. Narrowing only the list left it hugging the
  // title's left edge on a wide window.
  it("narrows the whole centred column, not the list alone", () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1440,
    });
    renderWithTheme(doctorRun());
    const heading = screen.getByRole("heading", { name: /Checking 2 sounds/ });
    const column = heading.closest<HTMLElement>('[style*="margin: 0px auto"]');
    expect(column?.style.maxWidth).toBe("768px");
    const row = document.querySelector<HTMLElement>('[data-check-row="k0"]');
    expect(row && column?.contains(row)).toBe(true);
    // Nothing between the list and that column narrows the list on its own.
    let el = row?.parentElement ?? null;
    while (el && el !== column) {
      expect(el.style.maxWidth).toBe("");
      el = el.parentElement;
    }
  });
});
