// Locks the untested-firmware notice: kicker + title + the below-floor version
// inline, "Use it anyway" wired to its handler, and no "Check again" (#305: while
// connected it could only re-serve the cached version, so it could change nothing).

import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { ThemeProvider } from "../theme/ThemeProvider";
import { FirmwareGate } from "../views/FirmwareGate";

function renderGate(onProceed = vi.fn()) {
  render(
    <ThemeProvider>
      <FirmwareGate detected="1.6.3" onProceed={onProceed} />
    </ThemeProvider>,
  );
  return { onProceed };
}

describe("FirmwareGate", () => {
  it("shows the untested kicker, title, and the detected version", () => {
    renderGate();
    expect(screen.getByText("Untested firmware")).toBeTruthy();
    expect(screen.getByText("This firmware hasn’t been tested")).toBeTruthy();
    expect(screen.getByText("1.6.3")).toBeTruthy();
  });

  it("offers no 'Check again' that can't change anything, and says how it re-checks", () => {
    renderGate();
    expect(screen.queryByText("Check again")).toBeNull();
    expect(screen.getByText(/checks again on its own/i)).toBeTruthy();
  });

  it("'Use it anyway' calls onProceed", async () => {
    const { onProceed } = renderGate();
    await userEvent.click(screen.getByText("Use it anyway"));
    expect(onProceed).toHaveBeenCalledTimes(1);
  });
});
