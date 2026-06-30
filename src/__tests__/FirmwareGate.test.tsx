// Locks the untested-firmware notice: kicker + title + the below-floor version
// inline, and the two actions wired to their handlers ("Check again" re-reads,
// "Use it anyway" proceeds).

import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { ThemeProvider } from "../theme/ThemeProvider";
import { FirmwareGate } from "../views/FirmwareGate";

function renderGate(onCheckAgain = vi.fn(), onProceed = vi.fn()) {
  render(
    <ThemeProvider>
      <FirmwareGate
        detected="1.6.3"
        onCheckAgain={onCheckAgain}
        onProceed={onProceed}
      />
    </ThemeProvider>,
  );
  return { onCheckAgain, onProceed };
}

describe("FirmwareGate", () => {
  it("shows the untested kicker, title, and the detected version", () => {
    renderGate();
    expect(screen.getByText("Untested firmware")).toBeTruthy();
    expect(screen.getByText("This firmware hasn’t been tested")).toBeTruthy();
    expect(screen.getByText("1.6.3")).toBeTruthy();
  });

  it("'Check again' calls onCheckAgain", async () => {
    const { onCheckAgain } = renderGate();
    await userEvent.click(screen.getByText("Check again"));
    expect(onCheckAgain).toHaveBeenCalledTimes(1);
  });

  it("'Use it anyway' calls onProceed", async () => {
    const { onProceed } = renderGate();
    await userEvent.click(screen.getByText("Use it anyway"));
    expect(onProceed).toHaveBeenCalledTimes(1);
  });
});
