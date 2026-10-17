// src/__tests__/InstrumentRow.test.tsx — a Settings instrument row (look B) and its
// recording (Tier-2 calibration) flow. REAL timers throughout: the countdown is
// 3 × 850 ms.
//
// - #301: every recording attempt has its own token. Cancel while recording, start
//   again, THEN let the cancelled capture resolve: the new countdown must keep running
//   into its own recording, and the stale result must not apply (no second
//   onCalibrated, no idle flip).
// - BUG-6: unmounting mid-recording can't setState / fire onCalibrated after unmount.
// - #361: a refused take's caveats, in words (no LU figure), and the row always says
//   what leveling uses.
// - Look B: the status Badge, the caption ("Bass VI", never "Bass VI · Bass VI"),
//   Record disabled while the unit is away, a failed take worded without backend text,
//   and the ⋯ menu.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { InstrumentRow } from "../views/settings/InstrumentRow";
import { stimulusSource, warningsOf } from "../views/settings/useCalibration";
import type { CalibrateResult, Profile, TopologyInfo } from "../lib/types";
import { fired, makeCalibrateResult, mockCommands } from "./commandMocks";
import { renderWithTheme } from "./testKit";

const profile: Profile = {
  id: "tele",
  name: "Telecaster",
  topology_id: "sc",
  calibration_lufs: null,
};

function renderRow(
  p: Profile,
  onCalibrated: () => void,
  {
    connected = true,
    topology = null,
  }: { connected?: boolean; topology?: TopologyInfo | null } = {},
) {
  return renderWithTheme(
    <InstrumentRow
      profile={p}
      topology={topology}
      connected={connected}
      onCalibrated={onCalibrated}
      onEdit={vi.fn()}
      onDelete={vi.fn()}
      onMoveDown={vi.fn()}
    />,
  );
}

const RECORDING = /^Recording \d+\.\d of 8 s$/;

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
      await user.click(screen.getByRole("button", { name: "Record" }));
      await screen.findByText(RECORDING, undefined, { timeout: 6000 });
      expect(resolvers).toHaveLength(1);

      // Cancel: idle again; the cancel itself re-reads the store once.
      await user.click(screen.getByRole("button", { name: "Cancel" }));
      expect(onCalibrated).toHaveBeenCalledTimes(1);

      // Attempt 2 starts its countdown; now the CANCELLED capture resolves.
      await user.click(screen.getByRole("button", { name: "Record" }));
      expect(screen.getByText(/^Start playing in/)).toBeInTheDocument();
      await act(async () => {
        resolvers[0](RESULT);
        await Promise.resolve();
      });

      // The stale result didn't apply: still counting down (no idle Record button),
      // no second callback…
      expect(screen.getByText(/^Start playing in/)).toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "Record" })).toBeNull();
      expect(onCalibrated).toHaveBeenCalledTimes(1);

      // …and the countdown wasn't killed: attempt 2 reaches its own recording.
      await screen.findByText(RECORDING, undefined, { timeout: 6000 });
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
      await user.click(screen.getByRole("button", { name: "Record" }));
      // Wait out the 3×850ms countdown into the recording phase (calibrate_profile in flight).
      await screen.findByText(RECORDING, undefined, { timeout: 6000 });
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
    "a severe clip + stimulus-ceiling caveats surface as two note lines, no LU (#361)",
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
      await user.click(screen.getByRole("button", { name: "Record again" }));
      // Each caveat is its own line — success still fires.
      expect(
        await screen.findByText(
          "Hard picks were too loud for the Tone Master Pro's USB recording, so this take wasn't kept. Leveling uses the test tone at your loudness.",
          undefined,
          { timeout: 8000 },
        ),
      ).toBeInTheDocument();
      expect(
        screen.getByText(
          "You play louder than the test tone can go, so your sounds are tested quieter than you play.",
        ),
      ).toBeInTheDocument();
      expect(screen.queryByText(/\bLU\b|2\.3/)).toBeNull();
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
        const warn = warningsOf(
          makeCalibrateResult({
            clip,
            capture_stored: clip !== "severe",
            kept_previous_capture: clip === "severe" && kept,
          }),
        ).join(" ");
        expect(warn).not.toMatch(/softer|volume down|meter/i);
      }
  });

  it("a mild clip is stored without comment", () => {
    expect(
      warningsOf(makeCalibrateResult({ clip: "mild", capture_stored: true })),
    ).toEqual([]);
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
    ).toEqual([
      "Hard picks were too loud for the Tone Master Pro's USB recording, so this take wasn't kept. Leveling still uses your earlier recording.",
    ]);
  });

  it("the row always shows what leveling re-amps", () => {
    const { unmount } = renderRow(
      { ...profile, calibration_lufs: -14, has_capture: true },
      vi.fn(),
    );
    expect(
      screen.getByText("Leveling uses your recording."),
    ).toBeInTheDocument();
    expect(screen.getByText("Recorded")).toBeInTheDocument();
    unmount();
    renderRow(profile, vi.fn());
    expect(screen.getByText("Not recorded")).toBeInTheDocument();
    expect(
      screen.getByText("Leveling uses the test tone."),
    ).toBeInTheDocument();
    expect(stimulusSource({ ...profile, calibration_lufs: -14 })).toBe(
      "Leveling uses the test tone.",
    );
  });
});

