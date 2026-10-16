// UpdateOverlay — each update phase in look B's words: a bottom-centre Toast with at
// most ONE link action and a close whose name says what closing means ("Later" for an
// update that can wait, "Dismiss" for a failed one), the review dialog with "You have
// <current>." and "Later" / "Update now", and a downloading toast with no close. Every
// action still maps to the same UpdaterApi call as before.

import { describe, it, expect, vi } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import type { UpdatePhase, UpdaterApi } from "../lib/useUpdater";
import { UpdateOverlay } from "../ui/UpdateOverlay";
import { renderWithTheme } from "./testKit";

function makeUpdater(
  phase: UpdatePhase,
  over: Partial<UpdaterApi> = {},
): UpdaterApi {
  return {
    phase,
    version: "2.1.0",
    notes: null,
    percent: 40,
    autoInstall: false,
    currentVersion: "2.0.3",
    hasUpdateChannel: true,
    setAutoInstall: vi.fn(),
    check: vi.fn(() => Promise.resolve("none" as const)),
    review: vi.fn(),
    cancelReview: vi.fn(),
    startDownload: vi.fn(),
    dismiss: vi.fn(),
    retry: vi.fn(),
    restart: vi.fn(),
    ...over,
  };
}

describe("UpdateOverlay", () => {
  it("idle renders nothing", () => {
    const { container } = renderWithTheme(
      <UpdateOverlay u={makeUpdater("idle")} />,
    );
    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("available — 'See what's new' reviews, the close is 'Later'", async () => {
    const u = makeUpdater("available");
    renderWithTheme(<UpdateOverlay u={u} />);
    expect(
      screen.getByText("TMP Companion 2.1.0 is available."),
    ).toBeInTheDocument();
    expect(screen.queryByText(/update available/i)).toBeNull(); // no uppercase chip
    await userEvent.click(
      screen.getByRole("button", { name: "See what's new" }),
    );
    expect(u.review).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByRole("button", { name: "Later" }));
    expect(u.dismiss).toHaveBeenCalledTimes(1);
  });

  it("reviewing — the notes as a list, 'You have <current>.', Later / Update now", async () => {
    const u = makeUpdater("reviewing", { notes: "- Faster connect\n- Fixes" });
    renderWithTheme(<UpdateOverlay u={u} />);
    expect(
      screen.getByRole("dialog", { name: "TMP Companion 2.1.0" }),
    ).toBeInTheDocument();
    expect(screen.getByText("You have 2.0.3.")).toBeInTheDocument();
    expect(screen.getAllByRole("listitem").map((li) => li.textContent)).toEqual(
      ["Faster connect", "Fixes"],
    );
    await userEvent.click(screen.getByRole("button", { name: "Update now" }));
    expect(u.startDownload).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByRole("button", { name: "Later" }));
    expect(u.cancelReview).toHaveBeenCalledTimes(1);
  });

  it("reviewing with no notes says 'Bug fixes and improvements.'", () => {
    renderWithTheme(<UpdateOverlay u={makeUpdater("reviewing")} />);
    expect(screen.getByText("Bug fixes and improvements.")).toBeInTheDocument();
  });

  it("downloading — progress and 'You can keep working.', no close", () => {
    renderWithTheme(<UpdateOverlay u={makeUpdater("downloading")} />);
    expect(
      screen.getByText("Downloading TMP Companion 2.1.0…"),
    ).toBeInTheDocument();
    expect(screen.getByText("You can keep working.")).toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("ready — 'Restart now' restarts, the close is 'Later'", async () => {
    const u = makeUpdater("ready");
    renderWithTheme(<UpdateOverlay u={u} />);
    expect(
      screen.getByText("TMP Companion 2.1.0 is ready to install."),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Restart now" }));
    expect(u.restart).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByRole("button", { name: "Later" }));
    expect(u.dismiss).toHaveBeenCalledTimes(1);
  });

  it("error — says nothing was touched; 'Try again' retries, the close is 'Dismiss'", async () => {
    const u = makeUpdater("error");
    renderWithTheme(<UpdateOverlay u={u} />);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "The update didn't download.Your presets and settings aren't touched.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(u.retry).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(u.dismiss).toHaveBeenCalledTimes(1);
  });
});
