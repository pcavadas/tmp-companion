// InstrumentRow's readout after a recording (look B): the bands as words with dots
// (solid = played, hollow = not played; the words carry it to a screen reader, "Lows
// played"), the sparse-take note, and NO LU figure. Session-only (CalibrateResult isn't
// persisted), so it renders right after a successful calibrate_profile call. Mirrors
// InstrumentRow.test.tsx's setup pattern.

import { describe, it, expect, vi } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { InstrumentRow } from "../views/settings/InstrumentRow";
import type { Profile } from "../lib/types";
import { makeCalibrateResult, mockCommands } from "./commandMocks";
import { renderWithTheme } from "./testKit";

const profile: Profile = {
  id: "tele",
  name: "Telecaster",
  topology_id: "sc",
  calibration_lufs: null,
};

// The backend's band labels (doctor.rs LABELS_6), shown as they come.
const BANDS = ["Lows", "Low mids", "Mids", "High mids", "Highs", "Air"];
const SPARSE =
  "Some ranges weren’t played, so Doctor sees less of your tone. Record again with chords and lead.";

async function recordWith(coverage: boolean[]) {
  mockCommands({
    calibrate_profile: () =>
      makeCalibrateResult({
        lufs: -18.2,
        spread_lu: 4.7,
        band_coverage: coverage,
        band_labels: BANDS,
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
    />,
  );
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "Record" }));
  await screen.findByText("Low mids", undefined, { timeout: 8000 });
}

describe("InstrumentRow — the take readout", () => {
  it("names every band in words, says which were played, and drops the LU figure", async () => {
    await recordWith([true, true, false, false, true, true]);

    // Every band is present (played and not), in on-screen spelling.
    for (const name of [
      "Lows",
      "Low mids",
      "Mids",
      "High mids",
      "Highs",
      "Air",
    ])
      expect(screen.getByText(name)).toBeInTheDocument();
    // The words carry played / not played (the dots are aria-hidden).
    expect(screen.getByText("Lows").textContent).toBe("Lows played");
    expect(screen.getByText("Mids").textContent).toBe("Mids not played");
    // No spread / LU figure (Voice: no LU on screen).
    expect(screen.queryByText(/spread|LU\b/)).toBeNull();

    // The sparse-take note fires because Mids and High mids weren't played.
    expect(screen.getByText(SPARSE)).toBeInTheDocument();
  });

  it("no sparse-take note when every band was played", async () => {
    await recordWith([true, true, true, true, true, true]);
    expect(screen.queryByText(SPARSE)).toBeNull();
  });

  it("no sparse-take note when ONLY Air is unplayed (a passive DI never covers 6–12 kHz)", async () => {
    await recordWith([true, true, true, true, true, false]);
    // Air still shows, as not played (honest data) …
    expect(screen.getByText("Air").textContent).toBe("Air not played");
    // … but the sparse-take note does NOT fire for Air alone.
    expect(screen.queryByText(SPARSE)).toBeNull();
  });
});
