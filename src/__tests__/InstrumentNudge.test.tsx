// InstrumentNudge — the "good → better → best" instrument caption in the Set up step.
// Covers the pure state derivation (instCalState) and the three rendered rungs:
//   none  → full ladder line   ·  uncal → calibrate line  ·  cal → nothing.
// Real timers (repo-wide RTL fake-timer hang gotcha); SetupBody renders synchronously.

import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";

import { ThemeProvider } from "../theme/ThemeProvider";
import { SetupBody } from "../views/overlays/SetupBody";
import { instCalState } from "../views/level/leveling";
import type { SetupOption } from "../views/level/leveling";
import type { PickOption } from "../views/overlays/Pick";

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

describe("instCalState", () => {
  it("treats missing / explicit-none as none", () => {
    expect(instCalState("", instrumentOptions)).toBe("none");
    expect(instCalState("none", instrumentOptions)).toBe("none");
  });
  it("treats an unknown id as none", () => {
    expect(instCalState("ghost", instrumentOptions)).toBe("none");
  });
  it("flags an instrument with no calibration as uncal", () => {
    expect(instCalState("tele", instrumentOptions)).toBe("uncal");
  });
  it("flags a calibrated instrument as cal", () => {
    expect(instCalState("strat", instrumentOptions)).toBe("cal");
  });
});

describe("InstrumentNudge rendering", () => {
  it("none → shows the full good→better→best ladder with a calibrate cue", () => {
    renderSetup("none");
    expect(
      screen.getByText(/Set an instrument for better results/),
    ).toBeTruthy();
    expect(screen.getByText("calibrate")).toBeTruthy();
  });

  it("uncal → shows only the calibrate line", () => {
    renderSetup("tele");
    expect(screen.getByText("Calibrate")).toBeTruthy();
    expect(
      screen.getByText(/this instrument for the best results/),
    ).toBeTruthy();
    expect(
      screen.queryByText(/Set an instrument for better results/),
    ).toBeNull();
  });

  it("cal → renders nothing (no caption, no leftover gap)", () => {
    renderSetup("strat");
    expect(screen.queryByText("calibrate")).toBeNull();
    expect(screen.queryByText("Calibrate")).toBeNull();
    expect(screen.queryByText(/for the best/)).toBeNull();
  });
});
