// App.tsx — the calibrate-nav wiring: the Level tab's "Record yours in Settings"
// link (on the leveling Set up page) jumps to Settings → Instruments, its "Add a target
// in Settings" link to Settings → Loudness targets, and a manual
// Settings-tab click clears that seed so a later ordinary visit lands back on
// the default category. Also: Settings' "Report a bug" opens the same dialog as
// the native Help menu. LevelView/SettingsView are shallow-mocked (their own
// internals are covered by SetupPage/SettingsView tests) so this test
// isolates the App-level tab + initialCategory plumbing without driving the
// whole device-connect → list → wizard chain.

import { describe, it, expect, beforeEach, vi } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { DISCLAIMER_PERM_KEY } from "../lib/gates";
import type { LevelViewProps } from "../views/level/LevelView";
import type { SettingsViewProps } from "../views/settings/SettingsView";

vi.mock("../views/level", () => ({
  LevelView: ({ onCalibrate, onAddTarget }: LevelViewProps) => (
    <>
      <button onClick={onCalibrate}>fire-calibrate</button>
      <button onClick={onAddTarget}>fire-add-target</button>
    </>
  ),
}));

vi.mock("../views/settings", () => ({
  SettingsView: ({ initialCategory, onReportBug }: SettingsViewProps) => (
    <div>
      <div data-testid="settings-pane">{initialCategory ?? "(default)"}</div>
      <button onClick={onReportBug}>settings-report-bug</button>
    </div>
  ),
}));

import App from "../App";
import { resetLevelFlow } from "../views/level/levelRunStore";
import { renderWithTheme } from "./testKit";

// The leveling wizard lives in a module store that outlives a mount; start each case clean.
beforeEach(() => {
  resetLevelFlow();
});

function renderApp() {
  localStorage.setItem(DISCLAIMER_PERM_KEY, "1"); // skip the startup disclaimer
  return renderWithTheme(<App />);
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

  it("onAddTarget jumps to Settings on the Loudness targets category", async () => {
    const user = userEvent.setup();
    renderApp();

    await user.click(await screen.findByText("fire-add-target"));

    expect(await screen.findByTestId("settings-pane")).toHaveTextContent(
      "targets",
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

describe("App — Report a bug from Settings", () => {
  beforeEach(() => {
    localStorage.clear();
    sessionStorage.clear();
  });

  it("Settings' Report a bug opens the Report a bug dialog", async () => {
    const user = userEvent.setup();
    renderApp();
    await user.click(screen.getByRole("button", { name: "Settings" }));
    expect(screen.queryByRole("dialog", { name: "Report a bug" })).toBeNull();
    await user.click(await screen.findByText("settings-report-bug"));
    expect(
      screen.getByRole("dialog", { name: "Report a bug" }),
    ).toBeInTheDocument();
  });
});
