// Settings "Support" section — the manual bug-report bundle export + the
// in-app "send report" flow.
//
//   • Renders the header + explainer + "Save support bundle" button.
//   • Disconnected (empty library scan) → the preset picker is a disabled hint.
//   • Clicking the button calls save_support_bundle (firmware/presetJson/presetName
//     all null with no preset picked) and, on success, surfaces the returned path.
//   • "Send a report": success shows the report ID; a send failure (non-200,
//     network error, or the placeholder empty endpoint) automatically falls
//     back to the local save and shows the saved path.
//
// Real timers (RTL's waitFor/findBy hang under vitest fake timers here).

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ThemeProvider } from "../theme/ThemeProvider";
import { SupportSection } from "../views/settings/SupportSection";

// The real endpoint is a placeholder empty string pre-deploy; stub a non-empty
// one here so the success/failure send tests actually reach `fetch`.
vi.mock("../lib/reportEndpoint", () => ({
  REPORT_ENDPOINT: "https://report.example",
  REPORT_TOKEN: "test-token",
}));

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

  it("sends a report and shows the returned report ID", async () => {
    const user = userEvent.setup();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ reportId: 42 }),
      }),
    );
    // Call order inside send(): build_support_bundle only (meta is prop-fed).
    vi.mocked(invoke).mockResolvedValueOnce(new ArrayBuffer(8));
    renderSection(false);

    await user.type(
      screen.getByPlaceholderText(/what happened/i),
      "The level meter froze.",
    );
    await user.click(screen.getByRole("button", { name: /send report/i }));

    expect(await screen.findByText(/Report #42 sent/i)).toBeInTheDocument();
    expect(vi.mocked(fetch)).toHaveBeenCalledWith(
      "https://report.example/report",
      expect.objectContaining({
        method: "POST",
        headers: { "x-report-token": "test-token" },
      }),
    );
  });

  it("falls back to a local save when sending fails", async () => {
    const user = userEvent.setup();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockRejectedValueOnce(new Error("network down")),
    );
    const path = "/Users/x/Downloads/tmp-companion-report-20260719-120000.tar";
    // Call order inside send(): build_support_bundle → (fetch throws) → the
    // local-save fallback's save_support_bundle.
    vi.mocked(invoke)
      .mockResolvedValueOnce(new ArrayBuffer(8))
      .mockResolvedValueOnce({ path });
    renderSection(false);

    await user.type(
      screen.getByPlaceholderText(/what happened/i),
      "The level meter froze.",
    );
    await user.click(screen.getByRole("button", { name: /send report/i }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      /couldn.t send/i,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(path);
  });
});
