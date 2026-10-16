// src/__tests__/DoctorSetupLabels.test.tsx — BUG→GATE (issue 2), Doctor's Set-up step.
// Same mislabel as Level's Set up (see SetupPage.labels.test.tsx): a hardcoded "Whole
// preset" for every base row shadowed `chosenFrom`'s own answer for a multi-scene
// preset. The fix is `nameLabel = o.sceneName` here too. `chosenFrom` now names every
// main sound "Main sound" (look B's one name); Doctor still renders it verbatim.

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

describe("DoctorSetup — main sound label (issue 2)", () => {
  it('renders the row\'s own name verbatim (`chosenFrom` names it "Main sound")', () => {
    renderSetup({
      key: "p0",
      slot: 0,
      presetName: "Friedman HBE",
      isBase: true,
      sceneSlot: null,
      sceneName: "Main sound",
      tag: "BASE",
      hasScenes: true,
    });
    expect(screen.getByText("Main sound")).toBeInTheDocument();
    expect(screen.queryByText("Base Preset")).toBeNull();
    expect(screen.queryByText("Whole preset")).toBeNull();
  });

  it("never re-derives it from whether the preset has scenes", () => {
    renderSetup({
      key: "p0",
      slot: 0,
      presetName: "Studio Clean",
      isBase: true,
      sceneSlot: null,
      sceneName: "Named by chosenFrom",
      tag: null,
      hasScenes: false,
    });
    expect(screen.getByText("Named by chosenFrom")).toBeInTheDocument();
  });
});
