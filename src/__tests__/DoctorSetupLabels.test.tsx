// src/__tests__/DoctorSetupLabels.test.tsx — BUG→GATE (issue 2), Doctor's Set-up step.
// Same mislabel as SetupPage's (see SetupPage.labels.test.tsx): a hardcoded "Whole
// preset" for every base row shadowed `chosenFrom`'s own "Base Preset" answer for a
// multi-scene preset. The fix is `nameLabel = o.sceneName` here too.

import { describe, it, expect, vi } from "vitest";
import { screen } from "@testing-library/react";

import { DoctorSetup } from "../views/doctor/DoctorSetup";
import type { SetupOption } from "../views/level/leveling";
import type { PickOption } from "../views/overlays/Pick";
import { renderWithTheme } from "./testKit";

const instrumentOptions: PickOption[] = [{ id: "none", label: "None" }];

function renderSetup(option: SetupOption) {
  return renderWithTheme(
    <DoctorSetup
      options={[option]}
      presetCount={1}
      instrumentOptions={instrumentOptions}
      store={null}
      onBack={vi.fn()}
      onRun={vi.fn()}
    />,
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
