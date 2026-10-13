// InstrumentRow's post-calibration "what was captured" readout: spread + a
// per-band coverage dot row + a sparse-take hint. Session-only (CalibrateResult
// isn't persisted), so it renders right after a successful calibrate_profile call.
// Mirrors InstrumentRow.test.tsx's setup pattern.

import { describe, it, expect, vi } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { InstrumentRow } from "../views/settings/InstrumentRow";
import type { Profile } from "../lib/types";
import { mockCommands } from "./commandMocks";
import { renderWithTheme } from "./testKit";

const profile: Profile = {
  id: "tele",
  name: "Telecaster",
  topology_id: "sc",
  calibration_lufs: null,
};

describe("InstrumentRow — capture quality readout", () => {
  it("renders spread + covered/uncovered band dots + the sparse-take hint", async () => {
    mockCommands({
      calibrate_profile: () => ({
        lufs: -18.2,
        clipped: false,
        stimulus_shortfall_lu: null,
        spread_lu: 4.7,
        band_coverage: [true, true, false, false, true, true],
        band_labels: ["Lo", "LoM", "Mid", "HiM", "Hi", "Air"],
      }),
    });
    renderWithTheme(
      <InstrumentRow
        profile={profile}
        topology={null}
        connected={true}
        onCalibrated={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onMove={vi.fn()}
      />,
    );
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /calibrate/i }));
    await screen.findByText(/spread 4\.7 LU/i, undefined, { timeout: 8000 });

    // Every band label is present (both covered and uncovered) — exact match to
    // avoid "Lo" ambiguously matching the "LoM" dot too.
    expect(screen.getByText("● Lo")).toBeInTheDocument();
    expect(screen.getByText("● Mid")).toBeInTheDocument();
    expect(screen.getByText("● Air")).toBeInTheDocument();

    // Sparse-take hint fires because Mid/HiM weren't covered.
    expect(screen.getByText(/some bands weren.t played/i)).toBeInTheDocument();
  });

  it("no sparse-take hint when every band was covered", async () => {
    mockCommands({
      calibrate_profile: () => ({
        lufs: -18.2,
        clipped: false,
        stimulus_shortfall_lu: null,
        spread_lu: 2.1,
        band_coverage: [true, true, true, true, true, true],
        band_labels: ["Lo", "LoM", "Mid", "HiM", "Hi", "Air"],
      }),
    });
    renderWithTheme(
      <InstrumentRow
        profile={profile}
        topology={null}
        connected={true}
        onCalibrated={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onMove={vi.fn()}
      />,
    );
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /calibrate/i }));
    await screen.findByText(/spread 2\.1 LU/i, undefined, { timeout: 8000 });
    expect(screen.queryByText(/some bands weren.t played/i)).toBeNull();
  });

  it("no sparse-take hint when ONLY Air is uncovered (a passive DI never covers 6–12 kHz)", async () => {
    mockCommands({
      calibrate_profile: () => ({
        lufs: -21.4,
        clipped: false,
        stimulus_shortfall_lu: null,
        spread_lu: 5.2,
        band_coverage: [true, true, true, true, true, false],
        band_labels: ["Lo", "LoM", "Mid", "HiM", "Hi", "Air"],
      }),
    });
    renderWithTheme(
      <InstrumentRow
        profile={profile}
        topology={null}
        connected={true}
        onCalibrated={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onMove={vi.fn()}
      />,
    );
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /calibrate/i }));
    await screen.findByText(/spread 5\.2 LU/i, undefined, { timeout: 8000 });
    // The Air dot still renders (dimmed, honest data) …
    expect(screen.getByText("● Air")).toBeInTheDocument();
    // … but the sparse-take warning does NOT fire for Air alone.
    expect(screen.queryByText(/some bands weren.t played/i)).toBeNull();
  });
});
