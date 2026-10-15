// src/__tests__/InstrumentRow.test.tsx — a Settings instrument row's calibration flow.
// REAL timers throughout: the countdown is 3 × 850 ms.
//
// - #301: every calibration attempt has its own token. Cancel while recording, start
//   again, THEN let the cancelled capture resolve: the new countdown must keep running
//   into its own recording, and the stale result must not apply (no second
//   onCalibrated, no "calibrated" line, no idle flip).
// - BUG-6: unmounting mid-recording can't setState / fire onCalibrated after unmount.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { InstrumentRow } from "../views/settings/InstrumentRow";
import { stimulusSource, warningsOf } from "../views/settings/useCalibration";
import type { CalibrateResult, Profile } from "../lib/types";
import { fired, makeCalibrateResult, mockCommands } from "./commandMocks";
import { renderWithTheme } from "./testKit";

const profile: Profile = {
  id: "tele",
  name: "Telecaster",
  topology_id: "sc",
  calibration_lufs: null,
};

function renderRow(p: Profile, onCalibrated: () => void) {
  return renderWithTheme(
    <InstrumentRow
      profile={p}
      topology={null}
      connected={true}
      onCalibrated={onCalibrated}
      onEdit={vi.fn()}
      onDelete={vi.fn()}
      onMove={vi.fn()}
    />,
  );
}

const RESULT: CalibrateResult = {
  lufs: -20,
  clip: "clean",
  capture_stored: true,
  kept_previous_capture: false,
  stimulus_shortfall_lu: null,
  spread_lu: 0.4,
  band_coverage: [],
  band_labels: [],
};

describe("InstrumentRow — a cancelled capture can't hijack the next attempt (#301)", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it(
    "cancel while recording → calibrate again → the first capture resolves → the new attempt continues",
    { timeout: 15000 },
    async () => {
      const resolvers: ((r: CalibrateResult) => void)[] = [];
      mockCommands({
        calibrate_profile: () =>
          new Promise<CalibrateResult>((res) => {
            resolvers.push(res);
          }),
      });
      const onCalibrated = vi.fn();
      renderRow(profile, onCalibrated);
      const user = userEvent.setup();

      // Attempt 1 → recording (its capture is in flight).
      await user.click(screen.getByRole("button", { name: /calibrate/i }));
      await screen.findByText(/play steadily/i, undefined, { timeout: 6000 });
      expect(resolvers).toHaveLength(1);

      // Cancel: idle again; the cancel itself re-reads the store once.
      await user.click(screen.getByRole("button", { name: "Cancel" }));
      expect(onCalibrated).toHaveBeenCalledTimes(1);

      // Attempt 2 starts its countdown; now the CANCELLED capture resolves.
      await user.click(screen.getByRole("button", { name: /calibrate/i }));
      expect(screen.getByText(/get ready to play/i)).toBeInTheDocument();
      await act(async () => {
        resolvers[0](RESULT);
        await Promise.resolve();
      });

      // The stale result didn't apply: still counting down, no second callback…
      expect(screen.getByText(/get ready to play/i)).toBeInTheDocument();
      expect(screen.queryByText(/spread/i)).toBeNull();
      expect(onCalibrated).toHaveBeenCalledTimes(1);

      // …and the countdown wasn't killed: attempt 2 reaches its own recording.
      await screen.findByText(/play steadily/i, undefined, { timeout: 6000 });
      expect(resolvers).toHaveLength(2);

      // Attempt 2's own result DOES apply.
      await act(async () => {
        resolvers[1](RESULT);
        await Promise.resolve();
      });
      await vi.waitFor(() => {
        expect(onCalibrated).toHaveBeenCalledTimes(2);
      });
    },
  );
});

describe("regression: BUG-6 calibration abort on unmount", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
  });

  it(
    "a calibrate that resolves AFTER unmount never calls onCalibrated",
    { timeout: 10000 },
    async () => {
      let resolveCal: () => void = () => undefined;
      const gate = new Promise<void>((res) => {
        resolveCal = res;
      });
      mockCommands({
        calibrate_profile: () =>
          gate.then(() =>
            makeCalibrateResult({
              spread_lu: 0,
              band_coverage: [],
              band_labels: [],
            }),
          ),
      });
      const onCalibrated = vi.fn();
      const { unmount } = renderRow(profile, onCalibrated);
      const user = userEvent.setup();
      await user.click(screen.getByRole("button", { name: /calibrate/i }));
      // Wait out the 3×850ms countdown into the recording phase (calibrate_profile in flight).
      // "play steadily" is unique to the recording sub-line (vs. two "Recording" strings).
      await screen.findByText(/play steadily/i, undefined, { timeout: 6000 });
      expect(fired("calibrate_profile")).toBe(true);
      // Unmount mid-recording, THEN let the backend resolve — the abort gate must hold.
      unmount();
      await act(async () => {
        resolveCal();
        await gate;
      });
      expect(onCalibrated).not.toHaveBeenCalled();
    },
  );

  it(
    "a severe clip + stimulus-ceiling caveats surface as a non-fatal warning (#361)",
    { timeout: 10000 },
    async () => {
      mockCommands({
        calibrate_profile: () =>
          makeCalibrateResult({
            lufs: -12.4,
            clip: "severe",
            capture_stored: false,
            stimulus_shortfall_lu: 2.3,
            spread_lu: 3.5,
          }),
      });
      const onCalibrated = vi.fn();
      renderRow({ ...profile, calibration_lufs: -12.4 }, onCalibrated);
      const user = userEvent.setup();
      await user.click(screen.getByRole("button", { name: /re-calibrate/i }));
      // Both caveats land on the idle sub-line, joined "; " — success still fires.
      const line = await screen.findByText(
        /clipped the unit's USB 3 send/i,
        undefined,
        {
          timeout: 8000,
        },
      );
      expect(line.textContent).toMatch(/leveling uses the test signal/i);
      expect(line.textContent).toMatch(/leveling drives ~2\.3 LU softer/i);
      expect(onCalibrated).toHaveBeenCalled();
    },
  );
});

describe("InstrumentRow — clipped calibration takes (#361)", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("never says play softer and never points at the mixer meter", () => {
    for (const clip of ["clean", "mild", "severe"] as const)
      for (const kept of [false, true]) {
        const warn =
          warningsOf(
            makeCalibrateResult({
              clip,
              capture_stored: clip !== "severe",
              kept_previous_capture: clip === "severe" && kept,
            }),
          ) ?? "";
        expect(warn).not.toMatch(/softer|volume down|meter/i);
      }
  });

  it("a mild clip is stored without comment", () => {
    expect(
      warningsOf(makeCalibrateResult({ clip: "mild", capture_stored: true })),
    ).toBeNull();
  });

  it("a refused take over an earlier recording says the recording was kept", () => {
    expect(
      warningsOf(
        makeCalibrateResult({
          clip: "severe",
          capture_stored: false,
          kept_previous_capture: true,
        }),
      ),
    ).toMatch(/keeps your earlier recording/i);
  });

  it("the row always shows what leveling re-amps", () => {
    const { unmount } = renderRow(
      { ...profile, calibration_lufs: -14, has_capture: true },
      vi.fn(),
    );
    expect(screen.getByText(/levels with your recording/i)).toBeInTheDocument();
    unmount();
    renderRow(profile, vi.fn());
    expect(screen.getByText("not calibrated")).toBeInTheDocument();
    expect(screen.getByText("levels with the test signal")).toBeInTheDocument();
    expect(stimulusSource({ ...profile, calibration_lufs: -14 })).toBe(
      "levels with the test signal",
    );
  });
});
