// Locks the untested-firmware page (a caution PageState): title + the below-floor version
// and the tested floor in the body, "Use it anyway" wired to its handler, and no "Check
// again" (#305: while connected it could only re-serve the cached version, so it could
// change nothing) — the caption says the page re-checks when the unit restarts.

import { describe, it, expect, vi } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { FirmwareGate } from "../views/FirmwareGate";
import { renderWithTheme } from "./testKit";

function renderGate(onProceed = vi.fn()) {
  renderWithTheme(<FirmwareGate detected="1.6.3" onProceed={onProceed} />);
  return { onProceed };
}

describe("FirmwareGate", () => {
  it("shows the title and the detected version against the tested floor", () => {
    renderGate();
    expect(
      screen.getByRole("heading", { name: "This firmware hasn't been tested" }),
    ).toBeTruthy();
    expect(
      screen.getByText(
        "Your Tone Master Pro runs firmware 1.6.3. TMP Companion is tested on 1.7 and later.",
      ),
    ).toBeTruthy();
    expect(screen.queryByText("Untested firmware")).toBeNull(); // no kicker: the toolbar says it
  });

  it("offers no 'Check again' that can't change anything, and says how it re-checks", () => {
    renderGate();
    expect(screen.queryByText("Check again")).toBeNull();
    expect(
      screen.getByText(
        "Updated the firmware? This page checks again when your Tone Master Pro restarts.",
      ),
    ).toBeTruthy();
  });

  it("'Use it anyway' calls onProceed", async () => {
    const { onProceed } = renderGate();
    await userEvent.click(screen.getByText("Use it anyway"));
    expect(onProceed).toHaveBeenCalledTimes(1);
  });
});
