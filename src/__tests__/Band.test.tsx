// src/__tests__/Band.test.tsx — BandMeter renders from the sound's own band layout
// (bandLabels), not a hardcoded six-band array. Covers the bass-vi 7-band case ("Sub"
// first) alongside the standard 6-band guitar/bass case, its accessible name, and the
// finding's bands drawn in the finding's tone.

import { describe, it, expect } from "vitest";
import { screen } from "@testing-library/react";

import { BandMeter } from "../views/doctor/BandMeter";
import { renderWithTheme } from "./testKit";

const GUITAR_LABELS = ["Lows", "Low mids", "Mids", "High mids", "Highs", "Air"];
const BASSVI_LABELS = ["Sub", ...GUITAR_LABELS];

describe("BandMeter", () => {
  it("renders 6 labels for a 6-band guitar/bass sound", () => {
    renderWithTheme(
      <BandMeter
        balanceDb={[-6, 4, -2, -8, -12, -18]}
        bandLabels={GUITAR_LABELS}
        bands={[1]}
        direction="strong"
        tone="danger"
      />,
    );
    for (const label of GUITAR_LABELS) {
      expect(screen.getByText(label)).toBeInTheDocument();
    }
  });

  it("names the finding's bands and which way they're off", () => {
    renderWithTheme(
      <BandMeter
        balanceDb={[-6, 4, -2, -8, -12, -18]}
        bandLabels={GUITAR_LABELS}
        bands={[1]}
        direction="strong"
        tone="danger"
      />,
    );
    expect(
      screen.getByRole("img", { name: "Tone by band: Low mids too strong" }),
    ).toBeInTheDocument();
  });

  it("says too weak, and lists several bands, for a finding that lacks them", () => {
    renderWithTheme(
      <BandMeter
        balanceDb={[-6, 4, -2, -8, -12, -18]}
        bandLabels={GUITAR_LABELS}
        bands={[3, 4, 5]}
        direction="weak"
        tone="caution"
      />,
    );
    expect(
      screen.getByRole("img", {
        name: "Tone by band: High mids, Highs and Air too weak",
      }),
    ).toBeInTheDocument();
  });

  it("renders 7 labels for a bass-vi sound, with Sub first", () => {
    renderWithTheme(
      <BandMeter
        balanceDb={[-10, -6, 4, -2, -8, -12, -18]}
        bandLabels={BASSVI_LABELS}
        bands={[0]}
        direction="strong"
        tone="danger"
      />,
    );
    const rendered = screen.getAllByText(
      /^(Sub|Lows|Low mids|Mids|High mids|Highs|Air)$/,
    );
    expect(rendered).toHaveLength(7);
    expect(rendered[0]).toHaveTextContent("Sub");
  });
});

describe("BandMeter — colours", () => {
  it("draws the finding's bands in its tone and the others in the meter grey", () => {
    renderWithTheme(
      <BandMeter
        balanceDb={[-6, 4, -2, -8, -12, -18]}
        bandLabels={GUITAR_LABELS}
        bands={[1]}
        direction="strong"
        tone="danger"
      />,
    );
    const bar = (label: string) =>
      document.querySelector<HTMLElement>(`[data-band-bar="${label}"]`);
    expect(bar("Low mids")?.style.background).toBe("rgb(169, 50, 38)");
    expect(bar("Lows")?.style.background).toBe("rgb(161, 161, 166)");
  });
});
