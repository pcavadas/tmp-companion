// src/__tests__/LoudnessMeter.test.tsx — the run screen's one live meter (look B):
// where the sound is against its target, with no dB or LUFS on screen.

import { describe, it, expect } from "vitest";
import { screen } from "@testing-library/react";

import { LoudnessMeter } from "../ui/LoudnessMeter";
import { renderWithTheme } from "./testKit";

function meter(): HTMLElement {
  return screen.getByRole("meter", {
    name: "Loudness of Crunch now, against its target",
  });
}

describe("LoudnessMeter", () => {
  it("is a named meter: the fill is the loudness now on the bar's range", () => {
    // Range −30 … −10: −21 is 45 % of the way.
    renderWithTheme(
      <LoudnessMeter
        lufs={-21}
        target={-18}
        targetName="Crunch"
        soundName="Crunch"
        min={-30}
        max={-10}
      />,
    );
    expect(meter().getAttribute("aria-valuenow")).toBe("45");
    expect(meter().getAttribute("aria-valuemin")).toBe("0");
    expect(meter().getAttribute("aria-valuemax")).toBe("100");
    const fill = meter().children[1] as HTMLElement;
    expect(fill.style.width).toBe("45%");
  });

  it("puts the target's tick and its name under it, with Quieter and Louder at the ends", () => {
    renderWithTheme(
      <LoudnessMeter
        lufs={-21}
        target={-18}
        targetName="Crunch"
        soundName="Crunch"
        min={-30}
        max={-10}
      />,
    );
    const tick = meter().children[2] as HTMLElement;
    expect(tick.style.left).toBe("calc(60% - 1px)");
    expect(tick.style.width).toBe("2px");
    expect(screen.getByText("Quieter")).toBeTruthy();
    expect(screen.getByText("Louder")).toBeTruthy();
    expect(screen.getAllByText("Crunch").length).toBeGreaterThan(0);
  });

  it("shows no numbers: no dB, no LUFS anywhere on screen", () => {
    const { container } = renderWithTheme(
      <LoudnessMeter
        lufs={-21.4}
        target={-18}
        targetName="Crunch"
        soundName="Crunch"
      />,
    );
    expect(container.textContent).not.toMatch(/\d/);
    expect(container.textContent).not.toMatch(/LUFS|dB/);
  });

  it("says where the sound is against the target in words", () => {
    const { rerender } = renderWithTheme(
      <LoudnessMeter
        lufs={-25}
        target={-18}
        targetName="Crunch"
        soundName="Crunch"
      />,
    );
    expect(meter().getAttribute("aria-valuetext")).toBe("Quieter than Crunch");
    rerender(
      <LoudnessMeter
        lufs={-18.4}
        target={-18}
        targetName="Crunch"
        soundName="Crunch"
      />,
    );
    expect(meter().getAttribute("aria-valuetext")).toBe("Close to Crunch");
    rerender(
      <LoudnessMeter
        lufs={-12}
        target={-18}
        targetName="Crunch"
        soundName="Crunch"
      />,
    );
    expect(meter().getAttribute("aria-valuetext")).toBe("Louder than Crunch");
  });

  it("before the first reading the fill is empty", () => {
    renderWithTheme(
      <LoudnessMeter
        lufs={null}
        target={-18}
        targetName="Crunch"
        soundName="Crunch"
      />,
    );
    expect(meter().getAttribute("aria-valuenow")).toBe("0");
    expect(meter().getAttribute("aria-valuetext")).toBe("Not heard yet");
  });

  it("clamps a reading outside the range to the bar", () => {
    renderWithTheme(
      <LoudnessMeter
        lufs={-60}
        target={-18}
        targetName="Crunch"
        soundName="Crunch"
        min={-32}
        max={-10}
      />,
    );
    expect(meter().getAttribute("aria-valuenow")).toBe("0");
  });
});
