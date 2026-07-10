// Smoke test for the DS slot-number cell (src/ui/SlotLabel.tsx).

import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { ThemeProvider } from "../theme/ThemeProvider";
import { light } from "../theme/tokens";
import { slotLabel } from "../lib/format";
import { SlotLabel } from "../ui/SlotLabel";
import type { ReactNode } from "react";

function under(node: ReactNode) {
  return render(<ThemeProvider>{node}</ThemeProvider>);
}

describe("SlotLabel — the DS mono slot-number cell", () => {
  it("renders slotLabel(index) text", () => {
    under(<SlotLabel index={0} />);
    expect(screen.getByText(slotLabel(0))).toBeTruthy();
    expect(slotLabel(0)).toBe("001");
  });

  it("defaults to mutedInk; faint switches to the faint token", () => {
    under(<SlotLabel index={4} />);
    expect(screen.getByText(slotLabel(4)).style.color).toBe(
      toRgb(light.mutedInk),
    );

    under(<SlotLabel index={9} faint />);
    expect(screen.getByText(slotLabel(9)).style.color).toBe(toRgb(light.faint));
  });
});

function toRgb(hex: string): string {
  const n = parseInt(hex.slice(1), 16);
  const r = (n >> 16) & 255;
  const g = (n >> 8) & 255;
  const b = n & 255;
  return `rgb(${String(r)}, ${String(g)}, ${String(b)})`;
}
