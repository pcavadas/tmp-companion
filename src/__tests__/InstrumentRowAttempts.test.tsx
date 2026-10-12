// InstrumentRow — #301: every calibration attempt has its own token. Cancel while
// recording, start again, THEN let the cancelled capture resolve: the new countdown
// must keep running into its own recording, and the stale result must not apply
// (no second onCalibrated, no "calibrated" line, no idle flip). REAL timers: the
// countdown is 3 × 850 ms.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ThemeProvider } from "../theme/ThemeProvider";
import { InstrumentRow } from "../views/settings/InstrumentRow";
import type { CalibrateResult, Profile } from "../lib/types";

const profile: Profile = {
  id: "tele",
  name: "Telecaster",
  topology_id: "sc",
  calibration_lufs: null,
};

const RESULT: CalibrateResult = {
  lufs: -20,
  clipped: false,
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
      vi.mocked(invoke).mockImplementation((command: string) =>
        command === "calibrate_profile"
          ? new Promise<CalibrateResult>((res) => {
              resolvers.push(res);
            })
          : Promise.resolve(null),
      );
      const onCalibrated = vi.fn();
      render(
        <ThemeProvider>
          <InstrumentRow
            profile={profile}
            topology={null}
            connected={true}
            onCalibrated={onCalibrated}
            onEdit={vi.fn()}
            onDelete={vi.fn()}
            onMove={vi.fn()}
          />
        </ThemeProvider>,
      );
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
