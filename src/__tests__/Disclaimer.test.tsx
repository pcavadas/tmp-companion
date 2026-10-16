// The first-run page (look B): the backup title, what saves to the unit, the liability
// line in its own words, and "I've backed up, continue" — permanent only when
// "Don't show this again" is ticked (box or words).

import { describe, it, expect, vi } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { Disclaimer } from "../views/Disclaimer";
import { renderWithTheme } from "./testKit";

describe("Disclaimer", () => {
  it("says what saves to the unit and keeps the liability line", () => {
    renderWithTheme(<Disclaimer onAccept={vi.fn()} />);
    expect(
      screen.getByRole("heading", {
        name: "Back up your Tone Master Pro first",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Leveling, Copy and Songs save changes/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/You use TMP Companion at your own risk\./),
    ).toBeInTheDocument();
    expect(screen.queryByText(/before you begin/i)).toBeNull(); // no kicker
  });

  it("continuing without the tick accepts for this session only", async () => {
    const onAccept = vi.fn();
    renderWithTheme(<Disclaimer onAccept={onAccept} />);
    await userEvent.click(
      screen.getByRole("button", { name: "I've backed up, continue" }),
    );
    expect(onAccept).toHaveBeenCalledWith(false);
  });

  it("ticking 'Don't show this again' (by its words) makes it permanent", async () => {
    const onAccept = vi.fn();
    renderWithTheme(<Disclaimer onAccept={onAccept} />);
    await userEvent.click(screen.getByText("Don't show this again"));
    expect(
      screen.getByRole("checkbox", { name: "Don't show this again" }),
    ).toBeChecked();
    await userEvent.click(
      screen.getByRole("button", { name: "I've backed up, continue" }),
    );
    expect(onAccept).toHaveBeenCalledWith(true);
  });
});