describe("InstrumentRow — look B", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("captions the type and pickup, and a Bass VI once", () => {
    const { unmount } = renderRow(profile, vi.fn(), {
      topology: {
        id: "guitar-humbucker",
        label: "Humbucker",
        instrument: "guitar",
      },
    });
    expect(screen.getByText("Guitar · Humbucker")).toBeInTheDocument();
    unmount();
    renderRow(profile, vi.fn(), {
      topology: { id: "bass-vi", label: "Bass VI", instrument: "bass-vi" },
    });
    expect(screen.getByText("Bass VI")).toBeInTheDocument();
    expect(screen.queryByText(/Bass VI · Bass VI/)).toBeNull();
  });

  it("shows Record disabled while the unit is away", () => {
    renderRow(profile, vi.fn(), { connected: false });
    expect(screen.getByRole("button", { name: "Record" })).toBeDisabled();
  });

  it(
    "counts down in words, then a failed take says what to check — never the backend's text",
    { timeout: 10000 },
    async () => {
      mockCommands({
        calibrate_profile: () => {
          throw new Error("capture stream died: -50");
        },
      });
      renderRow(profile, vi.fn());
      const user = userEvent.setup();
      await user.click(screen.getByRole("button", { name: "Record" }));
      expect(screen.getByText("Start playing in 3")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Cancel" })).toBeVisible();

      expect(
        await screen.findByText(
          "Couldn’t record it. Check that your instrument is plugged into the Tone Master Pro, then try again.",
          undefined,
          { timeout: 6000 },
        ),
      ).toBeInTheDocument();
      expect(screen.queryByText(/-50|stream died/)).toBeNull();
      // The badge is as it was before the attempt; "Try again" starts over.
      expect(screen.getByText("Not recorded")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Try again" })).toBeEnabled();
    },
  );

  it("hides the ⋯ while recording and offers Edit, the moves it can make, and Delete", async () => {
    mockCommands({
      calibrate_profile: () => new Promise<CalibrateResult>(() => undefined),
    });
    renderRow(profile, vi.fn());
    const user = userEvent.setup();

    await user.click(
      screen.getByRole("button", { name: "More for Telecaster" }),
    );
    const menu = screen.getByRole("menu");
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((el) => el.textContent),
    ).toEqual(["Edit instrument", "Move down", "Delete instrument"]);
    await user.click(
      screen.getByRole("button", { name: "More for Telecaster" }),
    );

    await user.click(screen.getByRole("button", { name: "Record" }));
    expect(
      screen.queryByRole("button", { name: "More for Telecaster" }),
    ).toBeNull();
  });
});
