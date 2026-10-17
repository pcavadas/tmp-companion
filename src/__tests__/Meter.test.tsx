// src/__tests__/Meter.test.tsx — the look B CPU meter (ui/Meter) and the two places it
// is applied (level/RowCpu, copy/CpuMeter).
//
// Two guarantees beyond the look:
//   1. The bar is BUDGET-relative: fill = value / CPU_BUDGET (a preset at 29.0% of the
//      audio core draws at 38%), clamped to the bar.
//   2. "Over" uses the SAME rule as Copy's validation and the device (`overCpuBudget`:
//      the total rounded half away from zero must not exceed the budget). A bare
//      `value > 76.5` would call exactly 76.5 fine while the save gate refuses it.
// Plus the old static-bar guarantees: the fill clamps, and nothing animates (the meter
// paints instantly — it is deliberately not ProgressBar).

import { describe, it, expect } from "vitest";
import { screen } from "@testing-library/react";

import { Meter } from "../ui/Meter";
import { RowCpu } from "../views/level/RowCpu";
import { CpuMeter } from "../views/copy/CpuMeter";
import { CPU_BUDGET, overCpuBudget } from "../models/cpu";
import { light } from "../theme/tokens";
import { renderWithTheme, toRgb } from "./testKit";

function bar(container: HTMLElement): HTMLElement {
  const el = container.querySelector<HTMLElement>('[aria-hidden="true"]');
  if (!el) throw new Error("no bar");
  return el;
}
function fill(container: HTMLElement): HTMLElement {
  return bar(container).firstElementChild as HTMLElement;
}
function fillPct(container: HTMLElement): number {
  return parseFloat(fill(container).style.width);
}

describe("Meter — the look B CPU meter", () => {
  it("is a 40×4 bar with the figure beside it in tabular numbers", () => {
    const { container } = renderWithTheme(<Meter value={29} />);
    expect(bar(container).style.width).toBe("40px");
    expect(bar(container).style.height).toBe("4px");
    const figure = screen.getByText("29.0%");
    expect(figure.style.fontVariantNumeric).toBe("tabular-nums");
    expect(figure.style.color).toBe(toRgb(light.textSecondary));
    expect(fill(container).style.background).toBe(toRgb(light.meter));
  });

  it("fills against the budget, not against 100 (29.0% draws at 38%)", () => {
    expect(CPU_BUDGET).toBe(76.5);
    const { container } = renderWithTheme(<Meter value={29} />);
    expect(Math.round(fillPct(container))).toBe(38);
    const { container: full } = renderWithTheme(<Meter value={CPU_BUDGET} />);
    expect(fillPct(full)).toBe(100);
  });

  it("clamps the fill to 0–100", () => {
    const { container: high } = renderWithTheme(<Meter value={150} />);
    const { container: low } = renderWithTheme(<Meter value={-10} />);
    expect(fillPct(high)).toBe(100);
    expect(fillPct(low)).toBe(0);
  });

  it("has no transition (paints instantly)", () => {
    const { container } = renderWithTheme(<Meter value={50} />);
    expect(bar(container).style.transition).toBe("");
    expect(fill(container).style.transition).toBe("");
  });

  it("well under the budget: grey, no Over", () => {
    renderWithTheme(<Meter value={61.2} />);
    expect(screen.queryByText("Over")).toBeNull();
  });

  it("over the budget: the fill and figure turn danger and the word Over follows", () => {
    const { container } = renderWithTheme(<Meter value={81.3} />);
    expect(screen.getByText("Over")).toBeTruthy();
    expect(screen.getByText("81.3%").style.color).toBe(toRgb(light.danger));
    expect(fill(container).style.background).toBe(toRgb(light.danger));
  });

  it("exactly 76.5 is Over — the device's rounding rule, not a bare > 76.5", () => {
    expect(overCpuBudget(76.5)).toBe(true);
    renderWithTheme(<Meter value={76.5} />);
    expect(screen.getByText("76.5%")).toBeTruthy();
    expect(screen.getByText("Over")).toBeTruthy();
  });

  it("just under the rounding edge is not Over (76.4 rounds to 76)", () => {
    expect(overCpuBudget(76.4)).toBe(false);
    renderWithTheme(<Meter value={76.4} />);
    expect(screen.queryByText("Over")).toBeNull();
  });

  it("agrees with overCpuBudget everywhere around the budget", () => {
    for (const v of [75.9, 76.0, 76.4, 76.45, 76.49, 76.5, 76.6, 77, 80]) {
      const { container, unmount } = renderWithTheme(<Meter value={v} />);
      expect(container.textContent.includes("Over"), String(v)).toBe(
        overCpuBudget(v),
      );
      unmount();
    }
  });
});

describe("the CPU meter where the rule is applied", () => {
  it("Level's RowCpu says Over at exactly 76.5", () => {
    renderWithTheme(<RowCpu value={76.5} />);
    expect(screen.getByText("Over")).toBeTruthy();
    expect(screen.getByText("76.5%")).toBeTruthy();
    // No hover title: the figure says it (look B).
    expect(document.querySelector("[title]")).toBeNull();
  });

  it("Level's RowCpu stays quiet under the budget", () => {
    renderWithTheme(<RowCpu value={29} />);
    expect(screen.queryByText("Over")).toBeNull();
    expect(screen.getByText("29.0%")).toBeTruthy();
  });

  it("Copy's CpuMeter says Over at exactly 76.5, matching its save gate", () => {
    renderWithTheme(<CpuMeter value={76.5} />);
    expect(screen.getByText("Over")).toBeTruthy();
  });

  it("Copy's CpuMeter shows the figure as a percentage, no budget suffix", () => {
    renderWithTheme(<CpuMeter value={42} />);
    expect(screen.getByText("42.0%")).toBeTruthy();
    expect(screen.queryByText(/over budget/i)).toBeNull();
  });
});
