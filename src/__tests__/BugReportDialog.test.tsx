// The "Report a bug" dialog — opened from the native Help menu and Settings — the
// manual bug-report bundle export + the in-app "send report" flow (look B).
//
//   • Renders the title + explainer + "Save to Downloads" and "Send report".
//   • Disconnected (empty library scan) → the preset Select is disabled on "No preset"
//     with the reason under it.
//   • "Save to Downloads" calls save_support_bundle (firmware/presetJson/presetName all
//     null with no preset picked) and says where the report went — never a file path,
//     never backend text.
//   • "Send report": disabled with its reason until something is described; success
//     shows the report number; a send failure (non-200, network error, or the
//     placeholder empty endpoint) automatically falls back to the local save.
//   • The header close button fires onClose.
//
// Real timers (RTL's waitFor/findBy hang under vitest fake timers here).

import { describe, it, expect, vi, beforeEach } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { BugReportDialog } from "../views/BugReportDialog";
import { mockCommands } from "./commandMocks";
import { renderWithTheme } from "./testKit";

// The real endpoint is a placeholder empty string pre-deploy; stub a non-empty
// one here so the success/failure send tests actually reach `fetch`.
vi.mock("../lib/reportEndpoint", () => ({
  REPORT_ENDPOINT: "https://report.example",
  REPORT_TOKEN: "test-token",
}));

function renderDialog(connected = false, onClose = vi.fn()) {
  renderWithTheme(
    <BugReportDialog connected={connected} firmware={null} onClose={onClose} />,
  );
  return onClose;
}

describe("BugReportDialog", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("renders the title, explainer, and the disconnected preset Select", () => {
    renderDialog(false);
    expect(
      screen.getByRole("dialog", { name: "Report a bug" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Describe what happened. The report adds recent logs, your Tone Master Pro's settings and the app version.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Save to Downloads" }),
    ).toBeInTheDocument();
    // No library scan (disconnected) → the Select shows what will be used, and why
    // it can't change.
    expect(
      screen.getByRole("button", { name: "Include a preset: No preset" }),
    ).toBeDisabled();
    expect(
      screen.getByText("Connect your Tone Master Pro to include a preset."),
    ).toBeInTheDocument();
  });

  it("Send report stays disabled, with its reason, until something is described", async () => {
    const user = userEvent.setup();
    renderDialog(false);
    expect(screen.getByRole("button", { name: "Send report" })).toBeDisabled();
    expect(
      screen.getByText("Describe what happened first."),
    ).toBeInTheDocument();
    await user.type(screen.getByLabelText("What happened?"), "It froze.");
    expect(screen.getByRole("button", { name: "Send report" })).toBeEnabled();
    expect(screen.queryByText("Describe what happened first.")).toBeNull();
  });

  it("fires onClose from the header close button", async () => {
    const user = userEvent.setup();
    const onClose = renderDialog(false);
    await user.click(screen.getByRole("button", { name: /close/i }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("saves a bundle and says it is in Downloads, without the path", async () => {
    const user = userEvent.setup();
    const path = "/Users/x/Downloads/tmp-companion-report-20260719-120000.tar";
    mockCommands({ save_support_bundle: () => ({ path }) });
    renderDialog(false);

    await user.click(screen.getByRole("button", { name: "Save to Downloads" }));

    expect(vi.mocked(invoke)).toHaveBeenCalledWith("save_support_bundle", {
      firmware: null,
      presetJson: null,
      presetName: null,
    });
    expect(
      await screen.findByText("Report saved in your Downloads folder."),
    ).toBeInTheDocument();
    expect(screen.queryByText(new RegExp(path))).toBeNull();
  });

  it("says the save failed in plain words, never the backend text", async () => {
    const user = userEvent.setup();
    mockCommands({
      save_support_bundle: () => {
        throw new Error("disk full");
      },
    });
    renderDialog(false);

    await user.click(screen.getByRole("button", { name: "Save to Downloads" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Couldn't save the report. Try again.",
    );
    expect(screen.queryByText(/disk full/i)).toBeNull();
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
    // send() calls build_support_bundle only (meta is prop-fed).
    mockCommands({ build_support_bundle: () => new ArrayBuffer(8) });
    renderDialog(false);

    await user.type(
      screen.getByLabelText("What happened?"),
      "The level meter froze.",
    );
    await user.click(screen.getByRole("button", { name: /send report/i }));

    expect(
      await screen.findByText(
        "Report 42 sent. Keep this number in case you need to ask about it.",
      ),
    ).toBeInTheDocument();
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
    // send(): build_support_bundle → (fetch throws) → the local-save fallback's
    // save_support_bundle.
    mockCommands({
      build_support_bundle: () => new ArrayBuffer(8),
      save_support_bundle: () => ({ path }),
    });
    renderDialog(false);

    await user.type(
      screen.getByLabelText("What happened?"),
      "The level meter froze.",
    );
    await user.click(screen.getByRole("button", { name: /send report/i }));

    expect(
      await screen.findByText(
        "Couldn't send the report, so it's saved in your Downloads folder. You can send it yourself.",
      ),
    ).toBeInTheDocument();
    expect(vi.mocked(invoke)).toHaveBeenCalledWith(
      "save_support_bundle",
      expect.anything(),
    );
    expect(screen.queryByText(new RegExp(path))).toBeNull();
  });

  it("a later save clears a prior send's report-ID message", async () => {
    const user = userEvent.setup();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ reportId: 42 }),
      }),
    );
    const path = "/Users/x/Downloads/tmp-companion-report-20260719-120000.tar";
    mockCommands({
      build_support_bundle: () => new ArrayBuffer(8), // send()
      save_support_bundle: () => ({ path }), // save()
    });
    renderDialog(false);

    await user.type(
      screen.getByLabelText("What happened?"),
      "The level meter froze.",
    );
    await user.click(screen.getByRole("button", { name: /send report/i }));
    expect(await screen.findByText(/Report 42 sent/i)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Save to Downloads" }));
    expect(
      await screen.findByText("Report saved in your Downloads folder."),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Report 42 sent/i)).not.toBeInTheDocument();
  });
});
