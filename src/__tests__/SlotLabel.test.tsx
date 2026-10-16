// Smoke test for the DS slot-number cell (src/ui/SlotLabel.tsx), look B.

import { describe, it, expect } from "vitest";
import { screen } from "@testing-library/react";
import { light } from "../theme/tokens";
import { slotLabel } from "../lib/format";
import { SlotLabel } from "../ui/SlotLabel";
import { renderWithTheme, toRgb } from "./testKit";

describe("SlotLabel — the DS slot-number cell (Inter, tabular)", () => {
  it("renders slotLabel(index) text", () => {
    renderWithTheme(<SlotLabel index={0} />);
    expect(screen.getByText(slotLabel(0))).toBeTruthy();
    expect(slotLabel(0)).toBe("001");
  });

  it("defaults to textSecondary; faint switches to textDisabled", () => {
    renderWithTheme(<SlotLabel index={4} />);
    expect(screen.getByText(slotLabel(4)).style.color).toBe(
      toRgb(light.textSecondary),
    );

    renderWithTheme(<SlotLabel index={9} faint />);
    expect(screen.getByText(slotLabel(9)).style.color).toBe(
      toRgb(light.textDisabled),
    );
  });

  it("uses tabular numbers in the sans font, not a monospace font", () => {
    renderWithTheme(<SlotLabel index={2} />);
    const el = screen.getByText(slotLabel(2));
    expect(el.style.fontVariantNumeric).toBe("tabular-nums");
    expect(el.style.fontFamily).toContain("Inter");
  });
});
