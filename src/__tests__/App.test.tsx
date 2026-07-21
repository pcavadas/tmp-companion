// App.tsx — the calibrate-nav wiring: the Level tab's "calibrate" cue (deep in
// the leveling Set-up step) jumps to Settings → Instruments, and a manual
// Settings-tab click clears that seed so a later ordinary visit lands back on
// the default category. LevelView/SettingsView are shallow-mocked (their own
// internals are covered by InstrumentNudge/SettingsView tests) so this test
// isolates the App-level tab + initialCategory plumbing without driving the
// whole device-connect → list → wizard chain.

import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { ThemeProvider } from "../theme/ThemeProvider";
import { DISCLAIMER_PERM_KEY } from "../lib/gates";
import type { LevelViewProps } from "../views/level/LevelView";
import type { SettingsViewProps } from "../views/settings/SettingsView";

vi.mock("../views/level", () => ({
  LevelView: ({ onCalibrate }: LevelViewProps) => (
    <button onClick={onCalibrate}>fire-calibrate</button>
  ),
}));

vi.mock("../views/settings", () => ({
  SettingsView: ({ initialCategory }: SettingsViewProps) => (
    <div data-testid="settings-pane">{initialCategory ?? "(default)"}</div>
  ),
}));

// Imported AFTER the mocks so App picks up the stand-ins above.
import App from "../App";

function renderApp() {
  localStorage.setItem(DISCLAIMER_PERM_KEY, "1"); // skip the startup disclaimer
  return render(
    <ThemeProvider>
      <App />
    </ThemeProvider>,
  );
}

describe("App — calibrate-nav wiring", () => {
  beforeEach(() => {
    localStorage.clear();
    sessionStorage.clear();
  });

  it("onCalibrate jumps to Settings on the Instruments category", async () => {
    const user = userEvent.setup();
    renderApp();

    await user.click(await screen.findByText("fire-calibrate"));

    expect(await screen.findByTestId("settings-pane")).toHaveTextContent(
      "instruments",
    );
  });

  it("a manual Settings tab click clears the seed (lands on the default category)", async () => {
    const user = userEvent.setup();
    renderApp();

    await user.click(await screen.findByText("fire-calibrate"));
    expect(await screen.findByTestId("settings-pane")).toHaveTextContent(
      "instruments",
    );

    // Away, then back to Settings via a plain tab click (not onCalibrate).
    await user.click(screen.getByRole("button", { name: "Level" }));
    await screen.findByText("fire-calibrate");
    await user.click(screen.getByRole("button", { name: "Settings" }));

    expect(await screen.findByTestId("settings-pane")).toHaveTextContent(
      "(default)",
    );
  });
});
