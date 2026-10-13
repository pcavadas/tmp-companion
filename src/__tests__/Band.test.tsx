// src/__tests__/Band.test.tsx — BandMeter/BandSpark render from the sound's own
// band layout (bandLabels/bandCount), not a hardcoded six-band array. Covers the
// bass-vi 7-band case ("Sub" first) alongside the standard 6-band guitar/bass case.

import { describe, it, expect } from "vitest";
import { screen } from "@testing-library/react";

import { BandMeter } from "../views/doctor/BandMeter";
import { BandSpark } from "../views/doctor/BandSpark";
import { renderWithTheme } from "./testKit";

const GUITAR_LABELS = ["Lows", "Low-mids", "Mids", "High-mids", "Highs", "Air"];
const BASSVI_LABELS = ["Sub", ...GUITAR_LABELS];

describe("BandMeter", () => {
  it("renders 6 labels for a 6-band guitar/bass sound", () => {
    renderWithTheme(
      <BandMeter
        balanceDb={[-6, 4, -2, -8, -12, -18]}
        bandLabels={GUITAR_LABELS}
        bands={[1]}
        sev="high"
      />,
    );
    for (const label of GUITAR_LABELS) {
      expect(screen.getByText(label)).toBeInTheDocument();
    }
  });

  it("renders 7 labels for a bass-vi sound, with Sub first", () => {
    renderWithTheme(
      <BandMeter
        balanceDb={[-10, -6, 4, -2, -8, -12, -18]}
        bandLabels={BASSVI_LABELS}
        bands={[0]}
        sev="high"
      />,
    );
    const rendered = screen.getAllByText(
      /^(Sub|Lows|Low-mids|Mids|High-mids|Highs|Air)$/,
    );
    expect(rendered).toHaveLength(7);
    expect(rendered[0]).toHaveTextContent("Sub");
  });
});

describe("BandSpark", () => {
  it("draws 6 bars for a 6-band sound", () => {
    renderWithTheme(
      <BandSpark
        balanceDb={[-6, 4, -2, -8, -12, -18]}
        bandCount={GUITAR_LABELS.length}
        hotBands={[1]}
        color="#d97757"
        muted={false}
      />,
    );
    const spark = screen.getByTitle("Band balance");
    expect(spark.children).toHaveLength(6);
  });

  it("draws 7 bars with no index gaps for a bass-vi sound", () => {
    renderWithTheme(
      <BandSpark
        balanceDb={[-10, -6, 4, -2, -8, -12, -18]}
        bandCount={BASSVI_LABELS.length}
        hotBands={[0, 6]}
        color="#d97757"
        muted={false}
      />,
    );
    const spark = screen.getByTitle("Band balance");
    expect(spark.children).toHaveLength(7);
  });
});
