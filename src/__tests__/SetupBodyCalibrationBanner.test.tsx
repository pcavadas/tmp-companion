// SetupBody's onboarding nudge toward Tier-2 calibration — a small dismissable
// banner shown only while the CHOSEN (apply-to-all) instrument is a real,
// uncalibrated profile. Mirrors InstrumentNudge.test.tsx's render pattern: real
// timers (repo-wide RTL fake-timer hang gotcha), SetupBody renders synchronously.

import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { ThemeProvider } from "../theme/ThemeProvider";
import { SetupBody } from "../views/overlays/SetupBody";
import type { SetupOption } from "../views/level/leveling";
import type { PickOption } from "../views/overlays/Pick";

const BANNER_TEXT = /Level with your own guitar/;

const baseOpt: SetupOption = {
  key: "p0",
  slot: 0,
  presetName: "Clean",
  isBase: true,
  sceneSlot: null,
  sceneName: "Whole preset",
  tag: null,
  hasScenes: false,
};

const instrumentOptions: PickOption[] = [
  { id: "none", label: "None" },
  { id: "tele", label: "Telecaster" }, // no calibration
  { id: "strat", label: "Strat", sub: "−9.5 dB", calibrated: true },
];

const targetOptions: PickOption[] = [{ id: "Rhythm", label: "Rhythm −18" }];

function renderSetup(defaultInst: string) {
  return render(
    <ThemeProvider>
      <SetupBody
        options={[baseOpt]}
        presetCount={1}
        isRelevel={false}
        instrumentOptions={instrumentOptions}
        targetOptions={targetOptions}
        defaultInst={defaultInst}
        defaultTarget="Rhythm"
        onCancel={vi.fn()}
        onStart={vi.fn()}
      />
    </ThemeProvider>,
  );
}

describe("SetupBody calibration onboarding banner", () => {
  it("uncalibrated profile chosen → shows the banner", () => {
    renderSetup("tele");
    expect(screen.getByText(BANNER_TEXT)).toBeTruthy();
  });

  it("calibrated profile chosen → no banner", () => {
    renderSetup("strat");
    expect(screen.queryByText(BANNER_TEXT)).toBeNull();
  });

  it("'None' instrument (no profile) → no banner", () => {
    renderSetup("none");
    expect(screen.queryByText(BANNER_TEXT)).toBeNull();
  });

  it("dismiss hides the banner", async () => {
    const user = userEvent.setup();
    renderSetup("tele");
    expect(screen.getByText(BANNER_TEXT)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByText(BANNER_TEXT)).toBeNull();
  });
});
