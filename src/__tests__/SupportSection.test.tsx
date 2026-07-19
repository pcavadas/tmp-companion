// Settings "Support" section — the manual bug-report bundle export.
//
//   • Renders the header + explainer + "Save support bundle" button.
//   • Disconnected (empty library scan) → the preset picker is a disabled hint.
//   • Clicking the button calls save_support_bundle (firmware/presetJson/presetName
//     all null with no preset picked) and, on success, surfaces the returned path.
//
// Real timers (RTL's waitFor/findBy hang under vitest fake timers here).

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ThemeProvider } from "../theme/ThemeProvider";
import { SupportSection } from "../views/settings/SupportSection";

function renderSection(connected = false) {
  render(
    <ThemeProvider>
      <SupportSection connected={connected} firmware={null} />
    </ThemeProvider>,
  );
}

describe("SupportSection", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("renders the header, explainer, and disconnected picker hint", () => {
    renderSection(false);
    expect(screen.getByText("Support")).toBeInTheDocument();
    expect(
      screen.getByText(/Bundles recent logs, device settings/i),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /save support bundle/i }),
    ).toBeInTheDocument();
    // No library scan (disconnected) → the picker is a disabled hint, not a menu.
    expect(
      screen.getByText(/connect to include a preset/i),
    ).toBeInTheDocument();
  });

  it("saves a bundle and shows the returned path", async () => {
    const user = userEvent.setup();
    const path = "/Users/x/Downloads/tmp-companion-report-20260719-120000.tar";
    vi.mocked(invoke).mockResolvedValueOnce({ path });
    renderSection(false);

    await user.click(
      screen.getByRole("button", { name: /save support bundle/i }),
    );

    expect(vi.mocked(invoke)).toHaveBeenCalledWith("save_support_bundle", {
      firmware: null,
      presetJson: null,
      presetName: null,
    });
    expect(await screen.findByText(path)).toBeInTheDocument();
    expect(screen.getByText("Support bundle saved")).toBeInTheDocument();
  });

  it("shows an inline error when the save fails", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockRejectedValueOnce(new Error("disk full"));
    renderSection(false);

    await user.click(
      screen.getByRole("button", { name: /save support bundle/i }),
    );

    expect(await screen.findByRole("alert")).toHaveTextContent(/disk full/i);
  });
});
