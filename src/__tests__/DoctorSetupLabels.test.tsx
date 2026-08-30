// src/__tests__/DoctorSetupLabels.test.tsx — BUG→GATE (issue 2), Doctor's Set-up step.
// Same mislabel as SetupBody's (see SetupBodyLabels.test.tsx): a hardcoded "Whole
// preset" for every base row shadowed `chosenFrom`'s own "Base Preset" answer for a
// multi-scene preset. The fix is `nameLabel = o.sceneName` here too.

import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";

import { ThemeProvider } from "../theme/ThemeProvider";
import { DoctorSetup } from "../views/doctor/DoctorSetup";
import type { SetupOption } from "../views/level/leveling";
import type { PickOption } from "../views/overlays/Pick";

const instrumentOptions: PickOption[] = [{ id: "none", label: "None" }];

function renderSetup(option: SetupOption) {
  return render(
    <ThemeProvider>
      <DoctorSetup
        options={[option]}
        presetCount={1}
        instrumentOptions={instrumentOptions}
        store={null}
        onBack={vi.fn()}
        onRun={vi.fn()}
      />
    </ThemeProvider>,
  );
}

describe("DoctorSetup — base row label (issue 2)", () => {
  it('a hasScenes base row renders "Base Preset"', () => {
    renderSetup({
      key: "p0",
      slot: 0,
      presetName: "Friedman HBE",
      isBase: true,
      sceneSlot: null,
      sceneName: "Base Preset",
      tag: "BASE",
      hasScenes: true,
    });
    expect(screen.getByText("Base Preset")).toBeInTheDocument();
    expect(screen.queryByText("Whole preset")).toBeNull();
  });

  it('a scene-less base row renders "Whole preset"', () => {
    renderSetup({
      key: "p0",
      slot: 0,
      presetName: "Studio Clean",
      isBase: true,
      sceneSlot: null,
      sceneName: "Whole preset",
      tag: null,
      hasScenes: false,
    });
    expect(screen.getByText("Whole preset")).toBeInTheDocument();
    expect(screen.queryByText("Base Preset")).toBeNull();
  });
});
