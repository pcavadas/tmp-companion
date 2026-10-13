// src/__tests__/DialogEscape.test.tsx — the DS Dialog shell has no keyboard
// shortcuts (this is a click-only app): Escape must not close a dialog, and
// removing that listener must not touch the backdrop-click path it used to
// sit alongside.

import { describe, it, expect, vi } from "vitest";
import { screen, fireEvent } from "@testing-library/react";

import { Dialog } from "../ui/Dialog";
import { renderWithTheme } from "./testKit";

describe("Dialog — no Escape shortcut (click-only app)", () => {
  it("Escape does not call onClose", () => {
    const onClose = vi.fn();
    renderWithTheme(
      <Dialog onClose={onClose} label="Test dialog">
        <div>content</div>
      </Dialog>,
    );
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("a backdrop click still calls onClose", () => {
    const onClose = vi.fn();
    renderWithTheme(
      <Dialog onClose={onClose} label="Test dialog">
        <div>content</div>
      </Dialog>,
    );
    const dialog = screen.getByRole("dialog");
    const backdrop = dialog.previousElementSibling;
    if (!backdrop) throw new Error("backdrop element not found");
    fireEvent.click(backdrop);
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
